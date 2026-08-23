//! Native-only Cognito administrator-invitation completion and SRP authentication.
//!
//! Credentials enter through one bounded command request, exist only for the
//! duration of a native operation, and are zeroized on drop. Cognito sessions
//! and tokens deliberately have no serialization path back to the webview.
//! This contract starts with a user already provisioned by Cognito
//! `AdminCreateUser`; it does not expose self-registration or create accounts.

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use cipher_native_transport::OperationCancellation;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::credential_store::SecretBytes;

mod provider;
mod srp;

pub use provider::AwsCognitoProvider;
use srp::CognitoSrpExchange;

/// Maximum accepted byte length for a Cognito email alias.
pub const MAX_AUTH_IDENTIFIER_BYTES: usize = 320;
/// Maximum accepted byte length for a one-time credential or password.
pub const MAX_AUTH_CREDENTIAL_BYTES: usize = 1_024;
/// Maximum local attempts retained in one rolling window.
pub const MAX_LOCAL_AUTH_ATTEMPTS: usize = 5;
/// Rolling process-local window used to bound authentication retries.
pub const LOCAL_AUTH_ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
const MIN_PASSWORD_BYTES: usize = 12;
const MAX_PASSWORD_BYTES: usize = 256;
const MAX_CHALLENGE_PARAMETER_BYTES: usize = 16 * 1024;
const MAX_COGNITO_SESSION_BYTES: usize = 16 * 1024;
const PASSWORD_VERIFIER_PARAMETERS: &[&str] = &[
    "SALT",
    "SECRET_BLOCK",
    "SRP_B",
    "USERNAME",
    "USER_ID_FOR_SRP",
];
const NEW_PASSWORD_REQUIRED_PARAMETERS: &[&str] = &["requiredAttributes", "userAttributes"];
const SOFTWARE_TOKEN_MFA_PARAMETERS: &[&str] =
    &["FRIENDLY_DEVICE_NAME", "USER_CODE_DELIVERY_SECONDS"];
const MFA_SETUP_PARAMETERS: &[&str] = &["MFAS_CAN_SETUP"];
const EMAIL_CODE_PARAMETERS: &[&str] = &[
    "CODE_DELIVERY_ATTRIBUTE",
    "CODE_DELIVERY_DELIVERY_MEDIUM",
    "CODE_DELIVERY_DESTINATION",
];

/// The two credential submissions implemented by the native SRP flow.
#[derive(Deserialize)]
#[serde(tag = "flow", rename_all = "snake_case")]
pub enum AuthenticationRequest {
    /// Signs an existing account in with its current password.
    SignIn {
        /// Cognito email alias.
        identifier: SecretText,
        /// Current Cognito password.
        password: SecretText,
    },
    /// Completes first sign-in for a user provisioned through Cognito `AdminCreateUser`.
    AcceptAdministratorInvitation {
        /// Cognito email alias named by the administrator-issued invitation.
        identifier: SecretText,
        /// Expiring Cognito temporary password delivered in the invitation.
        temporary_password: SecretText,
        /// Permanent password selected during redemption.
        new_password: SecretText,
    },
}

impl AuthenticationRequest {
    fn validate(&self) -> Result<(), NativeAuthError> {
        match self {
            Self::SignIn {
                identifier,
                password,
            } => {
                validate_identifier(identifier.as_str())?;
                validate_credential(password.as_str())
            }
            Self::AcceptAdministratorInvitation {
                identifier,
                temporary_password,
                new_password,
            } => {
                validate_identifier(identifier.as_str())?;
                validate_credential(temporary_password.as_str())?;
                validate_new_password(new_password.as_str())
            }
        }
    }

    fn srp_credential(&self) -> (&str, &str) {
        match self {
            Self::SignIn {
                identifier,
                password,
            } => (identifier.as_str(), password.as_str()),
            Self::AcceptAdministratorInvitation {
                identifier,
                temporary_password,
                ..
            } => (identifier.as_str(), temporary_password.as_str()),
        }
    }
}

/// A deserialized credential value with redacted formatting and zeroizing drop.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct SecretText(String);

impl SecretText {
    /// Borrows the value only for a Rust-owned authentication operation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretText([redacted])")
    }
}

/// A stable challenge category that never contains upstream challenge text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CognitoChallengeKind {
    /// Second step in a native SRP exchange.
    PasswordVerifier,
    /// First sign-in with an administrator-issued temporary credential.
    NewPasswordRequired,
    /// Time-based one-time-password verification.
    SoftwareTokenMfa,
    /// Time-based one-time-password enrollment.
    MfaSetup,
    /// Email verification or one-time-password entry.
    EmailCode,
    /// A challenge not enabled by Cipher's current client contract.
    Unsupported,
}

/// A bounded Cognito continuation held only by the native core.
pub struct CognitoChallengeStep {
    kind: CognitoChallengeKind,
    parameters: HashMap<String, String>,
    session: Zeroizing<String>,
}

impl CognitoChallengeStep {
    fn new(
        kind: CognitoChallengeKind,
        parameters: HashMap<String, String>,
        session: String,
    ) -> Result<Self, NativeAuthError> {
        let mut parameters = parameters;
        let mut session = session;
        if session.is_empty()
            || session.len() > MAX_COGNITO_SESSION_BYTES
            || session.bytes().any(|byte| byte.is_ascii_control())
            || !valid_challenge_parameters(kind, &parameters)
        {
            zeroize_parameters(&mut parameters);
            session.zeroize();
            return Err(NativeAuthError::new(NativeAuthErrorCode::InvalidResponse));
        }
        Ok(Self {
            kind,
            parameters,
            session: Zeroizing::new(session),
        })
    }

