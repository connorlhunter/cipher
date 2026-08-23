use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use cipher_realtime_protocol::{
    ClientFrame, MAX_SUBSCRIPTION_CONVERSATIONS, RealtimeSession, Sequence, decode_client_frame,
};
use cipher_types::protocol::{
    ConversationId, ErrorCode, IdempotencyKey, ProtocolVersion, SizeLimits,
};

use super::{
    AccessToken, AuthenticatedHttpRequest, AuthenticatedRealtimeRequest, MAX_RECONNECT_ATTEMPTS,
    MAX_REQUEST_PATH_BYTES, NativeHttpClient, NativeHttpMethod, NativeHttpOrigin,
    NativeHttpRequest, NativeHttpResponse, NativeHttpTransport, NativeRealtimeClient,
    NativeRealtimeConnection, NativeRealtimeConnector, NativeRealtimeOrigin, NativeRealtimeState,
    NativeRequestPath, NativeTransportError, NativeTransportErrorCode, OperationCancellation,
    SessionAuthenticator,
};

const ACCESS_TOKEN: &str = "native-access-token";
const CONVERSATION_ID: &str = "cnv_0198b1dc-0000-7000-8000-000000000003";
const IDEMPOTENCY_KEY: &str = "idem_0198b1dc-0000-7000-8000-000000000002";
const WELCOME: &str = r#"{
  "type":"welcome",
  "protocolVersion":1,
  "sessionId":"ses_0198b1dc-0000-7000-8000-000000000001",
  "nextServerSequence":1,
  "heartbeatIntervalMs":30000,
  "resumed":false
}"#;
const MESSAGE_AVAILABLE: &str = r#"{
  "type":"event",
  "protocolVersion":1,
  "sequence":1,
  "eventId":"evt_0198b1dc-0000-7000-8000-000000000004",
  "cursor":"cur_AQIDBQ",
  "event":{
    "type":"message_available",
    "conversationId":"cnv_0198b1dc-0000-7000-8000-000000000003",
    "messageId":"msg_0198b1dc-0000-7000-8000-000000000005"
  }
}"#;

struct FixedAuthenticator {
    token: Option<&'static str>,
}

impl FixedAuthenticator {
    fn available() -> Self {
        Self {
            token: Some(ACCESS_TOKEN),
        }
    }
}

