//! Native Cognito token validation and challenge-state contracts.
//!
//! This module keeps access tokens and challenge continuations in Rust-owned
//! values. It does not expose a Tauri command or a serialization path for
//! either value.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use cipher_native_transport::AccessToken;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, errors::ErrorKind};
use serde::Deserialize;

use crate::credential_store::SecretBytes;

const MAX_ACCESS_TOKEN_BYTES: usize = 16 * 1024;
const MAX_ISSUER_BYTES: usize = 2 * 1024;
const MAX_CLIENT_ID_BYTES: usize = 256;
const MAX_SCOPE_BYTES: usize = 256;
const MAX_SCOPE_CLAIM_BYTES: usize = 4 * 1024;
const MAX_REQUIRED_SCOPES: usize = 32;
const MAX_TOKEN_SCOPES: usize = 64;
const MAX_SUBJECT_BYTES: usize = 512;
const MAX_KEY_ID_BYTES: usize = 512;
const MAX_JWK_MODULUS_BYTES: usize = 1024;
const MAX_JWK_EXPONENT_BYTES: usize = 16;
const MAX_JWKS_BYTES: usize = 64 * 1024;
const MAX_JWKS_KEYS: usize = 16;
const MAX_CHALLENGE_CONTINUATION_BYTES: usize = 8 * 1024;
const MAX_NEW_PASSWORD_BYTES: usize = 256;
const MAX_CHALLENGE_LIFETIME: Duration = Duration::from_secs(15 * 60);

static NEXT_CHALLENGE_STATE_ID: AtomicU64 = AtomicU64::new(1);

/// The fixed Cognito issuer, app client, scopes, and JWKS lifetime accepted by
/// a native desktop configuration.
#[derive(Clone, Eq, PartialEq)]
pub struct CognitoTokenPolicy {
    issuer: String,
    client_id: String,
    required_scopes: BTreeSet<String>,
    jwks_max_age: Duration,
}

impl CognitoTokenPolicy {
    /// Builds a policy that requires every supplied access-token scope.
    ///
    /// The policy has no permissive defaults: issuer, client ID, at least one
    /// scope, and a positive JWKS cache lifetime are all required.
    pub fn new(
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        required_scopes: impl IntoIterator<Item = impl Into<String>>,
        jwks_max_age: Duration,
    ) -> Result<Self, CognitoTokenPolicyError> {
        let issuer = issuer.into();
        if !is_bounded_non_whitespace(&issuer, MAX_ISSUER_BYTES) {
            return Err(CognitoTokenPolicyError::InvalidIssuer);
        }

        let client_id = client_id.into();
        if !is_bounded_non_whitespace(&client_id, MAX_CLIENT_ID_BYTES) {
            return Err(CognitoTokenPolicyError::InvalidClientId);
        }

        let mut scopes = BTreeSet::new();
        let mut scope_count = 0usize;
        for scope in required_scopes {
            scope_count = scope_count
                .checked_add(1)
                .ok_or(CognitoTokenPolicyError::TooManyRequiredScopes)?;
            if scope_count > MAX_REQUIRED_SCOPES {
                return Err(CognitoTokenPolicyError::TooManyRequiredScopes);
            }
            let scope = scope.into();
            if !is_valid_scope(&scope) {
                return Err(CognitoTokenPolicyError::InvalidRequiredScope);
            }
            scopes.insert(scope);
        }
        if scopes.is_empty() {
            return Err(CognitoTokenPolicyError::MissingRequiredScope);
        }

        if jwks_max_age < Duration::from_secs(1) || jwks_max_age > Duration::from_secs(24 * 60 * 60)
        {
            return Err(CognitoTokenPolicyError::InvalidJwksMaxAge);
        }

        Ok(Self {
            issuer,
            client_id,
            required_scopes: scopes,
            jwks_max_age,
        })
    }
}

impl fmt::Debug for CognitoTokenPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CognitoTokenPolicy")
            .field("issuer", &"[configured]")
            .field("client_id", &"[configured]")
            .field("required_scopes", &"[configured]")
            .field("jwks_max_age", &self.jwks_max_age)
            .finish()
    }
}

