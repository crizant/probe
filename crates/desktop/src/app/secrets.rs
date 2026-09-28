use std::{collections::BTreeMap, path::Path};

use super::*;
use crate::credentials::{CredentialId, CredentialStore, CredentialStoreError};
use crate::execution::SecretPresenceReconciliation;

/// In-memory `CredentialId` keys for the secrets currently classified by the editor.
/// Rebuilt when the workspace, environment, or secret names change. Never persisted.
pub(super) struct EditorSecretIdentityCache {
    workspace: PathBuf,
    environment: String,
    names: BTreeSet<String>,
    keys: BTreeMap<String, String>,
}

pub(super) fn default_credential_store() -> Arc<dyn CredentialStore> {
    #[cfg(test)]
    {
        Arc::new(TestCredentialStore)
    }
    #[cfg(not(test))]
    {
        Arc::new(crate::credentials::NativeCredentialStore)
    }
}

#[cfg(test)]
struct TestCredentialStore;

#[cfg(test)]
impl CredentialStore for TestCredentialStore {
    fn status(
        &self,
        _: &CredentialId,
    ) -> Result<crate::credentials::CredentialStatus, CredentialStoreError> {
        panic!("presentation must not query credential status")
    }
    fn set(&self, _: &CredentialId, _: &str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::Unsupported)
    }
    fn delete(&self, _: &CredentialId) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::NotFound)
    }
    fn get(
        &self,
        _: &CredentialId,
    ) -> Result<Option<probe_core::SecretValue>, CredentialStoreError> {
        panic!("presentation must not read credential values")
    }
}

pub(super) struct SecretValueDialog {
    pub(super) name: String,
    pub(super) environment: String,
    pub(super) replacing: bool,
    pub(super) input: gpui::Entity<InputState>,
    pub(super) busy: bool,
    pub(super) error: Option<&'static str>,
    pub(super) _subscription: gpui::Subscription,
}

impl ProbeApp {
    pub(super) fn can_manage_secret(&self, name: &str) -> bool {
        let Some(dialog) = &self.environment_manager_dialog else {
            return false;
        };
        if self.environment_save_task.is_some() || self.environment_manager_is_dirty() {
            return false;
        }
        let Some(loaded) = &self.loaded_workspace else {
            return false;
        };
        loaded.workspace().effective_environment_variables(&dialog.draft).iter().any(|row| {
            matches!(&row.variable, EnvironmentVariable::Secret(secret) if secret.name.as_deref() == Some(name))
        })
    }

    /// Fill Environment Manager labels from presence metadata. Does not touch the
    /// native credential store.
    pub(super) fn sync_secret_statuses_from_presence(&mut self) {
        let Some(dialog) = &self.environment_manager_dialog else {
            return;
        };
        let Some(path) = self.workspace_path.clone() else {
            return;
        };
        let Some(loaded) = &self.loaded_workspace else {
            return;
        };
        let environment = dialog.draft.name.clone();
        let names: Vec<String> = loaded
            .workspace()
            .effective_environment_variables(&dialog.draft)
            .into_iter()
            .filter_map(|row| match row.variable {
                EnvironmentVariable::Secret(secret) => secret.name,
                EnvironmentVariable::Plain(_) => None,
            })
            .collect();
        let statuses = names
            .into_iter()
            .map(|name| {
                let stored = CredentialId::for_workspace(&path, &environment, &name)
                    .ok()
                    .is_some_and(|id| {
                        self.session
                            .stored_credentials
                            .contains(id.persistence_key())
                    });
                (
                    name,
                    if stored {
                        SecretUiStatus::Stored
                    } else {
                        SecretUiStatus::NotStored
                    },
                )
            })
            .collect();
        if let Some(dialog) = self.environment_manager_dialog.as_mut() {
            dialog.secret_statuses = statuses;
        }
    }

    pub(super) fn editor_secret_sets(
        &self,
        selected: &str,
        secrets_without_values: &BTreeSet<String>,
    ) -> (BTreeSet<String>, BTreeSet<String>) {
        let Some(workspace) = &self.workspace_path else {
            return (secrets_without_values.clone(), BTreeSet::new());
        };
        let keys = self.secret_persistence_keys(workspace, selected, secrets_without_values);
        let mut unresolved = secrets_without_values.clone();
        let mut resolved = BTreeSet::new();
        for name in secrets_without_values {
            if keys
                .get(name)
                .is_some_and(|key| self.session.stored_credentials.contains(key))
            {
                unresolved.remove(name);
                resolved.insert(name.clone());
            }
        }
        (unresolved, resolved)
    }

