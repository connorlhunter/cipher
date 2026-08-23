//! HTTP routing and process startup for the Cipher server.

use std::{
    io,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Extension, Json, Router,
    extract::ws::WebSocketUpgrade,
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use cipher_types::ServiceStatus;

use crate::{
    auth::{AuthenticationError, RequestAuthorizer},
    config::ServerConfig,
};

#[cfg(not(coverage))]
use crate::auth::{
    CognitoJwtValidator, CognitoTokenPolicy, DynamoPrincipalStore, HttpJwksSource,
    ServerAuthorizer, StoredPrincipalGate,
};

#[cfg(not(coverage))]
const COGNITO_REQUIRED_SCOPE: &str = "aws.cognito.signin.user.admin";
#[cfg(not(coverage))]
const JWKS_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(5 * 60);

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

/// Builds a router whose versioned HTTP and realtime entry points share one
/// default-deny principal path. Health probes intentionally remain public.
pub fn authenticated_app(authorizer: Arc<dyn RequestAuthorizer>) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(readiness))
        .route("/v1", get(authenticated_api_descriptor))
        .route("/v1/", get(authenticated_api_descriptor))
        .route("/v1/session/revoke", post(authenticated_session_revoke))
        .route("/v1/realtime", get(authenticated_realtime))
        .fallback(api_fallback)
        .layer(Extension(authorizer))
}

/// Binds the configured address and serves the Cipher API.
///
/// Returns I/O errors raised while binding or serving.
#[cfg(not(coverage))]
pub async fn run(config: ServerConfig) -> io::Result<()> {
    let authorizer = production_authorizer(&config).await?;
    run_with_authorizer(config, authorizer).await
}

#[cfg(coverage)]
/// Fails closed because deterministic coverage cannot contact production AWS services.
pub async fn run(_config: ServerConfig) -> io::Result<()> {
    Err(io::Error::other(
        "production AWS dependencies are unavailable in deterministic coverage",
    ))
}

/// Binds the configured address with the supplied shared request authorizer.
pub async fn run_with_authorizer(
    config: ServerConfig,
    authorizer: Arc<dyn RequestAuthorizer>,
) -> io::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(
        bind = %config.bind,
        region = %config.aws.region,
        api_origin = %config.endpoints.api_origin,
        "Cipher backend listening"
    );
    axum::serve(listener, authenticated_app(authorizer)).await
}

#[cfg(not(coverage))]
async fn production_authorizer(config: &ServerConfig) -> io::Result<Arc<dyn RequestAuthorizer>> {
    let issuer = format!(
        "https://cognito-idp.{}.amazonaws.com/{}",
        config.aws.region, config.aws.cognito_user_pool_id
    );
    let policy = CognitoTokenPolicy::new(
        issuer.clone(),
        config.aws.cognito_client_id.clone(),
        [COGNITO_REQUIRED_SCOPE],
        JWKS_MAX_AGE,
    )
    .map_err(|_| io::Error::other("invalid production authentication policy"))?;
    let source = HttpJwksSource::new(&issuer)
        .map_err(|_| io::Error::other("invalid Cognito JWKS endpoint"))?;
    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let store = DynamoPrincipalStore::new(
        aws_sdk_dynamodb::Client::new(&sdk_config),
        config.aws.users_table.clone(),
    )
    .map_err(|_| io::Error::other("invalid users-table configuration"))?;
    Ok(Arc::new(ServerAuthorizer::new(
        CognitoJwtValidator::new(policy, source),
        StoredPrincipalGate::new(store.clone()),
        store,
    )))
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

async fn authenticated_api_descriptor(
    headers: axum::http::HeaderMap,
    Extension(authorizer): Extension<Arc<dyn RequestAuthorizer>>,
) -> Response {
    match authorize(&headers, authorizer.as_ref()) {
        Ok(_) => api_descriptor().await,
        Err(response) => *response,
    }
}

/// Idempotently revokes the Cipher application session for the supplied valid
/// Cognito access-token family. This endpoint intentionally remains available
/// after a previous successful revocation so a desktop can safely finish an
/// interrupted logout cleanup attempt.
async fn authenticated_session_revoke(
    headers: axum::http::HeaderMap,
    Extension(authorizer): Extension<Arc<dyn RequestAuthorizer>>,
) -> Response {
    match revoke_current_session(&headers, authorizer.as_ref()) {
        Ok(()) => http_contract::success(
            SessionRevocation { revoked: true },
            http_contract::ResponseMeta::with_request_id(http_contract::new_request_id()),
        ),
        Err(response) => *response,
    }
}

async fn realtime(upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(|_socket| async move {
        tracing::debug!("WebSocket connected");
    })
}

async fn authenticated_realtime(
    headers: axum::http::HeaderMap,
    Extension(authorizer): Extension<Arc<dyn RequestAuthorizer>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    match authorize(&headers, authorizer.as_ref()) {
        Ok(_) => realtime(upgrade).await.into_response(),
        Err(response) => *response,
    }
}

fn authorize(
    headers: &axum::http::HeaderMap,
    authorizer: &dyn RequestAuthorizer,
) -> Result<crate::auth::CipherPrincipal, Box<Response>> {
    with_authorization_time(|now| authorizer.authorize_request(headers, now))
}

fn revoke_current_session(
    headers: &axum::http::HeaderMap,
    authorizer: &dyn RequestAuthorizer,
) -> Result<(), Box<Response>> {
    with_authorization_time(|now| authorizer.revoke_current_session(headers, now))
}

fn with_authorization_time<T>(
    action: impl FnOnce(i64) -> Result<T, AuthenticationError>,
) -> Result<T, Box<Response>> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            Box::new(http_contract::failure(
                http_contract::ApiErrorCode::Unavailable,
                http_contract::ResponseMeta::with_request_id(http_contract::new_request_id()),
            ))
        })?
        .as_secs();
    let now = i64::try_from(now).map_err(|_| {
        Box::new(http_contract::failure(
            http_contract::ApiErrorCode::Unavailable,
            http_contract::ResponseMeta::with_request_id(http_contract::new_request_id()),
        ))
    })?;
    action(now).map_err(|error| {
        let code = match error {
            AuthenticationError::SigningKeysUnavailable => http_contract::ApiErrorCode::Unavailable,
            AuthenticationError::MissingToken
            | AuthenticationError::InvalidToken
            | AuthenticationError::Revoked => http_contract::ApiErrorCode::Unauthenticated,
        };
        Box::new(http_contract::failure(
            code,
            http_contract::ResponseMeta::with_request_id(http_contract::new_request_id()),
        ))
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

#[derive(serde::Serialize)]
struct SessionRevocation {
    revoked: bool,
}

#[cfg(test)]
#[path = "tests/http.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/http_contract.rs"]
mod http_contract_tests;
