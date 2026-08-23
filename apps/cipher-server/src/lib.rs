//! HTTP routing and process startup for the Cipher server.

use std::io;

use axum::{
    Json, Router,
    extract::ws::WebSocketUpgrade,
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
};
use cipher_types::ServiceStatus;

use crate::config::ServerConfig;

pub mod auth;
pub mod config;
pub mod http_contract;

/// Builds the server's HTTP router.
pub fn app() -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(readiness))
        .route("/v1", get(api_descriptor))
        .route("/v1/", get(api_descriptor))
        .route("/v1/realtime", get(realtime))
        .fallback(api_fallback)
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

async fn api_descriptor() -> Response {
    http_contract::success(
        ApiDescriptor {
            api_version: http_contract::API_VERSION,
            media_type: "application/json",
        },
        http_contract::ResponseMeta::with_request_id(http_contract::new_request_id()),
    )
}

async fn realtime(upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(|_socket| async move {
        tracing::debug!("WebSocket connected");
    })
}

async fn api_fallback(uri: Uri) -> Response {
    match requested_api_version(uri.path()) {
        Some(http_contract::API_VERSION) => http_contract::failure(
            http_contract::ApiErrorCode::NotFound,
            http_contract::ResponseMeta::with_request_id(http_contract::new_request_id()),
        ),
        Some(_) => http_contract::failure(
            http_contract::ApiErrorCode::UnsupportedVersion,
            http_contract::ResponseMeta::with_request_id(http_contract::new_request_id()),
        ),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn requested_api_version(path: &str) -> Option<&str> {
    let segment = path.strip_prefix('/')?.split('/').next()?;
    segment
        .strip_prefix('v')
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .map(|_| segment)
}

#[derive(serde::Serialize)]
struct ApiDescriptor {
    #[serde(rename = "apiVersion")]
    api_version: &'static str,
    #[serde(rename = "mediaType")]
    media_type: &'static str,
}

#[cfg(test)]
#[path = "tests/http.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/http_contract.rs"]
mod http_contract_tests;
