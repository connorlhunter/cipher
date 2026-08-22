//! Native desktop lifecycle state and redacted diagnostics.
//!
//! The lifecycle controller owns cancellation, transport pause and reconnect
//! decisions, session locking, and renderer purge signals. It deliberately
//! stores no account, content, credential, endpoint, or capability data.

use std::fmt;

use serde::Serialize;

/// The maximum number of native operations that may be in flight at once.
pub const MAX_ACTIVE_OPERATIONS: u8 = 32;
/// The maximum number of actions one lifecycle transition may return.
pub const MAX_LIFECYCLE_ACTIONS: usize = 5;

/// A safe native desktop lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopLifecycleState {
    /// The desktop process has not completed its first native initialization.
    Starting,
    /// Native services may process supported work.
    Active,
    /// The session is locked and renderer state was purged.
    Locked,
    /// The device is asleep and native work is paused.
    Sleeping,
    /// The process is active but cannot contact the native transport endpoint.
    Offline,
    /// Native work is being cancelled and durable state is being closed.
    ShuttingDown,
    /// The native process completed its orderly shutdown sequence.
    Stopped,
}

/// A safe native transport availability state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeTransportState {
    /// The native transport may accept supported work.
    Ready,
    /// The native transport is paused until an explicit reconnect decision.
    Paused,
    /// The native transport is unavailable while the device is offline.
    Offline,
}

/// A lifecycle input that contains no account or message data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopLifecycleEvent {
    /// The desktop process completed its first cold start.
    ColdStart,
    /// A second launch request should focus the existing main window.
    SingleInstanceLaunch,
    /// The operating system notified the desktop process that it is sleeping.
    Sleep,
    /// The device woke and reports whether the network is currently usable.
    Wake {
        /// Whether native networking can safely be resumed after wake.
        network_available: bool,
    },
    /// The desktop process lost network reachability.
    NetworkOffline,
    /// The desktop process regained network reachability.
    NetworkOnline,
    /// The person using the desktop explicitly locked the application.
    AppLock,
    /// The active account was removed from the native session.
    Logout,
    /// A different account replaced the current account.
    AccountChanged,
    /// A native realtime event revoked this device.
    DeviceRevoked,
    /// Native authentication began a cancellable operation.
    AuthenticationStarted,
    /// Native messaging began a cancellable operation.
    MessagingStarted,
    /// One native authentication or messaging operation completed.
    OperationFinished,
    /// The desktop process began orderly shutdown.
    ShutdownRequested,
    /// The native shutdown sequence finished and the process may exit.
    ShutdownFinished,
}

/// A native action emitted by a lifecycle transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopLifecycleAction {
    /// Focus the existing window without transferring launch arguments to the renderer.
    FocusMainWindow,
    /// Cancel every active native authentication or messaging operation.
    CancelOperations,
    /// Pause native HTTP and realtime work.
    PauseTransport,
    /// Reconnect native HTTP and realtime work after an explicit online transition.
    ReconnectTransport,
    /// Lock native session state and purge every renderer-owned display cache.
    LockAndPurgeRenderer,
    /// Complete native durable-state cleanup before the process exits.
    FinalizeNativeShutdown,
}

/// A bounded result of one lifecycle transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopLifecycleTransition {
    actions: Vec<DesktopLifecycleAction>,
}

impl DesktopLifecycleTransition {
    /// Returns the ordered native actions for this transition.
    pub fn actions(&self) -> &[DesktopLifecycleAction] {
        &self.actions
    }

    fn none() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    fn with(actions: &[DesktopLifecycleAction]) -> Self {
        debug_assert!(actions.len() <= MAX_LIFECYCLE_ACTIONS);
        Self {
            actions: actions.to_vec(),
        }
    }
}

/// A stable reason a lifecycle event was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopLifecycleErrorCode {
    /// The event is not valid in the controller's current state.
    InvalidTransition,
    /// Starting another native operation would exceed the fixed concurrency limit.
    TooManyOperations,
}

/// A redacted lifecycle error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopLifecycleError {
    code: DesktopLifecycleErrorCode,
}

impl DesktopLifecycleError {
    /// Creates a redacted error from its stable category.
    pub const fn new(code: DesktopLifecycleErrorCode) -> Self {
        Self { code }
    }

    /// Returns the stable category without state or account details.
    pub const fn code(self) -> DesktopLifecycleErrorCode {
        self.code
    }

    /// Returns a fixed safe display message.
    pub const fn message(self) -> &'static str {
        match self.code {
            DesktopLifecycleErrorCode::InvalidTransition => {
                "The desktop lifecycle transition is not available."
            }
            DesktopLifecycleErrorCode::TooManyOperations => {
                "The desktop has too many active operations."
            }
        }
    }
}

