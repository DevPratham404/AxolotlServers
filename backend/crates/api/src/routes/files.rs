//! `/api/v1/servers/{id}/directory` and `/files` — the visual file manager.
//!
//! The daemon owns the volume sandbox. These handlers only forward already
//! validated relative paths; they never accept an absolute host path.

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bytes::Bytes;
use cluster_shared::{DirectoryEntry, FileCopyRequest, FileRenameRequest, FileWriteRequest};
use serde::Deserialize;

use crate::{error::ApiError, routes::servers, state::AppState};

/// Keep in sync with the daemon's raw upload cap.
const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/servers/{id}/directory", get(list_root))
        .route(
            "/servers/{id}/directory/{*path}",
            get(list_directory)
                .post(create_directory)
                .put(create_directory),
        )
        .route("/servers/{id}/files/rename", post(rename_file))
        .route("/servers/{id}/files/copy", post(copy_file))
        .route(
            "/servers/{id}/files/{*path}",
            get(get_file)
                // POST is the browser binary-upload path. PUT carries either a
                // raw body (programmatic binary writes) or a JSON text write;
                // the handler dispatches on Content-Type so both stay valid.
                .post(upload_file)
                .put(put_file)
                .delete(delete_file)
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
}

async fn list_root(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<DirectoryEntry>>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    let entries = state.daemon.list_directory(&server.id, "").await?;
    Ok(Json(entries))
}

async fn list_directory(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Json<Vec<DirectoryEntry>>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    let entries = state.daemon.list_directory(&server.id, &path).await?;
    Ok(Json(entries))
}

async fn create_directory(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    state.daemon.create_directory(&server.id, &path).await?;
    tracing::info!(server_id = %server.id, path = %path, "created server directory");
    Ok(StatusCode::CREATED)
}

#[derive(Debug, Deserialize)]
struct FileQuery {
    download: Option<bool>,
}

async fn get_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    Query(query): Query<FileQuery>,
) -> Result<Response, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;

    if query.download.unwrap_or(false) {
        // Return raw bytes with download headers.
        let bytes = state.daemon.read_raw_file(&server.id, &path).await?;
        let filename = download_filename(&path);
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            )
            .body(Body::from(bytes))
            .map_err(|error| ApiError::Internal(format!("build download response: {error}")))?;
        Ok(response)
    } else {
        // Return parsed text file as JSON.
        let file = state.daemon.read_file(&server.id, &path).await?;
        Ok(Json(file).into_response())
    }
}

async fn upload_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    state.daemon.upload_file(&server.id, &path, &body).await?;
    tracing::info!(server_id = %server.id, path = %path, bytes = %body.len(), "uploaded server file");
    Ok(StatusCode::OK)
}

/// `PUT` serves two clients: the editor sends JSON (`FileWriteRequest`) for a
/// conflict-checked text write, while binary clients send an octet stream.
async fn put_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;

    if is_json(&headers) {
        let request: FileWriteRequest = serde_json::from_slice(&body)
            .map_err(|error| ApiError::BadRequest(format!("invalid file write body: {error}")))?;
        let file = state
            .daemon
            .write_file(
                &server.id,
                &path,
                &request.content,
                request.expected_hash.as_deref(),
            )
            .await?;
        tracing::info!(server_id = %server.id, path = %path, "wrote server file");
        Ok(Json(file).into_response())
    } else {
        state.daemon.upload_file(&server.id, &path, &body).await?;
        tracing::info!(server_id = %server.id, path = %path, bytes = %body.len(), "uploaded server file");
        Ok(StatusCode::OK.into_response())
    }
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
}

async fn delete_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    state.daemon.delete_file(&server.id, &path).await?;
    tracing::info!(server_id = %server.id, path = %path, "deleted server file");
    Ok(StatusCode::NO_CONTENT)
}

async fn rename_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<FileRenameRequest>,
) -> Result<StatusCode, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    state
        .daemon
        .rename_file(&server.id, &request.old_path, &request.new_path)
        .await?;
    tracing::info!(
        server_id = %server.id,
        from = %request.old_path,
        to = %request.new_path,
        "renamed server path"
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn copy_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<FileCopyRequest>,
) -> Result<StatusCode, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    state
        .daemon
        .copy_file(&server.id, &request.source_path, &request.dest_path)
        .await?;
    tracing::info!(
        server_id = %server.id,
        from = %request.source_path,
        to = %request.dest_path,
        "copied server path"
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Build a header-safe download filename: no path separators, quotes, or
/// control characters that could break/inject the Content-Disposition header.
fn download_filename(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or("file");
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '"' | '\\' | '/') {
                '_'
            } else {
                character
            }
        })
        .collect();
    if cleaned.is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::download_filename;

    #[test]
    fn sanitizes_download_filenames() {
        assert_eq!(download_filename("config/paper.yml"), "paper.yml");
        assert_eq!(download_filename("weird\"name\n.txt"), "weird_name_.txt");
        assert_eq!(download_filename(""), "file");
    }
}
