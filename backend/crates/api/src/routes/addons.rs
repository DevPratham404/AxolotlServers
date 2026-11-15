//! Modrinth add-on browser: install `.jar`/`.zip` files into the right folder.
//!
//! The API resolves the destination from the server kind (and, for datapacks,
//! from `server.properties`) before forwarding a streamed download to the
//! daemon. The daemon still owns the path sandbox, so a bad directory can never
//! escape the server volume.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use cluster_shared::{AddonInstallRequest, AddonProjectType, InstalledAddon, ServerType};
use reqwest::Url;

use crate::{error::ApiError, properties, routes::servers, state::AppState};

/// Directories whose contents are shown as installed add-ons.
const INSTALLED_EXTENSIONS: [&str; 2] = ["jar", "zip"];
const GEYSER_PACKS_DIRECTORY: &str = "plugins/Geyser-Spigot/packs";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/servers/{id}/addons", get(list_addons))
        .route("/servers/{id}/addons/install", post(install_addon))
}

async fn install_addon(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<AddonInstallRequest>,
) -> Result<(StatusCode, Json<InstalledAddon>), ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    let url = validate_modrinth_url(&request.url)?;
    let filename = filename_from_url(&url)?;
    validate_extension(&filename, request.project_type)?;

    if request.project_type == AddonProjectType::Modpack {
        // Modpacks are Fabric/Forge only; the engine owns everything under mods/
        // plus the pack's overrides tree.
        install_directory(server.server_type, request.project_type, "world", false)?;
        let result = state
            .daemon
            .install_modpack(&server.id, url.as_str())
            .await?;
        tracing::info!(
            server_id = %server.id,
            name = %result.name,
            files = result.files,
            overrides = result.overrides,
            bytes = result.bytes,
            "installed modpack"
        );
        return Ok((
            StatusCode::CREATED,
            Json(InstalledAddon {
                name: result.name,
                path: "mods".into(),
                directory: "mods".into(),
                size: result.bytes,
                project_type: request.project_type,
            }),
        ));
    }

    let level_name = level_name(&state, &server.id).await?;
    let geyser = geyser_present(&state, &server.id).await?;
    let directory = install_directory(
        server.server_type,
        request.project_type,
        &level_name,
        geyser,
    )?;

    let result = state
        .daemon
        .download_addon(&server.id, url.as_str(), &filename, &directory)
        .await?;

    tracing::info!(
        server_id = %server.id,
        project_type = request.project_type.as_str(),
        url = %url,
        path = %result.path,
        bytes = result.bytes,
        "installed add-on"
    );

    Ok((
        StatusCode::CREATED,
        Json(InstalledAddon {
            name: filename,
            path: result.path,
            directory,
            size: result.bytes,
            project_type: request.project_type,
        }),
    ))
}

