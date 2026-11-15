pub mod addons;
pub mod auth;
pub mod files;
pub mod health;
pub mod players;
pub mod properties;
pub mod servers;
pub mod v1;
pub mod ws;

use axum::{Router, http::HeaderName, middleware, routing::get};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

use crate::{middleware::require_auth, state::AppState};

/// Assemble the HTTP router.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .merge(v1::router())
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    // The WebSocket upgrade carries its JWT in the query string or subprotocol,
    // so it bypasses the Authorization-header middleware and verifies itself.
    let telemetry = Router::new()
        .route("/api/v1/ws/servers/{id}", get(ws::server_socket))
        .with_state(state.clone());

    let request_id_header = HeaderName::from_static("x-request-id");

    Router::new()
        .merge(health::router())
        .nest("/api/auth", auth::router(state.clone()))
        .merge(telemetry)
        .nest("/api/v1", protected)
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::ServiceExt;

    use super::router;
    use crate::{auth::AuthService, config::Config, daemon::DaemonClient, state::AppState};

    fn test_state() -> AppState {
        let pool = SqlitePoolOptions::new()
            .connect_lazy("sqlite::memory:")
            .expect("lazy in-memory pool");
        let config = Config {
            addr: "127.0.0.1:0".into(),
            database_path: ":memory:".into(),
            jwt_secret: "test-secret".into(),
            jwt_ttl_hours: 1,
            admin_password: None,
            admin_username: "admin".into(),
            daemon_url: "http://127.0.0.1:9".into(),
            daemon_shared_secret: "test-secret".into(),
        };

        AppState {
            pool,
            auth: AuthService::new(&config.jwt_secret, config.jwt_ttl_hours),
            daemon: DaemonClient::new(&config.daemon_url, &config.daemon_shared_secret)
                .expect("daemon client"),
            config: Arc::new(config),
        }
    }

    async fn status(app: axum::Router, method: Method, uri: &str) -> StatusCode {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        app.oneshot(request)
            .await
            .expect("router response")
            .status()
    }

    /// Every M6.5 endpoint must resolve to the auth layer (401), never a
    /// route-missing 404 from a stale or incomplete router.
    #[tokio::test]
    async fn m6_5_routes_are_registered() {
        let app = router(test_state());
        let cases = [
            (Method::POST, "/api/v1/servers/import"),
            (Method::GET, "/api/v1/servers/x/players"),
            (Method::GET, "/api/v1/servers/x/players/history"),
            (Method::GET, "/api/v1/servers/x/players/access"),
            (Method::PUT, "/api/v1/servers/x/players/Steve/access"),
            (Method::POST, "/api/v1/servers/x/restart"),
            (Method::PATCH, "/api/v1/servers/x/resources"),
            (Method::POST, "/api/v1/servers/x/files/foo.txt"),
            (Method::PUT, "/api/v1/servers/x/files/foo.txt"),
            (Method::POST, "/api/v1/servers/x/files/rename"),
            (Method::POST, "/api/v1/servers/x/files/copy"),
            (Method::GET, "/api/v1/servers/x/addons"),
            (Method::POST, "/api/v1/servers/x/addons/install"),
            (Method::GET, "/api/v1/servers/x/files/foo.txt?download=true"),
            (Method::POST, "/api/v1/servers/x/directory/foo"),
            (Method::PUT, "/api/v1/servers/x/directory/foo"),
        ];

        for (method, uri) in cases {
            assert_eq!(
                status(app.clone(), method.clone(), uri).await,
                StatusCode::UNAUTHORIZED,
                "{method} {uri} should reach the auth layer"
            );
        }
    }

    /// Unsupported methods must not look like missing routes. The auth layer may
    /// answer first (401), but a 404 would signal a desynced router.
    #[tokio::test]
    async fn unsupported_methods_do_not_404() {
        let app = router(test_state());
        let cases = [
            (Method::GET, "/api/v1/servers/x/restart"),
            (Method::POST, "/api/v1/servers/x/resources"),
            (Method::DELETE, "/api/v1/servers/x/resources"),
            (Method::PUT, "/api/v1/servers/x/players"),
        ];

        for (method, uri) in cases {
            let status = status(app.clone(), method.clone(), uri).await;
            assert!(
                matches!(
                    status,
                    StatusCode::UNAUTHORIZED | StatusCode::METHOD_NOT_ALLOWED
                ),
                "{method} {uri} returned {status}; expected 401 or 405, not a missing route"
            );
        }
    }
}
