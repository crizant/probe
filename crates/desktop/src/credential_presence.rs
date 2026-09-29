use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{credentials::CredentialId, execution::SecretPresenceReconciliation};

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
        self.revision = self.revision.wrapping_add(1);
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
    ) -> BTreeMap<String, String> {
        let mut cache = self.editor_identities.borrow_mut();
        if let Some(cached) = cache.as_ref()
            && cached.workspace == workspace
            && cached.environment == environment
            && &cached.names == names
        {
            return cached.keys.clone();
        }
        let keys = names
            .iter()
            .filter_map(|name| {
                CredentialId::for_workspace(workspace, environment, name)
                    .ok()
                    .map(|id| (name.clone(), id.persistence_key().to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        *cache = Some(EditorSecretIdentityCache {
            workspace: workspace.to_path_buf(),
            environment: environment.to_owned(),
            names: names.clone(),
            keys: keys.clone(),
        });
        keys
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
