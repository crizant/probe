use super::*;
use crate::credentials::{CredentialId, CredentialStatus, CredentialStore, CredentialStoreError};

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
    fn status(&self, _: &CredentialId) -> Result<CredentialStatus, CredentialStoreError> {
        Ok(CredentialStatus::NotStored)
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
        panic!("status must not fetch a secret value")
    }
}

pub(super) struct SecretValueDialog {
    pub(super) name: String,
    pub(super) environment: String,
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

    pub(super) fn refresh_secret_statuses(&mut self, cx: &mut Context<Self>) {
        self.secret_status_task = None;
        self.secret_status_generation = self.secret_status_generation.wrapping_add(1);
        let generation = self.secret_status_generation;
        let Some(dialog) = self.environment_manager_dialog.as_mut() else {
            return;
        };
        dialog.secret_statuses.clear();
        let Some(loaded) = &self.loaded_workspace else {
            return;
        };
        let Some(path) = self.workspace_path.clone() else {
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
        if names.is_empty() {
            cx.notify();
            return;
        }
        for name in &names {
            dialog
                .secret_statuses
                .insert(name.clone(), SecretUiStatus::Loading);
        }
        let store = Arc::clone(&self.credential_store);
        self.secret_status_task = Some(cx.spawn(async move |view, cx| {
            let result = cx
                .background_spawn(async move {
                    names
                        .into_iter()
                        .map(|name| {
                            let status = CredentialId::for_workspace(&path, &environment, &name)
                                .and_then(|id| store.status(&id));
                            let status = match status {
                                Ok(CredentialStatus::Stored) => SecretUiStatus::Stored,
                                Ok(CredentialStatus::NotStored)
                                | Err(CredentialStoreError::NotFound) => SecretUiStatus::NotStored,
                                Err(_) => SecretUiStatus::Unavailable,
                            };
                            (name, status)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                if view.secret_status_generation != generation {
                    return;
                }
                view.secret_status_task = None;
                if let Some(dialog) = view.environment_manager_dialog.as_mut() {
                    dialog.secret_statuses = result.into_iter().collect();
                    cx.notify();
                }
            });
        }));
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
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Secret value")
                .masked(true)
        });
        let subscription =
            cx.subscribe_in(&input, window, |view, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => view.save_secret_value(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.secret_value_dialog = Some(SecretValueDialog {
            name,
            environment,
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
        let generation = self.secret_status_generation;
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
            let result = window
                .background_spawn(async move {
                    CredentialId::for_workspace(&path, &lookup_environment, &lookup_name)
                        .and_then(|id| store.set(&id, &value))
                })
                .await;
            let _ = view.update_in(window, |view, window, cx| {
                view.secret_write_in_progress = false;
                if view.secret_status_generation != generation || !view.can_manage_secret(&name) {
                    return;
                }
                if let Some(dialog) = view.secret_value_dialog.as_mut() {
                    if dialog.name != name || dialog.environment != environment {
                        return;
                    }
                    dialog.busy = false;
                    if result.is_err() {
                        dialog.error = Some("Could not save to the system credential store.");
                        cx.notify();
                        return;
                    }
                }
                if view.secret_value_dialog.is_some() {
                    view.close_secret_value_dialog(window, cx);
                }
                view.refresh_secret_statuses(cx);
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
                .is_none_or(|dialog| dialog.draft.name != environment)
        {
            return;
        }
        let Some(path) = self.workspace_path.clone() else {
            return;
        };
        let generation = self.secret_status_generation;
        self.secret_write_in_progress = true;
        let store = Arc::clone(&self.credential_store);
        if let Some(dialog) = self.environment_manager_dialog.as_mut() {
            dialog
                .secret_statuses
                .insert(name.clone(), SecretUiStatus::Loading);
        }
        cx.spawn_in(window, async move |view, window| {
            let lookup_name = name.clone();
            let lookup_environment = environment.clone();
            let result = window
                .background_spawn(async move {
                    CredentialId::for_workspace(&path, &lookup_environment, &lookup_name)
                        .and_then(|id| store.delete(&id))
                })
                .await;
            let _ = view.update_in(window, |view, _, cx| {
                view.secret_write_in_progress = false;
                if view.secret_status_generation != generation || !view.can_manage_secret(&name) {
                    return;
                }
                if result.is_err() && result != Err(CredentialStoreError::NotFound) {
                    view.show_toast(
                        ToastIntent::Error,
                        "Could not delete from the system credential store.",
                        cx,
                    );
                }
                view.refresh_secret_statuses(cx);
            });
        })
        .detach();
        cx.notify();
    }
}
