//! JWT authentication middleware for protected routes.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, header},
    middleware::Next,
    response::Response,
};

use crate::{auth::AuthUser, error::ApiError, state::AppState};

/// Paths a user may still call while a password change is pending. Anything
/// else is rejected so a leaked bootstrap password cannot manage servers.
///
/// The protected router is nested at `/api/v1`, which strips that prefix before
/// middleware sees the request URI, so the identity probe appears as `/me`.
pub const PASSWORD_PENDING_ALLOWED_PATHS: [&str; 2] = ["/me", "/api/v1/me"];

/// Full guard: validates the token and blocks control actions until the user
/// has rotated a flagged password.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let user = authenticate(&state, request.headers())?;

    if user.requires_password_change && !is_allowed_while_password_pending(request.uri().path()) {
        return Err(ApiError::PasswordChangeRequired);
    }

    request.extensions_mut().insert::<AuthUser>(user);
    Ok(next.run(request).await)
}

/// Validates the token but does **not** enforce the password-change flag.
/// Used only by `/api/auth/change-password`.
pub async fn require_auth_pending_ok(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let user = authenticate(&state, request.headers())?;
    request.extensions_mut().insert::<AuthUser>(user);
    Ok(next.run(request).await)
}

fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<AuthUser, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(extract_bearer)
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".into()))?;

    state.auth.verify(token)
}

pub fn is_allowed_while_password_pending(path: &str) -> bool {
    PASSWORD_PENDING_ALLOWED_PATHS.contains(&path)
}

fn extract_bearer(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") {
        let token = token.trim();
        (!token.is_empty()).then_some(token)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_bearer, is_allowed_while_password_pending};

    #[test]
    fn parses_bearer_tokens() {
        assert_eq!(extract_bearer("Bearer abc.def"), Some("abc.def"));
        assert_eq!(extract_bearer("bearer abc"), Some("abc"));
        assert_eq!(extract_bearer("Basic abc"), None);
        assert_eq!(extract_bearer("Bearer   "), None);
        assert_eq!(extract_bearer("abc"), None);
    }

    #[test]
    fn only_identity_probe_is_allowed_while_pending() {
        assert!(is_allowed_while_password_pending("/me"));
        assert!(is_allowed_while_password_pending("/api/v1/me"));
        assert!(!is_allowed_while_password_pending("/servers"));
        assert!(!is_allowed_while_password_pending("/api/v1/servers"));
        assert!(!is_allowed_while_password_pending(
            "/api/v1/servers/abc/start"
        ));
    }
}
