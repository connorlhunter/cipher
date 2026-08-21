//! Canonical primitives shared by Cipher's versioned protocols.
//!
//! Application identifiers are lower-case, prefixed UUID version 7 values. Cipher
//! creates its own `UserId` during account provisioning and stores a private
//! one-to-one mapping from the Cognito subject to that identifier. Cognito subjects
//! are authentication-provider data and never become Cipher protocol identifiers.
//! This lets Cipher keep a stable public identity if its authentication provider
//! changes later.
//!
//! All timestamps are UTC RFC 3339 values with exactly millisecond precision. Cursors
//! are opaque server-issued base64url tokens. Clients must not construct or interpret
//! a cursor, and must only reuse an `IdempotencyKey` for retries of the same
//! authenticated mutation.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Limits that apply before a versioned HTTP, realtime, or IPC contract adds narrower limits.
pub struct SizeLimits;

impl SizeLimits {
    /// The largest serialized HTTP request or response body, excluding direct S3 uploads.
    pub const MAX_HTTP_BODY_BYTES: usize = 64 * 1024;
    /// The largest serialized realtime frame.
    pub const MAX_REALTIME_FRAME_BYTES: usize = 64 * 1024;
    /// The largest encrypted message body before transport encoding.
    pub const MAX_MESSAGE_CIPHERTEXT_BYTES: usize = 32 * 1024;
    /// The largest photo source accepted for client-side encryption and upload.
    pub const MAX_PHOTO_SOURCE_BYTES: usize = 5 * 1024 * 1024;
    /// The maximum width or height of a source photo in pixels.
    pub const MAX_PHOTO_DIMENSION: u32 = 2_048;
    /// The largest server-issued opaque cursor, including its `cur_` prefix.
    pub const MAX_CURSOR_BYTES: usize = 512;
    /// The largest safe, user-displayable protocol error message.
    pub const MAX_ERROR_MESSAGE_BYTES: usize = 256;
    /// How long a server retains an idempotent mutation result for safe retries.
    pub const IDEMPOTENCY_RETENTION_SECONDS: u64 = 24 * 60 * 60;
}

/// Describes why a value cannot be used as a canonical Cipher primitive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimitiveParseError {
    primitive: &'static str,
    reason: &'static str,
}

impl PrimitiveParseError {
    fn new(primitive: &'static str, reason: &'static str) -> Self {
        Self { primitive, reason }
    }
}

impl fmt::Display for PrimitiveParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid {}: {}", self.primitive, self.reason)
    }
}

impl std::error::Error for PrimitiveParseError {}

macro_rules! prefixed_uuid_v7 {
    ($name:ident, $prefix:literal, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// The wire prefix that distinguishes this identifier from every other Cipher ID.
            pub const PREFIX: &str = concat!($prefix, "_");

            /// Parses a canonical, lower-case prefixed UUID version 7 identifier.
            pub fn parse(value: &str) -> Result<Self, PrimitiveParseError> {
                let uuid = value.strip_prefix(Self::PREFIX).ok_or_else(|| {
                    PrimitiveParseError::new(stringify!($name), "has the wrong prefix")
                })?;
                validate_uuid_v7(uuid, stringify!($name))?;
                Ok(Self(value.into()))
            }

            /// Returns the canonical string sent on the wire.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = PrimitiveParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(D::Error::custom)
            }
        }
    };
}

prefixed_uuid_v7!(
    UserId,
    "usr",
    "An application-owned public account identifier, mapped privately from a Cognito subject."
);
prefixed_uuid_v7!(
    DeviceId,
    "dev",
    "An identifier for one registered Cipher desktop device."
);
prefixed_uuid_v7!(
    SessionId,
    "ses",
    "An identifier for one authenticated Cipher session."
);
prefixed_uuid_v7!(
    ConversationId,
    "cnv",
    "An identifier for a direct or group conversation."
);
prefixed_uuid_v7!(ServerId, "srv", "An identifier for a Cipher server.");
prefixed_uuid_v7!(
    ChannelId,
    "chn",
    "An identifier for a channel within a Cipher server."
);
prefixed_uuid_v7!(MessageId, "msg", "An identifier for an encrypted message.");
prefixed_uuid_v7!(
    EventId,
    "evt",
    "An identifier for a persisted protocol event."
);
prefixed_uuid_v7!(
    MediaId,
    "med",
    "An identifier for client-encrypted photo or icon media."
);

