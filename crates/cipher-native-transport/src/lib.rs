//! Rust-owned authenticated HTTP and realtime transport boundaries.
//!
//! This crate deliberately keeps credentials, connection state, transport
//! diagnostics, and realtime frames on the native side. Its public views are
//! bounded Rust values and it does not provide a serialization path for access
//! tokens, authenticated requests, websocket connections, or response bodies.

use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use cipher_realtime_protocol::{
    ClientFrame, MAX_SUBSCRIPTION_CONVERSATIONS, RealtimeCommand, RealtimeEvent,
    RealtimeProtocolError, RealtimeSession, Sequence, ServerFrame,
};
use cipher_types::protocol::{
    ConversationId, ErrorCode, IdempotencyKey, ProtocolVersion, SizeLimits,
};
use serde::Serialize;

/// The largest relative API path or query accepted by the native transport.
pub const MAX_REQUEST_PATH_BYTES: usize = 1_024;
/// The number of reconnect attempts retained before the native client reports exhaustion.
pub const MAX_RECONNECT_ATTEMPTS: u8 = 5;
/// The longest backoff between native realtime reconnect attempts.
pub const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// A safe category for a native transport failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeTransportErrorCode {
    /// A caller cancelled an operation before the native transport completed it.
    Cancelled,
    /// A native request or transport frame violated the boundary contract.
    InvalidRequest,
    /// The locally held session is missing, expired, or otherwise unusable.
    Unauthenticated,
    /// The requested version is unavailable at the connected endpoint.
    UnsupportedVersion,
    /// A native request, response, or frame exceeded a fixed limit.
    TooLarge,
    /// The endpoint or native transport cannot complete the operation now.
    Unavailable,
}

/// A redacted native transport failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeTransportError {
    code: NativeTransportErrorCode,
}

impl NativeTransportError {
    /// Builds a bounded failure from a stable error category.
    pub const fn new(code: NativeTransportErrorCode) -> Self {
        Self { code }
    }

    /// Returns the stable category without exposing a transport implementation error.
    pub const fn code(self) -> NativeTransportErrorCode {
        self.code
    }

    /// Returns a fixed, safe display message.
    pub const fn message(self) -> &'static str {
        match self.code {
            NativeTransportErrorCode::Cancelled => "The operation was cancelled.",
            NativeTransportErrorCode::InvalidRequest => "The native request is invalid.",
            NativeTransportErrorCode::Unauthenticated => "The native session is not available.",
            NativeTransportErrorCode::UnsupportedVersion => {
                "The native transport version is not supported."
            }
            NativeTransportErrorCode::TooLarge => "The native transport payload is too large.",
            NativeTransportErrorCode::Unavailable => "The native transport is unavailable.",
        }
    }

    fn from_realtime(error: RealtimeProtocolError) -> Self {
        let code = match error.code() {
            ErrorCode::Unauthenticated | ErrorCode::Expired => {
                NativeTransportErrorCode::Unauthenticated
            }
            ErrorCode::UnsupportedVersion => NativeTransportErrorCode::UnsupportedVersion,
            ErrorCode::TooLarge => NativeTransportErrorCode::TooLarge,
            ErrorCode::Unavailable | ErrorCode::RateLimited => {
                NativeTransportErrorCode::Unavailable
            }
            _ => NativeTransportErrorCode::InvalidRequest,
        };
        Self::new(code)
    }

    fn from_server_error(code: ErrorCode) -> Self {
        let code = match code {
            ErrorCode::Unauthenticated | ErrorCode::Expired => {
                NativeTransportErrorCode::Unauthenticated
            }
            ErrorCode::UnsupportedVersion => NativeTransportErrorCode::UnsupportedVersion,
            ErrorCode::TooLarge => NativeTransportErrorCode::TooLarge,
            ErrorCode::Unavailable | ErrorCode::RateLimited => {
                NativeTransportErrorCode::Unavailable
            }
            _ => NativeTransportErrorCode::InvalidRequest,
        };
        Self::new(code)
    }
}

impl fmt::Display for NativeTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for NativeTransportError {}

/// A cancellation handle shared with a native transport implementation.
#[derive(Clone, Default)]
pub struct OperationCancellation(Arc<AtomicBool>);

