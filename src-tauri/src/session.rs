//! Native session establishment, restoration, and refresh for the desktop core.
//!
//! The webview has no route to this module. Access tokens remain in process
//! memory and can only leave this module through the native
//! [`SessionAuthenticator`] boundary. Refresh material is held by the platform
//! credential store and is never persisted in a file or renderer-owned store.

use std::{
    fmt,
    sync::{Arc, Condvar, Mutex, MutexGuard, RwLock},
    time::{Duration, Instant},
};

use cipher_native_transport::{
    AccessToken, NativeTransportError, NativeTransportErrorCode, OperationCancellation,
    SessionAuthenticator,
};

use crate::credential_store::{
    CredentialEntry, CredentialKind, CredentialScope, CredentialScopeError, CredentialStore,
    CredentialStoreError, PlatformCredentialStore, SecretBytes,
};

/// The maximum number of native refresh exchanges attempted for one refresh flight.
pub const MAX_REFRESH_ATTEMPTS: u8 = 2;
/// The largest accepted lifetime for an in-memory access token.
pub const MAX_ACCESS_TOKEN_LIFETIME: Duration = Duration::from_secs(15 * 60);

const SINGLE_FLIGHT_WAIT: Duration = Duration::from_millis(10);
const CLEANUP_ATTEMPTS: u8 = 2;
const ACTIVE_SESSION_SCOPE_REFERENCE: &str = "cipher native active session v1";

/// An opaque, validated account scope that is eligible for native session restoration.
///
/// Callers create this only after validating the stable account reference. The
/// source reference is immediately converted into a [`CredentialScope`] and
/// is never retained by this type.
#[derive(Clone, Eq, PartialEq)]
pub struct SupportedSession {
    scope: CredentialScope,
}

impl SupportedSession {
    /// Creates an eligible native session scope from a stable account reference.
    pub fn new(account_reference: &str) -> Result<Self, CredentialScopeError> {
        Ok(Self {
            scope: CredentialScope::new(account_reference)?,
        })
    }

    fn refresh_entry(&self) -> CredentialEntry {
        self.scope.entry(CredentialKind::RefreshMaterial)
    }

    fn commit_value(&self) -> SecretBytes {
        SecretBytes::new(self.scope.digest().to_vec())
    }

    fn from_commit_value(value: &SecretBytes) -> Result<Self, NativeSessionError> {
        let digest = value
            .as_bytes()
            .try_into()
            .map_err(|_| NativeSessionError::ReauthenticationRequired)?;
        Ok(Self {
            scope: CredentialScope::from_digest(digest),
        })
    }

    fn load_committed<S: CredentialStore + ?Sized>(
        store: &S,
    ) -> Result<Option<Self>, NativeSessionError> {
        store
            .load(&active_session_entry())
            .map_err(map_credential_error)?
            .map(|value| Self::from_commit_value(&value))
            .transpose()
    }

    fn is_committed<S: CredentialStore + ?Sized>(
        &self,
        store: &S,
    ) -> Result<bool, NativeSessionError> {
        Ok(Self::load_committed(store)?.as_ref() == Some(self))
    }
}

fn active_session_entry() -> CredentialEntry {
    CredentialScope::new(ACTIVE_SESSION_SCOPE_REFERENCE)
        .expect("the static active-session scope must remain valid")
        .entry(CredentialKind::ActiveSessionScope)
}

impl fmt::Debug for SupportedSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SupportedSession([redacted])")
    }
}

/// An initial native session that must persist its refresh material before it becomes active.
pub struct SessionGrant {
    access_token: AccessToken,
    refresh_material: SecretBytes,
    valid_for: Duration,
}

impl SessionGrant {
    /// Creates a bounded native session grant.
    pub fn new(
        access_token: AccessToken,
        refresh_material: SecretBytes,
        valid_for: Duration,
    ) -> Result<Self, NativeSessionError> {
        validate_lifetime(valid_for)?;
        Ok(Self {
            access_token,
            refresh_material,
            valid_for,
        })
    }
}

impl fmt::Debug for SessionGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionGrant")
            .field("access_token", &"[redacted]")
            .field("refresh_material", &"[redacted]")
            .field("valid_for", &self.valid_for)
            .finish()
    }
}

/// A refreshed in-memory access token, optionally accompanied by rotated refresh material.
pub struct SessionRefresh {
    access_token: AccessToken,
    refresh_material: Option<SecretBytes>,
    valid_for: Duration,
}

impl SessionRefresh {
    /// Creates a refresh result that continues to use the stored refresh material.
    pub fn new(access_token: AccessToken, valid_for: Duration) -> Result<Self, NativeSessionError> {
        validate_lifetime(valid_for)?;
        Ok(Self {
            access_token,
            refresh_material: None,
            valid_for,
        })
    }

    /// Creates a refresh result that must replace the native refresh material before activation.
    pub fn with_rotated_refresh_material(
        access_token: AccessToken,
        refresh_material: SecretBytes,
        valid_for: Duration,
    ) -> Result<Self, NativeSessionError> {
        validate_lifetime(valid_for)?;
        Ok(Self {
            access_token,
            refresh_material: Some(refresh_material),
            valid_for,
        })
    }
}

impl fmt::Debug for SessionRefresh {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionRefresh")
            .field("access_token", &"[redacted]")
            .field(
                "refresh_material",
                &self
                    .refresh_material
                    .as_ref()
                    .map(|_| "[redacted]")
                    .unwrap_or("[unchanged]"),
            )
            .field("valid_for", &self.valid_for)
            .finish()
    }
}

/// Exchanges native refresh material for an in-memory access token.
pub trait SessionRefresher: Send + Sync {
    /// Refreshes a session and may return a replacement for the stored refresh material.
    fn refresh(
        &self,
        refresh_material: &SecretBytes,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRefresh, NativeTransportError>;
}

/// Supplies monotonic time for native session expiry checks.
pub trait SessionClock: Send + Sync {
    /// Returns the current monotonic instant.
    fn now(&self) -> Instant;
}

/// The production monotonic clock for native sessions.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSessionClock;

impl SessionClock for SystemSessionClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// The result of restoring a supported native session after a process restart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRestore {
    /// Refresh material was present and produced an in-memory access token.
    Restored,
    /// No refresh material exists for the supported session.
    Absent,
    /// A committed session is waiting for the native refresh implementation.
    Deferred,
}

/// A redacted native session state that never claims an expired token is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeSessionState {
    /// There is no in-memory access token.
    Empty,
    /// A non-expired access token is held in process memory only.
    Active,
    /// A session establishment or refresh flight is in progress.
    Refreshing,
    /// The stored session is unusable and sign-in is required again.
    ReauthenticationRequired,
}

/// A fixed, redacted failure category for native session work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeSessionError {
    /// The operation was cancelled before native session work completed.
    Cancelled,
    /// No refresh material exists for the requested supported session.
    NoSession,
    /// The supplied access-token lifetime is outside the accepted bound.
    InvalidLifetime,
    /// The session cannot be refreshed and requires an interactive sign-in.
    ReauthenticationRequired,
    /// The platform credential store or refresh endpoint is temporarily unavailable.
    Unavailable,
}

impl fmt::Display for NativeSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("The native session operation was cancelled."),
            Self::NoSession | Self::ReauthenticationRequired => {
                formatter.write_str("The native session requires sign-in.")
            }
            Self::InvalidLifetime => formatter.write_str("The native session lifetime is invalid."),
            Self::Unavailable => formatter.write_str("The native session is unavailable."),
        }
    }
}

impl std::error::Error for NativeSessionError {}

/// A process-owned session cache backed by platform-held refresh material.
///
/// The manager is bound to exactly one [`SupportedSession`]. It does not
/// enumerate credential-store entries, so an application restart can restore
/// only an account scope that the caller already validated as supported.
pub struct NativeSessionManager<S: CredentialStore + ?Sized, R, C = SystemSessionClock> {
    store: Arc<S>,
    supported_session: SupportedSession,
    refresher: R,
    clock: C,
    runtime: Mutex<SessionRuntime>,
    wake: Condvar,
}

impl<S, R> NativeSessionManager<S, R, SystemSessionClock>
where
    S: CredentialStore + ?Sized,
    R: SessionRefresher,
{
    /// Creates a native session manager with the production monotonic clock.
    pub fn new(store: Arc<S>, supported_session: SupportedSession, refresher: R) -> Self {
        Self::with_clock(store, supported_session, refresher, SystemSessionClock)
    }
}

