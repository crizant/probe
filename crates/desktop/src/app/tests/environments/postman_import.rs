use super::*;
use crate::app::postman_environment_import::PostmanEnvironmentImport;
use probe_opencollection::load_workspace;

fn prepared(source: &str) -> PostmanEnvironmentImport {
    probe_postman::import_postman_environment(source)
        .unwrap()
        .into()
}

fn export_with_warning(name: &str) -> String {
    let mut source: serde_json::Value = serde_json::from_str(&export(name)).unwrap();
    source["values"][0]["description"] = serde_json::json!("PRIVATE_DESCRIPTION_SENTINEL");
    source.to_string()
}

fn export(name: &str) -> String {
    serde_json::json!({
        "name": name,
        "_postman_variable_scope": "environment",
        "values": [
            {"key": "token", "value": SECRET_SENTINEL, "type": "secret"},
            {"key": "disabled", "value": 42, "enabled": false}
        ]
    })
    .to_string()
}

fn choose_source(workspace: &EnvironmentWorkspace, cx: &mut TestAppContext, source: &str) {
    workspace.store.allow_execution_reads();
    {
        let mut visual = workspace.visual(cx);
        let action = visual
            .debug_bounds("environment-manager-import-postman")
            .unwrap();
        visual.simulate_click(action.center(), Modifiers::default());
        visual.run_until_parked();
    }
    respond_to_source(cx, source);
}

fn respond_to_source(cx: &mut TestAppContext, source: &str) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.json");
    fs::write(&path, source).unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![path])
    });
    cx.run_until_parked();
}