impl OperationCancellation {
    /// Marks the operation as cancelled. Calling this more than once is harmless.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn check(&self) -> Result<(), NativeTransportError> {
        if self.is_cancelled() {
            Err(NativeTransportError::new(
                NativeTransportErrorCode::Cancelled,
            ))
        } else {
            Ok(())
        }
    }
}

/// A non-serializable access token held only by native Rust code.
pub struct AccessToken(String);

impl AccessToken {
    /// Creates a token after native authentication validates its bounded wire value.
    pub fn new(value: String) -> Result<Self, NativeTransportError> {
        if value.is_empty()
            || value.len() > SizeLimits::MAX_HTTP_BODY_BYTES
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::Unauthenticated,
            ));
        }

        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessToken([redacted])")
    }
}

/// Supplies an in-memory access token to native HTTP and realtime clients.
///
/// Implementations may obtain refresh material from the platform credential
/// store, but must never return it through the desktop IPC contract.
pub trait SessionAuthenticator: Send + Sync {
    /// Returns a currently usable access token or a redacted native failure.
    fn access_token(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeTransportError>;
}

/// A validated HTTP API origin owned by the native client.
#[derive(Clone, Eq, PartialEq)]
pub struct NativeHttpOrigin(String);

impl NativeHttpOrigin {
    /// Parses an HTTPS origin, allowing loopback HTTP only for local development.
    pub fn parse(value: &str) -> Result<Self, NativeTransportError> {
        parse_origin(value, "https", "http").map(Self)
    }

    fn join(&self, path: &NativeRequestPath) -> String {
        format!("{}{}", self.0, path.0)
    }
}

impl fmt::Debug for NativeHttpOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeHttpOrigin([configured])")
    }
}

/// A validated WebSocket origin owned by the native client.
#[derive(Clone, Eq, PartialEq)]
pub struct NativeRealtimeOrigin(String);

impl NativeRealtimeOrigin {
    /// Parses a WSS origin, allowing loopback WS only for local development.
    pub fn parse(value: &str) -> Result<Self, NativeTransportError> {
        parse_origin(value, "wss", "ws").map(Self)
    }

    fn connection_url(&self) -> String {
        format!("{}/v1/realtime", self.0)
    }
}

impl fmt::Debug for NativeRealtimeOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeRealtimeOrigin([configured])")
    }
}

fn parse_origin(
    value: &str,
    secure_scheme: &str,
    loopback_scheme: &str,
) -> Result<String, NativeTransportError> {
    if value.is_empty()
        || value.len() > MAX_REQUEST_PATH_BYTES
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(NativeTransportError::new(
            NativeTransportErrorCode::InvalidRequest,
        ));
    }

    let (scheme, remainder) = value
        .split_once("://")
        .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::InvalidRequest))?;
    if remainder.is_empty()
        || remainder.contains(['/', '?', '#', '@'])
        || remainder.starts_with('.')
        || remainder.ends_with('.')
    {
        return Err(NativeTransportError::new(
            NativeTransportErrorCode::InvalidRequest,
        ));
    }

    let host = parse_origin_authority(remainder)?;
    let is_loopback = host.is_loopback;
    if scheme != secure_scheme && !(scheme == loopback_scheme && is_loopback) {
        return Err(NativeTransportError::new(
            NativeTransportErrorCode::InvalidRequest,
        ));
    }

    Ok(format!("{scheme}://{remainder}"))
}

struct NativeOriginHost {
    is_loopback: bool,
}

