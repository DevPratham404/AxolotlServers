use anyhow::{Context, Result};
use bollard::Docker;
use bollard::models::NetworkCreateRequest;
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptions, RemoveContainerOptionsBuilder,
    StopContainerOptionsBuilder,
};
use cluster_shared::CreateServerSpec;
use futures_util::TryStreamExt;

use crate::{
    config::DaemonConfig,
    error::{DaemonError, is_conflict, is_not_found, is_not_modified},
    minecraft, runtimes,
};

/// Docker's default stop grace period, matching the image's shutdown needs.
pub const STOP_TIMEOUT_SECS: i32 = 35;

/// Connect to the local Docker Engine (unix socket, or `DOCKER_HOST`).
pub fn connect() -> Result<Docker> {
    Docker::connect_with_local_defaults().context("connect to Docker Engine")
}

/// Round-trip check that returns the engine version string.
pub async fn ping(docker: &Docker) -> Result<String> {
    let version = docker
        .version()
        .await
        .context("query Docker engine version")?;
    Ok(version.version.unwrap_or_else(|| "unknown".to_string()))
}

/// Create the shared bridge network if it does not exist yet.
pub async fn ensure_network(docker: &Docker, name: &str) -> Result<(), DaemonError> {
    match docker.inspect_network(name, None).await {
        Ok(_) => Ok(()),
        Err(error) if is_not_found(&error) => {
            let request = NetworkCreateRequest {
                name: name.to_string(),
                driver: Some("bridge".to_string()),
                ..Default::default()
            };
            docker
                .create_network(request)
                .await
                .map(|_| ())
                .map_err(DaemonError::from)
        }
        Err(error) => Err(DaemonError::from(error)),
    }
}

/// Ensure the required container image exists locally; if not, pull it to completion.
pub async fn ensure_image(docker: &Docker, image: &str) -> Result<(), DaemonError> {
    match docker.inspect_image(image).await {
        Ok(_) => Ok(()),
        Err(error) if is_not_found(&error) => {
            tracing::info!(image, "image not found locally; pulling from registry");
            let options = Some(CreateImageOptions {
                from_image: Some(image.to_string()),
                ..Default::default()
            });
            let mut stream = docker.create_image(options, None, None);
            while let Some(info) = stream.try_next().await.map_err(DaemonError::from)? {
                if let Some(detail) = info.error_detail {
                    let msg = detail
                        .message
                        .unwrap_or_else(|| "unknown error".to_string());
                    return Err(DaemonError::Internal(format!(
                        "failed to pull image {image}: {msg}"
                    )));
                }
            }
            tracing::info!(image, "image pull completed");
            Ok(())
        }
        Err(error) => Err(DaemonError::from(error)),
    }
}

/// Create (but do not start) the server container.
pub async fn create_server(
    docker: &Docker,
    config: &DaemonConfig,
    spec: &CreateServerSpec,
) -> Result<String, DaemonError> {
    ensure_network(docker, &config.network).await?;
    let image = if matches!(spec.server_type, cluster_shared::ServerType::Bedrock) {
        &config.bedrock_image
    } else if spec.server_type.is_proxy() {
        &config.proxy_image
    } else {
        &config.image
    };
    ensure_image(docker, image).await?;

    // Docker creates the bind source if missing, but doing it here surfaces
    // permission errors in our logs instead of an opaque engine failure.
    let volume = format!(
        "{}/{}/data",
        config.data_root.trim_end_matches('/'),
        spec.id
    );
    std::fs::create_dir_all(&volume)
        .map_err(|error| DaemonError::Internal(format!("create volume {volume}: {error}")))?;

    let name = minecraft::container_name(&spec.id);
    let options = CreateContainerOptionsBuilder::default().name(&name).build();
    let rcon_password = minecraft::rcon_password(&config.shared_secret, &spec.id);
    let mut body = minecraft::create_body(
        spec,
        image,
        &config.network,
        &config.data_root,
        &rcon_password,
    );
    if matches!(spec.server_type, cluster_shared::ServerType::Arclight) {
        // Pin the release tag that actually contains the selected jar. Newer
        // Minecraft versions are published as prereleases, which the image's
        // default `latest` lookup would skip.
        let tag = runtimes::arclight_release(&spec.game_version).await?;
        body.env
            .get_or_insert_with(Vec::new)
            .push(format!("ARCLIGHT_RELEASE={tag}"));
    }
    if matches!(spec.server_type, cluster_shared::ServerType::CatServer) {
        // itzg has no CatServer TYPE, so resolve the upstream jar and feed it to
        // the generic custom-jar launcher.
        let url = runtimes::cat_server_download(&spec.game_version).await?;
        body.env
            .get_or_insert_with(Vec::new)
            .push(format!("CUSTOM_SERVER={url}"));
    }
    if matches!(spec.server_type, cluster_shared::ServerType::Magma) {
        // itzg's Magma installer needs an explicit MAGMA_VERSION; its API only
        // exposes the supported Minecraft line (`1.21.x`).
        let version = runtimes::magma_version(&spec.game_version).await?;
        body.env
            .get_or_insert_with(Vec::new)
            .push(format!("MAGMA_VERSION={version}"));
    }

    match docker.create_container(Some(options), body).await {
        Ok(response) => {
            tracing::info!(
                server_id = %spec.id,
                container_id = %response.id,
                container = %name,
                behind_proxy = spec.is_behind_proxy,
                game_port = spec.game_port,
                "created server container"
            );
            Ok(response.id)
        }
        Err(error) if is_conflict(&error) => Err(DaemonError::Conflict(format!(
            "container {name} already exists"
        ))),
        Err(error) => Err(DaemonError::from(error)),
    }
}

