//! Golden-fixture and boundary tests for shared Cipher protocol primitives.

use std::str::FromStr;

use cipher_types::protocol::{
    ChannelId, ConversationId, Cursor, DeviceId, ErrorCode, EventId, IdempotencyKey, MediaId,
    MessageId, ProtocolError, ProtocolVersion, ServerId, SessionId, SizeLimits, Timestamp, UserId,
};
use serde::{Deserialize, Serialize};

const FIXTURE: &str = include_str!("../../../contracts/primitives/v1.json");
const UUID: &str = "018f9a76-4c00-7a12-8b0c-4d5e6f708192";

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrimitiveFixture {
    user_id: UserId,
    device_id: DeviceId,
    session_id: SessionId,
    conversation_id: ConversationId,
    server_id: ServerId,
    channel_id: ChannelId,
    message_id: MessageId,
    event_id: EventId,
    media_id: MediaId,
    timestamp: Timestamp,
    cursor: Cursor,
    idempotency_key: IdempotencyKey,
    protocol_version: ProtocolVersion,
    error: ProtocolError,
}

#[test]
fn golden_fixture_round_trips_with_stable_json() {
    let fixture: PrimitiveFixture = serde_json::from_str(FIXTURE).unwrap();

    assert_eq!(
        serde_json::to_string_pretty(&fixture).unwrap(),
        FIXTURE.trim().replace("\r\n", "\n")
    );
    assert_eq!(fixture.user_id.as_str(), format!("usr_{UUID}"));
    assert_eq!(
        fixture.device_id.as_str(),
        format!("dev_{}", UUID.replace("4c00", "4c01"))
    );
    assert_eq!(fixture.timestamp.as_str(), "2026-08-21T18:42:15.123Z");
    assert_eq!(fixture.cursor.as_str(), "cur_AQIDBAUGBwgJCgsMDQ4PEA");
    assert_eq!(fixture.protocol_version, ProtocolVersion::V1);
    assert_eq!(fixture.error.code(), ErrorCode::UnsupportedVersion);
    assert_eq!(
        fixture.error.message(),
        "This Cipher version is no longer supported."
    );
    assert!(!fixture.error.is_retryable());
}

#[test]
fn identifiers_are_distinct_prefixed_uuid_v7_types() {
    macro_rules! assert_identifier {
        ($type:ty, $prefix:literal) => {{
            let value = format!("{}_{}", $prefix, UUID);
            let parsed = <$type>::parse(&value).unwrap();
            assert_eq!(parsed.as_str(), value);
            assert_eq!(parsed.to_string(), value);
            assert_eq!(<$type>::from_str(&value).unwrap(), parsed);
            assert_eq!(
                serde_json::to_string(&parsed).unwrap(),
                format!("\"{value}\"")
            );
            assert_eq!(
                serde_json::from_str::<$type>(&format!("\"{value}\"")).unwrap(),
                parsed
            );
        }};
    }

    assert_identifier!(UserId, "usr");
    assert_identifier!(DeviceId, "dev");
    assert_identifier!(SessionId, "ses");
    assert_identifier!(ConversationId, "cnv");
    assert_identifier!(ServerId, "srv");
    assert_identifier!(ChannelId, "chn");
    assert_identifier!(MessageId, "msg");
    assert_identifier!(EventId, "evt");
    assert_identifier!(MediaId, "med");
}

#[test]
fn identifiers_reject_wrong_prefixes_and_noncanonical_uuids() {
    for value in [
        "dev_018f9a76-4c00-7a12-8b0c-4d5e6f708192",
        "usr_018F9A76-4C00-7A12-8B0C-4D5E6F708192",
        "usr_018f9a76-4c00-4a12-8b0c-4d5e6f708192",
        "usr_018f9a76-4c00-7a12-7b0c-4d5e6f708192",
        "usr_018f9a764c007a128b0c4d5e6f708192",
        "usr_not-a-uuid",
    ] {
        assert!(UserId::parse(value).is_err(), "{value}");
        assert!(serde_json::from_str::<UserId>(&format!("\"{value}\"")).is_err());
    }
}

#[test]
fn cursor_is_bounded_opaque_base64url() {
    let cursor = Cursor::parse("cur_AQIDBAUGBwgJCgsMDQ4PEA").unwrap();
    assert_eq!(cursor.to_string(), cursor.as_str());
    assert_eq!(Cursor::from_str(cursor.as_str()).unwrap(), cursor);
    assert_eq!(
        serde_json::to_string(&cursor).unwrap(),
        "\"cur_AQIDBAUGBwgJCgsMDQ4PEA\""
    );
    assert_eq!(
        serde_json::from_str::<Cursor>("\"cur_AQIDBAUGBwgJCgsMDQ4PEA\"").unwrap(),
        cursor
    );

    for value in ["", "cur_", "other_AQID", "cur_abcde", "cur_has=padding"] {
        assert!(Cursor::parse(value).is_err(), "{value}");
    }
    let oversized = format!("cur_{}", "A".repeat(SizeLimits::MAX_CURSOR_BYTES));
    assert!(Cursor::parse(&oversized).is_err());
}