fn parse_origin_authority(value: &str) -> Result<NativeOriginHost, NativeTransportError> {
    let invalid = || NativeTransportError::new(NativeTransportErrorCode::InvalidRequest);
    let (host, port) = if let Some(remainder) = value.strip_prefix('[') {
        let (host, port) = remainder.split_once(']').ok_or_else(invalid)?;
        let port = match port {
            "" => None,
            port if port.starts_with(':') => Some(&port[1..]),
            _ => return Err(invalid()),
        };
        let address = host.parse::<Ipv6Addr>().map_err(|_| invalid())?;
        (
            NativeOriginHost {
                is_loopback: address.is_loopback(),
            },
            port,
        )
    } else {
        let (host, port) = match value.split_once(':') {
            Some((host, port)) if !port.contains(':') => (host, Some(port)),
            Some(_) => return Err(invalid()),
            None => (value, None),
        };
        if !is_valid_dns_or_ipv4_host(host) {
            return Err(invalid());
        }
        let is_loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<Ipv4Addr>()
                .is_ok_and(|address| address.is_loopback());
        (NativeOriginHost { is_loopback }, port)
    };

    if let Some(port) = port {
        let port = port.parse::<u16>().map_err(|_| invalid())?;
        if port == 0 {
            return Err(invalid());
        }
    }

    Ok(host)
}

fn is_valid_dns_or_ipv4_host(value: &str) -> bool {
    if value.is_empty() || value.len() > 253 {
        return false;
    }
    if value.parse::<Ipv4Addr>().is_ok() {
        return true;
    }

    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// A bounded relative v1 API path sent by native HTTP code.
#[derive(Clone, Eq, PartialEq)]
pub struct NativeRequestPath(String);

impl NativeRequestPath {
    /// Parses a relative `/v1` route with an optional bounded query string.
    pub fn parse(value: &str) -> Result<Self, NativeTransportError> {
        if value.is_empty()
            || value.len() > MAX_REQUEST_PATH_BYTES
            || !(value == "/v1" || value.starts_with("/v1/") || value.starts_with("/v1?"))
            || value.contains("://")
            || value.contains(['#', '\\'])
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::InvalidRequest,
            ));
        }

        Ok(Self(value.into()))
    }
}

impl fmt::Debug for NativeRequestPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeRequestPath([redacted])")
    }
}

/// An allowlisted HTTP method used by the native client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHttpMethod {
    /// Reads a native-owned HTTP resource.
    Get,
    /// Creates a native-owned HTTP resource.
    Post,
    /// Updates a native-owned HTTP resource.
    Patch,
    /// Removes a native-owned HTTP resource.
    Delete,
}

/// A bounded HTTP request that has not yet received native authentication.
pub struct NativeHttpRequest {
    method: NativeHttpMethod,
    path: NativeRequestPath,
    body: Vec<u8>,
}

impl NativeHttpRequest {
    /// Builds a native request with a bounded JSON body.
    pub fn new(
        method: NativeHttpMethod,
        path: NativeRequestPath,
        body: Vec<u8>,
    ) -> Result<Self, NativeTransportError> {
        if body.len() > SizeLimits::MAX_HTTP_BODY_BYTES {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::TooLarge,
            ));
        }

        Ok(Self { method, path, body })
    }

    /// Builds a bodyless native GET request.
    pub fn get(path: NativeRequestPath) -> Self {
        Self {
            method: NativeHttpMethod::Get,
            path,
            body: Vec::new(),
        }
    }
}

impl fmt::Debug for NativeHttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeHttpRequest")
            .field("method", &self.method)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// A bounded native HTTP response retained outside the renderer process.
pub struct NativeHttpResponse {
    status: u16,
    body: Vec<u8>,
}

impl NativeHttpResponse {
    /// Creates a bounded response produced by a native HTTP implementation.
    pub fn new(status: u16, body: Vec<u8>) -> Result<Self, NativeTransportError> {
        if body.len() > SizeLimits::MAX_HTTP_BODY_BYTES {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::TooLarge,
            ));
        }

        Ok(Self { status, body })
    }

    /// Returns the HTTP status without exposing the response body to IPC.
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Consumes the response for native Rust validation and decryption work.
    pub fn into_body(self) -> Vec<u8> {
        self.body
    }
}

impl fmt::Debug for NativeHttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeHttpResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// An authenticated HTTP request visible only to a native HTTP implementation.
pub struct AuthenticatedHttpRequest {
    method: NativeHttpMethod,
    url: String,
    body: Vec<u8>,
    authorization: AccessToken,
}

impl AuthenticatedHttpRequest {
    /// Returns the native-only HTTP method.
    pub const fn method(&self) -> NativeHttpMethod {
        self.method
    }

