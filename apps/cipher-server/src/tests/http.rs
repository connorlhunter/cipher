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
