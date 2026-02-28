//! MockUSDFC deployment logic.
//!
//! This module contains the core deployment functionality for the MockUSDFC token.

use super::foundry_setup::{get_mockusdfc_project_dir, setup_foundry_project};
use super::key_management::get_deployer_private_key;
use super::prerequisites::check_required_addresses;
use crate::commands::start::lotus_utils::get_lotus_rpc_url;
use crate::commands::start::step::SetupContext;
use crate::docker::builder::ContainerRunBuilder;
use std::error::Error;
use std::path::PathBuf;
use tracing::{error, info};

/// Deploy MockUSDFC using the Foundry project
pub fn deploy_mock_usdfc_foundry(
    context: &SetupContext,
    private_key: &str,
    lotus_rpc_url: &str,
    run_id: &str,
) -> Result<(String, PathBuf), Box<dyn Error>> {
    info!("Deploying MockUSDFC using Foundry project...");

    let contract_dir = get_mockusdfc_project_dir(run_id)?;

    setup_foundry_project(context, &contract_dir, run_id)?;

    info!("Executing deployment script...");

    let deploy_cmd = format!(
        "cd /workspace && \
         forge script script/Deploy.s.sol:DeployMockUSDFC \
         --rpc-url {} \
         --private-key {} \
         --broadcast \
         --slow \
         --gas-estimate-multiplier 10000 \
         -vv",
        lotus_rpc_url, private_key
    );

    let key = format!("usdfc_deploy_{}", run_id);
    let output = ContainerRunBuilder::builder_ephemeral(&format!("foc-{}-usdfc-deploy", run_id))
        .volume(&contract_dir.display().to_string(), "/workspace")
        .cmd(&["bash", "-c", &deploy_cmd])
        .run_logged(context, &key)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        error!("Deployment failed");
        if !stderr.is_empty() {
            for line in stderr.lines() {
                error!("{}", line);
            }
        }
        return Err("MockUSDFC deployment failed".into());
    }

    let contract_address = stdout
        .lines()
        .find(|line| line.contains("MockUSDFC deployed at:"))
        .and_then(|line| line.split_whitespace().last())
        .ok_or("Failed to extract contract address from deployment output")?;

    info!("MockUSDFC deployed at: {}", contract_address);

    Ok((contract_address.to_string(), contract_dir))
}

/// Perform the MockUSDFC deployment process
pub fn perform_token_deployment(
    _volumes_dir: &std::path::PathBuf,
    context: &super::super::step::SetupContext,
) -> Result<(), Box<dyn Error>> {
    info!("Deploying MockUSDFC token using Foundry project...");

    let (mockusdfc_deployer, mockusdfc_deployer_eth) = check_required_addresses(context)?;
    let private_key = get_deployer_private_key(&mockusdfc_deployer)?;

    info!("Deployer ETH address: {}", mockusdfc_deployer_eth);

    let lotus_rpc_url = get_lotus_rpc_url(context)?;
    let run_id = context.run_id();

    let (mock_usdfc_address, contract_dir) =
        deploy_mock_usdfc_foundry(context, &private_key, &lotus_rpc_url, run_id)?;

    context.set("mockusdfc_contract_address", &mock_usdfc_address);

    super::contract_storage::save_contract_address(run_id, "usdfc", &mock_usdfc_address)?;

    super::verification::verify_mock_usdfc(
        context,
        &private_key,
        &mock_usdfc_address,
        &lotus_rpc_url,
        run_id,
        &contract_dir,
    )?;

    info!("MockUSDFC token deployed successfully!");
    info!("Token Address: {}", mock_usdfc_address);
    info!(
        "Initial Supply: {} tokens",
        super::usdfc_deploy_step::MOCK_USDFC_INITIAL_SUPPLY
    );
    info!("Decimals: 18");

    Ok(())
}
