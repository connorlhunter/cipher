use cipher_types::protocol::ErrorCode;
use serde_json::{Value, json};

use super::{
    ClientFrame, HEARTBEAT_INTERVAL_MS, MAX_ERROR_MESSAGE_BYTES, MAX_FRAME_BYTES,
    MAX_HEARTBEAT_NONCE_BYTES, MAX_SUBSCRIPTION_CONVERSATIONS, RealtimeProtocolError,
    RealtimeSession, Sequence, ServerFrame, decode_client_frame, decode_server_frame,
};

const VALID_FIXTURE: &str = include_str!("../../../../contracts/realtime/v1/valid.json");
const MALFORMED_FIXTURE: &str = include_str!("../../../../contracts/realtime/v1/malformed.json");
const DUPLICATE_FIXTURE: &str = include_str!("../../../../contracts/realtime/v1/duplicate.json");
const STALE_FIXTURE: &str = include_str!("../../../../contracts/realtime/v1/stale.json");
const OVERSIZED_FIXTURE: &str = include_str!("../../../../contracts/realtime/v1/oversized.json");
const UNSUPPORTED_FIXTURE: &str =
    include_str!("../../../../contracts/realtime/v1/unsupported.json");
const V0_HELLO_FIXTURE: &str = include_str!("../../../../contracts/realtime/v0/hello.json");

const CONVERSATION_ID: &str = "cnv_0198b1dc-0000-7000-8000-000000000003";
const IDEMPOTENCY_KEY: &str = "idem_0198b1dc-0000-7000-8000-000000000002";

fn fixture(source: &str) -> Value {
    serde_json::from_str(source).unwrap()
}

fn wire(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn expected_error(value: &Value) -> ErrorCode {
    serde_json::from_value(value["expectedError"].clone()).unwrap()
}

fn hello(version: u16) -> String {
    wire(&json!({ "type": "hello", "supportedVersions": [version] }))
}

fn command(sequence: u64, idempotency_key: &str, conversation_ids: Value) -> String {
    wire(&json!({
        "type": "command",
        "protocolVersion": 1,
        "sequence": sequence,
        "idempotencyKey": idempotency_key,
        "command": { "type": "subscribe", "conversationIds": conversation_ids }
    }))
}

fn heartbeat(sequence: u64) -> String {
    wire(&json!({
        "type": "heartbeat",
        "protocolVersion": 1,
        "sequence": sequence,
        "nonce": format!("test-heartbeat-{sequence}")
    }))
}

fn acknowledge(sequence: u64, acknowledged_server_sequence: u64) -> String {
    wire(&json!({
        "type": "ack",
        "protocolVersion": 1,
        "sequence": sequence,
        "acknowledgedServerSequence": acknowledged_server_sequence
    }))
}

fn assert_code(result: Result<(), RealtimeProtocolError>, expected: ErrorCode) {
    assert_eq!(result.unwrap_err().code(), expected);
}

#[test]
fn valid_golden_frames_round_trip_and_advance_a_connection() {
    let fixture = fixture(VALID_FIXTURE);
    let client_frames = fixture["clientFrames"].as_array().unwrap();
    let server_frames = fixture["serverFrames"].as_array().unwrap();
    let mut session = RealtimeSession::default();

    for frame in client_frames {
        let decoded = decode_client_frame(&wire(frame)).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), *frame);
    }
    for frame in server_frames {
        let decoded = decode_server_frame(&wire(frame)).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), *frame);
    }

    session.receive_text(&wire(&client_frames[0])).unwrap();
    assert_eq!(session.selected_protocol_version().unwrap().get(), 1);
    assert_eq!(session.resume_cursor().unwrap().as_str(), "cur_AQIDBA");
    assert_eq!(
        session.last_acknowledged_server_sequence().unwrap().get(),
        42
    );
    session
        .observe_server_frame(&decode_server_frame(&wire(&server_frames[0])).unwrap())
        .unwrap();
    session
        .observe_server_frame(&decode_server_frame(&wire(&server_frames[2])).unwrap())
        .unwrap();
    session.receive_text(&wire(&client_frames[1])).unwrap();
    session.receive_text(&wire(&client_frames[2])).unwrap();
    assert_eq!(
        session.last_acknowledged_server_sequence().unwrap().get(),
        43
    );
    session.receive_text(&wire(&client_frames[3])).unwrap();
    session
        .observe_server_frame(&decode_server_frame(&wire(&server_frames[1])).unwrap())
        .unwrap();
    session
        .observe_server_frame(&decode_server_frame(&wire(&server_frames[3])).unwrap())
        .unwrap();
}

