use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{AppContext as _, Context, FocusHandle, Window};
use gpui_base::input::{InputEvent, InputState};

use super::{ApplicationDialog, ProbeApp, SecretUiStatus, ToastIntent};
use crate::credentials::{CredentialId, CredentialStore, CredentialStoreError};
use crate::execution::SecretPresenceReconciliation;
use crate::{components, theme::Theme};

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
                row.variable.is_secret()
                    && row.variable.name() == Some(name)
                    && !row.variable.is_disabled()
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
        loaded
            .workspace()
            .effective_environment_variables(&dialog.draft)
            .iter()
            .any(|row| row.variable.is_secret() && row.variable.name() == Some(name))
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
            .filter(|row| row.variable.is_secret())
            .filter_map(|row| row.variable.name().map(str::to_owned))
            .collect();
        let statuses = names
            .into_iter()
            .map(|name| {
                let key = CredentialId::for_workspace(&path, &environment, &name)
                    .ok()
                    .map(|id| id.persistence_key().to_owned());
                let status = match key.as_deref() {
                    Some(key) if self.session.presence.is_stored(key) => SecretUiStatus::Stored,
                    Some(key) if self.session.presence.is_missing(key) => SecretUiStatus::NotStored,
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
                Some(key) if self.session.presence.is_stored(key) => {
                    resolved.insert(name.clone());
                }
                Some(key) if self.session.presence.is_missing(key) => {
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
        self.session
            .presence
            .persistence_keys(workspace, environment, names)
    }

    fn remember_stored_credential(&mut self, id: &CredentialId, cx: &mut Context<Self>) {
        let changed = self
            .session
            .presence
            .record_write(id.persistence_key(), true);
        if changed {
            self.persist_session(cx);
        }
        self.sync_secret_statuses_from_presence();
        cx.notify();
    }

    fn forget_stored_credential(&mut self, id: &CredentialId, cx: &mut Context<Self>) {
        let changed = self
            .session
            .presence
            .record_write(id.persistence_key(), false);
        if changed {
            self.persist_session(cx);
        }
        self.sync_secret_statuses_from_presence();
        cx.notify();
    }

    pub(super) fn apply_secret_presence_reconciliation(
        &mut self,
        reconciliation: SecretPresenceReconciliation,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        let changed = self.session.presence.reconcile(reconciliation, revision);
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
            .is_some_and(|id| self.session.presence.is_stored(id.persistence_key()));
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
        let from_manager = dialog.from_manager;
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
                        if !from_manager || same_environment {
                            view.show_toast(
                                ToastIntent::Error,
                                "Could not delete from the system credential store.",
                                cx,
                            );
                        }
                        if same_environment {
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
            .is_some_and(|id| self.session.presence.is_stored(id.persistence_key()))
    }
}
