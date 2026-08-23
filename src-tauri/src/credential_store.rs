//! Platform-owned secret storage for the Cipher desktop core.
//!
//! This module deliberately has no Tauri command surface. The webview cannot
//! read, write, or name credential-store entries. On supported platforms it
//! uses only the native credential store; it does not provide a file, memory,
//! browser, or WebView fallback.

use std::{fmt, sync::Mutex};

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const SERVICE_NAME: &str = "me.connorhunter.cipher";
const SCOPE_DOMAIN_SEPARATOR: &[u8] = b"cipher credential scope v1\0";
const RECORD_MAGIC: &[u8; 8] = b"CIPHER\0\0";
const CURRENT_SCHEMA_VERSION: u8 = 1;
const LEGACY_SCHEMA_VERSION: u8 = 0;
const RECORD_HEADER_BYTES: usize = RECORD_MAGIC.len() + 1 + 1 + 4;
const MAX_SCOPE_REFERENCE_BYTES: usize = 512;
const MAX_REFRESH_MATERIAL_BYTES: usize = 8 * 1024;
const SESSION_SCOPE_BYTES: usize = 32;

/// The required byte length of a local-state wrapping key.
pub const LOCAL_STATE_WRAPPING_KEY_BYTES: usize = 32;

/// Selects the native credential type held for one local account scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialKind {
    /// Opaque long-lived material used to obtain a new short-lived session.
    RefreshMaterial,
    /// The 256-bit key that wraps local encrypted state for one account scope.
    LocalStateWrappingKey,
    /// The opaque account scope for the last fully committed native session.
    ActiveSessionScope,
}

impl CredentialKind {
    const fn code(self) -> u8 {
        match self {
            Self::RefreshMaterial => 1,
            Self::LocalStateWrappingKey => 2,
            Self::ActiveSessionScope => 3,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::RefreshMaterial => "refresh",
            Self::LocalStateWrappingKey => "wrapping-key",
            Self::ActiveSessionScope => "active-session",
        }
    }

    fn validates(self, bytes: &[u8]) -> bool {
        match self {
            Self::RefreshMaterial => !bytes.is_empty() && bytes.len() <= MAX_REFRESH_MATERIAL_BYTES,
            Self::LocalStateWrappingKey => bytes.len() == LOCAL_STATE_WRAPPING_KEY_BYTES,
            Self::ActiveSessionScope => bytes.len() == SESSION_SCOPE_BYTES,
        }
    }
}

/// An opaque per-account namespace for credentials held by the desktop core.
///
/// The source reference is hashed immediately and is never used in native
/// credential-store labels. It must be a stable account or device reference,
/// never secret material.
#[derive(Clone, Eq, PartialEq)]
pub struct CredentialScope([u8; 32]);

impl CredentialScope {
    /// Creates a scope from a stable non-secret account or device reference.
    pub fn new(reference: &str) -> Result<Self, CredentialScopeError> {
        if reference.is_empty() {
            return Err(CredentialScopeError::Empty);
        }
        if reference.len() > MAX_SCOPE_REFERENCE_BYTES {
            return Err(CredentialScopeError::TooLong);
        }

        let mut hasher = Sha256::new();
        hasher.update(SCOPE_DOMAIN_SEPARATOR);
        hasher.update(reference.as_bytes());
        Ok(Self(hasher.finalize().into()))
    }

    /// Selects one protected credential type within this scope.
    pub fn entry(&self, kind: CredentialKind) -> CredentialEntry {
        CredentialEntry {
            kind,
            scope: self.clone(),
        }
    }

    pub(crate) const fn from_digest(digest: [u8; SESSION_SCOPE_BYTES]) -> Self {
        Self(digest)
    }

    pub(crate) const fn digest(&self) -> &[u8; SESSION_SCOPE_BYTES] {
        &self.0
    }
}

impl fmt::Debug for CredentialScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialScope([redacted])")
    }
}

/// The reason a credential scope cannot be created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialScopeError {
    /// The stable account or device reference was absent.
    Empty,
    /// The stable account or device reference exceeded the bounded input size.
    TooLong,
}

impl fmt::Display for CredentialScopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("credential scope reference is required"),
            Self::TooLong => formatter.write_str("credential scope reference is too long"),
        }
    }
}

impl std::error::Error for CredentialScopeError {}

/// Selects one opaque credential value in a [`CredentialScope`].
#[derive(Clone, Eq, PartialEq)]
pub struct CredentialEntry {
    kind: CredentialKind,
    scope: CredentialScope,
}

impl CredentialEntry {
    /// Returns the type-specific rules that apply to this entry's value.
    pub const fn kind(&self) -> CredentialKind {
        self.kind
    }

    fn location(&self, schema_version: u8) -> NativeCredentialLocation {
        NativeCredentialLocation {
            target: format!(
                "cipher/{schema_version}/{}/{}",
                self.kind.label(),
                hex(&self.scope.0)
            ),
        }
    }

    fn current_location(&self) -> NativeCredentialLocation {
        self.location(CURRENT_SCHEMA_VERSION)
    }

    fn legacy_location(&self) -> NativeCredentialLocation {
        self.location(LEGACY_SCHEMA_VERSION)
    }
}

impl fmt::Debug for CredentialEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialEntry")
            .field("kind", &self.kind)
            .field("scope", &"[redacted]")
            .finish()
    }
}

