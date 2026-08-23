//! Default-deny authentication boundary for Cipher HTTP and realtime requests.
//!
//! Cryptographic token verification and application-state authorization are
//! deliberately separate: a valid signed Cognito access token alone cannot
//! observe a revoked Cipher session or device.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::Duration,
};

use aws_sdk_dynamodb::{Client as DynamoDbClient, types::AttributeValue};
use axum::http::{HeaderMap, header::AUTHORIZATION};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

const MAX_JWKS_BYTES: usize = 64 * 1024;
const MAX_JWKS_KEYS: usize = 16;
const MAX_KEY_ID_BYTES: usize = 512;
const JWKS_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const JWKS_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// A verified Cognito identity claim, retained only after JWT validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedIdentity {
    subject: String,
    session_family: String,
    expires_at: i64,
}

impl VerifiedIdentity {
    /// Creates a bounded identity after a JWT verifier has validated its claims.
    pub fn new(
        subject: impl Into<String>,
        session_family: impl Into<String>,
        expires_at: i64,
    ) -> Result<Self, AuthenticationError> {
        let subject = subject.into();
        let session_family = session_family.into();
        if subject.is_empty()
            || subject.len() > 512
            || subject
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            || !bounded_text(&session_family, 512)
            || expires_at < 0
        {
            return Err(AuthenticationError::InvalidToken);
        }
        Ok(Self {
            subject,
            session_family,
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

    /// Returns the Cognito token-family identifier used for Cipher session revocation.
    pub fn session_family(&self) -> &str {
        &self.session_family
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

/// Revokes the application session identified by one verified Cognito token family.
///
/// This operation is intentionally idempotent: a retry after an interrupted
/// response must be able to confirm that local cleanup can be discarded.
pub trait SessionRevoker: Send + Sync {
    /// Revokes the Cipher application session for one validated token family.
    fn revoke_session(&self, identity: &VerifiedIdentity) -> Result<(), AuthenticationError>;
}

/// One atomically read application authorization record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalState {
    /// Immutable Cipher user mapped from the Cognito subject.
    pub user_id: String,
    /// Device bound to the current session.
    pub device_id: String,
    /// Session identified by the access-token family.
    pub session_id: String,
    /// Whether the Cipher user remains enabled.
    pub user_enabled: bool,
    /// Whether the bound device remains active.
    pub device_active: bool,
    /// Whether the bound session remains active.
    pub session_active: bool,
}

/// Performs the one strongly consistent identity/session/device lookup.
///
/// A production DynamoDB implementation must read all three authorization
/// records in one consistent transaction or reject the request. Separating
/// them into eventual reads would permit a revoked session to race a request.
pub trait ConsistentPrincipalStore: Send + Sync {
    /// Resolves the current Cipher authorization state for a verified subject.
    fn load(
        &self,
        identity: &VerifiedIdentity,
    ) -> Result<Option<PrincipalState>, AuthenticationError>;
}

/// Applies Cipher's revocation policy to one strongly consistent state record.
pub struct StoredPrincipalGate<S> {
    store: S,
}

impl<S> StoredPrincipalGate<S> {
    /// Creates the application-state gate used after JWT verification.
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S: ConsistentPrincipalStore> PrincipalGate for StoredPrincipalGate<S> {
    fn authorize(
        &self,
        identity: VerifiedIdentity,
    ) -> Result<CipherPrincipal, AuthenticationError> {
        let Some(state) = self.store.load(&identity)? else {
            return Err(AuthenticationError::Revoked);
        };
        if !state.user_enabled
            || !state.device_active
            || !state.session_active
            || !opaque_identifier(&state.user_id, "usr_")
            || !opaque_identifier(&state.device_id, "dev_")
            || !opaque_identifier(&state.session_id, "ses_")
        {
            return Err(AuthenticationError::Revoked);
        }
        Ok(CipherPrincipal {
            identity,
            user_id: state.user_id,
            device_id: state.device_id,
            session_id: state.session_id,
        })
    }
}

/// Reads the Cipher identity, session, and device records from DynamoDB.
///
/// Each read is strongly consistent. The immutable identity claim is resolved
/// first; the session's device binding is then checked against active profile,
/// session, and device records before a principal can be issued.
#[derive(Clone)]
pub struct DynamoPrincipalStore {
    client: DynamoDbClient,
    users_table: String,
}

impl DynamoPrincipalStore {
    /// Creates a principal store for Cipher's users table.
    pub fn new(
        client: DynamoDbClient,
        users_table: impl Into<String>,
    ) -> Result<Self, AuthenticationError> {
        let users_table = users_table.into();
        if !bounded_text(&users_table, 255) {
            return Err(AuthenticationError::SigningKeysUnavailable);
        }
        Ok(Self {
            client,
            users_table,
        })
    }
}

impl ConsistentPrincipalStore for DynamoPrincipalStore {
    fn load(
        &self,
        identity: &VerifiedIdentity,
    ) -> Result<Option<PrincipalState>, AuthenticationError> {
        let identity_item = dynamo_get(
            &self.client,
            &self.users_table,
            format!("IDENTITY#COGNITO#{}", identity.subject()),
            "CLAIM".into(),
        )?;
        let Some(identity_item) = identity_item else {
            return Ok(None);
        };
        let Some(user_id) = string_attribute(&identity_item, "user_id") else {
            return Ok(None);
        };
        if !opaque_identifier(user_id, "usr_") {
            return Ok(None);
        }

        let profile = dynamo_get(
            &self.client,
            &self.users_table,
            format!("USER#{user_id}"),
            "PROFILE".into(),
        )?;
        let session = dynamo_get(
            &self.client,
            &self.users_table,
            format!("USER#{user_id}"),
            format!("SESSION#{}", identity.session_family()),
        )?;
        let (Some(profile), Some(session)) = (profile, session) else {
            return Ok(None);
        };
        let Some(device_id) = string_attribute(&session, "device_id") else {
            return Ok(None);
        };
        if !opaque_identifier(device_id, "dev_") {
            return Ok(None);
        }
        let device = dynamo_get(
            &self.client,
            &self.users_table,
            format!("USER#{user_id}"),
            format!("DEVICE#{device_id}"),
        )?;
        let Some(device) = device else {
            return Ok(None);
        };
        Ok(Some(PrincipalState {
            user_id: user_id.into(),
            device_id: device_id.into(),
            session_id: format!("ses_{}", identity.session_family()),
            user_enabled: string_attribute(&profile, "status") == Some("active"),
            device_active: string_attribute(&device, "status") == Some("active"),
            session_active: string_attribute(&session, "status") == Some("active"),
        }))
    }
}

impl SessionRevoker for DynamoPrincipalStore {
    fn revoke_session(&self, identity: &VerifiedIdentity) -> Result<(), AuthenticationError> {
        let identity_item = dynamo_get(
            &self.client,
            &self.users_table,
            format!("IDENTITY#COGNITO#{}", identity.subject()),
            "CLAIM".into(),
        )?;
        let Some(identity_item) = identity_item else {
            return Ok(());
        };
        let Some(user_id) = string_attribute(&identity_item, "user_id") else {
            return Ok(());
        };
        if !opaque_identifier(user_id, "usr_") {
            return Ok(());
        }

        dynamo_revoke_session(
            &self.client,
            &self.users_table,
            user_id,
            identity.session_family(),
        )
    }
}

fn dynamo_get(
    client: &DynamoDbClient,
    table: &str,
    partition_key: String,
    sort_key: String,
) -> Result<Option<std::collections::HashMap<String, AttributeValue>>, AuthenticationError> {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(
                client
                    .get_item()
                    .table_name(table)
                    .key("pk", AttributeValue::S(partition_key))
                    .key("sk", AttributeValue::S(sort_key))
                    .consistent_read(true)
                    .send(),
            )
            .map_err(Box::new)
    })
    .map_err(|_| AuthenticationError::SigningKeysUnavailable)
    .map(|response| response.item)
}

fn dynamo_revoke_session(
    client: &DynamoDbClient,
    table: &str,
    user_id: &str,
    session_family: &str,
) -> Result<(), AuthenticationError> {
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            client
                .update_item()
                .table_name(table)
                .key("pk", AttributeValue::S(format!("USER#{user_id}")))
                .key("sk", AttributeValue::S(format!("SESSION#{session_family}")))
                .update_expression("SET #status = :revoked")
                .condition_expression("attribute_exists(pk) AND attribute_exists(sk)")
                .expression_attribute_names("#status", "status")
                .expression_attribute_values(":revoked", AttributeValue::S("revoked".into()))
                .send()
                .await
                .map_err(Box::new)
        })
    });
    match result {
        Ok(_) => Ok(()),
        Err(error)
            if error
                .as_service_error()
                .is_some_and(|service| service.is_conditional_check_failed_exception()) =>
        {
            // The record is already absent or has already been revoked. Neither
            // outcome can restore access, so treating it as complete keeps this
            // client retry operation safe and idempotent.
            Ok(())
        }
        Err(_) => Err(AuthenticationError::SigningKeysUnavailable),
    }
}

