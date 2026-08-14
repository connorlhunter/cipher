use std::{io, net::SocketAddr};

use axum::{Json, Router, extract::ws::WebSocketUpgrade, response::IntoResponse, routing::get};
use cipher_types::ServiceStatus;

pub mod config;

pub fn app() -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(readiness))
        .route("/v1/realtime", get(realtime))
}

pub async fn run(bind: SocketAddr) -> io::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "Cipher backend listening");
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
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::app;

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.status().is_success());
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "{\"status\":\"ok\"}"
        );
    }
}