/// A zeroizing byte value that is never rendered through [`fmt::Debug`] or
/// [`fmt::Display`].
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    /// Wraps secret bytes until they are stored, rotated, or consumed in Rust.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(Zeroizing::new(bytes.into()))
    }

    /// Borrows the opaque bytes for a Rust-owned operation.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl AsRef<[u8]> for SecretBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretBytes([redacted])")
    }
}

/// The selected native credential store for a constructed store instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStorePlatform {
    /// The current user's macOS Keychain.
    MacosKeychain,
    /// The current user's Windows Credential Manager.
    WindowsCredentialManager,
    /// No selected platform credential store is available.
    Unsupported,
}

/// A stable error category that never embeds secret values or platform output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStoreError {
    /// The native store denied access, is locked, or cannot be opened.
    AccessDenied,
    /// Stored bytes do not match Cipher's versioned record shape.
    Corrupt,
    /// The supplied value does not satisfy its entry's bounded requirements.
    InvalidSecret,
    /// The platform has no supported native credential store or an operation failed.
    Unavailable,
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied => formatter.write_str("credential store access is unavailable"),
            Self::Corrupt => formatter.write_str("credential store data is corrupt"),
            Self::InvalidSecret => formatter.write_str("credential value is invalid"),
            Self::Unavailable => formatter.write_str("credential store is unavailable"),
        }
    }
}

impl std::error::Error for CredentialStoreError {}

/// The result of a native-to-native credential migration attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialMigration {
    /// No legacy native credential exists for the requested entry.
    NotFound,
    /// A valid legacy native credential was copied into the current schema and removed.
    Migrated,
    /// A valid current record already existed; any remaining legacy item was removed.
    AlreadyCurrent,
}

/// Synchronous operations on the platform credential store.
///
/// Implementations keep values in native credential stores only. A missing
/// credential is represented as `Ok(None)`; malformed records are never
/// treated as missing or used as a fallback value.
pub trait CredentialStore: Send + Sync {
    /// Loads the current-schema credential for an entry.
    fn load(&self, entry: &CredentialEntry) -> Result<Option<SecretBytes>, CredentialStoreError>;

    /// Transactionally replaces the current-schema value and removes a matching legacy item.
    ///
    /// An error leaves the previous current value in place when rollback succeeds.
    fn replace(
        &self,
        entry: &CredentialEntry,
        secret: &SecretBytes,
    ) -> Result<(), CredentialStoreError>;

    /// Migrates only a validated native legacy entry; it never reads files or renderer storage.
    fn migrate(&self, entry: &CredentialEntry)
    -> Result<CredentialMigration, CredentialStoreError>;

    /// Idempotently removes the current and legacy native items for one entry.
    fn delete(&self, entry: &CredentialEntry) -> Result<(), CredentialStoreError>;

    /// Idempotently removes every credential kind, including any legacy native items.
    fn delete_scope(&self, scope: &CredentialScope) -> Result<(), CredentialStoreError>;
}

/// A concrete store that selects Keychain on macOS and Credential Manager on Windows.
pub struct PlatformCredentialStore {
    backend: Mutex<Box<dyn NativeCredentialBackend>>,
    platform: CredentialStorePlatform,
}

impl PlatformCredentialStore {
    /// Opens the selected platform's native credential store.
    ///
    /// This method returns [`CredentialStoreError::Unavailable`] rather than
    /// selecting a weaker store on unsupported platforms.
    pub fn new() -> Result<Self, CredentialStoreError> {
        let backend = platform::system_backend().map_err(map_native_error)?;
        let platform = backend.platform();
        Ok(Self {
            backend: Mutex::new(backend),
            platform,
        })
    }

    /// Returns the platform selected for this store instance.
    pub const fn platform(&self) -> CredentialStorePlatform {
        self.platform
    }

    fn with_backend<T>(
        &self,
        operation: impl FnOnce(&dyn NativeCredentialBackend) -> Result<T, CredentialStoreError>,
    ) -> Result<T, CredentialStoreError> {
        let backend = self
            .backend
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?;
        operation(backend.as_ref())
    }

    #[cfg(test)]
    fn with_test_backend(backend: impl NativeCredentialBackend + 'static) -> Self {
        let platform = backend.platform();
        Self {
            backend: Mutex::new(Box::new(backend)),
            platform,
        }
    }
}

impl CredentialStore for PlatformCredentialStore {
    fn load(&self, entry: &CredentialEntry) -> Result<Option<SecretBytes>, CredentialStoreError> {
        self.with_backend(|backend| load_current(backend, entry))
    }

    fn replace(
        &self,
        entry: &CredentialEntry,
        secret: &SecretBytes,
    ) -> Result<(), CredentialStoreError> {
        let record = encode_current_record(entry.kind, secret.as_bytes())?;
        self.with_backend(|backend| {
            let previous = load_location(backend, &entry.current_location())?;
            backend
                .replace(&entry.current_location(), record.as_slice())
                .map_err(map_native_error)?;
            if let Err(error) = delete_location(backend, &entry.legacy_location()) {
                rollback_current(backend, entry, previous.as_deref().map(Vec::as_slice))?;
                return Err(error);
            }
            Ok(())
        })
    }

