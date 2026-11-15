//! Shared-secret HMAC guard for every `/daemon/*` route.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, header},
    middleware::Next,
    response::Response,
};
use cluster_shared::{
    HMAC_BODY_SHA256_HEADER, HMAC_SIGNATURE_HEADER, HMAC_TIMESTAMP_HEADER, verify, verify_digest,
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

use crate::{api::DaemonState, error::DaemonError, files, imports};

/// Refuse oversized control payloads before hashing them.
const MAX_BODY_BYTES: usize = 256 * 1024;

/// Path whose body is streamed and verified against a pre-computed digest.
const STREAMED_IMPORT_PATH: &str = "/daemon/servers/import";

pub async fn require_hmac(
    State(state): State<DaemonState>,
    request: Request,
    next: Next,
) -> Result<Response, DaemonError> {
    let (parts, body) = request.into_parts();

    if parts.method == Method::POST && parts.uri.path() == STREAMED_IMPORT_PATH {
        return require_streamed_hmac(state, parts, body, next).await;
    }

    // Binary uploads are the one route allowed to carry large bodies; everything
    // else stays on the small control-payload budget.
    let limit = if matches!(parts.method, Method::POST | Method::PUT)
        && parts.uri.path().contains("/raw-files/")
    {
        files::MAX_UPLOAD_BYTES
    } else {
        MAX_BODY_BYTES
    };

    let bytes = to_bytes(body, limit)
        .await
        .map_err(|error| DaemonError::BadRequest(format!("unreadable body: {error}")))?;

    let method = parts.method.as_str().to_string();
    let path = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());

    let timestamp = timestamp_of(&parts)?;
    let signature = header_of(&parts, HMAC_SIGNATURE_HEADER)?;

    verify(
        state.shared_secret.as_slice(),
        &method,
        &path,
        timestamp,
        &bytes,
        signature,
        now_unix(),
    )
    .map_err(|error| DaemonError::Unauthorized(error.to_string()))?;

    tracing::debug!(%method, %path, "verified daemon request signature");

    // Preserve the verified bytes for the handler; content type is untouched.
    let request = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(request).await)
}

/// Import uploads can be multi-gigabyte, so the API signs a pre-computed body
/// digest instead of buffering the body. The signature is checked before the
/// handler runs, and the body is hashed while it streams to catch tampering or
/// corruption. The streamed byte count is capped independently of headers.
async fn require_streamed_hmac(
    state: DaemonState,
    parts: axum::http::request::Parts,
    body: Body,
    next: Next,
) -> Result<Response, DaemonError> {
    let method = parts.method.as_str().to_string();
    let path = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());
    let timestamp = timestamp_of(&parts)?;
    let signature = header_of(&parts, HMAC_SIGNATURE_HEADER)?.to_string();
    let expected_digest = header_of(&parts, HMAC_BODY_SHA256_HEADER)?
        .trim()
        .to_ascii_lowercase();

    if let Some(length) = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if length > imports::MAX_IMPORT_BYTES as u64 {
            return Err(DaemonError::BadRequest(format!(
                "import body is larger than the {} byte limit",
                imports::MAX_IMPORT_BYTES
            )));
        }
    }

    verify_digest(
        state.shared_secret.as_slice(),
        &method,
        &path,
        timestamp,
        &expected_digest,
        &signature,
        now_unix(),
    )
    .map_err(|error| DaemonError::Unauthorized(error.to_string()))?;

    let stream = body.into_data_stream();
    let checked = futures_util::stream::try_unfold(
        (stream, Sha256::new(), 0u64, expected_digest),
        |(mut stream, mut hasher, mut total, expected)| async move {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    total = total.saturating_add(chunk.len() as u64);
                    if total > imports::MAX_IMPORT_BYTES as u64 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "import body exceeds the size limit",
                        ));
                    }
                    hasher.update(&chunk);
                    Ok(Some((chunk, (stream, hasher, total, expected))))
                }
                Some(Err(error)) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("read import body: {error}"),
                )),
                None => {
                    let actual = hex::encode(hasher.finalize());
                    if actual != expected {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "import body digest does not match its signature",
                        ));
                    }
                    Ok(None)
                }
            }
        },
    );

    tracing::debug!(%method, %path, "verified streamed daemon request signature");
    let request = Request::from_parts(parts, Body::from_stream(checked));
    Ok(next.run(request).await)
}

fn header_of<'a>(
    parts: &'a axum::http::request::Parts,
    name: &str,
) -> Result<&'a str, DaemonError> {
    parts
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| DaemonError::Unauthorized(format!("missing {name}")))
}

fn timestamp_of(parts: &axum::http::request::Parts) -> Result<i64, DaemonError> {
    header_of(parts, HMAC_TIMESTAMP_HEADER)?
        .parse::<i64>()
        .map_err(|_| {
            DaemonError::Unauthorized(format!("missing or invalid {HMAC_TIMESTAMP_HEADER}"))
        })
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