impl fmt::Display for DesktopLifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for DesktopLifecycleError {}

/// A bounded diagnostic export that excludes all sensitive desktop state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeDesktopDiagnostic {
    /// The current native lifecycle state.
    pub lifecycle_state: DesktopLifecycleState,
    /// The current native transport availability state.
    pub transport_state: NativeTransportState,
    /// The renderer purge generation, used only to invalidate local display state.
    pub renderer_epoch: u64,
    /// The count of active native operations, never their purpose or content.
    pub active_operations: u8,
    /// The number of completed cold starts in this process.
    pub cold_starts: u32,
    /// The number of wake events in this process.
    pub wakes: u32,
}

/// Owns the safety-critical desktop lifecycle state for one process.
#[derive(Debug)]
pub struct DesktopLifecycleController {
    lifecycle_state: DesktopLifecycleState,
    transport_state: NativeTransportState,
    renderer_epoch: u64,
    active_operations: u8,
    cold_starts: u32,
    wakes: u32,
}

impl DesktopLifecycleController {
    /// Creates a controller before the first cold-start transition.
    pub const fn new() -> Self {
        Self {
            lifecycle_state: DesktopLifecycleState::Starting,
            transport_state: NativeTransportState::Paused,
            renderer_epoch: 0,
            active_operations: 0,
            cold_starts: 0,
            wakes: 0,
        }
    }

