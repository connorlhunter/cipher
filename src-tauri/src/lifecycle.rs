//! Native lifecycle integration and content-free renderer purge notifications.

use std::sync::Mutex;

use cipher_desktop_lifecycle::{
    DesktopLifecycleAction, DesktopLifecycleController, DesktopLifecycleError,
    DesktopLifecycleEvent, DesktopLifecycleTransition, SafeDesktopDiagnostic,
};
use cipher_native_transport::OperationCancellation;
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

/// The native work category that must be cancelled during a safety transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeOperationKind {
    /// A credential exchange or other native authentication operation.
    Authentication,
    /// A native messaging, encryption, or synchronization operation.
    Messaging,
}

impl NativeOperationKind {
    fn lifecycle_event(self) -> DesktopLifecycleEvent {
        match self {
            Self::Authentication => DesktopLifecycleEvent::AuthenticationStarted,
            Self::Messaging => DesktopLifecycleEvent::MessagingStarted,
        }
    }
}

/// An opaque native operation registered with the desktop lifecycle service.
///
/// Native HTTP and realtime code receives a clone of its cancellation handle;
/// the webview cannot create, inspect, or complete this value.
pub struct NativeOperation {
    id: u64,
    cancellation: OperationCancellation,
}

impl NativeOperation {
    /// Returns the shared cancellation handle for this native operation.
    pub fn cancellation(&self) -> OperationCancellation {
        self.cancellation.clone()
    }
}

struct ActiveNativeOperation {
    id: u64,
    cancellation: OperationCancellation,
}

struct DesktopLifecycleRuntime {
    controller: DesktopLifecycleController,
    operations: Vec<ActiveNativeOperation>,
    next_operation_id: u64,
}

impl DesktopLifecycleRuntime {
    fn new() -> Self {
        Self {
            controller: DesktopLifecycleController::new(),
            operations: Vec::new(),
            next_operation_id: 1,
        }
    }

    fn transition(
        &mut self,
        event: DesktopLifecycleEvent,
    ) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        let transition = self.controller.transition(event)?;
        if transition
            .actions()
            .contains(&DesktopLifecycleAction::CancelOperations)
        {
            for operation in &self.operations {
                operation.cancellation.cancel();
            }
            self.operations.clear();
        }
        Ok(transition)
    }
}

/// The process-owned lifecycle controller registered with the Tauri application.
pub struct DesktopLifecycleService {
    runtime: Mutex<DesktopLifecycleRuntime>,
}

impl DesktopLifecycleService {
    /// Creates the native lifecycle controller before cold start completes.
    pub fn new() -> Self {
        Self {
            runtime: Mutex::new(DesktopLifecycleRuntime::new()),
        }
    }

