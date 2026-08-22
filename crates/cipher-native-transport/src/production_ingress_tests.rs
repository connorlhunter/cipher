use std::{env, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_native_tls::TlsConnector;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{HeaderValue, StatusCode, header::AUTHORIZATION},
    },
};

use super::{AccessToken, NativeHttpOrigin, NativeRealtimeOrigin};

const API_ORIGIN: &str = "https://cipher.connorhunter.me";
const REALTIME_URL: &str = "wss://cipher.connorhunter.me/v1/realtime";
const ACCESS_TOKEN_ENVIRONMENT_KEY: &str = "CIPHER_INGRESS_ACCESS_TOKEN";

fn ingress_access_token() -> AccessToken {
    let value = env::var(ACCESS_TOKEN_ENVIRONMENT_KEY).unwrap_or_else(|_| {
        panic!("{ACCESS_TOKEN_ENVIRONMENT_KEY} is required for the production ingress check.")
    });
    AccessToken::new(value)
        .unwrap_or_else(|_| panic!("{ACCESS_TOKEN_ENVIRONMENT_KEY} must be a valid access token."))
}

#[test]
fn production_ingress_uses_fixed_secure_native_origins() {
    assert_production_origins();
}

#[tokio::test]
#[ignore = "requires a deployed production ingress and an invited fixture-account access token"]
async fn production_ingress_serves_health_and_accepts_a_native_authorized_upgrade() {
    assert_production_origins();
    assert_production_health().await;

    let token = ingress_access_token();
    let mut request = REALTIME_URL.into_client_request().unwrap();
    let authorization = HeaderValue::from_str(&format!("Bearer {}", token.as_str())).unwrap();
    request.headers_mut().insert(AUTHORIZATION, authorization);

    let (connection, response) = timeout(Duration::from_secs(20), connect_async(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    drop(connection);
}

fn assert_production_origins() {
    assert!(NativeHttpOrigin::parse(API_ORIGIN).is_ok());
    let realtime_origin = NativeRealtimeOrigin::parse("wss://cipher.connorhunter.me").unwrap();
    assert_eq!(realtime_origin.connection_url(), REALTIME_URL);
}

async fn assert_production_health() {
    let stream = timeout(
        Duration::from_secs(20),
        TcpStream::connect(("cipher.connorhunter.me", 443)),
    )
    .await
    .unwrap()
    .unwrap();
    let tls = TlsConnector::from(native_tls::TlsConnector::new().unwrap());
    let mut stream = timeout(
        Duration::from_secs(20),
        tls.connect("cipher.connorhunter.me", stream),
    )
    .await
    .unwrap()
    .unwrap();
    stream
        .write_all(
            b"GET /healthz HTTP/1.1\r\nHost: cipher.connorhunter.me\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(20), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();

    assert!(response.starts_with(b"HTTP/1.1 200 "));
}