async fn list_addons(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<InstalledAddon>>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    let level_name = level_name(&state, &server.id).await?;
    let geyser = geyser_present(&state, &server.id).await?;

    let mut addons = Vec::new();
    for (directory, project_type) in addon_directories(server.server_type, &level_name, geyser) {
        let entries = match state.daemon.list_directory(&server.id, &directory).await {
            Ok(entries) => entries,
            Err(ApiError::NotFound(_)) => continue,
            Err(error) => return Err(error),
        };

        for entry in entries {
            if entry.is_dir || !is_addon_file(&entry.name) {
                continue;
            }
            addons.push(InstalledAddon {
                name: entry.name.clone(),
                path: format!("{directory}/{}", entry.name),
                directory: directory.clone(),
                size: entry.size,
                project_type,
            });
        }
    }

    addons.sort_by(|left, right| {
        left.directory
            .cmp(&right.directory)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(Json(addons))
}

/// Resolve the install folder for a project kind on a given server.
fn install_directory(
    server_type: ServerType,
    project_type: AddonProjectType,
    level_name: &str,
    geyser: bool,
) -> Result<String, ApiError> {
    match project_type {
        AddonProjectType::Mod => match server_type {
            ServerType::Fabric | ServerType::Forge => Ok("mods".into()),
            other => Err(ApiError::BadRequest(format!(
                "mods require a Fabric or Forge server, not {}",
                other.as_str()
            ))),
        },
        AddonProjectType::Plugin => match server_type {
            ServerType::Paper | ServerType::Velocity | ServerType::Bungeecord => {
                Ok("plugins".into())
            }
            other => Err(ApiError::BadRequest(format!(
                "plugins require a Paper, Velocity or BungeeCord server, not {}",
                other.as_str()
            ))),
        },
        AddonProjectType::Datapack => Ok(format!("{level_name}/datapacks")),
        AddonProjectType::Resourcepack => {
            if geyser {
                Ok(GEYSER_PACKS_DIRECTORY.into())
            } else {
                Ok("resourcepacks".into())
            }
        }
        AddonProjectType::Modpack => match server_type {
            ServerType::Fabric | ServerType::Forge => Ok("mods".into()),
            other => Err(ApiError::BadRequest(format!(
                "modpacks require a Fabric or Forge server, not {}",
                other.as_str()
            ))),
        },
    }
}

/// Every directory that can contain installed add-ons for this server.
fn addon_directories(
    server_type: ServerType,
    level_name: &str,
    geyser: bool,
) -> Vec<(String, AddonProjectType)> {
    let mut directories = Vec::new();
    if matches!(server_type, ServerType::Fabric | ServerType::Forge) {
        directories.push(("mods".to_string(), AddonProjectType::Mod));
    }
    if matches!(
        server_type,
        ServerType::Paper | ServerType::Velocity | ServerType::Bungeecord
    ) {
        directories.push(("plugins".to_string(), AddonProjectType::Plugin));
    }
    directories.push((
        format!("{level_name}/datapacks"),
        AddonProjectType::Datapack,
    ));
    directories.push((
        if geyser {
            GEYSER_PACKS_DIRECTORY.to_string()
        } else {
            "resourcepacks".to_string()
        },
        AddonProjectType::Resourcepack,
    ));
    directories
}

/// Read `level-name` from `server.properties`, defaulting to `world`.
async fn level_name(state: &AppState, server_id: &str) -> Result<String, ApiError> {
    let content = match state.daemon.read_file(server_id, "server.properties").await {
        Ok(file) => file.content,
        Err(ApiError::NotFound(_)) => return Ok("world".into()),
        Err(error) => return Err(error),
    };
    let entries = properties::parse(&content);
    let raw = properties::get(&entries, "level-name").unwrap_or("world");
    validate_level_name(raw)
}

fn validate_level_name(raw: &str) -> Result<String, ApiError> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok("world".into());
    }
    if value == "." || value == ".." || value.contains(['/', '\\', '\0']) {
        return Err(ApiError::BadRequest(
            "level-name must be a simple directory name".into(),
        ));
    }
    if value.chars().any(char::is_control) || value.chars().count() > 128 {
        return Err(ApiError::BadRequest("level-name is invalid".into()));
    }
    Ok(value.to_string())
}

