//! Native configuration and session establishment for the desktop Cognito flow.
//!
//! This module owns public Cognito configuration, token validation, and the
//! transition into the native session store. The webview receives only a small
//! display state; credentials, tokens, subjects, and refresh material never
//! cross this boundary.

use std::{
    cmp,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;

use crate::{
    auth::{
        AuthenticationRequest, AuthenticationView, AuthenticationViewState, AwsCognitoProvider,
        CognitoChallengeStep, CognitoProvider, NativeAuthError, NativeAuthErrorCode,
        NativeAuthOutcome, NativeCognitoAuthenticator,
    },
    cognito::{CognitoAccessTokenValidator, CognitoTokenPolicy, JwksSource, JwksSourceError},
    credential_store::SecretBytes,
    session::{
        DesktopSessionService, SessionGrant, SessionRefresh, SessionRefresher, SupportedSession,
    },
};
use cipher_native_transport::{
    NativeTransportError, NativeTransportErrorCode, OperationCancellation,
};

const COGNITO_REQUIRED_SCOPE: &str = "aws.cognito.signin.user.admin";
const JWKS_MAX_AGE: Duration = Duration::from_secs(5 * 60);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REGION_BYTES: usize = 64;
const MAX_POOL_ID_BYTES: usize = 128;
const MAX_CLIENT_ID_BYTES: usize = 128;

/// Public Cognito values embedded by the desktop packaging environment.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CognitoDesktopConfiguration {
    region: String,
    user_pool_id: String,
    client_id: String,
}

impl CognitoDesktopConfiguration {
    fn from_environment() -> Result<Self, NativeAuthError> {
        let region = std::env::var("CIPHER_COGNITO_REGION")
            .or_else(|_| std::env::var("CIPHER_AWS_REGION"))
            .map_err(|_| unavailable())?;
        let user_pool_id =
            std::env::var("CIPHER_COGNITO_USER_POOL_ID").map_err(|_| unavailable())?;
        let client_id = std::env::var("CIPHER_COGNITO_CLIENT_ID").map_err(|_| unavailable())?;
        let configuration = Self {
            region,
            user_pool_id,
            client_id,
        };
        configuration.validate()?;
        Ok(configuration)
    }

    fn validate(&self) -> Result<(), NativeAuthError> {
        let valid_region = !self.region.is_empty()
            && self.region.len() <= MAX_REGION_BYTES
            && self
                .region
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        let expected_prefix = format!("{}_", self.region);
        let valid_pool = self.user_pool_id.starts_with(&expected_prefix)
            && self.user_pool_id.len() <= MAX_POOL_ID_BYTES
            && self
                .user_pool_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        let valid_client = !self.client_id.is_empty()
            && self.client_id.len() <= MAX_CLIENT_ID_BYTES
            && self
                .client_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric());
        if valid_region && valid_pool && valid_client {
            Ok(())
        } else {
            Err(unavailable())
        }
    }

    fn issuer(&self) -> String {
        format!(
            "https://cognito-idp.{}.amazonaws.com/{}",
            self.region, self.user_pool_id
        )
    }

    fn jwks_url(&self) -> String {
        format!("{}/.well-known/jwks.json", self.issuer())
    }
}

struct HttpJwksSource {
    client: Client,
    endpoint: String,
}

impl HttpJwksSource {
    fn new(endpoint: String) -> Result<Self, NativeAuthError> {
        let client = Client::builder()
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_REQUEST_TIMEOUT)
            .https_only(true)
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self { client, endpoint })
    }
}

impl JwksSource for HttpJwksSource {
    fn fetch_jwks(&self) -> Result<String, JwksSourceError> {
        let response = self
            .client
            .get(&self.endpoint)
            .send()
            .map_err(|_| JwksSourceError::Unavailable)?;
        if !response.status().is_success() {
            return Err(JwksSourceError::Unavailable);
        }
        response
            .text()
            .map_err(|_| JwksSourceError::InvalidResponse)
    }
}

struct AuthenticationRuntime {
    authenticator: NativeCognitoAuthenticator,
    provider: Arc<AwsCognitoProvider>,
    validator: Arc<Mutex<CognitoAccessTokenValidator<HttpJwksSource>>>,
}