    /// Returns the native-only absolute request URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the bounded body for native transport serialization.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Returns the bearer token for a native HTTP authorization header.
    pub fn authorization(&self) -> &str {
        self.authorization.as_str()
    }
}

impl fmt::Debug for AuthenticatedHttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedHttpRequest")
            .field("method", &self.method)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// Performs bounded authenticated HTTP work entirely in native Rust.
pub trait NativeHttpTransport {
    /// Sends one authenticated request and returns a bounded native response.
    fn send(
        &mut self,
        request: AuthenticatedHttpRequest,
        cancellation: &OperationCancellation,
    ) -> Result<NativeHttpResponse, NativeTransportError>;
}

/// Owns HTTP authentication and dispatch for one native desktop session.
pub struct NativeHttpClient<T, A> {
    transport: T,
    authenticator: A,
    origin: NativeHttpOrigin,
}

impl<T, A> NativeHttpClient<T, A>
where
    T: NativeHttpTransport,
    A: SessionAuthenticator,
{
    /// Creates a native-only HTTP client for the configured API origin.
    pub fn new(transport: T, authenticator: A, origin: NativeHttpOrigin) -> Self {
        Self {
            transport,
            authenticator,
            origin,
        }
    }

    /// Authenticates and executes one bounded HTTP request outside the webview.
    pub fn execute(
        &mut self,
        request: NativeHttpRequest,
        cancellation: &OperationCancellation,
    ) -> Result<NativeHttpResponse, NativeTransportError> {
        cancellation.check()?;
        let authorization = self.authenticator.access_token(cancellation)?;
        cancellation.check()?;
        let request = AuthenticatedHttpRequest {
            method: request.method,
            url: self.origin.join(&request.path),
            body: request.body,
            authorization,
        };
        let response = self.transport.send(request, cancellation)?;
        cancellation.check()?;
        Ok(response)
    }

    /// Returns the inner native transport after the desktop session ends.
    pub fn into_transport(self) -> T {
        self.transport
    }
}

/// An authenticated WebSocket connection request visible only to native Rust.
pub struct AuthenticatedRealtimeRequest {
    url: String,
    authorization: AccessToken,
}

impl AuthenticatedRealtimeRequest {
    /// Returns the native-only WebSocket URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the bearer token for a native WebSocket authorization header.
    pub fn authorization(&self) -> &str {
        self.authorization.as_str()
    }
}

impl fmt::Debug for AuthenticatedRealtimeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthenticatedRealtimeRequest([redacted])")
    }
}

/// Sends and receives bounded realtime text frames in native Rust.
pub trait NativeRealtimeConnection {
    /// Sends one validated client frame.
    fn send_text(
        &mut self,
        frame: &str,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeTransportError>;

    /// Receives one bounded server frame.
    fn receive_text(
        &mut self,
        cancellation: &OperationCancellation,
    ) -> Result<String, NativeTransportError>;

    /// Closes the native connection without surfacing transport details to the renderer.
    fn close(&mut self);
}

/// Creates a native WebSocket connection from an authenticated native request.
pub trait NativeRealtimeConnector {
    /// Opens the transport connection used by the native realtime client.
    type Connection: NativeRealtimeConnection;

    /// Connects without exposing credentials or capability URLs through IPC.
    fn connect(
        &mut self,
        request: AuthenticatedRealtimeRequest,
        cancellation: &OperationCancellation,
    ) -> Result<Self::Connection, NativeTransportError>;
}

/// The lifecycle state retained by the native realtime client.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRealtimeState {
    /// No connection has been established for the current desktop session.
    Idle,
    /// Native Rust is authenticating and opening the WebSocket connection.
    Connecting,
    /// The native connection completed the versioned handshake.
    Connected,
    /// Native Rust retained state for a bounded reconnect attempt.
    Reconnecting,
    /// The native client exhausted reconnect attempts or was explicitly closed.
    Closed,
}

/// A redacted native transport diagnostic safe for a support export.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTransportDiagnostic {
    /// The current lifecycle state without account, endpoint, or message data.
    pub state: NativeRealtimeState,
    /// The bounded reconnect attempt count.
    pub reconnect_attempt: u8,
    /// The latest stable failure category, when one occurred.
    pub last_error: Option<NativeTransportErrorCode>,
}