#[test]
fn malformed_golden_frame_is_rejected_before_session_state_changes() {
    let fixture = fixture(MALFORMED_FIXTURE);
    let mut session = RealtimeSession::v1();

    assert_code(
        session.receive_text(fixture["wire"].as_str().unwrap()),
        expected_error(&fixture),
    );
    assert!(session.selected_protocol_version().is_none());
}

#[test]
fn duplicate_and_stale_golden_frames_are_rejected() {
    for source in [DUPLICATE_FIXTURE, STALE_FIXTURE] {
        let fixture = fixture(source);
        let frames = fixture["frames"].as_array().unwrap();
        let mut session = RealtimeSession::v1();

        for frame in &frames[..frames.len() - 1] {
            session.receive_text(&wire(frame)).unwrap();
        }
        assert_code(
            session.receive_text(&wire(frames.last().unwrap())),
            expected_error(&fixture),
        );
    }
}

#[test]
fn oversized_and_unsupported_golden_frames_are_rejected() {
    let oversized = fixture(OVERSIZED_FIXTURE);
    let minimum_bytes = oversized["minimumFrameBytes"].as_u64().unwrap() as usize;
    assert!(minimum_bytes > MAX_FRAME_BYTES);
    assert_code(
        decode_client_frame(&"x".repeat(minimum_bytes)).map(|_| ()),
        expected_error(&oversized),
    );

    let unsupported = fixture(UNSUPPORTED_FIXTURE);
    let mut session = RealtimeSession::v1();
    assert_code(
        session.receive_text(&wire(&unsupported["frame"])),
        expected_error(&unsupported),
    );
}

#[test]
fn hello_must_be_first_and_offer_a_canonical_bounded_version_list() {
    let mut session = RealtimeSession::v1();
    assert_code(
        session.receive_text(&heartbeat(1)),
        ErrorCode::InvalidRequest,
    );

    for offered_versions in [json!([]), json!([0, 1]), json!([1, 1]), json!([2, 1, 0])] {
        let mut session = RealtimeSession::v1();
        assert_code(
            session.receive_text(&wire(&json!({
                "type": "hello",
                "supportedVersions": offered_versions,
            }))),
            ErrorCode::InvalidRequest,
        );
    }

    let mut session = RealtimeSession::v1();
    assert_code(
        session.receive_text(&wire(&json!({
            "type": "hello",
            "supportedVersions": [1],
            "lastAcknowledgedServerSequence": 0,
        }))),
        ErrorCode::InvalidRequest,
    );

    let v0 = fixture(V0_HELLO_FIXTURE);
    let mut session = RealtimeSession::v1();
    session.receive_text(&wire(&v0["frame"])).unwrap();
    assert_eq!(
        session.selected_protocol_version().unwrap().get(),
        v0["expectedSelectedVersion"].as_u64().unwrap() as u16,
    );
    assert_code(session.receive_text(&hello(0)), ErrorCode::InvalidRequest);
}

#[test]
fn commands_are_bounded_unique_and_idempotent() {
    let mut session = RealtimeSession::v1();
    session.receive_text(&hello(1)).unwrap();
    assert_code(
        session.receive_text(&command(1, IDEMPOTENCY_KEY, json!([]))),
        ErrorCode::InvalidRequest,
    );

    let too_many = Value::Array(
        (0..=MAX_SUBSCRIPTION_CONVERSATIONS)
            .map(|index| json!(format!("cnv_0198b1dc-0000-7000-8000-{index:012}",)))
            .collect(),
    );
    assert_code(
        session.receive_text(&command(2, IDEMPOTENCY_KEY, too_many)),
        ErrorCode::TooLarge,
    );

    assert_code(
        session.receive_text(&command(
            3,
            IDEMPOTENCY_KEY,
            json!([CONVERSATION_ID, CONVERSATION_ID]),
        )),
        ErrorCode::InvalidRequest,
    );

    session
        .receive_text(&command(4, IDEMPOTENCY_KEY, json!([CONVERSATION_ID])))
        .unwrap();
    assert_code(
        session.receive_text(&command(5, IDEMPOTENCY_KEY, json!([CONVERSATION_ID]))),
        ErrorCode::Duplicate,
    );

    let unsubscribe = wire(&json!({
        "type": "command",
        "protocolVersion": 1,
        "sequence": 6,
        "idempotencyKey": "idem_0198b1dc-0000-7000-8000-000000000006",
        "command": { "type": "unsubscribe", "conversationIds": [CONVERSATION_ID] }
    }));
    session.receive_text(&unsubscribe).unwrap();
}

