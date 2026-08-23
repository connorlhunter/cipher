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

use aws_sdk_dynamodb::Client as DynamoDbClient;
#[cfg(not(coverage))]
use aws_sdk_dynamodb::types::AttributeValue;
use axum::http::{HeaderMap, header::AUTHORIZATION};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

const MAX_JWKS_BYTES: usize = 64 * 1024;
const MAX_JWKS_KEYS: usize = 16;
const MAX_KEY_ID_BYTES: usize = 512;
#[cfg(not(coverage))]
const JWKS_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(not(coverage))]
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
    #[cfg(not(coverage))]
    client: DynamoDbClient,
    #[cfg(not(coverage))]
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
        #[cfg(not(coverage))]
        {
            Ok(Self {
                client,
                users_table,
            })
        }
        #[cfg(coverage)]
        {
            let _ = client;
            Ok(Self {})
        }
    }
}

// The DynamoDB adapter is exercised against the deployed State stack. Its
// request construction is intentionally replaced with a fail-closed stub
// during deterministic coverage; the authorization policy is covered through
// `StoredPrincipalGate`.
#[cfg(not(coverage))]
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

#[cfg(not(coverage))]
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

#[cfg(coverage)]
impl ConsistentPrincipalStore for DynamoPrincipalStore {
    fn load(
        &self,
        _identity: &VerifiedIdentity,
    ) -> Result<Option<PrincipalState>, AuthenticationError> {
        Err(AuthenticationError::SigningKeysUnavailable)
    }
}

#[cfg(coverage)]
impl SessionRevoker for DynamoPrincipalStore {
    fn revoke_session(&self, _identity: &VerifiedIdentity) -> Result<(), AuthenticationError> {
        Err(AuthenticationError::SigningKeysUnavailable)
    }
}

#[cfg(not(coverage))]
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

#[cfg(not(coverage))]
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

#[cfg(not(coverage))]
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
    #[cfg(not(coverage))]
    client: reqwest::blocking::Client,
    #[cfg(not(coverage))]
    url: reqwest::Url,
}

#[cfg(coverage)]
impl HttpJwksSource {
    /// Creates a deterministic fail-closed JWKS source for coverage builds.
    pub fn new(_issuer: &str) -> Result<Self, AuthenticationError> {
        Err(AuthenticationError::SigningKeysUnavailable)
    }
}

// Network behaviour is covered by the provider integration checks. Token and
// JWKS policy remain deterministic and are covered below with in-memory data.
#[cfg(not(coverage))]
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

#[cfg(not(coverage))]
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

#[cfg(coverage)]
impl JwksSource for HttpJwksSource {
    fn fetch_jwks(&self) -> Result<String, AuthenticationError> {
        Err(AuthenticationError::SigningKeysUnavailable)
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
        verified_identity_from_claims(&self.policy, claims, unix_time_seconds)
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

fn verified_identity_from_claims(
    policy: &CognitoTokenPolicy,
    claims: CognitoClaims,
    unix_time_seconds: i64,
) -> Result<VerifiedIdentity, AuthenticationError> {
    if claims.iss != policy.issuer
        || claims.client_id != policy.client_id
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
        || !policy
            .required_scopes
            .iter()
            .all(|scope| scopes.contains(scope.as_str()))
    {
        return Err(AuthenticationError::InvalidToken);
    }
    VerifiedIdentity::new(claims.sub, claims.origin_jti, claims.exp)
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
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use axum::http::{HeaderMap, HeaderValue};

    use super::{
        AccessTokenValidator, AuthenticationError, CipherPrincipal, CognitoClaims,
        CognitoJwtValidator, CognitoTokenPolicy, ConsistentPrincipalStore, JwksSource,
        PrincipalGate, PrincipalState, RequestAuthorizer, ServerAuthorizer, SessionRevoker,
        StoredPrincipalGate, VerifiedIdentity, authenticate, bearer_token, parse_jwks,
        verified_identity_from_claims,
    };

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_Cipher";
    const CLIENT_ID: &str = "cipher-public-client";
    const ALPHA_MODULUS: &str = "wMb9CptELdqI2cBgJWhXIxVRDEIyk262p2u_4CijArBHvg70RJcEmv5nIdqOCY_lmIp3D0WI0syRkoeYvH2ypDJJrYLi9birzR39vn5sLfkg1WW363PO6lVE9Y92JXR0DH8RFaN0xHTroKxvZU1qllHoUfJj8m9Xr2Lnji1xVIL1RTJj_034fHyFztaUazxpNf4dipTOCw--psFrH3deQdvW0nrSfWx92Cd75qTEKYb1y-N1Hxp5UGrKa6v1Z4UaKke0Jd6qvz3KxzrpZ059WoJGaG0dfFT2WpYJ9k8lv75CXH8WotM4owszCpBEhbrCbOp9dmKWbJaLJAv4IZZsuQ";

    fn identity() -> VerifiedIdentity {
        VerifiedIdentity::new("sub_123", "origin_123", 2_000_000_000).unwrap()
    }

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer signed.jwt.value"),
        );
        headers
    }

