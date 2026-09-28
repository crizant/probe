//! Native credential service. All methods are synchronous and must run off the GPUI thread.

use std::{fmt, path::Path};

use keyring::{Entry, Error as KeyringError};
use probe_core::{SecretContext, SecretError, SecretProvider, SecretValue};
use sha2::{Digest, Sha256};

const SERVICE: &str = "dev.probe.desktop.credentials.v1";

/// Opaque, versioned key derived from a canonical workspace path and effective names.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct CredentialId(String);

impl CredentialId {
    /// The path must refer to an existing workspace. Canonicalization avoids relative-path
    /// and symlink aliases. A moved workspace deliberately receives a new v1 identity.
    pub fn for_workspace(
        path: &Path,
        environment: &str,
        variable: &str,
    ) -> Result<Self, CredentialStoreError> {
        if environment.is_empty()
            || variable.is_empty()
            || environment.contains('\0')
            || variable.contains('\0')
        {
            return Err(CredentialStoreError::InvalidIdentity);
        }
        let path = path
            .canonicalize()
            .map_err(|_| CredentialStoreError::InvalidIdentity)?;
        let path = path.to_str().ok_or(CredentialStoreError::InvalidIdentity)?;
        let mut digest = Sha256::new();
        for component in [path, environment, variable] {
            digest.update((component.len() as u64).to_be_bytes());
            digest.update(component.as_bytes());
        }
        let account = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self(format!("v1-{account}")))
    }

    /// Opaque `v1-` identity persisted as non-secret presence metadata.
    ///
    /// This is the account string already derived by [`Self::for_workspace`].
    pub(crate) fn persistence_key(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CredentialId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialId([OPAQUE])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStatus {
    Stored,
    NotStored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStoreError {
    NotFound,
    Unavailable,
    AccessDenied,
    InvalidIdentity,
    Unsupported,
    BackendFailure,
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "credential not found",
            Self::Unavailable => "native credential store unavailable",
            Self::AccessDenied => "native credential store access denied",
            Self::InvalidIdentity => "invalid credential identity",
            Self::Unsupported => "native credential store unsupported",
            Self::BackendFailure => "native credential store failed",
        })
    }
}

impl std::error::Error for CredentialStoreError {}

pub trait CredentialStore: Send + Sync {
    fn status(&self, id: &CredentialId) -> Result<CredentialStatus, CredentialStoreError>;
    /// Creates or replaces a credential.
    fn set(&self, id: &CredentialId, value: &str) -> Result<(), CredentialStoreError>;
    /// Deleting a missing credential returns `NotFound`.
    fn delete(&self, id: &CredentialId) -> Result<(), CredentialStoreError>;
    fn get(&self, id: &CredentialId) -> Result<Option<SecretValue>, CredentialStoreError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeCredentialStore;

impl NativeCredentialStore {
    fn entry(id: &CredentialId) -> Result<Entry, CredentialStoreError> {
        Entry::new(SERVICE, id.persistence_key()).map_err(map_error)
    }
}

impl CredentialStore for NativeCredentialStore {
    fn status(&self, id: &CredentialId) -> Result<CredentialStatus, CredentialStoreError> {
        // keyring's portable v1 API has no existence-only call. Discard the value here.
        self.get(id).map(|value| {
            if value.is_some() {
                CredentialStatus::Stored
            } else {
                CredentialStatus::NotStored
            }
        })
    }

    fn set(&self, id: &CredentialId, value: &str) -> Result<(), CredentialStoreError> {
        Self::entry(id)?.set_password(value).map_err(map_error)
    }

    fn delete(&self, id: &CredentialId) -> Result<(), CredentialStoreError> {
        Self::entry(id)?.delete_credential().map_err(map_error)
    }

    fn get(&self, id: &CredentialId) -> Result<Option<SecretValue>, CredentialStoreError> {
        match Self::entry(id)?.get_password() {
            Ok(value) => Ok(Some(SecretValue::new(value))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }
}

fn map_error(error: KeyringError) -> CredentialStoreError {
    match error {
        KeyringError::NoEntry => CredentialStoreError::NotFound,
        KeyringError::NoDefaultStore => CredentialStoreError::Unavailable,
        KeyringError::NoStorageAccess(_) => CredentialStoreError::AccessDenied,
        KeyringError::Invalid(_, _) | KeyringError::TooLong(_, _) => {
            CredentialStoreError::InvalidIdentity
        }
        KeyringError::NotSupportedByStore(_) => CredentialStoreError::Unsupported,
        _ => CredentialStoreError::BackendFailure,
    }
}

/// Adapter from Probe's credential service to core's safe runtime provider boundary.
pub struct NativeSecretProvider<'a, S: CredentialStore + ?Sized> {
    pub store: &'a S,
    pub workspace: &'a Path,
}

impl<S: CredentialStore + ?Sized> SecretProvider for NativeSecretProvider<'_, S> {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<SecretValue>, SecretError> {
        let environment = context.environment_name.ok_or(SecretError)?;
        let id = CredentialId::for_workspace(self.workspace, environment, context.variable_name)
            .map_err(|_| SecretError)?;
        self.store.get(&id).map_err(|_| SecretError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct FakeStore(Mutex<HashMap<String, String>>);

    impl CredentialStore for FakeStore {
        fn status(&self, id: &CredentialId) -> Result<CredentialStatus, CredentialStoreError> {
            Ok(
                if self.0.lock().unwrap().contains_key(id.persistence_key()) {
                    CredentialStatus::Stored
                } else {
                    CredentialStatus::NotStored
                },
            )
        }
        fn set(&self, id: &CredentialId, value: &str) -> Result<(), CredentialStoreError> {
            self.0
                .lock()
                .unwrap()
                .insert(id.persistence_key().to_owned(), value.to_owned());
            Ok(())
        }
        fn delete(&self, id: &CredentialId) -> Result<(), CredentialStoreError> {
            self.0
                .lock()
                .unwrap()
                .remove(id.persistence_key())
                .map(|_| ())
                .ok_or(CredentialStoreError::NotFound)
        }
        fn get(&self, id: &CredentialId) -> Result<Option<SecretValue>, CredentialStoreError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(id.persistence_key())
                .cloned()
                .map(SecretValue::new))
        }
    }

