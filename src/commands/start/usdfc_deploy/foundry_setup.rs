//! Foundry project setup for MockUSDFC deployment.
//!
//! This module handles the setup and preparation of the Foundry project
//! for deploying the MockUSDFC contract.

use crate::commands::start::step::SetupContext;
use crate::docker::builder::ContainerRunBuilder;
use crate::embedded_assets;
use crate::paths::foc_devnet_run_dir;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

/// Get or create the MockUSDFC project directory from embedded assets
pub fn get_mockusdfc_project_dir(run_id: &str) -> Result<PathBuf, Box<dyn Error>> {
    let run_dir = foc_devnet_run_dir(run_id);
    let extract_target = run_dir.join("mockusdfc-extract");

    if extract_target.exists() {
        fs::remove_dir_all(&extract_target)?;
    }

    embedded_assets::extract_mockusdfc_project(&extract_target)?;

    let mockusdfc_dir = extract_target.join("contracts").join("MockUSDFC");

    if !mockusdfc_dir.exists() {
        return Err(format!(
            "MockUSDFC directory not found after extraction at: {}",
            mockusdfc_dir.display()
        )
        .into());
    }

    Ok(mockusdfc_dir)
}

/// Setup the Foundry project (install dependencies if needed)
pub fn setup_foundry_project(
    context: &SetupContext,
    contract_dir: &Path,
    run_id: &str,
) -> Result<(), Box<dyn Error>> {
    let openzeppelin_path = contract_dir.join("lib/openzeppelin-contracts");

    if !openzeppelin_path.exists() {
        info!("Installing OpenZeppelin contracts...");

        let git_dir = contract_dir.join(".git");
        if !git_dir.exists() {
            info!("Initializing git repository...");
            let key = format!("usdfc_setup_git_init_{}", run_id);
            let output = ContainerRunBuilder::builder_ephemeral(&format!(
                "foc-{}-usdfc-git-init",
                run_id
            ))
            .volume(&contract_dir.display().to_string(), "/workspace")
            .cmd(&[
                "bash",
                "-c",
                "cd /workspace && git init && git config user.email 'foc@devnet' && git config user.name 'FOC DevNet'",
            ])
            .run_logged(context, &key)?;

            if !output.status.success() {
                return Err(format!(
                    "Failed to initialize git repository: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
        }

        let key = format!("usdfc_setup_install_deps_{}", run_id);
        let output = ContainerRunBuilder::builder_ephemeral(&format!(
            "foc-{}-usdfc-install-deps",
            run_id
        ))
        .volume(&contract_dir.display().to_string(), "/workspace")
        .cmd(&[
            "bash",
            "-c",
            "cd /workspace && \
             forge install OpenZeppelin/openzeppelin-contracts@v5.0.0 && \
             forge install foundry-rs/forge-std",
        ])
        .run_logged(context, &key)?;

        if !output.status.success() {
            return Err(format!(
                "Failed to install dependencies: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        info!("Dependencies installed");
    }

    info!("Building MockUSDFC contract...");
    let key = format!("usdfc_setup_build_{}", run_id);
    let output =
        ContainerRunBuilder::builder_ephemeral(&format!("foc-{}-usdfc-build", run_id))
            .volume(&contract_dir.display().to_string(), "/workspace")
            .cmd(&["bash", "-c", "cd /workspace && forge build"])
            .run_logged(context, &key)?;

    if !output.status.success() {
        return Err(format!(
            "Failed to build contracts: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    info!("Contracts built");
    Ok(())
}