fn string_attribute<'a>(
    item: &'a std::collections::HashMap<String, AttributeValue>,
    name: &str,
) -> Option<&'a str> {
    item.get(name)
        .and_then(|value| value.as_s().ok())
        .map(String::as_str)
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

/// Authorizes one HTTP or realtime handshake through Cipher's shared principal path.
pub trait RequestAuthorizer: Send + Sync {
    /// Returns the active Cipher principal or a safe rejection category.
    fn authorize_request(
        &self,
        headers: &HeaderMap,
        unix_time_seconds: i64,
    ) -> Result<CipherPrincipal, AuthenticationError>;

    /// Revokes the application session identified by a valid Cognito access token.
    ///
    /// Unlike normal request authorization, this must remain retryable once the
    /// session was marked revoked. It validates token ownership and then applies
    /// an idempotent state transition.
    fn revoke_current_session(
        &self,
        headers: &HeaderMap,
        unix_time_seconds: i64,
    ) -> Result<(), AuthenticationError>;
}

/// Combines a token verifier with the strongly consistent Cipher state gate.
pub struct ServerAuthorizer<V, G, R> {
    validator: V,
    gate: G,
    revoker: R,
}

impl<V, G, R> ServerAuthorizer<V, G, R> {
    /// Creates the one authorizer shared by HTTP and realtime entry points.
    pub fn new(validator: V, gate: G, revoker: R) -> Self {
        Self {
            validator,
            gate,
            revoker,
        }
    }
}