    #[test]
    fn identity_and_lifecycle() {
        let root = std::env::temp_dir();
        let a = CredentialId::for_workspace(&root, "production", "token").unwrap();
        let again = CredentialId::for_workspace(&root, "production", "token").unwrap();
        let other_environment = CredentialId::for_workspace(&root, "staging", "token").unwrap();
        let other_variable = CredentialId::for_workspace(&root, "production", "other").unwrap();
        let other_workspace =
            CredentialId::for_workspace(Path::new("/"), "production", "token").unwrap();
        assert_eq!(a, again);
        assert_ne!(a, other_environment);
        assert_ne!(a, other_variable);
        assert_ne!(a, other_workspace);
        let secret = "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR";
        assert!(!format!("{a:?}").contains(secret));
        assert!(!a.persistence_key().contains(secret));
        assert!(a.persistence_key().starts_with("v1-"));
        let store = FakeStore::default();
        assert_eq!(store.status(&a), Ok(CredentialStatus::NotStored));
        assert!(store.get(&a).unwrap().is_none());
        store.set(&a, secret).unwrap();
        assert_eq!(store.status(&a), Ok(CredentialStatus::Stored));
        assert!(!format!("{:?}", store.get(&a).unwrap()).contains(secret));
        store.set(&a, "replacement").unwrap();
        assert_eq!(
            store.get(&a).unwrap(),
            Some(SecretValue::new("replacement".into()))
        );
        store.delete(&a).unwrap();
        assert_eq!(store.delete(&a), Err(CredentialStoreError::NotFound));
    }

    #[test]
    fn provider_maps_failures_without_diagnostics() {
        struct Broken;
        impl CredentialStore for Broken {
            fn status(&self, _: &CredentialId) -> Result<CredentialStatus, CredentialStoreError> {
                Err(CredentialStoreError::Unavailable)
            }
            fn set(&self, _: &CredentialId, _: &str) -> Result<(), CredentialStoreError> {
                Err(CredentialStoreError::Unavailable)
            }
            fn delete(&self, _: &CredentialId) -> Result<(), CredentialStoreError> {
                Err(CredentialStoreError::Unavailable)
            }
            fn get(&self, _: &CredentialId) -> Result<Option<SecretValue>, CredentialStoreError> {
                Err(CredentialStoreError::Unavailable)
            }
        }
        let provider = NativeSecretProvider {
            store: &Broken,
            workspace: &std::env::temp_dir(),
        };
        let result = provider.resolve_secret(&SecretContext {
            variable_name: "token",
            environment_name: Some("production"),
            workspace_identity: None,
        });
        assert!(matches!(result, Err(SecretError)));
    }

    #[test]
    fn provider_reads_a_stored_credential_through_probe_types() {
        let workspace = std::env::temp_dir();
        let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
        let store = FakeStore::default();
        store
            .set(&id, "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
            .unwrap();
        let provider = NativeSecretProvider {
            store: &store,
            workspace: &workspace,
        };
        let resolved = provider
            .resolve_secret(&SecretContext {
                variable_name: "token",
                environment_name: Some("production"),
                workspace_identity: Some("ignored raw CLI path"),
            })
            .unwrap();
        assert!(resolved.is_some());
        assert!(!format!("{resolved:?}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
        assert!(
            !format!("{:?}", CredentialStoreError::BackendFailure)
                .contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
        );
    }

    #[test]
    fn backend_errors_are_classified_without_formatting_backend_data() {
        let secret = "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR";
        let cases = [
            (KeyringError::NoEntry, CredentialStoreError::NotFound),
            (
                KeyringError::NoDefaultStore,
                CredentialStoreError::Unavailable,
            ),
            (
                KeyringError::NoStorageAccess(Box::new(std::io::Error::other(secret))),
                CredentialStoreError::AccessDenied,
            ),
            (
                KeyringError::Invalid("account".into(), secret.into()),
                CredentialStoreError::InvalidIdentity,
            ),
            (
                KeyringError::NotSupportedByStore(secret.into()),
                CredentialStoreError::Unsupported,
            ),
            (
                KeyringError::BadEncoding(secret.as_bytes().to_vec()),
                CredentialStoreError::BackendFailure,
            ),
        ];
        for (backend, expected) in cases {
            let mapped = map_error(backend);
            assert_eq!(mapped, expected);
            assert!(!format!("{mapped:?} {mapped}").contains(secret));
        }
    }
}