/// A client-generated key for retrying one authenticated mutation without duplicating it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// The wire prefix reserved for idempotency keys.
    pub const PREFIX: &str = "idem_";

    /// Parses a canonical, lower-case UUID version 7 idempotency key.
    pub fn parse(value: &str) -> Result<Self, PrimitiveParseError> {
        let uuid = value
            .strip_prefix(Self::PREFIX)
            .ok_or_else(|| PrimitiveParseError::new("IdempotencyKey", "has the wrong prefix"))?;
        validate_uuid_v7(uuid, "IdempotencyKey")?;
        Ok(Self(value.into()))
    }

    /// Returns the canonical string sent on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for IdempotencyKey {
    type Err = PrimitiveParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for IdempotencyKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for IdempotencyKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

/// An opaque server-issued position within a bounded event or list stream.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Cursor(String);

impl Cursor {
    /// The wire prefix reserved for opaque cursors.
    pub const PREFIX: &str = "cur_";

    /// Parses a canonical opaque base64url cursor without padding.
    pub fn parse(value: &str) -> Result<Self, PrimitiveParseError> {
        if value.len() > SizeLimits::MAX_CURSOR_BYTES {
            return Err(PrimitiveParseError::new("Cursor", "exceeds the size limit"));
        }

        let token = value
            .strip_prefix(Self::PREFIX)
            .ok_or_else(|| PrimitiveParseError::new("Cursor", "has the wrong prefix"))?;
        if token.is_empty() {
            return Err(PrimitiveParseError::new("Cursor", "has an empty token"));
        }
        if token.len() % 4 == 1 {
            return Err(PrimitiveParseError::new(
                "Cursor",
                "is not unpadded base64url",
            ));
        }
        if !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(PrimitiveParseError::new(
                "Cursor",
                "is not unpadded base64url",
            ));
        }

        Ok(Self(value.into()))
    }

    /// Returns the opaque string sent on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for Cursor {
    type Err = PrimitiveParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for Cursor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

/// A canonical UTC RFC 3339 timestamp with exactly millisecond precision.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(String);

impl Timestamp {
    /// Parses a canonical `YYYY-MM-DDTHH:MM:SS.mmmZ` timestamp.
    pub fn parse(value: &str) -> Result<Self, PrimitiveParseError> {
        validate_timestamp(value)?;
        Ok(Self(value.into()))
    }

    /// Returns the canonical string sent on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for Timestamp {
    type Err = PrimitiveParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

/// A numeric version carried by a versioned Cipher protocol envelope.
///
/// A primitive version is not an acceptance policy: HTTP, realtime, and IPC each
/// choose their own supported versions while they roll out independently.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    /// The initial compatibility version retained by a contract that explicitly supports it.
    pub const V0: Self = Self(0);
    /// The first current-version value used by Cipher contracts.
    pub const V1: Self = Self(1);

    /// Creates a protocol version without deciding whether any endpoint supports it.
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the numeric version encoded in protocol envelopes.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl Serialize for ProtocolVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u16(self.0)
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::new(u16::deserialize(deserializer)?))
    }
}

/// A stable machine-readable category for an expected protocol failure.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request or frame is malformed or violates a field constraint.
    InvalidRequest,
    /// The caller did not provide a valid active session.
    Unauthenticated,
    /// The caller has no permission for the requested resource or action.
    Forbidden,
    /// The requested resource does not exist or is not visible to the caller.
    NotFound,
    /// The requested action conflicts with the resource's current state.
    Conflict,
    /// The request duplicates a previous non-idempotent command or event.
    Duplicate,
    /// The request uses a cursor, session, or sequence position that is too old.
    Stale,
    /// The request, transfer, or credential has expired.
    Expired,
    /// The request uses a protocol version the endpoint does not support.
    UnsupportedVersion,
    /// The request or frame exceeds a documented size limit.
    TooLarge,
    /// The caller must slow down before retrying.
    RateLimited,
    /// A temporary service condition allows a safe retry.
    Unavailable,
    /// An unexpected server condition occurred; messages must not expose sensitive details.
    Internal,
}