impl<V: AccessTokenValidator, G: PrincipalGate, R: SessionRevoker> RequestAuthorizer
    for ServerAuthorizer<V, G, R>
{
    fn authorize_request(
        &self,
        headers: &HeaderMap,
        unix_time_seconds: i64,
    ) -> Result<CipherPrincipal, AuthenticationError> {
        authenticate(headers, &self.validator, &self.gate, unix_time_seconds)
    }

    fn revoke_current_session(
        &self,
        headers: &HeaderMap,
        unix_time_seconds: i64,
    ) -> Result<(), AuthenticationError> {
        let token = bearer_token(headers)?;
        let identity = self.validator.validate(token, unix_time_seconds)?;
        self.revoker.revoke_session(&identity)
    }
}

/// The fixed issuer, public client, and scopes accepted from Cognito.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CognitoTokenPolicy {
    issuer: String,
    client_id: String,
    required_scopes: BTreeSet<String>,
    jwks_max_age: Duration,
}

impl CognitoTokenPolicy {
    /// Creates a fail-closed Cognito access-token policy.
    pub fn new(
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        required_scopes: impl IntoIterator<Item = impl Into<String>>,
        jwks_max_age: Duration,
    ) -> Result<Self, AuthenticationError> {
        let issuer = issuer.into();
        let client_id = client_id.into();
        if !bounded_text(&issuer, 2048)
            || !bounded_text(&client_id, 256)
            || jwks_max_age.is_zero()
            || jwks_max_age > Duration::from_secs(24 * 60 * 60)
        {
            return Err(AuthenticationError::InvalidToken);
        }
        let required_scopes = required_scopes
            .into_iter()
            .map(Into::into)
            .collect::<BTreeSet<String>>();
        if required_scopes.is_empty()
            || required_scopes.len() > 32
            || required_scopes.iter().any(|scope| !valid_scope(scope))
        {
            return Err(AuthenticationError::InvalidToken);
        }
        Ok(Self {
            issuer,
            client_id,
            required_scopes,
            jwks_max_age,
        })
    }
}

/// Supplies a JWKS document from the policy's exact Cognito issuer endpoint.
pub trait JwksSource: Send + Sync {
    /// Returns the unmodified current JWKS payload.
    fn fetch_jwks(&self) -> Result<String, AuthenticationError>;
}