/// The reason a static Cognito token policy cannot be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CognitoTokenPolicyError {
    /// The expected Cognito issuer is absent, too large, or contains whitespace.
    InvalidIssuer,
    /// The expected public app client ID is absent, too large, or contains whitespace.
    InvalidClientId,
    /// A required scope is absent, too large, or contains whitespace.
    InvalidRequiredScope,
    /// No required authorization scope was supplied.
    MissingRequiredScope,
    /// More authorization scopes were supplied than the native policy permits.
    TooManyRequiredScopes,
    /// The JWKS lifetime is zero or permits stale keys for too long.
    InvalidJwksMaxAge,
}

impl fmt::Display for CognitoTokenPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIssuer => formatter.write_str("the Cognito issuer is invalid"),
            Self::InvalidClientId => formatter.write_str("the Cognito client ID is invalid"),
            Self::InvalidRequiredScope => {
                formatter.write_str("a required Cognito scope is invalid")
            }
            Self::MissingRequiredScope => {
                formatter.write_str("at least one Cognito scope is required")
            }
            Self::TooManyRequiredScopes => {
                formatter.write_str("too many Cognito scopes are required")
            }
            Self::InvalidJwksMaxAge => formatter.write_str("the Cognito JWKS lifetime is invalid"),
        }
    }
}

impl std::error::Error for CognitoTokenPolicyError {}

/// Supplies the current JSON Web Key Set for one trusted Cognito issuer.
///
/// Implementations must obtain the set only from the issuer's configured
/// JWKS endpoint. The validator owns caching and never exposes the response.
pub trait JwksSource {
    /// Fetches an unmodified JWKS response for the configured issuer.
    fn fetch_jwks(&self) -> Result<String, JwksSourceError>;
}

/// A bounded category for a JWKS source failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JwksSourceError {
    /// The configured JWKS endpoint could not be reached or read.
    Unavailable,
    /// The source received a response it cannot safely provide to the validator.
    InvalidResponse,
}

impl fmt::Display for JwksSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("the Cognito JWKS source is unavailable"),
            Self::InvalidResponse => formatter.write_str("the Cognito JWKS response is invalid"),
        }
    }
}

impl std::error::Error for JwksSourceError {}

/// Validates Cognito access tokens against a bounded, fail-closed JWKS cache.
pub struct CognitoAccessTokenValidator<S> {
    policy: CognitoTokenPolicy,
    source: S,
    cache: Option<CachedJwks>,
}

impl<S> CognitoAccessTokenValidator<S> {
    /// Creates a validator with an empty JWKS cache.
    pub fn new(policy: CognitoTokenPolicy, source: S) -> Self {
        Self {
            policy,
            source,
            cache: None,
        }
    }
}

impl<S: JwksSource> CognitoAccessTokenValidator<S> {
    /// Verifies raw native access-token bytes at a supplied Unix timestamp.
    ///
    /// A caller must provide an access token obtained by native authentication;
    /// ID tokens are rejected. A stale cached key is never used. Missing key
    /// IDs fail before loading keys; stale or unknown key IDs trigger one reload.
    pub fn validate_at(
        &mut self,
        raw_access_token: &[u8],
        unix_time_seconds: i64,
    ) -> Result<ValidatedAccessToken, CognitoTokenValidationError> {
        if unix_time_seconds < 0 {
            return Err(CognitoTokenValidationError::InvalidClock);
        }
        if raw_access_token.is_empty()
            || raw_access_token.len() > MAX_ACCESS_TOKEN_BYTES
            || raw_access_token
                .iter()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err(CognitoTokenValidationError::MalformedToken);
        }

        let token = std::str::from_utf8(raw_access_token)
            .map_err(|_| CognitoTokenValidationError::MalformedToken)?;
        if token.split('.').count() != 3 {
            return Err(CognitoTokenValidationError::MalformedToken);
        }

        let header =
            decode_header(token).map_err(|_| CognitoTokenValidationError::MalformedToken)?;
        if header.alg != Algorithm::RS256 {
            return Err(CognitoTokenValidationError::UnsupportedAlgorithm);
        }
        let key_id = header
            .kid
            .filter(|key_id| is_bounded_non_whitespace(key_id, MAX_KEY_ID_BYTES))
            .ok_or(CognitoTokenValidationError::MissingKeyId)?;
        let key = self.decoding_key(&key_id, unix_time_seconds)?;