impl AuthenticationRuntime {
    fn new(configuration: CognitoDesktopConfiguration) -> Result<Self, NativeAuthError> {
        let provider = Arc::new(AwsCognitoProvider::new(
            &configuration.region,
            &configuration.client_id,
        )?);
        let authenticator = NativeCognitoAuthenticator::new(
            &configuration.user_pool_id,
            Arc::clone(&provider) as Arc<dyn CognitoProvider>,
        )?;
        let issuer = configuration.issuer();
        let jwks_url = configuration.jwks_url();
        let policy = CognitoTokenPolicy::new(
            issuer,
            configuration.client_id.clone(),
            [COGNITO_REQUIRED_SCOPE],
            JWKS_MAX_AGE,
        )
        .map_err(|_| unavailable())?;
        let validator = Arc::new(Mutex::new(CognitoAccessTokenValidator::new(
            policy,
            HttpJwksSource::new(jwks_url)?,
        )));
        Ok(Self {
            authenticator,
            provider,
            validator,
        })
    }

    fn refresher(&self) -> CognitoSessionRefresher {
        CognitoSessionRefresher {
            provider: Arc::clone(&self.provider),
            validator: Arc::clone(&self.validator),
        }
    }
}

struct CognitoSessionRefresher {
    provider: Arc<AwsCognitoProvider>,
    validator: Arc<Mutex<CognitoAccessTokenValidator<HttpJwksSource>>>,
}

impl SessionRefresher for CognitoSessionRefresher {
    fn refresh(
        &self,
        supported_session: &SupportedSession,
        refresh_material: &SecretBytes,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRefresh, NativeTransportError> {
        ensure_not_cancelled(cancellation)?;
        let refresh = tauri::async_runtime::block_on(self.provider.refresh(refresh_material))
            .map_err(map_auth_error)?;
        ensure_not_cancelled(cancellation)?;
        let now = unix_time_seconds().map_err(map_auth_error)?;
        let validated = self
            .validator
            .lock()
            .map_err(|_| unavailable())
            .and_then(|mut validator| {
                validator
                    .validate_at(refresh.access_token().as_bytes(), now)
                    .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))
            })
            .map_err(map_auth_error)?;
        let valid_for = bounded_token_lifetime(validated.expires_at(), now, refresh.valid_for())?;
        let (subject, access_token) = validated.into_parts();
        let refreshed_scope = SupportedSession::new(subject.as_str())
            .map_err(|_| NativeTransportError::new(NativeTransportErrorCode::Unauthenticated))?;
        if &refreshed_scope != supported_session {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::Unauthenticated,
            ));
        }
        ensure_not_cancelled(cancellation)?;
        SessionRefresh::new(access_token, valid_for).map_err(map_session_error)
    }
}

/// Owns the native sign-in boundary and keeps unavailable configuration fail-closed.
pub struct DesktopAuthenticationService {
    runtime: Result<AuthenticationRuntime, NativeAuthError>,
    pending_challenge: Mutex<Option<CognitoChallengeStep>>,
}

impl DesktopAuthenticationService {
    /// Creates the process-wide authentication service from public deployment configuration.
    pub fn new() -> Self {
        Self {
            runtime: CognitoDesktopConfiguration::from_environment()
                .and_then(AuthenticationRuntime::new),
            pending_challenge: Mutex::new(None),
        }
    }