/// A bounded, display-safe error envelope shared by Cipher protocols.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolError {
    code: ErrorCode,
    message: String,
    retryable: bool,
}

impl ProtocolError {
    /// Creates an error with a stable code, a safe user-displayable message, and retry guidance.
    pub fn new(
        code: ErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Result<Self, PrimitiveParseError> {
        let message = message.into();
        validate_error_message(&message)?;
        Ok(Self {
            code,
            message,
            retryable,
        })
    }

    /// Returns the stable machine-readable error code.
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns the bounded safe message intended for user display or logs.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns whether the caller may retry the same operation after backoff.
    pub const fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl<'de> Deserialize<'de> for ProtocolError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireError {
            code: ErrorCode,
            message: String,
            retryable: bool,
        }

        let wire = WireError::deserialize(deserializer)?;
        Self::new(wire.code, wire.message, wire.retryable).map_err(D::Error::custom)
    }
}

fn validate_uuid_v7(value: &str, primitive: &'static str) -> Result<(), PrimitiveParseError> {
    if value.len() != 36 {
        return Err(PrimitiveParseError::new(
            primitive,
            "must contain a UUID version 7",
        ));
    }

    for (index, byte) in value.bytes().enumerate() {
        match index {
            8 | 13 | 18 | 23 if byte != b'-' => {
                return Err(PrimitiveParseError::new(
                    primitive,
                    "must contain a UUID version 7",
                ));
            }
            8 | 13 | 18 | 23 => {}
            14 if byte != b'7' => {
                return Err(PrimitiveParseError::new(
                    primitive,
                    "must contain a UUID version 7",
                ));
            }
            19 if !matches!(byte, b'8' | b'9' | b'a' | b'b') => {
                return Err(PrimitiveParseError::new(
                    primitive,
                    "must contain a UUID version 7",
                ));
            }
            _ if !(byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')) => {
                return Err(PrimitiveParseError::new(
                    primitive,
                    "must contain a UUID version 7",
                ));
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_timestamp(value: &str) -> Result<(), PrimitiveParseError> {
    let bytes = value.as_bytes();
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[23] != b'Z'
    {
        return Err(PrimitiveParseError::new(
            "Timestamp",
            "must use YYYY-MM-DDTHH:MM:SS.mmmZ",
        ));
    }

    for index in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22] {
        if !bytes[index].is_ascii_digit() {
            return Err(PrimitiveParseError::new(
                "Timestamp",
                "must use YYYY-MM-DDTHH:MM:SS.mmmZ",
            ));
        }
    }

    let year = decimal_u16(&bytes[0..4]);
    let month = decimal_u8(&bytes[5..7]);
    let day = decimal_u8(&bytes[8..10]);
    let hour = decimal_u8(&bytes[11..13]);
    let minute = decimal_u8(&bytes[14..16]);
    let second = decimal_u8(&bytes[17..19]);

    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(PrimitiveParseError::new(
            "Timestamp",
            "is not a real UTC calendar time",
        ));
    }

    Ok(())
}

fn decimal_u16(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0_u16, |value, byte| value * 10 + u16::from(*byte - b'0'))
}

fn decimal_u8(bytes: &[u8]) -> u8 {
    bytes
        .iter()
        .fold(0_u8, |value, byte| value * 10 + (*byte - b'0'))
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn validate_error_message(value: &str) -> Result<(), PrimitiveParseError> {
    if value.is_empty() || value.len() > SizeLimits::MAX_ERROR_MESSAGE_BYTES {
        return Err(PrimitiveParseError::new(
            "ProtocolError",
            "message is empty or exceeds the size limit",
        ));
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(PrimitiveParseError::new(
            "ProtocolError",
            "message has unsupported whitespace or control characters",
        ));
    }
    Ok(())
}
