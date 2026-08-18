//! HTTP routing and process startup for the Cipher server.

use std::io;

use axum::{Json, Router, extract::ws::WebSocketUpgrade, response::IntoResponse, routing::get};
use cipher_types::ServiceStatus;

use crate::config::ServerConfig;

pub mod config;

/// Builds the server's HTTP router.
pub fn app() -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(readiness))
        .route("/v1/realtime", get(realtime))
}

/// Binds the configured address and serves the Cipher API.
///
/// Returns I/O errors raised while binding or serving.
pub async fn run(config: ServerConfig) -> io::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(
        bind = %config.bind,
        region = %config.aws.region,
        api_origin = %config.endpoints.api_origin,
        "Cipher backend listening"
    );
    axum::serve(listener, app()).await
}

async fn health() -> Json<ServiceStatus> {
    Json(ServiceStatus::ready())
}

async fn readiness() -> Json<ServiceStatus> {
    Json(ServiceStatus::ready())
}

async fn realtime(upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(|_socket| async move {
        tracing::debug!("WebSocket connected");
    })
}

#[cfg(test)]
#[path = "tests/http.rs"]
mod tests;
