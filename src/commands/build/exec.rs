//! Container execution for build processes.
//!
//! This module handles running build processes inside Docker containers with logging.

use super::docker;
use super::logging;
use super::Project;
use crate::docker::builder::ContainerRunBuilder;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use tracing::info;
use tracing::warn;

/// Time to wait for build container initialization (in seconds)
const BUILD_INIT_WAIT_SECS: u64 = 5;

/// Run the build process inside the Docker container.
pub fn run_build_in_container(
    source_dir: &str,
    output_dir: &str,
    project: &Project,
    image_tag: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Building {} in container...", project);

    let log_path = logging::create_build_log_path()?;
    info!("Logs will be saved to: {}", log_path.display());

    let container_source_dir = "/workspace/source";
    let container_output_dir = "/workspace/output";

    let builder =
        docker::setup_build_builder(source_dir, output_dir, image_tag, project)?;
    let build_script =
        docker::setup_build_script(project, container_source_dir, container_output_dir);

    execute_build_process(builder, build_script, &log_path, project)?;

    info!("Build logs saved to: {}", log_path.display());

    Ok(())
}

/// Execute the build process in the Docker container.
pub fn execute_build_process(
    builder: ContainerRunBuilder,
    build_script: String,
    log_path: &Path,
    project: &Project,
) -> Result<(), Box<dyn std::error::Error>> {
    let builder = builder.cmd(&["/bin/bash", "-c", &build_script]);

    let mut child = builder.spawn()?;

    // Get handles to stdout and stderr
    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    // Create log file
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    info!("NOTE: StdErr output does not necessarily indicate failure");

    std::thread::sleep(std::time::Duration::from_secs(BUILD_INIT_WAIT_SECS));

    // Stream stdout to both console and log file
    let stdout_handle = std::thread::spawn({
        let mut log_file = log_file.try_clone()?;
        move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                info!("(stdout): {}", line);
                writeln!(log_file, "{}", line).ok();
            }
        }
    });

    // Stream stderr to both console and log file
    let stderr_handle = std::thread::spawn({
        let mut log_file = log_file.try_clone()?;
        move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                warn!("(stderr): {}", line);
                writeln!(log_file, "{}", line).ok();
            }
        }
    });

    // Wait for both threads to finish
    stdout_handle.join().ok();
    stderr_handle.join().ok();

    // Wait for the child process to finish
    let status = child.wait()?;

    if !status.success() {
        return Err(format!(
            "Failed to build {} in container. Check logs at: {}",
            project,
            log_path.display()
        )
        .into());
    }

    Ok(())
}