#[test]
fn client_sequences_and_versions_are_strictly_monotonic() {
    let mut session = RealtimeSession::v1();
    session.receive_text(&hello(1)).unwrap();
    assert_code(
        session.receive_text(&heartbeat(0)),
        ErrorCode::InvalidRequest,
    );
    session.receive_text(&heartbeat(1)).unwrap();
    assert_code(session.receive_text(&heartbeat(1)), ErrorCode::Duplicate);
    assert_code(
        session.receive_text(&heartbeat(0)),
        ErrorCode::InvalidRequest,
    );
    assert_code(
        session.receive_text(&wire(&json!({
            "type": "heartbeat",
            "protocolVersion": 0,
            "sequence": 2,
            "nonce": "wrong-version"
        }))),
        ErrorCode::UnsupportedVersion,
    );
}

#[test]
fn acknowledgements_only_move_forward_over_observed_server_events() {
    let mut session = RealtimeSession::v1();
    session.receive_text(&hello(1)).unwrap();
    assert_code(
        session.receive_text(&acknowledge(1, 1)),
        ErrorCode::InvalidRequest,
    );

    let event = decode_server_frame(&wire(&json!({
        "type": "event",
        "protocolVersion": 1,
        "sequence": 3,
        "eventId": "evt_0198b1dc-0000-7000-8000-000000000004",
        "cursor": "cur_AQIDBQ",
        "event": {
            "type": "conversation_changed",
            "conversationId": CONVERSATION_ID
        }
    })))
    .unwrap();
    session.observe_server_frame(&event).unwrap();
    session.receive_text(&acknowledge(2, 3)).unwrap();
    assert_code(
        session.receive_text(&acknowledge(3, 3)),
        ErrorCode::Duplicate,
    );
    assert_code(session.receive_text(&acknowledge(4, 2)), ErrorCode::Stale);
    assert_code(
        session.receive_text(&acknowledge(5, 4)),
        ErrorCode::InvalidRequest,
    );
}

#[test]
fn server_frames_are_checked_after_the_handshake() {
    let welcome = decode_server_frame(&wire(&json!({
        "type": "welcome",
        "protocolVersion": 1,
        "sessionId": "ses_0198b1dc-0000-7000-8000-000000000001",
        "nextServerSequence": 1,
        "heartbeatIntervalMs": HEARTBEAT_INTERVAL_MS,
        "resumed": false
    })))
    .unwrap();
    let mut session = RealtimeSession::v1();
    assert_code(
        session.observe_server_frame(&welcome),
        ErrorCode::InvalidRequest,
    );
    session.receive_text(&hello(1)).unwrap();
    session.observe_server_frame(&welcome).unwrap();

    for frame in [
        json!({
            "type": "welcome",
            "protocolVersion": 0,
            "sessionId": "ses_0198b1dc-0000-7000-8000-000000000001",
            "nextServerSequence": 1,
            "heartbeatIntervalMs": HEARTBEAT_INTERVAL_MS,
            "resumed": false
        }),
        json!({
            "type": "welcome",
            "protocolVersion": 1,
            "sessionId": "ses_0198b1dc-0000-7000-8000-000000000001",
            "nextServerSequence": 0,
            "heartbeatIntervalMs": HEARTBEAT_INTERVAL_MS,
            "resumed": false
        }),
        json!({
            "type": "welcome",
            "protocolVersion": 1,
            "sessionId": "ses_0198b1dc-0000-7000-8000-000000000001",
            "nextServerSequence": 1,
            "heartbeatIntervalMs": 1,
            "resumed": false
        }),
    ] {
        assert_code(
            session.observe_server_frame(&decode_server_frame(&wire(&frame)).unwrap()),
            if frame["protocolVersion"] == 0 {
                ErrorCode::UnsupportedVersion
            } else {
                ErrorCode::InvalidRequest
            },
        );
    }
}

