//! Runtime-specific Minecraft version discovery.
//!
//! The vanilla manifest is the only source that lists every release, but not
//! every server project publishes a build for every release. Hybrids such as
//! Arclight, CatServer, Mohist and Magma lag behind, so offering them the
//! Mojang "LATEST" alias produces containers that fail during download.
//!
//! This endpoint resolves the versions each upstream project actually supports.
//! It is best-effort: upstream outages fall back to a small curated list so the
//! create/import wizards always have something valid to offer.

use std::collections::BTreeSet;
use std::str::FromStr;
use std::time::Duration;

use axum::{Json, Router, extract::Path, routing::get};
use cluster_shared::ServerType;
use serde::Serialize;
use serde_json::Value;

use crate::error::ApiError;

const MOJANG_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const ARCLIGHT_RELEASES: &str =
    "https://api.github.com/repos/IzzelAliz/Arclight/releases?per_page=30";
const CATSERVER_RELEASES: &str =
    "https://api.github.com/repos/Luohuayu/CatServer/releases?per_page=30";
const MOHIST_VERSIONS: &str = "https://api.mohistmc.com/project/mohist/versions";
const MAGMA_VERSIONS: &str = "https://magmafoundation.org/api/versions";

pub fn router() -> Router<crate::state::AppState> {
    Router::new().route("/runtimes/{server_type}/versions", get(versions))
}

#[derive(Debug, Serialize)]
struct RuntimeVersions {
    versions: Vec<String>,
}

async fn versions(Path(server_type): Path<String>) -> Result<Json<RuntimeVersions>, ApiError> {
    let server_type = ServerType::from_str(&server_type)
        .map_err(|_| ApiError::BadRequest(format!("unknown server type: {server_type}")))?;

    Ok(Json(RuntimeVersions {
        versions: supported_versions(server_type).await,
    }))
}

async fn supported_versions(server_type: ServerType) -> Vec<String> {
    let upstream = match server_type {
        ServerType::Arclight => fetch_arclight_versions().await,
        ServerType::CatServer => fetch_catserver_versions().await,
        ServerType::Mohist => fetch_mohist_versions().await,
        ServerType::Magma => fetch_magma_versions().await,
        _ => fetch_mojang_versions().await,
    };

    match upstream {
        Ok(versions) if !versions.is_empty() => versions,
        Ok(_) => fallback_versions(server_type),
        Err(error) => {
            tracing::warn!(
                server_type = server_type.as_str(),
                %error,
                "runtime version discovery failed; using fallback list"
            );
            fallback_versions(server_type)
        }
    }
}

async fn fetch_json(url: &str) -> Result<Value, reqwest::Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("cluster-console/1.0")
        .build()?;
    client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

async fn fetch_mojang_versions() -> Result<Vec<String>, reqwest::Error> {
    let manifest = fetch_json(MOJANG_MANIFEST).await?;
    let releases = manifest
        .get("versions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("release"))
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .take(40)
        .map(str::to_string)
        .collect::<Vec<_>>();

    let mut versions = vec!["LATEST".to_string()];
    versions.extend(releases);
    Ok(versions)
}

async fn fetch_arclight_versions() -> Result<Vec<String>, reqwest::Error> {
    let releases = fetch_json(ARCLIGHT_RELEASES).await?;
    let mut versions = BTreeSet::new();

    // The image is told the exact release tag at create time, so versions from
    // older/prerelease tags are installable. Only NeoForge assets count because
    // the container always runs `ARCLIGHT_TYPE=NEOFORGE`.
    for asset in release_assets(&releases) {
        let Some(rest) = asset.strip_prefix("arclight-neoforge-") else {
            continue;
        };
        let candidate = rest.split('-').next().unwrap_or_default();
        if is_version_like(candidate) {
            versions.insert(candidate.to_string());
        }
    }

    Ok(sorted_desc(versions))
}

async fn fetch_catserver_versions() -> Result<Vec<String>, reqwest::Error> {
    let releases = fetch_json(CATSERVER_RELEASES).await?;
    let mut versions = BTreeSet::new();

    if let Some(entries) = releases.as_array() {
        for release in entries {
            if let Some(name) = release.get("name").and_then(Value::as_str) {
                if let Some((_, rest)) = name.split_once('(') {
                    let candidate = rest.split(')').next().unwrap_or_default().trim();
                    if is_version_like(candidate) {
                        versions.insert(candidate.to_string());
                    }
                }
            }
        }
    }

    for asset in release_assets(&releases) {
        if let Some(rest) = asset.strip_prefix("CatServer-") {
            let candidate = rest.split('-').next().unwrap_or_default();
            if is_version_like(candidate) {
                versions.insert(candidate.to_string());
            }
        }
    }

    Ok(sorted_desc(versions))
}

async fn fetch_mohist_versions() -> Result<Vec<String>, reqwest::Error> {
    let versions = fetch_json(MOHIST_VERSIONS).await?;
    let mut found = BTreeSet::new();
    for entry in versions.as_array().into_iter().flatten() {
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            if is_version_like(name) {
                found.insert(name.to_string());
            }
        }
    }
    Ok(sorted_desc(found))
}