impl SessionAuthenticator for FixedAuthenticator {
    fn access_token(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeTransportError> {
        if cancellation.is_cancelled() {
            return Err(NativeTransportError::new(
                NativeTransportErrorCode::Cancelled,
            ));
        }

        self.token
            .map(|token| AccessToken::new(token.into()))
            .unwrap_or_else(|| {
                Err(NativeTransportError::new(
                    NativeTransportErrorCode::Unauthenticated,
                ))
            })
    }
}

#[derive(Default)]
struct HttpRecord {
    method: Option<NativeHttpMethod>,
    url: Option<String>,
    authorization: Option<String>,
    body: Option<Vec<u8>>,
    calls: usize,
}

struct RecordingHttpTransport {
    record: Rc<RefCell<HttpRecord>>,
    response: Result<NativeHttpResponse, NativeTransportError>,
}

impl NativeHttpTransport for RecordingHttpTransport {
    fn send(
        &mut self,
        request: AuthenticatedHttpRequest,
        cancellation: &OperationCancellation,
    ) -> Result<NativeHttpResponse, NativeTransportError> {
        cancellation.check()?;
        assert!(!format!("{request:?}").contains(ACCESS_TOKEN));
        let mut record = self.record.borrow_mut();
        record.calls += 1;
        record.method = Some(request.method());
        record.url = Some(request.url().into());
        record.authorization = Some(request.authorization().into());
        record.body = Some(request.body().into());
        drop(record);
        match &self.response {
            Ok(response) => NativeHttpResponse::new(response.status(), response.body.clone()),
            Err(error) => Err(*error),
        }
    }
}

struct CancellingAuthenticator;

impl SessionAuthenticator for CancellingAuthenticator {
    fn access_token(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeTransportError> {
        cancellation.cancel();
        AccessToken::new(ACCESS_TOKEN.into())
    }
}

struct CancellingHttpTransport;

impl NativeHttpTransport for CancellingHttpTransport {
    fn send(
        &mut self,
        _request: AuthenticatedHttpRequest,
        cancellation: &OperationCancellation,
    ) -> Result<NativeHttpResponse, NativeTransportError> {
        cancellation.cancel();
        NativeHttpResponse::new(200, Vec::new())
    }
}

#[derive(Default)]
struct RealtimeRecord {
    urls: Vec<String>,
    authorizations: Vec<String>,
    sent: Vec<String>,
    closes: usize,
}

struct MockRealtimeConnection {
    record: Rc<RefCell<RealtimeRecord>>,
    inbound: VecDeque<Result<String, NativeTransportError>>,
}

impl NativeRealtimeConnection for MockRealtimeConnection {
    fn send_text(
        &mut self,
        frame: &str,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeTransportError> {
        cancellation.check()?;
        self.record.borrow_mut().sent.push(frame.into());
        Ok(())
    }

    fn receive_text(
        &mut self,
        cancellation: &OperationCancellation,
    ) -> Result<String, NativeTransportError> {
        cancellation.check()?;
        self.inbound.pop_front().unwrap_or_else(|| {
            Err(NativeTransportError::new(
                NativeTransportErrorCode::Unavailable,
            ))
        })
    }

    fn close(&mut self) {
        self.record.borrow_mut().closes += 1;
    }
}

struct MockRealtimeConnector {
    record: Rc<RefCell<RealtimeRecord>>,
    connections:
        VecDeque<Result<VecDeque<Result<String, NativeTransportError>>, NativeTransportError>>,
}

impl MockRealtimeConnector {
    fn connected(
        record: Rc<RefCell<RealtimeRecord>>,
        frames: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            record,
            connections: VecDeque::from([Ok(frames
                .into_iter()
                .map(|frame| Ok(frame.into()))
                .collect())]),
        }
    }
}

impl NativeRealtimeConnector for MockRealtimeConnector {
    type Connection = MockRealtimeConnection;

    fn connect(
        &mut self,
        request: AuthenticatedRealtimeRequest,
        cancellation: &OperationCancellation,
    ) -> Result<Self::Connection, NativeTransportError> {
        cancellation.check()?;
        assert!(!format!("{request:?}").contains(ACCESS_TOKEN));
        let mut record = self.record.borrow_mut();
        record.urls.push(request.url().into());
        record.authorizations.push(request.authorization().into());
        drop(record);

        self.connections
            .pop_front()
            .unwrap_or_else(|| {
                Err(NativeTransportError::new(
                    NativeTransportErrorCode::Unavailable,
                ))
            })
            .map(|inbound| MockRealtimeConnection {
                record: Rc::clone(&self.record),
                inbound,
            })
    }
}

struct FailingRealtimeConnection {
    record: Rc<RefCell<RealtimeRecord>>,
    send_error: Option<NativeTransportError>,
    receive_error: Option<NativeTransportError>,
}

impl NativeRealtimeConnection for FailingRealtimeConnection {
    fn send_text(
        &mut self,
        _frame: &str,
        _cancellation: &OperationCancellation,
    ) -> Result<(), NativeTransportError> {
        self.send_error.take().map_or(Ok(()), Err)
    }

    fn receive_text(
        &mut self,
        _cancellation: &OperationCancellation,
    ) -> Result<String, NativeTransportError> {
        self.receive_error.take().map_or(
            Err(NativeTransportError::new(
                NativeTransportErrorCode::Unavailable,
            )),
            Err,
        )
    }

    fn close(&mut self) {
        self.record.borrow_mut().closes += 1;
    }
}

struct FailingRealtimeConnector {
    record: Rc<RefCell<RealtimeRecord>>,
    connect_error: Option<NativeTransportError>,
    connection: Option<FailingRealtimeConnection>,
}

impl NativeRealtimeConnector for FailingRealtimeConnector {
    type Connection = FailingRealtimeConnection;

    fn connect(
        &mut self,
        request: AuthenticatedRealtimeRequest,
        cancellation: &OperationCancellation,
    ) -> Result<Self::Connection, NativeTransportError> {
        cancellation.check()?;
        assert!(!format!("{request:?}").contains(ACCESS_TOKEN));
        let mut record = self.record.borrow_mut();
        record.urls.push(request.url().into());
        record.authorizations.push(request.authorization().into());
        drop(record);
        if let Some(error) = self.connect_error.take() {
            return Err(error);
        }
        self.connection
            .take()
            .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unavailable))
    }
}