fn confirm_import(workspace: &EnvironmentWorkspace, cx: &mut TestAppContext) {
    let mut visual = workspace.visual(cx);
    let action = visual
        .debug_bounds("application-dialog-import-environment")
        .unwrap();
    visual.simulate_click(action.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();
}

fn assert_no_leak(view: &ProbeApp) {
    assert!(!format!("{:?}", toast_debug(view)).contains(SECRET_SENTINEL));
}

/// Records only sanitized declarations at the instant a credential write begins.
struct AuditedImportStore {
    inner: Arc<FakeCredentials>,
    path: PathBuf,
    environment: &'static str,
    fail: Option<CredentialId>,
    fail_lookup: bool,
}

impl CredentialStore for AuditedImportStore {
    fn set(&self, id: &CredentialId, value: &str) -> Result<(), CredentialStoreError> {
        let persisted = load_workspace(&self.path).unwrap();
        let snapshot = format!("{:?}", persisted.workspace().environments());
        assert!(!snapshot.contains(SECRET_SENTINEL));
        assert!(!snapshot.contains("SECOND_SECRET_VALUE"));
        assert!(
            persisted
                .workspace()
                .environments()
                .iter()
                .find(|env| env.name == self.environment)
                .expect("the imported environment must be on disk before credential writes")
                .variables
                .iter()
                .any(EnvironmentVariable::is_secret)
        );
        if self.fail.as_ref() == Some(id) {
            return Err(CredentialStoreError::BackendFailure);
        }
        self.inner.set(id, value)
    }
    fn get(
        &self,
        id: &CredentialId,
    ) -> Result<Option<probe_core::SecretValue>, CredentialStoreError> {
        if self.fail_lookup {
            return Err(CredentialStoreError::Unavailable);
        }
        self.inner.get(id)
    }
    fn delete(&self, id: &CredentialId) -> Result<(), CredentialStoreError> {
        self.inner.delete(id)
    }
}

#[gpui_kit::test]
fn postman_environment_import_stores_secrets_after_yaml_without_activating(
    cx: &mut TestAppContext,
) {
    for bundled in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let workspace = if bundled {
            EnvironmentWorkspace::writable(cx, "postman-import-success")
        } else {
            fs::write(
                directory.path().join("opencollection.yml"),
                "opencollection: 1.0.0\ninfo:\n  name: Unbundled\nbundled: false\n",
            )
            .unwrap();
            fs::create_dir(directory.path().join("environments")).unwrap();
            fs::write(
                directory.path().join("environments/development.yml"),
                "name: development\nvariables: []\n",
            )
            .unwrap();
            EnvironmentWorkspace::load(cx, directory.path().canonicalize().unwrap(), false)
        };
        workspace.open_manager(cx, "development");
        let before = load_workspace(&workspace.path)
            .unwrap()
            .workspace()
            .environments()
            .to_vec();
        let mut source: serde_json::Value =
            serde_json::from_str(&export("Imported environment")).unwrap();
        source["values"][0]["key"] = serde_json::json!(" token ");
        source["values"].as_array_mut().unwrap().extend([
            serde_json::json!({"key": "disabled-secret", "value": "SECOND_SECRET_VALUE", "type": "secret", "enabled": false}),
            serde_json::json!({"key": "empty-secret", "value": "", "type": "secret"}),
            serde_json::json!({"key": "null-secret", "value": null, "type": "secret"}),
            serde_json::json!({"key": " regular ", "value": "normal-value", "type": "default", "enabled": true}),
        ]);
        let source = source.to_string();
        let expected = prepared(&source);
        assert!(!format!("{expected:?} {:?}", expected.environment).contains(SECRET_SENTINEL));
        workspace.update(cx, |view, _, _| {
            view.credential_store = Arc::new(AuditedImportStore {
                inner: workspace.store.clone(),
                path: workspace.path.clone(),
                environment: "Imported environment",
                fail: None,
                fail_lookup: false,
            })
        });
        choose_source(&workspace, cx, &source);
        let reloaded = load_workspace(&workspace.path).unwrap();
        for original in &before {
            assert_eq!(
                reloaded
                    .workspace()
                    .environments()
                    .iter()
                    .find(|env| env.name == original.name),
                Some(original)
            );
        }
        let imported = reloaded
            .workspace()
            .environments()
            .iter()
            .find(|env| env.name == "Imported environment")
            .unwrap();
        let original = probe_postman::import_postman_environment(&source)
            .unwrap()
            .environment;
        assert_eq!(imported.name, original.name);
        assert_eq!(imported.variables.len(), 6);
        for index in [1, 5] {
            assert_eq!(imported.variables[index], original.variables[index]);
        }
        for (index, name, disabled) in [
            (0, " token ", false),
            (2, "disabled-secret", true),
            (3, "empty-secret", false),
            (4, "null-secret", false),
        ] {
            assert_eq!(
                imported.variables[index],
                EnvironmentVariable::Secret(SecretVariable {
                    name: Some(name.into()),
                    value_type: None,
                    disabled
                })
            );
        }
        let yaml_path = if bundled {
            workspace.path.clone()
        } else {
            workspace.path.join("environments/Imported environment.yml")
        };
        let yaml = fs::read_to_string(yaml_path).unwrap();
        assert!(!yaml.contains(SECRET_SENTINEL));
        assert!(!yaml.contains("SECOND_SECRET_VALUE"));
        for (name, value, status) in [
            (" token ", Some(SECRET_SENTINEL), SecretUiStatus::Stored),
            (
                "disabled-secret",
                Some("SECOND_SECRET_VALUE"),
                SecretUiStatus::Stored,
            ),
            ("empty-secret", None, SecretUiStatus::NotStored),
            ("null-secret", None, SecretUiStatus::NotStored),
        ] {
            let id = workspace.credential("Imported environment", name);
            assert_eq!(workspace.store.value(&id).as_deref(), value);
            workspace.update(cx, |view, _, _| {
                assert_eq!(presence(view, &id), Some(value.is_some()));
                assert_eq!(manager_status(view, name), Some(status));
            });
        }
        workspace.update(cx, |view, _, _| {
            assert!(
                view.application_dialog.is_none(),
                "secrets alone must not prompt"
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .original_name,
                "Imported environment"
            );
            assert_eq!(view.shell.selected_environment(), Some("development"));
            assert_eq!(
                view.session.selected_environment_for(&workspace.path),
                Some("development")
            );
            assert!(
                !serde_json::to_string(&view.session)
                    .unwrap()
                    .contains(SECRET_SENTINEL)
            );
            assert_no_leak(view);
        });
    }
}

