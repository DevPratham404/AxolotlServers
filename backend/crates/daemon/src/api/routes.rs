//! `/daemon/servers/*` — signed, idempotent container operations.

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get, post, put},
};
use bollard::query_parameters::{LogsOptionsBuilder, StatsOptionsBuilder};
use bytes::Bytes;
use cluster_shared::{
    AddonDownloadRequest, AddonDownloadResult, CreateServerSpec, DirectoryEntry, FileContent,
    FileCopyRequest, FileRenameRequest, FileWriteRequest, ImportServerResult,
    ModpackInstallRequest, ModpackInstallResult, PlayerAccessLists, PlayerAccessUpdate,
    PlayerHistoryEntry, RconCommandRequest, RconCommandResponse, ServerResources,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    api::DaemonState, docker, error::DaemonError, files, imports, logs, minecraft, modpacks,
    players, rcon, stats,
};

pub fn router() -> Router<DaemonState> {
    Router::new()
        .route(
            "/daemon/servers/import",
            post(import_server).layer(DefaultBodyLimit::max(imports::MAX_IMPORT_BYTES)),
        )
        .route("/daemon/servers/{id}/create", post(create))
        .route("/daemon/servers/{id}/start", post(start))
        .route("/daemon/servers/{id}/stop", post(stop))
        .route("/daemon/servers/{id}/restart", post(restart))
        .route("/daemon/servers/{id}/command", post(command))
        .route("/daemon/servers/{id}/players", get(get_players))
        .route(
            "/daemon/servers/{id}/players/history",
            get(get_player_history),
        )
        .route(
            "/daemon/servers/{id}/players/access",
            get(get_player_access),
        )
        .route(
            "/daemon/servers/{id}/players/{username}/access",
            put(update_player_access),
        )
        .route("/daemon/servers/{id}/stats", get(stats_stream))
        .route("/daemon/servers/{id}/logs", get(logs_stream))
        .route("/daemon/servers/{id}/files/rename", post(rename_file))
        .route("/daemon/servers/{id}/files/copy", post(copy_file))
        .route("/daemon/servers/{id}/downloads", post(download_addon))
        .route(
            "/daemon/servers/{id}/modpacks/install",
            post(install_modpack),
        )
        .route(
            "/daemon/servers/{id}/files/{*path}",
            get(read_file).put(write_file).delete(delete_file),
        )
        .route("/daemon/servers/{id}/directory", get(list_directory_root))
        .route(
            "/daemon/servers/{id}/directory/{*path}",
            get(list_directory)
                .post(create_directory)
                .put(create_directory),
        )
        .route(
            "/daemon/servers/{id}/raw-files/{*path}",
            get(read_raw_file)
                .post(write_raw_file)
                .put(write_raw_file)
                .layer(DefaultBodyLimit::max(files::MAX_UPLOAD_BYTES)),
        )
        .route("/daemon/servers/{id}", delete(remove))
}

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Import existing server data from either a streamed ZIP part or an absolute
/// host path. Exactly one source is required. The destination volume must be
/// empty; the API creates and owns the container before calling this route.
async fn import_server(
    State(state): State<DaemonState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ImportServerResult>), DaemonError> {
    let mut server_id: Option<String> = None;
    let mut local_path: Option<String> = None;
    let mut upload = TempUpload::default();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| DaemonError::BadRequest(format!("invalid import form: {error}")))?
    {
        let field_name = field.name().map(ToString::to_string).unwrap_or_default();
        match field_name.as_str() {
            "server_id" => {
                let value = field
                    .text()
                    .await
                    .map_err(|error| DaemonError::BadRequest(format!("read server_id: {error}")))?;
                server_id = Some(value.trim().to_string());
            }
            "local_path" => {
                let value = field.text().await.map_err(|error| {
                    DaemonError::BadRequest(format!("read local_path: {error}"))
                })?;
                local_path = Some(value);
            }
            "file" => {
                if local_path.is_some() || upload.path.is_some() {
                    return Err(DaemonError::BadRequest(
                        "provide exactly one import source: file or local_path".into(),
                    ));
                }
                let directory = std::path::Path::new(&state.config.data_root).join(".imports");
                tokio::fs::create_dir_all(&directory)
                    .await
                    .map_err(import_io("create import staging directory"))?;
                let path = directory.join(format!("{}-{}.zip", uuid::Uuid::now_v7(), "upload"));
                let mut file = tokio::fs::File::create(&path)
                    .await
                    .map_err(import_io("create import archive"))?;
                let mut writer = tokio::io::BufWriter::new(&mut file);
                let mut total = 0usize;

                let write_result = async {
                    while let Some(chunk) = field.chunk().await.map_err(|error| {
                        DaemonError::BadRequest(format!("read import archive: {error}"))
                    })? {
                        total = total.saturating_add(chunk.len());
                        if total > imports::MAX_IMPORT_BYTES {
                            return Err(DaemonError::BadRequest(format!(
                                "import archive exceeds the {} byte limit",
                                imports::MAX_IMPORT_BYTES
                            )));
                        }
                        writer
                            .write_all(&chunk)
                            .await
                            .map_err(import_io("write import archive"))?;
                    }
                    writer
                        .flush()
                        .await
                        .map_err(import_io("flush import archive"))?;
                    Ok::<(), DaemonError>(())
                }
                .await;

                if let Err(error) = write_result {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(error);
                }
                if total == 0 {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(DaemonError::BadRequest(
                        "uploaded import archive is empty".into(),
                    ));
                }
                upload.path = Some(path);
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let server_id = server_id
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DaemonError::BadRequest("server_id is required".into()))?;
    validate_id(&server_id)?;

    let result = match (upload.path.as_deref(), local_path) {
        (Some(path), None) => {
            imports::import_zip(&state.config.data_root, &server_id, path).await?
        }
        (None, Some(path)) => {
            imports::import_local(&state.config.data_root, &server_id, &path).await?
        }
        (Some(_), Some(_)) => {
            return Err(DaemonError::BadRequest(
                "provide exactly one import source: file or local_path".into(),
            ));
        }
        (None, None) => {
            return Err(DaemonError::BadRequest(
                "provide an import source: file or local_path".into(),
            ));
        }
    };

    tracing::info!(
        server_id = %server_id,
        files = result.files,
        bytes = result.bytes,
        "imported server data"
    );
    Ok((StatusCode::CREATED, Json(result)))
}

#[derive(Default)]
struct TempUpload {
    path: Option<PathBuf>,
}

impl Drop for TempUpload {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn import_io(context: &'static str) -> impl FnOnce(std::io::Error) -> DaemonError + Copy {
    move |error| DaemonError::Internal(format!("{context}: {error}"))
}

async fn create(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(spec): Json<CreateServerSpec>,
) -> Result<(StatusCode, Json<Value>), DaemonError> {
    validate_id(&id)?;
    if spec.id != id {
        return Err(DaemonError::BadRequest(
            "path id does not match the payload id".into(),
        ));
    }
    validate_spec(&spec)?;

    let container_id = docker::create_server(&state.docker, &state.config, &spec).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": spec.id, "container_id": container_id })),
    ))
}

