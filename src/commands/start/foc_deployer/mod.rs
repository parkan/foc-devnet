//! FOC contract deployment logic.
//!
//! This module contains the core logic for deploying FOC contracts,
//! including private key extraction, deployment script execution,
//! and output parsing.

use super::foc_metadata::FOCMetadata;
use crate::constants::*;
use crate::docker::builder::ContainerRunBuilder;
use crate::paths::{foc_devnet_bin, foc_devnet_docker_volumes_cache};
use std::error::Error;
use tracing::{info, warn};

/// Deployment result containing contract addresses and network metadata
pub struct DeploymentResult {
    /// Map of contract names to addresses
    pub addresses: std::collections::HashMap<String, String>,
    /// FilBeam controller address
    pub filbeam_controller: Option<String>,
    /// FilBeam beneficiary address
    pub filbeam_beneficiary: Option<String>,
    /// Network configuration metadata
    pub metadata: FOCMetadata,
}

/// Get the private key for an address in hex format (for use with cast/forge).
/// Supports both Ethereum (0x) and Filecoin (t4) address formats.
pub fn get_private_key(address: &str, _lotus_container: &str) -> Result<String, Box<dyn Error>> {
    let keys = crate::commands::init::keys::load_keys()?;

    let key_info = if address.starts_with("0x") || address.starts_with("0X") {
        keys.iter()
            .find(|k| {
                k.eth_address
                    .as_ref()
                    .map(|eth| eth.eq_ignore_ascii_case(address))
                    .unwrap_or(false)
            })
            .ok_or(format!(
                "Private key not found for Ethereum address: {}",
                address
            ))?
    } else {
        keys.iter()
            .find(|k| k.filecoin_address.as_ref() == Some(&address.to_string()))
            .ok_or(format!(
                "Private key not found for Filecoin address: {}",
                address
            ))?
    };

    Ok(format!("0x{}", key_info.private_key))
}

/// Deploy FOC contracts using the deployment script
pub fn deploy_foc_contracts(
    foc_deployer: &str,
    deployer_eth_addr: &str,
    mock_usdfc_address: &str,
    services_repo_path: &std::path::Path,
    lotus_container: &str,
    lotus_rpc_url: &str,
    run_id: &str,
) -> Result<DeploymentResult, Box<dyn Error>> {
    info!("Running deploy-all-warm-storage.sh...");
    info!("Lotus RPC URL: {}", lotus_rpc_url);

    let services_repo = services_repo_path
        .canonicalize()
        .unwrap_or_else(|_| services_repo_path.to_path_buf());
    let contracts_dir = services_repo.join("service_contracts");
    let deploy_script = contracts_dir
        .join("tools")
        .join("deploy-all-warm-storage.sh");

    if !deploy_script.exists() {
        return Err(format!("Deployment script not found at {}", deploy_script.display()).into());
    }

    let bin_dir = foc_devnet_bin();
    let builder_volumes_dir =
        foc_devnet_docker_volumes_cache().join(crate::constants::BUILDER_CONTAINER);

    let private_key = get_private_key(foc_deployer, lotus_container)?;

    let deploy_cmd = format!(
        r#"set -e
mkdir -p /home/foc-user/.foundry/keystores
cast wallet import foc-deployer --private-key {} --unsafe-password ''
cd /service_contracts
bash /service_contracts/tools/deploy-all-warm-storage.sh 2>&1 | tee /tmp/foc-deploy.log"#,
        private_key
    );

    info!("This may take several minutes...");

    let container_name = format!("foc-{}-foc-deploy", run_id);

    let output = ContainerRunBuilder::builder_ephemeral(&container_name)
        .env("ETH_RPC_URL", lotus_rpc_url)
        .env("USDFC_TOKEN_ADDRESS", mock_usdfc_address)
        .env("SERVICE_NAME", "FOC DevNet Warm Storage")
        .env(
            "SERVICE_DESCRIPTION",
            "Warm storage service for FOC local development network",
        )
        .env("DRY_RUN", "false")
        .env("CHAIN", &LOCAL_NETWORK_CHAIN_ID.to_string())
        .env("DEPLOYER_ADDRESS", deployer_eth_addr)
        .env("AUTO_VERIFY", "false")
        .env("ETH_PRIVATE_KEY", &private_key)
        .env("PASSWORD", "")
        .env(
            "ETH_KEYSTORE",
            "/home/foc-user/.foundry/keystores/foc-deployer",
        )
        .volume(&bin_dir.display().to_string(), "/opt/bin")
        .volume(
            &builder_volumes_dir.join("cargo").display().to_string(),
            "/home/foc-user/.cargo",
        )
        .volume(&contracts_dir.display().to_string(), "/service_contracts")
        .cmd(&["/bin/bash", "-c", &deploy_cmd])
        .run_raw()?;

    let output_str = String::from_utf8_lossy(&output.stdout);

    if !output.status.success() {
        warn!(
            "Deployment container failed with exit status: {:?}",
            output.status.code()
        );

        let stderr_str = String::from_utf8_lossy(&output.stderr);
        if !stderr_str.is_empty() {
            warn!("=== STDERR ===");
            for line in stderr_str.lines() {
                warn!("{}", line);
            }
        }

        if !output_str.is_empty() {
            warn!("=== STDOUT ===");
            for line in output_str.lines() {
                warn!("{}", line);
            }
        }

        if let Ok(logs) = crate::docker::core::get_container_logs(&container_name) {
            if !logs.is_empty() {
                warn!("=== CONTAINER LOGS ===");
                for line in logs.lines() {
                    warn!("{}", line);
                }
            }
        }

        return Err("FOC contract deployment failed".into());
    }

    let deployment_result = parse_deployment_output(&output_str)?;

    Ok(deployment_result)
}

