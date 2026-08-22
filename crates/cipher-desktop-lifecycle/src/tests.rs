use super::{
    DesktopLifecycleAction, DesktopLifecycleController, DesktopLifecycleErrorCode,
    DesktopLifecycleEvent, DesktopLifecycleState, MAX_ACTIVE_OPERATIONS, MAX_LIFECYCLE_ACTIONS,
    NativeTransportState,
};

fn started() -> DesktopLifecycleController {
    let mut controller = DesktopLifecycleController::new();
    controller
        .transition(DesktopLifecycleEvent::ColdStart)
        .unwrap();
    controller
}

#[test]
fn cold_start_is_single_use_and_enables_native_transport() {
    let mut controller = DesktopLifecycleController::default();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::ColdStart)
            .unwrap()
            .actions(),
        []
    );
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::Active
    );
    assert_eq!(
        controller.diagnostic().transport_state,
        NativeTransportState::Ready
    );
    assert_eq!(controller.diagnostic().cold_starts, 1);
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::ColdStart)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
}

#[test]
fn single_instance_launch_only_focuses_allowed_existing_windows() {
    let mut starting = DesktopLifecycleController::new();
    assert_eq!(
        starting
            .transition(DesktopLifecycleEvent::SingleInstanceLaunch)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );

    let mut controller = started();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::SingleInstanceLaunch)
            .unwrap()
            .actions(),
        [DesktopLifecycleAction::FocusMainWindow]
    );
    controller
        .transition(DesktopLifecycleEvent::AppLock)
        .unwrap();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::SingleInstanceLaunch)
            .unwrap()
            .actions(),
        [DesktopLifecycleAction::FocusMainWindow]
    );
    controller
        .transition(DesktopLifecycleEvent::NetworkOffline)
        .unwrap();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::SingleInstanceLaunch)
            .unwrap()
            .actions(),
        [DesktopLifecycleAction::FocusMainWindow]
    );
}

#[test]
fn native_operations_are_bounded_and_cancelled_on_lock() {
    let mut controller = started();
    for index in 0..MAX_ACTIVE_OPERATIONS {
        let event = if index.is_multiple_of(2) {
            DesktopLifecycleEvent::AuthenticationStarted
        } else {
            DesktopLifecycleEvent::MessagingStarted
        };
        controller.transition(event).unwrap();
    }
    assert_eq!(
        controller.diagnostic().active_operations,
        MAX_ACTIVE_OPERATIONS
    );
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::MessagingStarted)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::TooManyOperations
    );

    let transition = controller
        .transition(DesktopLifecycleEvent::AppLock)
        .unwrap();
    assert_eq!(
        transition.actions(),
        [
            DesktopLifecycleAction::CancelOperations,
            DesktopLifecycleAction::PauseTransport,
            DesktopLifecycleAction::LockAndPurgeRenderer,
        ]
    );
    assert_eq!(controller.diagnostic().active_operations, 0);
    assert_eq!(controller.diagnostic().renderer_epoch, 1);
    assert_eq!(
        controller.diagnostic().transport_state,
        NativeTransportState::Paused
    );
    assert!(transition.actions().len() <= MAX_LIFECYCLE_ACTIONS);
}

#[test]
fn operation_completion_requires_an_active_operation() {
    let mut controller = started();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::OperationFinished)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
    controller
        .transition(DesktopLifecycleEvent::AuthenticationStarted)
        .unwrap();
    controller
        .transition(DesktopLifecycleEvent::OperationFinished)
        .unwrap();
    assert_eq!(controller.diagnostic().active_operations, 0);
}

#[test]
fn sleep_cancels_active_work_and_wake_requires_a_locked_session() {
    let mut controller = started();
    controller
        .transition(DesktopLifecycleEvent::MessagingStarted)
        .unwrap();
    let sleep = controller.transition(DesktopLifecycleEvent::Sleep).unwrap();
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::Sleeping
    );
    assert_eq!(
        sleep.actions(),
        [
            DesktopLifecycleAction::CancelOperations,
            DesktopLifecycleAction::PauseTransport,
            DesktopLifecycleAction::LockAndPurgeRenderer,
        ]
    );
    controller
        .transition(DesktopLifecycleEvent::Wake {
            network_available: true,
        })
        .unwrap();
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::Locked
    );
    assert_eq!(
        controller.diagnostic().transport_state,
        NativeTransportState::Paused
    );
    assert_eq!(controller.diagnostic().wakes, 1);
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::Wake {
                network_available: true,
            })
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
}

