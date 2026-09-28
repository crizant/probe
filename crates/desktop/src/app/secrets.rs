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
    pub(super) target: SecretTarget,
    pub(super) restore_focus: FocusHandle,
    pub(super) from_manager: bool,
    pub(super) replacing: bool,
    pub(super) input: gpui::Entity<InputState>,
    pub(super) busy: bool,
    pub(super) error: Option<&'static str>,
    pub(super) _subscription: gpui::Subscription,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SecretTarget {
    pub(super) workspace: PathBuf,
    pub(super) environment: String,
    pub(super) name: String,
}

impl ProbeApp {
    fn saved_secret_target(&self, environment: &str, name: &str) -> Option<SecretTarget> {
        let workspace = self.workspace_path.as_ref()?;
        let loaded = self.loaded_workspace.as_ref()?;
        let selected = loaded
            .workspace()
            .environments()
            .iter()
            .find(|candidate| candidate.name == environment)?;
        let effective_secret = loaded
            .workspace()
            .effective_environment_variables(selected)
            .into_iter()
            .any(|row| {
                matches!(row.variable,
                EnvironmentVariable::Secret(ref secret)
                    if secret.name.as_deref() == Some(name) && !secret.disabled)
            });
        effective_secret.then(|| SecretTarget {
            workspace: workspace.clone(),
            environment: environment.to_owned(),
            name: name.to_owned(),
        })
    }