impl<S, R, C> NativeSessionManager<S, R, C>
where
    S: CredentialStore + ?Sized,
    R: SessionRefresher,
    C: SessionClock,
{
    /// Creates a native session manager with an explicit monotonic clock.
    pub fn with_clock(
        store: Arc<S>,
        supported_session: SupportedSession,
        refresher: R,
        clock: C,
    ) -> Self {
        Self {
            store,
            supported_session,
            refresher,
            clock,
            runtime: Mutex::new(SessionRuntime::default()),
            wake: Condvar::new(),
        }
    }

    /// Persists refresh material, then activates the supplied in-memory access token.
    ///
    /// A failed native write never activates the candidate access token.
    pub fn establish(
        &self,
        grant: SessionGrant,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeSessionError> {
        self.claim_establishing(cancellation)?;
        let result = self.persist_grant(grant, cancellation);
        self.finish_establishing(result)
    }

    /// Restores this supported session only after a refresh succeeds.
    ///
    /// Access tokens are not persisted, so a restart always uses the platform
    /// refresh entry before reporting [`SessionRestore::Restored`].
    pub fn restore(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRestore, NativeSessionError> {
        match self.acquire_access_token(cancellation) {
            Ok(_) => Ok(SessionRestore::Restored),
            Err(NativeSessionError::NoSession) => Ok(SessionRestore::Absent),
            Err(error) => Err(error),
        }
    }

    /// Returns the current redacted state without exposing a token or account reference.
    pub fn state(&self) -> NativeSessionState {
        let Ok(mut runtime) = self.runtime.lock() else {
            return NativeSessionState::ReauthenticationRequired;
        };
        if runtime.retired {
            return NativeSessionState::Empty;
        }
        if !matches!(&runtime.operation, SessionOperation::Idle) {
            return NativeSessionState::Refreshing;
        }
        if runtime
            .cached
            .as_ref()
            .is_some_and(|cached| cached.expires_at <= self.clock.now())
        {
            runtime.cached = None;
            if runtime.state == NativeSessionState::Active {
                runtime.state = NativeSessionState::Empty;
            }
        }
        runtime.state
    }

    /// Cancels the manager-owned refresh flight during a native safety transition.
    pub fn cancel_active_refresh(&self) {
        let Ok(runtime) = self.runtime.lock() else {
            return;
        };
        if let SessionOperation::Refreshing { cancellation, .. } = &runtime.operation {
            cancellation.cancel();
        }
    }

    fn cancel_active_refresh_and_wait(&self) -> Result<(), NativeSessionError> {
        let mut runtime = self.lock_runtime()?;
        runtime.retired = true;
        if let SessionOperation::Refreshing { cancellation, .. } = &runtime.operation {
            cancellation.cancel();
        }
        while !matches!(&runtime.operation, SessionOperation::Idle) {
            runtime = self
                .wake
                .wait(runtime)
                .map_err(|_| NativeSessionError::Unavailable)?;
        }
        runtime.cached = None;
        runtime.state = NativeSessionState::Empty;
        Ok(())
    }

    fn claim_establishing(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeSessionError> {
        let mut runtime = self.lock_runtime()?;
        runtime = self.wait_for_idle(runtime, cancellation)?;
        ensure_not_cancelled(cancellation)?;
        if runtime.retired {
            return Err(NativeSessionError::Cancelled);
        }
        runtime.operation = SessionOperation::Establishing;
        runtime.state = NativeSessionState::Refreshing;
        runtime.completed_flight = None;
        Ok(())
    }

    fn persist_grant(
        &self,
        grant: SessionGrant,
        cancellation: &OperationCancellation,
    ) -> Result<CachedAccessToken, NativeSessionError> {
        ensure_not_cancelled(cancellation)?;
        let SessionGrant {
            access_token,
            refresh_material,
            valid_for,
        } = grant;
        let expires_at = expiration(self.clock.now(), valid_for)?;
        let previous_session = SupportedSession::load_committed(self.store.as_ref())?;
        self.ensure_entry_absent(&active_session_entry())?;
        ensure_not_cancelled(cancellation)?;

        let entry = self.supported_session.refresh_entry();
        if let Err(error) = self.store.replace(&entry, &refresh_material) {
            return Err(self.fail_establishment(map_credential_error(error)));
        }
        if let Err(error) = ensure_not_cancelled(cancellation) {
            return Err(self.fail_establishment(error));
        }

        if let Some(previous_session) = previous_session
            && previous_session != self.supported_session
            && let Err(error) = self.ensure_entry_absent(&previous_session.refresh_entry())
        {
            return Err(self.fail_establishment(error));
        }
        if let Err(error) = ensure_not_cancelled(cancellation) {
            return Err(self.fail_establishment(error));
        }

        if let Err(error) = self.store.replace(
            &active_session_entry(),
            &self.supported_session.commit_value(),
        ) {
            return Err(self.fail_establishment(map_credential_error(error)));
        }
        if let Err(error) = ensure_not_cancelled(cancellation) {
            return Err(self.fail_establishment(error));
        }

        Ok(CachedAccessToken {
            access_token,
            expires_at,
        })
    }

    fn fail_establishment(&self, original: NativeSessionError) -> NativeSessionError {
        self.rollback_establishment().err().unwrap_or(original)
    }

    fn rollback_establishment(&self) -> Result<(), NativeSessionError> {
        let commit_result = self.ensure_entry_absent(&active_session_entry());
        let refresh_result = self.ensure_entry_absent(&self.supported_session.refresh_entry());
        commit_result.and(refresh_result)
    }

    fn ensure_entry_absent(&self, entry: &CredentialEntry) -> Result<(), NativeSessionError> {
        let mut last_error = None;
        for _ in 0..CLEANUP_ATTEMPTS {
            if let Err(error) = self.store.delete(entry) {
                last_error = Some(map_credential_error(error));
            }
            match self.store.load(entry) {
                Ok(None) => return Ok(()),
                Ok(Some(_)) => {}
                Err(error) => last_error = Some(map_credential_error(error)),
            }
        }
        Err(last_error.unwrap_or(NativeSessionError::Unavailable))
    }

    fn finish_establishing(
        &self,
        result: Result<CachedAccessToken, NativeSessionError>,
    ) -> Result<(), NativeSessionError> {
        let mut runtime = self.lock_runtime()?;
        runtime.operation = SessionOperation::Idle;
        match result {
            Ok(cached) => {
                runtime.cached = Some(cached);
                runtime.state = NativeSessionState::Active;
                self.wake.notify_all();
                Ok(())
            }
            Err(error) => {
                runtime.cached = None;
                runtime.state = state_after_failure(error);
                self.wake.notify_all();
                Err(error)
            }
        }
    }

    fn acquire_access_token(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeSessionError> {
        let mut observed_flight = None;

        loop {
            ensure_not_cancelled(cancellation)?;
            let now = self.clock.now();
            let mut runtime = self.lock_runtime()?;

            if runtime.retired {
                return Err(NativeSessionError::Cancelled);
            }

            if let Some(cached) = runtime.cached.as_ref()
                && cached.expires_at > now
            {
                return Ok(cached.access_token.clone());
            }

            if runtime.cached.is_some() {
                runtime.cached = None;
                if runtime.state == NativeSessionState::Active {
                    runtime.state = NativeSessionState::Empty;
                }
            }

            if runtime.state == NativeSessionState::ReauthenticationRequired {
                return Err(NativeSessionError::ReauthenticationRequired);
            }

            match &runtime.operation {
                SessionOperation::Idle => {
                    if let Some(flight) = observed_flight
                        && let Some(completed) = runtime.completed_flight
                        && completed.id == flight
                        && let Some(error) = completed.error
                    {
                        return Err(error);
                    }

                    drop(runtime);
                    let committed = match self.supported_session.is_committed(self.store.as_ref()) {
                        Ok(committed) => committed,
                        Err(error) => {
                            let mut runtime = self.lock_runtime()?;
                            runtime.cached = None;
                            runtime.state = state_after_failure(error);
                            return Err(error);
                        }
                    };
                    if !committed {
                        return Err(NativeSessionError::NoSession);
                    }

                    let mut runtime = self.lock_runtime()?;
                    if runtime.retired {
                        return Err(NativeSessionError::Cancelled);
                    }
                    if !matches!(&runtime.operation, SessionOperation::Idle) {
                        continue;
                    }
                    let flight = runtime.next_flight;
                    runtime.next_flight = runtime.next_flight.checked_add(1).unwrap_or(1);
                    let flight_cancellation = OperationCancellation::default();
                    runtime.operation = SessionOperation::Refreshing {
                        flight,
                        cancellation: flight_cancellation.clone(),
                    };
                    runtime.state = NativeSessionState::Refreshing;
                    runtime.completed_flight = None;
                    drop(runtime);
                    let result = self.finish_refresh(flight, self.refresh(&flight_cancellation));
                    if cancellation.is_cancelled() {
                        return Err(NativeSessionError::Cancelled);
                    }
                    return result;
                }
                SessionOperation::Refreshing { flight, .. } => {
                    observed_flight = Some(*flight);
                    drop(self.wait_for_idle(runtime, cancellation)?);
                }
                SessionOperation::Establishing => {
                    drop(self.wait_for_idle(runtime, cancellation)?);
                }
            }
        }
    }

    fn refresh(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<CachedAccessToken, NativeSessionError> {
        ensure_not_cancelled(cancellation)?;
        let entry = self.supported_session.refresh_entry();
        let current_refresh_material = match self.store.load(&entry) {
            Ok(Some(refresh_material)) => refresh_material,
            Ok(None) => {
                self.invalidate_supported_session();
                return Err(NativeSessionError::NoSession);
            }
            Err(error) => {
                let error = map_credential_error(error);
                if error == NativeSessionError::ReauthenticationRequired {
                    self.invalidate_supported_session();
                }
                return Err(error);
            }
        };
        ensure_not_cancelled(cancellation)?;

        let refresh = match self.exchange_with_retry(&current_refresh_material, cancellation) {
            Ok(refresh) => refresh,
            Err(NativeSessionError::ReauthenticationRequired) => {
                self.invalidate_supported_session();
                return Err(NativeSessionError::ReauthenticationRequired);
            }
            Err(error) => return Err(error),
        };

        ensure_not_cancelled(cancellation)?;
        if !self.supported_session.is_committed(self.store.as_ref())? {
            return Err(NativeSessionError::NoSession);
        }
        let SessionRefresh {
            access_token,
            refresh_material: rotated_refresh_material,
            valid_for,
        } = refresh;
        let expires_at = expiration(self.clock.now(), valid_for)?;

        if let Some(rotated_refresh_material) = rotated_refresh_material {
            self.store
                .replace(&entry, &rotated_refresh_material)
                .map_err(map_credential_error)?;
            ensure_not_cancelled(cancellation)?;
        }
        drop(current_refresh_material);

        Ok(CachedAccessToken {
            access_token,
            expires_at,
        })
    }

    fn invalidate_supported_session(&self) {
        if SupportedSession::load_committed(self.store.as_ref())
            .ok()
            .flatten()
            .as_ref()
            == Some(&self.supported_session)
        {
            let _ = self.ensure_entry_absent(&active_session_entry());
        }
        let _ = self.ensure_entry_absent(&self.supported_session.refresh_entry());
    }

    fn exchange_with_retry(
        &self,
        refresh_material: &SecretBytes,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRefresh, NativeSessionError> {
        for attempt in 0..MAX_REFRESH_ATTEMPTS {
            ensure_not_cancelled(cancellation)?;
            match self.refresher.refresh(refresh_material, cancellation) {
                Ok(refresh) => {
                    ensure_not_cancelled(cancellation)?;
                    return Ok(refresh);
                }
                Err(error)
                    if error.code() == NativeTransportErrorCode::Unavailable
                        && attempt.saturating_add(1) < MAX_REFRESH_ATTEMPTS =>
                {
                    ensure_not_cancelled(cancellation)?;
                }
                Err(error) => return Err(map_transport_error(error)),
            }
        }
        Err(NativeSessionError::Unavailable)
    }

    fn finish_refresh(
        &self,
        flight: u64,
        result: Result<CachedAccessToken, NativeSessionError>,
    ) -> Result<AccessToken, NativeSessionError> {
        let mut runtime = self.lock_runtime()?;
        runtime.operation = SessionOperation::Idle;
        let result = match result {
            Ok(cached) => {
                let access_token = cached.access_token.clone();
                runtime.cached = Some(cached);
                runtime.state = NativeSessionState::Active;
                runtime.completed_flight = Some(CompletedFlight {
                    id: flight,
                    error: None,
                });
                Ok(access_token)
            }
            Err(error) => {
                runtime.cached = None;
                runtime.state = state_after_failure(error);
                runtime.completed_flight = Some(CompletedFlight {
                    id: flight,
                    error: Some(error),
                });
                Err(error)
            }
        };
        self.wake.notify_all();
        result
    }

    fn lock_runtime(&self) -> Result<MutexGuard<'_, SessionRuntime>, NativeSessionError> {
        self.runtime
            .lock()
            .map_err(|_| NativeSessionError::Unavailable)
    }

    fn wait_for_idle<'a>(
        &self,
        mut runtime: MutexGuard<'a, SessionRuntime>,
        cancellation: &OperationCancellation,
    ) -> Result<MutexGuard<'a, SessionRuntime>, NativeSessionError> {
        while !matches!(&runtime.operation, SessionOperation::Idle) {
            ensure_not_cancelled(cancellation)?;
            let (next_runtime, _) = self
                .wake
                .wait_timeout(runtime, SINGLE_FLIGHT_WAIT)
                .map_err(|_| NativeSessionError::Unavailable)?;
            runtime = next_runtime;
        }
        ensure_not_cancelled(cancellation)?;
        Ok(runtime)
    }
}

impl<S: CredentialStore + ?Sized, R, C> fmt::Debug for NativeSessionManager<S, R, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeSessionManager")
            .field("supported_session", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl<S, R, C> SessionAuthenticator for NativeSessionManager<S, R, C>
where
    S: CredentialStore + ?Sized,
    R: SessionRefresher,
    C: SessionClock,
{
    fn access_token(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeTransportError> {
        self.acquire_access_token(cancellation)
            .map_err(map_session_error)
    }
}

#[derive(Clone, Default)]
struct DesktopSessionRefresher {
    delegate: Arc<RwLock<Option<Arc<dyn SessionRefresher>>>>,
}

impl DesktopSessionRefresher {
    fn install(&self, refresher: Arc<dyn SessionRefresher>) -> Result<(), NativeSessionError> {
        *self
            .delegate
            .write()
            .map_err(|_| NativeSessionError::Unavailable)? = Some(refresher);
        Ok(())
    }

    fn is_installed(&self) -> Result<bool, NativeSessionError> {
        self.delegate
            .read()
            .map(|delegate| delegate.is_some())
            .map_err(|_| NativeSessionError::Unavailable)
    }
}

impl SessionRefresher for DesktopSessionRefresher {
    fn refresh(
        &self,
        refresh_material: &SecretBytes,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRefresh, NativeTransportError> {
        let refresher = self
            .delegate
            .read()
            .map_err(|_| NativeTransportError::new(NativeTransportErrorCode::Unavailable))?
            .clone()
            .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unavailable))?;
        refresher.refresh(refresh_material, cancellation)
    }
}

type ManagedDesktopSession =
    NativeSessionManager<dyn CredentialStore, DesktopSessionRefresher, SystemSessionClock>;

/// Owns the platform session store and active manager for the desktop process.
///
/// The service has no Tauri command surface. A native Cognito implementation
/// installs the validated refresh boundary before restoration can succeed.
pub struct DesktopSessionService {
    store: Option<Arc<dyn CredentialStore>>,
    refresher: DesktopSessionRefresher,
    transition: Mutex<()>,
    runtime: Mutex<DesktopSessionServiceRuntime>,
}

#[derive(Default)]
struct DesktopSessionServiceRuntime {
    manager: Option<Arc<ManagedDesktopSession>>,
    startup_restore_pending: bool,
}

impl DesktopSessionService {
    /// Opens the platform credential store without selecting a weaker fallback.
    pub fn new() -> Self {
        let store = PlatformCredentialStore::new()
            .ok()
            .map(|store| Arc::new(store) as Arc<dyn CredentialStore>);
        Self {
            store,
            refresher: DesktopSessionRefresher::default(),
            transition: Mutex::new(()),
            runtime: Mutex::new(DesktopSessionServiceRuntime::default()),
        }
    }

    /// Installs the native refresh implementation and completes any deferred startup restore.
    pub fn install_refresher(
        &self,
        refresher: Arc<dyn SessionRefresher>,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRestore, NativeSessionError> {
        let _transition = self
            .transition
            .lock()
            .map_err(|_| NativeSessionError::Unavailable)?;
        self.refresher.install(refresher)?;
        let (manager, pending) = {
            let runtime = self
                .runtime
                .lock()
                .map_err(|_| NativeSessionError::Unavailable)?;
            (runtime.manager.clone(), runtime.startup_restore_pending)
        };
        let Some(manager) = manager else {
            return Ok(SessionRestore::Absent);
        };
        if !pending {
            return Ok(if manager.state() == NativeSessionState::Active {
                SessionRestore::Restored
            } else {
                SessionRestore::Absent
            });
        }
        self.complete_startup_restore(manager, cancellation)
    }

    /// Restores the last fully committed supported session during desktop startup.
    pub fn restore_on_startup(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRestore, NativeSessionError> {
        let _transition = self
            .transition
            .lock()
            .map_err(|_| NativeSessionError::Unavailable)?;
        ensure_not_cancelled(cancellation)?;
        let store = self.store.clone().ok_or(NativeSessionError::Unavailable)?;
        let Some(supported_session) = SupportedSession::load_committed(store.as_ref())? else {
            *self
                .runtime
                .lock()
                .map_err(|_| NativeSessionError::Unavailable)? =
                DesktopSessionServiceRuntime::default();
            return Ok(SessionRestore::Absent);
        };
        let manager = Arc::new(NativeSessionManager::new(
            store,
            supported_session,
            self.refresher.clone(),
        ));
        {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| NativeSessionError::Unavailable)?;
            runtime.manager = Some(Arc::clone(&manager));
            runtime.startup_restore_pending = true;
        }
        if !self.refresher.is_installed()? {
            return Ok(SessionRestore::Deferred);
        }
        self.complete_startup_restore(manager, cancellation)
    }

    /// Commits a validated native session and makes it the process session.
    pub fn establish(
        &self,
        supported_session: SupportedSession,
        grant: SessionGrant,
        cancellation: &OperationCancellation,
    ) -> Result<(), NativeSessionError> {
        let _transition = self
            .transition
            .lock()
            .map_err(|_| NativeSessionError::Unavailable)?;
        ensure_not_cancelled(cancellation)?;
        let store = self.store.clone().ok_or(NativeSessionError::Unavailable)?;
        let manager = Arc::new(NativeSessionManager::new(
            store,
            supported_session,
            self.refresher.clone(),
        ));
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| NativeSessionError::Unavailable)?;
        if let Some(current) = &runtime.manager {
            current.cancel_active_refresh_and_wait()?;
        }
        ensure_not_cancelled(cancellation)?;
        *runtime = DesktopSessionServiceRuntime::default();
        manager.establish(grant, cancellation)?;
        runtime.manager = Some(manager);
        runtime.startup_restore_pending = false;
        Ok(())
    }

    /// Returns the redacted state of the process session.
    pub fn state(&self) -> NativeSessionState {
        self.runtime
            .lock()
            .ok()
            .and_then(|runtime| runtime.manager.clone())
            .map_or(NativeSessionState::Empty, |manager| manager.state())
    }

    /// Cancels the manager-owned refresh flight during a native safety transition.
    pub fn cancel_active_refresh(&self) {
        if let Some(manager) = self
            .runtime
            .lock()
            .ok()
            .and_then(|runtime| runtime.manager.clone())
        {
            manager.cancel_active_refresh();
        }
    }

    fn complete_startup_restore(
        &self,
        manager: Arc<ManagedDesktopSession>,
        cancellation: &OperationCancellation,
    ) -> Result<SessionRestore, NativeSessionError> {
        let result = manager.restore(cancellation);
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| NativeSessionError::Unavailable)?;
        if runtime
            .manager
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &manager))
        {
            runtime.startup_restore_pending = matches!(
                result,
                Err(NativeSessionError::Cancelled | NativeSessionError::Unavailable)
            );
        }
        result
    }

    #[cfg(test)]
    fn with_store(store: Arc<dyn CredentialStore>) -> Self {
        Self {
            store: Some(store),
            refresher: DesktopSessionRefresher::default(),
            transition: Mutex::new(()),
            runtime: Mutex::new(DesktopSessionServiceRuntime::default()),
        }
    }
}

impl Default for DesktopSessionService {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for DesktopSessionService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopSessionService")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl SessionAuthenticator for DesktopSessionService {
    fn access_token(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeTransportError> {
        let manager = self
            .runtime
            .lock()
            .map_err(|_| NativeTransportError::new(NativeTransportErrorCode::Unavailable))?
            .manager
            .clone()
            .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unauthenticated))?;
        manager.access_token(cancellation)
    }
}

