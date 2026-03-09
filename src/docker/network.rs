//! Docker network management for run-isolated clusters.
//!
//! All daemon containers share a single bridge network per run.
//! Ephemeral builder containers use host networking instead.

use super::core::docker_command;
use std::error::Error;
use tracing::info;

/// Get the cluster network name for a run ID
pub fn cluster_network_name(run_id: &str) -> String {
    format!("foc_{}", run_id)
}

/// Check if a Docker network exists
pub fn network_exists(network_name: &str) -> Result<bool, Box<dyn Error>> {
    let output = docker_command(&[
        "network",
        "ls",
        "--filter",
        &format!("name=^{}$", network_name),
        "--format",
        "{{.Name}}",
    ])?;

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .contains(network_name))
}

/// Create a Docker user-defined bridge network
pub fn create_network(network_name: &str) -> Result<(), Box<dyn Error>> {
    info!("Creating network '{}'...", network_name);

    if network_exists(network_name)? {
        info!("Network already exists");
        return Ok(());
    }

    docker_command(&["network", "create", "--driver", "bridge", network_name])?;
    info!("Network created");

    Ok(())
}

/// Delete a Docker network
pub fn delete_network(network_name: &str) -> Result<(), Box<dyn Error>> {
    info!("Removing network '{}'...", network_name);

    if !network_exists(network_name)? {
        info!("Network does not exist");
        return Ok(());
    }

    docker_command(&["network", "rm", network_name])?;
    info!("Network removed");

    Ok(())
}

/// Create the cluster network for a run
pub fn create_all_networks(run_id: &str, _active_pdp_sp_count: usize) -> Result<(), Box<dyn Error>> {
    info!("Creating Docker network...");
    create_network(&cluster_network_name(run_id))?;
    info!("Network created successfully");
    Ok(())
}

/// Delete the cluster network for a run
pub fn delete_all_networks(run_id: &str) -> Result<(), Box<dyn Error>> {
    info!("Removing Docker network...");
    delete_network(&cluster_network_name(run_id))?;
    info!("Network removed successfully");
    Ok(())
}
