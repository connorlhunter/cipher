//! Versioned, bounded WebSocket frames for Cipher realtime traffic.

use std::{error::Error, fmt};

use cipher_types::protocol::{
    ConversationId, Cursor, DeviceId, ErrorCode, EventId, IdempotencyKey, MessageId, ProtocolError,
    ProtocolVersion, SessionId, SizeLimits,
};
use serde::{Deserialize, Serialize};

/// The largest UTF-8 JSON control frame accepted by the protocol.
pub const MAX_FRAME_BYTES: usize = SizeLimits::MAX_REALTIME_FRAME_BYTES;
/// The current protocol offers at most the current and one previous version.
pub const MAX_SUPPORTED_VERSIONS: usize = 2;
/// The maximum number of conversations a subscription command may change.
pub const MAX_SUBSCRIPTION_CONVERSATIONS: usize = 100;
/// The largest printable heartbeat nonce.
pub const MAX_HEARTBEAT_NONCE_BYTES: usize = 64;
/// The fixed v1 heartbeat interval negotiated in a welcome frame.
pub const HEARTBEAT_INTERVAL_MS: u32 = 30_000;
/// The largest safe, fixed diagnostic text included in an error frame.
pub const MAX_ERROR_MESSAGE_BYTES: usize = 160;

/// A non-zero, monotonically increasing per-connection frame sequence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Sequence(u64);

impl Sequence {
    /// Creates a sequence when its wire value is non-zero.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// Returns the wire value of this sequence.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A client-to-server realtime frame.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientFrame {
    /// Opens a connection and selects a mutually supported protocol version.
    Hello {
        /// Versions the client can speak, ordered from newest to oldest.
        #[serde(rename = "supportedVersions")]
        supported_versions: Vec<ProtocolVersion>,
        /// The last opaque server cursor safely committed by the client.
        #[serde(rename = "resumeCursor", skip_serializing_if = "Option::is_none")]
        resume_cursor: Option<Cursor>,
        /// The latest server sequence safely committed by the client.
        #[serde(
            rename = "lastAcknowledgedServerSequence",
            skip_serializing_if = "Option::is_none"
        )]
        last_acknowledged_server_sequence: Option<Sequence>,
    },
    /// Requests a bounded realtime subscription change.
    Command {
        /// The version selected by the preceding hello exchange.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The strictly increasing client frame sequence.
        sequence: Sequence,
        /// A stable key that prevents replaying a command on reconnect.
        #[serde(rename = "idempotencyKey")]
        idempotency_key: IdempotencyKey,
        /// The subscription operation to apply.
        command: RealtimeCommand,
    },
    /// Acknowledges committed server events.
    Ack {
        /// The version selected by the preceding hello exchange.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The strictly increasing client frame sequence.
        sequence: Sequence,
        /// The highest contiguous server sequence committed by the client.
        #[serde(rename = "acknowledgedServerSequence")]
        acknowledged_server_sequence: Sequence,
    },
    /// Proves that an otherwise idle connection remains live.
    Heartbeat {
        /// The version selected by the preceding hello exchange.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The strictly increasing client frame sequence.
        sequence: Sequence,
        /// A printable nonce the peer must echo unchanged.
        nonce: String,
    },
}

/// A narrowly scoped client subscription command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RealtimeCommand {
    /// Starts receiving metadata events for the specified conversations.
    Subscribe {
        /// Conversations to add to the connection's subscription set.
        #[serde(rename = "conversationIds")]
        conversation_ids: Vec<ConversationId>,
    },
    /// Stops receiving metadata events for the specified conversations.
    Unsubscribe {
        /// Conversations to remove from the connection's subscription set.
        #[serde(rename = "conversationIds")]
        conversation_ids: Vec<ConversationId>,
    },
}