#[test]
fn idempotency_keys_are_prefixed_uuid_v7_values() {
    let value = format!("idem_{UUID}");
    let key = IdempotencyKey::parse(&value).unwrap();

    assert_eq!(key.as_str(), value);
    assert_eq!(key.to_string(), value);
    assert_eq!(IdempotencyKey::from_str(&value).unwrap(), key);
    assert_eq!(serde_json::to_string(&key).unwrap(), format!("\"{value}\""));
    assert_eq!(
        serde_json::from_str::<IdempotencyKey>(&format!("\"{value}\"")).unwrap(),
        key
    );
    assert!(IdempotencyKey::parse("idem_not-a-uuid").is_err());
}

#[test]
fn timestamps_are_canonical_utc_milliseconds() {
    let timestamp = Timestamp::parse("2024-02-29T23:59:59.999Z").unwrap();
    assert_eq!(timestamp.as_str(), "2024-02-29T23:59:59.999Z");
    assert_eq!(timestamp.to_string(), timestamp.as_str());
    assert_eq!(Timestamp::from_str(timestamp.as_str()).unwrap(), timestamp);
    assert_eq!(
        serde_json::to_string(&timestamp).unwrap(),
        "\"2024-02-29T23:59:59.999Z\""
    );
    assert_eq!(
        serde_json::from_str::<Timestamp>("\"2024-02-29T23:59:59.999Z\"").unwrap(),
        timestamp
    );

    for value in [
        "2024-02-29T23:59:59Z",
        "2024-02-29T23:59:59.999+00:00",
        "2023-02-29T23:59:59.999Z",
        "2024-04-31T23:59:59.999Z",
        "2024-02-29T24:00:00.000Z",
        "2024-02-29T23:60:00.000Z",
        "2024-02-29T23:59:60.000Z",
        "0000-01-01T00:00:00.000Z",
        "2024-02-29t23:59:59.999z",
    ] {
        assert!(Timestamp::parse(value).is_err(), "{value}");
    }
}

#[test]
fn protocol_versions_are_numeric_and_transport_neutral() {
    let version = ProtocolVersion::new(1);

    assert_eq!(version.get(), 1);
    assert_eq!(version.to_string(), "1");
    assert_eq!(ProtocolVersion::V0.get(), 0);
    assert_eq!(ProtocolVersion::V1, version);
    assert_eq!(serde_json::to_string(&version).unwrap(), "1");
    assert_eq!(
        serde_json::from_str::<ProtocolVersion>("1").unwrap(),
        version
    );
    assert_eq!(ProtocolVersion::new(2).get(), 2);
    assert_eq!(
        serde_json::from_str::<ProtocolVersion>("0").unwrap(),
        ProtocolVersion::V0
    );
}

#[test]
fn protocol_errors_use_stable_codes_and_safe_bounded_messages() {
    let error = ProtocolError::new(ErrorCode::Unavailable, "Try again shortly.", true).unwrap();

    assert_eq!(error.code(), ErrorCode::Unavailable);
    assert_eq!(error.message(), "Try again shortly.");
    assert!(error.is_retryable());
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        "{\"code\":\"unavailable\",\"message\":\"Try again shortly.\",\"retryable\":true}"
    );
    assert_eq!(
        serde_json::from_str::<ProtocolError>(
            "{\"code\":\"unavailable\",\"message\":\"Try again shortly.\",\"retryable\":true}"
        )
        .unwrap(),
        error
    );

    for message in [
        "",
        " has surrounding whitespace",
        "has surrounding whitespace ",
        "contains\nnewlines",
    ] {
        assert!(ProtocolError::new(ErrorCode::InvalidRequest, message, false).is_err());
    }
    assert!(
        ProtocolError::new(
            ErrorCode::InvalidRequest,
            "x".repeat(SizeLimits::MAX_ERROR_MESSAGE_BYTES + 1),
            false,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ProtocolError>(
            "{\"code\":\"internal\",\"message\":\"No details.\",\"retryable\":false,\"extra\":true}"
        )
        .is_err()
    );
}

#[test]
fn primitive_errors_describe_the_rejected_type_without_echoing_input() {
    let error = UserId::parse("usr_sensitive-value").unwrap_err();

    assert_eq!(
        error.to_string(),
        "invalid UserId: must contain a UUID version 7"
    );
}

#[test]
fn documented_limits_are_consistent_with_the_closed_alpha_scope() {
    assert_eq!(SizeLimits::MAX_HTTP_BODY_BYTES, 64 * 1024);
    assert_eq!(SizeLimits::MAX_REALTIME_FRAME_BYTES, 64 * 1024);
    assert_eq!(SizeLimits::MAX_MESSAGE_CIPHERTEXT_BYTES, 32 * 1024);
    assert_eq!(SizeLimits::MAX_PHOTO_SOURCE_BYTES, 5 * 1024 * 1024);
    assert_eq!(SizeLimits::MAX_PHOTO_DIMENSION, 2_048);
    assert_eq!(SizeLimits::MAX_CURSOR_BYTES, 512);
    assert_eq!(SizeLimits::MAX_ERROR_MESSAGE_BYTES, 256);
    assert_eq!(SizeLimits::IDEMPOTENCY_RETENTION_SECONDS, 86_400);
}
