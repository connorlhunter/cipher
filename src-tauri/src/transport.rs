//! Credential-backed session authentication for native transport clients.
//!
//! The webview has no route to this module. It turns platform-stored refresh
//! material into a non-serializable access token only for Rust-owned HTTP and
//! realtime clients.

use std::{fmt, sync::Arc};

use cipher_native_transport::{
    AccessToken, NativeTransportError, NativeTransportErrorCode, OperationCancellation,
    SessionAuthenticator,
};

use crate::credential_store::{
    CredentialEntry, CredentialStore, CredentialStoreError, SecretBytes,
};

/// Exchanges refresh material through a native Rust authentication implementation.
pub trait RefreshMaterialExchanger: Send + Sync {
    /// Returns a short-lived native-only access token for valid refresh material.
    fn exchange(
        &self,
        refresh_material: &SecretBytes,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeTransportError>;
}

/// Supplies a native access token from platform-stored refresh material.
pub struct CredentialSessionAuthenticator<S: CredentialStore + ?Sized, E> {
    store: Arc<S>,
    refresh_entry: CredentialEntry,
    exchanger: E,
}

impl<S: CredentialStore + ?Sized, E> CredentialSessionAuthenticator<S, E> {
    /// Creates a native-only authenticator for one refresh-material entry.
    pub fn new(store: Arc<S>, refresh_entry: CredentialEntry, exchanger: E) -> Self {
        Self {
            store,
            refresh_entry,
            exchanger,
        }
    }
}

impl<S: CredentialStore + ?Sized, E> fmt::Debug for CredentialSessionAuthenticator<S, E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialSessionAuthenticator")
            .field("refresh_entry", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl<S, E> SessionAuthenticator for CredentialSessionAuthenticator<S, E>
where
    S: CredentialStore + ?Sized,
    E: RefreshMaterialExchanger,
{
    fn access_token(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<AccessToken, NativeTransportError> {
        ensure_not_cancelled(cancellation)?;
        let refresh_material = self
            .store
            .load(&self.refresh_entry)
            .map_err(map_credential_error)?
            .ok_or_else(|| NativeTransportError::new(NativeTransportErrorCode::Unauthenticated))?;
        ensure_not_cancelled(cancellation)?;
        let token = self.exchanger.exchange(&refresh_material, cancellation)?;
        ensure_not_cancelled(cancellation)?;
        Ok(token)
    }
}

fn ensure_not_cancelled(cancellation: &OperationCancellation) -> Result<(), NativeTransportError> {
    if cancellation.is_cancelled() {
        Err(NativeTransportError::new(
            NativeTransportErrorCode::Cancelled,
        ))
    } else {
        Ok(())
    }
}

fn map_credential_error(error: CredentialStoreError) -> NativeTransportError {
    let code = match error {
        CredentialStoreError::AccessDenied | CredentialStoreError::Unavailable => {
            NativeTransportErrorCode::Unavailable
        }
        CredentialStoreError::Corrupt | CredentialStoreError::InvalidSecret => {
            NativeTransportErrorCode::Unauthenticated
        }
    };
    NativeTransportError::new(code)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use cipher_native_transport::{
        AccessToken, NativeTransportError, NativeTransportErrorCode, OperationCancellation,
        SessionAuthenticator,
    };

    use crate::credential_store::{
        CredentialEntry, CredentialKind, CredentialMigration, CredentialScope, CredentialStore,
        CredentialStoreError, SecretBytes,
    };

    use super::{CredentialSessionAuthenticator, RefreshMaterialExchanger};

    const REFRESH_MATERIAL: &[u8] = b"native-refresh-material";
    const ACCESS_TOKEN: &str = "native-access-token";

    struct FixedStore {
        result: Result<Option<Vec<u8>>, CredentialStoreError>,
    }

    impl CredentialStore for FixedStore {
        fn load(&self, _: &CredentialEntry) -> Result<Option<SecretBytes>, CredentialStoreError> {
            self.result
                .as_ref()
                .map(|secret| secret.as_ref().map(|value| SecretBytes::new(value.clone())))
                .map_err(|error| *error)
        }

        fn replace(
            &self,
            _: &CredentialEntry,
            _: &SecretBytes,
        ) -> Result<(), CredentialStoreError> {
            unreachable!("transport authentication never writes refresh material")
        }

        fn migrate(
            &self,
            _: &CredentialEntry,
        ) -> Result<CredentialMigration, CredentialStoreError> {
            unreachable!("transport authentication never migrates refresh material")
        }

        fn delete(&self, _: &CredentialEntry) -> Result<(), CredentialStoreError> {
            unreachable!("transport authentication never deletes refresh material")
        }

        fn delete_scope(&self, _: &CredentialScope) -> Result<(), CredentialStoreError> {
            unreachable!("transport authentication never deletes refresh material")
        }
    }

    #[derive(Default)]
    struct FixedExchanger {
        calls: Mutex<u8>,
        cancel_after_exchange: bool,
    }

    impl RefreshMaterialExchanger for FixedExchanger {
        fn exchange(
            &self,
            refresh_material: &SecretBytes,
            cancellation: &OperationCancellation,
        ) -> Result<AccessToken, NativeTransportError> {
            assert_eq!(refresh_material.as_bytes(), REFRESH_MATERIAL);
            *self.calls.lock().unwrap() += 1;
            if self.cancel_after_exchange {
                cancellation.cancel();
            }
            AccessToken::new(ACCESS_TOKEN.into())
        }
    }

    fn refresh_entry() -> CredentialEntry {
        CredentialScope::new("transport-test-account")
            .unwrap()
            .entry(CredentialKind::RefreshMaterial)
    }

    #[test]
    fn access_tokens_are_derived_in_native_rust_from_platform_refresh_material() {
        let exchanger = FixedExchanger::default();
        let authenticator = CredentialSessionAuthenticator::new(
            Arc::new(FixedStore {
                result: Ok(Some(REFRESH_MATERIAL.to_vec())),
            }),
            refresh_entry(),
            exchanger,
        );

        let token = authenticator
            .access_token(&OperationCancellation::default())
            .unwrap();
        assert!(!format!("{token:?}").contains(ACCESS_TOKEN));
        assert!(!format!("{authenticator:?}").contains("transport-test-account"));
        assert!(
            !format!("{authenticator:?}").contains(std::str::from_utf8(REFRESH_MATERIAL).unwrap())
        );
    }

    #[test]
    fn missing_or_invalid_native_refresh_material_never_has_a_fallback() {
        let missing = CredentialSessionAuthenticator::new(
            Arc::new(FixedStore { result: Ok(None) }),
            refresh_entry(),
            FixedExchanger::default(),
        );
        assert_eq!(
            missing
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Unauthenticated
        );

        for (error, expected) in [
            (
                CredentialStoreError::AccessDenied,
                NativeTransportErrorCode::Unavailable,
            ),
            (
                CredentialStoreError::Unavailable,
                NativeTransportErrorCode::Unavailable,
            ),
            (
                CredentialStoreError::Corrupt,
                NativeTransportErrorCode::Unauthenticated,
            ),
            (
                CredentialStoreError::InvalidSecret,
                NativeTransportErrorCode::Unauthenticated,
            ),
        ] {
            let unavailable = CredentialSessionAuthenticator::new(
                Arc::new(FixedStore { result: Err(error) }),
                refresh_entry(),
                FixedExchanger::default(),
            );
            assert_eq!(
                unavailable
                    .access_token(&OperationCancellation::default())
                    .unwrap_err()
                    .code(),
                expected
            );
        }
    }

    #[test]
    fn cancellation_blocks_native_refresh_exchange_before_and_after_use() {
        let cancellation = OperationCancellation::default();
        cancellation.cancel();
        let cancelled = CredentialSessionAuthenticator::new(
            Arc::new(FixedStore {
                result: Ok(Some(REFRESH_MATERIAL.to_vec())),
            }),
            refresh_entry(),
            FixedExchanger::default(),
        );
        assert_eq!(
            cancelled.access_token(&cancellation).unwrap_err().code(),
            NativeTransportErrorCode::Cancelled
        );

        let after_exchange = CredentialSessionAuthenticator::new(
            Arc::new(FixedStore {
                result: Ok(Some(REFRESH_MATERIAL.to_vec())),
            }),
            refresh_entry(),
            FixedExchanger {
                calls: Mutex::new(0),
                cancel_after_exchange: true,
            },
        );
        assert_eq!(
            after_exchange
                .access_token(&OperationCancellation::default())
                .unwrap_err()
                .code(),
            NativeTransportErrorCode::Cancelled
        );
    }
}
