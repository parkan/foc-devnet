//! SELinux policy generation for foc-devnet containers using udica.
//!
//! generates a tailored CIL policy from a representative container
//! so containers can run with proper SELinux confinement instead of
//! label=disable.

use crate::paths::{foc_devnet_home, foc_devnet_selinux};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tracing::{info, warn};

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
/// creates a probe container with representative mounts, runs udica
/// to generate a CIL policy, and installs it via semodule. all steps
/// are best-effort -- failures warn but don't block init.
pub fn setup_selinux_policy() -> Result<(), Box<dyn std::error::Error>> {
    if !selinux_enforcing() {
        info!("SELinux not enforcing, skipping policy generation");
        return Ok(());
    }

    if policy_loaded() {
        info!("foc_devnet SELinux policy already loaded");
        return Ok(());
    }

    if Command::new("udica").arg("--help").output().is_err() {
        warn!("udica not installed, containers will run with label=disable");
        warn!("install with: sudo dnf install udica");
        return Ok(());
    }

    info!("generating SELinux policy for foc-devnet containers...");

    let selinux_dir = foc_devnet_selinux();
    fs::create_dir_all(&selinux_dir)?;

    let home = foc_devnet_home();
    let container_name = "foc-selinux-probe";

    // clean up any leftover probe
    let _ = Command::new("docker")
        .args(["rm", "-f", container_name])
        .output();

    // ensure probe mount targets exist
    let bin_dir = home.join("bin");
    let volumes_dir = home.join("docker").join("volumes");
    let keys_dir = home.join("keys");
    fs::create_dir_all(&bin_dir).ok();
    fs::create_dir_all(&volumes_dir).ok();
    fs::create_dir_all(&keys_dir).ok();

    // create a representative container with typical bind mounts.
    // label=disable so create works before the policy exists.
    let create = Command::new("docker")
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
            &format!("{}:/output:z", bin_dir.display()),
            "-v",
            &format!("{}:/volumes:z", volumes_dir.display()),
            "-v",
            &format!("{}:/keys:z", keys_dir.display()),
            "-p",
            "1234:1234",
            "alpine:latest",
            "true",
        ])
        .output()?;

    if !create.status.success() {
        warn!(
            "failed to create probe container: {}",
            String::from_utf8_lossy(&create.stderr)
        );
        return Ok(());
    }

    let inspect = Command::new("docker")
        .args(["inspect", container_name])
        .output()?;

    // clean up probe container immediately
    let _ = Command::new("docker")
        .args(["rm", "-f", container_name])
        .output();

    if !inspect.status.success() {
        warn!("failed to inspect probe container");
        return Ok(());
    }

    // pipe inspect JSON to udica
    let mut udica_child = Command::new("udica")
        .arg(POLICY_MODULE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(&selinux_dir)
        .spawn()?;

    if let Some(mut stdin) = udica_child.stdin.take() {
        stdin.write_all(&inspect.stdout)?;
        // stdin dropped here, signaling EOF to udica
    }

    let udica_output = udica_child.wait_with_output()?;

    if !udica_output.status.success() {
        warn!(
            "udica failed: {}",
            String::from_utf8_lossy(&udica_output.stderr)
        );
        return Ok(());
    }

    let cil_path = selinux_dir.join(format!("{}.cil", POLICY_MODULE));
    if !cil_path.exists() {
        warn!("udica did not produce {}", cil_path.display());
        return Ok(());
    }

    info!("installing SELinux policy module...");

    // try without sudo first
    let install = Command::new("semodule")
        .args(["-i", &cil_path.to_string_lossy()])
        .output()?;

    if install.status.success() {
        info!("foc_devnet SELinux policy installed");
        return Ok(());
    }

    // fall back to sudo
    let install = Command::new("sudo")
        .args(["semodule", "-i", &cil_path.to_string_lossy()])
        .output()?;

    if install.status.success() {
        info!("foc_devnet SELinux policy installed");
    } else {
        warn!(
            "failed to install policy: {}",
            String::from_utf8_lossy(&install.stderr)
        );
        warn!(
            "install manually: sudo semodule -i {}",
            cil_path.display()
        );
    }

    Ok(())
}
