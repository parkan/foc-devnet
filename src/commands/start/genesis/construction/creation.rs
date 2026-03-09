//! Genesis file creation.
//!
//! This module handles creating the initial genesis file using lotus-seed.

use crate::commands::start::genesis::constants;
use crate::docker::builder::ContainerRunBuilder;
use crate::paths::{foc_devnet_bin, foc_devnet_docker_volumes_cache, foc_devnet_genesis};
use std::fs;
use tracing::info;

/// Create the initial genesis file.
///
/// Runs `lotus-seed genesis new` to create a new genesis file with the network name
/// and current timestamp.
///
/// Note: This function assumes the genesis file does not already exist.
/// The caller should check for existence first.
pub fn create_genesis_file(run_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let genesis_dir = foc_devnet_genesis(run_id);

    info!("Creating genesis file...");

    // Ensure genesis directory exists
    fs::create_dir_all(&genesis_dir)?;

    let now = std::time::SystemTime::now();
    let datetime: chrono::DateTime<chrono::Utc> = now.into();
    let timestamp = datetime.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let bin_dir = foc_devnet_bin();
    let builder_volumes_dir =
        foc_devnet_docker_volumes_cache().join(crate::constants::BUILDER_CONTAINER);

    let output = ContainerRunBuilder::run()
        .name(&format!("foc-{}-genesis-creation", run_id))
        .rm()
        .volume(&bin_dir.display().to_string(), "/opt/bin")
        .volume(
            &builder_volumes_dir.join("cargo").display().to_string(),
            "/home/foc-user/.cargo",
        )
        .volume(&genesis_dir.display().to_string(), "/genesis")
        .image(crate::constants::BUILDER_DOCKER_IMAGE)
        .cmd(&[
            "/bin/bash",
            "-c",
            &format!(
                "/opt/bin/lotus-seed genesis new --network-name {} --timestamp {} /genesis/{} && chmod 666 /genesis/{}",
                constants::NETWORK_NAME, timestamp, constants::GENESIS_FILE, constants::GENESIS_FILE
            ),
        ])
        .run_raw()?;

    if !output.status.success() {
        return Err(format!(
            "Failed to create genesis file: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    info!("Genesis file created successfully");
    Ok(())
}