    fn migrate(
        &self,
        entry: &CredentialEntry,
    ) -> Result<CredentialMigration, CredentialStoreError> {
        self.with_backend(|backend| {
            if load_current(backend, entry)?.is_some() {
                delete_location(backend, &entry.legacy_location())?;
                return Ok(CredentialMigration::AlreadyCurrent);
            }

            let Some(legacy) = load_legacy(backend, entry)? else {
                return Ok(CredentialMigration::NotFound);
            };
            let record = encode_current_record(entry.kind, legacy.as_bytes())?;
            backend
                .replace(&entry.current_location(), record.as_slice())
                .map_err(map_native_error)?;
            if let Err(error) = delete_location(backend, &entry.legacy_location()) {
                rollback_current(backend, entry, None)?;
                return Err(error);
            }
            Ok(CredentialMigration::Migrated)
        })
    }

    fn delete(&self, entry: &CredentialEntry) -> Result<(), CredentialStoreError> {
        self.with_backend(|backend| {
            let current_result = delete_location(backend, &entry.current_location());
            let legacy_result = delete_location(backend, &entry.legacy_location());
            current_result.and(legacy_result)
        })
    }

    fn delete_scope(&self, scope: &CredentialScope) -> Result<(), CredentialStoreError> {
        self.with_backend(|backend| {
            let mut first_error = None;
            for kind in [
                CredentialKind::RefreshMaterial,
                CredentialKind::LocalStateWrappingKey,
                CredentialKind::ActiveSessionScope,
            ] {
                let entry = scope.entry(kind);
                for location in [entry.current_location(), entry.legacy_location()] {
                    if let Err(error) = delete_location(backend, &location)
                        && first_error.is_none()
                    {
                        first_error = Some(error);
                    }
                }
            }
            first_error.map_or(Ok(()), Err)
        })
    }
}