#[gpui_kit::test]
fn postman_environment_import_rejects_duplicate_invalid_and_conflicting_sources(
    cx: &mut TestAppContext,
) {
    for (label, source, external_change, expected) in [
        ("duplicate", export("development"), false, "already exists"),
        (
            "invalid",
            format!("{{{SECRET_SENTINEL}"),
            false,
            "invalid or unsupported",
        ),
        ("failure", export("Imported"), true, "Could not persist"),
    ] {
        let workspace = EnvironmentWorkspace::writable(cx, &format!("postman-import-{label}"));
        workspace.open_manager(cx, "development");
        if external_change {
            fs::write(
                &workspace.path,
                format!("{}\n# external change\n", workspace.yaml()),
            )
            .unwrap();
        }
        let before = workspace.yaml();
        choose_source(&workspace, cx, &source);
        assert_eq!(workspace.yaml(), before);
        assert_eq!(workspace.store.get_calls(), 0);
        assert!(workspace.store.values.lock().unwrap().is_empty());
        workspace.update(cx, |view, _, _| {
            assert!(
                !view
                    .loaded_workspace
                    .as_ref()
                    .unwrap()
                    .workspace()
                    .environments()
                    .iter()
                    .any(|env| env.name == "Imported")
            );
            assert!(
                has_active_toast(view, ToastIntent::Error, expected),
                "{:?}",
                toast_debug(view)
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .original_name,
                "development"
            );
            assert_no_leak(view);
        });
    }
}

#[gpui_kit::test]
fn postman_environment_import_preserves_unsaved_drafts_and_pending_writes(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "postman-import-blocked");
    workspace.open_manager(cx, "development");
    let before = workspace.yaml();
    workspace.update(cx, |view, window, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft_mut().name = "Unsaved".into()
        });
        view.import_postman_environment(prepared(&export("Imported")), window, cx);
        assert!(view.environment_save_task.is_none());
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .draft()
                .name,
            "Unsaved"
        );
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft_mut().name = "development".into()
        });
        view.pending_environment_saves
            .insert(("development".into(), "token".into()));
        view.import_postman_environment(prepared(&export("Imported")), window, cx);
        assert!(view.environment_save_task.is_none());
        assert_no_leak(view);
    });
    assert_eq!(workspace.yaml(), before);
}

#[gpui_kit::test]
fn postman_environment_import_is_available_for_an_empty_collection(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable_source(
        cx,
        "postman-import-empty",
        "opencollection: 1.0.0\ninfo:\n  name: Empty\nbundled: true\n",
    );
    workspace.update(cx, |_, _, cx| cx.notify());
    cx.run_until_parked();
    {
        let mut visual = workspace.visual(cx);
        let trigger = visual.debug_bounds("request-environment-trigger").unwrap();
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
        let action = visual.debug_bounds("request-environment-action-2").unwrap();
        visual.simulate_click(action.center(), Modifiers::default());
        visual.run_until_parked();
    }
    let source = export("Imported").replace("\"secret\"", "\"default\"");
    respond_to_source(cx, &source);
    let reloaded = load_workspace(&workspace.path).unwrap();
    assert_eq!(
        reloaded.workspace().environments(),
        &[probe_postman::import_postman_environment(&source)
            .unwrap()
            .environment]
    );
    workspace.update(cx, |view, _, _| {
        assert_eq!(view.shell.selected_environment(), None);
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .original_name,
            "Imported"
        );
        assert!(view.create_environment_dialog.is_none());
        assert!(view.application_dialog.is_none());
        assert_no_leak(view);
    });
}

