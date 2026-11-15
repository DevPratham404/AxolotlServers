//! Protected control-plane routes (`/api/v1/*`).
//!
//! Milestone 1 provides the authenticated identity probe; server management
//! routes land in the next slice.

use axum::{Extension, Json, Router, routing::get};

use crate::{auth::AuthUser, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(me))
        .merge(super::servers::router())
        .merge(super::players::router())
        .merge(super::properties::router())
        .merge(super::files::router())
        .merge(super::addons::router())
}

/// Returns the principal behind the presented token.
async fn me(Extension(user): Extension<AuthUser>) -> Json<AuthUser> {
    Json(user)
}