    /// Returns the allowlisted challenge category.
    pub const fn kind(&self) -> CognitoChallengeKind {
        self.kind
    }

    /// Borrows bounded challenge parameters for native continuation logic.
    pub fn parameters(&self) -> &HashMap<String, String> {
        &self.parameters
    }

    /// Borrows the opaque upstream session only for a native Cognito call.
    pub fn session(&self) -> &str {
        &self.session
    }
}

impl Drop for CognitoChallengeStep {
    fn drop(&mut self) {
        zeroize_parameters(&mut self.parameters);
    }
}

impl fmt::Debug for CognitoChallengeStep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CognitoChallengeStep")
            .field("kind", &self.kind)
            .field("session", &"[redacted]")
            .finish_non_exhaustive()
    }
}

/// Cognito token material returned only to the native validation/session pipeline.
pub struct CognitoTokenSet {
    access_token: Zeroizing<String>,
    refresh_material: SecretBytes,
    valid_for: Duration,
}

impl CognitoTokenSet {
    fn new(
        mut access_token: String,
        mut refresh_material: String,
        valid_for: Duration,
    ) -> Result<Self, NativeAuthError> {
        if !valid_token(&access_token, 64 * 1024)
            || !valid_token(&refresh_material, 8 * 1024)
            || valid_for.is_zero()
            || valid_for > Duration::from_secs(60 * 60)
        {
            access_token.zeroize();
            refresh_material.zeroize();
            return Err(NativeAuthError::new(NativeAuthErrorCode::InvalidResponse));
        }
        Ok(Self {
            access_token: Zeroizing::new(access_token),
            refresh_material: SecretBytes::new(refresh_material.into_bytes()),
            valid_for,
        })
    }

    /// Borrows the raw access token only for native signature and claim validation.
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// Returns the server-declared lifetime for later comparison with validated claims.
    pub const fn valid_for(&self) -> Duration {
        self.valid_for
    }

    /// Consumes the token set and yields platform-store-bound refresh material.
    pub fn into_refresh_material(self) -> SecretBytes {
        self.refresh_material
    }
}

impl fmt::Debug for CognitoTokenSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CognitoTokenSet([redacted])")
    }
}

/// A native Cognito response after one network step.
#[derive(Debug)]
pub enum CognitoAuthStep {
    /// Cognito issued tokens after all required challenges completed.
    Authenticated(CognitoTokenSet),
    /// Cognito requires another native-only challenge response.
    Challenge(CognitoChallengeStep),
}

/// Result of invitation completion or sign-in before token validation and persistence.
#[derive(Debug)]
pub enum NativeAuthOutcome {
    /// Unvalidated native tokens that must pass #65's policy before use.
    Authenticated(CognitoTokenSet),
    /// A configured follow-up challenge to be placed in the native challenge store.
    Challenge(CognitoChallengeStep),
}

/// Narrow provider boundary used by the Cognito SDK adapter and deterministic tests.
#[async_trait]
pub trait CognitoProvider: Send + Sync {
    /// Starts `USER_SRP_AUTH` with public SRP parameters.
    async fn initiate_srp(
        &self,
        parameters: HashMap<String, String>,
    ) -> Result<CognitoAuthStep, NativeAuthError>;

    /// Responds to a bounded native Cognito challenge.
    async fn respond(
        &self,
        kind: CognitoChallengeKind,
        parameters: HashMap<String, String>,
        session: &str,
    ) -> Result<CognitoAuthStep, NativeAuthError>;
}

trait AuthClock: Send + Sync {
    fn monotonic_now(&self) -> Instant;
    fn cognito_timestamp(&self) -> String;
}

struct SystemAuthClock;

impl AuthClock for SystemAuthClock {
    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }

    fn cognito_timestamp(&self) -> String {
        Utc::now().format("%a %b %-d %H:%M:%S UTC %Y").to_string()
    }
}

/// Process-local rolling authentication limiter with no account identifiers.
struct AuthAttemptLimiter {
    attempts: Mutex<VecDeque<Instant>>,
}

impl AuthAttemptLimiter {
    fn new() -> Self {
        Self {
            attempts: Mutex::new(VecDeque::with_capacity(MAX_LOCAL_AUTH_ATTEMPTS)),
        }
    }

    fn record(&self, now: Instant) -> Result<(), NativeAuthError> {
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::RateLimited))?;
        while attempts.front().is_some_and(|attempt| {
            now.saturating_duration_since(*attempt) >= LOCAL_AUTH_ATTEMPT_WINDOW
        }) {
            attempts.pop_front();
        }
        if attempts.len() >= MAX_LOCAL_AUTH_ATTEMPTS {
            return Err(NativeAuthError::new(NativeAuthErrorCode::RateLimited));
        }
        attempts.push_back(now);
        Ok(())
    }
}

/// Runs one native SRP exchange against the configured public Cognito client.
pub struct NativeCognitoAuthenticator {
    pool_id: String,
    provider: Arc<dyn CognitoProvider>,
    limiter: AuthAttemptLimiter,
    clock: Arc<dyn AuthClock>,
}

impl NativeCognitoAuthenticator {
    /// Creates an authenticator with a validated pool and provider.
    pub fn new(
        pool_id: impl Into<String>,
        provider: Arc<dyn CognitoProvider>,
    ) -> Result<Self, NativeAuthError> {
        let pool_id = pool_id.into();
        validate_pool_id(&pool_id)?;
        Ok(Self {
            pool_id,
            provider,
            limiter: AuthAttemptLimiter::new(),
            clock: Arc::new(SystemAuthClock),
        })
    }