/// A server-to-client realtime frame.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerFrame {
    /// Confirms the handshake and tells the client how to maintain the session.
    Welcome {
        /// The version selected from the client's hello offer.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The server-issued connection session identifier.
        #[serde(rename = "sessionId")]
        session_id: SessionId,
        /// The sequence assigned to the next event emitted by the server.
        #[serde(rename = "nextServerSequence")]
        next_server_sequence: Sequence,
        /// The fixed interval at which both peers exchange heartbeats.
        #[serde(rename = "heartbeatIntervalMs")]
        heartbeat_interval_ms: u32,
        /// Whether the supplied resume cursor was retained and authorized.
        resumed: bool,
    },
    /// Delivers ordered metadata for a subscribed conversation or device.
    Event {
        /// The version selected by the preceding hello exchange.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The strictly increasing server event sequence.
        sequence: Sequence,
        /// A stable event identity used for reconnect de-duplication.
        #[serde(rename = "eventId")]
        event_id: EventId,
        /// The opaque cursor a client presents on a future hello frame.
        cursor: Cursor,
        /// The metadata event delivered to the native client.
        event: RealtimeEvent,
    },
    /// Confirms that the server accepted a client command.
    Ack {
        /// The version selected by the preceding hello exchange.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The client sequence whose command was accepted.
        #[serde(rename = "acknowledgedClientSequence")]
        acknowledged_client_sequence: Sequence,
        /// The idempotency key of the accepted command.
        #[serde(rename = "idempotencyKey")]
        idempotency_key: IdempotencyKey,
    },
    /// Reports a safe, typed protocol or application error.
    Error {
        /// The version selected by the preceding hello exchange.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The bounded, typed, shared error envelope.
        error: ProtocolError,
        /// Whether the peer closes the connection immediately after this frame.
        fatal: bool,
        /// The command key associated with the error, when applicable.
        #[serde(rename = "idempotencyKey", skip_serializing_if = "Option::is_none")]
        idempotency_key: Option<IdempotencyKey>,
    },
    /// Echoes a peer's heartbeat nonce.
    Heartbeat {
        /// The version selected by the preceding hello exchange.
        #[serde(rename = "protocolVersion")]
        protocol_version: ProtocolVersion,
        /// The echoed printable heartbeat nonce.
        nonce: String,
    },
}

/// Metadata-only events delivered by the realtime server.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RealtimeEvent {
    /// Indicates that an encrypted message record is ready to fetch over HTTP.
    MessageAvailable {
        /// The conversation containing the message.
        #[serde(rename = "conversationId")]
        conversation_id: ConversationId,
        /// The encrypted message record to retrieve through HTTP.
        #[serde(rename = "messageId")]
        message_id: MessageId,
    },
    /// Indicates that conversation metadata should be reconciled through HTTP.
    ConversationChanged {
        /// The conversation whose metadata changed.
        #[serde(rename = "conversationId")]
        conversation_id: ConversationId,
    },
    /// Indicates that a device no longer has access to the account.
    DeviceRevoked {
        /// The device whose access was revoked.
        #[serde(rename = "deviceId")]
        device_id: DeviceId,
    },
}

/// A safe protocol-validation failure that can be converted to an error frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeProtocolError {
    code: ErrorCode,
}

impl RealtimeProtocolError {
    fn new(code: ErrorCode) -> Self {
        Self { code }
    }

    /// Returns the shared machine-readable reason for the rejected frame.
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns a fixed, safe diagnostic for this rejection.
    pub const fn message(&self) -> &'static str {
        match self.code {
            ErrorCode::Duplicate => "The frame was already accepted.",
            ErrorCode::Stale => "The frame is older than the current session state.",
            ErrorCode::UnsupportedVersion => "The realtime protocol version is not supported.",
            ErrorCode::TooLarge => "The realtime frame exceeds the size limit.",
            _ => "The realtime frame is invalid.",
        }
    }

    /// Builds the corresponding bounded server error frame.
    pub fn into_error_frame(
        self,
        protocol_version: ProtocolVersion,
        fatal: bool,
        idempotency_key: Option<IdempotencyKey>,
    ) -> ServerFrame {
        let error = ProtocolError::new(self.code, self.message(), false)
            .expect("fixed realtime error messages are valid ProtocolError values");
        ServerFrame::Error {
            protocol_version,
            error,
            fatal,
            idempotency_key,
        }
    }
}