    /// Installs the validated native refresh boundary when desktop Cognito is configured.
    pub fn install_refresher(
        &self,
        session: &DesktopSessionService,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeAuthError> {
        let Ok(runtime) = &self.runtime else {
            return Ok(());
        };
        session
            .install_refresher(Arc::new(runtime.refresher()), cancellation)
            .map(|_| ())
            .map_err(map_session_to_auth_error)
    }

    /// Performs one bounded credential submission and establishes a native session on success.
    pub async fn authenticate(
        &self,
        request: &AuthenticationRequest,
        session: &DesktopSessionService,
        cancellation: &OperationCancellation,
    ) -> AuthenticationView {
        let runtime = match &self.runtime {
            Ok(runtime) => runtime,
            Err(error) => return failure(*error),
        };
        match request {
            AuthenticationRequest::BeginPasswordReset { identifier } => {
                return match runtime
                    .authenticator
                    .begin_password_reset(identifier, cancellation)
                    .await
                {
                    Ok(()) => AuthenticationView {
                        state: AuthenticationViewState::PasswordResetRequired,
                        message: "If an account is eligible, a recovery code is on its way.",
                    },
                    Err(error) => failure(error),
                };
            }
            AuthenticationRequest::ConfirmPasswordReset {
                identifier,
                code,
                new_password,
            } => {
                return match runtime
                    .authenticator
                    .confirm_password_reset(identifier, code, new_password, cancellation)
                    .await
                {
                    Ok(()) => AuthenticationView {
                        state: AuthenticationViewState::PasswordResetComplete,
                        message: "Password updated. Sign in with your new password.",
                    },
                    Err(error) => failure(error),
                };
            }
            _ => {}
        }
        if let Err(error) = self.install_refresher(session, cancellation) {
            return failure(error);
        }
        let result = match request {
            AuthenticationRequest::ContinueChallenge { code } => {
                let challenge = match self.pending_challenge.lock() {
                    Ok(mut pending) => pending.take(),
                    Err(_) => return failure(unavailable()),
                };
                let Some(challenge) = challenge else {
                    return failure(NativeAuthError::new(NativeAuthErrorCode::InvalidRequest));
                };
                let result = runtime
                    .authenticator
                    .continue_challenge(&challenge, code, cancellation)
                    .await;
                if result.is_err()
                    && let Ok(mut pending) = self.pending_challenge.lock()
                    && pending.is_none()
                {
                    *pending = Some(challenge);
                }
                result
            }
            _ => {
                runtime
                    .authenticator
                    .authenticate(request, cancellation)
                    .await
            }
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => return failure(error),
        };
        let tokens = match outcome {
            NativeAuthOutcome::Authenticated(tokens) => {
                if let Ok(mut pending) = self.pending_challenge.lock() {
                    *pending = None;
                }
                tokens
            }
            NativeAuthOutcome::Challenge(challenge) => {
                let Ok(mut pending) = self.pending_challenge.lock() else {
                    return failure(unavailable());
                };
                *pending = Some(challenge);
                return AuthenticationView {
                    state: AuthenticationViewState::ChallengeRequired,
                    message: "Enter the verification code to continue.",
                };
            }
        };

        let now = match unix_time_seconds() {
            Ok(now) => now,
            Err(error) => return failure(error),
        };
        let validated = match runtime
            .validator
            .lock()
            .map_err(|_| unavailable())
            .and_then(|mut validator| {
                validator
                    .validate_at(tokens.access_token().as_bytes(), now)
                    .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))
            }) {
            Ok(validated) => validated,
            Err(error) => return failure(error),
        };
        let remaining = match validated
            .expires_at()
            .checked_sub(now)
            .and_then(|seconds| u64::try_from(seconds).ok())
        {
            Some(seconds) if seconds > 0 => Duration::from_secs(seconds),
            _ => return failure(NativeAuthError::new(NativeAuthErrorCode::InvalidResponse)),
        };
        let (subject, access_token) = validated.into_parts();
        let supported_session = match SupportedSession::new(subject.as_str()) {
            Ok(supported_session) => supported_session,
            Err(_) => return failure(NativeAuthError::new(NativeAuthErrorCode::InvalidResponse)),
        };
        let valid_for = cmp::min(remaining, tokens.valid_for());
        let refresh_material = tokens.into_refresh_material();
        let grant = match SessionGrant::new(access_token, refresh_material, valid_for) {
            Ok(grant) => grant,
            Err(_) => return failure(NativeAuthError::new(NativeAuthErrorCode::InvalidResponse)),
        };
        match session.establish(supported_session, grant, cancellation) {
            Ok(()) => AuthenticationView {
                state: AuthenticationViewState::Authenticated,
                message: "Signed in securely.",
            },
            Err(error) => failure_from_session(error),
        }
    }
}

impl Default for DesktopAuthenticationService {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_time_seconds() -> Result<i64, NativeAuthError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable())?
        .as_secs();
    i64::try_from(seconds).map_err(|_| unavailable())
}

fn bounded_token_lifetime(
    expires_at: i64,
    now: i64,
    declared_lifetime: Duration,
) -> Result<Duration, NativeTransportError> {
    let remaining = expires_at
        .checked_sub(now)
        .and_then(|seconds| u64::try_from(seconds).ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unauthenticated))?;
    Ok(cmp::min(remaining, declared_lifetime))
}

fn ensure_not_cancelled(cancellation: &OperationCancellation) -> Result<(), NativeTransportError> {
    if cancellation.is_cancelled() {
        Err(NativeTransportError::new(
            NativeTransportErrorCode::Cancelled,
        ))
    } else {
        Ok(())
    }
}

