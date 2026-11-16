//! Backups and scheduled task API.
//!
//! Backup archives live on the daemon; the control plane keeps a catalog row so
//! the UI can list them without a daemon round-trip. Scheduled tasks are stored
//! in SQLite and executed by the background worker in [`crate::scheduler`].

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get, patch, post},
};
use cluster_shared::{BackupInfo, BackupVerification};
use serde::{Deserialize, Serialize};

use crate::{error::ApiError, routes::servers, scheduler, state::AppState};

const MAX_SCHEDULE_NAME: usize = 80;
const MAX_INTERVAL_MINUTES: i64 = 525_600; // one year
const MAX_COMMAND_CHARS: usize = 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/servers/{id}/backups",
            get(list_backups).post(create_backup),
        )
        .route(
            "/servers/{id}/backups/settings",
            get(get_backup_settings).patch(update_backup_settings),
        )
        .route("/servers/{id}/backups/{backup_id}", delete(remove_backup))
        .route(
            "/servers/{id}/backups/{backup_id}/restore",
            post(restore_backup),
        )
        .route(
            "/servers/{id}/backups/{backup_id}/download",
            get(download_backup),
        )
        .route(
            "/servers/{id}/backups/{backup_id}/verify",
            post(verify_backup),
        )
        .route(
            "/servers/{id}/schedules",
            get(list_schedules).post(create_schedule),
        )
        .route(
            "/servers/{id}/schedules/{schedule_id}",
            patch(update_schedule).delete(delete_schedule),
        )
        .route(
            "/servers/{id}/schedules/{schedule_id}/run",
            post(run_schedule),
        )
}

#[derive(Debug, Deserialize)]
struct CreateBackupRequest {
    #[serde(default)]
    name: Option<String>,
}

async fn list_backups(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<BackupInfo>>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    // Prefer the daemon (authoritative on disk), then reconcile the catalog.
    let backups = state.daemon.list_backups(&id).await?;
    for backup in &backups {
        sqlx::query(
            "INSERT INTO backups (id, server_id, name, size_bytes, sha256, server_was_running, created_at_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(id) DO UPDATE SET size_bytes = excluded.size_bytes, sha256 = excluded.sha256",
        )
        .bind(&backup.id)
        .bind(&id)
        .bind(&backup.name)
        .bind(i64::try_from(backup.size_bytes).unwrap_or(i64::MAX))
        .bind(&backup.sha256)
        .bind(i64::from(backup.server_was_running))
        .bind(backup.created_at_ms)
        .execute(&state.pool)
        .await?;
    }
    Ok(Json(backups))
}

async fn create_backup(
    State(state): State<AppState>,
    Path(id): Path<String>,
    payload: Option<Json<CreateBackupRequest>>,
) -> Result<(StatusCode, Json<BackupInfo>), ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let name = payload.and_then(|Json(request)| request.name);
    let backup = state.daemon.create_backup(&id, name).await?;
    crate::scheduler::record_backup(&state, &id, &backup).await?;
    if let Err(error) = prune_backups(&state, &id).await {
        // Retention is best effort: never fail a successful snapshot because
        // an old archive could not be removed.
        tracing::warn!(server_id = %id, %error, "backup retention prune failed");
    }
    Ok((StatusCode::CREATED, Json(backup)))
}

async fn remove_backup(
    State(state): State<AppState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    state.daemon.delete_backup(&id, &backup_id).await?;
    sqlx::query("DELETE FROM backups WHERE id = ?1 AND server_id = ?2")
        .bind(&backup_id)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_backup(
    State(state): State<AppState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> Result<Json<BackupInfo>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let backup = state.daemon.restore_backup(&id, &backup_id).await?;
    tracing::info!(server_id = %id, backup_id = %backup_id, "restored backup");
    Ok(Json(backup))
}

/// Stream a backup archive through the control plane. The daemon connection is
/// already signed and the bytes are forwarded without buffering.
async fn download_backup(
    State(state): State<AppState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let stream = state.daemon.download_backup_stream(&id, &backup_id).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"backup-{backup_id}.zip\""),
        )
        .body(Body::from_stream(stream))
        .map_err(|error| ApiError::Internal(format!("build backup download response: {error}")))
}

async fn verify_backup(
    State(state): State<AppState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> Result<Json<BackupVerification>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    Ok(Json(state.daemon.verify_backup(&id, &backup_id).await?))
}

/// How many of the newest snapshots to keep; `0` disables pruning.
#[derive(Debug, Clone, Serialize)]
pub struct BackupSettings {
    pub server_id: String,
    pub retention_count: i64,
}

#[derive(Debug, Deserialize)]
struct UpdateBackupSettings {
    retention_count: i64,
}