/// Apply the latest RAM/CPU limits to an existing container.
///
/// Docker fixes most of HostConfig at create time, but the resource controls are
/// updatable. Calling this before a start makes post-creation scaling effective.
pub async fn apply_resources(
    docker: &Docker,
    server_id: &str,
    ram_mb: i64,
    cpu_limit: f64,
) -> Result<(), DaemonError> {
    let name = minecraft::container_name(server_id);
    let body = minecraft::resource_update(ram_mb, cpu_limit);
    docker
        .update_container(&name, body)
        .await
        .map_err(DaemonError::from)?;
    tracing::info!(server_id, container = %name, ram_mb, cpu_limit, "applied container resources");
    Ok(())
}

/// Start a container. Already-running containers are treated as success.
pub async fn start_server(docker: &Docker, server_id: &str) -> Result<(), DaemonError> {
    let name = minecraft::container_name(server_id);
    match docker.start_container(&name, None).await {
        Ok(()) => {
            tracing::info!(server_id, container = %name, "started server container");
            Ok(())
        }
        Err(error) if is_not_modified(&error) => Ok(()),
        Err(error) => Err(DaemonError::from(error)),
    }
}

/// Stop a container gracefully, then remove it.
/// True when the server's container exists and is running.
pub async fn is_running(docker: &Docker, server_id: &str) -> Result<bool, DaemonError> {
    let name = minecraft::container_name(server_id);
    match docker.inspect_container(&name, None).await {
        Ok(container) => Ok(container
            .state
            .and_then(|state| state.running)
            .unwrap_or(false)),
        Err(error) if is_not_found(&error) => Ok(false),
        Err(error) => Err(DaemonError::from(error)),
    }
}

pub async fn stop_server(docker: &Docker, server_id: &str) -> Result<(), DaemonError> {
    let name = minecraft::container_name(server_id);
    let options = StopContainerOptionsBuilder::default()
        .t(STOP_TIMEOUT_SECS)
        .build();

    match docker.stop_container(&name, Some(options)).await {
        Ok(()) => {
            tracing::info!(server_id, container = %name, "stopped server container");
            Ok(())
        }
        Err(error) if is_not_modified(&error) => Ok(()),
        Err(error) => Err(DaemonError::from(error)),
    }
}

/// Force-remove a container (and its writable layer). The bind-mounted `/data`
/// directory is intentionally kept; deletion of world data is a separate,
/// explicit action.
pub async fn remove_server(docker: &Docker, server_id: &str) -> Result<(), DaemonError> {
    let name = minecraft::container_name(server_id);
    let options = RemoveContainerOptionsBuilder::default()
        .force(true)
        .v(true)
        .build();

    match docker.remove_container(&name, Some(options)).await {
        Ok(()) => {
            tracing::info!(server_id, container = %name, "removed server container");
            Ok(())
        }
        Err(error) if is_not_found(&error) => {
            tracing::debug!(server_id, container = %name, "container already absent");
            Ok(())
        }
        Err(error) => Err(DaemonError::from(error)),
    }
}