impl fmt::Display for RealtimeProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl Error for RealtimeProtocolError {}

/// Decodes and bounds a client text frame before applying connection state.
pub fn decode_client_frame(frame: &str) -> Result<ClientFrame, RealtimeProtocolError> {
    decode_frame(frame)
}

/// Decodes and bounds a server text frame before applying connection state.
pub fn decode_server_frame(frame: &str) -> Result<ServerFrame, RealtimeProtocolError> {
    decode_frame(frame)
}

fn decode_frame<T>(frame: &str) -> Result<T, RealtimeProtocolError>
where
    T: for<'de> Deserialize<'de>,
{
    if frame.len() > MAX_FRAME_BYTES {
        return Err(RealtimeProtocolError::new(ErrorCode::TooLarge));
    }

    serde_json::from_str(frame).map_err(|_| RealtimeProtocolError::new(ErrorCode::InvalidRequest))
}

/// Tracks the negotiated version and ordering state for one realtime connection.
#[derive(Debug)]
pub struct RealtimeSession {
    supported_versions: Vec<ProtocolVersion>,
    phase: SessionPhase,
}

#[derive(Debug)]
enum SessionPhase {
    AwaitingHello,
    Active(ActiveSession),
}

#[derive(Debug)]
struct ActiveSession {
    protocol_version: ProtocolVersion,
    resume_cursor: Option<Cursor>,
    last_client_sequence: Option<Sequence>,
    last_acknowledged_server_sequence: Option<Sequence>,
    last_server_sequence: Option<Sequence>,
    seen_idempotency_keys: Vec<IdempotencyKey>,
}

impl RealtimeSession {
    /// Creates a session with the transport-owned compatible protocol versions.
    pub fn new(supported_versions: Vec<ProtocolVersion>) -> Self {
        Self {
            supported_versions,
            phase: SessionPhase::AwaitingHello,
        }
    }

    /// Creates the current realtime compatibility policy, which accepts v1 and v0.
    pub fn v1() -> Self {
        Self::new(vec![ProtocolVersion::V1, ProtocolVersion::V0])
    }

    /// Returns the version selected by an accepted hello frame.
    pub fn selected_protocol_version(&self) -> Option<ProtocolVersion> {
        match &self.phase {
            SessionPhase::AwaitingHello => None,
            SessionPhase::Active(session) => Some(session.protocol_version),
        }
    }

    /// Returns the opaque cursor supplied during the accepted hello exchange.
    pub fn resume_cursor(&self) -> Option<&Cursor> {
        match &self.phase {
            SessionPhase::AwaitingHello => None,
            SessionPhase::Active(session) => session.resume_cursor.as_ref(),
        }
    }

    /// Returns the highest server sequence the client has committed so far.
    pub fn last_acknowledged_server_sequence(&self) -> Option<Sequence> {
        match &self.phase {
            SessionPhase::AwaitingHello => None,
            SessionPhase::Active(session) => session.last_acknowledged_server_sequence,
        }
    }

    /// Decodes and validates one client text frame against the connection state.
    pub fn receive_text(&mut self, frame: &str) -> Result<(), RealtimeProtocolError> {
        self.receive(decode_client_frame(frame)?)
    }