    fn secret_target_is_current(&self, target: &SecretTarget, from_manager: bool) -> bool {
        self.saved_secret_target(&target.environment, &target.name)
            .as_ref()
            == Some(target)
            && if from_manager {
                self.environment_manager_dialog
                    .as_ref()
                    .is_some_and(|dialog| {
                        dialog.draft.name == target.environment
                            && self.can_manage_secret(&target.name)
                    })
            } else {
                self.shell.selected_environment() == Some(target.environment.as_str())
            }
    }
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
                let key = CredentialId::for_workspace(&path, &environment, &name)
                    .ok()
                    .map(|id| id.persistence_key().to_owned());
                let status = match key.as_deref() {
                    Some(key) if self.session.stored_credentials.contains(key) => {
                        SecretUiStatus::Stored
                    }
                    Some(key) if self.session.missing_credentials.contains(key) => {
                        SecretUiStatus::NotStored
                    }
                    _ => SecretUiStatus::Unknown,
                };
                (name, status)
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
    ) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
        let Some(workspace) = &self.workspace_path else {
            return (
                BTreeSet::new(),
                BTreeSet::new(),
                secrets_without_values.clone(),
            );
        };
        let keys = self.secret_persistence_keys(workspace, selected, secrets_without_values);
        let mut missing = BTreeSet::new();
        let mut resolved = BTreeSet::new();
        let mut unknown = BTreeSet::new();
        for name in secrets_without_values {
            match keys.get(name) {
                Some(key) if self.session.stored_credentials.contains(key) => {
                    resolved.insert(name.clone());
                }
                Some(key) if self.session.missing_credentials.contains(key) => {
                    missing.insert(name.clone());
                }
                _ => {
                    unknown.insert(name.clone());
                }
            }
        }
        (missing, resolved, unknown)
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
        let changed = self.record_credential_presence(id.persistence_key(), true);
        self.note_credential_presence_changed();
        if changed {
            self.persist_session(cx);
        }
        self.sync_secret_statuses_from_presence();
        cx.notify();
    }

    fn forget_stored_credential(&mut self, id: &CredentialId, cx: &mut Context<Self>) {
        let changed = self.record_credential_presence(id.persistence_key(), false);
        self.note_credential_presence_changed();
        if changed {
            self.persist_session(cx);
        }
        self.sync_secret_statuses_from_presence();
        cx.notify();
    }

    /// Moves one opaque identity between the stored and known-missing sets.
    ///
    /// `stored` records a value Probe has learned is present. `false` records a
    /// trusted absence. Returns whether either set changed.
    fn record_credential_presence(&mut self, key: &str, stored: bool) -> bool {
        if stored {
            let inserted = self.session.stored_credentials.insert(key.to_owned());
            let cleared = self.session.missing_credentials.remove(key);
            inserted || cleared
        } else {
            let removed = self.session.stored_credentials.remove(key);
            let recorded = self.session.missing_credentials.insert(key.to_owned());
            removed || recorded
        }
    }

    fn note_credential_presence_changed(&mut self) {
        self.credential_presence_revision = self.credential_presence_revision.wrapping_add(1);
    }

    pub(super) fn apply_secret_presence_reconciliation(
        &mut self,
        reconciliation: SecretPresenceReconciliation,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if revision < self.credential_presence_revision {
            return;
        }
        let mut changed = false;
        for id in reconciliation.missing {
            changed |= self.record_credential_presence(&id, false);
        }
        for id in reconciliation.found {
            changed |= self.record_credential_presence(&id, true);
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
        let Some(target) = self.saved_secret_target(&environment, &name) else {
            return;
        };
        let replacing = self
            .environment_manager_dialog
            .as_ref()
            .is_some_and(|dialog| {
                dialog.secret_statuses.get(&name) == Some(&SecretUiStatus::Stored)
            });
        self.show_secret_value_dialog(
            target,
            replacing,
            self.environment_manager_dialog_focus.clone(),
            true,
            window,
            cx,
        );
    }

    pub(super) fn open_editor_secret_value_dialog(
        &mut self,
        workspace: PathBuf,
        environment: String,
        name: String,
        restore_focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.secret_write_in_progress
            || self.workspace_path.as_ref() != Some(&workspace)
            || self.shell.selected_environment() != Some(environment.as_str())
        {
            return;
        }
        let Some(target) = self.saved_secret_target(&environment, &name) else {
            return;
        };
        let replacing = CredentialId::for_workspace(&target.workspace, &target.environment, &name)
            .ok()
            .is_some_and(|id| {
                self.session
                    .stored_credentials
                    .contains(id.persistence_key())
            });
        self.show_secret_value_dialog(target, replacing, restore_focus, false, window, cx);
    }

    fn show_secret_value_dialog(
        &mut self,
        target: SecretTarget,
        replacing: bool,
        restore_focus: FocusHandle,
        from_manager: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            target,
            restore_focus,
            from_manager,
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
            dialog.restore_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn save_secret_value(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.secret_value_dialog.as_ref() else {
            return;
        };
        if dialog.busy
            || self.secret_write_in_progress
            || !self.secret_target_is_current(&dialog.target, dialog.from_manager)
        {
            return;
        }
        let value = dialog.input.read(cx).value().to_string();
        if value.is_empty() {
            return;
        }
        let target = dialog.target.clone();
        let store = Arc::clone(&self.credential_store);
        let dialog = self.secret_value_dialog.as_mut().unwrap();
        self.secret_write_in_progress = true;
        dialog.busy = true;
        dialog.error = None;
        dialog
            .input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.spawn_in(window, async move |view, window| {
            let lookup = target.clone();
            let result: Result<CredentialId, CredentialStoreError> = window
                .background_spawn(async move {
                    let id = CredentialId::for_workspace(
                        &lookup.workspace,
                        &lookup.environment,
                        &lookup.name,
                    )?;
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
                    .is_some_and(|dialog| dialog.target == target);
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
        let Some(dialog) = self.secret_value_dialog.as_ref() else {
            return;
        };
        if self.secret_write_in_progress
            || !dialog.replacing
            || dialog.target.name != name
            || !self.secret_target_is_current(&dialog.target, dialog.from_manager)
            || !self.secret_is_stored(&dialog.target, dialog.from_manager)
        {
            return;
        }
        let environment = dialog.target.environment.clone();
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
        let Some(dialog) = self.secret_value_dialog.as_ref() else {
            return;
        };
        if self.secret_write_in_progress
            || dialog.target.name != name
            || dialog.target.environment != environment
            || !self.secret_target_is_current(&dialog.target, dialog.from_manager)
            || !self.secret_is_stored(&dialog.target, dialog.from_manager)
        {
            return;
        }
        let target = dialog.target.clone();
        self.close_secret_value_dialog(window, cx);
        self.secret_write_in_progress = true;
        if let Some(dialog) = self.environment_manager_dialog.as_mut() {
            dialog
                .secret_statuses
                .insert(name.clone(), SecretUiStatus::Loading);
        }
        let store = Arc::clone(&self.credential_store);
        cx.spawn_in(window, async move |view, window| {
            let lookup = target.clone();
            let result: Result<CredentialId, CredentialStoreError> = window
                .background_spawn(async move {
                    let id = CredentialId::for_workspace(
                        &lookup.workspace,
                        &lookup.environment,
                        &lookup.name,
                    )?;
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

    pub(super) fn secret_is_stored(&self, target: &SecretTarget, from_manager: bool) -> bool {
        if from_manager {
            return self
                .environment_manager_dialog
                .as_ref()
                .is_some_and(|dialog| {
                    dialog.secret_statuses.get(&target.name) == Some(&SecretUiStatus::Stored)
                });
        }
        CredentialId::for_workspace(&target.workspace, &target.environment, &target.name)
            .ok()
            .is_some_and(|id| {
                self.session
                    .stored_credentials
                    .contains(id.persistence_key())
            })
    }
}
