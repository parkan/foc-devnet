//! Docker operations for building projects.
//!
//! This module handles Docker image building and container execution for project builds.

use crate::docker::{
    build::build_docker_image,
    builder::ContainerRunBuilder,
    core::image_exists,
};
use crate::embedded_assets;
use crate::paths::foc_devnet_docker_volumes_cache;
use std::collections::HashMap;
use std::fs;
use tracing::info;

use super::Project;

/// Build the builder Docker image.
pub fn build_builder_image(dockerfile_dir: &str) -> Result<String, Box<dyn std::error::Error>> {
    let image_tag = crate::constants::BUILDER_DOCKER_IMAGE;

    // Check if image already exists in Docker
    if image_exists(image_tag)? {
        info!("Docker image {} already exists, skipping build", image_tag);
    } else {
        info!("Building Docker image for builder...");
        build_image_from_dockerfile(dockerfile_dir, image_tag)?;
    }

    Ok(image_tag.to_string())
}

/// Build Docker image from Dockerfile.
pub fn build_image_from_dockerfile(
    dockerfile_dir: &str,
    image_tag: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let dockerfile_path = "docker/builder/Dockerfile";
    let output = build_docker_image(dockerfile_path, image_tag, dockerfile_dir)?;

    if !output.status.success() {
        return Err("Failed to build Docker image".into());
    }

    Ok(())
}

/// Load volume mappings from embedded volumes_map.toml file for a specific image.
pub fn load_volume_map(
    image_name: &str,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let content_bytes = embedded_assets::get_volumes_map(image_name)
        .ok_or_else(|| format!("Embedded volumes map not found for: {}", image_name))?;

    let content = std::str::from_utf8(content_bytes)
        .map_err(|e| format!("Invalid UTF-8 in volumes map for {}: {}", image_name, e))?;

    #[derive(serde::Deserialize)]
    struct VolumesMap {
        volumes: HashMap<String, String>,
    }

    let volume_config: VolumesMap = toml::from_str(content).map_err(|e| {
        format!(
            "Failed to parse embedded volumes map for {}: {}",
            image_name, e
        )
    })?;

    Ok(volume_config.volumes)
}

/// Set up a ContainerRunBuilder for the build container.
pub fn setup_build_builder(
    source_dir: &str,
    output_dir: &str,
    image_tag: &str,
    project: &Project,
) -> Result<ContainerRunBuilder, Box<dyn std::error::Error>> {
    let container_source_dir = "/workspace/source";
    let container_output_dir = "/workspace/output";
    let container_name = format!("{}-{}", crate::constants::BUILDER_CONTAINER, project);

    let mut builder = ContainerRunBuilder::run()
        .rm()
        .name(&container_name)
        .volume(source_dir, container_source_dir)
        .volume(output_dir, container_output_dir);

    // load and apply volume mappings for this image
    let volume_map = load_volume_map("builder")?;
    if !volume_map.is_empty() {
        let cache_dir = foc_devnet_docker_volumes_cache();
        let image_volumes_dir = cache_dir.join(crate::constants::BUILDER_CONTAINER);

        for (host_subdir, container_path) in volume_map {
            let host_path = image_volumes_dir.join(&host_subdir);
            fs::create_dir_all(&host_path)?;
            builder = builder.volume(&host_path.display().to_string(), &container_path);
        }
    }

    builder = builder.image(image_tag);

    Ok(builder)
}

/// Set up the build script for the specific project.
pub fn setup_build_script(
    project: &Project,
    container_source_dir: &str,
    container_output_dir: &str,
) -> String {
    match project {
        Project::Lotus => format!(
            r#"git config --global --add safe.directory {} && \
                cd {} && \
                make clean 2k && \
                cp lotus lotus-miner lotus-shed lotus-seed {}"#,
            container_source_dir, container_source_dir, container_output_dir
        ),
        Project::Curio => format!(
            r#"git config --global --add safe.directory {} && \
                cd {} && \
                make clean 2k pdptool && \
                cp curio sptool pdptool {}"#,
            container_source_dir, container_source_dir, container_output_dir
        ),
    }
}