struct CachedAccessToken {
    access_token: AccessToken,
    expires_at: Instant,
}

enum SessionOperation {
    Idle,
    Establishing,
    Refreshing {
        flight: u64,
        cancellation: OperationCancellation,
    },
}

#[derive(Clone, Copy)]
struct CompletedFlight {
    id: u64,
    error: Option<NativeSessionError>,
}

struct SessionRuntime {
    state: NativeSessionState,
    cached: Option<CachedAccessToken>,
    operation: SessionOperation,
    retired: bool,
    next_flight: u64,
    completed_flight: Option<CompletedFlight>,
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self {
            state: NativeSessionState::Empty,
            cached: None,
            operation: SessionOperation::Idle,
            retired: false,
            next_flight: 1,
            completed_flight: None,
        }
    }
}

fn validate_lifetime(valid_for: Duration) -> Result<(), NativeSessionError> {
    if valid_for.is_zero() || valid_for > MAX_ACCESS_TOKEN_LIFETIME {
        return Err(NativeSessionError::InvalidLifetime);
    }
    Ok(())
}

fn expiration(now: Instant, valid_for: Duration) -> Result<Instant, NativeSessionError> {
    now.checked_add(valid_for)
        .ok_or(NativeSessionError::InvalidLifetime)
}

fn ensure_not_cancelled(cancellation: &OperationCancellation) -> Result<(), NativeSessionError> {
    if cancellation.is_cancelled() {
        Err(NativeSessionError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_credential_error(error: CredentialStoreError) -> NativeSessionError {
    match error {
        CredentialStoreError::AccessDenied | CredentialStoreError::Unavailable => {
            NativeSessionError::Unavailable
        }
        CredentialStoreError::Corrupt | CredentialStoreError::InvalidSecret => {
            NativeSessionError::ReauthenticationRequired
        }
    }
}

fn map_transport_error(error: NativeTransportError) -> NativeSessionError {
    match error.code() {
        NativeTransportErrorCode::Cancelled => NativeSessionError::Cancelled,
        NativeTransportErrorCode::Unauthenticated | NativeTransportErrorCode::InvalidRequest => {
            NativeSessionError::ReauthenticationRequired
        }
        NativeTransportErrorCode::UnsupportedVersion
        | NativeTransportErrorCode::TooLarge
        | NativeTransportErrorCode::Unavailable => NativeSessionError::Unavailable,
    }
}

fn map_session_error(error: NativeSessionError) -> NativeTransportError {
    let code = match error {
        NativeSessionError::Cancelled => NativeTransportErrorCode::Cancelled,
        NativeSessionError::NoSession
        | NativeSessionError::InvalidLifetime
        | NativeSessionError::ReauthenticationRequired => NativeTransportErrorCode::Unauthenticated,
        NativeSessionError::Unavailable => NativeTransportErrorCode::Unavailable,
    };
    NativeTransportError::new(code)
}

fn state_after_failure(error: NativeSessionError) -> NativeSessionState {
    match error {
        NativeSessionError::ReauthenticationRequired => {
            NativeSessionState::ReauthenticationRequired
        }
        NativeSessionError::Cancelled
        | NativeSessionError::NoSession
        | NativeSessionError::InvalidLifetime
        | NativeSessionError::Unavailable => NativeSessionState::Empty,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc::{self, RecvTimeoutError},
        },
        thread,
        time::{Duration, Instant},
    };

    use cipher_native_transport::{
        AccessToken, NativeTransportError, NativeTransportErrorCode, OperationCancellation,
        SessionAuthenticator,
    };

    use crate::credential_store::{
        CredentialEntry, CredentialKind, CredentialMigration, CredentialScope, CredentialStore,
        CredentialStoreError, SecretBytes,
    };

    use super::{
        DesktopSessionService, MAX_ACCESS_TOKEN_LIFETIME, MAX_REFRESH_ATTEMPTS, NativeSessionError,
        NativeSessionManager, NativeSessionState, SessionClock, SessionGrant, SessionRefresh,
        SessionRefresher, SessionRestore, SupportedSession, SystemSessionClock,
        active_session_entry,
    };

    const ACCOUNT_REFERENCE: &str = "validated-cognito-subject";
    const OTHER_ACCOUNT_REFERENCE: &str = "another-validated-cognito-subject";
    const ORIGINAL_REFRESH: &[u8] = b"original-native-refresh-material";
    const ROTATED_REFRESH: &[u8] = b"rotated-native-refresh-material";
    const REPLACEMENT_REFRESH: &[u8] = b"replacement-native-refresh-material";
    const ACCESS_TOKEN: &str = "native-access-token";
    const REFRESHED_ACCESS_TOKEN: &str = "refreshed-native-access-token";
    const REPLACEMENT_ACCESS_TOKEN: &str = "replacement-native-access-token";

    #[derive(Default)]
    struct MemoryCredentialStore {
        values: Mutex<Vec<(CredentialEntry, Vec<u8>)>>,
        faults: Mutex<StoreFaults>,
        operations: Mutex<Vec<&'static str>>,
        replace_gate: Mutex<Option<Arc<ReplaceGate>>>,
    }

    #[derive(Default)]
    struct StoreFaults {
        load: Option<CredentialStoreError>,
        replace: VecDeque<CredentialStoreError>,
        replace_after_write: VecDeque<CredentialStoreError>,
        delete: VecDeque<(CredentialKind, CredentialStoreError)>,
        cancel_after_replace: Option<OperationCancellation>,
    }

    impl MemoryCredentialStore {
        fn seed(&self, entry: CredentialEntry, value: &[u8]) {
            let mut values = self.values.lock().unwrap();
            if let Some((_, existing)) =
                values.iter_mut().find(|(candidate, _)| *candidate == entry)
            {
                *existing = value.to_vec();
            } else {
                values.push((entry, value.to_vec()));
            }
        }

        fn value(&self, entry: &CredentialEntry) -> Option<Vec<u8>> {
            self.values
                .lock()
                .unwrap()
                .iter()
                .find(|(candidate, _)| candidate == entry)
                .map(|(_, value)| value.clone())
        }

        fn contains_value(&self, value: &[u8]) -> bool {
            self.values
                .lock()
                .unwrap()
                .iter()
                .any(|(_, stored)| stored.as_slice() == value)
        }

        fn fail_next_replace(&self, error: CredentialStoreError) {
            self.faults.lock().unwrap().replace.push_back(error);
        }

        fn fail_next_replace_after_write(&self, error: CredentialStoreError) {
            self.faults
                .lock()
                .unwrap()
                .replace_after_write
                .push_back(error);
        }

        fn fail_next_delete(&self, kind: CredentialKind, error: CredentialStoreError) {
            self.faults.lock().unwrap().delete.push_back((kind, error));
        }

        fn set_load_error(&self, error: CredentialStoreError) {
            self.faults.lock().unwrap().load = Some(error);
        }

        fn set_replace_gate(&self, gate: Arc<ReplaceGate>) {
            *self.replace_gate.lock().unwrap() = Some(gate);
        }

        fn cancel_after_next_replace(&self, cancellation: &OperationCancellation) {
            self.faults.lock().unwrap().cancel_after_replace = Some(cancellation.clone());
        }

        fn operation_count(&self, operation: &str) -> usize {
            self.operations
                .lock()
                .unwrap()
                .iter()
                .filter(|candidate| **candidate == operation)
                .count()
        }
    }

    impl CredentialStore for MemoryCredentialStore {
        fn load(
            &self,
            entry: &CredentialEntry,
        ) -> Result<Option<SecretBytes>, CredentialStoreError> {
            self.operations.lock().unwrap().push("load");
            if let Some(error) = self.faults.lock().unwrap().load {
                return Err(error);
            }
            Ok(self.value(entry).map(SecretBytes::new))
        }

        fn replace(
            &self,
            entry: &CredentialEntry,
            secret: &SecretBytes,
        ) -> Result<(), CredentialStoreError> {
            self.operations.lock().unwrap().push("replace");
            if let Some(gate) = self.replace_gate.lock().unwrap().clone() {
                gate.wait_until_released();
            }
            let mut faults = self.faults.lock().unwrap();
            if let Some(error) = faults.replace.pop_front() {
                return Err(error);
            }
            let cancel_after_replace = faults.cancel_after_replace.take();
            drop(faults);
            self.seed(entry.clone(), secret.as_bytes());
            if let Some(cancellation) = cancel_after_replace {
                cancellation.cancel();
            }
            if let Some(error) = self.faults.lock().unwrap().replace_after_write.pop_front() {
                return Err(error);
            }
            Ok(())
        }

        fn migrate(
            &self,
            _: &CredentialEntry,
        ) -> Result<CredentialMigration, CredentialStoreError> {
            Ok(CredentialMigration::NotFound)
        }

        fn delete(&self, entry: &CredentialEntry) -> Result<(), CredentialStoreError> {
            self.operations.lock().unwrap().push("delete");
            let mut faults = self.faults.lock().unwrap();
            if faults
                .delete
                .front()
                .is_some_and(|(kind, _)| *kind == entry.kind())
                && let Some((_, error)) = faults.delete.pop_front()
            {
                return Err(error);
            }
            drop(faults);
            self.values
                .lock()
                .unwrap()
                .retain(|(candidate, _)| candidate != entry);
            Ok(())
        }

        fn delete_scope(&self, scope: &CredentialScope) -> Result<(), CredentialStoreError> {
            for kind in [
                CredentialKind::RefreshMaterial,
                CredentialKind::LocalStateWrappingKey,
                CredentialKind::ActiveSessionScope,
            ] {
                self.delete(&scope.entry(kind))?;
            }
            Ok(())
        }
    }

    struct ReplaceGate {
        entered: Mutex<bool>,
        entered_wake: Condvar,
        released: Mutex<bool>,
        release_wake: Condvar,
    }

    impl ReplaceGate {
        fn new() -> Self {
            Self {
                entered: Mutex::new(false),
                entered_wake: Condvar::new(),
                released: Mutex::new(false),
                release_wake: Condvar::new(),
            }
        }

        fn wait_until_released(&self) {
            *self.entered.lock().unwrap() = true;
            self.entered_wake.notify_all();
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.release_wake.wait(released).unwrap();
            }
        }

        fn wait_until_entered(&self) {
            let mut entered = self.entered.lock().unwrap();
            while !*entered {
                let (next_entered, timeout) = self
                    .entered_wake
                    .wait_timeout(entered, Duration::from_secs(1))
                    .unwrap();
                assert!(!timeout.timed_out(), "refresh replacement did not begin");
                entered = next_entered;
            }
        }

        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.release_wake.notify_all();
        }
    }

    #[derive(Clone)]
    struct TestClock {
        inner: Arc<TestClockInner>,
    }

    struct TestClockInner {
        now: Mutex<Instant>,
        calls: Mutex<usize>,
        call_wake: Condvar,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                inner: Arc::new(TestClockInner {
                    now: Mutex::new(Instant::now()),
                    calls: Mutex::new(0),
                    call_wake: Condvar::new(),
                }),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.inner.now.lock().unwrap();
            *now = now.checked_add(duration).unwrap();
        }

        fn calls(&self) -> usize {
            *self.inner.calls.lock().unwrap()
        }

        fn wait_for_call_after(&self, previous: usize) {
            let mut calls = self.inner.calls.lock().unwrap();
            while *calls <= previous {
                let (next_calls, timeout) = self
                    .inner
                    .call_wake
                    .wait_timeout(calls, Duration::from_secs(5))
                    .unwrap();
                assert!(
                    !timeout.timed_out(),
                    "concurrent request did not reach the session cache"
                );
                calls = next_calls;
            }
        }
    }

    impl SessionClock for TestClock {
        fn now(&self) -> Instant {
            *self.inner.calls.lock().unwrap() += 1;
            self.inner.call_wake.notify_all();
            *self.inner.now.lock().unwrap()
        }
    }

    enum RefreshOutcome {
        Token {
            value: &'static str,
            valid_for: Duration,
        },
        Rotated {
            value: &'static str,
            refresh_material: &'static [u8],
            valid_for: Duration,
        },
        Error(NativeTransportErrorCode),
    }

    struct SequenceRefresher {
        outcomes: Mutex<VecDeque<RefreshOutcome>>,
        calls: AtomicUsize,
        observed_material: Mutex<Vec<Vec<u8>>>,
    }

    impl SequenceRefresher {
        fn new(outcomes: impl IntoIterator<Item = RefreshOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
                observed_material: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }

        fn observed_material(&self) -> Vec<Vec<u8>> {
            self.observed_material.lock().unwrap().clone()
        }
    }

    impl SessionRefresher for SequenceRefresher {
        fn refresh(
            &self,
            refresh_material: &SecretBytes,
            _: &OperationCancellation,
        ) -> Result<SessionRefresh, NativeTransportError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.observed_material
                .lock()
                .unwrap()
                .push(refresh_material.as_bytes().to_vec());
            match self.outcomes.lock().unwrap().pop_front().unwrap() {
                RefreshOutcome::Token { value, valid_for } => {
                    SessionRefresh::new(access_token(value), valid_for).map_err(map_test_error)
                }
                RefreshOutcome::Rotated {
                    value,
                    refresh_material,
                    valid_for,
                } => SessionRefresh::with_rotated_refresh_material(
                    access_token(value),
                    SecretBytes::new(refresh_material.to_vec()),
                    valid_for,
                )
                .map_err(map_test_error),
                RefreshOutcome::Error(code) => Err(NativeTransportError::new(code)),
            }
        }
    }

    #[derive(Clone)]
    struct BlockingRefresher {
        inner: Arc<BlockingRefresherInner>,
    }

    struct BlockingRefresherInner {
        calls: AtomicUsize,
        entered: Mutex<bool>,
        entered_wake: Condvar,
        released: Mutex<bool>,
        release_wake: Condvar,
        outcome: Mutex<Option<RefreshOutcome>>,
    }

    impl BlockingRefresher {
        fn new(outcome: RefreshOutcome) -> Self {
            Self {
                inner: Arc::new(BlockingRefresherInner {
                    calls: AtomicUsize::new(0),
                    entered: Mutex::new(false),
                    entered_wake: Condvar::new(),
                    released: Mutex::new(false),
                    release_wake: Condvar::new(),
                    outcome: Mutex::new(Some(outcome)),
                }),
            }
        }

        fn calls(&self) -> usize {
            self.inner.calls.load(Ordering::Acquire)
        }

        fn wait_until_entered(&self) {
            let mut entered = self.inner.entered.lock().unwrap();
            while !*entered {
                let (next_entered, timeout) = self
                    .inner
                    .entered_wake
                    .wait_timeout(entered, Duration::from_secs(1))
                    .unwrap();
                assert!(!timeout.timed_out(), "refresh exchange did not begin");
                entered = next_entered;
            }
        }

        fn release(&self) {
            *self.inner.released.lock().unwrap() = true;
            self.inner.release_wake.notify_all();
        }
    }

    impl SessionRefresher for BlockingRefresher {
        fn refresh(
            &self,
            _: &SecretBytes,
            _: &OperationCancellation,
        ) -> Result<SessionRefresh, NativeTransportError> {
            self.inner.calls.fetch_add(1, Ordering::AcqRel);
            *self.inner.entered.lock().unwrap() = true;
            self.inner.entered_wake.notify_all();
            let mut released = self.inner.released.lock().unwrap();
            while !*released {
                released = self.inner.release_wake.wait(released).unwrap();
            }
            match self.inner.outcome.lock().unwrap().take().unwrap() {
                RefreshOutcome::Token { value, valid_for } => {
                    SessionRefresh::new(access_token(value), valid_for).map_err(map_test_error)
                }
                RefreshOutcome::Rotated {
                    value,
                    refresh_material,
                    valid_for,
                } => SessionRefresh::with_rotated_refresh_material(
                    access_token(value),
                    SecretBytes::new(refresh_material.to_vec()),
                    valid_for,
                )
                .map_err(map_test_error),
                RefreshOutcome::Error(code) => Err(NativeTransportError::new(code)),
            }
        }
    }

    struct CancellingRefresher {
        calls: AtomicUsize,
    }

    impl SessionRefresher for CancellingRefresher {
        fn refresh(
            &self,
            _: &SecretBytes,
            cancellation: &OperationCancellation,
        ) -> Result<SessionRefresh, NativeTransportError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            cancellation.cancel();
            SessionRefresh::with_rotated_refresh_material(
                access_token(REFRESHED_ACCESS_TOKEN),
                SecretBytes::new(ROTATED_REFRESH.to_vec()),
                Duration::from_secs(60),
            )
            .map_err(map_test_error)
        }
    }

    fn access_token(value: &str) -> AccessToken {
        AccessToken::new(value.into()).unwrap()
    }

    fn map_test_error(error: NativeSessionError) -> NativeTransportError {
        match error {
            NativeSessionError::Cancelled => {
                NativeTransportError::new(NativeTransportErrorCode::Cancelled)
            }
            NativeSessionError::NoSession
            | NativeSessionError::InvalidLifetime
            | NativeSessionError::ReauthenticationRequired => {
                NativeTransportError::new(NativeTransportErrorCode::Unauthenticated)
            }
            NativeSessionError::Unavailable => {
                NativeTransportError::new(NativeTransportErrorCode::Unavailable)
            }
        }
    }

    fn supported_session() -> SupportedSession {
        SupportedSession::new(ACCOUNT_REFERENCE).unwrap()
    }

    fn refresh_entry() -> CredentialEntry {
        supported_session().refresh_entry()
    }

    fn seed_committed_refresh(store: &MemoryCredentialStore) {
        store.seed(refresh_entry(), ORIGINAL_REFRESH);
        store.seed(active_session_entry(), supported_session().scope.digest());
    }

    fn grant(valid_for: Duration) -> SessionGrant {
        SessionGrant::new(
            access_token(ACCESS_TOKEN),
            SecretBytes::new(ORIGINAL_REFRESH.to_vec()),
            valid_for,
        )
        .unwrap()
    }

    fn replacement_grant(valid_for: Duration) -> SessionGrant {
        SessionGrant::new(
            access_token(REPLACEMENT_ACCESS_TOKEN),
            SecretBytes::new(REPLACEMENT_REFRESH.to_vec()),
            valid_for,
        )
        .unwrap()
    }

    #[test]
    fn supported_session_is_opaque_and_lifetime_values_are_bounded() {
        let supported = supported_session();
        assert!(!format!("{supported:?}").contains(ACCOUNT_REFERENCE));
        assert_eq!(
            SupportedSession::new(""),
            Err(crate::credential_store::CredentialScopeError::Empty)
        );

        assert_eq!(
            SessionGrant::new(
                access_token(ACCESS_TOKEN),
                SecretBytes::new(ORIGINAL_REFRESH.to_vec()),
                Duration::ZERO,
            )
            .unwrap_err(),
            NativeSessionError::InvalidLifetime
        );
        let grant = grant(Duration::from_secs(60));
        assert!(!format!("{grant:?}").contains(ACCESS_TOKEN));
        assert!(!format!("{grant:?}").contains("original-native-refresh-material"));

        let unchanged_refresh = SessionRefresh::new(
            access_token(REFRESHED_ACCESS_TOKEN),
            Duration::from_secs(60),
        )
        .unwrap();
        let unchanged_debug = format!("{unchanged_refresh:?}");
        assert!(unchanged_debug.contains("[unchanged]"));
        assert!(!unchanged_debug.contains(REFRESHED_ACCESS_TOKEN));

        let rotated_refresh = SessionRefresh::with_rotated_refresh_material(
            access_token(REFRESHED_ACCESS_TOKEN),
            SecretBytes::new(ROTATED_REFRESH.to_vec()),
            Duration::from_secs(60),
        )
        .unwrap();
        let rotated_debug = format!("{rotated_refresh:?}");
        assert!(rotated_debug.contains("[redacted]"));
        assert!(!rotated_debug.contains(REFRESHED_ACCESS_TOKEN));
        assert!(!rotated_debug.contains("rotated-native-refresh-material"));

        let before = Instant::now();
        let current = SystemSessionClock.now();
        assert!(current >= before);

        for (error, message) in [
            (
                NativeSessionError::Cancelled,
                "The native session operation was cancelled.",
            ),
            (
                NativeSessionError::NoSession,
                "The native session requires sign-in.",
            ),
            (
                NativeSessionError::InvalidLifetime,
                "The native session lifetime is invalid.",
            ),
            (
                NativeSessionError::ReauthenticationRequired,
                "The native session requires sign-in.",
            ),
            (
                NativeSessionError::Unavailable,
                "The native session is unavailable.",
            ),
        ] {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn access_token_lifetime_accepts_the_configured_boundary() {
        assert_eq!(MAX_ACCESS_TOKEN_LIFETIME, Duration::from_secs(15 * 60));
        assert!(
            SessionGrant::new(
                access_token(ACCESS_TOKEN),
                SecretBytes::new(ORIGINAL_REFRESH.to_vec()),
                MAX_ACCESS_TOKEN_LIFETIME,
            )
            .is_ok()
        );
        assert!(
            SessionRefresh::new(
                access_token(REFRESHED_ACCESS_TOKEN),
                MAX_ACCESS_TOKEN_LIFETIME,
            )
            .is_ok()
        );
        assert!(
            SessionRefresh::with_rotated_refresh_material(
                access_token(REFRESHED_ACCESS_TOKEN),
                SecretBytes::new(ROTATED_REFRESH.to_vec()),
                MAX_ACCESS_TOKEN_LIFETIME,
            )
            .is_ok()
        );
    }

    #[test]
    fn access_token_lifetime_rejects_values_over_the_configured_boundary() {
        let over_limit = MAX_ACCESS_TOKEN_LIFETIME.saturating_add(Duration::from_secs(1));

        assert_eq!(
            SessionGrant::new(
                access_token(ACCESS_TOKEN),
                SecretBytes::new(ORIGINAL_REFRESH.to_vec()),
                over_limit,
            )
            .unwrap_err(),
            NativeSessionError::InvalidLifetime
        );
        assert_eq!(
            SessionRefresh::new(access_token(REFRESHED_ACCESS_TOKEN), over_limit).unwrap_err(),
            NativeSessionError::InvalidLifetime
        );
        assert_eq!(
            SessionRefresh::with_rotated_refresh_material(
                access_token(REFRESHED_ACCESS_TOKEN),
                SecretBytes::new(ROTATED_REFRESH.to_vec()),
                over_limit,
            )
            .unwrap_err(),
            NativeSessionError::InvalidLifetime
        );
    }

    #[test]
    fn establish_persists_refresh_material_before_activating_an_access_token() {
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = SequenceRefresher::new([]);
        let manager = NativeSessionManager::new(store.clone(), supported_session(), refresher);

        manager
            .establish(
                grant(Duration::from_secs(60)),
                &OperationCancellation::default(),
            )
            .unwrap();

        assert_eq!(
            store.value(&refresh_entry()),
            Some(ORIGINAL_REFRESH.to_vec())
        );
        assert_eq!(
            store.value(&active_session_entry()),
            Some(supported_session().scope.digest().to_vec())
        );
        assert!(!store.contains_value(ACCESS_TOKEN.as_bytes()));
        assert_eq!(store.operation_count("replace"), 2);
        assert_eq!(manager.state(), NativeSessionState::Active);
        assert!(
            manager
                .access_token(&OperationCancellation::default())
                .is_ok()
        );
    }

    #[test]
    fn failed_or_cancelled_establishment_never_activates_the_candidate_session() {
        let cancelled_before_write = OperationCancellation::default();
        cancelled_before_write.cancel();
        let untouched_store = Arc::new(MemoryCredentialStore::default());
        let untouched_manager = NativeSessionManager::new(
            untouched_store.clone(),
            supported_session(),
            SequenceRefresher::new([]),
        );
        assert_eq!(
            untouched_manager.establish(grant(Duration::from_secs(60)), &cancelled_before_write,),
            Err(NativeSessionError::Cancelled)
        );
        assert_eq!(untouched_store.operation_count("replace"), 0);
        assert_eq!(untouched_manager.state(), NativeSessionState::Empty);

        let unavailable_store = Arc::new(MemoryCredentialStore::default());
        unavailable_store.fail_next_replace(CredentialStoreError::Unavailable);
        let unavailable_manager = NativeSessionManager::new(
            unavailable_store.clone(),
            supported_session(),
            SequenceRefresher::new([]),
        );
        assert_eq!(
            unavailable_manager.establish(
                grant(Duration::from_secs(60)),
                &OperationCancellation::default(),
            ),
            Err(NativeSessionError::Unavailable)
        );
        assert_eq!(unavailable_store.value(&refresh_entry()), None);
        assert_eq!(unavailable_manager.state(), NativeSessionState::Empty);

        let cancellation = OperationCancellation::default();
        let cleanup_store = Arc::new(MemoryCredentialStore::default());
        cleanup_store.cancel_after_next_replace(&cancellation);
        let cleanup_manager = NativeSessionManager::new(
            cleanup_store.clone(),
            supported_session(),
            SequenceRefresher::new([]),
        );
        assert_eq!(
            cleanup_manager.establish(grant(Duration::from_secs(60)), &cancellation),
            Err(NativeSessionError::Cancelled)
        );
        assert_eq!(cleanup_store.value(&refresh_entry()), None);
        assert_eq!(cleanup_store.value(&active_session_entry()), None);
        assert!(cleanup_store.operation_count("delete") >= 2);
        assert_eq!(cleanup_manager.state(), NativeSessionState::Empty);
    }

    #[test]
    fn partial_successful_establishment_write_is_removed_before_failure_returns() {
        let store = Arc::new(MemoryCredentialStore::default());
        store.fail_next_replace_after_write(CredentialStoreError::Unavailable);
        let service = DesktopSessionService::with_store(store.clone());

        assert_eq!(
            service.establish(
                supported_session(),
                grant(Duration::from_secs(60)),
                &OperationCancellation::default(),
            ),
            Err(NativeSessionError::Unavailable)
        );
        assert_eq!(store.value(&refresh_entry()), None);
        assert_eq!(store.value(&active_session_entry()), None);
        assert_eq!(service.state(), NativeSessionState::Empty);
    }

    #[test]
    fn cancelled_establishment_retries_failed_refresh_cleanup() {
        let cancellation = OperationCancellation::default();
        let store = Arc::new(MemoryCredentialStore::default());
        let gate = Arc::new(ReplaceGate::new());
        store.set_replace_gate(Arc::clone(&gate));
        store.cancel_after_next_replace(&cancellation);
        let manager = Arc::new(NativeSessionManager::new(
            store.clone(),
            supported_session(),
            SequenceRefresher::new([]),
        ));
        let establishment = {
            let manager = Arc::clone(&manager);
            let cancellation = cancellation.clone();
            thread::spawn(move || manager.establish(grant(Duration::from_secs(60)), &cancellation))
        };

        gate.wait_until_entered();
        store.fail_next_delete(
            CredentialKind::RefreshMaterial,
            CredentialStoreError::Unavailable,
        );
        gate.release();

        assert_eq!(
            establishment.join().unwrap(),
            Err(NativeSessionError::Cancelled)
        );
        assert_eq!(store.value(&refresh_entry()), None);
        assert_eq!(store.value(&active_session_entry()), None);
        assert!(store.operation_count("delete") >= 4);
        assert_eq!(manager.state(), NativeSessionState::Empty);
    }

    #[test]
    fn restore_only_uses_the_supported_session_scope() {
        let store = Arc::new(MemoryCredentialStore::default());
        seed_committed_refresh(&store);
        let refresher = SequenceRefresher::new([RefreshOutcome::Token {
            value: REFRESHED_ACCESS_TOKEN,
            valid_for: Duration::from_secs(60),
        }]);
        let manager = NativeSessionManager::new(store.clone(), supported_session(), refresher);

        assert_eq!(
            manager.restore(&OperationCancellation::default()),
            Ok(SessionRestore::Restored)
        );
        assert_eq!(manager.state(), NativeSessionState::Active);

        let other_refresher = SequenceRefresher::new([]);
        let other_manager = NativeSessionManager::new(
            store,
            SupportedSession::new(OTHER_ACCOUNT_REFERENCE).unwrap(),
            other_refresher,
        );
        assert_eq!(
            other_manager.restore(&OperationCancellation::default()),
            Ok(SessionRestore::Absent)
        );
        assert_eq!(other_manager.state(), NativeSessionState::Empty);
    }

    #[test]
    fn desktop_startup_reconstructs_and_restores_the_committed_session() {
        let store = Arc::new(MemoryCredentialStore::default());
        let first_process = DesktopSessionService::with_store(store.clone());
        first_process
            .establish(
                supported_session(),
                grant(Duration::from_secs(60)),
                &OperationCancellation::default(),
            )
            .unwrap();
        drop(first_process);

        let restarted_process = DesktopSessionService::with_store(store.clone());
        assert_eq!(
            restarted_process.restore_on_startup(&OperationCancellation::default()),
            Ok(SessionRestore::Deferred)
        );
        assert_eq!(restarted_process.state(), NativeSessionState::Empty);

        assert_eq!(
            restarted_process.install_refresher(
                Arc::new(SequenceRefresher::new([RefreshOutcome::Token {
                    value: REFRESHED_ACCESS_TOKEN,
                    valid_for: Duration::from_secs(60),
                }])),
                &OperationCancellation::default(),
            ),
            Ok(SessionRestore::Restored)
        );
        assert_eq!(restarted_process.state(), NativeSessionState::Active);
        assert!(
            restarted_process
                .access_token(&OperationCancellation::default())
                .is_ok()
        );
        assert!(!store.contains_value(REFRESHED_ACCESS_TOKEN.as_bytes()));
        assert!(!format!("{restarted_process:?}").contains(ACCOUNT_REFERENCE));
        assert!(!format!("{restarted_process:?}").contains(REFRESHED_ACCESS_TOKEN));
    }

    #[test]
    fn desktop_startup_without_a_committed_scope_stays_empty() {
        let service = DesktopSessionService::with_store(Arc::new(MemoryCredentialStore::default()));

        assert_eq!(
            service.restore_on_startup(&OperationCancellation::default()),
            Ok(SessionRestore::Absent)
        );
        assert_eq!(service.state(), NativeSessionState::Empty);
        assert_eq!(
            service
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unauthenticated
        );
    }

    #[test]
    fn desktop_service_construction_and_cancellation_stay_native_and_redacted() {
        let platform_service = DesktopSessionService::new();
        platform_service.cancel_active_refresh();
        assert!(!format!("{platform_service:?}").contains(ACCOUNT_REFERENCE));

        let default_service = DesktopSessionService::default();
        assert_eq!(default_service.state(), NativeSessionState::Empty);

        let store = Arc::new(MemoryCredentialStore::default());
        let active_service = DesktopSessionService::with_store(store.clone());
        active_service
            .establish(
                supported_session(),
                grant(Duration::from_secs(60)),
                &OperationCancellation::default(),
            )
            .unwrap();
        active_service.cancel_active_refresh();
        assert_eq!(active_service.state(), NativeSessionState::Active);
        assert_eq!(
            active_service.install_refresher(
                Arc::new(SequenceRefresher::new([])),
                &OperationCancellation::default(),
            ),
            Ok(SessionRestore::Restored)
        );
    }

    #[test]
    fn same_scope_reestablishment_waits_for_the_cancelled_refresh_flight() {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = Arc::new(DesktopSessionService::with_store(store.clone()));
        service
            .establish(
                supported_session(),
                grant(Duration::from_nanos(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        let refresher = BlockingRefresher::new(RefreshOutcome::Rotated {
            value: REFRESHED_ACCESS_TOKEN,
            refresh_material: ROTATED_REFRESH,
            valid_for: Duration::from_secs(60),
        });
        service
            .install_refresher(
                Arc::new(refresher.clone()),
                &OperationCancellation::default(),
            )
            .unwrap();

        let refresh = {
            let service = Arc::clone(&service);
            thread::spawn(move || service.access_token(&OperationCancellation::default()))
        };
        refresher.wait_until_entered();

        let (finished, completion) = mpsc::channel();
        let replacement = {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                let result = service.establish(
                    supported_session(),
                    replacement_grant(Duration::from_secs(60)),
                    &OperationCancellation::default(),
                );
                finished.send(result).unwrap();
            })
        };
        let early_completion = completion.recv_timeout(Duration::from_millis(50));
        let waited_for_refresh = matches!(&early_completion, Err(RecvTimeoutError::Timeout));
        refresher.release();
        let establishment = match early_completion {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                completion.recv_timeout(Duration::from_secs(1)).unwrap()
            }
            Err(RecvTimeoutError::Disconnected) => panic!("replacement thread disconnected"),
        };

        replacement.join().unwrap();
        assert_eq!(
            refresh.join().unwrap().unwrap_err().code(),
            NativeTransportErrorCode::Cancelled
        );
        assert_eq!(establishment, Ok(()));
        assert!(waited_for_refresh);
        assert_eq!(
            store.value(&refresh_entry()),
            Some(REPLACEMENT_REFRESH.to_vec())
        );
        assert!(!store.contains_value(ROTATED_REFRESH));
        assert_eq!(service.state(), NativeSessionState::Active);
    }

    #[test]
    fn retired_manager_clone_cannot_start_a_refresh_after_reestablishment() {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = DesktopSessionService::with_store(store.clone());
        service
            .establish(
                supported_session(),
                grant(Duration::from_nanos(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        let refresher = Arc::new(SequenceRefresher::new([RefreshOutcome::Rotated {
            value: REFRESHED_ACCESS_TOKEN,
            refresh_material: ROTATED_REFRESH,
            valid_for: Duration::from_secs(60),
        }]));
        service
            .install_refresher(refresher.clone(), &OperationCancellation::default())
            .unwrap();
        let stale_manager = service.runtime.lock().unwrap().manager.clone().unwrap();

        service
            .establish(
                supported_session(),
                replacement_grant(Duration::from_secs(60)),
                &OperationCancellation::default(),
            )
            .unwrap();

        assert_eq!(
            stale_manager
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Cancelled
        );
        assert_eq!(refresher.calls(), 0);
        assert_eq!(
            store.value(&refresh_entry()),
            Some(REPLACEMENT_REFRESH.to_vec())
        );
        assert!(!store.contains_value(ROTATED_REFRESH));
        assert_eq!(service.state(), NativeSessionState::Active);
    }

    #[test]
    fn deferred_startup_reauthentication_invalidates_the_committed_session() {
        let store = Arc::new(MemoryCredentialStore::default());
        seed_committed_refresh(&store);
        let service = DesktopSessionService::with_store(store.clone());
        assert_eq!(
            service.restore_on_startup(&OperationCancellation::default()),
            Ok(SessionRestore::Deferred)
        );

        assert_eq!(
            service.install_refresher(
                Arc::new(SequenceRefresher::new([RefreshOutcome::Error(
                    NativeTransportErrorCode::Unauthenticated,
                )])),
                &OperationCancellation::default(),
            ),
            Err(NativeSessionError::ReauthenticationRequired)
        );
        assert_eq!(
            service.state(),
            NativeSessionState::ReauthenticationRequired
        );
        assert_eq!(store.value(&refresh_entry()), None);
        assert_eq!(store.value(&active_session_entry()), None);
    }

    #[test]
    fn expired_access_tokens_refresh_without_reloading_before_expiry() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = SequenceRefresher::new([RefreshOutcome::Token {
            value: REFRESHED_ACCESS_TOKEN,
            valid_for: Duration::from_secs(60),
        }]);
        let manager =
            NativeSessionManager::with_clock(store, supported_session(), refresher, clock.clone());
        manager
            .establish(
                grant(Duration::from_secs(5)),
                &OperationCancellation::default(),
            )
            .unwrap();

        assert!(
            manager
                .access_token(&OperationCancellation::default())
                .is_ok()
        );
        assert_eq!(manager.refresher.calls(), 0);

        clock.advance(Duration::from_secs(5));
        assert_eq!(manager.state(), NativeSessionState::Empty);
        assert!(
            manager
                .access_token(&OperationCancellation::default())
                .is_ok()
        );
        assert_eq!(manager.refresher.calls(), 1);
        assert_eq!(manager.state(), NativeSessionState::Active);
        assert_eq!(
            manager.refresher.observed_material(),
            [ORIGINAL_REFRESH.to_vec()]
        );
    }

    #[test]
    fn concurrent_expiry_uses_one_refresh_flight() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = BlockingRefresher::new(RefreshOutcome::Token {
            value: REFRESHED_ACCESS_TOKEN,
            valid_for: Duration::from_secs(60),
        });
        let manager = Arc::new(NativeSessionManager::with_clock(
            store,
            supported_session(),
            refresher.clone(),
            clock.clone(),
        ));
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        clock.advance(Duration::from_secs(1));

        let leader = {
            let manager = Arc::clone(&manager);
            thread::spawn(move || manager.access_token(&OperationCancellation::default()))
        };
        refresher.wait_until_entered();

        let calls_before_follower = clock.calls();
        let follower = {
            let manager = Arc::clone(&manager);
            thread::spawn(move || manager.access_token(&OperationCancellation::default()))
        };
        clock.wait_for_call_after(calls_before_follower);
        assert_eq!(manager.state(), NativeSessionState::Refreshing);

        refresher.release();
        assert!(leader.join().unwrap().is_ok());
        assert!(follower.join().unwrap().is_ok());
        assert_eq!(refresher.calls(), 1);
        assert_eq!(manager.state(), NativeSessionState::Active);
    }

    #[test]
    fn cancelled_refresh_leader_does_not_cancel_a_live_follower() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = BlockingRefresher::new(RefreshOutcome::Token {
            value: REFRESHED_ACCESS_TOKEN,
            valid_for: Duration::from_secs(60),
        });
        let manager = Arc::new(NativeSessionManager::with_clock(
            store,
            supported_session(),
            refresher.clone(),
            clock.clone(),
        ));
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        clock.advance(Duration::from_secs(1));

        let leader_cancellation = OperationCancellation::default();
        let leader = {
            let manager = Arc::clone(&manager);
            let cancellation = leader_cancellation.clone();
            thread::spawn(move || manager.access_token(&cancellation))
        };
        refresher.wait_until_entered();

        let calls_before_follower = clock.calls();
        let follower = {
            let manager = Arc::clone(&manager);
            thread::spawn(move || manager.access_token(&OperationCancellation::default()))
        };
        clock.wait_for_call_after(calls_before_follower);
        leader_cancellation.cancel();
        refresher.release();

        assert_eq!(
            leader.join().unwrap().unwrap_err().code(),
            NativeTransportErrorCode::Cancelled
        );
        assert!(follower.join().unwrap().is_ok());
        assert_eq!(refresher.calls(), 1);
        assert_eq!(manager.state(), NativeSessionState::Active);
    }

    #[test]
    fn cancelled_waiter_leaves_the_single_refresh_flight_running_for_other_callers() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = BlockingRefresher::new(RefreshOutcome::Token {
            value: REFRESHED_ACCESS_TOKEN,
            valid_for: Duration::from_secs(60),
        });
        let manager = Arc::new(NativeSessionManager::with_clock(
            store,
            supported_session(),
            refresher.clone(),
            clock.clone(),
        ));
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        clock.advance(Duration::from_secs(1));

        let leader = {
            let manager = Arc::clone(&manager);
            thread::spawn(move || manager.access_token(&OperationCancellation::default()))
        };
        refresher.wait_until_entered();

        let cancellation = OperationCancellation::default();
        let calls_before_waiter = clock.calls();
        let waiter = {
            let manager = Arc::clone(&manager);
            let cancellation = cancellation.clone();
            thread::spawn(move || manager.access_token(&cancellation))
        };
        clock.wait_for_call_after(calls_before_waiter);
        cancellation.cancel();
        assert_eq!(
            waiter.join().unwrap().unwrap_err().code(),
            NativeTransportErrorCode::Cancelled
        );
        assert_eq!(refresher.calls(), 1);

        refresher.release();
        assert!(leader.join().unwrap().is_ok());
        assert_eq!(manager.state(), NativeSessionState::Active);
    }

    #[test]
    fn refresh_retries_only_the_bounded_unavailable_failures() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = SequenceRefresher::new([
            RefreshOutcome::Error(NativeTransportErrorCode::Unavailable),
            RefreshOutcome::Error(NativeTransportErrorCode::Unavailable),
            RefreshOutcome::Token {
                value: REFRESHED_ACCESS_TOKEN,
                valid_for: Duration::from_secs(60),
            },
        ]);
        let manager = NativeSessionManager::with_clock(
            store.clone(),
            supported_session(),
            refresher,
            clock.clone(),
        );
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        clock.advance(Duration::from_secs(1));

        assert_eq!(
            manager
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unavailable
        );
        assert_eq!(manager.refresher.calls(), usize::from(MAX_REFRESH_ATTEMPTS));
        assert_eq!(manager.state(), NativeSessionState::Empty);
        assert_eq!(
            store.value(&refresh_entry()),
            Some(ORIGINAL_REFRESH.to_vec())
        );
    }

    #[test]
    fn unavailable_refresh_preserves_native_material_for_a_later_attempt() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = SequenceRefresher::new([
            RefreshOutcome::Error(NativeTransportErrorCode::Unavailable),
            RefreshOutcome::Error(NativeTransportErrorCode::Unavailable),
            RefreshOutcome::Token {
                value: REFRESHED_ACCESS_TOKEN,
                valid_for: Duration::from_secs(60),
            },
        ]);
        let manager = NativeSessionManager::with_clock(
            store.clone(),
            supported_session(),
            refresher,
            clock.clone(),
        );
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        clock.advance(Duration::from_secs(1));

        assert_eq!(
            manager
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unavailable
        );
        assert_eq!(
            store.value(&refresh_entry()),
            Some(ORIGINAL_REFRESH.to_vec())
        );

        assert!(
            manager
                .access_token(&OperationCancellation::default())
                .is_ok()
        );
        assert_eq!(
            manager.refresher.calls(),
            usize::from(MAX_REFRESH_ATTEMPTS) + 1
        );
        assert_eq!(manager.state(), NativeSessionState::Active);
    }

    #[test]
    fn rotated_refresh_material_is_stored_before_the_candidate_access_token_activates() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = SequenceRefresher::new([RefreshOutcome::Rotated {
            value: REFRESHED_ACCESS_TOKEN,
            refresh_material: ROTATED_REFRESH,
            valid_for: Duration::from_secs(60),
        }]);
        let manager = Arc::new(NativeSessionManager::with_clock(
            store.clone(),
            supported_session(),
            refresher,
            clock.clone(),
        ));
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        let gate = Arc::new(ReplaceGate::new());
        store.set_replace_gate(Arc::clone(&gate));
        clock.advance(Duration::from_secs(1));

        let refresh = {
            let manager = Arc::clone(&manager);
            thread::spawn(move || manager.access_token(&OperationCancellation::default()))
        };
        gate.wait_until_entered();
        assert_eq!(manager.state(), NativeSessionState::Refreshing);
        assert_eq!(
            store.value(&refresh_entry()),
            Some(ORIGINAL_REFRESH.to_vec())
        );

        gate.release();
        assert!(refresh.join().unwrap().is_ok());
        assert_eq!(
            store.value(&refresh_entry()),
            Some(ROTATED_REFRESH.to_vec())
        );
        assert_eq!(manager.state(), NativeSessionState::Active);
    }

    #[test]
    fn failed_rotated_storage_replacement_never_activates_the_candidate_token() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = SequenceRefresher::new([
            RefreshOutcome::Rotated {
                value: REFRESHED_ACCESS_TOKEN,
                refresh_material: ROTATED_REFRESH,
                valid_for: Duration::from_secs(60),
            },
            RefreshOutcome::Error(NativeTransportErrorCode::Unavailable),
            RefreshOutcome::Error(NativeTransportErrorCode::Unavailable),
        ]);
        let manager = NativeSessionManager::with_clock(
            store.clone(),
            supported_session(),
            refresher,
            clock.clone(),
        );
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        store.fail_next_replace(CredentialStoreError::Unavailable);
        clock.advance(Duration::from_secs(1));

        assert_eq!(
            manager
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unavailable
        );
        assert_eq!(manager.state(), NativeSessionState::Empty);
        assert_eq!(
            store.value(&refresh_entry()),
            Some(ORIGINAL_REFRESH.to_vec())
        );

        assert_eq!(
            manager
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unavailable
        );
        assert_eq!(manager.refresher.calls(), 3);
    }

    #[test]
    fn cancellation_prevents_refresh_exchange_and_rotated_storage_mutation() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        let refresher = SequenceRefresher::new([RefreshOutcome::Token {
            value: REFRESHED_ACCESS_TOKEN,
            valid_for: Duration::from_secs(60),
        }]);
        let manager = NativeSessionManager::with_clock(
            store.clone(),
            supported_session(),
            refresher,
            clock.clone(),
        );
        manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        clock.advance(Duration::from_secs(1));

        let cancelled = OperationCancellation::default();
        cancelled.cancel();
        assert_eq!(
            manager.access_token(&cancelled).unwrap_err().code(),
            NativeTransportErrorCode::Cancelled
        );
        assert_eq!(manager.refresher.calls(), 0);
        assert_eq!(
            store.value(&refresh_entry()),
            Some(ORIGINAL_REFRESH.to_vec())
        );

        let cancellation = OperationCancellation::default();
        let cancelling_refresher = CancellingRefresher {
            calls: AtomicUsize::new(0),
        };
        let rotating_manager = NativeSessionManager::with_clock(
            store.clone(),
            supported_session(),
            cancelling_refresher,
            clock,
        );
        rotating_manager
            .establish(
                grant(Duration::from_secs(1)),
                &OperationCancellation::default(),
            )
            .unwrap();
        rotating_manager.clock.advance(Duration::from_secs(1));
        assert_eq!(
            rotating_manager
                .access_token(&cancellation)
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Cancelled
        );
        assert_eq!(
            store.value(&refresh_entry()),
            Some(ORIGINAL_REFRESH.to_vec())
        );
        assert_eq!(rotating_manager.state(), NativeSessionState::Empty);
    }

    #[test]
    fn invalid_or_revoked_refresh_material_requires_reauthentication() {
        let clock = TestClock::new();
        let store = Arc::new(MemoryCredentialStore::default());
        seed_committed_refresh(&store);
        let refresher = SequenceRefresher::new([RefreshOutcome::Error(
            NativeTransportErrorCode::Unauthenticated,
        )]);
        let manager =
            NativeSessionManager::with_clock(store.clone(), supported_session(), refresher, clock);

        assert_eq!(
            manager.restore(&OperationCancellation::default()),
            Err(NativeSessionError::ReauthenticationRequired)
        );
        assert_eq!(
            manager.state(),
            NativeSessionState::ReauthenticationRequired
        );
        assert_eq!(store.value(&refresh_entry()), None);
        assert_eq!(store.value(&active_session_entry()), None);
        assert!(store.operation_count("delete") >= 2);
        assert_eq!(manager.refresher.calls(), 1);
        assert_eq!(
            manager
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unauthenticated
        );
        assert_eq!(manager.refresher.calls(), 1);
    }

    #[test]
    fn missing_or_corrupt_native_material_never_claims_a_restored_session() {
        let missing_store = Arc::new(MemoryCredentialStore::default());
        let missing_manager = NativeSessionManager::new(
            missing_store,
            supported_session(),
            SequenceRefresher::new([]),
        );
        assert_eq!(
            missing_manager.restore(&OperationCancellation::default()),
            Ok(SessionRestore::Absent)
        );
        assert_eq!(missing_manager.state(), NativeSessionState::Empty);

        let stale_store = Arc::new(MemoryCredentialStore::default());
        stale_store.seed(active_session_entry(), supported_session().scope.digest());
        let stale_manager = NativeSessionManager::new(
            stale_store.clone(),
            supported_session(),
            SequenceRefresher::new([]),
        );
        assert_eq!(
            stale_manager.restore(&OperationCancellation::default()),
            Ok(SessionRestore::Absent)
        );
        assert_eq!(stale_store.value(&active_session_entry()), None);

        let corrupt_store = Arc::new(MemoryCredentialStore::default());
        seed_committed_refresh(&corrupt_store);
        corrupt_store.set_load_error(CredentialStoreError::Corrupt);
        let corrupt_manager = NativeSessionManager::new(
            corrupt_store.clone(),
            supported_session(),
            SequenceRefresher::new([]),
        );
        assert_eq!(
            corrupt_manager.restore(&OperationCancellation::default()),
            Err(NativeSessionError::ReauthenticationRequired)
        );
        assert_eq!(
            corrupt_manager.state(),
            NativeSessionState::ReauthenticationRequired
        );
        assert_eq!(corrupt_store.operation_count("delete"), 0);
    }

    #[test]
    fn session_types_do_not_expose_fallback_or_renderer_paths() {
        let manager = NativeSessionManager::new(
            Arc::new(MemoryCredentialStore::default()),
            supported_session(),
            SequenceRefresher::new([]),
        );
        assert!(!format!("{manager:?}").contains(ACCOUNT_REFERENCE));
        assert!(
            !format!("{:?}", NativeSessionError::Unavailable)
                .contains(std::str::from_utf8(ORIGINAL_REFRESH).unwrap())
        );

        let source = include_str!("session.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "tauri::command",
            "std::fs",
            "File::",
            "localStorage",
            "sessionStorage",
            "indexedDB",
            "WebView",
        ] {
            assert!(
                !production_source.contains(forbidden),
                "unexpected session path: {forbidden}"
            );
        }
    }
}
