//! Background scheduler for recurring tasks.
//!
//! A single tokio worker polls SQLite every 20 seconds, claims due rows by
//! advancing `next_run_at`, and executes them through the same daemon client the
//! HTTP routes use. There is no durable job queue yet; a crash during execution
//! means the task is retried on its next interval.

use std::time::Duration;

use cluster_shared::BackupInfo;
use sqlx::SqlitePool;

use crate::{error::ApiError, routes::servers, state::AppState};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScheduledTask {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub task_type: String,
    pub command: Option<String>,
    pub interval_minutes: i64,
    pub enabled: bool,
    pub next_run_at: String,
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // Let the API finish starting before the first poll.
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            if let Err(error) = tick(&state).await {
                tracing::error!(%error, "scheduler tick failed");
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });
}

async fn tick(state: &AppState) -> Result<(), ApiError> {
    let due = sqlx::query_as::<_, ScheduledTask>(
        "SELECT id, server_id, name, task_type, command, interval_minutes, enabled, next_run_at \
         FROM scheduled_tasks \
         WHERE enabled = 1 AND next_run_at <= strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         ORDER BY next_run_at ASC LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await?;

    for task in due {
        // Claim the row before running so a slow task is not selected twice.
        sqlx::query(
            "UPDATE scheduled_tasks SET next_run_at = strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1), \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        )
        .bind(format!("+{} minutes", task.interval_minutes))
        .bind(&task.id)
        .execute(&state.pool)
        .await?;

        let state = state.clone();
        tokio::spawn(async move {
            run_task(&state, &task).await;
        });
    }
    Ok(())
}

pub async fn fetch_task(pool: &SqlitePool, id: &str) -> Result<ScheduledTask, ApiError> {
    sqlx::query_as::<_, ScheduledTask>(
        "SELECT id, server_id, name, task_type, command, interval_minutes, enabled, next_run_at \
         FROM scheduled_tasks WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("schedule not found".into()))
}

/// Execute a task and persist its outcome. Errors become `last_status = error`;
/// they are logged rather than propagated because this is called from workers.
pub async fn run_task(state: &AppState, task: &ScheduledTask) {
    let outcome = execute(state, task).await;
    let (status, error) = match &outcome {
        Ok(()) => ("success", None),
        Err(error) => {
            tracing::warn!(schedule_id = %task.id, server_id = %task.server_id, %error, "scheduled task failed");
            ("error", Some(truncate(&error.to_string(), 500)))
        }
    };

    if let Err(error) = sqlx::query(
        "UPDATE scheduled_tasks SET last_run_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), \
         last_status = ?1, last_error = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?3",
    )
    .bind(status)
    .bind(error)
    .bind(&task.id)
    .execute(&state.pool)
    .await
    {
        tracing::error!(schedule_id = %task.id, %error, "failed to record schedule outcome");
    }
}

async fn execute(state: &AppState, task: &ScheduledTask) -> Result<(), ApiError> {
    let server = servers::fetch(&state.pool, &task.server_id).await?;
    match task.task_type.as_str() {
        "backup" => {
            let backup = state
                .daemon
                .create_backup(&server.id, Some(task.name.clone()))
                .await?;
            record_backup(state, &server.id, &backup).await?;
            if let Err(error) = crate::routes::automation::prune_backups(state, &server.id).await {
                tracing::warn!(server_id = %server.id, %error, "scheduled backup retention prune failed");
            }
            tracing::info!(server_id = %server.id, backup_id = %backup.id, "scheduled backup complete");
        }
        "start" => {
            state
                .daemon
                .start_server(&server.id, resources(&server))
                .await?;
            set_status(&state.pool, &server.id, "running").await?;
        }
        "stop" => {
            state.daemon.stop_server(&server.id).await?;
            set_status(&state.pool, &server.id, "stopped").await?;
        }
        "restart" => {
            state
                .daemon
                .restart_server(&server.id, resources(&server))
                .await?;
            set_status(&state.pool, &server.id, "running").await?;
        }
        "command" => {
            let command = task
                .command
                .as_deref()
                .ok_or_else(|| ApiError::Internal("command task has no command".into()))?;
            state.daemon.run_command(&server.id, command).await?;
        }
        other => {
            return Err(ApiError::Internal(format!(
                "unknown scheduled task type: {other}"
            )));
        }
    }
    Ok(())
}

fn resources(server: &servers::ServerView) -> cluster_shared::ServerResources {
    cluster_shared::ServerResources {
        ram_mb: server.ram_mb,
        cpu_limit: server.cpu_limit,
    }
}

async fn set_status(pool: &SqlitePool, id: &str, status: &str) -> Result<(), ApiError> {
    sqlx::query("UPDATE servers SET status = ?1 WHERE id = ?2")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_backup(
    state: &AppState,
    server_id: &str,
    backup: &BackupInfo,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO backups (id, server_id, name, size_bytes, sha256, server_was_running, created_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(&backup.id)
    .bind(server_id)
    .bind(&backup.name)
    .bind(i64::try_from(backup.size_bytes).unwrap_or(i64::MAX))
    .bind(&backup.sha256)
    .bind(i64::from(backup.server_was_running))
    .bind(backup.created_at_ms)
    .execute(&state.pool)
    .await?;
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
