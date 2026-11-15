use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde_json::{Value, json};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness))
}

/// Liveness: the process is up. Never depends on external resources.
async fn liveness() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness: dependencies are usable. Only the database is wired so far.
async fn readiness(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let database_ok = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    let status = if database_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(json!({
            "status": if database_ok { "ok" } else { "degraded" },
            "checks": {
                "database": if database_ok { "ok" } else { "unavailable" },
                "docker": "not_configured"
            }
        })),
    )
}
