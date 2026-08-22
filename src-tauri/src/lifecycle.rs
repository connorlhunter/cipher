//! Native lifecycle integration and content-free renderer purge notifications.

use std::sync::Mutex;

use cipher_desktop_lifecycle::{
    DesktopLifecycleAction, DesktopLifecycleController, DesktopLifecycleError,
    DesktopLifecycleEvent, DesktopLifecycleTransition, SafeDesktopDiagnostic,
};
use tauri::{AppHandle, Emitter, EventTarget, Manager, Runtime};

use crate::{ipc, security::MAIN_WINDOW_LABEL};

/// Renderer event emitted after native logout cleanup.
pub const RENDERER_LOGOUT_EVENT: &str = "cipher://renderer-data/logout";
/// Renderer event emitted after native device-revocation cleanup.
pub const RENDERER_DEVICE_REVOKED_EVENT: &str = "cipher://renderer-data/device-revoked";
/// Renderer event emitted after native lock or sleep cleanup.
pub const RENDERER_APP_LOCKED_EVENT: &str = "cipher://renderer-data/app-locked";
/// Renderer event emitted after a native account replacement.
pub const RENDERER_ACCOUNT_CHANGED_EVENT: &str = "cipher://renderer-data/account-changed";

/// The process-owned lifecycle controller registered with the Tauri application.
pub struct DesktopLifecycleService {
    controller: Mutex<DesktopLifecycleController>,
}

impl DesktopLifecycleService {
    /// Creates the native lifecycle controller before cold start completes.
    pub const fn new() -> Self {
        Self {
            controller: Mutex::new(DesktopLifecycleController::new()),
        }
    }

    /// Applies a native lifecycle event and emits an empty renderer-purge event when required.
    ///
    /// This method is Rust-only. Tauri commands never accept lifecycle events from the webview.
    pub fn handle_native_event<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        event: DesktopLifecycleEvent,
    ) -> Result<(), ipc::IpcError> {
        let transition = self.transition(event)?;
        if transition
            .actions()
            .contains(&DesktopLifecycleAction::LockAndPurgeRenderer)
        {
            let event_name = renderer_purge_event(event).ok_or_else(ipc::IpcError::unavailable)?;
            app.emit_to(
                EventTarget::webview_window(MAIN_WINDOW_LABEL),
                event_name,
                (),
            )
            .map_err(|_| ipc::IpcError::unavailable())?;
        }
        Ok(())
    }

    /// Returns a bounded diagnostic view for the current desktop protocol version.
    pub fn diagnostic(
        &self,
        protocol_version: Option<u16>,
    ) -> Result<SafeDesktopDiagnostic, ipc::IpcError> {
        ipc::require_current_protocol_version(protocol_version)?;
        self.controller
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())
            .map(|controller| controller.diagnostic())
    }

    fn transition(
        &self,
        event: DesktopLifecycleEvent,
    ) -> Result<DesktopLifecycleTransition, ipc::IpcError> {
        self.controller
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())?
            .transition(event)
            .map_err(map_lifecycle_error)
    }
}

impl Default for DesktopLifecycleService {
    fn default() -> Self {
        Self::new()
    }
}

/// Handles a second desktop launch without forwarding command-line values into the webview.
pub fn handle_single_instance_launch<R: Runtime>(app: &AppHandle<R>) {
    let Some(service) = app.try_state::<DesktopLifecycleService>() else {
        return;
    };
    if service
        .handle_native_event(app, DesktopLifecycleEvent::SingleInstanceLaunch)
        .is_err()
    {
        return;
    }

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Applies safe event-loop transitions without passing operating-system details to the webview.
pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: &tauri::RunEvent) {
    let Some(service) = app.try_state::<DesktopLifecycleService>() else {
        return;
    };
    let lifecycle_event = match event {
        tauri::RunEvent::Resumed => Some(DesktopLifecycleEvent::Wake {
            network_available: false,
        }),
        tauri::RunEvent::ExitRequested { .. } => Some(DesktopLifecycleEvent::ShutdownRequested),
        tauri::RunEvent::Exit => Some(DesktopLifecycleEvent::ShutdownFinished),
        _ => None,
    };
    if let Some(lifecycle_event) = lifecycle_event {
        let _ = service.handle_native_event(app, lifecycle_event);
    }
}

fn map_lifecycle_error(_: DesktopLifecycleError) -> ipc::IpcError {
    ipc::IpcError::unavailable()
}

fn renderer_purge_event(event: DesktopLifecycleEvent) -> Option<&'static str> {
    match event {
        DesktopLifecycleEvent::Logout => Some(RENDERER_LOGOUT_EVENT),
        DesktopLifecycleEvent::DeviceRevoked => Some(RENDERER_DEVICE_REVOKED_EVENT),
        DesktopLifecycleEvent::AppLock | DesktopLifecycleEvent::Sleep => {
            Some(RENDERER_APP_LOCKED_EVENT)
        }
        DesktopLifecycleEvent::AccountChanged => Some(RENDERER_ACCOUNT_CHANGED_EVENT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use cipher_desktop_lifecycle::{
        DesktopLifecycleEvent, DesktopLifecycleState, NativeTransportState,
    };

    use super::{
        DesktopLifecycleService, RENDERER_ACCOUNT_CHANGED_EVENT, RENDERER_APP_LOCKED_EVENT,
        RENDERER_DEVICE_REVOKED_EVENT, RENDERER_LOGOUT_EVENT, renderer_purge_event,
    };

    #[test]
    fn renderer_purge_events_have_fixed_content_free_names() {
        assert_eq!(
            renderer_purge_event(DesktopLifecycleEvent::Logout),
            Some(RENDERER_LOGOUT_EVENT)
        );
        assert_eq!(
            renderer_purge_event(DesktopLifecycleEvent::DeviceRevoked),
            Some(RENDERER_DEVICE_REVOKED_EVENT)
        );
        assert_eq!(
            renderer_purge_event(DesktopLifecycleEvent::AppLock),
            Some(RENDERER_APP_LOCKED_EVENT)
        );
        assert_eq!(
            renderer_purge_event(DesktopLifecycleEvent::Sleep),
            Some(RENDERER_APP_LOCKED_EVENT)
        );
        assert_eq!(
            renderer_purge_event(DesktopLifecycleEvent::AccountChanged),
            Some(RENDERER_ACCOUNT_CHANGED_EVENT)
        );
        assert_eq!(renderer_purge_event(DesktopLifecycleEvent::ColdStart), None);
    }

    #[test]
    fn diagnostics_are_bounded_and_require_the_current_protocol() {
        let service = DesktopLifecycleService::new();
        assert!(service.diagnostic(Some(0)).is_err());
        assert!(service.transition(DesktopLifecycleEvent::ColdStart).is_ok());

        let diagnostic = service.diagnostic(Some(1)).unwrap();
        assert_eq!(diagnostic.lifecycle_state, DesktopLifecycleState::Active);
        assert_eq!(diagnostic.transport_state, NativeTransportState::Ready);
        let encoded = serde_json::to_string(&diagnostic).unwrap();
        assert!(!encoded.contains("token"));
        assert!(!encoded.contains("credential"));
        assert!(!encoded.contains("ciphertext"));
    }
}