        let claims = decode::<AccessTokenClaims>(token, key, &token_validation())
            .map_err(map_jwt_error)?
            .claims;
        validate_claims(&self.policy, claims, unix_time_seconds, token)
    }

    fn decoding_key(
        &mut self,
        key_id: &str,
        unix_time_seconds: i64,
    ) -> Result<&DecodingKey, CognitoTokenValidationError> {
        let needs_refresh = match self.cache.as_ref() {
            Some(cache) => {
                cache.is_stale(unix_time_seconds, self.policy.jwks_max_age)
                    || !cache.keys.contains_key(key_id)
            }
            None => true,
        };
        if needs_refresh {
            self.refresh_keys(unix_time_seconds)?;
        }

        self.cache
            .as_ref()
            .and_then(|cache| cache.keys.get(key_id))
            .ok_or(CognitoTokenValidationError::UnknownKeyId)
    }

    fn refresh_keys(&mut self, unix_time_seconds: i64) -> Result<(), CognitoTokenValidationError> {
        let response = self
            .source
            .fetch_jwks()
            .map_err(|_| CognitoTokenValidationError::KeyUnavailable)?;
        let keys = parse_jwks(&response).ok_or(CognitoTokenValidationError::KeyUnavailable)?;
        self.cache = Some(CachedJwks {
            fetched_at: unix_time_seconds,
            keys,
        });
        Ok(())
    }
}

impl<S> fmt::Debug for CognitoAccessTokenValidator<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CognitoAccessTokenValidator")
            .field("policy", &self.policy)
            .field("jwks_cache", &self.cache.as_ref().map(|_| "[loaded]"))
            .finish_non_exhaustive()
    }
}

/// The reason a supplied Cognito access token is not authorized for native use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CognitoTokenValidationError {
    /// The supplied Unix timestamp is not a valid non-negative epoch value.
    InvalidClock,
    /// The token is not a bounded, well-formed compact JWT.
    MalformedToken,
    /// The JWT header does not require Cognito's RS256 algorithm.
    UnsupportedAlgorithm,
    /// The JWT header does not contain a valid signing key ID.
    MissingKeyId,
    /// A current JWKS response could not be loaded and parsed.
    KeyUnavailable,
    /// The current JWKS does not contain the requested signing key ID.
    UnknownKeyId,
    /// The JWT signature does not match the selected Cognito public key.
    InvalidSignature,
    /// The access token is expired at the supplied Unix timestamp.
    Expired,
    /// The token issuer does not exactly match the configured Cognito issuer.
    InvalidIssuer,
    /// The access-token client ID does not exactly match the configured public app client.
    InvalidClientId,
    /// The token is not a Cognito access token.
    InvalidTokenUse,
    /// The token does not contain every configured authorization scope.
    MissingRequiredScope,
    /// The stable Cognito subject is missing or exceeds its bounded contract.
    InvalidSubject,
}

impl fmt::Display for CognitoTokenValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidClock => formatter.write_str("the validation time is invalid"),
            Self::MalformedToken => formatter.write_str("the access token is malformed"),
            Self::UnsupportedAlgorithm => {
                formatter.write_str("the access token uses an unsupported algorithm")
            }
            Self::MissingKeyId => formatter.write_str("the access token signing key ID is missing"),
            Self::KeyUnavailable => {
                formatter.write_str("the access token signing keys are unavailable")
            }
            Self::UnknownKeyId => formatter.write_str("the access token signing key is unknown"),
            Self::InvalidSignature => formatter.write_str("the access token signature is invalid"),
            Self::Expired => formatter.write_str("the access token is expired"),
            Self::InvalidIssuer => formatter.write_str("the access token issuer is invalid"),
            Self::InvalidClientId => formatter.write_str("the access token client ID is invalid"),
            Self::InvalidTokenUse => formatter.write_str("the token is not an access token"),
            Self::MissingRequiredScope => {
                formatter.write_str("the access token does not contain the required scope")
            }
            Self::InvalidSubject => formatter.write_str("the Cognito subject is invalid"),
        }
    }
}