    /// Validates one decoded client frame against the connection state.
    pub fn receive(&mut self, frame: ClientFrame) -> Result<(), RealtimeProtocolError> {
        match (&mut self.phase, frame) {
            (
                SessionPhase::AwaitingHello,
                ClientFrame::Hello {
                    supported_versions,
                    resume_cursor,
                    last_acknowledged_server_sequence,
                    ..
                },
            ) => {
                validate_offered_versions(&supported_versions)?;
                if let Some(sequence) = last_acknowledged_server_sequence {
                    validate_sequence(sequence)?;
                }
                let protocol_version = supported_versions
                    .into_iter()
                    .find(|version| self.supported_versions.contains(version))
                    .ok_or_else(|| RealtimeProtocolError::new(ErrorCode::UnsupportedVersion))?;
                self.phase = SessionPhase::Active(ActiveSession {
                    protocol_version,
                    resume_cursor,
                    last_client_sequence: None,
                    last_acknowledged_server_sequence,
                    last_server_sequence: None,
                    seen_idempotency_keys: Vec::new(),
                });
                Ok(())
            }
            (SessionPhase::AwaitingHello, _) => {
                Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest))
            }
            (SessionPhase::Active(_), ClientFrame::Hello { .. }) => {
                Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest))
            }
            (SessionPhase::Active(session), frame) => session.receive(frame),
        }
    }

    /// Records a server frame so a subsequent client acknowledgement is checked.
    pub fn observe_server_frame(
        &mut self,
        frame: &ServerFrame,
    ) -> Result<(), RealtimeProtocolError> {
        let SessionPhase::Active(session) = &mut self.phase else {
            return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
        };

        session.observe_server_frame(frame)
    }
}

impl Default for RealtimeSession {
    fn default() -> Self {
        Self::v1()
    }
}

impl ActiveSession {
    fn receive(&mut self, frame: ClientFrame) -> Result<(), RealtimeProtocolError> {
        match frame {
            ClientFrame::Hello { .. } => Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest)),
            ClientFrame::Command {
                protocol_version,
                sequence,
                idempotency_key,
                command,
            } => {
                self.validate_version(protocol_version)?;
                self.accept_client_sequence(sequence)?;
                validate_command(&command)?;
                if self.seen_idempotency_keys.contains(&idempotency_key) {
                    return Err(RealtimeProtocolError::new(ErrorCode::Duplicate));
                }
                self.seen_idempotency_keys.push(idempotency_key);
                Ok(())
            }
            ClientFrame::Ack {
                protocol_version,
                sequence,
                acknowledged_server_sequence,
            } => {
                self.validate_version(protocol_version)?;
                self.accept_client_sequence(sequence)?;
                self.validate_acknowledgement(acknowledged_server_sequence)?;
                self.last_acknowledged_server_sequence = Some(acknowledged_server_sequence);
                Ok(())
            }
            ClientFrame::Heartbeat {
                protocol_version,
                sequence,
                nonce,
            } => {
                self.validate_version(protocol_version)?;
                self.accept_client_sequence(sequence)?;
                validate_nonce(&nonce)?;
                Ok(())
            }
        }
    }

    fn observe_server_frame(&mut self, frame: &ServerFrame) -> Result<(), RealtimeProtocolError> {
        match frame {
            ServerFrame::Welcome {
                protocol_version,
                next_server_sequence,
                heartbeat_interval_ms,
                ..
            } => {
                self.validate_version(*protocol_version)?;
                validate_sequence(*next_server_sequence)?;
                if *heartbeat_interval_ms != HEARTBEAT_INTERVAL_MS {
                    return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
                }
            }
            ServerFrame::Event {
                protocol_version,
                sequence,
                ..
            } => {
                self.validate_version(*protocol_version)?;
                self.validate_server_sequence(*sequence)?;
                self.last_server_sequence = Some(*sequence);
            }
            ServerFrame::Ack {
                protocol_version, ..
            } => self.validate_version(*protocol_version)?,
            ServerFrame::Error {
                protocol_version,
                error,
                ..
            } => {
                self.validate_version(*protocol_version)?;
                validate_error_message(error.message())?;
            }
            ServerFrame::Heartbeat {
                protocol_version,
                nonce,
            } => {
                self.validate_version(*protocol_version)?;
                validate_nonce(nonce)?;
            }
        }

        Ok(())
    }

    fn validate_version(&self, version: ProtocolVersion) -> Result<(), RealtimeProtocolError> {
        if version == self.protocol_version {
            Ok(())
        } else {
            Err(RealtimeProtocolError::new(ErrorCode::UnsupportedVersion))
        }
    }

    fn validate_client_sequence(&self, sequence: Sequence) -> Result<(), RealtimeProtocolError> {
        validate_sequence(sequence)?;
        match self.last_client_sequence {
            Some(last) if sequence == last => Err(RealtimeProtocolError::new(ErrorCode::Duplicate)),
            Some(last) if sequence < last => Err(RealtimeProtocolError::new(ErrorCode::Stale)),
            _ => Ok(()),
        }
    }

    fn accept_client_sequence(&mut self, sequence: Sequence) -> Result<(), RealtimeProtocolError> {
        self.validate_client_sequence(sequence)?;
        self.last_client_sequence = Some(sequence);
        Ok(())
    }

    fn validate_server_sequence(&self, sequence: Sequence) -> Result<(), RealtimeProtocolError> {
        validate_sequence(sequence)?;
        match self.last_server_sequence {
            Some(last) if sequence == last => Err(RealtimeProtocolError::new(ErrorCode::Duplicate)),
            Some(last) if sequence < last => Err(RealtimeProtocolError::new(ErrorCode::Stale)),
            _ => Ok(()),
        }
    }

    fn validate_acknowledgement(&self, sequence: Sequence) -> Result<(), RealtimeProtocolError> {
        validate_sequence(sequence)?;
        match self.last_acknowledged_server_sequence {
            Some(last) if sequence == last => {
                return Err(RealtimeProtocolError::new(ErrorCode::Duplicate));
            }
            Some(last) if sequence < last => {
                return Err(RealtimeProtocolError::new(ErrorCode::Stale));
            }
            _ => {}
        }

        match self.last_server_sequence {
            Some(last_server_sequence) if sequence <= last_server_sequence => Ok(()),
            _ => Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest)),
        }
    }
}

