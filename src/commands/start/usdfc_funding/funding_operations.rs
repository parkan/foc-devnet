//! MockUSDFC token transfer operations.
//!
//! This module provides utilities for transferring MockUSDFC tokens between addresses.

use crate::commands::start::step::SetupContext;
use crate::docker::builder::ContainerRunBuilder;
use crate::utils::retry::{retry_with_fixed_delay, DEFAULT_MAX_RETRIES, DEFAULT_RETRY_DELAY_SECS};
use ethers_core::types::U256;
use hex;
use std::error::Error;
use tracing::info;

/// Parameters for MockUSDFC transfer operations
pub struct USDFCTransferParams<'a> {
    pub from_private_key: &'a str,
    pub to_eth_address: &'a str,
    pub amount: &'a str,
    pub token_address: &'a str,
    pub description: &'a str,
    pub nonce: Option<u64>,
    pub lotus_rpc_url: &'a str,
}

/// Transfer MockUSDFC tokens from one address to another using cast
pub fn transfer_mock_usdfc(
    params: &USDFCTransferParams,
    context: &SetupContext,
) -> Result<(), Box<dyn Error>> {
    info!("Transferring MockUSDFC tokens: {}...", params.description);

    let mut cast_cmd = format!(
        "cd /workspace && cast send {} \
         --private-key {} \
         --rpc-url {} \
         'transfer(address,uint256)' {} {} \
         --gas-limit 100000000",
        params.token_address,
        params.from_private_key,
        params.lotus_rpc_url,
        params.to_eth_address,
        params.amount
    );

    if let Some(nonce_val) = params.nonce {
        cast_cmd.push_str(&format!(" --nonce {}", nonce_val));
    }

    let key = format!("usdfc_transfer_{}", params.description.replace(" ", "_"));
    let container_name = format!(
        "foc-{}-usdfc-transfer-{}",
        context.run_id(),
        params.description.replace(" ", "-").replace("→", "to")
    );
    let output = ContainerRunBuilder::builder_ephemeral(&container_name)
        .volume("/tmp", "/workspace")
        .cmd(&["bash", "-c", &cast_cmd])
        .run_logged(context, &key)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("Transfer failed");
        return Err(format!("Failed to transfer MockUSDFC: {}", stderr).into());
    }

    Ok(())
}

/// Check the MockUSDFC balance of an address
pub fn check_mock_usdfc_balance(
    context: &SetupContext,
    eth_address: &str,
    token_address: &str,
    lotus_rpc_url: &str,
) -> Result<U256, Box<dyn Error>> {
    retry_with_fixed_delay(
        || {
            let key = format!("usdfc_balance_check_{}", eth_address);
            let container_name = format!(
                "foc-{}-usdfc-balance-check-{}",
                context.run_id(),
                &eth_address[..8]
            );
            let output = ContainerRunBuilder::builder_ephemeral(&container_name)
                .cmd(&[
                    "bash",
                    "-c",
                    &format!(
                        "cast call {} \
                         --rpc-url {} \
                         'balanceOf(address)' {}",
                        token_address, lotus_rpc_url, eth_address
                    ),
                ])
                .run_logged(context, &key)?;

            if !output.status.success() {
                return Err(format!(
                    "Failed to check balance for {}: {}",
                    eth_address,
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }

            let balance_hex = String::from_utf8_lossy(&output.stdout).trim().to_string();

            if balance_hex.is_empty() || balance_hex == "0x" {
                return Ok(U256::zero());
            }

            let hex_str = balance_hex.strip_prefix("0x").unwrap_or(&balance_hex);
            let bytes = hex::decode(hex_str).map_err(|e| -> Box<dyn Error> {
                format!("Failed to decode hex string: {}: {}", hex_str, e).into()
            })?;
            let balance_u256 = U256::from_big_endian(&bytes);

            Ok(balance_u256)
        },
        DEFAULT_MAX_RETRIES,
        DEFAULT_RETRY_DELAY_SECS,
        &format!("MockUSDFC balance check for {}", eth_address),
    )
}