fn conversation_id() -> ConversationId {
    CONVERSATION_ID.parse().unwrap()
}

fn idempotency_key() -> IdempotencyKey {
    IDEMPOTENCY_KEY.parse().unwrap()
}

#[test]
fn native_transport_errors_are_fixed_and_redacted() {
    for (code, expected) in [
        (NativeTransportErrorCode::Cancelled, "cancelled"),
        (NativeTransportErrorCode::InvalidRequest, "invalid"),
        (NativeTransportErrorCode::Unauthenticated, "session"),
        (NativeTransportErrorCode::UnsupportedVersion, "version"),
        (NativeTransportErrorCode::TooLarge, "large"),
        (NativeTransportErrorCode::Unavailable, "unavailable"),
    ] {
        let error = NativeTransportError::new(code);
        assert!(error.message().contains(expected));
        assert_eq!(error.to_string(), error.message());
        assert!(!error.to_string().contains(ACCESS_TOKEN));
    }

    let unsupported = RealtimeSession::v1()
        .receive(ClientFrame::Hello {
            supported_versions: vec![ProtocolVersion::new(99)],
            resume_cursor: None,
            last_acknowledged_server_sequence: None,
        })
        .unwrap_err();
    assert_eq!(
        NativeTransportError::from_realtime(unsupported).code(),
        NativeTransportErrorCode::UnsupportedVersion
    );
    assert_eq!(
        NativeTransportError::from_realtime(decode_client_frame("not json").unwrap_err()).code(),
        NativeTransportErrorCode::InvalidRequest
    );
    assert_eq!(
        NativeTransportError::from_realtime(
            decode_client_frame(&"x".repeat(SizeLimits::MAX_REALTIME_FRAME_BYTES + 1)).unwrap_err()
        )
        .code(),
        NativeTransportErrorCode::TooLarge
    );

    for (code, expected) in [
        (
            ErrorCode::Unauthenticated,
            NativeTransportErrorCode::Unauthenticated,
        ),
        (
            ErrorCode::Expired,
            NativeTransportErrorCode::Unauthenticated,
        ),
        (
            ErrorCode::UnsupportedVersion,
            NativeTransportErrorCode::UnsupportedVersion,
        ),
        (ErrorCode::TooLarge, NativeTransportErrorCode::TooLarge),
        (
            ErrorCode::Unavailable,
            NativeTransportErrorCode::Unavailable,
        ),
        (
            ErrorCode::RateLimited,
            NativeTransportErrorCode::Unavailable,
        ),
        (
            ErrorCode::Forbidden,
            NativeTransportErrorCode::InvalidRequest,
        ),
    ] {
        assert_eq!(
            NativeTransportError::from_server_error(code).code(),
            expected
        );
    }
}

#[test]
fn native_origins_require_secure_origins_except_explicit_loopback() {
    assert!(NativeHttpOrigin::parse("https://api.example.test").is_ok());
    assert!(NativeRealtimeOrigin::parse("wss://api.example.test").is_ok());
    assert!(NativeHttpOrigin::parse("http://localhost:8080").is_ok());
    assert!(NativeRealtimeOrigin::parse("ws://[::1]:8080").is_ok());
    assert!(NativeHttpOrigin::parse("http://LOCALHOST:8080").is_ok());

    for origin in [
        "",
        "api.example.test",
        "http://api.example.test",
        "ws://api.example.test",
        "https://api.example.test/path",
        "https://token@api.example.test",
        "https://api.example.test?capability=secret",
        "https://api.example.test\n",
        "https://",
        "https://:443",
        "https://api.example.test:invalid",
        "https://api.example.test:0",
        "https://api..example.test",
        "https://-api.example.test",
        "https://[::1]unexpected",
        "http://[::2]:8080",
    ] {
        assert!(NativeHttpOrigin::parse(origin).is_err());
    }

    assert!(
        !format!(
            "{:?}",
            NativeRealtimeOrigin::parse("wss://api.example.test").unwrap()
        )
        .contains("api.example.test")
    );
}