    /// Applies one safe desktop lifecycle event.
    pub fn transition(
        &mut self,
        event: DesktopLifecycleEvent,
    ) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        match event {
            DesktopLifecycleEvent::ColdStart => self.cold_start(),
            DesktopLifecycleEvent::SingleInstanceLaunch => self.focus_existing_window(),
            DesktopLifecycleEvent::Sleep => self.sleep(),
            DesktopLifecycleEvent::Wake { network_available } => self.wake(network_available),
            DesktopLifecycleEvent::NetworkOffline => self.network_offline(),
            DesktopLifecycleEvent::NetworkOnline => self.network_online(),
            DesktopLifecycleEvent::AppLock
            | DesktopLifecycleEvent::Logout
            | DesktopLifecycleEvent::AccountChanged
            | DesktopLifecycleEvent::DeviceRevoked => self.lock_and_purge(),
            DesktopLifecycleEvent::AuthenticationStarted
            | DesktopLifecycleEvent::MessagingStarted => self.start_operation(),
            DesktopLifecycleEvent::OperationFinished => self.finish_operation(),
            DesktopLifecycleEvent::ShutdownRequested => self.begin_shutdown(),
            DesktopLifecycleEvent::ShutdownFinished => self.finish_shutdown(),
        }
    }

    /// Returns the current redacted diagnostic export.
    pub const fn diagnostic(&self) -> SafeDesktopDiagnostic {
        SafeDesktopDiagnostic {
            lifecycle_state: self.lifecycle_state,
            transport_state: self.transport_state,
            renderer_epoch: self.renderer_epoch,
            active_operations: self.active_operations,
            cold_starts: self.cold_starts,
            wakes: self.wakes,
        }
    }

    fn cold_start(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        if self.lifecycle_state != DesktopLifecycleState::Starting {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            ));
        }

        self.lifecycle_state = DesktopLifecycleState::Active;
        self.transport_state = NativeTransportState::Ready;
        self.cold_starts = self.cold_starts.saturating_add(1);
        Ok(DesktopLifecycleTransition::none())
    }

    fn focus_existing_window(&self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        match self.lifecycle_state {
            DesktopLifecycleState::Active
            | DesktopLifecycleState::Locked
            | DesktopLifecycleState::Offline => Ok(DesktopLifecycleTransition::with(&[
                DesktopLifecycleAction::FocusMainWindow,
            ])),
            _ => Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            )),
        }
    }

    fn sleep(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        match self.lifecycle_state {
            DesktopLifecycleState::Active | DesktopLifecycleState::Offline => {
                self.lifecycle_state = DesktopLifecycleState::Sleeping;
                self.transport_state = NativeTransportState::Paused;
                Ok(self.cancel_lock_and_purge())
            }
            DesktopLifecycleState::Locked => {
                self.lifecycle_state = DesktopLifecycleState::Sleeping;
                self.transport_state = NativeTransportState::Paused;
                Ok(DesktopLifecycleTransition::with(&[
                    DesktopLifecycleAction::PauseTransport,
                ]))
            }
            _ => Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            )),
        }
    }

    fn wake(
        &mut self,
        network_available: bool,
    ) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        if self.lifecycle_state != DesktopLifecycleState::Sleeping {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            ));
        }

        self.wakes = self.wakes.saturating_add(1);
        self.lifecycle_state = DesktopLifecycleState::Locked;
        self.transport_state = if network_available {
            NativeTransportState::Paused
        } else {
            NativeTransportState::Offline
        };
        Ok(DesktopLifecycleTransition::none())
    }

    fn network_offline(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        match self.lifecycle_state {
            DesktopLifecycleState::Active => {
                self.lifecycle_state = DesktopLifecycleState::Offline;
                self.transport_state = NativeTransportState::Offline;
                Ok(DesktopLifecycleTransition::with(&[
                    DesktopLifecycleAction::PauseTransport,
                ]))
            }
            DesktopLifecycleState::Locked => {
                self.transport_state = NativeTransportState::Offline;
                Ok(DesktopLifecycleTransition::none())
            }
            DesktopLifecycleState::Offline => Ok(DesktopLifecycleTransition::none()),
            _ => Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            )),
        }
    }

    fn network_online(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        match self.lifecycle_state {
            DesktopLifecycleState::Offline => {
                self.lifecycle_state = DesktopLifecycleState::Active;
                self.transport_state = NativeTransportState::Ready;
                Ok(DesktopLifecycleTransition::with(&[
                    DesktopLifecycleAction::ReconnectTransport,
                ]))
            }
            DesktopLifecycleState::Locked => {
                self.transport_state = NativeTransportState::Paused;
                Ok(DesktopLifecycleTransition::none())
            }
            DesktopLifecycleState::Active => Ok(DesktopLifecycleTransition::none()),
            _ => Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            )),
        }
    }

    fn lock_and_purge(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        if matches!(
            self.lifecycle_state,
            DesktopLifecycleState::ShuttingDown | DesktopLifecycleState::Stopped
        ) {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            ));
        }

        self.lifecycle_state = DesktopLifecycleState::Locked;
        self.transport_state = NativeTransportState::Paused;
        self.renderer_epoch = self.renderer_epoch.saturating_add(1);
        self.active_operations = 0;
        Ok(DesktopLifecycleTransition::with(&[
            DesktopLifecycleAction::CancelOperations,
            DesktopLifecycleAction::PauseTransport,
            DesktopLifecycleAction::LockAndPurgeRenderer,
        ]))
    }

    fn start_operation(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        if self.lifecycle_state != DesktopLifecycleState::Active
            || self.transport_state != NativeTransportState::Ready
        {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            ));
        }
        if self.active_operations >= MAX_ACTIVE_OPERATIONS {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::TooManyOperations,
            ));
        }

        self.active_operations += 1;
        Ok(DesktopLifecycleTransition::none())
    }

    fn finish_operation(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        if self.active_operations == 0 {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            ));
        }

        self.active_operations -= 1;
        Ok(DesktopLifecycleTransition::none())
    }

    fn begin_shutdown(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        if self.lifecycle_state == DesktopLifecycleState::Stopped {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            ));
        }

        self.lifecycle_state = DesktopLifecycleState::ShuttingDown;
        self.transport_state = NativeTransportState::Paused;
        self.renderer_epoch = self.renderer_epoch.saturating_add(1);
        self.active_operations = 0;
        Ok(DesktopLifecycleTransition::with(&[
            DesktopLifecycleAction::CancelOperations,
            DesktopLifecycleAction::PauseTransport,
            DesktopLifecycleAction::LockAndPurgeRenderer,
            DesktopLifecycleAction::FinalizeNativeShutdown,
        ]))
    }

    fn finish_shutdown(&mut self) -> Result<DesktopLifecycleTransition, DesktopLifecycleError> {
        if self.lifecycle_state != DesktopLifecycleState::ShuttingDown {
            return Err(DesktopLifecycleError::new(
                DesktopLifecycleErrorCode::InvalidTransition,
            ));
        }

        self.lifecycle_state = DesktopLifecycleState::Stopped;
        Ok(DesktopLifecycleTransition::none())
    }

    fn cancel_lock_and_purge(&mut self) -> DesktopLifecycleTransition {
        self.renderer_epoch = self.renderer_epoch.saturating_add(1);
        self.active_operations = 0;
        DesktopLifecycleTransition::with(&[
            DesktopLifecycleAction::CancelOperations,
            DesktopLifecycleAction::PauseTransport,
            DesktopLifecycleAction::LockAndPurgeRenderer,
        ])
    }
}

impl Default for DesktopLifecycleController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
