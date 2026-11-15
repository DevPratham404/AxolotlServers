//! `/api/auth` — login and password rotation.

use axum::{Extension, Json, Router, extract::State, middleware, routing::post};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AuthUser, hash_password, verify_password},
    error::ApiError,
    middleware::require_auth_pending_ok,
    state::AppState,
};

const MIN_PASSWORD_LEN: usize = 8;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new().route("/login", post(login)).route(
        "/change-password",
        post(change_password).route_layer(middleware::from_fn_with_state(
            state,
            require_auth_pending_ok,
        )),
    )
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub token_type: &'static str,
    /// Unix seconds.
    pub expires_at: i64,
    pub user: AuthUser,
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let username = request.username.trim();
    if username.is_empty() || request.password.is_empty() {
        return Err(ApiError::BadRequest(
            "username and password are required".into(),
        ));
    }

    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT id, password_hash, requires_password_change FROM users WHERE username = ?1",
    )
    .bind(username)
    .fetch_optional(&state.pool)
    .await?;

    // Same error for unknown user and wrong password: no account enumeration.
    let Some((id, password_hash, requires_password_change)) = row else {
        return Err(ApiError::Unauthorized(
            "invalid username or password".into(),
        ));
    };

    let password = request.password;
    let valid = verify_password_blocking(password, password_hash).await?;
    if !valid {
        return Err(ApiError::Unauthorized(
            "invalid username or password".into(),
        ));
    }

    let user = AuthUser {
        id,
        username: username.to_string(),
        requires_password_change: requires_password_change != 0,
    };

    tracing::info!(user_id = %user.id, username = %user.username, "login succeeded");
    Ok(Json(issue_response(&state, user)?))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Rotates the caller's password and returns a fresh token without the
/// `requires_password_change` flag.
async fn change_password(
    State(state): State<AppState>,
    Extension(caller): Extension<AuthUser>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    if request.new_password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::BadRequest(format!(
            "new password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if request.new_password == request.current_password {
        return Err(ApiError::BadRequest(
            "new password must differ from the current password".into(),
        ));
    }

    let row: Option<(String,)> = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?1")
        .bind(&caller.id)
        .fetch_optional(&state.pool)
        .await?;

    let Some((password_hash,)) = row else {
        return Err(ApiError::Unauthorized("account no longer exists".into()));
    };

    if !verify_password_blocking(request.current_password, password_hash).await? {
        return Err(ApiError::Unauthorized(
            "current password is incorrect".into(),
        ));
    }

    let new_password = request.new_password;
    let new_hash = tokio::task::spawn_blocking(move || hash_password(&new_password))
        .await
        .map_err(|error| ApiError::Internal(format!("password hashing task: {error}")))??;

    sqlx::query("UPDATE users SET password_hash = ?1, requires_password_change = 0 WHERE id = ?2")
        .bind(&new_hash)
        .bind(&caller.id)
        .execute(&state.pool)
        .await?;

    tracing::info!(user_id = %caller.id, "password changed");

    let user = AuthUser {
        requires_password_change: false,
        ..caller
    };
    Ok(Json(issue_response(&state, user)?))
}

fn issue_response(state: &AppState, user: AuthUser) -> Result<AuthResponse, ApiError> {
    let (token, expires_at) = state.auth.issue(&user)?;
    Ok(AuthResponse {
        token,
        token_type: "Bearer",
        expires_at,
        user,
    })
}

async fn verify_password_blocking(password: String, hash: String) -> Result<bool, ApiError> {
    tokio::task::spawn_blocking(move || verify_password(&password, &hash))
        .await
        .map_err(|error| ApiError::Internal(format!("password verification task: {error}")))
}