async fn fetch_magma_versions() -> Result<Vec<String>, reqwest::Error> {
    let response = fetch_json(MAGMA_VERSIONS).await?;
    let mut found = BTreeSet::new();
    for entry in response
        .get("versions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(version) = entry.get("minecraftVersion").and_then(Value::as_str) {
            // Magma labels its supported line as `1.21.x`. The itzg launcher
            // still needs a concrete version for `resolveVersion`, but the
            // daemon pins the exact Magma build via `MAGMA_VERSION`.
            if let Some(base) = version.strip_suffix(".x") {
                found.insert(format!("{base}.1"));
            } else if is_version_like(version) {
                found.insert(version.to_string());
            }
        }
    }
    Ok(sorted_desc(found))
}

fn release_assets(releases: &Value) -> Vec<&str> {
    releases
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|release| release.get("assets").and_then(Value::as_array))
        .flatten()
        .filter_map(|asset| asset.get("name").and_then(Value::as_str))
        .collect()
}

fn is_version_like(value: &str) -> bool {
    !value.is_empty()
        && value.contains('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        && value != "."
}

/// Newest first. Non-numeric suffixes such as Magma's `1.21.x` sort below the
/// concrete release they describe.
fn sorted_desc(versions: BTreeSet<String>) -> Vec<String> {
    let mut versions = versions.into_iter().collect::<Vec<_>>();
    versions.sort_by(|left, right| {
        version_key(right)
            .cmp(&version_key(left))
            .then_with(|| right.cmp(left))
    });
    versions
}

fn version_key(value: &str) -> Vec<u64> {
    value
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn fallback_versions(server_type: ServerType) -> Vec<String> {
    match server_type {
        ServerType::Arclight => vec!["26.1.2".into()],
        ServerType::CatServer => vec!["1.18.2".into(), "1.16.5".into(), "1.12.2".into()],
        ServerType::Mohist => vec![
            "1.21.4".into(),
            "1.21.1".into(),
            "1.20.1".into(),
            "1.19.4".into(),
            "1.18.2".into(),
            "1.16.5".into(),
            "1.12.2".into(),
        ],
        ServerType::Magma => vec!["1.21.1".into()],
        _ => vec!["LATEST".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_arclight_versions_from_asset_names() {
        let releases = serde_json::json!([
            {"assets": [
                {"name": "arclight-neoforge-26.1.2-1.1.0-SNAPSHOT-9e3febe.jar"},
                {"name": "arclight-forge-1.21.1-1.0.1-8ec9529.jar"}
            ]},
            {"assets": [{"name": "arclight-neoforge-1.20.4-1.0.4-80ec5df.jar"}]}
        ]);
        let mut versions = BTreeSet::new();
        for asset in release_assets(&releases) {
            // Only NeoForge assets are installable by this product.
            let Some(rest) = asset.strip_prefix("arclight-neoforge-") else {
                continue;
            };
            versions.insert(rest.split('-').next().expect("version").to_string());
        }
        assert_eq!(sorted_desc(versions), vec!["26.1.2", "1.20.4"]);
    }

    #[test]
    fn rejects_non_versions_and_orders_numerically() {
        assert!(is_version_like("1.21.1"));
        assert!(is_version_like("26.1.2"));
        assert!(!is_version_like("universal"));
        assert!(!is_version_like("1.21.x"));
        let versions = BTreeSet::from([
            "1.9.4".to_string(),
            "1.21.11".to_string(),
            "1.21.1".to_string(),
        ]);
        assert_eq!(sorted_desc(versions), vec!["1.21.11", "1.21.1", "1.9.4"]);
    }

    #[test]
    fn every_pinned_runtime_has_a_fallback() {
        for server_type in ServerType::ALL {
            let versions = fallback_versions(server_type);
            assert!(!versions.is_empty());
            if server_type.requires_explicit_game_version() {
                assert!(
                    versions.iter().all(|version| version != "LATEST"),
                    "{} must not fall back to LATEST",
                    server_type.as_str()
                );
            }
        }
    }
}