#[test]
fn request_paths_and_bodies_are_bounded_and_relative() {
    assert!(NativeRequestPath::parse("/v1/conversations?limit=50").is_ok());
    for path in [
        "https://api.example.test/v1",
        "/v2/conversations",
        "/v1/conversations#fragment",
        "/v1\\conversations",
        "/v1/conversations\nheader: value",
    ] {
        assert!(NativeRequestPath::parse(path).is_err());
    }
    assert!(
        NativeRequestPath::parse(&format!("/v1/{}", "x".repeat(MAX_REQUEST_PATH_BYTES))).is_err()
    );

    let path = NativeRequestPath::parse("/v1/conversations").unwrap();
    assert!(
        NativeHttpRequest::new(
            NativeHttpMethod::Post,
            path,
            vec![0; cipher_types::protocol::SizeLimits::MAX_HTTP_BODY_BYTES + 1],
        )
        .is_err()
    );
    assert!(
        NativeHttpResponse::new(
            200,
            vec![0; cipher_types::protocol::SizeLimits::MAX_HTTP_BODY_BYTES + 1],
        )
        .is_err()
    );
}

#[test]
fn tokens_and_authenticated_requests_redact_debug_output() {
    let token = AccessToken::new(ACCESS_TOKEN.into()).unwrap();
    assert!(!format!("{token:?}").contains(ACCESS_TOKEN));
    assert!(AccessToken::new("has whitespace".into()).is_err());
    assert!(AccessToken::new("\n".into()).is_err());

    let request = NativeHttpRequest::get(NativeRequestPath::parse("/v1").unwrap());
    assert!(!format!("{request:?}").contains("/v1"));
    assert!(!format!("{:?}", NativeRequestPath::parse("/v1").unwrap()).contains("/v1"));
    assert!(
        !format!(
            "{:?}",
            NativeHttpOrigin::parse("https://api.example.test").unwrap()
        )
        .contains("api.example.test")
    );
    assert!(!format!("{:?}", NativeHttpResponse::new(200, vec![1]).unwrap()).contains("1]"));
}

