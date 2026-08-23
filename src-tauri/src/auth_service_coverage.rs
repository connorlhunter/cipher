//! Deterministic coverage-only implementation of the native authentication surface.

use crate::{
    auth::{AuthenticationRequest, AuthenticationView, AuthenticationViewState, NativeAuthError},
    session::DesktopSessionService,
};
use cipher_native_transport::OperationCancellation;

/// Preserves the production service shape without starting external network work.
pub struct DesktopAuthenticationService;

impl DesktopAuthenticationService {
    /// Creates the deterministic coverage service.
    pub const fn new() -> Self {
        Self
    }

    /// Accepts session restoration setup without introducing a network dependency.
    pub fn install_refresher(
        &self,
        _: &DesktopSessionService,
        _: &OperationCancellation,
    ) -> Result<(), NativeAuthError> {
        Ok(())
    }

    /// Returns only a bounded unavailable result for coverage command tests.
    pub async fn authenticate(
        &self,
        _: &AuthenticationRequest,
        _: &DesktopSessionService,
        _: &OperationCancellation,
    ) -> AuthenticationView {
        AuthenticationView {
            state: AuthenticationViewState::Failed,
            message: "Authentication is temporarily unavailable.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopAuthenticationService;
    use crate::{auth::AuthenticationRequest, session::DesktopSessionService};
    use cipher_native_transport::OperationCancellation;

    #[test]
    fn coverage_adapter_is_fail_closed() {
        let service = DesktopAuthenticationService::new();
        assert!(matches!(service, DesktopAuthenticationService));
        let session = DesktopSessionService::new();
        let cancellation = OperationCancellation::default();
        assert!(service.install_refresher(&session, &cancellation).is_ok());
        let request = serde_json::from_value::<AuthenticationRequest>(serde_json::json!({
            "flow": "sign_in",
            "identifier": "person@example.test",
            "password": "passphrase"
        }))
        .unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(service.authenticate(&request, &session, &cancellation))
                .state,
            crate::auth::AuthenticationViewState::Failed
        );
    }
}
