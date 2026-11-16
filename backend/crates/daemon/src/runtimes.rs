//! Small runtime installers that the itzg image cannot resolve by itself.
//!
//! CatServer has no dedicated `TYPE` in itzg. Its GitHub releases publish a
//! ready-to-run jar, so the daemon resolves the matching asset and hands it to
//! `TYPE=CUSTOM` via `CUSTOM_SERVER`.

use std::time::Duration;

use serde_json::Value;

use crate::error::DaemonError;

const ARCLIGHT_RELEASES: &str =
    "https://api.github.com/repos/IzzelAliz/Arclight/releases?per_page=100";
const CATSERVER_RELEASES: &str =
    "https://api.github.com/repos/Luohuayu/CatServer/releases?per_page=30";
const MAGMA_VERSIONS: &str = "https://magmafoundation.org/api/versions";

async fn fetch_json(url: &str, what: &str) -> Result<Value, DaemonError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("cluster-console/1.0")
        .build()
        .map_err(|error| DaemonError::Internal(format!("build {what} client: {error}")))?;

    client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| DaemonError::Unavailable(format!("resolve {what}: {error}")))?
        .error_for_status()
        .map_err(|error| DaemonError::Unavailable(format!("resolve {what}: {error}")))?
        .json()
        .await
        .map_err(|error| DaemonError::Unavailable(format!("decode {what}: {error}")))
}

/// Resolve the Arclight GitHub release tag that contains a NeoForge jar for a
/// specific Minecraft version.
///
/// The image defaults `ARCLIGHT_RELEASE=latest`, which points at GitHub's latest
/// *stable* release. Arclight publishes newer Minecraft versions as prereleases,
/// so leaving the default in place makes the installer look for the requested
/// jar in the wrong tag. Pinning the tag keeps the advertised version and the
/// downloaded asset in sync.
pub async fn arclight_release(game_version: &str) -> Result<String, DaemonError> {
    let releases = fetch_json(ARCLIGHT_RELEASES, "Arclight releases").await?;

    let requested = game_version.trim();
    let prefix = format!("arclight-neoforge-{requested}-");

    for release in releases.as_array().into_iter().flatten() {
        let Some(tag) = release.get("tag_name").and_then(Value::as_str) else {
            continue;
        };
        let matches = release
            .get("assets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|asset| asset.get("name").and_then(Value::as_str))
            .any(|name| name.starts_with(&prefix) && name.ends_with(".jar"));
        if matches {
            tracing::info!(
                game_version = requested,
                release = tag,
                "resolved Arclight release"
            );
            return Ok(tag.to_string());
        }
    }

    Err(DaemonError::BadRequest(format!(
        "Arclight has no NeoForge release for Minecraft {requested}"
    )))
}

/// Resolve the CatServer release jar for a specific Minecraft version.
pub async fn cat_server_download(game_version: &str) -> Result<String, DaemonError> {
    let releases = fetch_json(CATSERVER_RELEASES, "CatServer releases").await?;

    let requested = game_version.trim();
    let mut latest: Option<(Vec<u64>, String)> = None;

    for release in releases.as_array().into_iter().flatten() {
        let release_version = release
            .get("name")
            .and_then(Value::as_str)
            .and_then(|name| name.split_once('('))
            .and_then(|(_, rest)| rest.split(')').next())
            .map(str::trim)
            .filter(|value| is_version_like(value));

        for asset in release
            .get("assets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(name) = asset.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !name.ends_with(".jar") {
                continue;
            }
            let Some(url) = asset.get("browser_download_url").and_then(Value::as_str) else {
                continue;
            };

            let asset_version = name
                .strip_prefix("CatServer-")
                .and_then(|rest| rest.split('-').next())
                .filter(|value| is_version_like(value));
            let Some(version) = asset_version.or(release_version) else {
                continue;
            };

            if version == requested {
                tracing::info!(version, %url, "resolved CatServer release jar");
                return Ok(url.to_string());
            }

            let key = version_key(version);
            if latest.as_ref().is_none_or(|(current, _)| key > *current) {
                latest = Some((key, url.to_string()));
            }
        }
    }

    if requested.eq_ignore_ascii_case("LATEST") {
        if let Some((_, url)) = latest {
            return Ok(url);
        }
    }

    Err(DaemonError::BadRequest(format!(
        "CatServer has no release jar for Minecraft {requested}"
    )))
}

/// Resolve a Magma build version for a concrete Minecraft version.
///
/// Magma's API describes its supported line as `1.21.x` while the itzg launcher
/// requires a resolvable `VERSION` and a non-empty `MAGMA_VERSION`.
pub async fn magma_version(minecraft_version: &str) -> Result<String, DaemonError> {
    let response = fetch_json(MAGMA_VERSIONS, "Magma builds").await?;

    let requested = version_base(minecraft_version);
    for entry in response
        .get("versions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(line) = entry.get("minecraftVersion").and_then(Value::as_str) else {
            continue;
        };
        if version_base(&line.replace(".x", ".1")) != requested {
            continue;
        }
        if let Some(version) = entry.get("version").and_then(Value::as_str) {
            tracing::info!(
                minecraft_version,
                magma_version = version,
                "resolved Magma build"
            );
            return Ok(version.to_string());
        }
    }

    Err(DaemonError::BadRequest(format!(
        "Magma has no build for Minecraft {minecraft_version}"
    )))
}

fn version_base(value: &str) -> Vec<u64> {
    let mut key = version_key(value);
    key.truncate(2);
    key
}

fn is_version_like(value: &str) -> bool {
    !value.is_empty()
        && value.contains('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
}

fn version_key(value: &str) -> Vec<u64> {
    value
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_release_versions() {
        assert!(is_version_like("1.18.2"));
        assert!(!is_version_like("universal"));
        assert!(!is_version_like(""));
        assert!(version_key("1.18.2") > version_key("1.16.5"));
        assert_eq!(version_base("1.21.1"), version_base("1.21.x"));
        assert_ne!(version_base("1.21.1"), version_base("1.20.1"));
    }
}
