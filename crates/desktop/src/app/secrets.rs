use std::path::Path;

use super::*;
use crate::credentials::{CredentialId, CredentialStatus, CredentialStore, CredentialStoreError};

/// Credential availability for request highlighting. Values are never stored.
#[derive(Default)]
pub(super) struct SecretAvailabilityCache {
    statuses: BTreeMap<SecretAvailabilityKey, SecretUiStatus>,
    queries: BTreeMap<SecretAvailabilityScope, SecretAvailabilityQuery>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct SecretAvailabilityKey {
    workspace: PathBuf,
    environment: String,
    name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct SecretAvailabilityScope {
    workspace: PathBuf,
    environment: String,
}

struct SecretAvailabilityQuery {
    generation: u64,
    names: BTreeSet<String>,
    updates_dialog: bool,
    dialog_generation: u64,
    _task: Task<()>,
}

impl SecretAvailabilityCache {
    fn clear(&mut self) {
        self.statuses.clear();
        self.queries.clear();
    }

    fn status(&self, workspace: &Path, environment: &str, name: &str) -> Option<SecretUiStatus> {
        self.statuses
            .get(&SecretAvailabilityKey {
                workspace: workspace.to_path_buf(),
                environment: environment.to_owned(),
                name: name.to_owned(),
            })
            .copied()
    }

    fn record(&mut self, workspace: &Path, environment: &str, name: &str, status: SecretUiStatus) {
        self.statuses.insert(
            SecretAvailabilityKey {
                workspace: workspace.to_path_buf(),
                environment: environment.to_owned(),
                name: name.to_owned(),
            },
            status,
        );
    }
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

    pub(super) fn clear_secret_availability(&mut self) {
        self.secret_availability.clear();
    }

    /// Re-check the selected environment even when a previous answer is cached.
    /// Terminal answers stay visible until the new check finishes.
    pub(super) fn revalidate_selected_secret_availability(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.workspace_path.clone() else {
            return;
        };
        let Some(environment) = self.shell.selected_environment().map(str::to_owned) else {
            return;
        };
        let Some(names) = self.selected_enabled_secret_names() else {
            return;
        };
        if names.is_empty() {
            return;
        }
        if self.secret_query_covers(&path, &environment, &names) {
            return;
        }
        self.schedule_secret_availability(path, environment, names, false, cx);
    }

    /// Fill availability gaps for the secrets already classified in this frame.
    /// Does not read the credential store on the GPUI thread.
    pub(super) fn ensure_editor_secret_availability(
        &mut self,
        context: &components::VariableContext,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.workspace_path.clone() else {
            return;
        };
        let Some(environment) = self.shell.selected_environment().map(str::to_owned) else {
            return;
        };
        let mut names = context.secrets.clone();
        names.extend(context.resolved_secrets.iter().cloned());
        if names.is_empty()
            || names.iter().all(|name| {
                self.secret_availability
                    .status(&path, &environment, name)
                    .is_some_and(SecretUiStatus::is_terminal)
            })
            || self.secret_query_covers(&path, &environment, &names)
        {
            return;
        }
        self.schedule_secret_availability(path, environment, names, false, cx);
    }

    pub(super) fn refresh_secret_statuses(&mut self, cx: &mut Context<Self>) {
        self.secret_status_generation = self.secret_status_generation.wrapping_add(1);
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
        let names: BTreeSet<String> = loaded
            .workspace()
            .effective_environment_variables(&dialog.draft)
            .into_iter()
            .filter_map(|row| match row.variable {
                EnvironmentVariable::Secret(secret) => secret.name,
                EnvironmentVariable::Plain(_) => None,
            })
            .collect();
        if names.is_empty() {
            self.secret_availability
                .queries
                .remove(&SecretAvailabilityScope {
                    workspace: path,
                    environment,
                });
            cx.notify();
            return;
        }
        for name in &names {
            dialog
                .secret_statuses
                .insert(name.clone(), SecretUiStatus::Loading);
        }
        self.schedule_secret_availability(path, environment, names, true, cx);
        cx.notify();
    }

    fn selected_enabled_secret_names(&self) -> Option<BTreeSet<String>> {
        let selected = self.shell.selected_environment()?;
        let loaded = self.loaded_workspace.as_ref()?;
        let environment = resolve_environment(loaded.workspace().environments(), selected).ok()?;
        Some(environment.secrets_without_values().clone())
    }

    fn secret_query_covers(
        &self,
        workspace: &Path,
        environment: &str,
        names: &BTreeSet<String>,
    ) -> bool {
        self.secret_availability
            .queries
            .get(&SecretAvailabilityScope {
                workspace: workspace.to_path_buf(),
                environment: environment.to_owned(),
            })
            .is_some_and(|query| names.is_subset(&query.names))
    }

    pub(super) fn editor_secret_sets(
        &self,
        selected: &str,
        secrets_without_values: &BTreeSet<String>,
    ) -> (BTreeSet<String>, BTreeSet<String>) {
        let Some(workspace) = &self.workspace_path else {
            return (secrets_without_values.clone(), BTreeSet::new());
        };
        let mut unresolved = secrets_without_values.clone();
        let mut resolved = BTreeSet::new();
        for name in secrets_without_values {
            if self.secret_availability.status(workspace, selected, name)
                == Some(SecretUiStatus::Stored)
            {
                unresolved.remove(name);
                resolved.insert(name.clone());
            }
        }
        (unresolved, resolved)
    }

    fn record_secret_availability(
        &mut self,
        workspace: &Path,
        environment: &str,
        name: &str,
        status: SecretUiStatus,
    ) {
        self.secret_availability
            .record(workspace, environment, name, status);
    }

    /// `status` is availability only. Callers must not read secret values here.
    fn schedule_secret_availability(
        &mut self,
        workspace: PathBuf,
        environment: String,
        names: BTreeSet<String>,
        updates_dialog: bool,
        cx: &mut Context<Self>,
    ) {
        let scope = SecretAvailabilityScope {
            workspace: workspace.clone(),
            environment: environment.clone(),
        };
        let generation = self
            .secret_availability
            .queries
            .get(&scope)
            .map(|query| query.generation)
            .unwrap_or(0)
            .wrapping_add(1);
        let (updates_dialog, dialog_generation) = if updates_dialog {
            (true, self.secret_status_generation)
        } else if let Some(existing) = self.secret_availability.queries.get(&scope) {
            (existing.updates_dialog, existing.dialog_generation)
        } else {
            (false, self.secret_status_generation)
        };
        let store = Arc::clone(&self.credential_store);
        let names_for_task = names.clone();
        let task_workspace = workspace;
        let task_environment = environment;
        let task_scope = scope.clone();
        let query_generation = generation;
        let write_dialog = updates_dialog;
        let write_dialog_generation = dialog_generation;
        let task = cx.spawn(async move |view, cx| {
            let result = cx
                .background_spawn(async move {
                    names_for_task
                        .into_iter()
                        .map(|name| {
                            let status = CredentialId::for_workspace(
                                &task_workspace,
                                &task_environment,
                                &name,
                            )
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
                let current = view
                    .secret_availability
                    .queries
                    .get(&task_scope)
                    .map(|query| query.generation);
                if current != Some(query_generation) {
                    return;
                }
                view.secret_availability.queries.remove(&task_scope);
                for (name, status) in &result {
                    view.secret_availability.record(
                        &task_scope.workspace,
                        &task_scope.environment,
                        name,
                        *status,
                    );
                }
                if write_dialog
                    && view.secret_status_generation == write_dialog_generation
                    && view.workspace_path.as_deref() == Some(task_scope.workspace.as_path())
                    && let Some(dialog) = view.environment_manager_dialog.as_mut()
                    && dialog.draft.name == task_scope.environment
                {
                    dialog.secret_statuses = result.into_iter().collect();
                }
                cx.notify();
            });
        });
        self.secret_availability.queries.insert(
            scope,
            SecretAvailabilityQuery {
                generation,
                names,
                updates_dialog,
                dialog_generation,
                _task: task,
            },
        );
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
        let workspace_path = path.clone();
        cx.spawn_in(window, async move |view, window| {
            let lookup_name = name.clone();
            let lookup_environment = environment.clone();
            let result = window
                .background_spawn(async move {
                    CredentialId::for_workspace(&path, &lookup_environment, &lookup_name)
                        .and_then(|id| store.set(&id, &value))
                })
                .await;
            let _ =
                view.update_in(window, |view, window, cx| {
                    view.secret_write_in_progress = false;
                    // Status generation only drops stale status queries. A finished write
                    // still completes when this workspace, environment, and secret are current.
                    let same_context = view.workspace_path.as_ref() == Some(&workspace_path)
                        && view
                            .environment_manager_dialog
                            .as_ref()
                            .is_some_and(|dialog| dialog.draft.name == environment)
                        && view.can_manage_secret(&name);
                    if !same_context {
                        if let Some(dialog) = view.secret_value_dialog.as_mut()
                            && dialog.name == name
                            && dialog.environment == environment
                        {
                            dialog.busy = false;
                            cx.notify();
                        }
                        return;
                    }
                    if result.is_err() {
                        if let Some(dialog) = view.secret_value_dialog.as_mut()
                            && dialog.name == name
                            && dialog.environment == environment
                        {
                            dialog.busy = false;
                            dialog.error = Some("Could not save to the system credential store.");
                            cx.notify();
                        }
                        return;
                    }
                    if view.secret_value_dialog.as_ref().is_some_and(|dialog| {
                        dialog.name == name && dialog.environment == environment
                    }) {
                        view.close_secret_value_dialog(window, cx);
                    }
                    view.record_secret_availability(
                        &workspace_path,
                        &environment,
                        &name,
                        SecretUiStatus::Stored,
                    );
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
        let store = Arc::clone(&self.credential_store);
        if let Some(path) = self.workspace_path.clone() {
            self.record_secret_availability(&path, &environment, &name, SecretUiStatus::Loading);
        }
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
                let same_environment = view
                    .environment_manager_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.draft.name == environment);
                if !same_environment || !view.can_manage_secret(&name) {
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