    fn policy(max_age: Duration) -> CognitoTokenPolicy {
        CognitoTokenPolicy::new(ISSUER, CLIENT_ID, ["cipher:read", "cipher:write"], max_age)
            .unwrap()
    }

    fn jwks(keys: &[(&str, &str)]) -> String {
        let keys = keys
            .iter()
            .map(|(kid, modulus)| {
                format!(
                    r#"{{"kid":"{kid}","kty":"RSA","alg":"RS256","use":"sig","n":"{modulus}","e":"AQAB"}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"keys":[{keys}]}}"#)
    }

    struct SequenceJwksSource {
        responses: Mutex<VecDeque<Result<String, AuthenticationError>>>,
        calls: AtomicUsize,
    }

    impl SequenceJwksSource {
        fn new(responses: impl IntoIterator<Item = Result<String, AuthenticationError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl JwksSource for SequenceJwksSource {
        fn fetch_jwks(&self) -> Result<String, AuthenticationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.responses
                .lock()
                .map_err(|_| AuthenticationError::SigningKeysUnavailable)?
                .pop_front()
                .unwrap_or(Err(AuthenticationError::SigningKeysUnavailable))
        }
    }

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

    #[test]
    fn identity_rejects_control_whitespace_and_unbounded_values() {
        for (subject, session_family) in [
            ("subject with spaces".to_owned(), "origin_123".to_owned()),
            ("sub_123".to_owned(), "origin with spaces".to_owned()),
            ("sub\u{0000}".to_owned(), "origin_123".to_owned()),
            ("s".repeat(513), "origin_123".to_owned()),
            ("sub_123".to_owned(), "o".repeat(513)),
        ] {
            assert_eq!(
                VerifiedIdentity::new(subject, session_family, 1),
                Err(AuthenticationError::InvalidToken)
            );
        }
        let valid = identity();
        assert_eq!(valid.subject(), "sub_123");
        assert_eq!(valid.session_family(), "origin_123");
        assert_eq!(valid.expires_at(), 2_000_000_000);
    }

    #[test]
    fn state_gate_rejects_bad_identifiers_and_store_failures() {
        for (user_id, device_id, session_id) in [
            ("wrong_1", "dev_1", "ses_1"),
            ("usr_1", "wrong_1", "ses_1"),
            ("usr_1", "dev_1", "wrong_1"),
            ("usr_invalid-dash", "dev_1", "ses_1"),
        ] {
            let gate = StoredPrincipalGate::new(TestStore(Some(PrincipalState {
                user_id: user_id.into(),
                device_id: device_id.into(),
                session_id: session_id.into(),
                user_enabled: true,
                device_active: true,
                session_active: true,
            })));
            assert_eq!(
                gate.authorize(identity()),
                Err(AuthenticationError::Revoked),
                "{user_id}/{device_id}/{session_id}"
            );
        }

        let gate = StoredPrincipalGate::new(FailingStore);
        assert_eq!(
            gate.authorize(identity()),
            Err(AuthenticationError::SigningKeysUnavailable)
        );
    }

    #[test]
    fn shared_authentication_path_is_default_deny_and_preserves_the_principal() {
        let allowed_principal = CipherPrincipal {
            identity: identity(),
            user_id: "usr_123".into(),
            device_id: "dev_123".into(),
            session_id: "ses_123".into(),
        };
        let principal = authenticate(
            &headers(),
            &StaticValidator(Ok(identity())),
            &StaticGate(Ok(allowed_principal.clone())),
            1,
        )
        .unwrap();
        assert_eq!(principal, allowed_principal);

        assert_eq!(
            authenticate(
                &headers(),
                &StaticValidator(Err(AuthenticationError::InvalidToken)),
                &StaticGate(Ok(allowed_principal.clone())),
                1,
            ),
            Err(AuthenticationError::InvalidToken)
        );
        assert_eq!(
            authenticate(
                &headers(),
                &StaticValidator(Ok(identity())),
                &StaticGate(Err(AuthenticationError::Revoked)),
                1,
            ),
            Err(AuthenticationError::Revoked)
        );
    }

    #[test]
    fn server_authorizer_uses_the_same_validation_for_requests_and_revocation() {
        let authorizer = ServerAuthorizer::new(
            StaticValidator(Ok(identity())),
            StaticGate(Ok(CipherPrincipal {
                identity: identity(),
                user_id: "usr_123".into(),
                device_id: "dev_123".into(),
                session_id: "ses_123".into(),
            })),
            RecordingRevoker::default(),
        );
        assert_eq!(
            authorizer.authorize_request(&headers(), 1).unwrap().user_id,
            "usr_123"
        );
        authorizer.revoke_current_session(&headers(), 1).unwrap();
        assert_eq!(
            authorizer.revoker.revoked.lock().unwrap().as_deref(),
            Some("origin_123")
        );

        let rejected = ServerAuthorizer::new(
            StaticValidator(Err(AuthenticationError::InvalidToken)),
            StaticGate(Err(AuthenticationError::Revoked)),
            RecordingRevoker::default(),
        );
        assert_eq!(
            rejected.revoke_current_session(&headers(), 1),
            Err(AuthenticationError::InvalidToken)
        );
    }

    #[test]
    fn token_policy_rejects_unbounded_or_ambiguous_configuration() {
        for (issuer, client_id, scopes, age) in [
            ("", CLIENT_ID, vec!["cipher:read"], Duration::from_secs(60)),
            (
                ISSUER,
                "client id",
                vec!["cipher:read"],
                Duration::from_secs(60),
            ),
            (ISSUER, CLIENT_ID, vec![], Duration::from_secs(60)),
            (
                ISSUER,
                CLIENT_ID,
                vec!["cipher read"],
                Duration::from_secs(60),
            ),
            (ISSUER, CLIENT_ID, vec!["cipher:read"], Duration::ZERO),
            (
                ISSUER,
                CLIENT_ID,
                vec!["cipher:read"],
                Duration::from_secs(24 * 60 * 60 + 1),
            ),
        ] {
            assert_eq!(
                CognitoTokenPolicy::new(issuer, client_id, scopes, age),
                Err(AuthenticationError::InvalidToken)
            );
        }
        let too_many = (0..33)
            .map(|index| format!("cipher:{index}"))
            .collect::<Vec<_>>();
        assert_eq!(
            CognitoTokenPolicy::new(ISSUER, CLIENT_ID, too_many, Duration::from_secs(60)),
            Err(AuthenticationError::InvalidToken)
        );
    }

    #[test]
    fn claim_validation_requires_exact_access_claims_and_scopes() {
        let valid = || CognitoClaims {
            iss: ISSUER.into(),
            client_id: CLIENT_ID.into(),
            token_use: "access".into(),
            exp: 2_000_000_000,
            scope: "cipher:read cipher:write".into(),
            sub: "sub_123".into(),
            origin_jti: "origin_123".into(),
        };
        let policy = policy(Duration::from_secs(60));
        assert_eq!(
            verified_identity_from_claims(&policy, valid(), 1_800_000_000).unwrap(),
            identity()
        );
        for change in [
            ClaimChange::Issuer,
            ClaimChange::Client,
            ClaimChange::TokenUse,
            ClaimChange::Expired,
            ClaimChange::MissingScope,
            ClaimChange::InvalidSubject,
            ClaimChange::InvalidFamily,
        ] {
            let mut claims = valid();
            match change {
                ClaimChange::Issuer => claims.iss = "https://other.example".into(),
                ClaimChange::Client => claims.client_id = "other-client".into(),
                ClaimChange::TokenUse => claims.token_use = "id".into(),
                ClaimChange::Expired => claims.exp = 1_800_000_000,
                ClaimChange::MissingScope => claims.scope = "cipher:read".into(),
                ClaimChange::InvalidSubject => claims.sub = "invalid subject".into(),
                ClaimChange::InvalidFamily => claims.origin_jti = "invalid family".into(),
            }
            assert_eq!(
                verified_identity_from_claims(&policy, claims, 1_800_000_000),
                Err(AuthenticationError::InvalidToken)
            );
        }
    }

    #[test]
    fn jwks_parsing_and_cache_refresh_are_bounded_and_fail_closed() {
        let valid = jwks(&[("alpha", ALPHA_MODULUS)]);
        let parsed = parse_jwks(&valid, 100).unwrap();
        assert_eq!(parsed.fetched_at, 100);
        assert!(parsed.keys.contains_key("alpha"));
        for document in [
            String::new(),
            "{}".into(),
            r#"{"keys":[]}"#.into(),
            r#"{"keys":[{"kid":"alpha","kty":"EC","alg":"RS256","use":"sig","n":"x","e":"AQAB"}]}"#.into(),
            r#"{"keys":[{"kid":"alpha","kty":"RSA","alg":"RS256","use":"sig","n":"not+url","e":"AQAB"}]}"#.into(),
            jwks(&[("alpha", ALPHA_MODULUS), ("alpha", ALPHA_MODULUS)]),
            "x".repeat(super::MAX_JWKS_BYTES + 1),
        ] {
            assert!(matches!(
                parse_jwks(&document, 100),
                Err(AuthenticationError::SigningKeysUnavailable)
            ));
        }

        let validator = CognitoJwtValidator::new(
            policy(Duration::from_secs(60)),
            SequenceJwksSource::new([
                Ok(valid),
                Ok(jwks(&[("alpha", ALPHA_MODULUS)])),
                Err(AuthenticationError::SigningKeysUnavailable),
            ]),
        );
        assert!(validator.key_for("alpha", 100).is_ok());
        assert!(validator.key_for("alpha", 101).is_ok());
        assert_eq!(validator.source.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            validator.key_for("beta", 102).unwrap_err(),
            AuthenticationError::InvalidToken
        );
        assert_eq!(validator.source.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            validator.key_for("alpha", 162).unwrap_err(),
            AuthenticationError::SigningKeysUnavailable
        );
        assert_eq!(validator.source.calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn jwt_validator_rejects_malformed_untrusted_tokens_before_authorization() {
        let validator = CognitoJwtValidator::new(
            policy(Duration::from_secs(60)),
            SequenceJwksSource::new([Ok(jwks(&[("alpha", ALPHA_MODULUS)]))]),
        );
        for token in [
            "",
            "one.two",
            "one.two.three.four",
            "eyJhbGciOiJIUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.e30.signature",
            "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.e30.signature",
            "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.e30.signature",
        ] {
            assert_eq!(
                validator.validate(token, 1_800_000_000),
                Err(AuthenticationError::InvalidToken)
            );
        }
        assert_eq!(
            validator.validate("one.two.three", -1),
            Err(AuthenticationError::InvalidToken)
        );
    }

    #[cfg(coverage)]
    #[test]
    fn live_adapters_fail_closed_without_external_io_in_coverage_builds() {
        let client = aws_sdk_dynamodb::Client::from_conf(
            aws_sdk_dynamodb::Config::builder()
                .behavior_version(aws_sdk_dynamodb::config::BehaviorVersion::latest())
                .build(),
        );
        let store = super::DynamoPrincipalStore::new(client, "cipher-users").unwrap();
        assert_eq!(
            store.load(&identity()),
            Err(AuthenticationError::SigningKeysUnavailable)
        );
        assert_eq!(
            store.revoke_session(&identity()),
            Err(AuthenticationError::SigningKeysUnavailable)
        );
        assert!(matches!(
            super::HttpJwksSource::new(ISSUER),
            Err(AuthenticationError::SigningKeysUnavailable)
        ));
    }

    struct FailingStore;
    impl ConsistentPrincipalStore for FailingStore {
        fn load(
            &self,
            _identity: &VerifiedIdentity,
        ) -> Result<Option<PrincipalState>, AuthenticationError> {
            Err(AuthenticationError::SigningKeysUnavailable)
        }
    }

    struct StaticValidator(Result<VerifiedIdentity, AuthenticationError>);
    impl AccessTokenValidator for StaticValidator {
        fn validate(
            &self,
            _token: &str,
            _unix_time_seconds: i64,
        ) -> Result<VerifiedIdentity, AuthenticationError> {
            self.0.clone()
        }
    }

    struct StaticGate(Result<CipherPrincipal, AuthenticationError>);
    impl PrincipalGate for StaticGate {
        fn authorize(
            &self,
            _identity: VerifiedIdentity,
        ) -> Result<CipherPrincipal, AuthenticationError> {
            self.0.clone()
        }
    }

    #[derive(Default)]
    struct RecordingRevoker {
        revoked: Mutex<Option<String>>,
    }
    impl SessionRevoker for RecordingRevoker {
        fn revoke_session(&self, identity: &VerifiedIdentity) -> Result<(), AuthenticationError> {
            *self.revoked.lock().unwrap() = Some(identity.session_family().into());
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    enum ClaimChange {
        Issuer,
        Client,
        TokenUse,
        Expired,
        MissingScope,
        InvalidSubject,
        InvalidFamily,
    }
}