impl std::error::Error for CognitoTokenValidationError {}

/// A bounded, validated Cognito subject suitable for a native session identity.
#[derive(Clone, Eq, PartialEq)]
pub struct CognitoSubject(String);

impl CognitoSubject {
    /// Returns the stable subject after bounded token validation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CognitoSubject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CognitoSubject([redacted])")
    }
}

/// A verified native access token paired with its stable Cognito subject.
pub struct ValidatedAccessToken {
    subject: CognitoSubject,
    expires_at: i64,
    access_token: AccessToken,
}

impl ValidatedAccessToken {
    /// Borrows the validated stable Cognito subject for native session construction.
    pub fn subject(&self) -> &CognitoSubject {
        &self.subject
    }

    /// Returns the validated expiration timestamp in Unix seconds.
    pub const fn expires_at(&self) -> i64 {
        self.expires_at
    }

    /// Consumes the result into the validated subject and native-only access token.
    pub fn into_parts(self) -> (CognitoSubject, AccessToken) {
        (self.subject, self.access_token)
    }
}

impl fmt::Debug for ValidatedAccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedAccessToken")
            .field("subject", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .field("access_token", &"[redacted]")
            .finish()
    }
}

/// The native Cognito flow to which a challenge ticket is bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CognitoChallengeFlow {
    /// A confirmation-code challenge for a newly invited user's email address.
    EmailVerification,
    /// A confirmation-code and replacement-password challenge.
    PasswordReset,
    /// A six-digit time-based one-time-password challenge.
    SoftwareTokenMfa,
}

/// An opaque, non-serializable reference to one pending native Cognito challenge.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct CognitoChallengeTicket {
    state_id: u64,
    sequence: u64,
    flow: CognitoChallengeFlow,
    expires_at: i64,
}

impl CognitoChallengeTicket {
    /// Returns the one flow accepted by this ticket.
    pub const fn flow(self) -> CognitoChallengeFlow {
        self.flow
    }

    /// Returns the Unix timestamp after which this ticket cannot be completed.
    pub const fn expires_at(self) -> i64 {
        self.expires_at
    }
}

impl fmt::Debug for CognitoChallengeTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CognitoChallengeTicket([redacted])")
    }
}

/// A secret native response to one Cognito challenge flow.
pub enum CognitoChallengeResponse {
    /// A six-digit email confirmation code.
    EmailVerification {
        /// The user-provided confirmation code.
        code: SecretBytes,
    },
    /// A six-digit reset code and a replacement password.
    PasswordReset {
        /// The user-provided reset code.
        code: SecretBytes,
        /// The replacement password kept in Rust-owned memory.
        new_password: SecretBytes,
    },
    /// A six-digit time-based one-time password.
    SoftwareTokenMfa {
        /// The user-provided time-based one-time password.
        code: SecretBytes,
    },
}

impl CognitoChallengeResponse {
    fn flow(&self) -> CognitoChallengeFlow {
        match self {
            Self::EmailVerification { .. } => CognitoChallengeFlow::EmailVerification,
            Self::PasswordReset { .. } => CognitoChallengeFlow::PasswordReset,
            Self::SoftwareTokenMfa { .. } => CognitoChallengeFlow::SoftwareTokenMfa,
        }
    }

    fn is_well_formed(&self) -> bool {
        match self {
            Self::EmailVerification { code } | Self::SoftwareTokenMfa { code } => {
                is_six_digit_code(code)
            }
            Self::PasswordReset { code, new_password } => {
                is_six_digit_code(code) && is_valid_new_password(new_password)
            }
        }
    }
}

impl fmt::Debug for CognitoChallengeResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CognitoChallengeResponse([redacted])")
    }
}

