//! Podman-native container command builder.
//!
//! Single abstraction for constructing `podman run`, `podman create`, and
//! `podman exec` commands with all rootless-podman flags applied by
//! construction. Replaces the old docker_command() arg-patching approach.

use crate::commands::start::step::SetupContext;
use crate::docker::command_logger::{format_command_strings, run_and_log_command_strings};
use crate::docker::core::run_command;
use std::error::Error;
use std::process::{Child, Command, Output, Stdio};
use std::sync::OnceLock;

#[derive(Clone, Debug)]
enum Subcommand {
    Run,
    Create,
    Exec(String), // container name
}

#[derive(Clone, Debug)]
struct VolumeMount {
    source: String,
    target: String,
    options: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ContainerRunBuilder {
    subcommand: Subcommand,
    name: Option<String>,
    image: Option<String>,
    networks: Vec<String>,
    ports: Vec<(u16, u16)>,
    volumes: Vec<VolumeMount>,
    env_vars: Vec<(String, String)>,
    workdir: Option<String>,
    command: Vec<String>,
    detach: bool,
    remove_after: bool,
    extra_args: Vec<String>,
    restart_policy: Option<String>,
}

// system paths that SELinux won't allow relabeling
fn is_system_path(path: &str) -> bool {
    matches!(path, "/tmp" | "/var" | "/etc" | "/proc" | "/sys")
}

// named volumes have no path separator -- they're just a name like "portainer_data"
fn is_named_volume(source: &str) -> bool {
    !source.starts_with('/') && !source.starts_with('.')
}

static SELINUX_POLICY_LOADED: OnceLock<bool> = OnceLock::new();

// check once per process whether the foc_devnet SELinux policy is installed
fn foc_devnet_policy_available() -> bool {
    *SELINUX_POLICY_LOADED.get_or_init(|| {
        Command::new("semodule")
            .args(["-l"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.split_whitespace().next() == Some("foc_devnet"))
            })
            .unwrap_or(false)
    })
}

fn volume_mount_string(vol: &VolumeMount) -> String {
    let mut s = format!("{}:{}", vol.source, vol.target);
    let mut opts = vol.options.clone();

    // auto-add :z for bind mounts (not named volumes, not system paths)
    if !is_named_volume(&vol.source) && !is_system_path(&vol.source) {
        let has_label = opts.iter().any(|o| {
            o == "z" || o == "Z" || o.starts_with("z,") || o.starts_with("Z,")
        });
        if !has_label {
            opts.push("z".to_string());
        }
    }

    if !opts.is_empty() {
        s.push(':');
        s.push_str(&opts.join(","));
    }
    s
}

impl ContainerRunBuilder {
    // -- constructors --

    pub fn run() -> Self {
        Self {
            subcommand: Subcommand::Run,
            name: None,
            image: None,
            networks: Vec::new(),
            ports: Vec::new(),
            volumes: Vec::new(),
            env_vars: Vec::new(),
            workdir: None,
            command: Vec::new(),
            detach: false,
            remove_after: false,
            extra_args: Vec::new(),
            restart_policy: None,
        }
    }

    pub fn create() -> Self {
        let mut b = Self::run();
        b.subcommand = Subcommand::Create;
        b
    }

    pub fn exec(container: &str) -> Self {
        Self {
            subcommand: Subcommand::Exec(container.to_string()),
            name: None,
            image: None,
            networks: Vec::new(),
            ports: Vec::new(),
            volumes: Vec::new(),
            env_vars: Vec::new(),
            workdir: None,
            command: Vec::new(),
            detach: false,
            remove_after: false,
            extra_args: Vec::new(),
            restart_policy: None,
        }
    }

    // -- presets --

    /// ephemeral builder container with host network and --rm
    pub fn builder_ephemeral(name: &str) -> Self {
        Self::run()
            .name(name)
            .network("host")
            .rm()
            .image(crate::constants::BUILDER_DOCKER_IMAGE)
    }

    /// long-running daemon container
    pub fn daemon(name: &str, network: &str) -> Self {
        Self::run()
            .name(name)
            .network(network)
            .detach()
    }

    // -- builder methods (all consume and return self for chaining) --

    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    pub fn image(mut self, image: &str) -> Self {
        self.image = Some(image.to_string());
        self
    }

    pub fn network(mut self, network: &str) -> Self {
        self.networks.push(network.to_string());
        self
    }

    pub fn port(mut self, host: u16, container: u16) -> Self {
        self.ports.push((host, container));
        self
    }

    pub fn volume(mut self, source: &str, target: &str) -> Self {
        self.volumes.push(VolumeMount {
            source: source.to_string(),
            target: target.to_string(),
            options: Vec::new(),
        });
        self
    }

