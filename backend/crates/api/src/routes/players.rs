//! `/api/v1/servers/{id}/command` and semantic player actions.
//!
//! Everything here ultimately becomes an RCON command executed by the daemon.
//! Inputs are validated and re-assembled server-side (never concatenated
//! blindly) so a username or reason cannot smuggle extra commands.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use cluster_shared::{PlayerAccessLists, PlayerAccessUpdate, PlayerHistoryEntry};
use serde::{Deserialize, Serialize};

use crate::{error::ApiError, routes::servers, state::AppState};

const MAX_COMMAND_CHARS: usize = 1024;
const MAX_USERNAME_CHARS: usize = 16;
const MAX_REASON_CHARS: usize = 256;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/servers/{id}/players", get(list_players))
        .route("/servers/{id}/players/history", get(list_player_history))
        .route("/servers/{id}/players/access", get(get_player_access))
        .route(
            "/servers/{id}/players/{username}/access",
            put(update_player_access),
        )
        .route("/servers/{id}/command", post(command))
        .route("/servers/{id}/players/{username}/kick", post(kick))
        .route("/servers/{id}/players/{username}/ban", post(ban))
        .route("/servers/{id}/players/{username}/op", post(op))
}

#[derive(Debug, Deserialize)]
struct CommandRequest {
    command: String,
}

#[derive(Debug, Deserialize, Default)]
struct PlayerActionRequest {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct CommandResult {
    command: String,
    output: String,
}

#[derive(Debug, Clone, Copy)]
enum PlayerAction {
    Kick,
    Ban,
    Op,
}

async fn list_players(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<String>>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    if server.status != "running" {
        return Err(ApiError::Conflict(
            "the server must be running to list players".into(),
        ));
    }
    let players = state.daemon.get_players(&server.id).await?;
    Ok(Json(players))
}

async fn command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<CommandRequest>,
) -> Result<Json<CommandResult>, ApiError> {
    let command = validate_raw_command(&request.command)?;
    Ok(Json(run(&state, &id, command).await?))
}

/// Historical roster from `usercache.json`; safe while the server is stopped.
async fn list_player_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<PlayerHistoryEntry>>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    let history = state.daemon.get_player_history(&server.id).await?;
    Ok(Json(history))
}

async fn get_player_access(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PlayerAccessLists>, ApiError> {
    let server = servers::fetch(&state.pool, &id).await?;
    let lists = state.daemon.get_player_access(&server.id).await?;
    Ok(Json(lists))
}

async fn update_player_access(
    State(state): State<AppState>,
    Path((id, username)): Path<(String, String)>,
    Json(update): Json<PlayerAccessUpdate>,
) -> Result<Json<PlayerAccessLists>, ApiError> {
    let username = validate_username(&username)?;
    let server = servers::fetch(&state.pool, &id).await?;
    let lists = state
        .daemon
        .update_player_access(&server.id, &username, update)
        .await?;
    Ok(Json(lists))
}

async fn kick(
    State(state): State<AppState>,
    Path((id, username)): Path<(String, String)>,
    Json(request): Json<PlayerActionRequest>,
) -> Result<Json<CommandResult>, ApiError> {
    let reason = validate_reason(request.reason.as_deref())?;
    let command = player_command(PlayerAction::Kick, &username, reason.as_deref())?;
    Ok(Json(run(&state, &id, command).await?))
}

async fn ban(
    State(state): State<AppState>,
    Path((id, username)): Path<(String, String)>,
    Json(request): Json<PlayerActionRequest>,
) -> Result<Json<CommandResult>, ApiError> {
    let reason = validate_reason(request.reason.as_deref())?;
    let command = player_command(PlayerAction::Ban, &username, reason.as_deref())?;
    Ok(Json(run(&state, &id, command).await?))
}

async fn op(
    State(state): State<AppState>,
    Path((id, username)): Path<(String, String)>,
) -> Result<Json<CommandResult>, ApiError> {
    let command = player_command(PlayerAction::Op, &username, None)?;
    Ok(Json(run(&state, &id, command).await?))
}

async fn run(
    state: &AppState,
    server_id: &str,
    command: String,
) -> Result<CommandResult, ApiError> {
    let server = servers::fetch(&state.pool, server_id).await?;
    if server.status != "running" {
        return Err(ApiError::Conflict(
            "the server must be running to accept commands".into(),
        ));
    }

    let output = state.daemon.run_command(&server.id, &command).await?;
    Ok(CommandResult { command, output })
}

/// Translate a semantic action into the vanilla command. The username is
/// validated before interpolation; the reason is a plain trailing argument.
fn player_command(
    action: PlayerAction,
    username: &str,
    reason: Option<&str>,
) -> Result<String, ApiError> {
    let username = validate_username(username)?;
    let mut command = match action {
        PlayerAction::Kick => format!("kick {username}"),
        PlayerAction::Ban => format!("ban {username}"),
        PlayerAction::Op => format!("op {username}"),
    };

    if let Some(reason) = reason {
        command.push(' ');
        command.push_str(reason);
    }
    Ok(command)
}

fn validate_username(raw: &str) -> Result<String, ApiError> {
    let username = raw.trim();
    let length = username.chars().count();
    if length == 0 || length > MAX_USERNAME_CHARS {
        return Err(ApiError::BadRequest(format!(
            "username must be between 1 and {MAX_USERNAME_CHARS} characters"
        )));
    }
    if !username
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(ApiError::BadRequest(
            "username may only contain letters, digits and underscores".into(),
        ));
    }
    Ok(username.to_string())
}