/// The reason a native Cognito challenge cannot be completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CognitoChallengeError {
    /// The configured challenge lifetime is outside the bounded native range.
    InvalidLifetime,
    /// The supplied Unix timestamp is not a valid non-negative epoch value.
    InvalidClock,
    /// The process cannot allocate another independent challenge state identifier.
    StateIdentifierExhausted,
    /// The state cannot allocate another challenge ticket.
    TicketIdentifierExhausted,
    /// A continuation value is empty or exceeds the bounded native contract.
    InvalidContinuation,
    /// A different native challenge is already pending.
    ChallengePending,
    /// The ticket does not belong to this state or its pending challenge.
    UnknownTicket,
    /// The response flow differs from the flow bound to the ticket.
    WrongFlow,
    /// The ticket's lifetime has elapsed and its continuation was discarded.
    Expired,
    /// The response does not meet the bounded code or password shape.
    MalformedResponse,
    /// The ticket was already completed or discarded after expiry.
    Replay,
}

impl fmt::Display for CognitoChallengeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLifetime => formatter.write_str("the challenge lifetime is invalid"),
            Self::InvalidClock => formatter.write_str("the challenge time is invalid"),
            Self::StateIdentifierExhausted => {
                formatter.write_str("a challenge state identifier is unavailable")
            }
            Self::TicketIdentifierExhausted => {
                formatter.write_str("a challenge ticket identifier is unavailable")
            }
            Self::InvalidContinuation => {
                formatter.write_str("the challenge continuation is invalid")
            }
            Self::ChallengePending => formatter.write_str("a Cognito challenge is already pending"),
            Self::UnknownTicket => formatter.write_str("the Cognito challenge ticket is unknown"),
            Self::WrongFlow => formatter.write_str("the Cognito challenge flow does not match"),
            Self::Expired => formatter.write_str("the Cognito challenge has expired"),
            Self::MalformedResponse => {
                formatter.write_str("the Cognito challenge response is invalid")
            }
            Self::Replay => formatter.write_str("the Cognito challenge ticket was already used"),
        }
    }
}

impl std::error::Error for CognitoChallengeError {}

/// Holds at most one pending native Cognito challenge and its opaque continuation.
pub struct CognitoChallengeState {
    state_id: u64,
    lifetime: Duration,
    next_ticket_sequence: u64,
    pending: Option<PendingChallenge>,
    retired_through: u64,
}

impl CognitoChallengeState {
    /// Creates an empty challenge state with a positive lifetime up to fifteen minutes.
    pub fn new(lifetime: Duration) -> Result<Self, CognitoChallengeError> {
        if lifetime < Duration::from_secs(1) || lifetime > MAX_CHALLENGE_LIFETIME {
            return Err(CognitoChallengeError::InvalidLifetime);
        }
        let state_id = NEXT_CHALLENGE_STATE_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| CognitoChallengeError::StateIdentifierExhausted)?;