/// Parse deployment output to extract contract addresses and network metadata
pub fn parse_deployment_output(output_str: &str) -> Result<DeploymentResult, Box<dyn Error>> {
    let mut addresses = std::collections::HashMap::new();
    let mut filbeam_controller = None;
    let mut filbeam_beneficiary = None;
    let mut network_name = String::from("devnet");
    let mut challenge_finality = String::new();
    let mut max_proving_period = String::new();
    let mut challenge_window_size = String::new();
    let mut service_name = String::new();
    let mut service_description = String::new();

    info!("Parsing deployment output for contract addresses...");

    let mut in_summary = false;
    let mut in_network_config = false;

    for line in output_str.lines() {
        // Parse "SessionKeyRegistry deployed at 0x..." (appears before summary)
        if line.contains("SessionKeyRegistry deployed at") {
            if let Some(addr) = extract_address_from_deployed_line(line) {
                info!("Found SessionKeyRegistry: {}", addr);
                addresses.insert("session_key_registry".to_string(), addr);
                continue;
            }
        }

        if line.contains("DEPLOYMENT SUMMARY") {
            in_summary = true;
            info!("Found DEPLOYMENT SUMMARY section");
            continue;
        }

        if in_summary && line.contains(":") && line.contains("0x") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() == 2 {
                let name = parts[0].trim();
                let addr = parts[1].trim();
                if addr.starts_with("0x") && !addr.is_empty() {
                    let snake_case_name = to_snake_case(name);
                    info!("Found contract: {} -> {}", snake_case_name, addr);
                    addresses.insert(snake_case_name, addr.to_string());
                }
            }
        }

        if line.contains("Network Configuration") {
            in_network_config = true;
            in_summary = false;
            info!("Found Network Configuration section");

            if let Some(start) = line.find('(') {
                if let Some(end) = line.find(')') {
                    network_name = line[start + 1..end].to_string();
                    info!("Network name: {}", network_name);
                }
            }
            continue;
        }

        if in_network_config && line.contains(":") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let key = parts[0].trim();
                let value = parts[1..].join(":").trim().to_string();

                match key {
                    "Challenge finality" => {
                        challenge_finality = value.replace(" epochs", "").trim().to_string();
                    }
                    "Max proving period" => {
                        max_proving_period = value.replace(" epochs", "").trim().to_string();
                    }
                    "Challenge window size" => {
                        challenge_window_size = value.replace(" epochs", "").trim().to_string();
                    }
                    "FilBeam controller address" => {
                        filbeam_controller = Some(value.clone());
                    }
                    "FilBeam beneficiary address" => {
                        filbeam_beneficiary = Some(value.clone());
                    }
                    "Service name" => {
                        service_name = value.clone();
                    }
                    "Service description" => {
                        service_description = value.clone();
                    }
                    _ => {}
                }
            }
        }
    }

    if addresses.is_empty() {
        warn!("No contract addresses found in output");
        for line in output_str.lines() {
            info!("{}", line);
        }
    } else {
        info!(
            "Successfully parsed {} contracts from output",
            addresses.len()
        );
    }

    let metadata = FOCMetadata {
        network_name,
        challenge_finality,
        max_proving_period,
        challenge_window_size,
        service_name,
        service_description,
    };

    Ok(DeploymentResult {
        addresses,
        filbeam_controller,
        filbeam_beneficiary,
        metadata,
    })
}

/// Extract an address from a "<Name> deployed at 0x..." line.
///
/// # Example
/// ```text
/// SessionKeyRegistry deployed at 0xaF69542d01111EdfB7B63Aa974E6A2c9A31EA1E9
/// ```
fn extract_address_from_deployed_line(line: &str) -> Option<String> {
    let marker = "deployed at ";
    let idx = line.find(marker)?;
    let addr = line[idx + marker.len()..].trim();
    if addr.starts_with("0x") && addr.len() >= 42 {
        Some(addr.to_string())
    } else {
        None
    }
}

/// Convert a contract name to snake_case
fn to_snake_case(name: &str) -> String {
    let mut result = name
        .chars()
        .enumerate()
        .fold(String::new(), |mut acc, (i, c)| {
            if c.is_uppercase() && i > 0 {
                acc.push('_');
            }
            acc.push(c.to_lowercase().next().unwrap());
            acc
        })
        .replace(" ", "_");

    while result.contains("__") {
        result = result.replace("__", "_");
    }

    result
}