fn validate_reason(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let reason = raw.trim();
    if reason.is_empty() {
        return Ok(None);
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        return Err(ApiError::BadRequest(format!(
            "reason must be at most {MAX_REASON_CHARS} characters"
        )));
    }
    if reason.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "reason must not contain control characters".into(),
        ));
    }
    Ok(Some(reason.to_string()))
}

fn validate_raw_command(raw: &str) -> Result<String, ApiError> {
    let command = raw.trim().trim_start_matches('/').trim();
    if command.is_empty() {
        return Err(ApiError::BadRequest("command must not be empty".into()));
    }
    if command.chars().count() > MAX_COMMAND_CHARS {
        return Err(ApiError::BadRequest(format!(
            "command must be at most {MAX_COMMAND_CHARS} characters"
        )));
    }
    if command.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "command must not contain control characters".into(),
        ));
    }
    Ok(command.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_player_actions() {
        assert_eq!(
            player_command(PlayerAction::Kick, "Steve", Some("being rude")).expect("kick"),
            "kick Steve being rude"
        );
        assert_eq!(
            player_command(PlayerAction::Ban, "Alex", None).expect("ban"),
            "ban Alex"
        );
        assert_eq!(
            player_command(PlayerAction::Op, "Notch", None).expect("op"),
            "op Notch"
        );
    }

    #[test]
    fn rejects_command_injection_in_usernames() {
        assert!(player_command(PlayerAction::Op, "Steve stop", None).is_err());
        assert!(player_command(PlayerAction::Op, "Steve\nstop", None).is_err());
        assert!(player_command(PlayerAction::Op, "", None).is_err());
        assert!(player_command(PlayerAction::Op, "a".repeat(17).as_str(), None).is_err());
    }

    #[test]
    fn normalises_reasons_and_rejects_controls() {
        assert_eq!(
            validate_reason(Some("  spam  ")).expect("reason"),
            Some("spam".into())
        );
        assert_eq!(validate_reason(Some("   ")).expect("blank"), None);
        assert_eq!(validate_reason(None).expect("none"), None);
        assert!(validate_reason(Some("line\nbreak")).is_err());
        assert!(validate_reason(Some(&"a".repeat(MAX_REASON_CHARS + 1))).is_err());
    }

    #[test]
    fn normalises_raw_commands() {
        assert_eq!(validate_raw_command(" /say hi ").expect("valid"), "say hi");
        assert!(validate_raw_command("  ").is_err());
        assert!(validate_raw_command("say hi\nop Steve").is_err());
        assert!(validate_raw_command(&"a".repeat(MAX_COMMAND_CHARS + 1)).is_err());
    }
}
