//! Application state shared by every handler.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::{auth::AuthService, bootstrap, config::Config, daemon::DaemonClient};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub auth: AuthService,
    pub daemon: DaemonClient,
    pub config: Arc<Config>,
}

/// Open the database, apply migrations, and seed the first admin user.
pub async fn init(config: Config) -> anyhow::Result<AppState> {
    let database_path = std::path::Path::new(&config.database_path);
    if let Some(parent) = database_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create database directory {}", parent.display()))?;
        }
    }

    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", config.database_path))
        .with_context(|| format!("invalid database path {}", config.database_path))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("connect to SQLite")?;

    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .context("run database migrations")?;

    bootstrap::seed_admin(
        &pool,
        config.admin_username.as_str(),
        config.admin_password.as_deref(),
    )
    .await
    .context("seed default admin user")?;

    let auth = AuthService::new(config.jwt_secret.clone(), config.jwt_ttl_hours);
    let daemon = DaemonClient::new(
        config.daemon_url.clone(),
        config.daemon_shared_secret.clone(),
    )
    .context("build daemon client")?;

    Ok(AppState {
        pool,
        auth,
        daemon,
        config: Arc::new(config),
    })
}