    #[cfg(test)]
    fn with_clock(
        pool_id: impl Into<String>,
        provider: Arc<dyn CognitoProvider>,
        clock: Arc<dyn AuthClock>,
    ) -> Result<Self, NativeAuthError> {
        let mut authenticator = Self::new(pool_id, provider)?;
        authenticator.clock = clock;
        Ok(authenticator)
    }

    /// Performs administrator-invitation completion or sign-in and discards cancelled results.
    pub async fn authenticate(
        &self,
        request: &AuthenticationRequest,
        cancellation: &OperationCancellation,
    ) -> Result<NativeAuthOutcome, NativeAuthError> {
        ensure_not_cancelled(cancellation)?;
        self.limiter.record(self.clock.monotonic_now())?;
        request.validate()?;

        let (identifier, credential) = request.srp_credential();
        let exchange = CognitoSrpExchange::begin(&self.pool_id, identifier, credential)
            .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::Unavailable))?;
        ensure_not_cancelled(cancellation)?;
        let first = self
            .provider
            .initiate_srp(exchange.initial_parameters())
            .await?;
        ensure_not_cancelled(cancellation)?;
        let verifier = require_challenge(first, CognitoChallengeKind::PasswordVerifier)?;
        let canonical_username = challenge_username(verifier.parameters())?.to_owned();
        let response = exchange
            .password_verifier(verifier.parameters(), &self.clock.cognito_timestamp())
            .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))?;
        ensure_not_cancelled(cancellation)?;
        let second = self
            .provider
            .respond(
                CognitoChallengeKind::PasswordVerifier,
                response,
                verifier.session(),
            )
            .await?;
        ensure_not_cancelled(cancellation)?;

        match request {
            AuthenticationRequest::SignIn { .. } => finish_sign_in(second),
            AuthenticationRequest::AcceptAdministratorInvitation { new_password, .. } => {
                let password_challenge =
                    require_challenge(second, CognitoChallengeKind::NewPasswordRequired)?;
                validate_administrator_invitation_challenge(password_challenge.parameters())?;
                ensure_not_cancelled(cancellation)?;
                let response = HashMap::from([
                    ("USERNAME".to_owned(), canonical_username),
                    ("NEW_PASSWORD".to_owned(), new_password.as_str().to_owned()),
                ]);
                let final_step = self
                    .provider
                    .respond(
                        CognitoChallengeKind::NewPasswordRequired,
                        response,
                        password_challenge.session(),
                    )
                    .await
                    .map_err(invitation_completion_error)?;
                if cancellation.is_cancelled() {
                    return Err(invitation_completion_uncertain());
                }
                finish_authenticated_or_challenge(final_step)
            }
        }
    }
}

impl fmt::Debug for NativeCognitoAuthenticator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeCognitoAuthenticator")
            .field("configuration", &"[configured]")
            .finish_non_exhaustive()
    }
}

/// Stable native authentication error codes without upstream text or account details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAuthErrorCode {
    /// The desktop lifecycle cancelled the operation.
    Cancelled,
    /// The local request violated fixed bounds.
    InvalidRequest,
    /// The supplied sign-in or invitation credential was unusable.
    InvalidCredentials,
    /// The selected permanent password violates the configured policy.
    PasswordRejected,
    /// Local or upstream retry limits rejected this attempt.
    RateLimited,
    /// Cognito returned a malformed or contradictory response.
    InvalidResponse,
    /// Cognito might have accepted the permanent password, but no trustworthy result arrived.
    InvitationCompletionUncertain,
    /// Authentication is not configured or the service is unavailable.
    Unavailable,
}

/// A redacted native authentication error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAuthError {
    code: NativeAuthErrorCode,
}

impl NativeAuthError {
    /// Creates a redacted error from a stable category.
    pub const fn new(code: NativeAuthErrorCode) -> Self {
        Self { code }
    }

    /// Returns the stable category.
    pub const fn code(self) -> NativeAuthErrorCode {
        self.code
    }

    /// Returns a fixed message safe for a bounded renderer error state.
    pub const fn message(self) -> &'static str {
        match self.code {
            NativeAuthErrorCode::Cancelled => "The authentication attempt was cancelled.",
            NativeAuthErrorCode::InvalidRequest => "The authentication request is invalid.",
            NativeAuthErrorCode::InvalidCredentials => {
                "The sign-in or invitation could not be completed."
            }
            NativeAuthErrorCode::PasswordRejected => {
                "The new password does not meet the account requirements."
            }
            NativeAuthErrorCode::RateLimited => {
                "Too many authentication attempts were made. Try again later."
            }
            NativeAuthErrorCode::InvitationCompletionUncertain => {
                "The invitation status could not be confirmed. Sign in with the new password to continue."
            }
            NativeAuthErrorCode::InvalidResponse | NativeAuthErrorCode::Unavailable => {
                "Authentication is temporarily unavailable."
            }
        }
    }
}

impl fmt::Display for NativeAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for NativeAuthError {}

/// Bounded response shape permitted across the one-time credential command.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationView {
    /// Safe state selected by native validation and challenge handling.
    pub state: AuthenticationViewState,
    /// Fixed display text without identifiers, credentials, sessions, or tokens.
    pub message: &'static str,
}

/// Safe renderer authentication states.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationViewState {
    /// The validated native session was established.
    Authenticated,
    /// A configured verification or MFA screen must collect one bounded response.
    ChallengeRequired,
}

