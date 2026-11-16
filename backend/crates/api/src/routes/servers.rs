//! `/api/v1/servers` — create, list, and drive server lifecycle via the daemon.

use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use cluster_shared::{CreateServerSpec, ServerResources, ServerType, normalize_game_version};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{daemon::DaemonImportSource, error::ApiError, state::AppState};

/// Import uploads can be up to 10 GiB; the normal JSON body limit stays small.
const MAX_IMPORT_BYTES: usize = 10 * 1024 * 1024 * 1024;

const SELECT_ALL: &str = "SELECT id, name, game_port, server_type, game_version, ram_mb, cpu_limit, \
     is_behind_proxy, proxy_network_id, status, world_name, world_seed, world_type, difficulty, gamemode, created_at, updated_at \
     FROM servers ORDER BY created_at DESC";

const SELECT_BY_ID: &str = "SELECT id, name, game_port, server_type, game_version, ram_mb, cpu_limit, \
     is_behind_proxy, proxy_network_id, status, world_name, world_seed, world_type, difficulty, gamemode, created_at, updated_at \
     FROM servers WHERE id = ?1";

const MIN_GAME_PORT: u16 = 1024;
const MIN_RAM_MB: i64 = 512;
const MAX_RAM_MB: i64 = 65_536;
const MAX_CPU_LIMIT: f64 = 32.0;
const MAX_NAME_CHARS: usize = 64;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/servers", get(list).post(create))
        .route(
            "/servers/import",
            post(import_server).layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)),
        )
        .route(
            "/servers/{id}/import-data",
            post(import_existing_data).layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)),
        )
        .route("/servers/{id}", get(get_one).delete(remove))
        .route("/servers/{id}/start", post(start))
        .route("/servers/{id}/stop", post(stop))
        .route("/servers/{id}/restart", post(restart))
        .route("/servers/{id}/resources", patch(update_resources))
}