        Ok(Self {
            state_id,
            lifetime,
            next_ticket_sequence: 1,
            pending: None,
            retired_through: 0,
        })
    }

    /// Begins one flow and binds an opaque native continuation to an expiring ticket.
    pub fn begin(
        &mut self,
        flow: CognitoChallengeFlow,
        continuation: SecretBytes,
        unix_time_seconds: i64,
    ) -> Result<CognitoChallengeTicket, CognitoChallengeError> {
        if unix_time_seconds < 0 {
            return Err(CognitoChallengeError::InvalidClock);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| unix_time_seconds >= pending.ticket.expires_at)
        {
            let expired = self
                .pending
                .take()
                .expect("an expired Cognito challenge must remain pending");
            self.retired_through = expired.ticket.sequence;
        }
        if continuation.as_bytes().is_empty()
            || continuation.as_bytes().len() > MAX_CHALLENGE_CONTINUATION_BYTES
        {
            return Err(CognitoChallengeError::InvalidContinuation);
        }
        if self.pending.is_some() {
            return Err(CognitoChallengeError::ChallengePending);
        }

        let lifetime_seconds = i64::try_from(self.lifetime.as_secs())
            .map_err(|_| CognitoChallengeError::InvalidLifetime)?;
        let expires_at = unix_time_seconds
            .checked_add(lifetime_seconds)
            .ok_or(CognitoChallengeError::InvalidClock)?;
        let sequence = self.next_ticket_sequence;
        self.next_ticket_sequence = self
            .next_ticket_sequence
            .checked_add(1)
            .ok_or(CognitoChallengeError::TicketIdentifierExhausted)?;
        let ticket = CognitoChallengeTicket {
            state_id: self.state_id,
            sequence,
            flow,
            expires_at,
        };
        self.pending = Some(PendingChallenge {
            ticket,
            continuation,
        });
        Ok(ticket)
    }

    /// Validates and consumes one matching challenge response exactly once.
    ///
    /// A malformed or wrong-flow response leaves a live ticket pending so a
    /// caller can retry. An expired ticket discards its continuation before
    /// returning an error.
    pub fn complete(
        &mut self,
        ticket: CognitoChallengeTicket,
        response: CognitoChallengeResponse,
        unix_time_seconds: i64,
    ) -> Result<CognitoChallengeResolution, CognitoChallengeError> {
        if unix_time_seconds < 0 {
            return Err(CognitoChallengeError::InvalidClock);
        }
        if ticket.state_id != self.state_id {
            return Err(CognitoChallengeError::UnknownTicket);
        }
        if ticket.sequence <= self.retired_through {
            return Err(CognitoChallengeError::Replay);
        }

        let pending = self
            .pending
            .as_ref()
            .ok_or(CognitoChallengeError::UnknownTicket)?;
        if pending.ticket != ticket {
            return Err(CognitoChallengeError::UnknownTicket);
        }
        if unix_time_seconds >= ticket.expires_at {
            self.pending = None;
            self.retired_through = ticket.sequence;
            return Err(CognitoChallengeError::Expired);
        }
        if response.flow() != ticket.flow {
            return Err(CognitoChallengeError::WrongFlow);
        }
        if !response.is_well_formed() {
            return Err(CognitoChallengeError::MalformedResponse);
        }

        let pending = self
            .pending
            .take()
            .expect("the matching Cognito challenge must remain pending");
        self.retired_through = ticket.sequence;
        Ok(CognitoChallengeResolution {
            flow: ticket.flow,
            continuation: pending.continuation,
            response,
        })
    }
}

impl fmt::Debug for CognitoChallengeState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CognitoChallengeState")
            .field("lifetime", &self.lifetime)
            .field("pending", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

/// A consumed native challenge continuation and validated response for one flow.
pub struct CognitoChallengeResolution {
    flow: CognitoChallengeFlow,
    continuation: SecretBytes,
    response: CognitoChallengeResponse,
}

impl CognitoChallengeResolution {
    /// Returns the flow for the consumed challenge response.
    pub const fn flow(&self) -> CognitoChallengeFlow {
        self.flow
    }

    /// Consumes the resolution into native-only continuation and response values.
    pub fn into_parts(self) -> (CognitoChallengeFlow, SecretBytes, CognitoChallengeResponse) {
        (self.flow, self.continuation, self.response)
    }
}

impl fmt::Debug for CognitoChallengeResolution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CognitoChallengeResolution([redacted])")
    }
}

struct CachedJwks {
    fetched_at: i64,
    keys: BTreeMap<String, DecodingKey>,
}

impl CachedJwks {
    fn is_stale(&self, unix_time_seconds: i64, max_age: Duration) -> bool {
        if unix_time_seconds < self.fetched_at {
            return true;
        }
        let elapsed = u64::try_from(unix_time_seconds - self.fetched_at).unwrap_or(u64::MAX);
        elapsed >= max_age.as_secs()
    }
}

struct PendingChallenge {
    ticket: CognitoChallengeTicket,
    continuation: SecretBytes,
}

#[derive(Deserialize)]
struct AccessTokenClaims {
    iss: String,
    client_id: String,
    token_use: String,
    exp: i64,
    scope: String,
    sub: String,
}

#[derive(Deserialize)]
struct JwksDocument {
    keys: Vec<JwksKey>,
}

#[derive(Deserialize)]
struct JwksKey {
    kid: String,
    kty: String,
    alg: String,
    #[serde(rename = "use")]
    usage: String,
    n: String,
    e: String,
}

fn token_validation() -> Validation {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = false;
    validation
}