#[test]
fn locked_sleep_only_pauses_transport_and_offline_wake_stays_offline() {
    let mut controller = started();
    controller
        .transition(DesktopLifecycleEvent::AppLock)
        .unwrap();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::Sleep)
            .unwrap()
            .actions(),
        [DesktopLifecycleAction::PauseTransport]
    );
    controller
        .transition(DesktopLifecycleEvent::Wake {
            network_available: false,
        })
        .unwrap();
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::Locked
    );
    assert_eq!(
        controller.diagnostic().transport_state,
        NativeTransportState::Offline
    );
}

#[test]
fn network_transitions_pause_and_reconnect_only_when_safe() {
    let mut controller = started();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::NetworkOffline)
            .unwrap()
            .actions(),
        [DesktopLifecycleAction::PauseTransport]
    );
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::Offline
    );
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::NetworkOffline)
            .unwrap()
            .actions(),
        []
    );
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::NetworkOnline)
            .unwrap()
            .actions(),
        [DesktopLifecycleAction::ReconnectTransport]
    );
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::Active
    );

    controller
        .transition(DesktopLifecycleEvent::AppLock)
        .unwrap();
    controller
        .transition(DesktopLifecycleEvent::NetworkOffline)
        .unwrap();
    assert_eq!(
        controller.diagnostic().transport_state,
        NativeTransportState::Offline
    );
    controller
        .transition(DesktopLifecycleEvent::NetworkOnline)
        .unwrap();
    assert_eq!(
        controller.diagnostic().transport_state,
        NativeTransportState::Paused
    );
}

#[test]
fn account_safety_events_all_purge_renderer_state() {
    for event in [
        DesktopLifecycleEvent::Logout,
        DesktopLifecycleEvent::AccountChanged,
        DesktopLifecycleEvent::DeviceRevoked,
    ] {
        let mut controller = started();
        let transition = controller.transition(event).unwrap();
        assert_eq!(
            controller.diagnostic().lifecycle_state,
            DesktopLifecycleState::Locked
        );
        assert_eq!(controller.diagnostic().renderer_epoch, 1);
        assert!(
            transition
                .actions()
                .contains(&DesktopLifecycleAction::LockAndPurgeRenderer)
        );
    }
}

#[test]
fn orderly_shutdown_cancels_purges_and_requires_completion() {
    let mut controller = started();
    controller
        .transition(DesktopLifecycleEvent::AuthenticationStarted)
        .unwrap();
    let transition = controller
        .transition(DesktopLifecycleEvent::ShutdownRequested)
        .unwrap();
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::ShuttingDown
    );
    assert_eq!(
        transition.actions(),
        [
            DesktopLifecycleAction::CancelOperations,
            DesktopLifecycleAction::PauseTransport,
            DesktopLifecycleAction::LockAndPurgeRenderer,
            DesktopLifecycleAction::FinalizeNativeShutdown,
        ]
    );
    controller
        .transition(DesktopLifecycleEvent::ShutdownFinished)
        .unwrap();
    assert_eq!(
        controller.diagnostic().lifecycle_state,
        DesktopLifecycleState::Stopped
    );
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::ShutdownFinished)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
}

#[test]
fn diagnostics_and_errors_exclude_sensitive_desktop_data() {
    let controller = started();
    let diagnostic = serde_json::to_string(&controller.diagnostic()).unwrap();
    for forbidden in [
        "token",
        "secret",
        "key",
        "mls",
        "plaintext",
        "ciphertext",
        "endpoint",
        "url",
        "screenshot",
        "account",
        "content",
    ] {
        assert!(!diagnostic.contains(forbidden));
    }
    for code in [
        DesktopLifecycleErrorCode::InvalidTransition,
        DesktopLifecycleErrorCode::TooManyOperations,
    ] {
        let error = super::DesktopLifecycleError::new(code);
        assert_eq!(error.to_string(), error.message());
        assert!(!error.to_string().contains("account"));
    }
}

#[test]
fn invalid_lifecycle_events_do_not_restart_stopped_or_sleeping_processes() {
    let mut controller = started();
    controller.transition(DesktopLifecycleEvent::Sleep).unwrap();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::NetworkOnline)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
    controller
        .transition(DesktopLifecycleEvent::Wake {
            network_available: true,
        })
        .unwrap();
    controller
        .transition(DesktopLifecycleEvent::ShutdownRequested)
        .unwrap();
    controller
        .transition(DesktopLifecycleEvent::ShutdownFinished)
        .unwrap();
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::NetworkOffline)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::AppLock)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
    assert_eq!(
        controller
            .transition(DesktopLifecycleEvent::ShutdownRequested)
            .unwrap_err()
            .code(),
        DesktopLifecycleErrorCode::InvalidTransition
    );
}
