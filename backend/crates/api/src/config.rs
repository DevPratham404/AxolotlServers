use std::env;

/// Runtime configuration for the control plane, sourced from the environment.
///
/// Every value has a development-safe default except `JWT_SECRET`, which is
/// required in production (see `validate`).
#[derive(Debug, Clone)]
pub struct Config {
    /// `API_ADDR`, default `0.0.0.0:8080`.
    pub addr: String,
    /// `DATABASE_PATH`, default `./data/control.db`.
    pub database_path: String,
    /// `JWT_SECRET`; must be overridden outside local development.
    pub jwt_secret: String,
    /// `JWT_TTL_HOURS`, default 24.
    pub jwt_ttl_hours: i64,
    /// `ADMIN_PASSWORD` used once when seeding the first admin. When absent a
    /// random password is generated and logged.
    pub admin_password: Option<String>,
    /// `ADMIN_USERNAME`, default `admin`.
    pub admin_username: String,
    /// `DAEMON_URL`, default `http://127.0.0.1:9090`.
    pub daemon_url: String,
    /// `DAEMON_SHARED_SECRET`; must match the daemon.
    pub daemon_shared_secret: String,
}

const DEV_JWT_SECRET: &str = "dev-only-insecure-secret";
const DEV_DAEMON_SECRET: &str = "dev-only-daemon-secret";

impl Config {
    pub fn from_env() -> Self {
        let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| DEV_JWT_SECRET.to_string());
        if jwt_secret == DEV_JWT_SECRET {
            tracing::warn!("JWT_SECRET is unset — using an insecure development default");
        }

        let jwt_ttl_hours = env::var("JWT_TTL_HOURS")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|hours| *hours > 0)
            .unwrap_or(24);

        let daemon_shared_secret =
            env::var("DAEMON_SHARED_SECRET").unwrap_or_else(|_| DEV_DAEMON_SECRET.to_string());
        if daemon_shared_secret == DEV_DAEMON_SECRET {
            tracing::warn!("DAEMON_SHARED_SECRET is unset — using an insecure development default");
        }

        Self {
            addr: env::var("API_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_path: env::var("DATABASE_PATH").unwrap_or_else(|_| "./data/control.db".into()),
            jwt_secret,
            jwt_ttl_hours,
            admin_password: env::var("ADMIN_PASSWORD")
                .ok()
                .filter(|value| !value.is_empty()),
            admin_username: env::var("ADMIN_USERNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "admin".into()),
            daemon_url: env::var("DAEMON_URL").unwrap_or_else(|_| "http://127.0.0.1:9090".into()),
            daemon_shared_secret,
        }
    }

    /// True when the insecure development JWT secret is in use.
    pub fn uses_dev_jwt_secret(&self) -> bool {
        self.jwt_secret == DEV_JWT_SECRET
    }
}