    pub fn volume_ro(mut self, source: &str, target: &str) -> Self {
        self.volumes.push(VolumeMount {
            source: source.to_string(),
            target: target.to_string(),
            options: vec!["ro".to_string()],
        });
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env_vars.push((key.to_string(), value.to_string()));
        self
    }

    pub fn workdir(mut self, path: &str) -> Self {
        self.workdir = Some(path.to_string());
        self
    }

    pub fn detach(mut self) -> Self {
        self.detach = true;
        self
    }

    pub fn rm(mut self) -> Self {
        self.remove_after = true;
        self
    }

    pub fn restart(mut self, policy: &str) -> Self {
        self.restart_policy = Some(policy.to_string());
        self
    }

    pub fn arg(mut self, a: &str) -> Self {
        self.extra_args.push(a.to_string());
        self
    }

    pub fn cmd(mut self, args: &[&str]) -> Self {
        self.command = args.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn cmd_strings(mut self, args: Vec<String>) -> Self {
        self.command = args;
        self
    }

    // -- build the final args list --

    pub fn build(&self) -> Vec<String> {
        match &self.subcommand {
            Subcommand::Run => self.build_run_create("run"),
            Subcommand::Create => self.build_run_create("create"),
            Subcommand::Exec(container) => self.build_exec(container),
        }
    }

    fn build_run_create(&self, subcmd: &str) -> Vec<String> {
        let mut args: Vec<String> = vec![subcmd.to_string()];

        // podman rootless flags -- always applied
        args.push("--userns=keep-id".to_string());

        // use tailored SELinux policy if installed, otherwise disable labeling
        if foc_devnet_policy_available() {
            args.extend_from_slice(&[
                "--security-opt".to_string(),
                "label=type:foc_devnet.process".to_string(),
            ]);
        } else {
            args.extend_from_slice(&[
                "--security-opt".to_string(),
                "label=disable".to_string(),
            ]);
        }

        // HOME -- mapped user has no /etc/passwd entry
        args.extend_from_slice(&[
            "-e".to_string(),
            "HOME=/home/foc-user".to_string(),
        ]);

        // git safe.directory -- dubious ownership check workaround
        args.extend_from_slice(&[
            "-e".to_string(),
            "GIT_CONFIG_COUNT=1".to_string(),
            "-e".to_string(),
            "GIT_CONFIG_KEY_0=safe.directory".to_string(),
            "-e".to_string(),
            "GIT_CONFIG_VALUE_0=*".to_string(),
        ]);

        if self.detach {
            args.push("-d".to_string());
        }

        if self.remove_after {
            args.push("--rm".to_string());
        }

        if let Some(name) = &self.name {
            args.extend_from_slice(&["--name".to_string(), name.clone()]);
        }

        for net in &self.networks {
            args.extend_from_slice(&["--network".to_string(), net.clone()]);
        }

        for (host, container) in &self.ports {
            args.extend_from_slice(&["-p".to_string(), format!("{}:{}", host, container)]);
        }

        for vol in &self.volumes {
            args.extend_from_slice(&["-v".to_string(), volume_mount_string(vol)]);
        }

        for (k, v) in &self.env_vars {
            args.extend_from_slice(&["-e".to_string(), format!("{}={}", k, v)]);
        }

        if let Some(wd) = &self.workdir {
            // use /tmp as safe fallback -- podman fails if workdir doesn't exist
            args.extend_from_slice(&["-w".to_string(), wd.clone()]);
        }

        if let Some(policy) = &self.restart_policy {
            args.extend_from_slice(&["--restart".to_string(), policy.clone()]);
        }

        for a in &self.extra_args {
            args.push(a.clone());
        }

        if let Some(img) = &self.image {
            args.push(img.clone());
        }

        args.extend(self.command.iter().cloned());

        args
    }

    fn build_exec(&self, container: &str) -> Vec<String> {
        let mut args: Vec<String> = vec!["exec".to_string()];

        for (k, v) in &self.env_vars {
            args.extend_from_slice(&["-e".to_string(), format!("{}={}", k, v)]);
        }

        if let Some(wd) = &self.workdir {
            args.extend_from_slice(&["-w".to_string(), wd.clone()]);
        }

        for a in &self.extra_args {
            args.push(a.clone());
        }

        args.push(container.to_string());

        args.extend(self.command.iter().cloned());

        args
    }

    // -- execution methods --

    /// run and return output, no context logging
    pub fn run_raw(&self) -> Result<Output, Box<dyn Error>> {
        let args = self.build();
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_command("docker", &refs)
    }

    /// run with context logging for audit trail
    pub fn run_logged(
        &self,
        context: &SetupContext,
        key: &str,
    ) -> Result<Output, Box<dyn Error>> {
        let args = self.build();
        run_and_log_command_strings("docker", &args, context, key)
    }

    /// spawn child process with piped stdout/stderr for streamed output
    pub fn spawn(&self) -> Result<Child, Box<dyn Error>> {
        let args = self.build();
        Command::new("docker")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(Into::into)
    }

    /// spawn with null stdout/stderr (for background containers)
    pub fn spawn_quiet(&self) -> Result<Child, Box<dyn Error>> {
        let args = self.build();
        Command::new("docker")
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(Into::into)
    }

    /// format the command string for display/logging
    pub fn format(&self) -> String {
        let args = self.build();
        format_command_strings("docker", &args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_run() {
        let args = ContainerRunBuilder::run()
            .name("test")
            .image("alpine")
            .cmd(&["echo", "hello"])
            .build();

        assert_eq!(args[0], "run");
        assert!(args.contains(&"--userns=keep-id".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"test".to_string()));
        assert!(args.contains(&"alpine".to_string()));
        assert!(args.contains(&"echo".to_string()));
        assert!(args.contains(&"hello".to_string()));
    }

    #[test]
    fn test_volume_bind_mount_gets_z() {
        let args = ContainerRunBuilder::run()
            .image("alpine")
            .volume("/home/user/data", "/data")
            .build();

        let v_idx = args.iter().position(|a| a == "-v").unwrap();
        assert_eq!(args[v_idx + 1], "/home/user/data:/data:z");
    }

    #[test]
    fn test_volume_system_path_no_z() {
        let args = ContainerRunBuilder::run()
            .image("alpine")
            .volume("/tmp", "/workspace")
            .build();

        let v_idx = args.iter().position(|a| a == "-v").unwrap();
        assert_eq!(args[v_idx + 1], "/tmp:/workspace");
    }

    #[test]
    fn test_volume_named_no_z() {
        let args = ContainerRunBuilder::run()
            .image("alpine")
            .volume("my_volume", "/data")
            .build();

        let v_idx = args.iter().position(|a| a == "-v").unwrap();
        assert_eq!(args[v_idx + 1], "my_volume:/data");
    }

    #[test]
    fn test_volume_ro() {
        let args = ContainerRunBuilder::run()
            .image("alpine")
            .volume_ro("/home/user/data", "/data")
            .build();

        let v_idx = args.iter().position(|a| a == "-v").unwrap();
        assert_eq!(args[v_idx + 1], "/home/user/data:/data:ro,z");
    }

    #[test]
    fn test_exec_no_userns() {
        let args = ContainerRunBuilder::exec("mycontainer")
            .cmd(&["ls", "-la"])
            .build();

        assert_eq!(args[0], "exec");
        assert!(!args.contains(&"--userns=keep-id".to_string()));
        assert!(args.contains(&"mycontainer".to_string()));
        assert!(args.contains(&"ls".to_string()));
    }

    #[test]
    fn test_env_vars() {
        let args = ContainerRunBuilder::run()
            .image("alpine")
            .env("FOO", "bar")
            .build();

        // should have HOME, GIT_CONFIG vars, and FOO
        assert!(args.contains(&"FOO=bar".to_string()));
        assert!(args.contains(&"HOME=/home/foc-user".to_string()));
        assert!(args.contains(&"GIT_CONFIG_COUNT=1".to_string()));
    }

    #[test]
    fn test_builder_ephemeral_preset() {
        let args = ContainerRunBuilder::builder_ephemeral("test-container")
            .cmd(&["echo", "hi"])
            .build();

        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"host".to_string()));
        assert!(args.contains(&crate::constants::BUILDER_DOCKER_IMAGE.to_string()));
    }

    #[test]
    fn test_daemon_preset() {
        let args = ContainerRunBuilder::daemon("my-daemon", "my-net")
            .image("my-image")
            .build();

        assert!(args.contains(&"-d".to_string()));
        assert!(args.contains(&"my-net".to_string()));
    }

    #[test]
    fn test_create_subcommand() {
        let args = ContainerRunBuilder::create()
            .name("test")
            .image("alpine")
            .build();

        assert_eq!(args[0], "create");
        assert!(args.contains(&"--userns=keep-id".to_string()));
    }

    #[test]
    fn test_security_opt_present() {
        let args = ContainerRunBuilder::run()
            .image("alpine")
            .build();

        assert!(args.contains(&"--security-opt".to_string()));
        // either policy-based or disabled, depending on system state
        let sec_idx = args.iter().position(|a| a == "--security-opt").unwrap();
        let sec_val = &args[sec_idx + 1];
        assert!(
            sec_val == "label=type:foc_devnet.process" || sec_val == "label=disable",
            "unexpected --security-opt value: {}",
            sec_val
        );
    }

    #[test]
    fn test_exec_no_security_opt() {
        let args = ContainerRunBuilder::exec("mycontainer")
            .cmd(&["ls"])
            .build();

        assert!(!args.contains(&"--security-opt".to_string()));
    }
}