#[gpui_kit::test]
fn postman_environment_import_cancel_stale_and_pending_confirmations_do_not_write(
    cx: &mut TestAppContext,
) {
    for state in ["cancel", "stale", "pending", "conflict"] {
        let workspace = EnvironmentWorkspace::writable(cx, &format!("postman-confirm-{state}"));
        workspace.open_manager(cx, "development");
        let before = workspace.yaml();
        choose_source(&workspace, cx, &export_with_warning("Imported"));
        workspace.update(cx, |view, window, cx| {
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::ConfirmPostmanEnvironmentImport { .. })
            ));
            match state {
                "stale" => view.loaded_workspace = Some(load_workspace(&workspace.path).unwrap()),
                "pending" => {
                    view.pending_environment_saves
                        .insert(("development".into(), "token".into()));
                }
                "conflict" => {
                    view.enqueue_application_dialog(ApplicationDialog::FilesystemConflict {
                        path: Some(workspace.path.clone()),
                        detail: "External change".into(),
                    })
                }
                _ => {}
            }
            view.handle_application_dialog_action(
                if state == "cancel" {
                    ApplicationDialogAction::Cancel
                } else {
                    ApplicationDialogAction::ImportEnvironment
                },
                window,
                cx,
            );
            assert!(view.environment_save_task.is_none());
            assert_eq!(view.shell.selected_environment(), Some("development"));
            assert_no_leak(view);
        });
        cx.run_until_parked();
        assert_eq!(workspace.store.get_calls(), 0);
        assert!(workspace.store.values.lock().unwrap().is_empty());
        assert_eq!(workspace.yaml(), before);
        assert!(
            !load_workspace(&workspace.path)
                .unwrap()
                .workspace()
                .environments()
                .iter()
                .any(|environment| environment.name == "Imported")
        );
    }
}

#[gpui_kit::test]
fn postman_environment_import_surfaces_compatibility_warnings_without_disclosing_values(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "postman-import-diagnostics");
    workspace.open_manager(cx, "development");
    let before = workspace.yaml();
    let mut source: serde_json::Value = serde_json::from_str(&export("Imported")).unwrap();
    source["values"][0]["description"] = serde_json::json!("PRIVATE_DESCRIPTION_SENTINEL");
    source["values"][1]["value"] = serde_json::json!("{{$randomUUID}}");
    choose_source(&workspace, cx, &source.to_string());
    workspace.update(cx, |view, window, cx| {
        let detail = view.application_dialog.as_ref().unwrap().description();
        assert!(detail.contains("descriptions cannot be represented"));
        assert!(detail.contains("dynamic variables"));
        assert!(!detail.contains(SECRET_SENTINEL));
        assert!(!detail.contains("PRIVATE_DESCRIPTION_SENTINEL"));
        if let Some(ApplicationDialog::ConfirmPostmanEnvironmentImport { import, .. }) =
            &view.application_dialog
        {
            assert!(!format!("{import:?} {:?}", import.environment).contains(SECRET_SENTINEL));
        }
        view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
    });
    assert_eq!(workspace.yaml(), before);
}

#[gpui_kit::test]
fn postman_environment_import_preserves_successful_credential_writes_on_partial_failure(
    cx: &mut TestAppContext,
) {
    for (label, fail_all, fail_lookup) in [
        ("partial", false, false),
        ("all", true, false),
        ("lookup", false, true),
    ] {
        let workspace = EnvironmentWorkspace::writable(
            cx,
            &format!("postman-import-credential-failure-{label}"),
        );
        workspace.open_manager(cx, "development");
        let mut source: serde_json::Value =
            serde_json::from_str(&export_with_warning("Imported")).unwrap();
        source["values"].as_array_mut().unwrap().push(serde_json::json!({"key": "other-token", "value": "SECOND_SECRET_VALUE", "type": "secret"}));
        let first_id = workspace.credential("Imported", "token");
        let other_id = workspace.credential("Imported", "other-token");
        workspace.store.fail_set.store(fail_all, Ordering::SeqCst);
        workspace.update(cx, |view, _, _| {
            view.credential_store = Arc::new(AuditedImportStore {
                inner: workspace.store.clone(),
                path: workspace.path.clone(),
                environment: "Imported",
                fail: Some(other_id.clone()),
                fail_lookup,
            })
        });
        choose_source(&workspace, cx, &source.to_string());
        assert_eq!(
            workspace.store.get_calls(),
            0,
            "confirmation must precede credential access"
        );
        confirm_import(&workspace, cx);
        assert_eq!(
            workspace.store.value(&first_id).as_deref(),
            if fail_all || fail_lookup {
                None
            } else {
                Some(SECRET_SENTINEL)
            }
        );
        assert!(workspace.store.value(&other_id).is_none());
        assert!(!workspace.yaml().contains(SECRET_SENTINEL));
        assert!(!workspace.yaml().contains("SECOND_SECRET_VALUE"));
        assert!(
            load_workspace(&workspace.path)
                .unwrap()
                .workspace()
                .environments()
                .iter()
                .any(|env| env.name == "Imported")
        );
        workspace.update(cx, |view, _, _| {
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "partially completed"
            ));
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "Some secret values could not be imported"
            ));
            assert!(!has_active_toast(
                view,
                ToastIntent::Warning,
                "remain in use"
            ));
            assert_eq!(
                presence(view, &first_id),
                if fail_lookup { None } else { Some(!fail_all) }
            );
            assert_eq!(
                presence(view, &other_id),
                if fail_lookup { None } else { Some(false) }
            );
            assert_eq!(
                manager_status(view, "token"),
                Some(if fail_lookup {
                    SecretUiStatus::Unknown
                } else if fail_all {
                    SecretUiStatus::NotStored
                } else {
                    SecretUiStatus::Stored
                })
            );
            assert_eq!(
                manager_status(view, "other-token"),
                Some(if fail_lookup {
                    SecretUiStatus::Unknown
                } else {
                    SecretUiStatus::NotStored
                })
            );
            assert_no_leak(view);
        });
    }
}