fn finish_sign_in(step: CognitoAuthStep) -> Result<NativeAuthOutcome, NativeAuthError> {
    if matches!(
        &step,
        CognitoAuthStep::Challenge(challenge)
            if challenge.kind() == CognitoChallengeKind::NewPasswordRequired
    ) {
        return Err(NativeAuthError::new(
            NativeAuthErrorCode::InvalidCredentials,
        ));
    }
    finish_authenticated_or_challenge(step)
}

fn finish_authenticated_or_challenge(
    step: CognitoAuthStep,
) -> Result<NativeAuthOutcome, NativeAuthError> {
    Ok(match step {
        CognitoAuthStep::Authenticated(tokens) => NativeAuthOutcome::Authenticated(tokens),
        CognitoAuthStep::Challenge(challenge) => NativeAuthOutcome::Challenge(challenge),
    })
}

fn require_challenge(
    step: CognitoAuthStep,
    expected: CognitoChallengeKind,
) -> Result<CognitoChallengeStep, NativeAuthError> {
    match step {
        CognitoAuthStep::Challenge(challenge) if challenge.kind() == expected => Ok(challenge),
        CognitoAuthStep::Authenticated(_) | CognitoAuthStep::Challenge(_) => Err(
            NativeAuthError::new(NativeAuthErrorCode::InvalidCredentials),
        ),
    }
}

fn valid_challenge_parameters(
    kind: CognitoChallengeKind,
    parameters: &HashMap<String, String>,
) -> bool {
    let Some((allowed, required)) = challenge_parameter_schema(kind) else {
        return false;
    };
    parameters.len() <= allowed.len()
        && required.iter().all(|name| parameters.contains_key(*name))
        && parameters.iter().all(|(name, value)| {
            allowed.contains(&name.as_str())
                && !value.is_empty()
                && value.len() <= challenge_parameter_limit(name)
                && !value.bytes().any(|byte| byte.is_ascii_control())
        })
}

fn challenge_parameter_schema(
    kind: CognitoChallengeKind,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    const PASSWORD_VERIFIER_REQUIRED: &[&str] =
        &["SALT", "SECRET_BLOCK", "SRP_B", "USER_ID_FOR_SRP"];
    const NEW_PASSWORD_REQUIRED_REQUIRED: &[&str] = &["requiredAttributes", "userAttributes"];
    const MFA_SETUP_REQUIRED: &[&str] = &["MFAS_CAN_SETUP"];
    const EMAIL_CODE_REQUIRED: &[&str] =
        &["CODE_DELIVERY_DELIVERY_MEDIUM", "CODE_DELIVERY_DESTINATION"];

    match kind {
        CognitoChallengeKind::PasswordVerifier => {
            Some((PASSWORD_VERIFIER_PARAMETERS, PASSWORD_VERIFIER_REQUIRED))
        }
        CognitoChallengeKind::NewPasswordRequired => Some((
            NEW_PASSWORD_REQUIRED_PARAMETERS,
            NEW_PASSWORD_REQUIRED_REQUIRED,
        )),
        CognitoChallengeKind::SoftwareTokenMfa => Some((SOFTWARE_TOKEN_MFA_PARAMETERS, &[])),
        CognitoChallengeKind::MfaSetup => Some((MFA_SETUP_PARAMETERS, MFA_SETUP_REQUIRED)),
        CognitoChallengeKind::EmailCode => Some((EMAIL_CODE_PARAMETERS, EMAIL_CODE_REQUIRED)),
        CognitoChallengeKind::Unsupported => None,
    }
}

fn challenge_parameter_limit(name: &str) -> usize {
    match name {
        "SRP_B" => 384 * 2,
        "SALT" => 512 * 2,
        "USERNAME" | "USER_ID_FOR_SRP" | "CODE_DELIVERY_DESTINATION" => MAX_AUTH_IDENTIFIER_BYTES,
        "CODE_DELIVERY_ATTRIBUTE"
        | "CODE_DELIVERY_DELIVERY_MEDIUM"
        | "FRIENDLY_DEVICE_NAME"
        | "USER_CODE_DELIVERY_SECONDS"
        | "MFAS_CAN_SETUP" => 1_024,
        "SECRET_BLOCK" | "requiredAttributes" | "userAttributes" => MAX_CHALLENGE_PARAMETER_BYTES,
        _ => 0,
    }
}

fn zeroize_parameters(parameters: &mut HashMap<String, String>) {
    for value in parameters.values_mut() {
        value.zeroize();
    }
}

fn validate_administrator_invitation_challenge(
    parameters: &HashMap<String, String>,
) -> Result<(), NativeAuthError> {
    let required_attributes = parameters
        .get("requiredAttributes")
        .ok_or_else(|| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))?;
    let required_attributes: Vec<String> = serde_json::from_str(required_attributes)
        .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))?;
    if !required_attributes.is_empty() {
        return Err(NativeAuthError::new(NativeAuthErrorCode::InvalidResponse));
    }

    let user_attributes = parameters
        .get("userAttributes")
        .ok_or_else(|| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))?;
    let mut user_attributes: HashMap<String, String> = serde_json::from_str(user_attributes)
        .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))?;
    let valid = user_attributes.len() <= 32
        && !user_attributes.iter().any(|(name, value)| {
            name.is_empty()
                || name.len() > 128
                || value.len() > MAX_CHALLENGE_PARAMETER_BYTES
                || name.bytes().any(|byte| byte.is_ascii_control())
                || value.bytes().any(|byte| byte.is_ascii_control())
        });
    zeroize_parameters(&mut user_attributes);
    if !valid {
        return Err(NativeAuthError::new(NativeAuthErrorCode::InvalidResponse));
    }
    Ok(())
}