fn validate_offered_versions(
    supported_versions: &[ProtocolVersion],
) -> Result<(), RealtimeProtocolError> {
    if supported_versions.is_empty() || supported_versions.len() > MAX_SUPPORTED_VERSIONS {
        return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
    }

    if supported_versions.windows(2).any(|pair| pair[0] <= pair[1]) {
        return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
    }

    Ok(())
}

fn validate_sequence(sequence: Sequence) -> Result<(), RealtimeProtocolError> {
    if sequence.get() == 0 {
        Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest))
    } else {
        Ok(())
    }
}

fn validate_command(command: &RealtimeCommand) -> Result<(), RealtimeProtocolError> {
    let conversation_ids = match command {
        RealtimeCommand::Subscribe { conversation_ids }
        | RealtimeCommand::Unsubscribe { conversation_ids } => conversation_ids,
    };

    if conversation_ids.is_empty() {
        return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
    }
    if conversation_ids.len() > MAX_SUBSCRIPTION_CONVERSATIONS {
        return Err(RealtimeProtocolError::new(ErrorCode::TooLarge));
    }
    if conversation_ids
        .iter()
        .enumerate()
        .any(|(index, id)| conversation_ids[..index].contains(id))
    {
        return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
    }

    Ok(())
}

fn validate_nonce(nonce: &str) -> Result<(), RealtimeProtocolError> {
    if nonce.is_empty()
        || nonce.len() > MAX_HEARTBEAT_NONCE_BYTES
        || !nonce.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
    }

    Ok(())
}

fn validate_error_message(message: &str) -> Result<(), RealtimeProtocolError> {
    if message.is_empty() || message.len() > MAX_ERROR_MESSAGE_BYTES {
        return Err(RealtimeProtocolError::new(ErrorCode::InvalidRequest));
    }

    Ok(())
}

#[cfg(test)]
#[path = "tests/frames.rs"]
mod tests;