async fn start(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    payload: Option<Json<ServerResources>>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    if let Some(Json(resources)) = payload {
        apply_resources(&state, &id, &resources).await?;
    }
    docker::start_server(&state.docker, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn stop(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    docker::stop_server(&state.docker, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restart(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    payload: Option<Json<ServerResources>>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    docker::stop_server(&state.docker, &id).await?;
    if let Some(Json(resources)) = payload {
        // Applied while stopped so shrinking a limit cannot trip over live usage.
        apply_resources(&state, &id, &resources).await?;
    }
    docker::start_server(&state.docker, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Apply scaling limits supplied by the control plane (the daemon cannot read
/// SQLite, so the API carries the latest values on every start/restart).
async fn apply_resources(
    state: &DaemonState,
    id: &str,
    resources: &ServerResources,
) -> Result<(), DaemonError> {
    validate_resources(resources)?;
    docker::apply_resources(&state.docker, id, resources.ram_mb, resources.cpu_limit).await
}

async fn get_players(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<String>>, DaemonError> {
    validate_id(&id)?;
    let credentials = rcon::credentials(&state.docker, &state.config, &id).await?;
    let output = rcon::execute(
        &credentials.host,
        credentials.port,
        &credentials.password,
        "list",
    )
    .await?;

    let player_list = players::parse_player_list(&output);
    Ok(Json(player_list))
}

/// Players recorded in `usercache.json`, available even while the server is off.
async fn get_player_history(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<PlayerHistoryEntry>>, DaemonError> {
    validate_id(&id)?;
    let history = files::read_player_history(&state.config.data_root, &id).await?;
    Ok(Json(history))
}

async fn get_player_access(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<PlayerAccessLists>, DaemonError> {
    validate_id(&id)?;
    let lists = files::read_access_lists(&state.config.data_root, &id).await?;
    Ok(Json(lists))
}

/// Toggle operator/whitelist/ban state. A running server is driven through
/// RCON; otherwise the vanilla JSON lists are edited directly.
async fn update_player_access(
    State(state): State<DaemonState>,
    Path((id, username)): Path<(String, String)>,
    Json(update): Json<PlayerAccessUpdate>,
) -> Result<Json<PlayerAccessLists>, DaemonError> {
    validate_id(&id)?;
    let username = validate_username(&username)?;

    let running = docker::is_running(&state.docker, &id).await?;
    if running {
        let credentials = rcon::credentials(&state.docker, &state.config, &id).await?;
        for command in access_commands(&username, update) {
            rcon::execute(
                &credentials.host,
                credentials.port,
                &credentials.password,
                &command,
            )
            .await?;
        }
    } else {
        files::update_player_access_offline(&state.config.data_root, &id, &username, update)
            .await?;
    }

    let lists = files::read_access_lists(&state.config.data_root, &id).await?;
    tracing::info!(server_id = %id, username = %username, online = running, "updated player access");
    Ok(Json(lists))
}

fn access_commands(username: &str, update: PlayerAccessUpdate) -> Vec<String> {
    let mut commands = Vec::new();
    if let Some(enabled) = update.operator {
        commands.push(format!(
            "{} {username}",
            if enabled { "op" } else { "deop" }
        ));
    }
    if let Some(enabled) = update.whitelisted {
        commands.push(format!(
            "whitelist {} {username}",
            if enabled { "add" } else { "remove" }
        ));
    }
    if let Some(enabled) = update.banned {
        commands.push(format!(
            "{} {username}",
            if enabled { "ban" } else { "pardon" }
        ));
    }
    commands
}

async fn remove(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    docker::remove_server(&state.docker, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Read a text file from the server's sandboxed `/data` volume.
async fn read_file(
    State(state): State<DaemonState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Json<FileContent>, DaemonError> {
    validate_id(&id)?;
    let file = files::read_text(&state.config.data_root, &id, &path).await?;
    Ok(Json(file))
}

/// Atomically replace a text file in the server's sandboxed `/data` volume.
async fn write_file(
    State(state): State<DaemonState>,
    Path((id, path)): Path<(String, String)>,
    Json(request): Json<FileWriteRequest>,
) -> Result<Json<FileContent>, DaemonError> {
    validate_id(&id)?;
    let file = files::write_text(&state.config.data_root, &id, &path, &request).await?;
    tracing::info!(server_id = %id, path = %file.path, "wrote server file");
    Ok(Json(file))
}

/// List the server data root.
async fn list_directory_root(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<DirectoryEntry>>, DaemonError> {
    validate_id(&id)?;
    let entries = files::list_directory(&state.config.data_root, &id, "").await?;
    Ok(Json(entries))
}

/// List a subdirectory inside the server's sandboxed `/data` volume.
async fn list_directory(
    State(state): State<DaemonState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Json<Vec<DirectoryEntry>>, DaemonError> {
    validate_id(&id)?;
    let entries = files::list_directory(&state.config.data_root, &id, &path).await?;
    Ok(Json(entries))
}

/// Create a directory inside the server's sandboxed `/data` volume.
async fn create_directory(
    State(state): State<DaemonState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    files::create_directory(&state.config.data_root, &id, &path).await?;
    Ok(StatusCode::CREATED)
}

/// Read raw file bytes (for downloading or binary inspection).
async fn read_raw_file(
    State(state): State<DaemonState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response, DaemonError> {
    validate_id(&id)?;
    let (bytes, _) = files::read_raw(&state.config.data_root, &id, &path).await?;
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes))
        .map_err(|e| DaemonError::Internal(format!("build response: {e}")))?;
    Ok(resp)
}

/// Write raw file bytes (binary upload).
async fn write_raw_file(
    State(state): State<DaemonState>,
    Path((id, path)): Path<(String, String)>,
    body: Bytes,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    files::write_raw(&state.config.data_root, &id, &path, &body).await?;
    Ok(StatusCode::OK)
}

/// Delete a regular file in the server's sandboxed `/data` volume.
async fn delete_file(
    State(state): State<DaemonState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    files::delete_file(&state.config.data_root, &id, &path).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Rename or move a file/directory inside the sandbox.
async fn rename_file(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(request): Json<FileRenameRequest>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    files::rename_path(
        &state.config.data_root,
        &id,
        &request.old_path,
        &request.new_path,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Copy a file or directory inside the sandbox.
async fn copy_file(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(request): Json<FileCopyRequest>,
) -> Result<StatusCode, DaemonError> {
    validate_id(&id)?;
    files::copy_path(
        &state.config.data_root,
        &id,
        &request.source_path,
        &request.dest_path,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stream a remote add-on (Modrinth jar/zip) into the sandboxed `/data` volume.
async fn download_addon(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(request): Json<AddonDownloadRequest>,
) -> Result<Json<AddonDownloadResult>, DaemonError> {
    validate_id(&id)?;
    let (path, bytes) = files::download_addon(
        &state.config.data_root,
        &id,
        &request.url,
        &request.filename,
        &request.target_directory,
    )
    .await?;
    Ok(Json(AddonDownloadResult { path, bytes }))
}

/// Install a Modrinth `.mrpack`: download it, download its server-side files,
/// then apply `overrides/` on top of the server root.
async fn install_modpack(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(request): Json<ModpackInstallRequest>,
) -> Result<Json<ModpackInstallResult>, DaemonError> {
    validate_id(&id)?;
    let result = modpacks::install_modpack(&state.config.data_root, &id, &request.url).await?;
    Ok(Json(result))
}

/// Run a raw Minecraft command over RCON (control-plane initiated).
async fn command(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(request): Json<RconCommandRequest>,
) -> Result<Json<RconCommandResponse>, DaemonError> {
    validate_id(&id)?;
    let command = validate_command(&request.command)?;

    let credentials = rcon::credentials(&state.docker, &state.config, &id).await?;
    let output = rcon::execute(
        &credentials.host,
        credentials.port,
        &credentials.password,
        &command,
    )
    .await?;

    tracing::info!(server_id = %id, command = %command, "executed rcon command");
    Ok(Json(RconCommandResponse { output }))
}

/// Stream container resource samples as newline-delimited JSON.
async fn stats_stream(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Response, DaemonError> {
    validate_id(&id)?;
    let name = minecraft::container_name(&id);
    let docker = state.docker.clone();
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    tokio::spawn(async move {
        let options = StatsOptionsBuilder::default()
            .stream(true)
            .one_shot(false)
            .build();
        let mut stream = docker.stats(&name, Some(options));

        while let Some(item) = stream.next().await {
            match item {
                Ok(frame) => {
                    let Some(sample) = stats::from_docker(&frame) else {
                        continue;
                    };
                    if !send_json_line(&tx, &sample).await {
                        return;
                    }
                }
                Err(error) => {
                    tracing::warn!(server_id = %id, %error, "container stats stream ended");
                    break;
                }
            }
        }
    });

    Ok(ndjson_response(rx))
}

/// Stream Docker log lines (with timestamps) as newline-delimited JSON.
async fn logs_stream(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Response, DaemonError> {
    validate_id(&id)?;
    let name = minecraft::container_name(&id);
    let docker = state.docker.clone();
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(256);

    tokio::spawn(async move {
        let options = LogsOptionsBuilder::default()
            .follow(true)
            .stdout(true)
            .stderr(true)
            .timestamps(true)
            .tail("200")
            .build();
        let mut stream = docker.logs(&name, Some(options));

        while let Some(item) = stream.next().await {
            match item {
                Ok(output) => {
                    for line in logs::lines_from_output(output) {
                        if !send_json_line(&tx, &line).await {
                            return;
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(server_id = %id, %error, "container log stream ended");
                    break;
                }
            }
        }
    });

    Ok(ndjson_response(rx))
}

async fn send_json_line<T: serde::Serialize>(
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
    value: &T,
) -> bool {
    let mut encoded = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::error!(%error, "failed to serialize telemetry line");
            return true;
        }
    };
    encoded.push(b'\n');

    // A closed receiver means the API disconnected; stop polling Docker.
    tx.send(Ok(Bytes::from(encoded))).await.is_ok()
}

fn ndjson_response(rx: mpsc::Receiver<Result<Bytes, std::io::Error>>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(ReceiverStream::new(rx)))
        .unwrap_or_else(|error| {
            tracing::error!(%error, "failed to build telemetry response");
            Response::new(Body::empty())
        })
}

fn validate_id(id: &str) -> Result<(), DaemonError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| DaemonError::BadRequest("server id must be a UUID".into()))
}

const MAX_USERNAME_CHARS: usize = 16;

fn validate_username(raw: &str) -> Result<String, DaemonError> {
    let username = raw.trim();
    let length = username.chars().count();
    if length == 0 || length > MAX_USERNAME_CHARS {
        return Err(DaemonError::BadRequest(format!(
            "username must be between 1 and {MAX_USERNAME_CHARS} characters"
        )));
    }
    if !username
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(DaemonError::BadRequest(
            "username may only contain letters, digits and underscores".into(),
        ));
    }
    Ok(username.to_string())
}

const MAX_COMMAND_CHARS: usize = 1024;

/// RCON is a privileged channel: reject control characters (which would let a
/// caller inject packet structure) and cap the size before it reaches the
/// server. A leading slash is accepted for convenience and stripped.
fn validate_command(raw: &str) -> Result<String, DaemonError> {
    let command = raw.trim().trim_start_matches('/').trim();
    if command.is_empty() {
        return Err(DaemonError::BadRequest("command must not be empty".into()));
    }
    if command.chars().count() > MAX_COMMAND_CHARS {
        return Err(DaemonError::BadRequest(format!(
            "command must be at most {MAX_COMMAND_CHARS} characters"
        )));
    }
    if command.chars().any(char::is_control) {
        return Err(DaemonError::BadRequest(
            "command must not contain control characters".into(),
        ));
    }
    Ok(command.to_string())
}

/// Defence in depth: the control plane validates too, but the daemon never
/// trusts it with host resources.
fn validate_spec(spec: &CreateServerSpec) -> Result<(), DaemonError> {
    if spec.ram_mb < 512 {
        return Err(DaemonError::BadRequest(
            "ram_mb must be at least 512".into(),
        ));
    }
    if !spec.cpu_limit.is_finite() || spec.cpu_limit <= 0.0 || spec.cpu_limit > 32.0 {
        return Err(DaemonError::BadRequest(
            "cpu_limit must be greater than 0 and at most 32".into(),
        ));
    }
    if spec.game_port == 0 {
        return Err(DaemonError::BadRequest("game_port must be non-zero".into()));
    }
    cluster_shared::normalize_game_version(&spec.game_version).map_err(DaemonError::BadRequest)?;
    if spec.is_behind_proxy && spec.server_type.is_proxy() {
        return Err(DaemonError::BadRequest(
            "a proxy container cannot itself be behind a proxy".into(),
        ));
    }
    Ok(())
}

fn validate_resources(resources: &ServerResources) -> Result<(), DaemonError> {
    if resources.ram_mb < 512 {
        return Err(DaemonError::BadRequest(
            "ram_mb must be at least 512".into(),
        ));
    }
    if !resources.cpu_limit.is_finite() || resources.cpu_limit <= 0.0 || resources.cpu_limit > 32.0
    {
        return Err(DaemonError::BadRequest(
            "cpu_limit must be greater than 0 and at most 32".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_and_normalises_commands() {
        assert_eq!(
            validate_command("  say hello  ").expect("valid"),
            "say hello"
        );
        assert_eq!(validate_command("/list").expect("leading slash"), "list");
    }

    #[test]
    fn rejects_empty_or_oversized_commands() {
        assert!(validate_command("   ").is_err());
        assert!(validate_command(&"a".repeat(MAX_COMMAND_CHARS + 1)).is_err());
    }

    #[test]
    fn rejects_control_characters() {
        assert!(validate_command("say hi\nstop").is_err());
        assert!(validate_command("say hi\rstop").is_err());
        assert!(validate_command("say\u{0}").is_err());
    }

    #[test]
    fn validates_scaling_resources() {
        let ok = ServerResources {
            ram_mb: 2048,
            cpu_limit: 2.0,
        };
        assert!(validate_resources(&ok).is_ok());

        assert!(
            validate_resources(&ServerResources {
                ram_mb: 128,
                cpu_limit: 2.0
            })
            .is_err()
        );
        assert!(
            validate_resources(&ServerResources {
                ram_mb: 2048,
                cpu_limit: f64::NAN
            })
            .is_err()
        );
    }
}
