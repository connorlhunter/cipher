use std::net::SocketAddr;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{HeaderMap, Request},
};
use http_body_util::BodyExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;

use super::{app, authenticated_app, run_with_authorizer};
use crate::auth::{AuthenticationError, CipherPrincipal, RequestAuthorizer, VerifiedIdentity};
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
async fn authenticated_router_denies_a_missing_bearer_token_before_serving_v1() {
    let response = authenticated_app(Arc::new(AllowOnlyKnownToken))
        .oneshot(Request::builder().uri("/v1").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_router_uses_the_same_principal_path_for_v1() {
    let response = authenticated_app(Arc::new(AllowOnlyKnownToken))
        .oneshot(
            Request::builder()
                .uri("/v1")
                .header("authorization", "Bearer signed.jwt.value")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(response.status().is_success());
}

#[tokio::test]
async fn authenticated_router_revokes_the_current_session_idempotently() {
    let response = authenticated_app(Arc::new(AllowOnlyKnownToken))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/session/revoke")
                .header("authorization", "Bearer signed.jwt.value")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(response.status().is_success());
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["data"]["revoked"], true);
    assert_eq!(body["meta"]["apiVersion"], "v1");
    assert!(
        body["meta"]["requestId"]
            .as_str()
            .is_some_and(|request_id| request_id.starts_with("req_"))
    );
}

#[tokio::test]
async fn session_revocation_requires_the_same_valid_bearer_token() {
    let response = authenticated_app(Arc::new(AllowOnlyKnownToken))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/session/revoke")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn realtime_endpoint_accepts_a_websocket_upgrade() {
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = reservation.local_addr().unwrap();
    drop(reservation);
    let task = tokio::spawn(run_with_authorizer(
        test_config(bind),
        Arc::new(AllowOnlyKnownToken),
    ));
    let mut stream = connect_when_ready(bind).await;

    stream
        .write_all(
            b"GET /v1/realtime HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer signed.jwt.value\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
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
async fn realtime_endpoint_denies_an_upgrade_without_an_authorized_principal() {
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = reservation.local_addr().unwrap();
    drop(reservation);
    let task = tokio::spawn(run_with_authorizer(
        test_config(bind),
        Arc::new(AllowOnlyKnownToken),
    ));
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
            .starts_with("HTTP/1.1 401 Unauthorized")
    );

    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn run_binds_the_configured_address() {
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = reservation.local_addr().unwrap();
    drop(reservation);

    let task = tokio::spawn(run_with_authorizer(
        test_config(bind),
        Arc::new(AllowOnlyKnownToken),
    ));
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

struct AllowOnlyKnownToken;

impl RequestAuthorizer for AllowOnlyKnownToken {
    fn authorize_request(
        &self,
        headers: &HeaderMap,
        _unix_time_seconds: i64,
    ) -> Result<CipherPrincipal, AuthenticationError> {
        if headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            != Some("Bearer signed.jwt.value")
        {
            return Err(AuthenticationError::InvalidToken);
        }
        Ok(CipherPrincipal {
            identity: VerifiedIdentity::new("sub_123", "origin_123", 1_800_000_000).unwrap(),
            user_id: "usr_123".into(),
            device_id: "dev_123".into(),
            session_id: "ses_123".into(),
        })
    }

    fn revoke_current_session(
        &self,
        headers: &HeaderMap,
        unix_time_seconds: i64,
    ) -> Result<(), AuthenticationError> {
        self.authorize_request(headers, unix_time_seconds)
            .map(|_| ())
    }
}
