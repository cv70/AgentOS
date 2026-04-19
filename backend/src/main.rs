mod api;
mod config;
mod domain;
mod error;
mod executor;
mod runtime;
mod state;
mod storage;

use axum::{Router, routing::get};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::config::config::{AppConfig, parse_config_path_from_args};
use crate::error::AppResult;
use crate::runtime::agent_runtime::AgentRuntime;
use crate::state::AppState;

#[tokio::main]
async fn main() -> AppResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let config = match parse_config_path_from_args(std::env::args()) {
        Some(path) => AppConfig::load_from_path(&path)?,
        None => AppConfig::load()?,
    };

    let runtime = AgentRuntime::new(config.clone()).await?;
    let state = AppState { runtime };

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .nest("/api/v1", api::v1::routes::routes())
        .with_state(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("AgentOS backend listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(())
}
