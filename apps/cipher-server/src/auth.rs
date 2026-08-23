//! Default-deny authentication boundary for Cipher HTTP and realtime requests.
//!
//! Cryptographic token verification and application-state authorization are
//! deliberately separate: a valid signed Cognito access token alone cannot
//! observe a revoked Cipher session or device.

use axum::http::{HeaderMap, header::AUTHORIZATION};

/// A verified Cognito identity claim, retained only after JWT validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedIdentity {
    subject: String,
    expires_at: i64,
}

impl VerifiedIdentity {
    /// Creates a bounded identity after a JWT verifier has validated its claims.
    pub fn new(subject: impl Into<String>, expires_at: i64) -> Result<Self, AuthenticationError> {
        let subject = subject.into();
        if subject.is_empty()
            || subject.len() > 512
            || subject
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            || expires_at < 0
        {
            return Err(AuthenticationError::InvalidToken);
        }
        Ok(Self {
            subject,
            expires_at,
        })
    }

    /// Returns the immutable Cognito subject used to resolve Cipher state.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Returns the token expiry validated by the JWT verifier.
    pub const fn expires_at(&self) -> i64 {
        self.expires_at
    }
}

/// An authorized Cipher principal after session and device state have been checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CipherPrincipal {
    /// The authenticated Cognito identity.
    pub identity: VerifiedIdentity,
    /// Opaque application-owned user identifier.
    pub user_id: String,
    /// Opaque active-device identifier.
    pub device_id: String,
    /// Opaque active-session identifier.
    pub session_id: String,
}

/// Validates a Cognito access token's signature, claims, scope, and expiry.
pub trait AccessTokenValidator: Send + Sync {
    /// Returns a verified identity or a fail-closed authentication outcome.
    fn validate(
        &self,
        token: &str,
        unix_time_seconds: i64,
    ) -> Result<VerifiedIdentity, AuthenticationError>;
}

/// Resolves the application session and active device with a strongly consistent read.
pub trait PrincipalGate: Send + Sync {
    /// Rejects disabled users, revoked sessions, and revoked or inactive devices.
    fn authorize(&self, identity: VerifiedIdentity)
    -> Result<CipherPrincipal, AuthenticationError>;
}

/// The safe category of a rejected authentication attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationError {
    /// No syntactically valid bearer token was supplied.
    MissingToken,
    /// The token did not satisfy the configured Cognito policy.
    InvalidToken,
    /// Token verification cannot safely obtain a current signing key.
    SigningKeysUnavailable,
    /// The application user, session, or device is no longer active.
    Revoked,
}

/// Extracts exactly one bounded bearer token from a request header map.
pub fn bearer_token(headers: &HeaderMap) -> Result<&str, AuthenticationError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(AuthenticationError::MissingToken)?;
    let token = value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty() && token.len() <= 16 * 1024)
        .filter(|token| {
            !token
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        })
        .ok_or(AuthenticationError::MissingToken)?;
    Ok(token)
}

/// Runs the shared default-deny authentication path for HTTP and realtime.
pub fn authenticate(
    headers: &HeaderMap,
    validator: &dyn AccessTokenValidator,
    gate: &dyn PrincipalGate,
    unix_time_seconds: i64,
) -> Result<CipherPrincipal, AuthenticationError> {
    let token = bearer_token(headers)?;
    let identity = validator.validate(token, unix_time_seconds)?;
    gate.authorize(identity)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{AuthenticationError, VerifiedIdentity, bearer_token};

    #[test]
    fn bearer_token_rejects_missing_wrong_scheme_and_multiple_tokens() {
        assert_eq!(
            bearer_token(&HeaderMap::new()),
            Err(AuthenticationError::MissingToken)
        );
        for value in ["Basic token", "Bearer", "Bearer a b"] {
            let mut headers = HeaderMap::new();
            headers.insert("authorization", HeaderValue::from_str(value).unwrap());
            assert_eq!(
                bearer_token(&headers),
                Err(AuthenticationError::MissingToken)
            );
        }
    }

    #[test]
    fn bearer_token_accepts_one_bounded_bearer_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer signed.jwt.value"),
        );
        assert_eq!(bearer_token(&headers), Ok("signed.jwt.value"));
    }

    #[test]
    fn verified_identity_is_bounded_and_requires_a_valid_expiry() {
        assert!(VerifiedIdentity::new("sub_123", 1).is_ok());
        assert_eq!(
            VerifiedIdentity::new("", 1),
            Err(AuthenticationError::InvalidToken)
        );
        assert_eq!(
            VerifiedIdentity::new("sub", -1),
            Err(AuthenticationError::InvalidToken)
        );
    }
}