#[gpui_kit::test]
fn postman_environment_import_declarations_only_preserve_presence_without_warning(
    cx: &mut TestAppContext,
) {
    for fail_lookup in [false, true] {
        let workspace = EnvironmentWorkspace::writable(cx, "postman-import-declarations-only");
        workspace.open_manager(cx, "development");
        let id = workspace.credential("Imported", "token");
        workspace.store.set(&id, SECRET_SENTINEL).unwrap();
        workspace.update(cx, |view, _, _| {
            view.credential_store = Arc::new(AuditedImportStore {
                inner: workspace.store.clone(),
                path: workspace.path.clone(),
                environment: "Imported",
                fail: None,
                fail_lookup,
            });
        });
        let mut source: serde_json::Value = serde_json::from_str(&export("Imported")).unwrap();
        source["values"][0]["value"] = serde_json::json!("");
        source["values"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"key": "null-secret", "value": null, "type": "secret"}));
        choose_source(&workspace, cx, &source.to_string());
        assert_eq!(workspace.store.value(&id).as_deref(), Some(SECRET_SENTINEL));
        workspace.update(cx, |view, _, _| {
            assert_eq!(
                presence(view, &id),
                if fail_lookup { None } else { Some(true) }
            );
            assert_eq!(
                presence(view, &workspace.credential("Imported", "null-secret")),
                if fail_lookup { None } else { Some(false) }
            );
            assert!(!has_active_toast(
                view,
                ToastIntent::Warning,
                "partially completed"
            ));
            assert_no_leak(view);
        });
    }
}

#[gpui_kit::test]
fn postman_environment_import_never_replaces_existing_credentials(cx: &mut TestAppContext) {
    for mixed in [false, true] {
        let workspace = EnvironmentWorkspace::writable(cx, "postman-import-existing-credentials");
        workspace.open_manager(cx, "development");
        let existing = workspace.credential("development", "token");
        let orphan = workspace.credential("Imported", "token");
        workspace.store.set(&existing, "existing-value").unwrap();
        workspace.store.set(&orphan, "orphan-value").unwrap();
        choose_source(&workspace, cx, &export("development"));
        assert_eq!(workspace.store.get_calls(), 0);
        assert_eq!(
            workspace.store.value(&existing).as_deref(),
            Some("existing-value")
        );
        let mut source: serde_json::Value = serde_json::from_str(&export("Imported")).unwrap();
        if mixed {
            source["values"].as_array_mut().unwrap().push(serde_json::json!({"key": "other-token", "value": "SECOND_SECRET_VALUE", "type": "secret"}));
            workspace.store.fail_set.store(true, Ordering::SeqCst);
        }
        choose_source(&workspace, cx, &source.to_string());
        assert_eq!(
            workspace.store.value(&orphan).as_deref(),
            Some("orphan-value")
        );
        workspace.update(cx, |view, _, _| {
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "Existing stored credentials remain in use; their Postman values were not imported."
            ));
            assert_eq!(
                has_active_toast(
                    view,
                    ToastIntent::Warning,
                    "Some other secret values could not be imported"
                ),
                mixed
            );
            assert_eq!(
                view.toasts
                    .iter()
                    .filter(|(_, toast, _)| toast.intent == ToastIntent::Warning)
                    .count(),
                1
            );
            assert!(
                !toast_debug(view)
                    .iter()
                    .any(|message| message.contains("orphan-value")
                        || message.contains("existing-value"))
            );
            if mixed {
                assert_eq!(
                    presence(view, &workspace.credential("Imported", "other-token")),
                    Some(false)
                );
            }
            assert_eq!(manager_status(view, "token"), Some(SecretUiStatus::Stored));
            assert_eq!(presence(view, &orphan), Some(true));
            assert_no_leak(view);
        });
    }
}

