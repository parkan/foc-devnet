//! Setup functions for Lotus daemon startup.

use super::super::step::SetupContext;
use crate::constants::LOTUS_DOCKER_IMAGE;
use crate::docker::builder::ContainerRunBuilder;
use crate::docker::containers::lotus_container_name;
use crate::docker::network::cluster_network_name;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// Create a pre-configured config.toml with FEVM and ChainIndexer enabled
pub fn create_fevm_config(lotus_data_dir: &PathBuf) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(lotus_data_dir)?;
    let config_path = lotus_data_dir.join("config.toml");

    let config_content = r#"[API]
  ListenAddress = "/ip4/0.0.0.0/tcp/1234/http"
  Timeout = "30s"
  DisableAuth = true

[Chainstore]
  EnableSplitstore = false

[Fevm]
  EnableEthRPC = true

[ChainIndexer]
  EnableIndexer = true
"#;

    fs::write(&config_path, config_content)?;
    Ok(())
}

/// Set up necessary directories for Lotus daemon
pub fn setup_directories(volumes_dir: &Path) -> Result<(), Box<dyn Error>> {
    let lotus_data_dir = volumes_dir.join("lotus-data");
    fs::create_dir_all(&lotus_data_dir)?;

    let devgen_dir = volumes_dir.join("devgen");
    fs::create_dir_all(&devgen_dir)?;

    create_fevm_config(&lotus_data_dir)?;

    Ok(())
}

/// Build a ContainerRunBuilder for starting Lotus daemon
pub fn build_lotus_builder(
    volumes_dir: &Path,
    context: &SetupContext,
) -> Result<ContainerRunBuilder, Box<dyn Error>> {
    use super::super::genesis::constants::GENESIS_FILE;
    use crate::paths::{
        foc_devnet_bin, foc_devnet_genesis, foc_devnet_genesis_sectors, foc_devnet_lotus_keys,
        foc_devnet_proof_parameters, CONTAINER_FILECOIN_PROOF_PARAMS_PATH,
    };

    let lotus_api_port: u16 = context
        .get("lotus_api_port")
        .ok_or("Lotus API port not found in context")?
        .parse()?;
    let lotus_p2p_port: u16 = context
        .get("lotus_p2p_port")
        .ok_or("Lotus P2P port not found in context")?
        .parse()?;

    let run_id = context.run_id();
    let container_name = lotus_container_name(run_id);
    let network_name = cluster_network_name(run_id);

    let bin_dir = foc_devnet_bin();
    let params_dir = foc_devnet_proof_parameters();
    let genesis_dir = foc_devnet_genesis(run_id);
    let sectors_dir = foc_devnet_genesis_sectors(run_id);
    let keys_dir = foc_devnet_lotus_keys(run_id);
    let genesis_file = genesis_dir.join(GENESIS_FILE);

    let genesis_filename = genesis_file
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let lotus_cmd = format!(
        r#"/usr/local/bin/lotus-bins/lotus daemon \
            --lotus-make-genesis=/devgen/devgen.car \
            --genesis-template=/genesis/{} \
            --bootstrap=false"#,
        genesis_filename
    );

    let builder = ContainerRunBuilder::daemon(&container_name, &network_name)
        .port(lotus_api_port, 1234)
        .port(lotus_p2p_port, 1946)
        .volume(&bin_dir.display().to_string(), "/usr/local/bin/lotus-bins")
        .volume(
            &volumes_dir.join("lotus-data").display().to_string(),
            "/home/foc-user/.lotus-local-net",
        )
        .volume(
            &volumes_dir.join("devgen").display().to_string(),
            "/devgen",
        )
        .volume(
            &params_dir.display().to_string(),
            CONTAINER_FILECOIN_PROOF_PARAMS_PATH,
        )
        .volume(&genesis_dir.display().to_string(), "/genesis")
        .volume(&sectors_dir.display().to_string(), "/sectors")
        .volume(&keys_dir.display().to_string(), "/keys")
        .workdir("/tmp")
        .image(LOTUS_DOCKER_IMAGE)
        .cmd(&["/bin/bash", "-c", &lotus_cmd]);

    Ok(builder)
}
