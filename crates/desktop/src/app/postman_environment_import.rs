use super::*;
use crate::credentials::{CredentialId, CredentialStoreError};
use probe_core::ImportDiagnostic;
use probe_postman::ImportedPostmanEnvironment;

/// Only `environment` is handed to YAML persistence or the Environment Manager.
pub(super) struct PostmanEnvironmentImport {
    pub(super) environment: Environment,
    pub(super) diagnostics: Vec<ImportDiagnostic>,
    secrets: ImportedEnvironmentSecrets,
}

pub(super) struct ImportedEnvironmentSecrets(Vec<(String, Option<String>)>);

impl std::fmt::Debug for PostmanEnvironmentImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostmanEnvironmentImport")
            .field("variable_count", &self.environment.variables.len())
            .field("secret_count", &self.secrets.0.len())
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

impl From<ImportedPostmanEnvironment> for PostmanEnvironmentImport {
    fn from(imported: ImportedPostmanEnvironment) -> Self {
        let mut environment = imported.environment;
        let mut secrets = Vec::new();
        for variable in &mut environment.variables {
            let EnvironmentVariable::Plain(plain) = variable else {
                continue;
            };
            let Some(name) = plain.name.as_ref() else {
                continue;
            };
            if imported.variable_types.get(name).map(String::as_str) != Some("secret") {
                continue;
            }
            // The parser converts secret-marked values to strings, except exported null.
            let value = match plain.value.take() {
                Some(VariableValueSet::Single(VariableValue::String(value)))
                    if !value.is_empty() =>
                {
                    Some(value)
                }
                _ => None,
            };
            secrets.push((name.clone(), value));
            *variable = EnvironmentVariable::Secret(SecretVariable {
                name: plain.name.take(),
                value_type: None,
                disabled: plain.disabled,
            });
        }
        Self {
            environment,
            diagnostics: imported
                .diagnostics
                .into_iter()
                .filter(|d| d.code != "secret_value_plaintext")
                .collect(),
            secrets: ImportedEnvironmentSecrets(secrets),
        }
    }
}

impl PostmanEnvironmentImport {
    pub(super) fn into_parts(self) -> (Environment, ImportedEnvironmentSecrets) {
        (self.environment, self.secrets)
    }
}

impl ImportedEnvironmentSecrets {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl ProbeApp {
    fn import_credentials_are_current(
        &self,
        path: &Path,
        baseline: probe_opencollection::WorkspaceBaseline,
    ) -> bool {
        self.workspace_path.as_deref() == Some(path)
            && self
                .loaded_workspace
                .as_ref()
                .is_some_and(|loaded| loaded.baseline() == baseline)
            && !self.loading
            && self.application_dialog.is_none()
            && self.pending_application_dialogs.is_empty()
    }

    /// Called only after the declaration-only YAML write has committed and integrated.
    pub(super) fn store_imported_environment_secrets(
        &mut self,
        secrets: ImportedEnvironmentSecrets,
        environment: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if secrets.0.is_empty() {
            self.secret_write_in_progress = false;
            self.pump_workspace_writes(window, cx);
            return;
        }
        let Some(path) = self.workspace_path.clone() else {
            self.secret_write_in_progress = false;
            self.show_imported_secret_failure(cx);
            self.pump_workspace_writes(window, cx);
            return;
        };
        let baseline = self.loaded_workspace.as_ref().unwrap().baseline();
        let store = Arc::clone(&self.credential_store);
        #[cfg(test)]
        let mut gate = self.environment_import_credentials_gate.take();
        self.environment_save_task = Some(cx.spawn_in(window, async move |view, window| {
            let mut incomplete = false;
            for (name, value) in secrets.0 {
                let current = view
                    .update_in(window, |view, _, _| {
                        view.import_credentials_are_current(&path, baseline)
                    })
                    .unwrap_or(false);
                if !current {
                    incomplete = true;
                    break;
                }
                let lookup_path = path.clone();
                let lookup_environment = environment.clone();
                let lookup_store = Arc::clone(&store);
                let lookup = window
                    .background_spawn(async move {
                        let id =
                            CredentialId::for_workspace(&lookup_path, &lookup_environment, &name)?;
                        // Preserve credentials left by a previously removed declaration as well.
                        let exists = match lookup_store.get(&id) {
                            Ok(value) => value.is_some(),
                            Err(CredentialStoreError::NotFound) => false,
                            Err(error) => return Err(error),
                        };
                        Ok((id, exists))
                    })
                    .await;
                #[cfg(test)]
                if let Some(gate) = gate.take() {
                    let _ = gate.await;
                }
                // Recheck after a potentially slow native lookup, before starting a write.
                let current = view
                    .update_in(window, |view, _, cx| {
                        if !view.import_credentials_are_current(&path, baseline) {
                            return false;
                        }
                        if let Ok((id, exists)) = &lookup {
                            if *exists {
                                view.remember_stored_credential(id, cx);
                            } else {
                                view.forget_stored_credential(id, cx);
                            }
                        }
                        true
                    })
                    .unwrap_or(false);
                if !current {
                    incomplete = true;
                    break;
                }
                let (id, exists) = match lookup {
                    Ok(lookup) => lookup,
                    Err(_) => {
                        incomplete |= value.is_some();
                        continue;
                    }
                };
                let Some(value) = value else {
                    continue;
                };
                if exists {
                    incomplete = true;
                    continue;
                }
                let write_store = Arc::clone(&store);
                let result = window
                    .background_spawn(async move { write_store.set(&id, &value).map(|()| id) })
                    .await;
                match result {
                    Ok(id) => {
                        let _ = view.update_in(window, |view, _, cx| {
                            view.remember_stored_credential(&id, cx)
                        });
                    }
                    Err(_) => incomplete = true,
                }
            }
            let _ = view.update_in(window, |view, window, cx| {
                view.environment_save_task = None;
                view.secret_write_in_progress = false;
                view.sync_secret_statuses_from_presence();
                if incomplete {
                    view.show_imported_secret_failure(cx);
                }
                view.pump_workspace_writes(window, cx);
                cx.notify();
            });
        }));
    }

    pub(super) fn show_imported_secret_failure(&mut self, cx: &mut Context<Self>) {
        self.show_toast(ToastIntent::Warning, "Import partially completed: the environment was saved, but some secret values were not imported. Set or replace them in Environment Manager.", cx);
    }
}