/// Owns native WebSocket negotiation, reconnect state, and subscription restoration.
pub struct NativeRealtimeClient<C, A>
where
    C: NativeRealtimeConnector,
    A: SessionAuthenticator,
{
    connector: C,
    authenticator: A,
    origin: NativeRealtimeOrigin,
    connection: Option<C::Connection>,
    session: RealtimeSession,
    subscriptions: Vec<ConversationId>,
    resume_cursor: Option<cipher_types::protocol::Cursor>,
    state: NativeRealtimeState,
    reconnect_attempt: u8,
    last_error: Option<NativeTransportErrorCode>,
}

impl<C, A> NativeRealtimeClient<C, A>
where
    C: NativeRealtimeConnector,
    A: SessionAuthenticator,
{
    /// Creates a disconnected native realtime client for one desktop session.
    pub fn new(connector: C, authenticator: A, origin: NativeRealtimeOrigin) -> Self {
        Self {
            connector,
            authenticator,
            origin,
            connection: None,
            session: RealtimeSession::v1(),
            subscriptions: Vec::new(),
            resume_cursor: None,
            state: NativeRealtimeState::Idle,
            reconnect_attempt: 0,
            last_error: None,
        }
    }

    /// Returns a redacted snapshot suitable for native diagnostics only.
    pub fn diagnostic(&self) -> NativeTransportDiagnostic {
        NativeTransportDiagnostic {
            state: self.state,
            reconnect_attempt: self.reconnect_attempt,
            last_error: self.last_error,
        }
    }

    /// Replaces the subscriptions restored after a successful reconnect.
    pub fn replace_subscriptions(
        &mut self,
        subscriptions: Vec<ConversationId>,
    ) -> Result<(), NativeTransportError> {
        if subscriptions.len() > MAX_SUBSCRIPTION_CONVERSATIONS
            || subscriptions
                .iter()
                .enumerate()
                .any(|(index, id)| subscriptions[..index].contains(id))
        {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::InvalidRequest,
            ));
        }

        self.subscriptions = subscriptions;
        Ok(())
    }

    /// Opens and negotiates the WebSocket connection from native Rust.
    pub fn connect(
        &mut self,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeTransportError> {
        cancellation.check()?;
        self.close_connection();
        self.state = NativeRealtimeState::Connecting;

        let authorization = match self.authenticator.access_token(cancellation) {
            Ok(token) => token,
            Err(error) => return Err(self.record_failure(error)),
        };
        let request = AuthenticatedRealtimeRequest {
            url: self.origin.connection_url(),
            authorization,
        };
        let connection = match self.connector.connect(request, cancellation) {
            Ok(connection) => connection,
            Err(error) => return Err(self.record_failure(error)),
        };
        self.connection = Some(connection);
        self.session = RealtimeSession::v1();

        let hello = ClientFrame::Hello {
            supported_versions: vec![ProtocolVersion::V1, ProtocolVersion::V0],
            resume_cursor: self.resume_cursor.clone(),
            last_acknowledged_server_sequence: None,
        };
        if let Err(error) = self.session.receive(hello.clone()) {
            return Err(self.record_failure(NativeTransportError::from_realtime(error)));
        }
        if let Err(error) = self.send_client_frame(hello, cancellation) {
            return Err(self.record_failure(error));
        }

        let welcome = match self.receive_server_frame(cancellation) {
            Ok(frame @ ServerFrame::Welcome { .. }) => frame,
            Ok(_) => {
                return Err(self.record_failure(NativeTransportError::new(
                    NativeTransportErrorCode::InvalidRequest,
                )));
            }
            Err(error) => return Err(self.record_failure(error)),
        };
        if let Err(error) = self.session.observe_server_frame(&welcome) {
            return Err(self.record_failure(NativeTransportError::from_realtime(error)));
        }

        self.state = NativeRealtimeState::Connected;
        self.reconnect_attempt = 0;
        self.last_error = None;
        Ok(())
    }

    /// Restores all retained subscriptions after a successful native reconnect.
    pub fn restore_subscriptions(
        &mut self,
        sequence: Sequence,
        idempotency_key: IdempotencyKey,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeTransportError> {
        if self.subscriptions.is_empty() {
            return Ok(());
        }
        if self.state != NativeRealtimeState::Connected {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::Unavailable,
            ));
        }

        let protocol_version = self
            .session
            .selected_protocol_version()
            .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unavailable))?;
        let frame = ClientFrame::Command {
            protocol_version,
            sequence,
            idempotency_key,
            command: RealtimeCommand::Subscribe {
                conversation_ids: self.subscriptions.clone(),
            },
        };
        if let Err(error) = self.session.receive(frame.clone()) {
            return Err(self.record_failure(NativeTransportError::from_realtime(error)));
        }
        if let Err(error) = self.send_client_frame(frame, cancellation) {
            return Err(self.record_failure(error));
        }

        Ok(())
    }

    /// Receives one metadata-only server event for native application handling.
    pub fn receive_event(
        &mut self,
        cancellation: &OperationCancellation,
    ) -> Result<RealtimeEvent, NativeTransportError> {
        let frame = self.receive_server_frame(cancellation)?;
        if let Err(error) = self.session.observe_server_frame(&frame) {
            return Err(self.record_failure(NativeTransportError::from_realtime(error)));
        }

        match frame {
            ServerFrame::Event { cursor, event, .. } => {
                self.resume_cursor = Some(cursor);
                Ok(event)
            }
            ServerFrame::Error { error, .. } => {
                Err(self.record_failure(NativeTransportError::from_server_error(error.code())))
            }
            _ => Err(self.record_failure(NativeTransportError::new(
                NativeTransportErrorCode::InvalidRequest,
            ))),
        }
    }

    /// Records an interrupted connection and returns its bounded next retry delay.
    pub fn reconnect_delay_after(&mut self, error: NativeTransportError) -> Option<Duration> {
        self.record_failure(error);
        if self.state == NativeRealtimeState::Closed {
            return None;
        }

        let exponent = self.reconnect_attempt.saturating_sub(1).min(5);
        Some(Duration::from_secs(1_u64 << exponent).min(MAX_RECONNECT_DELAY))
    }

    /// Explicitly closes the native connection and discards reconnect state.
    pub fn close(&mut self) {
        self.close_connection();
        self.state = NativeRealtimeState::Closed;
        self.reconnect_attempt = MAX_RECONNECT_ATTEMPTS;
    }

    /// Returns the connector after the native session is closed.
    pub fn into_connector(mut self) -> C {
        self.close_connection();
        self.connector
    }

    fn send_client_frame(
        &mut self,
        frame: ClientFrame,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeTransportError> {
        cancellation.check()?;
        let serialized = serde_json::to_string(&frame)
            .map_err(|_| NativeTransportError::new(NativeTransportErrorCode::InvalidRequest))?;
        if serialized.len() > SizeLimits::MAX_REALTIME_FRAME_BYTES {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::TooLarge,
            ));
        }

        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unavailable))?;
        connection.send_text(&serialized, cancellation)?;
        cancellation.check()
    }

    fn receive_server_frame(
        &mut self,
        cancellation: &OperationCancellation,
    ) -> Result<ServerFrame, NativeTransportError> {
        cancellation.check()?;
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unavailable))?;
        let frame = connection.receive_text(cancellation)?;
        cancellation.check()?;
        if frame.len() > SizeLimits::MAX_REALTIME_FRAME_BYTES {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::TooLarge,
            ));
        }

        cipher_realtime_protocol::decode_server_frame(&frame)
            .map_err(NativeTransportError::from_realtime)
    }

    fn record_failure(&mut self, error: NativeTransportError) -> NativeTransportError {
        self.close_connection();
        self.last_error = Some(error.code());
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        self.state = if self.reconnect_attempt >= MAX_RECONNECT_ATTEMPTS {
            NativeRealtimeState::Closed
        } else {
            NativeRealtimeState::Reconnecting
        };
        error
    }

    fn close_connection(&mut self) {
        if let Some(connection) = self.connection.as_mut() {
            connection.close();
        }
        self.connection = None;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod production_ingress_tests;
