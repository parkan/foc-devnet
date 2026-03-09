//! SELinux policy generation for foc-devnet containers using udica.
//!
//! creates a container from the actual foc-builder image with representative
//! mounts, inspects it, and pipes the JSON to udica to generate a CIL policy.
//! requires sudo for semodule -i.

use crate::constants::BUILDER_DOCKER_IMAGE;
use crate::paths::{
    foc_devnet_bin, foc_devnet_docker_volumes, foc_devnet_keys, foc_devnet_proof_parameters,
    foc_devnet_selinux, CONTAINER_FILECOIN_PROOF_PARAMS_PATH,
};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tracing::info;

pub const POLICY_MODULE: &str = "foc_devnet";

fn selinux_enforcing() -> bool {
    Command::new("getenforce")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "Enforcing")
        .unwrap_or(false)
}

/// check if the foc_devnet SELinux policy module is loaded
pub fn policy_loaded() -> bool {
    Command::new("semodule")
        .args(["-l"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.split_whitespace().next() == Some(POLICY_MODULE))
        })
        .unwrap_or(false)
}

/// generate and install an SELinux policy for foc-devnet containers.
///
/// creates a probe container from the real foc-builder image with the
/// same mount layout used at runtime, runs `podman inspect | udica` to
/// generate a CIL policy, and installs it via `sudo semodule`.
///
/// skipped (not an error) when SELinux is not enforcing or policy is
/// already loaded. errors out if any step in the pipeline fails.
pub fn setup_selinux_policy() -> Result<(), Box<dyn std::error::Error>> {
    if !selinux_enforcing() {
        info!("SELinux not enforcing, skipping policy generation");
        return Ok(());
    }

    if policy_loaded() {
        info!("foc_devnet SELinux policy already loaded");
        return Ok(());
    }

    info!("generating SELinux policy for foc-devnet containers...");

    let selinux_dir = foc_devnet_selinux();
    fs::create_dir_all(&selinux_dir)?;

    let container_name = "foc-selinux-probe";

    // clean up any leftover probe from a previous failed run
    let _ = Command::new("podman")
        .args(["rm", "-f", container_name])
        .output();

    let bin_dir = foc_devnet_bin();
    let volumes_dir = foc_devnet_docker_volumes();
    let keys_dir = foc_devnet_keys();
    let proof_params_dir = foc_devnet_proof_parameters();

    // create a container from the actual builder image with representative
    // mounts matching what runtime containers use. label=disable because
    // the policy doesn't exist yet.
    let create = Command::new("podman")
        .args([
            "create",
            "--userns=keep-id",
            "--security-opt",
            "label=disable",
            "--name",
            container_name,
            "--network",
            "host",
            "-v",
            &format!("{}:/usr/local/bin/lotus-bins:z", bin_dir.display()),
            "-v",
            &format!("{}:/volumes:z", volumes_dir.display()),
            "-v",
            &format!("{}:/keys:z,ro", keys_dir.display()),
            "-v",
            &format!(
                "{}:{}:z",
                proof_params_dir.display(),
                CONTAINER_FILECOIN_PROOF_PARAMS_PATH
            ),
            "-p",
            "1234:1234",
            "-p",
            "2345:2345",
            "-p",
            "5433:5433",
            BUILDER_DOCKER_IMAGE,
            "true",
        ])
        .output()?;

    if !create.status.success() {
        return Err(format!(
            "podman create probe container failed: {}",
            String::from_utf8_lossy(&create.stderr)
        )
        .into());
    }

    let inspect = Command::new("podman")
        .args(["inspect", container_name])
        .output()?;

    // clean up probe container before checking result
    let _ = Command::new("podman")
        .args(["rm", "-f", container_name])
        .output();

    if !inspect.status.success() {
        return Err(format!(
            "podman inspect failed: {}",
            String::from_utf8_lossy(&inspect.stderr)
        )
        .into());
    }

    // pipe inspect JSON to udica
    let mut udica_child = Command::new("udica")
        .args(["-e", "podman", "-j", "-", POLICY_MODULE])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(&selinux_dir)
        .spawn()?;

    if let Some(mut stdin) = udica_child.stdin.take() {
        stdin.write_all(&inspect.stdout)?;
    }

    let udica_output = udica_child.wait_with_output()?;

    if !udica_output.status.success() {
        return Err(format!(
            "udica failed: {}",
            String::from_utf8_lossy(&udica_output.stderr)
        )
        .into());
    }

    // install policy -- requires root
    let cil_path = selinux_dir.join(format!("{}.cil", POLICY_MODULE));
    info!("installing SELinux policy module (requires sudo)...");

    let install = Command::new("sudo")
        .args(["semodule", "-i", &cil_path.to_string_lossy()])
        .output()?;

    if !install.status.success() {
        return Err(format!(
            "sudo semodule -i failed: {}",
            String::from_utf8_lossy(&install.stderr)
        )
        .into());
    }

    info!("foc_devnet SELinux policy installed");
    Ok(())
}