#[test]
fn native_http_client_authenticates_and_dispatches_without_ipc_data() {
    let record = Rc::new(RefCell::new(HttpRecord::default()));
    let transport = RecordingHttpTransport {
        record: Rc::clone(&record),
        response: Ok(NativeHttpResponse::new(200, br#"{"ok":true}"#.to_vec()).unwrap()),
    };
    let mut client = NativeHttpClient::new(
        transport,
        FixedAuthenticator::available(),
        NativeHttpOrigin::parse("https://api.example.test").unwrap(),
    );
    let response = client
        .execute(
            NativeHttpRequest::new(
                NativeHttpMethod::Post,
                NativeRequestPath::parse("/v1/conversations").unwrap(),
                br#"{"title":"native"}"#.to_vec(),
            )
            .unwrap(),
            &OperationCancellation::default(),
        )
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.into_body(), br#"{"ok":true}"#);
    let record = record.borrow();
    assert_eq!(record.calls, 1);
    assert_eq!(record.method, Some(NativeHttpMethod::Post));
    assert_eq!(
        record.url.as_deref(),
        Some("https://api.example.test/v1/conversations")
    );
    assert_eq!(record.authorization.as_deref(), Some(ACCESS_TOKEN));
    assert_eq!(
        record.body.as_deref(),
        Some(br#"{"title":"native"}"#.as_slice())
    );
}

#[test]
fn access_token_clone_stays_redacted_and_has_no_serialization_surface() {
    let token = AccessToken::new(ACCESS_TOKEN.into()).unwrap();
    let cloned = token.clone();

    assert!(!format!("{token:?}").contains(ACCESS_TOKEN));
    assert!(!format!("{cloned:?}").contains(ACCESS_TOKEN));
    assert_eq!(token.as_str(), ACCESS_TOKEN);
    assert_eq!(cloned.as_str(), ACCESS_TOKEN);

    let source = include_str!("lib.rs");
    let start = source.find("/// A non-serializable access token").unwrap();
    let end = source
        .find("/// Supplies an in-memory access token")
        .unwrap();
    let token_surface = &source[start..end];
    assert!(token_surface.contains("Zeroizing<String>"));
    assert!(!token_surface.contains("pub struct AccessToken(String)"));
    assert!(!token_surface.contains("serde"));
    assert!(!token_surface.contains("Serialize"));
}

#[test]
fn native_http_client_obeys_cancellation_and_missing_session() {
    let record = Rc::new(RefCell::new(HttpRecord::default()));
    let response = NativeHttpResponse::new(200, Vec::new()).unwrap();
    let mut client = NativeHttpClient::new(
        RecordingHttpTransport {
            record: Rc::clone(&record),
            response: Ok(response),
        },
        FixedAuthenticator { token: None },
        NativeHttpOrigin::parse("https://api.example.test").unwrap(),
    );
    let request = NativeHttpRequest::get(NativeRequestPath::parse("/v1").unwrap());
    assert_eq!(
        client
            .execute(request, &OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Unauthenticated
    );
    assert_eq!(record.borrow().calls, 0);

    let cancellation = OperationCancellation::default();
    cancellation.cancel();
    let request = NativeHttpRequest::get(NativeRequestPath::parse("/v1").unwrap());
    assert_eq!(
        client.execute(request, &cancellation).unwrap_err().code(),
        NativeTransportErrorCode::Cancelled
    );
}

#[test]
fn native_http_client_checks_cancellation_at_every_native_boundary() {
    let path = NativeRequestPath::parse("/v1").unwrap();
    let response = NativeHttpResponse::new(200, Vec::new()).unwrap();
    let mut after_authentication = NativeHttpClient::new(
        RecordingHttpTransport {
            record: Rc::new(RefCell::new(HttpRecord::default())),
            response: Ok(response),
        },
        CancellingAuthenticator,
        NativeHttpOrigin::parse("https://api.example.test").unwrap(),
    );
    assert_eq!(
        after_authentication
            .execute(
                NativeHttpRequest::get(path),
                &OperationCancellation::default()
            )
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Cancelled
    );

    let mut after_transport = NativeHttpClient::new(
        CancellingHttpTransport,
        FixedAuthenticator::available(),
        NativeHttpOrigin::parse("https://api.example.test").unwrap(),
    );
    assert_eq!(
        after_transport
            .execute(
                NativeHttpRequest::get(NativeRequestPath::parse("/v1").unwrap()),
                &OperationCancellation::default(),
            )
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Cancelled
    );

    let transport = after_transport.into_transport();
    assert_eq!(std::mem::size_of_val(&transport), 0);
}

#[test]
fn native_realtime_negotiates_and_restores_subscriptions_without_exposing_tokens() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let connector =
        MockRealtimeConnector::connected(Rc::clone(&record), [WELCOME, MESSAGE_AVAILABLE]);
    let mut client = NativeRealtimeClient::new(
        connector,
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    client
        .replace_subscriptions(vec![conversation_id()])
        .unwrap();
    client.connect(&OperationCancellation::default()).unwrap();
    client
        .restore_subscriptions(
            Sequence::new(1).unwrap(),
            idempotency_key(),
            &OperationCancellation::default(),
        )
        .unwrap();
    let event = client
        .receive_event(&OperationCancellation::default())
        .unwrap();

    assert!(matches!(
        event,
        cipher_realtime_protocol::RealtimeEvent::MessageAvailable { .. }
    ));
    assert_eq!(client.diagnostic().state, NativeRealtimeState::Connected);
    let record = record.borrow();
    assert_eq!(record.urls, ["wss://api.example.test/v1/realtime"]);
    assert_eq!(record.authorizations, [ACCESS_TOKEN]);
    assert_eq!(record.sent.len(), 2);
    assert!(record.sent[0].contains("\"type\":\"hello\""));
    assert!(record.sent[1].contains("\"type\":\"command\""));
}

#[test]
fn native_realtime_rejects_duplicate_subscriptions_and_unready_restore() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut client = NativeRealtimeClient::new(
        MockRealtimeConnector::connected(Rc::clone(&record), []),
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        client
            .replace_subscriptions(vec![conversation_id(), conversation_id()])
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::InvalidRequest
    );
    client
        .replace_subscriptions(vec![conversation_id()])
        .unwrap();
    assert_eq!(
        client
            .restore_subscriptions(
                Sequence::new(1).unwrap(),
                idempotency_key(),
                &OperationCancellation::default(),
            )
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Unavailable
    );
}

#[test]
fn native_realtime_backs_off_and_redacts_diagnostics() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut client = NativeRealtimeClient::new(
        MockRealtimeConnector::connected(Rc::clone(&record), []),
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    let unavailable = NativeTransportError::new(NativeTransportErrorCode::Unavailable);
    assert_eq!(
        client.reconnect_delay_after(unavailable).unwrap().as_secs(),
        1
    );
    assert_eq!(client.diagnostic().state, NativeRealtimeState::Reconnecting);
    assert_eq!(
        client.diagnostic().last_error,
        Some(NativeTransportErrorCode::Unavailable)
    );

    for _ in 1..MAX_RECONNECT_ATTEMPTS {
        client.reconnect_delay_after(unavailable);
    }
    assert_eq!(client.diagnostic().state, NativeRealtimeState::Closed);
    assert_eq!(client.reconnect_delay_after(unavailable), None);

    let diagnostic = serde_json::to_string(&client.diagnostic()).unwrap();
    assert!(!diagnostic.contains(ACCESS_TOKEN));
    assert!(!diagnostic.contains("api.example.test"));
    assert!(!diagnostic.contains("/v1/realtime"));
}

#[test]
fn native_realtime_closes_connections_and_maps_protocol_failures() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let connector = MockRealtimeConnector::connected(
        Rc::clone(&record),
        [r#"{"type":"welcome","protocolVersion":9}"#],
    );
    let mut client = NativeRealtimeClient::new(
        connector,
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        client
            .connect(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::InvalidRequest
    );
    assert_eq!(client.diagnostic().state, NativeRealtimeState::Reconnecting);
    assert_eq!(record.borrow().closes, 1);

    client.close();
    assert_eq!(client.diagnostic().state, NativeRealtimeState::Closed);
}

#[test]
fn native_realtime_records_authentication_connection_and_frame_failures() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut missing_session = NativeRealtimeClient::new(
        MockRealtimeConnector::connected(Rc::clone(&record), []),
        FixedAuthenticator { token: None },
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        missing_session
            .connect(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Unauthenticated
    );
    assert_eq!(
        missing_session.diagnostic().state,
        NativeRealtimeState::Reconnecting
    );

    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut failed_connection = NativeRealtimeClient::new(
        FailingRealtimeConnector {
            record: Rc::clone(&record),
            connect_error: Some(NativeTransportError::new(
                NativeTransportErrorCode::Unavailable,
            )),
            connection: None,
        },
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        failed_connection
            .connect(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Unavailable
    );

    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut failed_send = NativeRealtimeClient::new(
        FailingRealtimeConnector {
            record: Rc::clone(&record),
            connect_error: None,
            connection: Some(FailingRealtimeConnection {
                record: Rc::clone(&record),
                send_error: Some(NativeTransportError::new(
                    NativeTransportErrorCode::Unavailable,
                )),
                receive_error: None,
            }),
        },
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        failed_send
            .connect(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Unavailable
    );
    assert_eq!(record.borrow().closes, 1);

    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut failed_receive = NativeRealtimeClient::new(
        FailingRealtimeConnector {
            record: Rc::clone(&record),
            connect_error: None,
            connection: Some(FailingRealtimeConnection {
                record: Rc::clone(&record),
                send_error: None,
                receive_error: Some(NativeTransportError::new(
                    NativeTransportErrorCode::Unavailable,
                )),
            }),
        },
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        failed_receive
            .connect(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Unavailable
    );

    let cancellation = OperationCancellation::default();
    cancellation.cancel();
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut cancelled = NativeRealtimeClient::new(
        MockRealtimeConnector::connected(Rc::clone(&record), []),
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        cancelled.connect(&cancellation).unwrap_err().code(),
        NativeTransportErrorCode::Cancelled
    );
    assert_eq!(cancelled.diagnostic().state, NativeRealtimeState::Idle);
}

#[test]
fn native_realtime_rejects_non_welcome_and_invalid_welcome_frames() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut non_welcome = NativeRealtimeClient::new(
        MockRealtimeConnector::connected(Rc::clone(&record), [MESSAGE_AVAILABLE]),
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        non_welcome
            .connect(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::InvalidRequest
    );

    let invalid_welcome = r#"{
      "type":"welcome",
      "protocolVersion":1,
      "sessionId":"ses_0198b1dc-0000-7000-8000-000000000001",
      "nextServerSequence":1,
      "heartbeatIntervalMs":1,
      "resumed":false
    }"#;
    let mut bad_interval = NativeRealtimeClient::new(
        MockRealtimeConnector::connected(Rc::clone(&record), [invalid_welcome]),
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert_eq!(
        bad_interval
            .connect(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::InvalidRequest
    );
}

#[test]
fn native_realtime_handles_empty_and_invalid_subscription_restoration() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let mut idle = NativeRealtimeClient::new(
        MockRealtimeConnector::connected(Rc::clone(&record), []),
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    assert!(
        idle.restore_subscriptions(
            Sequence::new(1).unwrap(),
            idempotency_key(),
            &OperationCancellation::default(),
        )
        .is_ok()
    );
    assert_eq!(
        idle.replace_subscriptions(vec![conversation_id(); MAX_SUBSCRIPTION_CONVERSATIONS + 1])
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::InvalidRequest
    );

    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let connector = MockRealtimeConnector::connected(Rc::clone(&record), [WELCOME]);
    let mut connected = NativeRealtimeClient::new(
        connector,
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    connected
        .replace_subscriptions(vec![conversation_id()])
        .unwrap();
    connected
        .connect(&OperationCancellation::default())
        .unwrap();
    connected
        .restore_subscriptions(
            Sequence::new(1).unwrap(),
            idempotency_key(),
            &OperationCancellation::default(),
        )
        .unwrap();
    assert_eq!(
        connected
            .restore_subscriptions(
                Sequence::new(1).unwrap(),
                idempotency_key(),
                &OperationCancellation::default(),
            )
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::InvalidRequest
    );
    assert_eq!(
        connected.diagnostic().state,
        NativeRealtimeState::Reconnecting
    );
}

#[test]
fn native_realtime_rejects_safe_non_event_frames_and_server_errors() {
    let heartbeat = r#"{"type":"heartbeat","protocolVersion":1,"nonce":"alive"}"#;
    let server_error = r#"{
      "type":"error",
      "protocolVersion":1,
      "error":{"code":"rate_limited","message":"The service is busy.","retryable":true},
      "fatal":false
    }"#;
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let connector = MockRealtimeConnector::connected(Rc::clone(&record), [WELCOME, heartbeat]);
    let mut heartbeat_client = NativeRealtimeClient::new(
        connector,
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    heartbeat_client
        .connect(&OperationCancellation::default())
        .unwrap();
    assert_eq!(
        heartbeat_client
            .receive_event(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::InvalidRequest
    );

    let connector = MockRealtimeConnector::connected(Rc::clone(&record), [WELCOME, server_error]);
    let mut error_client = NativeRealtimeClient::new(
        connector,
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    error_client
        .connect(&OperationCancellation::default())
        .unwrap();
    assert_eq!(
        error_client
            .receive_event(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::Unavailable
    );
}

#[test]
fn native_realtime_bounds_received_frames_and_returns_its_connector() {
    let record = Rc::new(RefCell::new(RealtimeRecord::default()));
    let oversized = "x".repeat(SizeLimits::MAX_REALTIME_FRAME_BYTES + 1);
    let connector = MockRealtimeConnector {
        record: Rc::clone(&record),
        connections: VecDeque::from([Ok(VecDeque::from([Ok(WELCOME.into()), Ok(oversized)]))]),
    };
    let mut client = NativeRealtimeClient::new(
        connector,
        FixedAuthenticator::available(),
        NativeRealtimeOrigin::parse("wss://api.example.test").unwrap(),
    );
    client.connect(&OperationCancellation::default()).unwrap();
    assert_eq!(
        client
            .receive_event(&OperationCancellation::default())
            .unwrap_err()
            .code(),
        NativeTransportErrorCode::TooLarge
    );
    let _connector = client.into_connector();
    assert!(record.borrow().closes >= 1);
}