    fn secret_persistence_keys(
        &self,
        workspace: &Path,
        environment: &str,
        names: &BTreeSet<String>,
    ) -> BTreeMap<String, String> {
        let mut cache = self.editor_secret_identities.borrow_mut();
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

    fn remember_stored_credential(&mut self, id: &CredentialId, cx: &mut Context<Self>) {
        if self
            .session
            .stored_credentials
            .insert(id.persistence_key().to_owned())
        {
            self.persist_session(cx);
        }
        self.sync_secret_statuses_from_presence();
        cx.notify();
    }

    fn forget_stored_credential(&mut self, id: &CredentialId, cx: &mut Context<Self>) {
        if self.session.stored_credentials.remove(id.persistence_key()) {
            self.persist_session(cx);
        }
        self.sync_secret_statuses_from_presence();
        cx.notify();
    }

    pub(super) fn apply_secret_presence_reconciliation(
        &mut self,
        reconciliation: SecretPresenceReconciliation,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for id in reconciliation.missing {
            changed |= self.session.stored_credentials.remove(&id);
        }
        for id in reconciliation.found {
            changed |= self.session.stored_credentials.insert(id);
        }
        if changed {
            self.persist_session(cx);
            self.sync_secret_statuses_from_presence();
        }
        cx.notify();
    }

    pub(super) fn open_secret_value_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.secret_write_in_progress || !self.can_manage_secret(&name) {
            return;
        }
        let environment = self
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft
            .name
            .clone();
        let replacing = self
            .environment_manager_dialog
            .as_ref()
            .is_some_and(|dialog| {
                dialog.secret_statuses.get(&name) == Some(&SecretUiStatus::Stored)
            });
        let theme = Theme::for_window_appearance(window.appearance());
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .placeholder("Secret value")
                .masked(true);
            input.set_editor_style(components::editor_paint_style(theme));
            input
        });
        let subscription = cx.subscribe_in(&input, window, |_, _, event, _, cx| {
            if let InputEvent::Change = event {
                cx.notify();
            }
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.secret_value_dialog = Some(SecretValueDialog {
            name,
            environment,
            replacing,
            input,
            busy: false,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn close_secret_value_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.secret_value_dialog.take() {
            dialog
                .input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.environment_manager_dialog_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn save_secret_value(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.secret_value_dialog.as_ref() else {
            return;
        };
        if dialog.busy || self.secret_write_in_progress || !self.can_manage_secret(&dialog.name) {
            return;
        }
        let value = dialog.input.read(cx).value().to_string();
        if value.is_empty() {
            return;
        }
        let Some(path) = self.workspace_path.clone() else {
            return;
        };
        let name = dialog.name.clone();
        let environment = dialog.environment.clone();
        let store = Arc::clone(&self.credential_store);
        let dialog = self.secret_value_dialog.as_mut().unwrap();
        self.secret_write_in_progress = true;
        dialog.busy = true;
        dialog.error = None;
        dialog
            .input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.spawn_in(window, async move |view, window| {
            let lookup_name = name.clone();
            let lookup_environment = environment.clone();
            let result: Result<CredentialId, CredentialStoreError> = window
                .background_spawn(async move {
                    let id = CredentialId::for_workspace(&path, &lookup_environment, &lookup_name)?;
                    store.set(&id, &value)?;
                    Ok(id)
                })
                .await;
            let _ = view.update_in(window, |view, window, cx| {
                view.secret_write_in_progress = false;
                // Presence follows the completed write. Dialog lifetime must not
                // decide whether the editor learns that the credential is stored.
                if let Ok(id) = &result {
                    view.remember_stored_credential(id, cx);
                }
                let dialog_matches = view
                    .secret_value_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.name == name && dialog.environment == environment);
                if result.is_err() {
                    if dialog_matches {
                        let dialog = view.secret_value_dialog.as_mut().unwrap();
                        dialog.busy = false;
                        dialog.error = Some("Could not save to the system credential store.");
                        cx.notify();
                    }
                    return;
                }
                if dialog_matches {
                    view.close_secret_value_dialog(window, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn confirm_delete_stored_secret(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.secret_write_in_progress
            || !self.can_manage_secret(&name)
            || self
                .secret_value_dialog
                .as_ref()
                .is_none_or(|dialog| !dialog.replacing || dialog.name != name)
            || self
                .environment_manager_dialog
                .as_ref()
                .is_none_or(|dialog| {
                    dialog.secret_statuses.get(&name) != Some(&SecretUiStatus::Stored)
                })
        {
            return;
        }
        let environment = self
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft
            .name
            .clone();
        self.show_application_dialog(ApplicationDialog::DeleteStoredSecret {
            detail: format!("Requests using {{{{{name}}}}} in {environment} will fail until another value is stored. The secret declaration will remain."),
            name, environment,
        }, window, cx);
    }

    pub(super) fn delete_stored_secret(
        &mut self,
        name: String,
        environment: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.secret_write_in_progress
            || !self.can_manage_secret(&name)
            || self
                .environment_manager_dialog
                .as_ref()
                .is_none_or(|dialog| {
                    dialog.draft.name != environment
                        || dialog.secret_statuses.get(&name) != Some(&SecretUiStatus::Stored)
                })
        {
            return;
        }
        let Some(path) = self.workspace_path.clone() else {
            return;
        };
        self.close_secret_value_dialog(window, cx);
        self.secret_write_in_progress = true;
        if let Some(dialog) = self.environment_manager_dialog.as_mut() {
            dialog
                .secret_statuses
                .insert(name.clone(), SecretUiStatus::Loading);
        }
        let store = Arc::clone(&self.credential_store);
        cx.spawn_in(window, async move |view, window| {
            let lookup_name = name.clone();
            let lookup_environment = environment.clone();
            let result: Result<CredentialId, CredentialStoreError> = window
                .background_spawn(async move {
                    let id = CredentialId::for_workspace(&path, &lookup_environment, &lookup_name)?;
                    match store.delete(&id) {
                        Ok(()) | Err(CredentialStoreError::NotFound) => Ok(id),
                        Err(error) => Err(error),
                    }
                })
                .await;
            let _ = view.update_in(window, |view, _, cx| {
                view.secret_write_in_progress = false;
                match result {
                    Ok(id) => view.forget_stored_credential(&id, cx),
                    Err(_) => {
                        let same_environment = view
                            .environment_manager_dialog
                            .as_ref()
                            .is_some_and(|dialog| dialog.draft.name == environment);
                        if same_environment {
                            view.show_toast(
                                ToastIntent::Error,
                                "Could not delete from the system credential store.",
                                cx,
                            );
                            view.sync_secret_statuses_from_presence();
                        }
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }
}
