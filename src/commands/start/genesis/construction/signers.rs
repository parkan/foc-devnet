//! Genesis signers management.
//!
//! This module handles adding BLS signer keys to the genesis file.

use crate::commands::start::genesis::constants;
use crate::commands::start::genesis::keys::get_bls_addresses;
use crate::docker::builder::ContainerRunBuilder;
use crate::paths::{
    foc_devnet_bin, foc_devnet_docker_volumes_cache, foc_devnet_genesis, foc_devnet_lotus_keys,
};
use tracing::info;

/// Add signers to the genesis file.
///
/// Runs `lotus-seed genesis set-signers` to add the BLS signer keys
/// with the configured threshold.
pub fn add_signers_to_genesis(run_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    info!("Adding signers to genesis...");

    let genesis_dir = foc_devnet_genesis(run_id);
    let keys_dir = foc_devnet_lotus_keys(run_id);

    let addresses = get_bls_addresses(
        "BLS_SIGNER",
        constants::NUM_SIGNER_KEYS
            .try_into()
            .expect("cannot cast NUM_SIGNER_KEYS into usize"),
        run_id,
    )?;

    if addresses.len() != constants::NUM_SIGNER_KEYS as usize {
        return Err(format!(
            "Expected {} BLS signer addresses, found {}",
            constants::NUM_SIGNER_KEYS,
            addresses.len()
        )
        .into());
    }
    info!("Using signers:");
    for (i, addr) in addresses.iter().enumerate() {
        info!(
            "Key {}: {}...{}",
            i + 1,
            &addr[..6],
            &addr[addr.len() - 4..]
        );
    }

    let bin_dir = foc_devnet_bin();
    let builder_volumes_dir =
        foc_devnet_docker_volumes_cache().join(crate::constants::BUILDER_CONTAINER);

    let output = ContainerRunBuilder::run()
        .name(&format!("foc-{}-genesis-signers", run_id))
        .rm()
        .volume(&bin_dir.display().to_string(), "/opt/bin")
        .volume(
            &builder_volumes_dir.join("cargo").display().to_string(),
            "/home/foc-user/.cargo",
        )
        .volume(&genesis_dir.display().to_string(), "/genesis")
        .volume(&keys_dir.display().to_string(), "/keys")
        .image(crate::constants::BUILDER_DOCKER_IMAGE)
        .cmd(&[
            "/bin/bash",
            "-c",
            &format!(
                "/opt/bin/lotus-seed genesis set-signers --threshold={} --signers {} --signers {} /genesis/{}",
                constants::SIGNERS_THRESHOLD, addresses[0], addresses[1], constants::GENESIS_FILE
            ),
        ])
        .run_raw()?;

    if !output.status.success() {
        return Err(format!(
            "Failed to add signers to genesis: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    info!("Signers added successfully");
    Ok(())
}
