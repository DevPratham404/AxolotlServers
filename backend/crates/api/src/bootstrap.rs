//! First-run bootstrap: create the default admin when the users table is empty.

use rand::distr::{Alphanumeric, SampleString};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::auth::hash_password;

/// Seeding is idempotent: it only runs when no user exists.
///
/// If no password is supplied, a random one is generated and logged **once**.
pub async fn seed_admin(
    pool: &SqlitePool,
    username: &str,
    password: Option<&str>,
) -> anyhow::Result<()> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .map_err(|error| anyhow::anyhow!("count users: {error}"))?;

    if user_count > 0 {
        tracing::debug!(user_count, "users table already seeded");
        return Ok(());
    }

    let generated = password.is_none();
    let password = password
        .map(str::to_owned)
        .unwrap_or_else(|| Alphanumeric.sample_string(&mut rand::rng(), 24));

    let password_for_hash = password.clone();
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password_for_hash))
        .await
        .map_err(|error| anyhow::anyhow!("password hashing task: {error}"))?
        .map_err(|error| anyhow::anyhow!("password hashing: {error}"))?;

    sqlx::query(
        "INSERT INTO users (id, username, password_hash, requires_password_change) \
         VALUES (?1, ?2, ?3, 0)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(username)
    .bind(&password_hash)
    .execute(pool)
    .await
    .map_err(|error| anyhow::anyhow!("insert admin user: {error}"))?;

    if generated {
        tracing::warn!(
            username,
            password,
            "seeded default admin user; set ADMIN_PASSWORD to control the bootstrap password"
        );
    } else {
        tracing::info!(username, "seeded default admin user from ADMIN_PASSWORD");
    }

    Ok(())
}
