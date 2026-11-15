//! Browser-facing telemetry socket.
//!
//! `GET /api/v1/ws/servers/{id}` upgrades to a WebSocket, then multiplexes the
//! daemon's stats and log NDJSON streams into a single JSON envelope stream.
//! Browsers cannot set `Authorization` on the upgrade request, so the JWT is
//! accepted either as `?token=` or as a `bearer.<jwt>` WebSocket subprotocol.

use axum::{
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
};
use cluster_shared::{LogLine, StatsSample, TelemetryMessage};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::{auth::AuthUser, error::ApiError, routes::servers, state::AppState};

const SUBPROTOCOL_PREFIX: &str = "bearer.";
const MAX_LOG_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    token: Option<String>,
}

pub async fn server_socket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (token, selected_protocol) = extract_token(&headers, query.token)?;
    let user = state.auth.verify(&token)?;

    if user.requires_password_change {
        return Err(ApiError::PasswordChangeRequired);
    }

    // A missing server is a 404 before the socket is upgraded, which lets the
    // browser surface a normal API error instead of an opaque close frame.
    servers::fetch(&state.pool, &id).await?;

    let mut upgrade = ws;
    if let Some(protocol) = selected_protocol {
        upgrade = upgrade.protocols([protocol]);
    }

    Ok(upgrade.on_upgrade(move |socket| handle_socket(socket, state, user, id)))
}

/// Prefer the query parameter (used by the SPA); fall back to the
/// `bearer.<jwt>` subprotocol so non-browser clients can avoid URLs with tokens.
fn extract_token(
    headers: &HeaderMap,
    query_token: Option<String>,
) -> Result<(String, Option<String>), ApiError> {
    if let Some(token) = query_token.filter(|value| !value.trim().is_empty()) {
        return Ok((token, None));
    }

    let protocols = headers
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    for candidate in protocols.split(',').map(str::trim) {
        if let Some(token) = candidate.strip_prefix(SUBPROTOCOL_PREFIX) {
            if !token.is_empty() {
                return Ok((token.to_string(), Some(candidate.to_string())));
            }
        }
    }

    Err(ApiError::Unauthorized(
        "missing websocket token (use ?token= or the bearer.<jwt> subprotocol)".into(),
    ))
}

async fn handle_socket(socket: WebSocket, state: AppState, user: AuthUser, server_id: String) {
    tracing::debug!(server_id = %server_id, user = %user.username, "telemetry socket opened");

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<TelemetryMessage>(256);

    let ready = TelemetryMessage::Ready {
        server_id: server_id.clone(),
    };
    if tx.send(ready).await.is_err() {
        return;
    }

    let stats_task = {
        let state = state.clone();
        let id = server_id.clone();
        let tx = tx.clone();
        tokio::spawn(async move { forward_stream(state, id, StreamKind::Stats, tx).await })
    };
    let logs_task = {
        let state = state.clone();
        let id = server_id.clone();
        let tx = tx.clone();
        tokio::spawn(async move { forward_stream(state, id, StreamKind::Logs, tx).await })
    };
    drop(tx);

    loop {
        tokio::select! {
            message = rx.recv() => {
                let Some(message) = message else { break };
                let Ok(encoded) = serde_json::to_string(&message) else {
                    continue;
                };
                if sender.send(Message::Text(encoded.into())).await.is_err() {
                    break;
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = sender.send(Message::Pong(payload)).await;
                    }
                    Some(Err(error)) => {
                        tracing::debug!(server_id = %server_id, %error, "telemetry socket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    stats_task.abort();
    logs_task.abort();
    tracing::debug!(server_id = %server_id, "telemetry socket closed");
}

#[derive(Clone, Copy)]
enum StreamKind {
    Stats,
    Logs,
}

impl StreamKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stats => "stats",
            Self::Logs => "logs",
        }
    }
}

/// Consume one daemon NDJSON stream and forward decoded items onto the socket.
async fn forward_stream(
    state: AppState,
    server_id: String,
    kind: StreamKind,
    tx: mpsc::Sender<TelemetryMessage>,
) {
    let path = format!("/daemon/servers/{server_id}/{}", kind.as_str());

    let stream = match state.daemon.open_stream(&path).await {
        Ok(stream) => stream,
        Err(error) => {
            let _ = tx
                .send(TelemetryMessage::Error {
                    code: "daemon_stream_unavailable".into(),
                    message: public_message(error),
                })
                .await;
            return;
        }
    };

    tokio::pin!(stream);
    let mut buffer: Vec<u8> = Vec::with_capacity(4096);

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                buffer.extend_from_slice(&bytes);

                while let Some(index) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=index).collect();
                    let line = &line[..line.len() - 1];
                    if line.is_empty() {
                        continue;
                    }
                    if line.len() > MAX_LOG_LINE_BYTES {
                        tracing::warn!(server_id = %server_id, "dropping oversized telemetry line");
                        continue;
                    }

                    let Some(message) = decode_line(kind, line) else {
                        continue;
                    };
                    if tx.send(message).await.is_err() {
                        return;
                    }
                }
            }
            Err(error) => {
                tracing::warn!(server_id = %server_id, stream = kind.as_str(), %error, "daemon stream failed");
                let _ = tx
                    .send(TelemetryMessage::Error {
                        code: "daemon_stream_failed".into(),
                        message: format!("{} stream failed", kind.as_str()),
                    })
                    .await;
                break;
            }
        }
    }

    let _ = tx
        .send(TelemetryMessage::StreamEnded {
            stream: kind.as_str().to_string(),
        })
        .await;
}

fn decode_line(kind: StreamKind, line: &[u8]) -> Option<TelemetryMessage> {
    match kind {
        StreamKind::Stats => serde_json::from_slice::<StatsSample>(line)
            .ok()
            .map(|sample| TelemetryMessage::Stats { sample }),
        StreamKind::Logs => serde_json::from_slice::<LogLine>(line)
            .ok()
            .map(|line| TelemetryMessage::Log { line }),
    }
}

fn public_message(error: ApiError) -> String {
    match error {
        ApiError::NotFound(message) | ApiError::BadRequest(message) => message,
        ApiError::Unavailable(_) | ApiError::Internal(_) => "daemon unavailable".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn query_token_wins() {
        let headers = HeaderMap::new();
        let (token, protocol) =
            extract_token(&headers, Some("abc.def".into())).expect("query token");
        assert_eq!(token, "abc.def");
        assert!(protocol.is_none());
    }

    #[test]
    fn falls_back_to_subprotocol() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "sec-websocket-protocol",
            HeaderValue::from_static("chat, bearer.abc.def"),
        );
        let (token, protocol) = extract_token(&headers, None).expect("subprotocol token");
        assert_eq!(token, "abc.def");
        assert_eq!(protocol.as_deref(), Some("bearer.abc.def"));
    }

    #[test]
    fn rejects_missing_tokens() {
        let headers = HeaderMap::new();
        assert!(extract_token(&headers, None).is_err());
        assert!(extract_token(&headers, Some("  ".into())).is_err());
    }
}