/// HTTPS JWKS source for a single, already validated Cognito issuer.
pub struct HttpJwksSource {
    client: reqwest::blocking::Client,
    url: reqwest::Url,
}

impl HttpJwksSource {
    /// Creates the exact Cognito JWKS endpoint derived from an issuer URL.
    pub fn new(issuer: &str) -> Result<Self, AuthenticationError> {
        let issuer =
            reqwest::Url::parse(issuer).map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
        if issuer.scheme() != "https" || issuer.query().is_some() || issuer.fragment().is_some() {
            return Err(AuthenticationError::SigningKeysUnavailable);
        }
        let url = issuer
            .join(".well-known/jwks.json")
            .map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(JWKS_CONNECT_TIMEOUT)
            .timeout(JWKS_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
        Ok(Self { client, url })
    }
}

impl JwksSource for HttpJwksSource {
    fn fetch_jwks(&self) -> Result<String, AuthenticationError> {
        let response = self
            .client
            .get(self.url.clone())
            .send()
            .map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_JWKS_BYTES as u64)
        {
            return Err(AuthenticationError::SigningKeysUnavailable);
        }
        let document = response
            .text()
            .map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
        if document.len() > MAX_JWKS_BYTES {
            return Err(AuthenticationError::SigningKeysUnavailable);
        }
        Ok(document)
    }
}

/// A bounded cached Cognito RS256 verifier suitable for server requests.
pub struct CognitoJwtValidator<S> {
    policy: CognitoTokenPolicy,
    source: S,
    cache: Mutex<Option<CachedJwks>>,
}

impl<S> CognitoJwtValidator<S> {
    /// Creates a verifier with no signing-key cache.
    pub fn new(policy: CognitoTokenPolicy, source: S) -> Self {
        Self {
            policy,
            source,
            cache: Mutex::new(None),
        }
    }
}

impl<S: JwksSource> AccessTokenValidator for CognitoJwtValidator<S> {
    fn validate(
        &self,
        token: &str,
        unix_time_seconds: i64,
    ) -> Result<VerifiedIdentity, AuthenticationError> {
        if unix_time_seconds < 0 || token.split('.').count() != 3 {
            return Err(AuthenticationError::InvalidToken);
        }
        let header = decode_header(token).map_err(|_| AuthenticationError::InvalidToken)?;
        if header.alg != Algorithm::RS256 {
            return Err(AuthenticationError::InvalidToken);
        }
        let key_id = header
            .kid
            .filter(|value| bounded_text(value, MAX_KEY_ID_BYTES))
            .ok_or(AuthenticationError::InvalidToken)?;
        let key = self.key_for(&key_id, unix_time_seconds)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = false;
        let claims = decode::<CognitoClaims>(token, &key, &validation)
            .map_err(|_| AuthenticationError::InvalidToken)?
            .claims;
        if claims.iss != self.policy.issuer
            || claims.client_id != self.policy.client_id
            || claims.token_use != "access"
            || claims.exp <= unix_time_seconds
        {
            return Err(AuthenticationError::InvalidToken);
        }
        let scopes = claims
            .scope
            .split_ascii_whitespace()
            .collect::<BTreeSet<_>>();
        if scopes.is_empty()
            || !self
                .policy
                .required_scopes
                .iter()
                .all(|scope| scopes.contains(scope.as_str()))
        {
            return Err(AuthenticationError::InvalidToken);
        }
        VerifiedIdentity::new(claims.sub, claims.origin_jti, claims.exp)
    }
}

impl<S: JwksSource> CognitoJwtValidator<S> {
    fn key_for(&self, key_id: &str, now: i64) -> Result<DecodingKey, AuthenticationError> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
        let refresh = cache.as_ref().is_none_or(|cached| {
            now < cached.fetched_at
                || u64::try_from(now - cached.fetched_at).unwrap_or(u64::MAX)
                    >= self.policy.jwks_max_age.as_secs()
                || !cached.keys.contains_key(key_id)
        });
        if refresh {
            let document = self.source.fetch_jwks()?;
            *cache = Some(parse_jwks(&document, now)?);
        }
        cache
            .as_ref()
            .and_then(|cached| cached.keys.get(key_id))
            .cloned()
            .ok_or(AuthenticationError::InvalidToken)
    }
}

