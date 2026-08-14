//! Data types shared by Cipher services and clients.

use serde::Serialize;

/// JSON status returned by health and readiness endpoints.
#[derive(Debug, Serialize)]
pub struct ServiceStatus {
    /// Machine-readable service state.
    pub status: &'static str,
}

impl ServiceStatus {
    /// Creates a ready service status.
    pub const fn ready() -> Self {
        Self { status: "ok" }
    }
}