#[gpui_kit::test]
fn postman_environment_import_rechecks_workspace_after_yaml_before_credentials(
    cx: &mut TestAppContext,
) {
    for values in ["nonempty", "empty", "null", "empty-then-nonempty"] {
        for change in ["switch", "reload", "conflict"] {
            let workspace =
                EnvironmentWorkspace::writable(cx, &format!("postman-import-after-yaml-{change}"));
            let other =
                EnvironmentWorkspace::writable(cx, &format!("postman-import-other-{change}"));
            workspace.open_manager(cx, "development");
            let (release, gate) = tokio::sync::oneshot::channel();
            workspace.update(cx, |view, _, _| {
                view.environment_import_credentials_gate = Some(gate)
            });
            let mut source: serde_json::Value = serde_json::from_str(&export("Imported")).unwrap();
            if values != "nonempty" {
                source["values"][0]["value"] = if values == "null" {
                    serde_json::Value::Null
                } else {
                    serde_json::json!("")
                };
            }
            if values == "empty-then-nonempty" {
                source["values"].as_array_mut().unwrap().push(serde_json::json!({"key": "other-token", "value": "SECOND_SECRET_VALUE", "type": "secret"}));
            }
            choose_source(&workspace, cx, &source.to_string());
            assert!(
                load_workspace(&workspace.path)
                    .unwrap()
                    .workspace()
                    .environments()
                    .iter()
                    .any(|env| env.name == "Imported")
            );
            assert_eq!(workspace.store.get_calls(), 1);
            workspace.update(cx, |view, _, cx| {
                assert!(view.environment_save_task.is_some());
                assert!(view.secret_write_in_progress);
                match change {
                    "switch" => {
                        view.set_workspace(other.path.clone(), load_workspace(&other.path).unwrap())
                    }
                    "reload" => {
                        view.loaded_workspace = Some(load_workspace(&workspace.path).unwrap())
                    }
                    _ => view.enqueue_application_dialog(ApplicationDialog::FilesystemConflict {
                        path: Some(workspace.path.clone()),
                        detail: "External change".into(),
                    }),
                }
                cx.notify();
            });
            release.send(()).unwrap();
            cx.run_until_parked();
            assert!(workspace.store.values.lock().unwrap().is_empty());
            assert_eq!(workspace.store.get_calls(), 1);
            assert!(!workspace.yaml().contains(SECRET_SENTINEL));
            assert!(!other.yaml().contains(SECRET_SENTINEL));
            workspace.update(cx, |view, _, _| {
                assert_eq!(
                    has_active_toast(
                        view,
                        ToastIntent::Warning,
                        "Some secret values could not be imported"
                    ),
                    values == "nonempty" || values == "empty-then-nonempty"
                );
                assert_eq!(
                    presence(view, &workspace.credential("Imported", "token")),
                    None
                );
                assert!(!view.secret_write_in_progress);
                assert!(view.environment_save_task.is_none());
                if change == "switch" {
                    assert!(view.environment_manager_dialog.is_none());
                    assert_eq!(presence(view, &other.credential("Imported", "token")), None);
                }
                assert_no_leak(view);
            });
        }
    }
}