#[test]
fn server_event_order_error_bounds_and_heartbeat_nonces_are_checked() {
    let mut session = RealtimeSession::v1();
    session.receive_text(&hello(1)).unwrap();

    let event = |sequence: u64, event: Value| {
        decode_server_frame(&wire(&json!({
            "type": "event",
            "protocolVersion": 1,
            "sequence": sequence,
            "eventId": "evt_0198b1dc-0000-7000-8000-000000000004",
            "cursor": "cur_AQIDBQ",
            "event": event,
        })))
        .unwrap()
    };
    session
        .observe_server_frame(&event(
            1,
            json!({ "type": "device_revoked", "deviceId": "dev_0198b1dc-0000-7000-8000-000000000007" }),
        ))
        .unwrap();
    assert_code(
        session.observe_server_frame(&event(
            1,
            json!({ "type": "conversation_changed", "conversationId": CONVERSATION_ID }),
        )),
        ErrorCode::Duplicate,
    );
    session
        .observe_server_frame(&event(
            2,
            json!({ "type": "conversation_changed", "conversationId": CONVERSATION_ID }),
        ))
        .unwrap();
    assert_code(
        session.observe_server_frame(&event(
            1,
            json!({ "type": "conversation_changed", "conversationId": CONVERSATION_ID }),
        )),
        ErrorCode::Stale,
    );
    assert_code(
        session.observe_server_frame(&event(
            0,
            json!({ "type": "conversation_changed", "conversationId": CONVERSATION_ID }),
        )),
        ErrorCode::InvalidRequest,
    );

    let error = |message: String| {
        decode_server_frame(&wire(&json!({
            "type": "error",
            "protocolVersion": 1,
            "error": {
                "code": "invalid_request",
                "message": message,
                "retryable": false,
            },
            "fatal": false,
        })))
    };
    assert_code(error(String::new()).map(|_| ()), ErrorCode::InvalidRequest);
    let too_long_error = error("x".repeat(MAX_ERROR_MESSAGE_BYTES + 1)).unwrap();
    assert_code(
        session.observe_server_frame(&too_long_error),
        ErrorCode::InvalidRequest,
    );
    let valid_error = RealtimeProtocolError::new(ErrorCode::InvalidRequest).into_error_frame(
        cipher_types::protocol::ProtocolVersion::V1,
        false,
        None,
    );
    session.observe_server_frame(&valid_error).unwrap();

    let invalid_nonces = [
        String::new(),
        "x".repeat(MAX_HEARTBEAT_NONCE_BYTES + 1),
        "has space".into(),
    ];
    for nonce in invalid_nonces {
        let frame = decode_server_frame(&wire(&json!({
            "type": "heartbeat",
            "protocolVersion": 1,
            "nonce": nonce,
        })))
        .unwrap();
        assert_code(
            session.observe_server_frame(&frame),
            ErrorCode::InvalidRequest,
        );
    }
}

#[test]
fn unknown_fields_and_frame_kinds_are_not_forward_compatible() {
    for raw in [
        json!({ "type": "hello", "supportedVersions": [1], "extra": true }),
        json!({ "type": "future", "protocolVersion": 1 }),
    ] {
        assert!(decode_client_frame(&wire(&raw)).is_err());
    }
    for raw in [
        json!({
            "type": "ack",
            "protocolVersion": 1,
            "acknowledgedClientSequence": 1,
            "idempotencyKey": IDEMPOTENCY_KEY,
            "extra": true,
        }),
        json!({ "type": "future", "protocolVersion": 1 }),
    ] {
        assert!(decode_server_frame(&wire(&raw)).is_err());
    }
}

#[test]
fn protocol_errors_produce_safe_error_frames() {
    for (error, expected_message) in [
        (
            RealtimeProtocolError::new(ErrorCode::Duplicate),
            "The frame was already accepted.",
        ),
        (
            RealtimeProtocolError::new(ErrorCode::Stale),
            "The frame is older than the current session state.",
        ),
        (
            RealtimeProtocolError::new(ErrorCode::UnsupportedVersion),
            "The realtime protocol version is not supported.",
        ),
        (
            RealtimeProtocolError::new(ErrorCode::TooLarge),
            "The realtime frame exceeds the size limit.",
        ),
        (
            RealtimeProtocolError::new(ErrorCode::InvalidRequest),
            "The realtime frame is invalid.",
        ),
    ] {
        assert_eq!(error.message(), expected_message);
        assert_eq!(error.to_string(), expected_message);
    }

    let frame = RealtimeProtocolError::new(ErrorCode::Duplicate).into_error_frame(
        cipher_types::protocol::ProtocolVersion::V1,
        false,
        Some(cipher_types::protocol::IdempotencyKey::parse(IDEMPOTENCY_KEY).unwrap()),
    );
    let ServerFrame::Error {
        error,
        fatal,
        idempotency_key,
        ..
    } = frame
    else {
        panic!("expected an error frame");
    };
    assert_eq!(error.code(), ErrorCode::Duplicate);
    assert_eq!(error.message(), "The frame was already accepted.");
    assert!(!error.is_retryable());
    assert!(!fatal);
    assert!(idempotency_key.is_some());
}

#[test]
fn sequence_values_are_non_zero_at_protocol_boundaries() {
    assert_eq!(Sequence::new(0), None);
    assert_eq!(Sequence::new(4).unwrap().get(), 4);
    assert!(
        serde_json::from_str::<ClientFrame>(
            r#"{"type":"heartbeat","protocolVersion":1,"sequence":0,"nonce":"zero"}"#,
        )
        .is_ok()
    );
}