    /// Begins native work that must stop when the desktop enters a safety state.
    pub fn begin_native_operation(
        &self,
        kind: NativeOperationKind,
    ) -> Result<NativeOperation, ipc::IpcError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())?;
        let id = runtime.next_operation_id;
        let next_operation_id = id.checked_add(1).ok_or_else(ipc::IpcError::unavailable)?;
        runtime
            .transition(kind.lifecycle_event())
            .map_err(map_lifecycle_error)?;

        let cancellation = OperationCancellation::default();
        runtime.operations.push(ActiveNativeOperation {
            id,
            cancellation: cancellation.clone(),
        });
        runtime.next_operation_id = next_operation_id;
        Ok(NativeOperation { id, cancellation })
    }

    /// Marks completed native work as no longer cancellable by lifecycle cleanup.
    pub fn finish_native_operation(&self, operation: NativeOperation) -> Result<(), ipc::IpcError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())?;
        let Some(index) = runtime
            .operations
            .iter()
            .position(|active| active.id == operation.id)
        else {
            return if operation.cancellation.is_cancelled() {
                Ok(())
            } else {
                Err(ipc::IpcError::unavailable())
            };
        };

        runtime
            .transition(DesktopLifecycleEvent::OperationFinished)
            .map_err(map_lifecycle_error)?;
        runtime.operations.swap_remove(index);
        Ok(())
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
        self.runtime
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())
            .map(|runtime| runtime.controller.diagnostic())
    }

    fn transition(
        &self,
        event: DesktopLifecycleEvent,
    ) -> Result<DesktopLifecycleTransition, ipc::IpcError> {
        self.runtime
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
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Arc,
    };

    use cipher_desktop_lifecycle::{
        DesktopLifecycleEvent, DesktopLifecycleState, NativeTransportState,
    };
    use tauri::{Manager, WebviewWindowBuilder};

    use super::{
        DesktopLifecycleService, NativeOperationKind, RENDERER_ACCOUNT_CHANGED_EVENT,
        RENDERER_APP_LOCKED_EVENT, RENDERER_DEVICE_REVOKED_EVENT, RENDERER_LOGOUT_EVENT,
        handle_run_event, handle_single_instance_launch, renderer_purge_event,
    };

    fn managed_mock_app() -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_builder()
            .manage(DesktopLifecycleService::new())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        app
    }

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

    #[test]
    fn lifecycle_safety_events_cancel_the_native_operation_handles() {
        let service = DesktopLifecycleService::new();
        service
            .transition(DesktopLifecycleEvent::ColdStart)
            .unwrap();
        let authentication = service
            .begin_native_operation(NativeOperationKind::Authentication)
            .unwrap();
        let messaging = service
            .begin_native_operation(NativeOperationKind::Messaging)
            .unwrap();

        assert_eq!(service.diagnostic(Some(1)).unwrap().active_operations, 2);
        service.transition(DesktopLifecycleEvent::AppLock).unwrap();

        assert!(authentication.cancellation().is_cancelled());
        assert!(messaging.cancellation().is_cancelled());
        assert_eq!(service.diagnostic(Some(1)).unwrap().active_operations, 0);
        assert!(service.finish_native_operation(authentication).is_ok());
        assert!(service.finish_native_operation(messaging).is_ok());
    }

    #[test]
    fn completed_native_operations_leave_no_cancellable_handle() {
        let service = DesktopLifecycleService::new();
        service
            .transition(DesktopLifecycleEvent::ColdStart)
            .unwrap();
        let operation = service
            .begin_native_operation(NativeOperationKind::Authentication)
            .unwrap();
        let cancellation = operation.cancellation();

        service.finish_native_operation(operation).unwrap();

        assert!(!cancellation.is_cancelled());
        assert_eq!(service.diagnostic(Some(1)).unwrap().active_operations, 0);
    }

    #[test]
    fn rejects_native_operations_that_cannot_start_or_complete() {
        let service = DesktopLifecycleService::default();
        assert!(
            service
                .begin_native_operation(NativeOperationKind::Authentication)
                .is_err()
        );

        service
            .transition(DesktopLifecycleEvent::ColdStart)
            .unwrap();
        assert!(
            service
                .finish_native_operation(super::NativeOperation {
                    id: 42,
                    cancellation: Default::default(),
                })
                .is_err()
        );
    }

    #[test]
    fn poisoned_lifecycle_state_fails_closed_without_diagnostics() {
        let service = Arc::new(DesktopLifecycleService::new());
        let poisoned = Arc::clone(&service);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _guard = poisoned.runtime.lock().unwrap();
                panic!("test lifecycle lock poisoning");
            }))
            .is_err()
        );

        assert!(service.diagnostic(Some(1)).is_err());
        assert!(
            service
                .transition(DesktopLifecycleEvent::ColdStart)
                .is_err()
        );
    }

    #[test]
    fn app_handle_lifecycle_paths_use_the_managed_native_service() {
        let app = managed_mock_app();
        let handle = app.handle().clone();
        let service = app.state::<DesktopLifecycleService>();

        service
            .handle_native_event(&handle, DesktopLifecycleEvent::ColdStart)
            .unwrap();
        handle_single_instance_launch(&handle);
        service
            .handle_native_event(&handle, DesktopLifecycleEvent::Sleep)
            .unwrap();
        handle_run_event(&handle, &tauri::RunEvent::Resumed);

        let diagnostic = service.diagnostic(Some(1)).unwrap();
        assert_eq!(diagnostic.lifecycle_state, DesktopLifecycleState::Locked);
        assert_eq!(diagnostic.transport_state, NativeTransportState::Offline);
    }
}