fn map_auth_error(error: NativeAuthError) -> NativeTransportError {
    let code = match error.code() {
        NativeAuthErrorCode::Cancelled => NativeTransportErrorCode::Cancelled,
        NativeAuthErrorCode::InvalidCredentials
        | NativeAuthErrorCode::InvalidRequest
        | NativeAuthErrorCode::InvalidResponse
        | NativeAuthErrorCode::InvitationCompletionUncertain
        | NativeAuthErrorCode::PasswordRejected => NativeTransportErrorCode::Unauthenticated,
        NativeAuthErrorCode::RateLimited | NativeAuthErrorCode::Unavailable => {
            NativeTransportErrorCode::Unavailable
        }
    };
    NativeTransportError::new(code)
}

fn map_session_error(error: crate::session::NativeSessionError) -> NativeTransportError {
    let code = match error {
        crate::session::NativeSessionError::Cancelled => NativeTransportErrorCode::Cancelled,
        crate::session::NativeSessionError::InvalidLifetime => {
            NativeTransportErrorCode::InvalidRequest
        }
        crate::session::NativeSessionError::NoSession
        | crate::session::NativeSessionError::ReauthenticationRequired => {
            NativeTransportErrorCode::Unauthenticated
        }
        crate::session::NativeSessionError::Unavailable => NativeTransportErrorCode::Unavailable,
    };
    NativeTransportError::new(code)
}

fn failure(error: NativeAuthError) -> AuthenticationView {
    AuthenticationView {
        state: AuthenticationViewState::Failed,
        message: error.message(),
    }
}

fn failure_from_session(error: crate::session::NativeSessionError) -> AuthenticationView {
    let code = match error {
        crate::session::NativeSessionError::Cancelled => NativeAuthErrorCode::Cancelled,
        crate::session::NativeSessionError::InvalidLifetime => NativeAuthErrorCode::InvalidResponse,
        crate::session::NativeSessionError::NoSession
        | crate::session::NativeSessionError::ReauthenticationRequired => {
            NativeAuthErrorCode::InvalidCredentials
        }
        crate::session::NativeSessionError::Unavailable => NativeAuthErrorCode::Unavailable,
    };
    failure(NativeAuthError::new(code))
}

fn map_session_to_auth_error(error: crate::session::NativeSessionError) -> NativeAuthError {
    let code = match error {
        crate::session::NativeSessionError::Cancelled => NativeAuthErrorCode::Cancelled,
        crate::session::NativeSessionError::InvalidLifetime => NativeAuthErrorCode::InvalidResponse,
        crate::session::NativeSessionError::NoSession
        | crate::session::NativeSessionError::ReauthenticationRequired => {
            NativeAuthErrorCode::InvalidCredentials
        }
        crate::session::NativeSessionError::Unavailable => NativeAuthErrorCode::Unavailable,
    };
    NativeAuthError::new(code)
}

const fn unavailable() -> NativeAuthError {
    NativeAuthError::new(NativeAuthErrorCode::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::{CognitoDesktopConfiguration, bounded_token_lifetime, map_auth_error, unavailable};
    use crate::auth::{NativeAuthError, NativeAuthErrorCode};
    use cipher_native_transport::NativeTransportErrorCode;
    use std::time::Duration;

    #[test]
    fn public_cognito_configuration_is_bounded_and_issuer_scoped() {
        let configuration = CognitoDesktopConfiguration {
            region: "us-east-1".into(),
            user_pool_id: "us-east-1_examplePool".into(),
            client_id: "publicclient123".into(),
        };
        assert!(configuration.validate().is_ok());
        assert_eq!(
            configuration.issuer(),
            "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_examplePool"
        );
        assert_eq!(
            configuration.jwks_url(),
            "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_examplePool/.well-known/jwks.json"
        );

        let invalid = CognitoDesktopConfiguration {
            user_pool_id: "us-west-2_otherPool".into(),
            ..configuration
        };
        assert_eq!(invalid.validate(), Err(unavailable()));
    }

    #[test]
    fn refresh_lifetime_never_outlives_the_validated_token() {
        assert_eq!(
            bounded_token_lifetime(1_010, 1_000, Duration::from_secs(15)).unwrap(),
            Duration::from_secs(10)
        );
        assert_eq!(
            bounded_token_lifetime(1_000, 1_000, Duration::from_secs(15))
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unauthenticated
        );
    }

    #[test]
    fn refresh_error_mapping_stays_bounded() {
        assert_eq!(
            map_auth_error(NativeAuthError::new(
                NativeAuthErrorCode::InvalidCredentials
            ))
            .code(),
            NativeTransportErrorCode::Unauthenticated
        );
        assert_eq!(
            map_auth_error(NativeAuthError::new(NativeAuthErrorCode::Unavailable)).code(),
            NativeTransportErrorCode::Unavailable
        );
    }
}