fn invitation_completion_error(error: NativeAuthError) -> NativeAuthError {
    match error.code() {
        NativeAuthErrorCode::InvalidCredentials
        | NativeAuthErrorCode::PasswordRejected
        | NativeAuthErrorCode::RateLimited
        | NativeAuthErrorCode::InvitationCompletionUncertain => error,
        NativeAuthErrorCode::Cancelled
        | NativeAuthErrorCode::InvalidRequest
        | NativeAuthErrorCode::InvalidResponse
        | NativeAuthErrorCode::Unavailable => invitation_completion_uncertain(),
    }
}

const fn invitation_completion_uncertain() -> NativeAuthError {
    NativeAuthError::new(NativeAuthErrorCode::InvitationCompletionUncertain)
}

fn challenge_username(parameters: &HashMap<String, String>) -> Result<&str, NativeAuthError> {
    parameters
        .get("USERNAME")
        .or_else(|| parameters.get("USER_ID_FOR_SRP"))
        .map(String::as_str)
        .filter(|value| !value.is_empty() && value.len() <= MAX_AUTH_IDENTIFIER_BYTES)
        .ok_or_else(|| NativeAuthError::new(NativeAuthErrorCode::InvalidResponse))
}

fn validate_pool_id(pool_id: &str) -> Result<(), NativeAuthError> {
    let Some((region, identifier)) = pool_id.split_once('_') else {
        return Err(NativeAuthError::new(NativeAuthErrorCode::Unavailable));
    };
    if region.is_empty()
        || identifier.is_empty()
        || pool_id.len() > 128
        || !region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || !identifier.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(NativeAuthError::new(NativeAuthErrorCode::Unavailable));
    }
    Ok(())
}

fn validate_identifier(identifier: &str) -> Result<(), NativeAuthError> {
    let valid = !identifier.is_empty()
        && identifier.len() <= MAX_AUTH_IDENTIFIER_BYTES
        && identifier.trim() == identifier
        && !identifier.bytes().any(|byte| byte.is_ascii_control())
        && identifier
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
    if valid {
        Ok(())
    } else {
        Err(NativeAuthError::new(NativeAuthErrorCode::InvalidRequest))
    }
}

fn validate_credential(credential: &str) -> Result<(), NativeAuthError> {
    if credential.is_empty()
        || credential.len() > MAX_AUTH_CREDENTIAL_BYTES
        || credential.bytes().any(|byte| byte.is_ascii_control())
    {
        Err(NativeAuthError::new(NativeAuthErrorCode::InvalidRequest))
    } else {
        Ok(())
    }
}

fn validate_new_password(password: &str) -> Result<(), NativeAuthError> {
    validate_credential(password)?;
    if password.len() < MIN_PASSWORD_BYTES
        || password.len() > MAX_PASSWORD_BYTES
        || password.bytes().any(|byte| byte.is_ascii_whitespace())
        || !password.bytes().any(|byte| byte.is_ascii_lowercase())
        || !password.bytes().any(|byte| byte.is_ascii_uppercase())
        || !password.bytes().any(|byte| byte.is_ascii_digit())
        || !password
            .bytes()
            .any(|byte| byte.is_ascii_graphic() && !byte.is_ascii_alphanumeric())
    {
        Err(NativeAuthError::new(NativeAuthErrorCode::PasswordRejected))
    } else {
        Ok(())
    }
}

