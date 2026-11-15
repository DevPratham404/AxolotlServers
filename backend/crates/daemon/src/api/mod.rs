//! HMAC-authenticated daemon control API.

pub mod auth;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use bollard::Docker;

use crate::config::DaemonConfig;

#[derive(Clone)]
pub struct DaemonState {
    pub docker: Docker,
    pub config: Arc<DaemonConfig>,
    pub shared_secret: Arc<Vec<u8>>,
}

pub fn router(state: DaemonState) -> Router {
    let protected = routes::router().layer(axum::middleware::from_fn_with_state(
        state.clone(),
        auth::require_hmac,
    ));

    Router::new()
        .route("/healthz", axum::routing::get(routes::health))
        .merge(protected)
        .with_state(state)
}
