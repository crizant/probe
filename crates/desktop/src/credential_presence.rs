use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    credentials::{CredentialId, CredentialStoreError},
    execution::SecretPresenceReconciliation,
};

#[derive(Clone, Debug, PartialEq)]
struct EditorSecretIdentityCache {
    workspace: PathBuf,
    environment: String,
    names: BTreeSet<String>,
    keys: BTreeMap<String, String>,
}

/// Presentation metadata only. Credential values and store access stay outside this state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct CredentialPresenceState {
    /// Opaque persistence keys learned to be present.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) stored_credentials: BTreeSet<String>,
    /// Opaque persistence keys learned to be absent.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) missing_credentials: BTreeSet<String>,
    #[serde(skip)]
    revision: u64,
    #[serde(skip)]
    editor_identities: RefCell<Option<EditorSecretIdentityCache>>,
}

impl PartialEq for CredentialPresenceState {
    fn eq(&self, other: &Self) -> bool {
        self.stored_credentials == other.stored_credentials
            && self.missing_credentials == other.missing_credentials
    }
}

impl CredentialPresenceState {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn is_stored(&self, key: &str) -> bool {
        self.stored_credentials.contains(key)
    }

    pub(crate) fn is_missing(&self, key: &str) -> bool {
        self.missing_credentials.contains(key)
    }

    /// A successful native write or delete supersedes older execution observations.
    pub(crate) fn record_write(&mut self, key: &str, stored: bool) -> bool {
        let changed = self.record(key, stored);
        self.revision = self
            .revision
            .checked_add(1)
            .expect("credential presence revision overflow");
        changed
    }

    fn record(&mut self, key: &str, stored: bool) -> bool {
        if stored {
            self.stored_credentials.insert(key.to_owned()) | self.missing_credentials.remove(key)
        } else {
            self.stored_credentials.remove(key) | self.missing_credentials.insert(key.to_owned())
        }
    }

    /// Ignores execution observations captured before a completed native write.
    pub(crate) fn reconcile(
        &mut self,
        result: SecretPresenceReconciliation,
        revision: u64,
    ) -> bool {
        if revision < self.revision {
            return false;
        }
        let mut changed = false;
        for key in result.missing {
            changed |= self.record(&key, false);
        }
        for key in result.found {
            changed |= self.record(&key, true);
        }
        changed
    }

    pub(crate) fn persistence_keys(
        &self,
        workspace: &Path,
        environment: &str,
        names: &BTreeSet<String>,
    ) -> Result<BTreeMap<String, String>, CredentialStoreError> {
        let mut cache = self.editor_identities.borrow_mut();
        if let Some(cached) = cache.as_ref()
            && cached.workspace == workspace
            && cached.environment == environment
            && &cached.names == names
        {
            return Ok(cached.keys.clone());
        }
        let mut keys = BTreeMap::new();
        for name in names {
            let id = CredentialId::for_workspace(workspace, environment, name)?;
            keys.insert(name.clone(), id.persistence_key().to_owned());
        }
        *cache = Some(EditorSecretIdentityCache {
            workspace: workspace.to_path_buf(),
            environment: environment.to_owned(),
            names: names.clone(),
            keys: keys.clone(),
        });
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_supersedes_old_execution_result() {
        let mut state = CredentialPresenceState::default();
        let before = state.revision();
        assert!(state.record_write("v1-key", true));
        assert!(!state.reconcile(
            SecretPresenceReconciliation {
                missing: ["v1-key".into()].into(),
                found: BTreeSet::new()
            },
            before
        ));
        assert!(state.is_stored("v1-key"));
        assert!(!state.is_missing("v1-key"));
    }

    #[test]
    fn delete_supersedes_execution_that_found_the_old_value() {
        let mut state = CredentialPresenceState::default();
        assert!(state.record_write("v1-key", true));
        let captured = state.revision();
        assert!(state.record_write("v1-key", false));
        assert!(!state.reconcile(
            SecretPresenceReconciliation {
                missing: BTreeSet::new(),
                found: ["v1-key".into()].into()
            },
            captured
        ));
        assert!(state.is_missing("v1-key"));
        assert!(!state.is_stored("v1-key"));
    }

    #[test]
    fn execution_after_the_latest_write_is_applied() {
        let mut state = CredentialPresenceState::default();
        assert!(state.record_write("v1-key", true));
        assert!(state.reconcile(
            SecretPresenceReconciliation {
                missing: ["v1-key".into()].into(),
                found: BTreeSet::new()
            },
            state.revision()
        ));
        assert!(state.is_missing("v1-key"));
    }

    #[test]
    fn repeated_writes_report_no_change_but_still_advance_the_revision() {
        let mut state = CredentialPresenceState::default();
        assert!(state.record_write("v1-key", true));
        let before = state.revision();
        assert!(!state.record_write("v1-key", true));
        assert!(state.revision() > before);
    }

    #[test]
    fn persistence_keys_follow_workspace_environment_and_names() {
        let state = CredentialPresenceState::default();
        let workspace = std::env::temp_dir();
        let names: BTreeSet<String> = ["token".into()].into();
        let key = |workspace: &Path, environment: &str| {
            CredentialId::for_workspace(workspace, environment, "token")
                .unwrap()
                .persistence_key()
                .to_owned()
        };

        let development = state
            .persistence_keys(&workspace, "development", &names)
            .unwrap();
        assert_eq!(development["token"], key(&workspace, "development"));
        let base = state.persistence_keys(&workspace, "base", &names).unwrap();
        assert_eq!(base["token"], key(&workspace, "base"));
        let other = state
            .persistence_keys(Path::new("/"), "base", &names)
            .unwrap();
        assert_eq!(other["token"], key(Path::new("/"), "base"));
        let renamed: BTreeSet<String> = ["other".into()].into();
        assert!(
            !state
                .persistence_keys(Path::new("/"), "base", &renamed)
                .unwrap()
                .contains_key("token")
        );
    }

    #[test]
    fn persistence_keys_returns_invalid_identity_instead_of_dropping_names() {
        let state = CredentialPresenceState::default();
        let workspace = std::env::temp_dir();
        let names: BTreeSet<String> = ["token".into(), String::new()].into();
        assert_eq!(
            state.persistence_keys(&workspace, "development", &names),
            Err(CredentialStoreError::InvalidIdentity)
        );
        let valid: BTreeSet<String> = ["token".into()].into();
        assert!(
            state
                .persistence_keys(&workspace, "development", &valid)
                .is_ok()
        );
    }

    #[test]
    fn execution_reconciliation_moves_identity_between_sets() {
        let mut state = CredentialPresenceState::default();
        assert!(state.reconcile(
            SecretPresenceReconciliation {
                missing: ["v1-key".into()].into(),
                found: BTreeSet::new()
            },
            0
        ));
        assert!(state.is_missing("v1-key"));
        assert!(state.reconcile(
            SecretPresenceReconciliation {
                missing: BTreeSet::new(),
                found: ["v1-key".into()].into()
            },
            0
        ));
        assert!(state.is_stored("v1-key"));
        assert!(!state.is_missing("v1-key"));
    }
}
