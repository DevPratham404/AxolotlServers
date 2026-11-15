use std::env;

const DEV_SHARED_SECRET: &str = "dev-only-daemon-secret";

/// Daemon configuration. The Docker endpoint comes from the standard
/// `DOCKER_HOST` / local defaults handled by bollard.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// `DAEMON_ADDR`, default `127.0.0.1:9090` (never expose this publicly).
    pub addr: String,
    /// `DAEMON_SHARED_SECRET` — must match the control plane.
    pub shared_secret: String,
    /// `MC_NETWORK`, default `mc-net`.
    pub network: String,
    /// `MC_IMAGE`, default `itzg/minecraft-server:latest`.
    pub image: String,
    /// `MC_BEDROCK_IMAGE`, dedicated image for Bedrock Edition servers.
    pub bedrock_image: String,
    /// `MC_DATA_ROOT` — host directory bind-mounted at `/data`.
    pub data_root: String,
}

impl DaemonConfig {
    pub fn from_env() -> Self {
        let shared_secret =
            env::var("DAEMON_SHARED_SECRET").unwrap_or_else(|_| DEV_SHARED_SECRET.to_string());
        if shared_secret == DEV_SHARED_SECRET {
            tracing::warn!("DAEMON_SHARED_SECRET is unset — using an insecure development default");
        }

        Self {
            addr: env::var("DAEMON_ADDR").unwrap_or_else(|_| "127.0.0.1:9090".into()),
            shared_secret,
            network: env::var("MC_NETWORK").unwrap_or_else(|_| "mc-net".into()),
            image: env::var("MC_IMAGE").unwrap_or_else(|_| "itzg/minecraft-server:latest".into()),
            bedrock_image: env::var("MC_BEDROCK_IMAGE")
                .unwrap_or_else(|_| "itzg/minecraft-bedrock-server:latest".into()),
            data_root: env::var("MC_DATA_ROOT").unwrap_or_else(|_| "./data/servers".into()),
        }
    }

    /// Ensure the data root exists and is absolute (Docker bind mounts require
    /// absolute host paths).
    pub fn resolve_data_root(&mut self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.data_root)
            .map_err(|error| anyhow::anyhow!("create MC_DATA_ROOT {}: {error}", self.data_root))?;
        self.data_root = std::fs::canonicalize(&self.data_root)
            .map_err(|error| anyhow::anyhow!("resolve MC_DATA_ROOT {}: {error}", self.data_root))?
            .to_string_lossy()
            .into_owned();
        Ok(())
    }
}