fn valid_token(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn ensure_not_cancelled(cancellation: &OperationCancellation) -> Result<(), NativeAuthError> {
    if cancellation.is_cancelled() {
        Err(NativeAuthError::new(NativeAuthErrorCode::Cancelled))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use async_trait::async_trait;
    use cipher_native_transport::OperationCancellation;

    use super::{
        AuthClock, AuthenticationRequest, CognitoAuthStep, CognitoChallengeKind,
        CognitoChallengeStep, CognitoProvider, CognitoTokenSet, MAX_LOCAL_AUTH_ATTEMPTS,
        NativeAuthError, NativeAuthErrorCode, NativeAuthOutcome, NativeCognitoAuthenticator,
        SecretText, SystemAuthClock, validate_administrator_invitation_challenge,
    };

    const POOL_ID: &str = "us-east-1_TestPool123";
    const EMAIL: &str = "person@example.com";
    const PASSWORD: &str = "Strong-password1!";

    struct FixedClock {
        now: Mutex<Instant>,
    }

    impl FixedClock {
        fn new() -> Self {
            Self {
                now: Mutex::new(Instant::now()),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.now.lock().unwrap();
            *now += duration;
        }
    }

    impl AuthClock for FixedClock {
        fn monotonic_now(&self) -> Instant {
            *self.now.lock().unwrap()
        }

        fn cognito_timestamp(&self) -> String {
            "Mon Feb 10 18:30:12 UTC 2025".into()
        }
    }

    struct ScriptedProvider {
        steps: Mutex<VecDeque<Result<CognitoAuthStep, NativeAuthError>>>,
        calls: Mutex<Vec<(CognitoChallengeKind, HashMap<String, String>)>>,
        cancel_after_call: Option<(usize, OperationCancellation)>,
        delay_after_call: Option<(usize, Duration)>,
    }

    impl ScriptedProvider {
        fn new(steps: Vec<CognitoAuthStep>) -> Self {
            Self {
                steps: Mutex::new(steps.into_iter().map(Ok).collect()),
                calls: Mutex::new(Vec::new()),
                cancel_after_call: None,
                delay_after_call: None,
            }
        }

        fn failing(error: NativeAuthError) -> Self {
            Self::scripted(vec![Err(error)])
        }

        fn scripted(steps: Vec<Result<CognitoAuthStep, NativeAuthError>>) -> Self {
            Self {
                steps: Mutex::new(steps.into()),
                calls: Mutex::new(Vec::new()),
                cancel_after_call: None,
                delay_after_call: None,
            }
        }

        async fn next(&self) -> Result<CognitoAuthStep, NativeAuthError> {
            let current_call = self.calls.lock().unwrap().len();
            if let Some((call, duration)) = self.delay_after_call
                && current_call == call
            {
                tokio::time::sleep(duration).await;
            }
            if let Some((call, cancellation)) = &self.cancel_after_call
                && current_call == *call
            {
                cancellation.cancel();
            }
            self.steps.lock().unwrap().pop_front().unwrap()
        }
    }

    #[async_trait]
    impl CognitoProvider for ScriptedProvider {
        async fn initiate_srp(
            &self,
            parameters: HashMap<String, String>,
        ) -> Result<CognitoAuthStep, NativeAuthError> {
            self.calls
                .lock()
                .unwrap()
                .push((CognitoChallengeKind::PasswordVerifier, parameters));
            self.next().await
        }

        async fn respond(
            &self,
            kind: CognitoChallengeKind,
            parameters: HashMap<String, String>,
            _: &str,
        ) -> Result<CognitoAuthStep, NativeAuthError> {
            self.calls.lock().unwrap().push((kind, parameters));
            self.next().await
        }
    }

    fn challenge(kind: CognitoChallengeKind) -> CognitoAuthStep {
        let parameters = match kind {
            CognitoChallengeKind::PasswordVerifier => HashMap::from([
                ("SECRET_BLOCK".into(), "AQIDBA==".into()),
                ("USER_ID_FOR_SRP".into(), "canonical-user".into()),
                ("SALT".into(), "deadbeef".into()),
                ("SRP_B".into(), "abcdef1234567890".into()),
            ]),
            CognitoChallengeKind::NewPasswordRequired => HashMap::from([
                ("requiredAttributes".into(), "[]".into()),
                (
                    "userAttributes".into(),
                    r#"{"email":"person@example.com","email_verified":"true"}"#.into(),
                ),
            ]),
            CognitoChallengeKind::SoftwareTokenMfa => HashMap::new(),
            CognitoChallengeKind::MfaSetup => {
                HashMap::from([("MFAS_CAN_SETUP".into(), r#"["SOFTWARE_TOKEN_MFA"]"#.into())])
            }
            CognitoChallengeKind::EmailCode => HashMap::from([
                ("CODE_DELIVERY_DELIVERY_MEDIUM".into(), "EMAIL".into()),
                (
                    "CODE_DELIVERY_DESTINATION".into(),
                    "p***@example.com".into(),
                ),
            ]),
            CognitoChallengeKind::Unsupported => HashMap::new(),
        };
        CognitoAuthStep::Challenge(
            CognitoChallengeStep::new(kind, parameters, "opaque-session".into()).unwrap(),
        )
    }

    fn tokens() -> CognitoAuthStep {
        CognitoAuthStep::Authenticated(
            CognitoTokenSet::new(
                "access-token".into(),
                "refresh-material".into(),
                Duration::from_secs(600),
            )
            .unwrap(),
        )
    }

    fn sign_in() -> AuthenticationRequest {
        AuthenticationRequest::SignIn {
            identifier: SecretText::new(EMAIL),
            password: SecretText::new(PASSWORD),
        }
    }

    fn administrator_invitation() -> AuthenticationRequest {
        AuthenticationRequest::AcceptAdministratorInvitation {
            identifier: SecretText::new(EMAIL),
            temporary_password: SecretText::new("Temporary1!"),
            new_password: SecretText::new(PASSWORD),
        }
    }

    #[tokio::test]
    async fn sign_in_runs_srp_and_keeps_tokens_native() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            challenge(CognitoChallengeKind::PasswordVerifier),
            tokens(),
        ]));
        let clock = Arc::new(FixedClock::new());
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider.clone(), clock).unwrap();

        let outcome = authenticator
            .authenticate(&sign_in(), &OperationCancellation::default())
            .await
            .unwrap();
        let NativeAuthOutcome::Authenticated(tokens) = outcome else {
            panic!("expected native token outcome");
        };
        assert_eq!(tokens.access_token(), "access-token");
        assert_eq!(tokens.valid_for(), Duration::from_secs(600));
        assert_eq!(provider.calls.lock().unwrap().len(), 2);
        let debug = format!("{authenticator:?} {tokens:?}");
        assert!(!debug.contains(EMAIL));
        assert!(!debug.contains("access-token"));
        assert_eq!(
            tokens.into_refresh_material().as_bytes(),
            b"refresh-material"
        );
    }

    #[tokio::test]
    async fn administrator_invitation_completes_cognito_temporary_password_challenge() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            challenge(CognitoChallengeKind::PasswordVerifier),
            challenge(CognitoChallengeKind::NewPasswordRequired),
            tokens(),
        ]));
        let authenticator = NativeCognitoAuthenticator::with_clock(
            POOL_ID,
            provider.clone(),
            Arc::new(FixedClock::new()),
        )
        .unwrap();

        assert!(matches!(
            authenticator
                .authenticate(
                    &administrator_invitation(),
                    &OperationCancellation::default(),
                )
                .await
                .unwrap(),
            NativeAuthOutcome::Authenticated(_)
        ));
        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[2].0, CognitoChallengeKind::NewPasswordRequired);
        assert_eq!(calls[2].1["USERNAME"], "canonical-user");
        assert_eq!(calls[2].1["NEW_PASSWORD"], PASSWORD);
    }

    #[tokio::test]
    async fn invitation_lost_response_requires_safe_sign_in_recovery() {
        let provider = Arc::new(ScriptedProvider::scripted(vec![
            Ok(challenge(CognitoChallengeKind::PasswordVerifier)),
            Ok(challenge(CognitoChallengeKind::NewPasswordRequired)),
            Err(NativeAuthError::new(NativeAuthErrorCode::Unavailable)),
        ]));
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider, Arc::new(FixedClock::new()))
                .unwrap();

        let error = authenticator
            .authenticate(
                &administrator_invitation(),
                &OperationCancellation::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            NativeAuthErrorCode::InvitationCompletionUncertain
        );
        assert!(error.message().contains("Sign in with the new password"));
    }

    #[tokio::test]
    async fn delayed_response_after_cancellation_discards_tokens_as_uncertain() {
        let cancellation = OperationCancellation::default();
        let provider = Arc::new(ScriptedProvider {
            steps: Mutex::new(VecDeque::from([
                Ok(challenge(CognitoChallengeKind::PasswordVerifier)),
                Ok(challenge(CognitoChallengeKind::NewPasswordRequired)),
                Ok(tokens()),
            ])),
            calls: Mutex::new(Vec::new()),
            cancel_after_call: None,
            delay_after_call: Some((3, Duration::from_millis(50))),
        });
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider, Arc::new(FixedClock::new()))
                .unwrap();
        let cancellation_request = cancellation.clone();
        let cancel = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancellation_request.cancel();
        });

        assert_eq!(
            authenticator
                .authenticate(&administrator_invitation(), &cancellation)
                .await
                .unwrap_err()
                .code(),
            NativeAuthErrorCode::InvitationCompletionUncertain
        );
        cancel.await.unwrap();
    }

    #[test]
    fn challenge_parameters_use_exact_per_challenge_schemas() {
        assert!(matches!(
            challenge(CognitoChallengeKind::NewPasswordRequired),
            CognitoAuthStep::Challenge(_)
        ));

        let wrong_case = HashMap::from([
            ("REQUIRED_ATTRIBUTES".into(), "[]".into()),
            ("userAttributes".into(), "{}".into()),
        ]);
        assert_eq!(
            CognitoChallengeStep::new(
                CognitoChallengeKind::NewPasswordRequired,
                wrong_case,
                "session".into(),
            )
            .unwrap_err()
            .code(),
            NativeAuthErrorCode::InvalidResponse
        );

        let cross_challenge = HashMap::from([("MFAS_CAN_SETUP".into(), "[]".into())]);
        assert!(
            CognitoChallengeStep::new(
                CognitoChallengeKind::SoftwareTokenMfa,
                cross_challenge,
                "session".into(),
            )
            .is_err()
        );

        let oversized_public_b = HashMap::from([
            ("SECRET_BLOCK".into(), "AQIDBA==".into()),
            ("USER_ID_FOR_SRP".into(), "canonical-user".into()),
            ("SALT".into(), "01".into()),
            ("SRP_B".into(), "a".repeat(384 * 2 + 1)),
        ]);
        assert!(
            CognitoChallengeStep::new(
                CognitoChallengeKind::PasswordVerifier,
                oversized_public_b,
                "session".into(),
            )
            .is_err()
        );
    }

    #[test]
    fn administrator_invitation_attributes_are_complete_and_bounded() {
        for parameters in [
            HashMap::from([("userAttributes".into(), "{}".into())]),
            HashMap::from([
                ("requiredAttributes".into(), "not-json".into()),
                ("userAttributes".into(), "{}".into()),
            ]),
            HashMap::from([("requiredAttributes".into(), "[]".into())]),
            HashMap::from([
                ("requiredAttributes".into(), "[]".into()),
                ("userAttributes".into(), "not-json".into()),
            ]),
        ] {
            assert_eq!(
                validate_administrator_invitation_challenge(&parameters)
                    .unwrap_err()
                    .code(),
                NativeAuthErrorCode::InvalidResponse
            );
        }

        let required_attribute = HashMap::from([
            ("requiredAttributes".into(), r#"["email"]"#.into()),
            ("userAttributes".into(), "{}".into()),
        ]);
        assert!(validate_administrator_invitation_challenge(&required_attribute).is_err());

        let user_attributes = (0..33)
            .map(|index| (format!("attribute-{index}"), "value".to_owned()))
            .collect::<HashMap<_, _>>();
        let oversized_attributes = HashMap::from([
            ("requiredAttributes".into(), "[]".into()),
            (
                "userAttributes".into(),
                serde_json::to_string(&user_attributes).unwrap(),
            ),
        ]);
        assert!(validate_administrator_invitation_challenge(&oversized_attributes).is_err());
    }

    #[tokio::test]
    async fn configured_follow_up_challenges_stay_native() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            challenge(CognitoChallengeKind::PasswordVerifier),
            challenge(CognitoChallengeKind::SoftwareTokenMfa),
        ]));
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider, Arc::new(FixedClock::new()))
                .unwrap();

        let outcome = authenticator
            .authenticate(&sign_in(), &OperationCancellation::default())
            .await
            .unwrap();
        let NativeAuthOutcome::Challenge(challenge) = outcome else {
            panic!("expected native challenge");
        };
        assert_eq!(challenge.kind(), CognitoChallengeKind::SoftwareTokenMfa);
        assert!(!format!("{challenge:?}").contains("opaque-session"));
    }

    #[tokio::test]
    async fn invalid_reused_and_expired_administrator_invitations_fail_closed() {
        for error in [
            NativeAuthErrorCode::InvalidCredentials,
            NativeAuthErrorCode::RateLimited,
            NativeAuthErrorCode::Unavailable,
        ] {
            let provider = Arc::new(ScriptedProvider::failing(NativeAuthError::new(error)));
            let authenticator = NativeCognitoAuthenticator::with_clock(
                POOL_ID,
                provider,
                Arc::new(FixedClock::new()),
            )
            .unwrap();
            assert_eq!(
                authenticator
                    .authenticate(
                        &administrator_invitation(),
                        &OperationCancellation::default(),
                    )
                    .await
                    .unwrap_err()
                    .code(),
                error
            );
        }

        let provider = Arc::new(ScriptedProvider::new(vec![
            challenge(CognitoChallengeKind::PasswordVerifier),
            tokens(),
        ]));
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider, Arc::new(FixedClock::new()))
                .unwrap();
        assert_eq!(
            authenticator
                .authenticate(
                    &administrator_invitation(),
                    &OperationCancellation::default(),
                )
                .await
                .unwrap_err()
                .code(),
            NativeAuthErrorCode::InvalidCredentials
        );
    }

    #[tokio::test]
    async fn cancellation_discards_results_before_and_after_network_use() {
        let cancellation = OperationCancellation::default();
        cancellation.cancel();
        let provider = Arc::new(ScriptedProvider::new(Vec::new()));
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider, Arc::new(FixedClock::new()))
                .unwrap();
        assert_eq!(
            authenticator
                .authenticate(&sign_in(), &cancellation)
                .await
                .unwrap_err()
                .code(),
            NativeAuthErrorCode::Cancelled
        );

        let cancellation = OperationCancellation::default();
        let provider = Arc::new(ScriptedProvider {
            steps: Mutex::new(VecDeque::from([Ok(challenge(
                CognitoChallengeKind::PasswordVerifier,
            ))])),
            calls: Mutex::new(Vec::new()),
            cancel_after_call: Some((1, cancellation.clone())),
            delay_after_call: None,
        });
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider, Arc::new(FixedClock::new()))
                .unwrap();
        assert_eq!(
            authenticator
                .authenticate(&sign_in(), &cancellation)
                .await
                .unwrap_err()
                .code(),
            NativeAuthErrorCode::Cancelled
        );
    }

    #[tokio::test]
    async fn process_local_rate_limit_recovers_after_the_fixed_window() {
        let steps = (0..(MAX_LOCAL_AUTH_ATTEMPTS + 1))
            .flat_map(|_| [challenge(CognitoChallengeKind::PasswordVerifier), tokens()])
            .collect();
        let provider = Arc::new(ScriptedProvider::new(steps));
        let clock = Arc::new(FixedClock::new());
        let authenticator =
            NativeCognitoAuthenticator::with_clock(POOL_ID, provider, clock.clone()).unwrap();

        for _ in 0..MAX_LOCAL_AUTH_ATTEMPTS {
            assert!(
                authenticator
                    .authenticate(&sign_in(), &OperationCancellation::default())
                    .await
                    .is_ok()
            );
        }
        assert_eq!(
            authenticator
                .authenticate(&sign_in(), &OperationCancellation::default())
                .await
                .unwrap_err()
                .code(),
            NativeAuthErrorCode::RateLimited
        );

        clock.advance(Duration::from_secs(60));
        assert!(
            authenticator
                .authenticate(&sign_in(), &OperationCancellation::default())
                .await
                .is_ok()
        );
    }

    #[test]
    fn request_validation_and_debug_never_echo_credentials() {
        for request in [
            AuthenticationRequest::SignIn {
                identifier: SecretText::new("invalid"),
                password: SecretText::new(PASSWORD),
            },
            AuthenticationRequest::AcceptAdministratorInvitation {
                identifier: SecretText::new(EMAIL),
                temporary_password: SecretText::new("Temporary1!"),
                new_password: SecretText::new("weak"),
            },
        ] {
            assert!(request.validate().is_err());
        }

        let secret = SecretText::new("never-print-this");
        assert!(!format!("{secret:?}").contains("never-print-this"));
        for code in [
            NativeAuthErrorCode::Cancelled,
            NativeAuthErrorCode::InvalidRequest,
            NativeAuthErrorCode::InvalidCredentials,
            NativeAuthErrorCode::PasswordRejected,
            NativeAuthErrorCode::RateLimited,
            NativeAuthErrorCode::InvalidResponse,
            NativeAuthErrorCode::InvitationCompletionUncertain,
            NativeAuthErrorCode::Unavailable,
        ] {
            let error = NativeAuthError::new(code);
            assert!(!error.message().is_empty());
            assert_eq!(error.to_string(), error.message());
        }

        let system_clock = SystemAuthClock;
        let _ = system_clock.monotonic_now();
        assert!(system_clock.cognito_timestamp().contains(" UTC "));
        assert!(
            NativeCognitoAuthenticator::new("bad-pool", Arc::new(ScriptedProvider::new(vec![])))
                .is_err()
        );
    }
}
