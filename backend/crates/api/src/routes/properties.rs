//! `/api/v1/servers/{id}/properties` — structured access to `server.properties`.
//!
//! The daemon owns the file and enforces the volume sandbox; this layer parses
//! the Java properties format into ordered entries, redacts secrets, and
//! serialises the form back. Writes are optimistic: the hash from the last read
//! is sent along so a concurrent edit cannot be silently overwritten.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use cluster_shared::FileContent;
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    properties::{self, PropertyEntry},
    routes::servers,
    state::AppState,
};

const PROPERTIES_PATH: &str = "server.properties";

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/servers/{id}/properties",
        get(read_properties).put(write_properties),
    )
}

#[derive(Debug, Serialize)]
struct PropertiesResponse {
    path: String,
    hash: String,
    entries: Vec<PropertyEntry>,
    /// Keys whose values were withheld; the UI renders them as managed.
    redacted: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdatePropertiesRequest {
    entries: Vec<PropertyEntry>,
    #[serde(default)]
    expected_hash: Option<String>,
}

async fn read_properties(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PropertiesResponse>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    let file = state
        .daemon
        .read_file(&server.id, PROPERTIES_PATH)
        .await
        .map_err(map_missing)?;

    Ok(Json(present(file)))
}

async fn write_properties(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdatePropertiesRequest>,
) -> Result<Json<PropertiesResponse>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    properties::validate(&request.entries).map_err(ApiError::BadRequest)?;

    if let Some(hash) = &request.expected_hash {
        if !is_hash(hash) {
            return Err(ApiError::BadRequest(
                "expected_hash must be a sha256 hex digest".into(),
            ));
        }
    }

    // Restore secrets the browser never saw. If the file does not exist yet,
    // there is nothing to restore and the write creates it.
    let current = match state.daemon.read_file(&server.id, PROPERTIES_PATH).await {
        Ok(file) => properties::parse(&file.content),
        Err(ApiError::NotFound(_)) => Vec::new(),
        Err(error) => return Err(error),
    };

    let mut entries = request.entries;
    properties::restore_redacted(&mut entries, &current);

    let content = properties::serialise(&entries);
    let stored = state
        .daemon
        .write_file(
            &server.id,
            PROPERTIES_PATH,
            &content,
            request.expected_hash.as_deref(),
        )
        .await?;

    tracing::info!(server_id = %server.id, "updated server.properties");
    Ok(Json(present(stored)))
}

/// Convert a raw daemon file into the browser-facing shape.
fn present(file: FileContent) -> PropertiesResponse {
    let mut entries = properties::parse(&file.content);
    properties::redact(&mut entries);

    let redacted = properties::REDACTED_KEYS
        .iter()
        .filter(|key| entries.iter().any(|entry| entry.key() == Some(**key)))
        .map(|key| (*key).to_string())
        .collect();

    PropertiesResponse {
        path: file.path,
        hash: file.hash,
        entries,
        redacted,
    }
}

fn map_missing(error: ApiError) -> ApiError {
    match error {
        ApiError::NotFound(_) => ApiError::NotFound(
            "server.properties does not exist yet — start the server once to generate it".into(),
        ),
        other => other,
    }
}

fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_hashes() {
        assert!(is_hash(&"a".repeat(64)));
        assert!(!is_hash("abc"));
        assert!(!is_hash(&"z".repeat(64)));
    }

    #[test]
    fn presents_redacted_properties() {
        let file = FileContent {
            path: "server.properties".into(),
            content: "motd=hello\nrcon.password=secret\n".into(),
            hash: "deadbeef".into(),
        };
        let response = present(file);
        assert_eq!(response.redacted, vec!["rcon.password".to_string()]);
        assert_eq!(
            properties::get(&response.entries, "rcon.password"),
            Some("")
        );
        assert_eq!(properties::get(&response.entries, "motd"), Some("hello"));
    }
}
