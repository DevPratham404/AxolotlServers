use cluster_api::{config::Config, routes, state, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    telemetry::init();

    let config = Config::from_env();
    let state = state::init(config).await?;
    let addr = state.config.addr.clone();
    let app = routes::router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, api_version = cluster_shared::API_VERSION, "cluster-api listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("cluster-api stopped");
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install Ctrl+C handler");
        return;
    }
    tracing::info!("shutdown signal received");
}
