use std::sync::Arc;

use cluster_daemon::{
    api::{self, DaemonState},
    config::DaemonConfig,
    docker, telemetry,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    telemetry::init();

    let mut config = DaemonConfig::from_env();
    config.resolve_data_root()?;

    if config.shared_secret.is_empty() {
        anyhow::bail!("DAEMON_SHARED_SECRET must not be empty");
    }

    let shared_secret = Arc::new(config.shared_secret.clone().into_bytes());
    let docker = docker::connect()?;

    match docker::ping(&docker).await {
        Ok(version) => tracing::info!(docker_version = %version, "Docker Engine reachable"),
        Err(error) => tracing::warn!(
            %error,
            "Docker Engine unavailable — the daemon will accept control requests but container \
             operations will fail until it is running"
        ),
    }

    let addr = config.addr.clone();
    let state = DaemonState {
        docker,
        config: Arc::new(config),
        shared_secret,
    };
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(
        %addr,
        protocol = cluster_shared::CONTROL_PROTOCOL_VERSION,
        "cluster-daemon listening"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("cluster-daemon stopped");
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install Ctrl+C handler");
        return;
    }
    tracing::info!("shutdown signal received");
}