impl fmt::Debug for PlatformCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlatformCredentialStore")
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct NativeCredentialLocation {
    target: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeCredentialError {
    AccessDenied,
    Corrupt,
    NotFound,
    Unavailable,
}

trait NativeCredentialBackend: Send {
    fn platform(&self) -> CredentialStorePlatform;
    fn get(&self, location: &NativeCredentialLocation) -> Result<Vec<u8>, NativeCredentialError>;
    fn replace(
        &self,
        location: &NativeCredentialLocation,
        value: &[u8],
    ) -> Result<(), NativeCredentialError>;
    fn delete(&self, location: &NativeCredentialLocation) -> Result<(), NativeCredentialError>;
}

fn map_native_error(error: NativeCredentialError) -> CredentialStoreError {
    match error {
        NativeCredentialError::AccessDenied => CredentialStoreError::AccessDenied,
        NativeCredentialError::Corrupt => CredentialStoreError::Corrupt,
        NativeCredentialError::NotFound | NativeCredentialError::Unavailable => {
            CredentialStoreError::Unavailable
        }
    }
}

fn load_current(
    backend: &dyn NativeCredentialBackend,
    entry: &CredentialEntry,
) -> Result<Option<SecretBytes>, CredentialStoreError> {
    let Some(record) = load_location(backend, &entry.current_location())? else {
        return Ok(None);
    };
    decode_current_record(entry.kind, record.as_slice()).map(Some)
}

fn load_legacy(
    backend: &dyn NativeCredentialBackend,
    entry: &CredentialEntry,
) -> Result<Option<SecretBytes>, CredentialStoreError> {
    let Some(value) = load_location(backend, &entry.legacy_location())? else {
        return Ok(None);
    };
    if !entry.kind.validates(value.as_slice()) {
        return Err(CredentialStoreError::Corrupt);
    }
    Ok(Some(SecretBytes(value)))
}

fn load_location(
    backend: &dyn NativeCredentialBackend,
    location: &NativeCredentialLocation,
) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialStoreError> {
    match backend.get(location) {
        Ok(value) => Ok(Some(Zeroizing::new(value))),
        Err(NativeCredentialError::NotFound) => Ok(None),
        Err(error) => Err(map_native_error(error)),
    }
}

fn delete_location(
    backend: &dyn NativeCredentialBackend,
    location: &NativeCredentialLocation,
) -> Result<(), CredentialStoreError> {
    match backend.delete(location) {
        Ok(()) | Err(NativeCredentialError::NotFound) => Ok(()),
        Err(error) => Err(map_native_error(error)),
    }
}

fn rollback_current(
    backend: &dyn NativeCredentialBackend,
    entry: &CredentialEntry,
    previous: Option<&[u8]>,
) -> Result<(), CredentialStoreError> {
    match previous {
        Some(previous) => backend
            .replace(&entry.current_location(), previous)
            .map_err(map_native_error),
        None => delete_location(backend, &entry.current_location()),
    }
}

fn encode_current_record(
    kind: CredentialKind,
    secret: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CredentialStoreError> {
    if !kind.validates(secret) {
        return Err(CredentialStoreError::InvalidSecret);
    }

    let length = u32::try_from(secret.len()).map_err(|_| CredentialStoreError::InvalidSecret)?;
    let mut record = Zeroizing::new(Vec::with_capacity(RECORD_HEADER_BYTES + secret.len()));
    record.extend_from_slice(RECORD_MAGIC);
    record.push(CURRENT_SCHEMA_VERSION);
    record.push(kind.code());
    record.extend_from_slice(&length.to_be_bytes());
    record.extend_from_slice(secret);
    Ok(record)
}

fn decode_current_record(
    expected_kind: CredentialKind,
    record: &[u8],
) -> Result<SecretBytes, CredentialStoreError> {
    if record.len() < RECORD_HEADER_BYTES || !record.starts_with(RECORD_MAGIC) {
        return Err(CredentialStoreError::Corrupt);
    }

    let schema_version = record[RECORD_MAGIC.len()];
    let kind = record[RECORD_MAGIC.len() + 1];
    let length_start = RECORD_MAGIC.len() + 2;
    let declared_length = u32::from_be_bytes(
        record[length_start..length_start + 4]
            .try_into()
            .map_err(|_| CredentialStoreError::Corrupt)?,
    ) as usize;
    let value = &record[RECORD_HEADER_BYTES..];

    if schema_version != CURRENT_SCHEMA_VERSION
        || kind != expected_kind.code()
        || declared_length != value.len()
        || !expected_kind.validates(value)
    {
        return Err(CredentialStoreError::Corrupt);
    }

    Ok(SecretBytes::new(value.to_vec()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}

mod platform {
    use super::{
        CredentialStorePlatform, NativeCredentialBackend, NativeCredentialError,
        NativeCredentialLocation, SERVICE_NAME,
    };

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn map_keyring_error(error: keyring_core::Error) -> NativeCredentialError {
        use zeroize::Zeroize;

        match error {
            keyring_core::Error::NoEntry => NativeCredentialError::NotFound,
            keyring_core::Error::NoStorageAccess(_) => NativeCredentialError::AccessDenied,
            keyring_core::Error::BadEncoding(mut value)
            | keyring_core::Error::BadDataFormat(mut value, _) => {
                value.zeroize();
                NativeCredentialError::Corrupt
            }
            _ => NativeCredentialError::Unavailable,
        }
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
    mod tests {
        use super::{NativeCredentialError, map_keyring_error};

        #[test]
        fn maps_keyring_errors_without_retaining_values() {
            assert_eq!(
                map_keyring_error(keyring_core::Error::NoEntry),
                NativeCredentialError::NotFound
            );
            assert_eq!(
                map_keyring_error(keyring_core::Error::BadEncoding(vec![7, 8, 9])),
                NativeCredentialError::Corrupt
            );
            assert_eq!(
                map_keyring_error(keyring_core::Error::BadDataFormat(
                    vec![7, 8, 9],
                    Box::new(std::io::Error::other("invalid native encoding")),
                )),
                NativeCredentialError::Corrupt
            );
            assert_eq!(
                map_keyring_error(keyring_core::Error::NoStorageAccess(Box::new(
                    std::io::Error::other("native access denied"),
                ))),
                NativeCredentialError::AccessDenied
            );
            assert_eq!(
                map_keyring_error(keyring_core::Error::NoDefaultStore),
                NativeCredentialError::Unavailable
            );
        }
    }

    #[cfg(target_os = "macos")]
    mod implementation {
        use std::sync::Arc;

        use apple_native_keyring_store::keychain::Store;
        use keyring_core::{Entry, api::CredentialStoreApi};

        use super::{
            CredentialStorePlatform, NativeCredentialBackend, NativeCredentialError,
            NativeCredentialLocation, SERVICE_NAME, map_keyring_error,
        };

        pub(super) struct SystemCredentialBackend {
            store: Arc<Store>,
        }

        impl SystemCredentialBackend {
            pub(super) fn new() -> Result<Self, NativeCredentialError> {
                Ok(Self {
                    store: Store::new().map_err(map_keyring_error)?,
                })
            }

            fn entry(
                &self,
                location: &NativeCredentialLocation,
            ) -> Result<Entry, NativeCredentialError> {
                self.store
                    .build(SERVICE_NAME, &location.target, None)
                    .map_err(map_keyring_error)
            }
        }

        impl NativeCredentialBackend for SystemCredentialBackend {
            fn platform(&self) -> CredentialStorePlatform {
                CredentialStorePlatform::MacosKeychain
            }

            fn get(
                &self,
                location: &NativeCredentialLocation,
            ) -> Result<Vec<u8>, NativeCredentialError> {
                self.entry(location)?
                    .get_secret()
                    .map_err(map_keyring_error)
            }

            fn replace(
                &self,
                location: &NativeCredentialLocation,
                value: &[u8],
            ) -> Result<(), NativeCredentialError> {
                self.entry(location)?
                    .set_secret(value)
                    .map_err(map_keyring_error)
            }

            fn delete(
                &self,
                location: &NativeCredentialLocation,
            ) -> Result<(), NativeCredentialError> {
                self.entry(location)?
                    .delete_credential()
                    .map_err(map_keyring_error)
            }
        }
    }

    #[cfg(target_os = "windows")]
    mod implementation {
        use std::{collections::HashMap, sync::Arc};

        use keyring_core::{Entry, api::CredentialStoreApi};
        use windows_native_keyring_store::Store;

        use super::{
            CredentialStorePlatform, NativeCredentialBackend, NativeCredentialError,
            NativeCredentialLocation, SERVICE_NAME, map_keyring_error,
        };

        pub(super) struct SystemCredentialBackend {
            store: Arc<Store>,
        }

        impl SystemCredentialBackend {
            pub(super) fn new() -> Result<Self, NativeCredentialError> {
                Ok(Self {
                    store: Store::new().map_err(map_keyring_error)?,
                })
            }

            fn entry(
                &self,
                location: &NativeCredentialLocation,
            ) -> Result<Entry, NativeCredentialError> {
                let modifiers = HashMap::from([
                    ("target", location.target.as_str()),
                    ("persistence", "Local"),
                ]);
                self.store
                    .build(SERVICE_NAME, &location.target, Some(&modifiers))
                    .map_err(map_keyring_error)
            }
        }

        impl NativeCredentialBackend for SystemCredentialBackend {
            fn platform(&self) -> CredentialStorePlatform {
                CredentialStorePlatform::WindowsCredentialManager
            }

            fn get(
                &self,
                location: &NativeCredentialLocation,
            ) -> Result<Vec<u8>, NativeCredentialError> {
                self.entry(location)?
                    .get_secret()
                    .map_err(map_keyring_error)
            }

            fn replace(
                &self,
                location: &NativeCredentialLocation,
                value: &[u8],
            ) -> Result<(), NativeCredentialError> {
                self.entry(location)?
                    .set_secret(value)
                    .map_err(map_keyring_error)
            }

            fn delete(
                &self,
                location: &NativeCredentialLocation,
            ) -> Result<(), NativeCredentialError> {
                self.entry(location)?
                    .delete_credential()
                    .map_err(map_keyring_error)
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    mod implementation {
        use super::{
            CredentialStorePlatform, NativeCredentialBackend, NativeCredentialError,
            NativeCredentialLocation,
        };

        pub(super) struct SystemCredentialBackend;

        impl SystemCredentialBackend {
            pub(super) fn new() -> Result<Self, NativeCredentialError> {
                Err(NativeCredentialError::Unavailable)
            }
        }

        impl NativeCredentialBackend for SystemCredentialBackend {
            fn platform(&self) -> CredentialStorePlatform {
                CredentialStorePlatform::Unsupported
            }

            fn get(&self, _: &NativeCredentialLocation) -> Result<Vec<u8>, NativeCredentialError> {
                Err(NativeCredentialError::Unavailable)
            }

            fn replace(
                &self,
                _: &NativeCredentialLocation,
                _: &[u8],
            ) -> Result<(), NativeCredentialError> {
                Err(NativeCredentialError::Unavailable)
            }

            fn delete(&self, _: &NativeCredentialLocation) -> Result<(), NativeCredentialError> {
                Err(NativeCredentialError::Unavailable)
            }
        }
    }

    pub(super) fn system_backend() -> Result<Box<dyn NativeCredentialBackend>, NativeCredentialError>
    {
        Ok(Box::new(implementation::SystemCredentialBackend::new()?))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        CURRENT_SCHEMA_VERSION, CredentialEntry, CredentialKind, CredentialMigration,
        CredentialScope, CredentialScopeError, CredentialStore, CredentialStoreError,
        CredentialStorePlatform, LOCAL_STATE_WRAPPING_KEY_BYTES, NativeCredentialBackend,
        NativeCredentialError, NativeCredentialLocation, PlatformCredentialStore,
        SESSION_SCOPE_BYTES, SecretBytes, decode_current_record, encode_current_record,
    };

    #[derive(Clone, Default)]
    struct MemoryCredentialBackend {
        state: Arc<Mutex<MemoryCredentialState>>,
    }

    #[derive(Default)]
    struct MemoryCredentialState {
        failure: Option<(MemoryOperation, NativeCredentialError)>,
        values: BTreeMap<String, Vec<u8>>,
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum MemoryOperation {
        Delete,
        Get,
        Replace,
    }

    impl MemoryCredentialBackend {
        fn seed(&self, location: NativeCredentialLocation, value: Vec<u8>) {
            self.state
                .lock()
                .unwrap()
                .values
                .insert(location.target, value);
        }

        fn value(&self, location: NativeCredentialLocation) -> Option<Vec<u8>> {
            self.state
                .lock()
                .unwrap()
                .values
                .get(&location.target)
                .cloned()
        }

        fn fail_once(&self, operation: MemoryOperation, error: NativeCredentialError) {
            self.state.lock().unwrap().failure = Some((operation, error));
        }

        fn take_failure(
            state: &mut MemoryCredentialState,
            operation: MemoryOperation,
        ) -> Option<NativeCredentialError> {
            match state.failure {
                Some((expected, error)) if expected == operation => {
                    state.failure = None;
                    Some(error)
                }
                _ => None,
            }
        }
    }

    impl NativeCredentialBackend for MemoryCredentialBackend {
        fn platform(&self) -> CredentialStorePlatform {
            CredentialStorePlatform::MacosKeychain
        }

        fn get(
            &self,
            location: &NativeCredentialLocation,
        ) -> Result<Vec<u8>, NativeCredentialError> {
            let mut state = self.state.lock().unwrap();
            if let Some(error) = Self::take_failure(&mut state, MemoryOperation::Get) {
                return Err(error);
            }
            state
                .values
                .get(&location.target)
                .cloned()
                .ok_or(NativeCredentialError::NotFound)
        }

        fn replace(
            &self,
            location: &NativeCredentialLocation,
            value: &[u8],
        ) -> Result<(), NativeCredentialError> {
            let mut state = self.state.lock().unwrap();
            if let Some(error) = Self::take_failure(&mut state, MemoryOperation::Replace) {
                return Err(error);
            }
            state.values.insert(location.target.clone(), value.to_vec());
            Ok(())
        }

        fn delete(&self, location: &NativeCredentialLocation) -> Result<(), NativeCredentialError> {
            let mut state = self.state.lock().unwrap();
            if let Some(error) = Self::take_failure(&mut state, MemoryOperation::Delete) {
                return Err(error);
            }
            state
                .values
                .remove(&location.target)
                .map(|_| ())
                .ok_or(NativeCredentialError::NotFound)
        }
    }

    fn test_store() -> (PlatformCredentialStore, MemoryCredentialBackend) {
        let backend = MemoryCredentialBackend::default();
        (
            PlatformCredentialStore::with_test_backend(backend.clone()),
            backend,
        )
    }

    fn test_entry(kind: CredentialKind) -> CredentialEntry {
        CredentialScope::new("test-account-reference")
            .unwrap()
            .entry(kind)
    }

    #[test]
    fn scopes_are_bounded_hashed_and_redacted() {
        assert_eq!(CredentialScope::new(""), Err(CredentialScopeError::Empty));
        assert_eq!(
            CredentialScope::new(&"x".repeat(513)),
            Err(CredentialScopeError::TooLong)
        );

        let scope = CredentialScope::new("personally-identifying-reference").unwrap();
        let entry = scope.entry(CredentialKind::RefreshMaterial);
        assert_eq!(entry.kind(), CredentialKind::RefreshMaterial);
        assert!(!format!("{scope:?}").contains("personally-identifying-reference"));
        assert!(!format!("{entry:?}").contains("personally-identifying-reference"));
        assert_ne!(
            entry.current_location().target,
            entry.legacy_location().target
        );
    }

    #[test]
    fn secret_values_and_errors_are_redacted() {
        let secret = SecretBytes::new(b"do-not-render-this-value".to_vec());
        assert_eq!(secret.as_bytes(), b"do-not-render-this-value");
        assert_eq!(secret.as_ref(), b"do-not-render-this-value");
        assert!(!format!("{secret:?}").contains("do-not-render-this-value"));
        assert!(!format!("{:?}", CredentialStoreError::Corrupt).contains("do-not-render"));
        assert_eq!(
            CredentialScopeError::Empty.to_string(),
            "credential scope reference is required"
        );
        assert_eq!(
            CredentialScopeError::TooLong.to_string(),
            "credential scope reference is too long"
        );
        assert_eq!(
            CredentialStoreError::AccessDenied.to_string(),
            "credential store access is unavailable"
        );
        assert_eq!(
            CredentialStoreError::Corrupt.to_string(),
            "credential store data is corrupt"
        );
        assert_eq!(
            CredentialStoreError::InvalidSecret.to_string(),
            "credential value is invalid"
        );
        assert_eq!(
            CredentialStoreError::Unavailable.to_string(),
            "credential store is unavailable"
        );
    }

    #[test]
    fn current_records_round_trip_and_do_not_store_raw_values() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        let secret = SecretBytes::new(b"opaque-refresh-material".to_vec());

        store.replace(&entry, &secret).unwrap();

        let stored = backend.value(entry.current_location()).unwrap();
        assert_ne!(stored, secret.as_bytes());
        assert_eq!(&stored[..8], b"CIPHER\0\0");
        assert_eq!(stored[8], CURRENT_SCHEMA_VERSION);
        assert_eq!(
            store.load(&entry).unwrap().unwrap().as_bytes(),
            secret.as_bytes()
        );
    }

    #[test]
    fn replace_enforces_type_specific_value_bounds() {
        let (store, _) = test_store();
        let refresh = test_entry(CredentialKind::RefreshMaterial);
        let wrapping_key = test_entry(CredentialKind::LocalStateWrappingKey);
        let active_session = test_entry(CredentialKind::ActiveSessionScope);

        assert_eq!(
            store.replace(&refresh, &SecretBytes::new(Vec::new())),
            Err(CredentialStoreError::InvalidSecret)
        );
        assert_eq!(
            store.replace(
                &wrapping_key,
                &SecretBytes::new(vec![0; LOCAL_STATE_WRAPPING_KEY_BYTES - 1])
            ),
            Err(CredentialStoreError::InvalidSecret)
        );
        store
            .replace(
                &wrapping_key,
                &SecretBytes::new(vec![0; LOCAL_STATE_WRAPPING_KEY_BYTES]),
            )
            .unwrap();
        assert_eq!(
            store.replace(
                &active_session,
                &SecretBytes::new(vec![0; SESSION_SCOPE_BYTES - 1])
            ),
            Err(CredentialStoreError::InvalidSecret)
        );
        store
            .replace(
                &active_session,
                &SecretBytes::new(vec![0; SESSION_SCOPE_BYTES]),
            )
            .unwrap();
    }

    #[test]
    fn corrupted_current_records_are_rejected_without_fallback() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        backend.seed(entry.current_location(), b"corrupt-but-not-secret".to_vec());
        backend.seed(entry.legacy_location(), b"valid-legacy-refresh".to_vec());

        assert_eq!(
            store.load(&entry).unwrap_err(),
            CredentialStoreError::Corrupt
        );
        assert_eq!(store.migrate(&entry), Err(CredentialStoreError::Corrupt));
        assert!(backend.value(entry.legacy_location()).is_some());
    }

    #[test]
    fn migration_copies_valid_native_legacy_values_before_removal() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        backend.seed(entry.legacy_location(), b"native-legacy-refresh".to_vec());

        assert_eq!(store.migrate(&entry), Ok(CredentialMigration::Migrated));
        assert_eq!(backend.value(entry.legacy_location()), None);
        assert_eq!(
            store.load(&entry).unwrap().unwrap().as_bytes(),
            b"native-legacy-refresh"
        );
    }

    #[test]
    fn migration_reports_when_no_legacy_native_item_exists() {
        let (store, _) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);

        assert_eq!(store.migrate(&entry), Ok(CredentialMigration::NotFound));
    }

    #[test]
    fn migration_preserves_legacy_value_when_the_current_write_fails() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        backend.seed(entry.legacy_location(), b"native-legacy-refresh".to_vec());
        backend.fail_once(
            MemoryOperation::Replace,
            NativeCredentialError::AccessDenied,
        );

        assert_eq!(
            store.migrate(&entry),
            Err(CredentialStoreError::AccessDenied)
        );
        assert_eq!(
            backend.value(entry.legacy_location()),
            Some(b"native-legacy-refresh".to_vec())
        );
        assert_eq!(backend.value(entry.current_location()), None);
    }

    #[test]
    fn migration_prefers_a_valid_current_record_and_cleans_up_legacy_data() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        store
            .replace(&entry, &SecretBytes::new(b"current-refresh".to_vec()))
            .unwrap();
        backend.seed(entry.legacy_location(), b"older-refresh".to_vec());

        assert_eq!(
            store.migrate(&entry),
            Ok(CredentialMigration::AlreadyCurrent)
        );
        assert_eq!(backend.value(entry.legacy_location()), None);
        assert_eq!(
            store.load(&entry).unwrap().unwrap().as_bytes(),
            b"current-refresh"
        );
    }

    #[test]
    fn invalid_legacy_values_are_preserved_for_explicit_recovery() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::LocalStateWrappingKey);
        backend.seed(
            entry.legacy_location(),
            vec![0; LOCAL_STATE_WRAPPING_KEY_BYTES - 1],
        );

        assert_eq!(store.migrate(&entry), Err(CredentialStoreError::Corrupt));
        assert!(backend.value(entry.legacy_location()).is_some());
        assert_eq!(backend.value(entry.current_location()), None);
    }

    #[test]
    fn replacement_rolls_back_when_legacy_cleanup_fails() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        store
            .replace(&entry, &SecretBytes::new(b"previous-refresh".to_vec()))
            .unwrap();
        backend.seed(entry.legacy_location(), b"older-refresh".to_vec());
        backend.fail_once(MemoryOperation::Delete, NativeCredentialError::AccessDenied);

        assert_eq!(
            store.replace(&entry, &SecretBytes::new(b"current-refresh".to_vec())),
            Err(CredentialStoreError::AccessDenied)
        );
        assert_eq!(
            store.load(&entry).unwrap().unwrap().as_bytes(),
            b"previous-refresh"
        );
        assert!(backend.value(entry.legacy_location()).is_some());
    }

    #[test]
    fn migration_rolls_back_current_data_when_legacy_cleanup_fails() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        backend.seed(entry.legacy_location(), b"older-refresh".to_vec());
        backend.fail_once(MemoryOperation::Delete, NativeCredentialError::AccessDenied);

        assert_eq!(
            store.migrate(&entry),
            Err(CredentialStoreError::AccessDenied)
        );
        assert_eq!(backend.value(entry.current_location()), None);
        assert_eq!(
            backend.value(entry.legacy_location()),
            Some(b"older-refresh".to_vec())
        );
    }

    #[test]
    fn deletion_is_idempotent_and_removes_current_and_legacy_entries() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        store
            .replace(&entry, &SecretBytes::new(b"current-refresh".to_vec()))
            .unwrap();
        backend.seed(entry.legacy_location(), b"older-refresh".to_vec());

        store.delete(&entry).unwrap();
        store.delete(&entry).unwrap();
        assert_eq!(backend.value(entry.current_location()), None);
        assert_eq!(backend.value(entry.legacy_location()), None);
    }

    #[test]
    fn deleting_a_scope_removes_every_credential_kind() {
        let (store, backend) = test_store();
        let scope = CredentialScope::new("scope-to-delete").unwrap();
        let refresh = scope.entry(CredentialKind::RefreshMaterial);
        let wrapping_key = scope.entry(CredentialKind::LocalStateWrappingKey);
        let active_session = scope.entry(CredentialKind::ActiveSessionScope);

        store
            .replace(&refresh, &SecretBytes::new(b"current-refresh".to_vec()))
            .unwrap();
        store
            .replace(
                &wrapping_key,
                &SecretBytes::new(vec![0; LOCAL_STATE_WRAPPING_KEY_BYTES]),
            )
            .unwrap();
        store
            .replace(
                &active_session,
                &SecretBytes::new(vec![0; SESSION_SCOPE_BYTES]),
            )
            .unwrap();
        backend.seed(refresh.legacy_location(), b"older-refresh".to_vec());
        backend.seed(
            wrapping_key.legacy_location(),
            vec![0; LOCAL_STATE_WRAPPING_KEY_BYTES],
        );

        store.delete_scope(&scope).unwrap();
        store.delete_scope(&scope).unwrap();
        for entry in [refresh, wrapping_key, active_session] {
            assert_eq!(backend.value(entry.current_location()), None);
            assert_eq!(backend.value(entry.legacy_location()), None);
        }
    }

    #[test]
    fn scope_deletion_attempts_remaining_entries_after_an_access_error() {
        let (store, backend) = test_store();
        let scope = CredentialScope::new("scope-with-delete-error").unwrap();
        let refresh = scope.entry(CredentialKind::RefreshMaterial);
        let wrapping_key = scope.entry(CredentialKind::LocalStateWrappingKey);
        backend.seed(refresh.current_location(), b"current-refresh".to_vec());
        backend.seed(
            wrapping_key.current_location(),
            encode_current_record(
                CredentialKind::LocalStateWrappingKey,
                &[0; LOCAL_STATE_WRAPPING_KEY_BYTES],
            )
            .unwrap()
            .to_vec(),
        );
        backend.fail_once(MemoryOperation::Delete, NativeCredentialError::AccessDenied);

        assert_eq!(
            store.delete_scope(&scope),
            Err(CredentialStoreError::AccessDenied)
        );
        assert!(backend.value(wrapping_key.current_location()).is_none());
    }

    #[test]
    fn native_access_failures_never_trigger_an_alternate_store() {
        let (store, backend) = test_store();
        let entry = test_entry(CredentialKind::RefreshMaterial);
        backend.fail_once(MemoryOperation::Get, NativeCredentialError::AccessDenied);

        assert_eq!(
            store.load(&entry).unwrap_err(),
            CredentialStoreError::AccessDenied
        );
        assert_eq!(backend.value(entry.current_location()), None);
    }

    #[test]
    fn encoded_records_reject_wrong_kind_version_length_and_value_shape() {
        let encoded =
            encode_current_record(CredentialKind::RefreshMaterial, b"valid-refresh-material")
                .unwrap();
        assert_eq!(
            decode_current_record(CredentialKind::RefreshMaterial, encoded.as_slice())
                .unwrap()
                .as_bytes(),
            b"valid-refresh-material"
        );

        let mut wrong_kind = encoded.to_vec();
        wrong_kind[9] = CredentialKind::LocalStateWrappingKey.code();
        assert_eq!(
            decode_current_record(CredentialKind::RefreshMaterial, &wrong_kind).unwrap_err(),
            CredentialStoreError::Corrupt
        );

        let mut wrong_version = encoded.to_vec();
        wrong_version[8] = CURRENT_SCHEMA_VERSION + 1;
        assert_eq!(
            decode_current_record(CredentialKind::RefreshMaterial, &wrong_version).unwrap_err(),
            CredentialStoreError::Corrupt
        );

        let mut wrong_length = encoded.to_vec();
        wrong_length[13] = wrong_length[13].saturating_add(1);
        assert_eq!(
            decode_current_record(CredentialKind::RefreshMaterial, &wrong_length).unwrap_err(),
            CredentialStoreError::Corrupt
        );
        assert_eq!(
            decode_current_record(CredentialKind::RefreshMaterial, b"bad").unwrap_err(),
            CredentialStoreError::Corrupt
        );
    }

    #[test]
    fn poisoned_store_lock_returns_a_safe_error() {
        let (store, _) = test_store();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = store.backend.lock().unwrap();
            panic!("intentional test-only lock poison");
        }));

        assert_eq!(
            store
                .load(&test_entry(CredentialKind::RefreshMaterial))
                .unwrap_err(),
            CredentialStoreError::Unavailable
        );
    }

    #[test]
    fn store_debug_output_exposes_only_the_platform() {
        let (store, _) = test_store();
        let debug = format!("{store:?}");
        assert!(debug.contains("PlatformCredentialStore"));
        assert!(debug.contains("MacosKeychain"));
    }

    #[test]
    fn native_error_mapping_preserves_only_safe_categories() {
        assert_eq!(
            super::map_native_error(NativeCredentialError::Corrupt),
            CredentialStoreError::Corrupt
        );
        assert_eq!(
            super::map_native_error(NativeCredentialError::Unavailable),
            CredentialStoreError::Unavailable
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn platform_store_round_trips_and_deletes_a_native_test_credential() {
        struct Cleanup<'store> {
            entry: CredentialEntry,
            store: &'store PlatformCredentialStore,
        }

        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = self.store.delete(&self.entry);
            }
        }

        let store = PlatformCredentialStore::new().unwrap();
        let platform = store.platform();
        #[cfg(target_os = "macos")]
        assert_eq!(platform, CredentialStorePlatform::MacosKeychain);
        #[cfg(target_os = "windows")]
        assert_eq!(platform, CredentialStorePlatform::WindowsCredentialManager);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let entry =
            CredentialScope::new(&format!("platform-test-{}-{timestamp}", std::process::id()))
                .unwrap()
                .entry(CredentialKind::RefreshMaterial);
        let _cleanup = Cleanup {
            entry: entry.clone(),
            store: &store,
        };
        let secret = SecretBytes::new(b"platform-store-test-value".to_vec());

        assert!(store.load(&entry).unwrap().is_none());
        store.replace(&entry, &secret).unwrap();
        assert_eq!(
            store.load(&entry).unwrap().unwrap().as_bytes(),
            secret.as_bytes()
        );
        store.delete(&entry).unwrap();
        assert!(store.load(&entry).unwrap().is_none());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn unsupported_platforms_do_not_select_a_weaker_store() {
        assert_eq!(
            PlatformCredentialStore::new().map(|store| store.platform()),
            Err(CredentialStoreError::Unavailable)
        );
    }
}