#[derive(Debug, sqlx::FromRow)]
struct ServerRow {
    id: String,
    name: String,
    game_port: i64,
    server_type: String,
    game_version: String,
    ram_mb: i64,
    cpu_limit: f64,
    is_behind_proxy: i64,
    proxy_network_id: Option<String>,
    status: String,
    world_name: String,
    world_seed: String,
    world_type: String,
    difficulty: String,
    gamemode: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ServerView {
    pub id: String,
    pub name: String,
    pub game_port: u16,
    pub server_type: ServerType,
    pub game_version: String,
    pub ram_mb: i64,
    pub cpu_limit: f64,
    pub is_behind_proxy: bool,
    pub proxy_network_id: Option<String>,
    pub status: String,
    pub world_name: String,
    pub world_seed: String,
    pub world_type: String,
    pub difficulty: String,
    pub gamemode: String,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<ServerRow> for ServerView {
    type Error = ApiError;

    fn try_from(row: ServerRow) -> Result<Self, ApiError> {
        let server_type = ServerType::from_str(&row.server_type).map_err(|_| {
            ApiError::Internal(format!(
                "corrupt server_type in database: {}",
                row.server_type
            ))
        })?;

        Ok(Self {
            id: row.id,
            name: row.name,
            game_port: u16::try_from(row.game_port)
                .map_err(|_| ApiError::Internal("corrupt game_port in database".into()))?,
            server_type,
            game_version: row.game_version,
            ram_mb: row.ram_mb,
            cpu_limit: row.cpu_limit,
            is_behind_proxy: row.is_behind_proxy != 0,
            proxy_network_id: row.proxy_network_id,
            status: row.status,
            world_name: row.world_name,
            world_seed: row.world_seed,
            world_type: row.world_type,
            difficulty: row.difficulty,
            gamemode: row.gamemode,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateServerRequest {
    pub name: String,
    pub server_type: ServerType,
    #[serde(default = "default_game_version")]
    pub game_version: String,
    pub game_port: u16,
    pub ram_mb: i64,
    pub cpu_limit: f64,
    #[serde(default)]
    pub is_behind_proxy: bool,
    #[serde(default)]
    pub proxy_network_id: Option<String>,
    #[serde(default = "default_world_name")]
    pub world_name: String,
    #[serde(default)]
    pub world_seed: String,
    #[serde(default = "default_world_type")]
    pub world_type: String,
    #[serde(default = "default_difficulty")]
    pub difficulty: String,
    #[serde(default = "default_gamemode")]
    pub gamemode: String,
}

fn default_game_version() -> String {
    "LATEST".to_string()
}
fn default_world_name() -> String {
    "world".into()
}
fn default_world_type() -> String {
    "default".into()
}
fn default_difficulty() -> String {
    "normal".into()
}
fn default_gamemode() -> String {
    "survival".into()
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<ServerView>>, ApiError> {
    let rows = sqlx::query_as::<_, ServerRow>(SELECT_ALL)
        .fetch_all(&state.pool)
        .await?;

    let servers = rows
        .into_iter()
        .map(ServerView::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(servers))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerView>, ApiError> {
    Ok(Json(fetch(&state.pool, &id).await?))
}

async fn create(
    State(state): State<AppState>,
    Json(request): Json<CreateServerRequest>,
) -> Result<(StatusCode, Json<ServerView>), ApiError> {
    let (name, game_version) = validate(&request)?;

    if let Some(network_id) = &request.proxy_network_id {
        ensure_proxy_network_exists(&state.pool, network_id).await?;
    }

    let id = Uuid::now_v7().to_string();

    let insert = sqlx::query(
        "INSERT INTO servers \
         (id, name, game_port, server_type, game_version, ram_mb, cpu_limit, is_behind_proxy, proxy_network_id, status, world_name, world_seed, world_type, difficulty, gamemode) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'provisioning', ?10, ?11, ?12, ?13, ?14)",
    )
    .bind(&id)
    .bind(&name)
    .bind(i64::from(request.game_port))
    .bind(request.server_type.as_str())
    .bind(&game_version)
    .bind(request.ram_mb)
    .bind(request.cpu_limit)
    .bind(i64::from(request.is_behind_proxy))
    .bind(&request.proxy_network_id)
    .bind(&request.world_name).bind(&request.world_seed).bind(&request.world_type).bind(&request.difficulty).bind(&request.gamemode)
    .execute(&state.pool)
    .await
    .map_err(map_unique_violation)?;

    debug_assert_eq!(insert.rows_affected(), 1);

    let spec = CreateServerSpec {
        id: id.clone(),
        name,
        server_type: request.server_type,
        game_version,
        game_port: request.game_port,
        ram_mb: request.ram_mb,
        cpu_limit: request.cpu_limit,
        is_behind_proxy: request.is_behind_proxy,
        world_name: request.world_name.clone(),
        world_seed: request.world_seed.clone(),
        world_type: request.world_type.clone(),
        difficulty: request.difficulty.clone(),
        gamemode: request.gamemode.clone(),
    };

    if let Err(error) = state.daemon.create_server(&spec).await {
        // Compensate: no container exists, so the row must not linger.
        if let Err(cleanup) = sqlx::query("DELETE FROM servers WHERE id = ?1")
            .bind(&id)
            .execute(&state.pool)
            .await
        {
            tracing::error!(server_id = %id, %cleanup, "failed to roll back server row");
        }
        return Err(error);
    }

    set_status(&state.pool, &id, "stopped").await?;
    tracing::info!(server_id = %id, "provisioned server");
    Ok((StatusCode::CREATED, Json(fetch(&state.pool, &id).await?)))
}

/// Import existing server data from a multipart request. The form carries the
/// same configuration the create flow needs, plus exactly one source: a ZIP
/// `file` part or an absolute daemon-host `local_path`.
async fn import_server(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ServerView>), ApiError> {
    let mut name: Option<String> = None;
    let mut server_type: Option<ServerType> = None;
    let mut game_version: Option<String> = None;
    let mut ram_mb: Option<i64> = None;
    let mut cpu_limit: Option<f64> = None;
    let mut local_path: Option<String> = None;
    let mut upload = TempUpload::default();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::BadRequest(format!("invalid import form: {error}")))?
    {
        let field_name = field.name().map(ToString::to_string).unwrap_or_default();
        match field_name.as_str() {
            "name" => name = Some(read_text(field, 256).await?),
            "server_type" => {
                let value = read_text(field, 32).await?;
                server_type = Some(ServerType::from_str(value.trim()).map_err(|_| {
                    ApiError::BadRequest(format!("unknown server type: {}", value.trim()))
                })?);
            }
            "game_version" => game_version = Some(read_text(field, 64).await?),
            "ram_mb" => {
                ram_mb = Some(parse_number(&read_text(field, 32).await?, "ram_mb")?);
            }
            "cpu_limit" => {
                cpu_limit = Some(parse_number(&read_text(field, 32).await?, "cpu_limit")?);
            }
            "local_path" => {
                if upload.path.is_some() || local_path.is_some() {
                    return Err(ApiError::BadRequest(
                        "provide exactly one import source: file or local_path".into(),
                    ));
                }
                local_path = Some(read_text(field, 4096).await?);
            }
            "file" => {
                if upload.path.is_some() || local_path.is_some() {
                    return Err(ApiError::BadRequest(
                        "provide exactly one import source: file or local_path".into(),
                    ));
                }
                let path =
                    std::env::temp_dir().join(format!("cluster-import-{}.zip", Uuid::now_v7()));
                let file = tokio::fs::File::create(&path).await.map_err(|error| {
                    ApiError::Internal(format!("create import temporary file: {error}"))
                })?;
                let mut writer = tokio::io::BufWriter::new(file);
                let mut total = 0usize;

                while let Some(chunk) = field.chunk().await.map_err(|error| {
                    ApiError::BadRequest(format!("read import archive: {error}"))
                })? {
                    total = total.saturating_add(chunk.len());
                    if total > MAX_IMPORT_BYTES {
                        let _ = tokio::fs::remove_file(&path).await;
                        return Err(ApiError::BadRequest(format!(
                            "import archive exceeds the {MAX_IMPORT_BYTES} byte limit"
                        )));
                    }
                    writer.write_all(&chunk).await.map_err(|error| {
                        ApiError::Internal(format!("write import temporary file: {error}"))
                    })?;
                }
                writer.flush().await.map_err(|error| {
                    ApiError::Internal(format!("flush import temporary file: {error}"))
                })?;
                writer.into_inner().sync_all().await.map_err(|error| {
                    ApiError::Internal(format!("sync import temporary file: {error}"))
                })?;
                if total == 0 {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(ApiError::BadRequest(
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

    let name = name.unwrap_or_default();
    let server_type =
        server_type.ok_or_else(|| ApiError::BadRequest("server_type is required".into()))?;
    let game_version = game_version.unwrap_or_default();
    let ram_mb = ram_mb.ok_or_else(|| ApiError::BadRequest("ram_mb is required".into()))?;
    let cpu_limit =
        cpu_limit.ok_or_else(|| ApiError::BadRequest("cpu_limit is required".into()))?;

    let source = match (upload.path.clone(), local_path) {
        (Some(path), None) => DaemonImportSource::Zip { path },
        (None, Some(path)) => DaemonImportSource::LocalPath(path),
        (Some(_), Some(_)) => {
            return Err(ApiError::BadRequest(
                "provide exactly one import source: file or local_path".into(),
            ));
        }
        (None, None) => {
            return Err(ApiError::BadRequest(
                "provide an import source: file or local_path".into(),
            ));
        }
    };

    let validation = CreateServerRequest {
        name,
        server_type,
        game_version,
        game_port: 25565,
        ram_mb,
        cpu_limit,
        is_behind_proxy: false,
        world_name: default_world_name(),
        world_seed: String::new(),
        world_type: default_world_type(),
        difficulty: default_difficulty(),
        gamemode: default_gamemode(),
        proxy_network_id: None,
    };
    let (name, game_version) = validate(&validation)?;
    let game_port = allocate_port(&state.pool).await?;
    let id = Uuid::now_v7().to_string();

    let insert = sqlx::query(
        "INSERT INTO servers \
         (id, name, game_port, server_type, game_version, ram_mb, cpu_limit, \
          is_behind_proxy, proxy_network_id, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, NULL, 'provisioning')",
    )
    .bind(&id)
    .bind(&name)
    .bind(i64::from(game_port))
    .bind(server_type.as_str())
    .bind(&game_version)
    .bind(ram_mb)
    .bind(cpu_limit)
    .execute(&state.pool)
    .await
    .map_err(map_unique_violation)?;
    debug_assert_eq!(insert.rows_affected(), 1);

    let spec = CreateServerSpec {
        id: id.clone(),
        name,
        server_type,
        game_version,
        game_port,
        ram_mb,
        cpu_limit,
        is_behind_proxy: false,
        world_name: default_world_name(),
        world_seed: String::new(),
        world_type: default_world_type(),
        difficulty: default_difficulty(),
        gamemode: default_gamemode(),
    };

    // Create the (stopped) container first: this proves Docker and the image are
    // ready before a potentially huge upload is consumed.
    if let Err(error) = state.daemon.create_server(&spec).await {
        if let Err(cleanup) = sqlx::query("DELETE FROM servers WHERE id = ?1")
            .bind(&id)
            .execute(&state.pool)
            .await
        {
            tracing::error!(server_id = %id, %cleanup, "failed to roll back imported server row");
        }
        return Err(error);
    }

    let import = state.daemon.import_server(&id, source).await;
    if let Err(error) = import {
        if let Err(cleanup) = state.daemon.delete_server(&id).await {
            tracing::error!(server_id = %id, %cleanup, "failed to remove container after import error");
        }
        if let Err(cleanup) = sqlx::query("DELETE FROM servers WHERE id = ?1")
            .bind(&id)
            .execute(&state.pool)
            .await
        {
            tracing::error!(server_id = %id, %cleanup, "failed to roll back imported server row");
        }
        return Err(error);
    }

    set_status(&state.pool, &id, "stopped").await?;
    tracing::info!(server_id = %id, "imported server data");
    Ok((StatusCode::CREATED, Json(fetch(&state.pool, &id).await?)))
}

/// Import a ZIP into an already-provisioned (and still stopped) server. This is
/// used by the create wizard's optional world/server archive step.
async fn import_existing_data(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ServerView>), ApiError> {
    let server = fetch(&state.pool, &id).await?;
    if server.status != "stopped" && server.status != "error" {
        return Err(ApiError::Conflict(
            "the server must be stopped before importing data".into(),
        ));
    }

    let mut upload = TempUpload::default();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::BadRequest(format!("invalid import form: {error}")))?
    {
        if field.name() != Some("file") {
            let _ = field.bytes().await;
            continue;
        }
        if upload.path.is_some() {
            return Err(ApiError::BadRequest("provide exactly one ZIP file".into()));
        }
        let path = std::env::temp_dir().join(format!("cluster-data-{}.zip", Uuid::now_v7()));
        let file = tokio::fs::File::create(&path).await.map_err(|error| {
            ApiError::Internal(format!("create import temporary file: {error}"))
        })?;
        let mut writer = tokio::io::BufWriter::new(file);
        let mut total = 0usize;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|error| ApiError::BadRequest(format!("read import archive: {error}")))?
        {
            total = total.saturating_add(chunk.len());
            if total > MAX_IMPORT_BYTES {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(ApiError::BadRequest("import archive is too large".into()));
            }
            writer.write_all(&chunk).await.map_err(|error| {
                ApiError::Internal(format!("write import temporary file: {error}"))
            })?;
        }
        writer
            .flush()
            .await
            .map_err(|error| ApiError::Internal(format!("flush import temporary file: {error}")))?;
        writer
            .into_inner()
            .sync_all()
            .await
            .map_err(|error| ApiError::Internal(format!("sync import temporary file: {error}")))?;
        if total == 0 {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(ApiError::BadRequest(
                "uploaded import archive is empty".into(),
            ));
        }
        upload.path = Some(path);
    }

    let Some(path) = upload.path.clone() else {
        return Err(ApiError::BadRequest("a ZIP file is required".into()));
    };
    state
        .daemon
        .import_server(&id, DaemonImportSource::Zip { path })
        .await?;
    tracing::info!(server_id = %id, "imported data into existing server");
    Ok((StatusCode::CREATED, Json(fetch(&state.pool, &id).await?)))
}

async fn read_text(
    mut field: axum::extract::multipart::Field<'_>,
    max_bytes: usize,
) -> Result<String, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| ApiError::BadRequest(format!("read import field: {error}")))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::BadRequest(
                "import form field is too large".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes)
        .map_err(|_| ApiError::BadRequest("import form field is not valid UTF-8".into()))
}

fn parse_number<T: std::str::FromStr>(value: &str, field: &str) -> Result<T, ApiError> {
    value
        .trim()
        .parse::<T>()
        .map_err(|_| ApiError::BadRequest(format!("{field} has an invalid value")))
}

async fn allocate_port(pool: &SqlitePool) -> Result<u16, ApiError> {
    let used: Vec<i64> = sqlx::query_scalar("SELECT game_port FROM servers")
        .fetch_all(pool)
        .await?;
    let used: HashSet<i64> = used.into_iter().collect();
    for port in 25565u16..=u16::MAX {
        if !used.contains(&i64::from(port)) {
            return Ok(port);
        }
    }
    Err(ApiError::Conflict("no free game port is available".into()))
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

async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerView>, ApiError> {
    let server = fetch(&state.pool, &id).await?;
    state
        .daemon
        .start_server(&server.id, resources_of(&server))
        .await?;
    set_status(&state.pool, &id, "running").await?;
    Ok(Json(fetch(&state.pool, &id).await?))
}

async fn stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerView>, ApiError> {
    let server = fetch(&state.pool, &id).await?;
    state.daemon.stop_server(&server.id).await?;
    set_status(&state.pool, &id, "stopped").await?;
    Ok(Json(fetch(&state.pool, &id).await?))
}

async fn restart(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerView>, ApiError> {
    let server = fetch(&state.pool, &id).await?;
    set_status(&state.pool, &id, "stopping").await?;
    // The daemon applies these limits before bringing the container back up.
    state
        .daemon
        .restart_server(&server.id, resources_of(&server))
        .await?;
    set_status(&state.pool, &id, "running").await?;
    tracing::info!(server_id = %id, "restarted server");
    Ok(Json(fetch(&state.pool, &id).await?))
}

/// The current DB resource limits, carried to the daemon on start/restart.
fn resources_of(server: &ServerView) -> ServerResources {
    ServerResources {
        ram_mb: server.ram_mb,
        cpu_limit: server.cpu_limit,
    }
}

#[derive(Debug, Deserialize)]
struct UpdateResourcesRequest {
    ram_mb: Option<i64>,
    cpu_limit: Option<f64>,
}

async fn update_resources(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateResourcesRequest>,
) -> Result<Json<ServerView>, ApiError> {
    // Ensure the server exists first.
    fetch(&state.pool, &id).await?;

    // Validate everything before touching the row so a mixed request cannot be
    // half-applied.
    if let Some(ram) = request.ram_mb {
        if !(MIN_RAM_MB..=MAX_RAM_MB).contains(&ram) {
            return Err(ApiError::BadRequest(format!(
                "ram_mb must be between {MIN_RAM_MB} and {MAX_RAM_MB}"
            )));
        }
    }
    if let Some(cpu) = request.cpu_limit {
        if !cpu.is_finite() || cpu <= 0.0 || cpu > MAX_CPU_LIMIT {
            return Err(ApiError::BadRequest(format!(
                "cpu_limit must be greater than 0 and at most {MAX_CPU_LIMIT}"
            )));
        }
    }

    if let Some(ram) = request.ram_mb {
        sqlx::query(
            "UPDATE servers SET ram_mb = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        )
        .bind(ram)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    }

    if let Some(cpu) = request.cpu_limit {
        sqlx::query(
            "UPDATE servers SET cpu_limit = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        )
        .bind(cpu)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    }

    tracing::info!(server_id = %id, "updated server resources");
    Ok(Json(fetch(&state.pool, &id).await?))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    fetch(&state.pool, &id).await?;
    state.daemon.delete_server(&id).await?;

    sqlx::query("DELETE FROM servers WHERE id = ?1")
        .bind(&id)
        .execute(&state.pool)
        .await?;

    tracing::info!(server_id = %id, "deleted server");
    Ok(StatusCode::NO_CONTENT)
}

/// Validate and normalise the request. Returns the trimmed name and version.
fn validate(request: &CreateServerRequest) -> Result<(String, String), ApiError> {
    let name = request.name.trim();
    let name_len = name.chars().count();
    if name_len == 0 || name_len > MAX_NAME_CHARS {
        return Err(ApiError::BadRequest(format!(
            "name must be between 1 and {MAX_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "name must not contain control characters".into(),
        ));
    }

    let game_version =
        normalize_game_version(&request.game_version).map_err(ApiError::BadRequest)?;

    if request.server_type.requires_explicit_game_version()
        && matches!(game_version.as_str(), "LATEST" | "SNAPSHOT")
    {
        return Err(ApiError::BadRequest(format!(
            "{} does not publish builds for {}, choose a supported game version",
            request.server_type, game_version
        )));
    }

    if request.game_port < MIN_GAME_PORT {
        return Err(ApiError::BadRequest(format!(
            "game_port must be at least {MIN_GAME_PORT}"
        )));
    }

    if !(MIN_RAM_MB..=MAX_RAM_MB).contains(&request.ram_mb) {
        return Err(ApiError::BadRequest(format!(
            "ram_mb must be between {MIN_RAM_MB} and {MAX_RAM_MB}"
        )));
    }

    if !request.cpu_limit.is_finite()
        || request.cpu_limit <= 0.0
        || request.cpu_limit > MAX_CPU_LIMIT
    {
        return Err(ApiError::BadRequest(format!(
            "cpu_limit must be greater than 0 and at most {MAX_CPU_LIMIT}"
        )));
    }

    if request.is_behind_proxy && request.server_type.is_proxy() {
        return Err(ApiError::BadRequest(
            "a velocity/bungeecord proxy cannot itself be behind a proxy".into(),
        ));
    }
    if request.is_behind_proxy && request.proxy_network_id.is_none() {
        return Err(ApiError::BadRequest(
            "proxy_network_id is required when is_behind_proxy is true".into(),
        ));
    }

    validate_world_name(&request.world_name)?;
    validate_world_seed(&request.world_seed)?;
    validate_choice(
        "world_type",
        &request.world_type,
        &[
            "default",
            "flat",
            "largeBiomes",
            "amplified",
            "normal",
            "large_biomes",
        ],
    )?;
    validate_choice(
        "difficulty",
        &request.difficulty,
        &["peaceful", "easy", "normal", "hard"],
    )?;
    validate_choice(
        "gamemode",
        &request.gamemode,
        &["survival", "creative", "adventure", "spectator"],
    )?;

    Ok((name.to_string(), game_version))
}

/// The world name becomes both the itzg `LEVEL` env value and a directory name
/// inside the data volume. Keep it to a single safe path component.
fn validate_world_name(raw: &str) -> Result<(), ApiError> {
    let value = raw.trim();
    if value.is_empty() || value.chars().count() > 128 {
        return Err(ApiError::BadRequest(
            "world_name must be between 1 and 128 characters".into(),
        ));
    }
    if value == "." || value == ".." || value.contains(['/', '\\', '\0']) {
        return Err(ApiError::BadRequest(
            "world_name must be a single directory name".into(),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "world_name must not contain control characters".into(),
        ));
    }
    Ok(())
}

fn validate_world_seed(raw: &str) -> Result<(), ApiError> {
    if raw.chars().count() > 128 {
        return Err(ApiError::BadRequest(
            "world_seed must be at most 128 characters".into(),
        ));
    }
    if raw.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "world_seed must not contain control characters".into(),
        ));
    }
    Ok(())
}

fn validate_choice(field: &str, raw: &str, allowed: &[&str]) -> Result<(), ApiError> {
    if allowed.contains(&raw.trim()) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "{field} must be one of: {}",
            allowed.join(", ")
        )))
    }
}

async fn ensure_proxy_network_exists(pool: &SqlitePool, network_id: &str) -> Result<(), ApiError> {
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM proxy_networks WHERE id = ?1")
        .bind(network_id)
        .fetch_optional(pool)
        .await?;

    if exists.is_none() {
        return Err(ApiError::BadRequest("unknown proxy_network_id".into()));
    }
    Ok(())
}

pub(crate) async fn fetch(pool: &SqlitePool, id: &str) -> Result<ServerView, ApiError> {
    let row = sqlx::query_as::<_, ServerRow>(SELECT_BY_ID)
        .bind(id)
        .fetch_optional(pool)
        .await?;

    row.map(ServerView::try_from)
        .transpose()?
        .ok_or_else(|| ApiError::NotFound("server not found".into()))
}

async fn set_status(pool: &SqlitePool, id: &str, status: &str) -> Result<(), ApiError> {
    sqlx::query("UPDATE servers SET status = ?1 WHERE id = ?2")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

fn map_unique_violation(error: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(database_error) = &error {
        if database_error.is_unique_violation() {
            return ApiError::Conflict(
                "a server with that name or game port already exists".into(),
            );
        }
    }
    error.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CreateServerRequest {
        CreateServerRequest {
            name: "  survival  ".into(),
            server_type: ServerType::Paper,
            game_version: "1.21.1".into(),
            game_port: 25565,
            ram_mb: 4096,
            cpu_limit: 2.0,
            is_behind_proxy: false,
            proxy_network_id: None,
            world_name: default_world_name(),
            world_seed: String::new(),
            world_type: default_world_type(),
            difficulty: default_difficulty(),
            gamemode: default_gamemode(),
        }
    }

    #[test]
    fn trims_and_accepts_valid_request() {
        let (name, version) = validate(&request()).expect("valid request");
        assert_eq!(name, "survival");
        assert_eq!(version, "1.21.1");
    }

    #[test]
    fn rejects_bad_ports_and_limits() {
        let mut req = request();
        req.game_port = 80;
        assert!(validate(&req).is_err());

        let mut req = request();
        req.ram_mb = 128;
        assert!(validate(&req).is_err());

        let mut req = request();
        req.cpu_limit = f64::NAN;
        assert!(validate(&req).is_err());

        let mut req = request();
        req.cpu_limit = 64.0;
        assert!(validate(&req).is_err());
    }

    #[test]
    fn requires_proxy_network_for_proxied_backends() {
        let mut req = request();
        req.is_behind_proxy = true;
        assert!(validate(&req).is_err());

        req.proxy_network_id = Some("network".into());
        assert!(validate(&req).is_ok());
    }

    #[test]
    fn proxies_cannot_be_behind_a_proxy() {
        let mut req = request();
        req.server_type = ServerType::Velocity;
        req.is_behind_proxy = true;
        req.proxy_network_id = Some("network".into());
        assert!(validate(&req).is_err());
    }

    #[test]
    fn rejects_blank_names() {
        let mut req = request();
        req.name = "   ".into();
        assert!(validate(&req).is_err());
    }

    #[test]
    fn normalises_and_rejects_game_versions() {
        let mut req = request();
        req.game_version = " latest ".into();
        assert_eq!(validate(&req).expect("valid version").1, "LATEST");

        req.game_version = "1.21 1".into();
        assert!(validate(&req).is_err());

        req.game_version = String::new();
        assert!(validate(&req).is_err());
    }

    #[test]
    fn hybrids_reject_the_latest_alias() {
        for server_type in [
            ServerType::Arclight,
            ServerType::CatServer,
            ServerType::Mohist,
            ServerType::Magma,
        ] {
            let mut req = request();
            req.server_type = server_type;
            req.game_version = "LATEST".into();
            assert!(
                validate(&req).is_err(),
                "{} must require a pinned version",
                server_type.as_str()
            );

            req.game_version = "1.21.1".into();
            assert!(validate(&req).is_ok());
        }
    }
}
