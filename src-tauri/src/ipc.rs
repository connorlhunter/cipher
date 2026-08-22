//! Allowlisted, versioned contracts between the Cipher webview and Rust core.

use cipher_types::protocol::ProtocolVersion;
use serde::Serialize;

/// The version used by newly introduced desktop commands.
pub const CURRENT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1;
/// The version retained temporarily for rolling desktop updates.
pub const PREVIOUS_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V0;
/// The maximum length of display-only status text.
pub const MAX_STATUS_MESSAGE_LENGTH: usize = 160;
const STATUS_MESSAGE: &str = "Desktop core is ready.";
const _: () = assert!(STATUS_MESSAGE.len() <= MAX_STATUS_MESSAGE_LENGTH);

/// A display-safe native-core status view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopStatus {
    /// Short bounded text intended for the current screen only.
    pub message: &'static str,
}

/// Typed error codes permitted across the Tauri boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorCode {
    /// A caller intentionally cancelled a cancellable operation.
    Cancelled,
    /// The payload did not match the allowlisted contract.
    InvalidRequest,
    /// The desktop protocol version is no longer compatible.
    UnsupportedVersion,
    /// The native core cannot currently fulfill the request.
    Unavailable,
}

/// A bounded error response safe to show in the webview.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    /// Stable error category for programmatic handling.
    pub code: IpcErrorCode,
    /// Safe display text without secret or account material.
    pub message: &'static str,
}

impl IpcError {
    /// Creates an unsupported-version response.
    pub const fn unsupported_version() -> Self {
        Self {
            code: IpcErrorCode::UnsupportedVersion,
            message: "This desktop version is not compatible with the native core.",
        }
    }

    /// Creates a generic native-service response without implementation details.
    pub const fn unavailable() -> Self {
        Self {
            code: IpcErrorCode::Unavailable,
            message: "This desktop service is not available.",
        }
    }
}

/// Returns whether the supplied desktop protocol version is compatible.
pub fn supports_protocol_version(version: ProtocolVersion) -> bool {
    version == CURRENT_PROTOCOL_VERSION || version == PREVIOUS_PROTOCOL_VERSION
}

/// Requires the current version for a command introduced after protocol version zero.
pub fn require_current_protocol_version(protocol_version: Option<u16>) -> Result<(), IpcError> {
    if ProtocolVersion::new(protocol_version.unwrap_or(PREVIOUS_PROTOCOL_VERSION.get()))
        != CURRENT_PROTOCOL_VERSION
    {
        return Err(IpcError::unsupported_version());
    }
    Ok(())
}

/// Returns the bounded status view for a compatible caller.
pub fn desktop_status(protocol_version: Option<u16>) -> Result<DesktopStatus, IpcError> {
    let version = ProtocolVersion::new(protocol_version.unwrap_or(PREVIOUS_PROTOCOL_VERSION.get()));
    if !supports_protocol_version(version) {
        return Err(IpcError::unsupported_version());
    }

    Ok(DesktopStatus {
        message: STATUS_MESSAGE,
    })
}

#[cfg(test)]
mod tests {
    use cipher_types::protocol::ProtocolVersion;

    use super::{
        CURRENT_PROTOCOL_VERSION, IpcErrorCode, MAX_STATUS_MESSAGE_LENGTH,
        PREVIOUS_PROTOCOL_VERSION, desktop_status, require_current_protocol_version,
        supports_protocol_version,
    };

    #[test]
    fn accepts_current_and_previous_protocol_versions() {
        assert!(supports_protocol_version(PREVIOUS_PROTOCOL_VERSION));
        assert!(supports_protocol_version(CURRENT_PROTOCOL_VERSION));
        assert!(!supports_protocol_version(ProtocolVersion::new(
            CURRENT_PROTOCOL_VERSION.get() + 1,
        )));
    }

    #[test]
    fn status_view_is_bounded_and_safe() {
        let status = desktop_status(Some(CURRENT_PROTOCOL_VERSION.get())).unwrap();
        assert!(status.message.len() <= MAX_STATUS_MESSAGE_LENGTH);
    }

    #[test]
    fn rejects_an_unsupported_protocol_version() {
        let error = desktop_status(Some(CURRENT_PROTOCOL_VERSION.get() + 1)).unwrap_err();
        assert_eq!(error.code, IpcErrorCode::UnsupportedVersion);
    }

    #[test]
    fn reserves_new_commands_for_the_current_protocol_version() {
        assert!(require_current_protocol_version(Some(CURRENT_PROTOCOL_VERSION.get())).is_ok());
        assert!(require_current_protocol_version(Some(PREVIOUS_PROTOCOL_VERSION.get())).is_err());
        assert!(require_current_protocol_version(None).is_err());
    }

    #[test]
    fn shared_status_fixtures_cover_current_and_previous_versions() {
        for fixture in [
            include_str!("../../contracts/ipc/v0/desktop-status.json"),
            include_str!("../../contracts/ipc/v1/desktop-status.json"),
        ] {
            let fixture: serde_json::Value = serde_json::from_str(fixture).unwrap();
            let version = ProtocolVersion::new(fixture["protocolVersion"].as_u64().unwrap() as u16);
            assert!(supports_protocol_version(version));
            assert_eq!(fixture["command"], "desktop_status");
            assert_eq!(fixture["response"]["message"], "Desktop core is ready.");
        }
    }
}
