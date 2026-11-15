use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use cluster_shared::ApiErrorBody;

/// Daemon-side error type. Messages are safe to return to the (trusted)
/// control plane; they never include host paths beyond the volume root.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("Docker Engine unavailable: {0}")]
    Unavailable(String),
    #[error(transparent)]
    Docker(#[from] bollard::errors::Error),
    #[error("{0}")]
    Internal(String),
}

impl DaemonError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Docker(error) => match docker_status_code(error) {
                Some(404) => StatusCode::NOT_FOUND,
                Some(400 | 409 | 422) => StatusCode::BAD_REQUEST,
                _ => StatusCode::BAD_GATEWAY,
            },
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(_) => "unauthorized",
            Self::BadRequest(_) => "bad_request",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Unavailable(_) => "docker_unavailable",
            Self::Internal(_) => "internal_error",
            Self::Docker(_) => "docker_error",
        }
    }
}

impl IntoResponse for DaemonError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(%status, error = %self, "daemon request failed");
        } else {
            tracing::warn!(%status, error = %self, "daemon request rejected");
        }

        let body = ApiErrorBody::new(self.code(), self.to_string());
        (status, Json(body)).into_response()
    }
}

impl From<DaemonError> for String {
    fn from(error: DaemonError) -> Self {
        error.to_string()
    }
}

pub fn docker_status_code(error: &bollard::errors::Error) -> Option<u16> {
    match error {
        bollard::errors::Error::DockerResponseServerError { status_code, .. } => Some(*status_code),
        _ => None,
    }
}

pub fn is_not_found(error: &bollard::errors::Error) -> bool {
    docker_status_code(error) == Some(404)
}

pub fn is_not_modified(error: &bollard::errors::Error) -> bool {
    docker_status_code(error) == Some(304)
}

pub fn is_conflict(error: &bollard::errors::Error) -> bool {
    docker_status_code(error) == Some(409)
}