async fn geyser_present(state: &AppState, server_id: &str) -> Result<bool, ApiError> {
    match state.daemon.list_directory(server_id, "plugins").await {
        Ok(entries) => Ok(entries
            .iter()
            .any(|entry| entry.is_dir && entry.name.eq_ignore_ascii_case("Geyser-Spigot"))),
        Err(ApiError::NotFound(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Only Modrinth-hosted URLs are accepted so installs cannot be used as a
/// generic download proxy.
fn validate_modrinth_url(raw: &str) -> Result<Url, ApiError> {
    let url = Url::parse(raw.trim())
        .map_err(|error| ApiError::BadRequest(format!("invalid add-on URL: {error}")))?;
    if url.scheme() != "https" {
        return Err(ApiError::BadRequest("add-on URL must use https".into()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::BadRequest("add-on URL has no host".into()))?;
    let host = host.to_ascii_lowercase();
    if host != "modrinth.com" && !host.ends_with(".modrinth.com") {
        return Err(ApiError::BadRequest(
            "add-on URL must point at Modrinth".into(),
        ));
    }
    Ok(url)
}

fn filename_from_url(url: &Url) -> Result<String, ApiError> {
    let name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ApiError::BadRequest("add-on URL has no filename".into()))?;

    if name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(ApiError::BadRequest(
            "add-on URL has an invalid filename".into(),
        ));
    }
    if name.chars().any(char::is_control) || name.chars().count() > 200 {
        return Err(ApiError::BadRequest(
            "add-on URL has an invalid filename".into(),
        ));
    }
    Ok(name.to_string())
}

fn validate_extension(filename: &str, project_type: AddonProjectType) -> Result<(), ApiError> {
    let extension = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    if extension != project_type.extension() {
        return Err(ApiError::BadRequest(format!(
            "{} add-ons must be .{} files",
            project_type.as_str(),
            project_type.extension()
        )));
    }
    Ok(())
}

fn is_addon_file(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, extension)| {
            INSTALLED_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_directories_per_server_kind() {
        assert_eq!(
            install_directory(ServerType::Fabric, AddonProjectType::Mod, "world", false)
                .expect("fabric mods"),
            "mods"
        );
        assert_eq!(
            install_directory(ServerType::Paper, AddonProjectType::Plugin, "world", false)
                .expect("paper plugins"),
            "plugins"
        );
        assert!(
            install_directory(ServerType::Paper, AddonProjectType::Mod, "world", false).is_err()
        );
        assert!(
            install_directory(ServerType::Fabric, AddonProjectType::Plugin, "world", false)
                .is_err()
        );
        assert_eq!(
            install_directory(
                ServerType::Vanilla,
                AddonProjectType::Datapack,
                "my world",
                false
            )
            .expect("datapacks"),
            "my world/datapacks"
        );
        assert_eq!(
            install_directory(
                ServerType::Paper,
                AddonProjectType::Resourcepack,
                "world",
                true
            )
            .expect("geyser packs"),
            GEYSER_PACKS_DIRECTORY
        );
        assert_eq!(
            install_directory(
                ServerType::Paper,
                AddonProjectType::Resourcepack,
                "world",
                false
            )
            .expect("vanilla packs"),
            "resourcepacks"
        );
        assert_eq!(
            install_directory(
                ServerType::Fabric,
                AddonProjectType::Modpack,
                "world",
                false
            )
            .expect("fabric modpack"),
            "mods"
        );
        assert!(
            install_directory(ServerType::Paper, AddonProjectType::Modpack, "world", false)
                .is_err()
        );
    }

    #[test]
    fn lists_relevant_addon_directories() {
        let paper = addon_directories(ServerType::Paper, "world", true);
        assert!(paper.iter().any(|(dir, _)| dir == "plugins"));
        assert!(paper.iter().all(|(dir, _)| dir != "mods"));
        assert!(paper.iter().any(|(dir, _)| dir == GEYSER_PACKS_DIRECTORY));

        let fabric = addon_directories(ServerType::Fabric, "world", false);
        assert!(fabric.iter().any(|(dir, _)| dir == "mods"));
        assert!(fabric.iter().all(|(dir, _)| dir != "plugins"));
    }

    #[test]
    fn validates_level_names() {
        assert_eq!(validate_level_name("world").expect("world"), "world");
        assert_eq!(validate_level_name("  ").expect("default"), "world");
        assert!(validate_level_name("../escape").is_err());
        assert!(validate_level_name("nested/world").is_err());
    }

    #[test]
    fn validates_modrinth_urls_and_filenames() {
        let url = validate_modrinth_url(
            "https://cdn.modrinth.com/data/AABB/versions/CCDD/example-mod-1.0.jar",
        )
        .expect("valid cdn URL");
        assert_eq!(
            filename_from_url(&url).expect("filename"),
            "example-mod-1.0.jar"
        );

        assert!(validate_modrinth_url("http://cdn.modrinth.com/x.jar").is_err());
        assert!(validate_modrinth_url("https://evil.example/x.jar").is_err());
        assert!(validate_extension("example-mod-1.0.jar", AddonProjectType::Mod).is_ok());
        assert!(validate_extension("example-mod-1.0.zip", AddonProjectType::Mod).is_err());
        assert!(validate_extension("example-pack.mrpack", AddonProjectType::Modpack).is_ok());
        assert!(validate_extension("example-pack.zip", AddonProjectType::Modpack).is_err());
        assert!(is_addon_file("pack.zip"));
        assert!(!is_addon_file("config.yml"));
    }
}