async fn get_backup_settings(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BackupSettings>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    Ok(Json(fetch_backup_settings(&state, &id).await?))
}

async fn update_backup_settings(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateBackupSettings>,
) -> Result<Json<BackupSettings>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    if !(0..=500).contains(&request.retention_count) {
        return Err(ApiError::BadRequest(
            "retention_count must be between 0 and 500 (0 keeps every backup)".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO backup_settings (server_id, retention_count) VALUES (?1, ?2) \
         ON CONFLICT(server_id) DO UPDATE SET retention_count = excluded.retention_count, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
    )
    .bind(&id)
    .bind(request.retention_count)
    .execute(&state.pool)
    .await?;
    Ok(Json(fetch_backup_settings(&state, &id).await?))
}

async fn fetch_backup_settings(
    state: &AppState,
    server_id: &str,
) -> Result<BackupSettings, ApiError> {
    let retention_count = sqlx::query_scalar::<_, i64>(
        "SELECT retention_count FROM backup_settings WHERE server_id = ?1",
    )
    .bind(server_id)
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or(0);
    Ok(BackupSettings {
        server_id: server_id.to_string(),
        retention_count,
    })
}

/// Delete snapshots beyond the configured retention count.
///
/// The daemon returns backups newest-first, so `skip(keep)` removes the oldest
/// archives. A deletion failure is logged and the remaining candidates are
/// still attempted; the caller treats pruning as best effort.
pub async fn prune_backups(state: &AppState, server_id: &str) -> Result<usize, ApiError> {
    let settings = fetch_backup_settings(state, server_id).await?;
    if settings.retention_count <= 0 {
        return Ok(0);
    }
    let keep = usize::try_from(settings.retention_count).unwrap_or(usize::MAX);
    let backups = state.daemon.list_backups(server_id).await?;
    let mut removed = 0usize;
    for backup in backups.iter().skip(keep) {
        match state.daemon.delete_backup(server_id, &backup.id).await {
            Ok(()) => {
                let _ = sqlx::query("DELETE FROM backups WHERE id = ?1 AND server_id = ?2")
                    .bind(&backup.id)
                    .bind(server_id)
                    .execute(&state.pool)
                    .await;
                removed += 1;
            }
            Err(error) => {
                tracing::warn!(
                    server_id = %server_id,
                    backup_id = %backup.id,
                    %error,
                    "backup retention could not delete an archive"
                );
            }
        }
    }
    if removed > 0 {
        tracing::info!(
            server_id = %server_id,
            removed,
            keep = settings.retention_count,
            "pruned old backups"
        );
    }
    Ok(removed)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ScheduleView {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub task_type: String,
    pub command: Option<String>,
    pub interval_minutes: i64,
    pub enabled: bool,
    pub next_run_at: String,
    pub last_run_at: Option<String>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateScheduleRequest {
    name: String,
    task_type: String,
    #[serde(default)]
    command: Option<String>,
    interval_minutes: i64,
    #[serde(default = "default_enabled")]
    enabled: bool,
    /// Run once immediately in addition to the interval.
    #[serde(default)]
    run_now: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateScheduleRequest {
    name: Option<String>,
    command: Option<String>,
    interval_minutes: Option<i64>,
    enabled: Option<bool>,
}

fn default_enabled() -> bool {
    true
}

async fn list_schedules(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ScheduleView>>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let rows = sqlx::query_as::<_, ScheduleView>(
        "SELECT id, server_id, name, task_type, command, interval_minutes, enabled, \
         next_run_at, last_run_at, last_status, last_error, created_at, updated_at \
         FROM scheduled_tasks WHERE server_id = ?1 ORDER BY created_at ASC",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_schedule(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleView>), ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let (name, task_type, command) = validate_task(
        &request.name,
        &request.task_type,
        request.command.as_deref(),
        request.interval_minutes,
    )?;
    let schedule_id = uuid::Uuid::now_v7().to_string();
    let offset = if request.run_now {
        0
    } else {
        request.interval_minutes
    };

    sqlx::query(
        "INSERT INTO scheduled_tasks \
         (id, server_id, name, task_type, command, interval_minutes, enabled, next_run_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, \
                 strftime('%Y-%m-%dT%H:%M:%fZ','now', ?8))",
    )
    .bind(&schedule_id)
    .bind(&id)
    .bind(name)
    .bind(task_type)
    .bind(command)
    .bind(request.interval_minutes)
    .bind(i64::from(request.enabled as u8))
    .bind(format!("+{offset} minutes"))
    .execute(&state.pool)
    .await?;

    let row = fetch_schedule(&state, &id, &schedule_id).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_schedule(
    State(state): State<AppState>,
    Path((id, schedule_id)): Path<(String, String)>,
    Json(request): Json<UpdateScheduleRequest>,
) -> Result<Json<ScheduleView>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let existing = fetch_schedule(&state, &id, &schedule_id).await?;
    let name = request.name.as_deref().unwrap_or(&existing.name);
    let task_type = existing.task_type.as_str();
    let command = request.command.as_deref().or(existing.command.as_deref());
    let interval = request
        .interval_minutes
        .unwrap_or(existing.interval_minutes);
    let (name, task_type, command) = validate_task(name, task_type, command, interval)?;

    sqlx::query(
        "UPDATE scheduled_tasks SET name = ?1, task_type = ?2, command = ?3, \
         interval_minutes = ?4, enabled = ?5, \
         next_run_at = strftime('%Y-%m-%dT%H:%M:%fZ','now', ?6), \
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?7 AND server_id = ?8",
    )
    .bind(name)
    .bind(task_type)
    .bind(command)
    .bind(interval)
    .bind(i64::from(request.enabled.unwrap_or(existing.enabled) as u8))
    .bind(format!("+{interval} minutes"))
    .bind(&schedule_id)
    .bind(&id)
    .execute(&state.pool)
    .await?;

    Ok(Json(fetch_schedule(&state, &id, &schedule_id).await?))
}

async fn delete_schedule(
    State(state): State<AppState>,
    Path((id, schedule_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let result = sqlx::query("DELETE FROM scheduled_tasks WHERE id = ?1 AND server_id = ?2")
        .bind(&schedule_id)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound("schedule not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn run_schedule(
    State(state): State<AppState>,
    Path((id, schedule_id)): Path<(String, String)>,
) -> Result<Json<ScheduleView>, ApiError> {
    servers::fetch(&state.pool, &id).await?;
    let task = scheduler::fetch_task(&state.pool, &schedule_id).await?;
    if task.server_id != id {
        return Err(ApiError::NotFound("schedule not found".into()));
    }
    scheduler::run_task(&state, &task).await;
    Ok(Json(fetch_schedule(&state, &id, &schedule_id).await?))
}

async fn fetch_schedule(
    state: &AppState,
    server_id: &str,
    schedule_id: &str,
) -> Result<ScheduleView, ApiError> {
    sqlx::query_as::<_, ScheduleView>(
        "SELECT id, server_id, name, task_type, command, interval_minutes, enabled, \
         next_run_at, last_run_at, last_status, last_error, created_at, updated_at \
         FROM scheduled_tasks WHERE id = ?1 AND server_id = ?2",
    )
    .bind(schedule_id)
    .bind(server_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("schedule not found".into()))
}

fn validate_task(
    raw_name: &str,
    task_type: &str,
    command: Option<&str>,
    interval_minutes: i64,
) -> Result<(String, String, Option<String>), ApiError> {
    let name = raw_name.trim();
    if name.is_empty() || name.chars().count() > MAX_SCHEDULE_NAME {
        return Err(ApiError::BadRequest(format!(
            "schedule name must be between 1 and {MAX_SCHEDULE_NAME} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "schedule name must not contain control characters".into(),
        ));
    }
    if !(1..=MAX_INTERVAL_MINUTES).contains(&interval_minutes) {
        return Err(ApiError::BadRequest(format!(
            "interval_minutes must be between 1 and {MAX_INTERVAL_MINUTES}"
        )));
    }
    let task_type = task_type.trim().to_ascii_lowercase();
    if !matches!(
        task_type.as_str(),
        "backup" | "start" | "stop" | "restart" | "command"
    ) {
        return Err(ApiError::BadRequest(
            "task_type must be backup, start, stop, restart or command".into(),
        ));
    }

    let command = if task_type == "command" {
        let value = command.unwrap_or_default().trim();
        if value.is_empty() {
            return Err(ApiError::BadRequest(
                "command is required for a command task".into(),
            ));
        }
        if value.chars().count() > MAX_COMMAND_CHARS || value.chars().any(char::is_control) {
            return Err(ApiError::BadRequest(
                "command must be at most 1024 characters and contain no control characters".into(),
            ));
        }
        Some(value.to_string())
    } else {
        None
    };

    Ok((name.to_string(), task_type, command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_scheduled_tasks() {
        assert!(validate_task("nightly", "backup", None, 60).is_ok());
        assert!(validate_task("restart", "restart", None, 5).is_ok());
        assert!(validate_task("say", "command", Some("say hello"), 5).is_ok());
        assert!(validate_task("say", "command", None, 5).is_err());
        assert!(validate_task("bad", "explode", None, 5).is_err());
        assert!(validate_task("bad", "backup", None, 0).is_err());
        assert!(validate_task("", "backup", None, 5).is_err());
    }
}
