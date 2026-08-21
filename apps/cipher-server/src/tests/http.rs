use std::net::SocketAddr;

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;

use super::{app, run};
use crate::config::{AwsConfig, PublicEndpoints, ServerConfig};

fn test_config(bind: SocketAddr) -> ServerConfig {
    ServerConfig {
        bind,
        aws: AwsConfig {
            region: "us-east-1".into(),
            account_id: "123456789012".into(),
            cognito_user_pool_id: "us-east-1_a1B2c3D4e".into(),
            cognito_client_id: "1a2b3c4d5e6f7g8h9i".into(),
            users_table: "cipher-users".into(),
            conversations_table: "cipher-conversations".into(),
            messages_table: "cipher-messages".into(),
            media_table: "cipher-media".into(),
            media_bucket: "cipher-123456789012-media".into(),
        },
        endpoints: PublicEndpoints {
            api_origin: "https://cipher.connorhunter.me".into(),
            realtime_url: "wss://cipher.connorhunter.me/v1/realtime".into(),
        },
    }
}

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

#[tokio::test]
async fn readiness_endpoint_returns_ok() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/readyz")
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

#[tokio::test]
async fn realtime_endpoint_accepts_a_websocket_upgrade() {
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = reservation.local_addr().unwrap();
    drop(reservation);
    let task = tokio::spawn(run(test_config(bind)));
    let mut stream = connect_when_ready(bind).await;

    stream
        .write_all(
            b"GET /v1/realtime HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        )
        .await
        .unwrap();
    let mut response = [0; 1024];
    let length = stream.read(&mut response).await.unwrap();

    assert!(
        std::str::from_utf8(&response[..length])
            .unwrap()
            .starts_with("HTTP/1.1 101 Switching Protocols")
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn run_binds_the_configured_address() {
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = reservation.local_addr().unwrap();
    drop(reservation);

    let task = tokio::spawn(run(test_config(bind)));
    let connection = connect_when_ready(bind).await;

    drop(connection);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

async fn connect_when_ready(bind: SocketAddr) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = TcpStream::connect(bind).await {
            return stream;
        }
        tokio::task::yield_now().await;
    }
    panic!("server did not bind {bind}");
}