#[derive(Deserialize)]
struct CognitoClaims {
    iss: String,
    client_id: String,
    token_use: String,
    exp: i64,
    scope: String,
    sub: String,
    origin_jti: String,
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
struct CachedJwks {
    fetched_at: i64,
    keys: BTreeMap<String, DecodingKey>,
}

fn parse_jwks(document: &str, fetched_at: i64) -> Result<CachedJwks, AuthenticationError> {
    if document.is_empty() || document.len() > MAX_JWKS_BYTES {
        return Err(AuthenticationError::SigningKeysUnavailable);
    }
    let document: JwksDocument =
        serde_json::from_str(document).map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
    if document.keys.is_empty() || document.keys.len() > MAX_JWKS_KEYS {
        return Err(AuthenticationError::SigningKeysUnavailable);
    }
    let mut keys = BTreeMap::new();
    for key in document.keys {
        if key.kty != "RSA"
            || key.alg != "RS256"
            || key.usage != "sig"
            || !bounded_text(&key.kid, MAX_KEY_ID_BYTES)
            || !base64url(&key.n, 1024)
            || !base64url(&key.e, 16)
        {
            return Err(AuthenticationError::SigningKeysUnavailable);
        }
        let decoding_key = DecodingKey::from_rsa_components(&key.n, &key.e)
            .map_err(|_| AuthenticationError::SigningKeysUnavailable)?;
        if keys.insert(key.kid, decoding_key).is_some() {
            return Err(AuthenticationError::SigningKeysUnavailable);
        }
    }
    Ok(CachedJwks { fetched_at, keys })
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control())
}
fn valid_scope(value: &str) -> bool {
    bounded_text(value, 256)
        && value
            .bytes()
            .all(|byte| matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e))
}
fn base64url(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn opaque_identifier(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 128
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{
        AuthenticationError, ConsistentPrincipalStore, PrincipalGate, PrincipalState,
        StoredPrincipalGate, VerifiedIdentity, bearer_token,
    };

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
        assert!(VerifiedIdentity::new("sub_123", "origin_123", 1).is_ok());
        assert_eq!(
            VerifiedIdentity::new("", "origin_123", 1),
            Err(AuthenticationError::InvalidToken)
        );
        assert_eq!(
            VerifiedIdentity::new("sub", "origin_123", -1),
            Err(AuthenticationError::InvalidToken)
        );
    }

    #[test]
    fn state_gate_rejects_each_revoked_application_record() {
        for state in [
            None,
            Some(PrincipalState {
                user_id: "usr_1".into(),
                device_id: "dev_1".into(),
                session_id: "ses_1".into(),
                user_enabled: false,
                device_active: true,
                session_active: true,
            }),
            Some(PrincipalState {
                user_id: "usr_1".into(),
                device_id: "dev_1".into(),
                session_id: "ses_1".into(),
                user_enabled: true,
                device_active: false,
                session_active: true,
            }),
            Some(PrincipalState {
                user_id: "usr_1".into(),
                device_id: "dev_1".into(),
                session_id: "ses_1".into(),
                user_enabled: true,
                device_active: true,
                session_active: false,
            }),
        ] {
            let gate = StoredPrincipalGate::new(TestStore(state));
            assert_eq!(
                gate.authorize(VerifiedIdentity::new("sub_1", "origin_1", 1).unwrap()),
                Err(AuthenticationError::Revoked)
            );
        }
    }

    #[test]
    fn state_gate_only_allows_an_active_bounded_principal() {
        let gate = StoredPrincipalGate::new(TestStore(Some(PrincipalState {
            user_id: "usr_1".into(),
            device_id: "dev_1".into(),
            session_id: "ses_1".into(),
            user_enabled: true,
            device_active: true,
            session_active: true,
        })));
        let principal = gate
            .authorize(VerifiedIdentity::new("sub_1", "origin_1", 1).unwrap())
            .unwrap();
        assert_eq!(principal.user_id, "usr_1");
    }

    struct TestStore(Option<PrincipalState>);
    impl ConsistentPrincipalStore for TestStore {
        fn load(
            &self,
            _identity: &VerifiedIdentity,
        ) -> Result<Option<PrincipalState>, AuthenticationError> {
            Ok(self.0.clone())
        }
    }
}
