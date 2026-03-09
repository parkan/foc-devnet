//! Docker command building for Lotus-Miner.

use std::error::Error;
use std::path::Path;

use super::constants::LOTUS_API_WAIT_SLEEP_SECS;
use crate::commands::start::lotus_utils::{build_fullnode_api_info, read_lotus_token};
use crate::commands::start::step::SetupContext;
use crate::docker::builder::ContainerRunBuilder;
use crate::docker::containers::{lotus_container_name, lotus_miner_container_name};
use crate::docker::network::cluster_network_name;
use crate::paths::{
    foc_devnet_bin, foc_devnet_docker_volumes_cache, foc_devnet_genesis_sectors_lotus_miner,
    foc_devnet_proof_parameters, CONTAINER_FILECOIN_PROOF_PARAMS_PATH,
};

/// Build a ContainerRunBuilder for Lotus-Miner
pub fn build_miner_builder(
    volumes_dir: &Path,
    preseal_files: &(String, String),
    context: &SetupContext,
) -> Result<ContainerRunBuilder, Box<dyn Error>> {
    let (preseal_file, preseal_key_file) = preseal_files;
    let run_id = context.run_id();
    let container_name = lotus_miner_container_name(run_id);
    let filecoin_network = cluster_network_name(run_id);
    let lotus_name = lotus_container_name(run_id);

    let lotus_data_dir = volumes_dir.join("lotus-data");

    let lotus_token = read_lotus_token(run_id)?;
    let fullnode_api_info = build_fullnode_api_info(&lotus_token, &lotus_name);

    let bin_dir = foc_devnet_bin();
    let sectors_dir = foc_devnet_genesis_sectors_lotus_miner(run_id);
    let builder_volumes_dir =
        foc_devnet_docker_volumes_cache().join(crate::constants::BUILDER_CONTAINER);
    let params_dir = foc_devnet_proof_parameters();

    let miner_api_port: u16 = context
        .get("lotus_miner_api_port")
        .ok_or("Lotus-Miner API port not found in context")?
        .parse()?;

    let miner_data_dir = volumes_dir.join("lotus-miner-data");

    let miner_cmd = format!(
        r#"echo "Waiting for Lotus daemon API to be ready..." && \
           until /usr/local/bin/lotus-bins/lotus version >/dev/null 2>&1; do \
             echo "Lotus API not ready yet, waiting..." && sleep {}; \
           done && \
           echo "Lotus daemon API is ready!" && \
           if [ ! -f $LOTUS_MINER_PATH/config.toml ]; then \
             echo "Importing pre-sealed miner key..." && \
             (/usr/local/bin/lotus-bins/lotus wallet import --as-default /sectors/{} 2>&1 | grep --invert-match "key already exists" || true) && \
             echo "Initializing lotus-miner..." && \
             /usr/local/bin/lotus-bins/lotus-miner init --genesis-miner --actor=t01000 --sector-size=2KiB \
               --pre-sealed-sectors=/sectors --pre-sealed-metadata=/sectors/{} --nosync; \
           fi && \
           echo "Starting lotus-miner..." && \
           /usr/local/bin/lotus-bins/lotus-miner run --nosync"#,
        LOTUS_API_WAIT_SLEEP_SECS, preseal_key_file, preseal_file
    );

    let builder = ContainerRunBuilder::daemon(&container_name, &filecoin_network)
        .port(miner_api_port, 2345)
        .volume(&bin_dir.display().to_string(), "/usr/local/bin/lotus-bins")
        .volume(
            &miner_data_dir.display().to_string(),
            "/home/foc-user/.lotus-miner-local-net",
        )
        .volume(
            &lotus_data_dir.display().to_string(),
            "/home/foc-user/.lotus-local-net",
        )
        .volume(&sectors_dir.display().to_string(), "/sectors")
        .volume(
            &params_dir.display().to_string(),
            CONTAINER_FILECOIN_PROOF_PARAMS_PATH,
        )
        .volume(
            &builder_volumes_dir.join("cargo").display().to_string(),
            "/cargo",
        )
        .env("FULLNODE_API_INFO", &fullnode_api_info)
        .workdir("/home/foc-user/.lotus-miner-local-net")
        .image(super::constants::IMAGE_NAME)
        .cmd(&["/bin/bash", "-c", &miner_cmd]);

    Ok(builder)
}