fn map_jwt_error(error: jsonwebtoken::errors::Error) -> CognitoTokenValidationError {
    match error.kind() {
        ErrorKind::InvalidSignature => CognitoTokenValidationError::InvalidSignature,
        ErrorKind::ExpiredSignature => CognitoTokenValidationError::Expired,
        ErrorKind::InvalidAlgorithm => CognitoTokenValidationError::UnsupportedAlgorithm,
        _ => CognitoTokenValidationError::MalformedToken,
    }
}

fn parse_jwks(response: &str) -> Option<BTreeMap<String, DecodingKey>> {
    if response.is_empty() || response.len() > MAX_JWKS_BYTES {
        return None;
    }
    let document = serde_json::from_str::<JwksDocument>(response).ok()?;
    if document.keys.is_empty() || document.keys.len() > MAX_JWKS_KEYS {
        return None;
    }

    let mut keys = BTreeMap::new();
    for key in document.keys {
        if key.kty != "RSA"
            || key.alg != "RS256"
            || key.usage != "sig"
            || !is_bounded_non_whitespace(&key.kid, MAX_KEY_ID_BYTES)
            || !is_bounded_base64url(&key.n, MAX_JWK_MODULUS_BYTES)
            || !is_bounded_base64url(&key.e, MAX_JWK_EXPONENT_BYTES)
        {
            return None;
        }
        let decoding_key = DecodingKey::from_rsa_components(&key.n, &key.e).ok()?;
        if keys.insert(key.kid, decoding_key).is_some() {
            return None;
        }
    }
    Some(keys)
}

fn validate_claims(
    policy: &CognitoTokenPolicy,
    claims: AccessTokenClaims,
    unix_time_seconds: i64,
    raw_access_token: &str,
) -> Result<ValidatedAccessToken, CognitoTokenValidationError> {
    if claims.iss != policy.issuer {
        return Err(CognitoTokenValidationError::InvalidIssuer);
    }
    if claims.client_id != policy.client_id {
        return Err(CognitoTokenValidationError::InvalidClientId);
    }
    if claims.token_use != "access" {
        return Err(CognitoTokenValidationError::InvalidTokenUse);
    }
    if claims.exp <= unix_time_seconds {
        return Err(CognitoTokenValidationError::Expired);
    }

    let scopes =
        parse_scopes(&claims.scope).ok_or(CognitoTokenValidationError::MissingRequiredScope)?;
    if !policy
        .required_scopes
        .iter()
        .all(|required_scope| scopes.contains(required_scope))
    {
        return Err(CognitoTokenValidationError::MissingRequiredScope);
    }
    if !is_bounded_non_whitespace(&claims.sub, MAX_SUBJECT_BYTES) {
        return Err(CognitoTokenValidationError::InvalidSubject);
    }

    let access_token = AccessToken::new(raw_access_token.into())
        .map_err(|_| CognitoTokenValidationError::MalformedToken)?;
    Ok(ValidatedAccessToken {
        subject: CognitoSubject(claims.sub),
        expires_at: claims.exp,
        access_token,
    })
}

fn parse_scopes(value: &str) -> Option<BTreeSet<String>> {
    if value.is_empty() || value.len() > MAX_SCOPE_CLAIM_BYTES {
        return None;
    }
    let mut scopes = BTreeSet::new();
    for scope in value.split_ascii_whitespace() {
        if !is_valid_scope(scope) || !scopes.insert(scope.to_owned()) {
            return None;
        }
        if scopes.len() > MAX_TOKEN_SCOPES {
            return None;
        }
    }
    (!scopes.is_empty()).then_some(scopes)
}

fn is_valid_scope(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SCOPE_BYTES
        && value
            .bytes()
            .all(|byte| matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e))
}

fn is_bounded_base64url(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn is_bounded_non_whitespace(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control())
}

fn is_six_digit_code(value: &SecretBytes) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 6 && bytes.iter().all(u8::is_ascii_digit)
}

fn is_valid_new_password(value: &SecretBytes) -> bool {
    let bytes = value.as_bytes();
    let Ok(password) = std::str::from_utf8(bytes) else {
        return false;
    };
    !password.is_empty()
        && bytes.len() <= MAX_NEW_PASSWORD_BYTES
        && !password.chars().any(char::is_control)
}

#[cfg(test)]
#[path = "cognito_tests.rs"]
mod tests;
