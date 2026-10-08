use super::*;
use gpui::Focusable as _;
use probe_core::VariableStatus::{Resolved, SecretWithoutValue};
use std::path::Path;

fn renders(visual: &mut VisualTestContext, selector: &'static str) -> bool {
    visual.debug_bounds(selector).is_some()
}

fn set_manager_status(
    workspace: &EnvironmentWorkspace,
    cx: &mut TestAppContext,
    status: SecretUiStatus,
) {
    workspace.update(cx, |view, _, cx| {
        view.environment_manager_dialog
            .as_mut()
            .unwrap()
            .secret_statuses
            .insert("secretToken".into(), status);
        cx.notify();
    });
}

fn secret_input_is_focused(view: &ProbeApp, window: &Window, cx: &gpui::App) -> bool {
    view.secret_value_dialog
        .as_ref()
        .unwrap()
        .input
        .read(cx)
        .focus_handle(cx)
        .is_focused(window)
}

#[gpui::test]
fn manager_secret_status_comes_from_presence_for_the_effective_environment(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.mark_stored(cx, &workspace.development_secret());
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, _, cx| {
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Stored)
        );
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        let defined_in = view
            .loaded_workspace
            .as_ref()
            .unwrap()
            .workspace()
            .effective_environment_variables(dialog.draft())
            .into_iter()
            .find(|row| matches!(&row.variable, EnvironmentVariable::Secret(secret) if secret.name.as_deref() == Some("secretToken")))
            .unwrap()
            .defined_in;
        assert_eq!(
            defined_in, "base",
            "the inherited declaration keeps its source"
        );
        cx.notify();
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, cx| {
        view.select_environment_manager_environment("base", cx)
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .original_name,
            "base"
        );
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Unknown),
            "presence is scoped to the effective environment"
        );
    });
}

#[gpui::test]
fn manager_secret_row_actions_follow_presence_and_saved_state(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "base");
    let mut visual = workspace.visual(cx);
    assert!(renders(&mut visual, "environment-secret-set-secretToken"));
    assert!(!renders(
        &mut visual,
        "environment-secret-replace-secretToken"
    ));
    assert!(
        !renders(&mut visual, "environment-secret-delete-secretToken"),
        "deleting a stored value is offered only inside the Replace dialog"
    );

    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        assert!(!view.secret_value_dialog.as_ref().unwrap().replacing);
        assert!(secret_input_is_focused(view, window, cx));
    });
    visual.run_until_parked();
    assert!(renders(&mut visual, "secret-value-identity"));
    assert!(!renders(&mut visual, "secret-value-delete-stored"));
    workspace.update(cx, |view, window, cx| {
        view.close_secret_value_dialog(window, cx)
    });

    set_manager_status(&workspace, cx, SecretUiStatus::Stored);
    visual.run_until_parked();
    assert!(renders(
        &mut visual,
        "environment-secret-replace-secretToken"
    ));
    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        assert!(view.secret_value_dialog.as_ref().unwrap().replacing);
        assert!(secret_input_is_empty(view, cx));
    });
    visual.run_until_parked();
    assert!(renders(&mut visual, "secret-value-delete-stored"));

    set_manager_status(&workspace, cx, SecretUiStatus::NotStored);
    workspace.update(cx, |view, window, cx| {
        view.confirm_delete_stored_secret("secretToken".into(), window, cx);
        assert!(
            view.application_dialog.is_none(),
            "a value that is no longer stored cannot be deleted"
        );
    });
    visual.run_until_parked();
    assert!(!renders(&mut visual, "secret-value-delete-stored"));
    set_manager_status(&workspace, cx, SecretUiStatus::Stored);
    visual.run_until_parked();
    assert!(renders(&mut visual, "secret-value-delete-stored"));
    workspace.update(cx, |view, window, cx| {
        assert!(secret_input_is_focused(view, window, cx));
        view.close_secret_value_dialog(window, cx);
    });

    workspace.update(cx, |view, _, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft_mut().color = Some("#abcdef".into())
        });
    });
    visual.run_until_parked();
    assert!(
        !renders(&mut visual, "environment-secret-replace-secretToken")
            && !renders(&mut visual, "environment-secret-set-secretToken"),
        "credential actions are disabled while the environment draft is unsaved"
    );
    assert!(renders(
        &mut visual,
        "environment-secret-status-secretToken"
    ));
}

#[gpui::test]
fn manager_new_secret_must_be_saved_before_credential_can_be_set(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "new-secret");
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog
                .draft_mut()
                .variables
                .push(EnvironmentVariable::Secret(SecretVariable {
                    name: Some(String::new()),
                    value_type: None,
                    disabled: false,
                }))
        });
        view.save_environment_manager_dialog(window, cx);
        assert!(
            view.environment_save_task.is_none(),
            "blank secret names must be rejected"
        );
        assert!(!view.environment_manager_draft_has_required_names());
        view.apply_environment_manager_draft(cx, |dialog| {
            if let Some(EnvironmentVariable::Secret(secret)) =
                dialog.draft_mut().variables.last_mut()
            {
                secret.name = Some("newToken".into());
            }
        });
        assert!(!view.can_manage_secret("newToken"));
        view.open_secret_value_dialog("newToken".into(), window, cx);
        assert!(view.secret_value_dialog.is_none());
        view.save_environment_manager_dialog(window, cx);
    });
    cx.run_until_parked();
    let yaml = workspace.yaml();
    assert!(yaml.contains("secret: true"));
    assert!(yaml.contains("newToken"), "{yaml}");

    workspace.update(cx, |view, window, cx| {
        assert!(view.can_manage_secret("newToken"));
        view.open_secret_value_dialog("newToken".into(), window, cx);
        assert!(secret_input_is_empty(view, cx));
        type_secret(view, window, cx, SECRET_SENTINEL);
        assert!(!format!("{:?}", view.session).contains(SECRET_SENTINEL));
        assert!(
            !serde_json::to_string(&view.session)
                .unwrap()
                .contains(SECRET_SENTINEL)
        );
        view.save_secret_value(window, cx);
        assert!(
            secret_input_is_empty(view, cx),
            "the input is cleared as soon as the value is submitted"
        );
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(view.secret_value_dialog.is_none());
        assert_eq!(
            manager_status(view, "newToken"),
            Some(SecretUiStatus::Stored)
        );
    });
    assert_eq!(
        workspace
            .store
            .value(&workspace.credential("development", "newToken"))
            .as_deref(),
        Some(SECRET_SENTINEL)
    );
    assert!(!workspace.yaml().contains(SECRET_SENTINEL));
}

#[gpui::test]
fn manager_replaces_and_deletes_native_value_without_changing_declaration(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "replace-delete-secret");
    let before = workspace.yaml();
    let id = workspace.development_secret();
    workspace.store_secret(cx, &id, "old-value");
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Stored)
        );
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        assert!(
            secret_input_is_empty(view, cx),
            "Replace must never prefill the old value"
        );
        type_secret(view, window, cx, SECRET_SENTINEL);
        view.save_secret_value(window, cx);
    });
    cx.run_until_parked();
    assert_eq!(workspace.store.value(&id).as_deref(), Some(SECRET_SENTINEL));
    assert_eq!(workspace.yaml(), before);

    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        view.confirm_delete_stored_secret("secretToken".into(), window, cx);
        assert!(matches!(
            view.application_dialog,
            Some(ApplicationDialog::DeleteStoredSecret { .. })
        ));
        assert!(view.secret_value_dialog.is_some());
    });
    set_manager_status(&workspace, cx, SecretUiStatus::NotStored);
    workspace.update(cx, |view, window, cx| {
        view.handle_application_dialog_action(ApplicationDialogAction::Delete, window, cx);
        assert!(
            view.secret_value_dialog.is_some() && !view.secret_write_in_progress,
            "confirmation must not delete a value that stopped being stored"
        );
    });
    set_manager_status(&workspace, cx, SecretUiStatus::Stored);
    workspace.update(cx, |view, window, cx| {
        view.confirm_delete_stored_secret("secretToken".into(), window, cx);
        view.handle_application_dialog_action(ApplicationDialogAction::Delete, window, cx);
    });
    cx.run_until_parked();
    assert!(workspace.store.value(&id).is_none());
    workspace.update(cx, |view, window, cx| {
        assert!(view.secret_value_dialog.is_none());
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::NotStored)
        );
        view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
    });
    cx.run_until_parked();
    assert_eq!(workspace.yaml(), before);
}

#[gpui::test]
fn manager_trash_removes_declaration_without_deleting_native_value(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "remove-secret-declaration");
    let id = workspace.credential("base", "secretToken");
    workspace.store.set(&id, "private").unwrap();
    workspace.open_manager(cx, "base");
    let is_token = |variable: &EnvironmentVariable| matches!(variable, EnvironmentVariable::Secret(secret) if secret.name.as_deref() == Some("secretToken"));

    let mut visual = workspace.visual(cx);
    let trash = visual
        .debug_bounds("environment-variable-delete-direct-base-4")
        .unwrap();
    visual.simulate_click(trash.center(), Modifiers::default());
    visual.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        let draft = &view.environment_manager_dialog.as_ref().unwrap().draft();
        assert!(!draft.variables.iter().any(is_token));
        view.save_environment_manager_dialog(window, cx);
    });
    cx.run_until_parked();

    let saved = probe_opencollection::load_workspace(&workspace.path).unwrap();
    let base = saved
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == "base")
        .unwrap();
    assert!(!base.variables.iter().any(is_token));
    assert_eq!(workspace.store.value(&id).as_deref(), Some("private"));
}

#[gpui::test]
fn manager_set_failure_reports_error_without_recording_presence(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "secret-failure");
    let before = workspace.yaml();
    workspace.store.fail_set.store(true, Ordering::SeqCst);
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Unknown)
        );
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        type_secret(view, window, cx, SECRET_SENTINEL);
        view.save_secret_value(window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        let dialog = view.secret_value_dialog.as_ref().unwrap();
        assert!(dialog.error.is_some() && !dialog.busy);
        assert!(
            secret_input_is_empty(view, cx),
            "a failed write must not keep the value in the input"
        );
        view.close_secret_value_dialog(window, cx);
        assert_eq!(presence(view, &workspace.development_secret()), None);
        assert_unknown_secret(&view.variable_context(cx), "secretToken");
    });
    assert_eq!(workspace.yaml(), before);
}

#[gpui::test]
fn manager_renames_change_credential_identity_without_migration(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "secret-identity-rename");
    let old_id = workspace.development_secret();
    workspace.store.set(&old_id, "private").unwrap();
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft_mut().name = "production".into()
        });
        assert!(!view.can_manage_secret("secretToken"));
        view.save_environment_manager_dialog(window, cx);
        assert!(matches!(
            view.application_dialog,
            Some(ApplicationDialog::RenameStoredSecrets {
                kind: StoredSecretRename::Environment,
            })
        ));
        assert!(view.environment_save_task.is_none());
        view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
        assert!(view.application_dialog.is_none());
        assert!(view.environment_save_task.is_none());
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .draft()
                .name,
            "production"
        );
    });
    let yaml = workspace.yaml();
    assert!(
        yaml.contains("name: development") && !yaml.contains("name: production"),
        "cancel must leave the saved environment name unchanged"
    );

    workspace.update(cx, |view, window, cx| {
        view.save_environment_manager_dialog(window, cx);
        view.handle_application_dialog_action(ApplicationDialogAction::Rename, window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .draft()
                .name,
            "production"
        );
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Unknown)
        );
    });
    assert_eq!(workspace.store.value(&old_id).as_deref(), Some("private"));
    assert!(
        workspace
            .store
            .value(&workspace.credential("production", "secretToken"))
            .is_none(),
        "credentials are not migrated to the new identity"
    );

    workspace.update(cx, |view, window, cx| {
        view.select_environment_manager_environment("base", cx);
        view.apply_environment_manager_draft(cx, |dialog| {
            for variable in &mut dialog.draft_mut().variables {
                if let EnvironmentVariable::Secret(secret) = variable
                    && secret.name.as_deref() == Some("secretToken")
                {
                    secret.name = Some("renamedToken".into());
                }
            }
        });
        assert!(!view.can_manage_secret("renamedToken"));
        view.save_environment_manager_dialog(window, cx);
        let dialog = view.application_dialog.as_ref().unwrap();
        assert!(matches!(
            dialog,
            ApplicationDialog::RenameStoredSecrets {
                kind: StoredSecretRename::Variable { from, to },
            } if from == "secretToken" && to == "renamedToken"
        ));
        let description = dialog.description();
        assert!(description.contains("secretToken") && description.contains("renamedToken"));
        view.handle_application_dialog_action(ApplicationDialogAction::Rename, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(workspace.store.value(&old_id).as_deref(), Some("private"));
    assert!(workspace.yaml().contains("renamedToken"));
}

#[gpui::test]
fn environment_manager_close_save_confirms_secret_rename_before_closing(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "secret-rename-close");
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft_mut().name = "production".into()
        });
        view.request_close_environment_manager_dialog(window, cx);
        view.handle_application_dialog_action(ApplicationDialogAction::Save, window, cx);
        assert!(matches!(
            view.application_dialog,
            Some(ApplicationDialog::RenameStoredSecrets {
                kind: StoredSecretRename::Environment,
            })
        ));
        assert!(view.environment_manager_close_after_save);
        assert!(view.environment_manager_dialog.is_some());
        view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
        assert!(!view.environment_manager_close_after_save);
        assert!(view.environment_manager_dialog.is_some());
        assert!(view.application_dialog.is_none());
        view.request_close_environment_manager_dialog(window, cx);
        view.handle_application_dialog_action(ApplicationDialogAction::Save, window, cx);
        view.handle_application_dialog_action(ApplicationDialogAction::Rename, window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(view.environment_manager_dialog.is_none());
        assert!(view.application_dialog.is_none());
    });
    assert!(workspace.yaml().contains("name: production"));
}

#[gpui::test]
fn in_flight_set_survives_closing_its_dialog_and_the_manager(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        let input = view.secret_value_dialog.as_ref().unwrap().input.clone();
        type_secret(view, window, cx, "stored-after-close");
        view.save_secret_value(window, cx);
        assert!(view.secret_write_in_progress);
        assert!(view.secret_value_dialog.as_ref().unwrap().busy);

        view.close_secret_value_dialog(window, cx);
        assert!(view.secret_value_dialog.is_none());
        assert!(input.read(cx).value().is_empty());
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        assert!(
            view.secret_value_dialog.is_none(),
            "a second write must stay blocked until the first finishes"
        );
        view.close_environment_manager_dialog(window, cx);
        assert_eq!(presence(view, &id), None);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        assert!(!view.secret_write_in_progress);
        assert_eq!(presence(view, &id), Some(true));
        assert_eq!(view.variable_context(cx).status("secretToken"), Resolved);
    });
    assert_eq!(
        workspace.store.value(&id).as_deref(),
        Some("stored-after-close")
    );

    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Stored)
        );
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        assert!(view.secret_value_dialog.as_ref().unwrap().replacing);
        assert!(secret_input_is_empty(view, cx));
    });
}

#[gpui::test]
fn manager_secret_keyboard_enter_submits_and_escape_discards(cx: &mut TestAppContext) {
    cx.update(bind_platform_hotkeys);
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        type_secret(view, window, cx, "discard-me");
    });
    cx.simulate_keystrokes(workspace.window.into(), "escape");
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        assert!(view.secret_value_dialog.is_none());
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        assert!(secret_input_is_empty(view, cx));
        type_secret(view, window, cx, "saved-by-enter");
    });
    cx.simulate_keystrokes(workspace.window.into(), "enter");
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| assert!(view.secret_value_dialog.is_none()));
    assert!(
        workspace
            .store
            .value(&id)
            .is_some_and(|value| value == "saved-by-enter")
    );

    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        assert!(view.secret_value_dialog.as_ref().unwrap().replacing);
        type_secret(view, window, cx, "replaced-by-enter");
        view.environment_manager_dialog_focus.focus(window, cx);
    });
    cx.simulate_keystrokes(workspace.window.into(), "enter");
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| assert!(view.secret_value_dialog.is_none()));
    assert_eq!(
        workspace.store.value(&id).as_deref(),
        Some("replaced-by-enter"),
        "Enter submits the secret dialog even when focus is on the manager behind it"
    );
}

#[gpui::test]
fn manager_secret_dialog_cannot_save_after_draft_becomes_dirty(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        type_secret(view, window, cx, "must-not-write");
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog
                .draft_mut()
                .variables
                .push(EnvironmentVariable::Plain(Variable {
                    name: Some("newPlain".into()),
                    value: None,
                    disabled: false,
                }));
        });
        assert!(!view.can_manage_secret("secretToken"));
        view.save_secret_value(window, cx);
        assert!(!view.secret_value_dialog.as_ref().unwrap().busy);
        view.close_secret_value_dialog(window, cx);
        assert!(view.environment_manager_dialog_focus.is_focused(window));
    });
    cx.run_until_parked();
    assert!(workspace.store.values.lock().unwrap().is_empty());
}

#[gpui::test]
fn manager_delete_failure_restores_status_and_reports_error(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    workspace.store_secret(cx, &id, "keep-me");
    workspace.store.fail_delete.store(true, Ordering::SeqCst);
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Loading)
        );
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(has_active_toast(
            view,
            ToastIntent::Error,
            "Could not delete from the system credential store."
        ));
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Stored)
        );
        assert_eq!(presence(view, &id), Some(true));
    });
    assert_eq!(workspace.store.value(&id).as_deref(), Some("keep-me"));
}

fn open_editor_secret_dialog(
    view: &mut ProbeApp,
    workspace: &Path,
    window: &mut Window,
    cx: &mut Context<ProbeApp>,
) {
    view.open_editor_secret_value_dialog(
        workspace.to_path_buf(),
        "development".into(),
        "secretToken".into(),
        view.focus_handle.clone(),
        window,
        cx,
    );
}

#[gpui::test]
fn editor_delete_failure_reports_error_without_changing_presence(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    workspace.store_secret(cx, &id, "keep-me");
    workspace.store.fail_delete.store(true, Ordering::SeqCst);
    workspace.select_environment(cx, "development");
    workspace.update(cx, |view, window, cx| {
        open_editor_secret_dialog(view, &workspace.path, window, cx);
        assert!(view.environment_manager_dialog.is_none());
        view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(has_active_toast(
            view,
            ToastIntent::Error,
            "Could not delete from the system credential store."
        ));
        assert_eq!(presence(view, &id), Some(true));
    });
    assert_eq!(workspace.store.value(&id).as_deref(), Some("keep-me"));
}

#[gpui::test]
fn editor_secret_dialog_rejects_stale_environment_and_workspace(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let other = http_environment_fixture().canonicalize().unwrap();
    let fixture = workspace.path.clone();
    workspace.select_environment(cx, "development");
    workspace.update(cx, |view, window, cx| {
        open_editor_secret_dialog(view, &fixture, window, cx);
        type_secret(view, window, cx, "should-not-write");
        view.select_environment(Some("base".into()), cx);
        view.save_secret_value(window, cx);
        assert!(!view.secret_value_dialog.as_ref().unwrap().busy);
        view.select_environment(Some("development".into()), cx);
        view.workspace_path = Some(other.clone());
        view.save_secret_value(window, cx);
        assert!(!view.secret_value_dialog.as_ref().unwrap().busy);
        view.workspace_path = Some(fixture.clone());
        view.close_secret_value_dialog(window, cx);

        view.workspace_path = Some(other.clone());
        open_editor_secret_dialog(view, &fixture, window, cx);
        assert!(view.secret_value_dialog.is_none());
        view.workspace_path = Some(fixture.clone());
    });
    cx.run_until_parked();
    assert!(workspace.store.values.lock().unwrap().is_empty());
}

#[gpui::test]
fn editor_replace_dialog_rejects_mismatched_and_stale_delete_targets(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    let fixture = workspace.path.clone();
    workspace.store_secret(cx, &id, "keep-me");
    workspace.select_environment(cx, "development");
    workspace.update(cx, |view, window, cx| {
        open_editor_secret_dialog(view, &fixture, window, cx);
        assert!(view.secret_value_dialog.as_ref().unwrap().replacing);
        view.confirm_delete_stored_secret("other".into(), window, cx);
        assert!(view.application_dialog.is_none());
        view.delete_stored_secret("other".into(), "development".into(), window, cx);
        view.delete_stored_secret("secretToken".into(), "base".into(), window, cx);
        assert!(!view.secret_write_in_progress);

        view.select_environment(Some("base".into()), cx);
        view.confirm_delete_stored_secret("secretToken".into(), window, cx);
        assert!(view.application_dialog.is_none());
        view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
        assert!(!view.secret_write_in_progress);
        view.select_environment(Some("development".into()), cx);
        view.close_secret_value_dialog(window, cx);

        let key = id.persistence_key();
        view.session.presence.stored_credentials.remove(key);
        view.session
            .presence
            .missing_credentials
            .insert(key.to_owned());
        open_editor_secret_dialog(view, &fixture, window, cx);
        assert!(!view.secret_value_dialog.as_ref().unwrap().replacing);
        view.session.presence.missing_credentials.remove(key);
        view.session
            .presence
            .stored_credentials
            .insert(key.to_owned());
        view.confirm_delete_stored_secret("secretToken".into(), window, cx);
        assert!(
            view.application_dialog.is_none() && view.secret_value_dialog.is_some(),
            "a Set dialog cannot become a Delete even if presence changes underneath it"
        );
        view.close_secret_value_dialog(window, cx);
    });
    cx.run_until_parked();
    assert_eq!(workspace.store.value(&id).as_deref(), Some("keep-me"));
}

#[gpui::test]
fn editor_secret_tooltip_sets_replaces_and_deletes_in_effective_environment(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::open(cx);
    cx.update(bind_platform_hotkeys);
    let id = workspace.development_secret();
    let request_key = workspace.update(cx, |view, _, cx| {
        let request_key = view.loaded_workspace.as_ref().unwrap().requests()[0].key();
        view.select_request(request_key, cx);
        view.select_environment(Some("development".into()), cx);
        view.edit_request(
            request_key,
            |request| request.url = Some("https://{{secretToken}}".into()),
            cx,
        );
        assert_unknown_secret(&view.variable_context(cx), "secretToken");
        request_key
    });
    cx.run_until_parked();

    let trigger = workspace
        .visual(cx)
        .debug_bounds("variable-hover-trigger")
        .unwrap()
        .center();
    let click_secret_action = |cx: &mut TestAppContext| {
        let mut visual = workspace.visual(cx);
        visual.simulate_mouse_move(point(px(1000.0), px(700.0)), None, Modifiers::default());
        visual.run_until_parked();
        hover_and_wait(cx, workspace.window, trigger);
        let mut visual = workspace.visual(cx);
        assert!(
            !renders(&mut visual, "variable-tooltip-value-input"),
            "secret tooltips never render a value input"
        );
        let action = visual
            .debug_bounds("variable-tooltip-secret-action")
            .expect("secret tooltip should offer Set or Replace");
        visual.simulate_click(action.center(), Modifiers::default());
        visual.run_until_parked();
    };

    click_secret_action(cx);
    workspace.update(cx, |view, window, cx| {
        let dialog = view.secret_value_dialog.as_ref().unwrap();
        assert!(!dialog.from_manager && !dialog.replacing);
        assert_eq!(dialog.target.environment, "development");
        assert_eq!(dialog.target.workspace, workspace.path);
        assert!(view.environment_manager_dialog.is_none());
        type_secret(view, window, cx, SECRET_SENTINEL);
        assert!(secret_input_is_focused(view, window, cx));
        assert!(view.execution.response(request_key).is_none());
    });
    workspace
        .visual(cx)
        .simulate_keystrokes(super::super::send_shortcut());
    workspace.update(cx, |view, window, cx| {
        assert!(
            view.execution.response(request_key).is_none(),
            "Send must be blocked while editing a secret outside Environment Manager"
        );
        assert!(secret_input_is_focused(view, window, cx));
        assert!(!secret_input_is_empty(view, cx));
        view.save_secret_value(window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        assert!(view.secret_value_dialog.is_none());
        let context = view.variable_context(cx);
        assert!(context.resolved_secrets.contains("secretToken"));
        assert_secret_stays_out_of_editor_context(&context);
    });
    assert_eq!(workspace.store.value(&id).as_deref(), Some(SECRET_SENTINEL));

    click_secret_action(cx);
    workspace.update(cx, |view, _, cx| {
        assert!(view.secret_value_dialog.as_ref().unwrap().replacing);
        assert!(secret_input_is_empty(view, cx));
        assert!(view.environment_manager_dialog.is_none());
    });
    cx.run_until_parked();
    assert!(renders(
        &mut workspace.visual(cx),
        "secret-value-delete-stored"
    ));
    workspace.update(cx, |view, window, cx| {
        view.confirm_delete_stored_secret("secretToken".into(), window, cx);
        assert!(matches!(
            view.application_dialog,
            Some(ApplicationDialog::DeleteStoredSecret { .. })
        ));
        view.handle_application_dialog_action(ApplicationDialogAction::Delete, window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        assert!(view.secret_value_dialog.is_none());
        assert!(view.variable_context(cx).secrets.contains("secretToken"));
    });
    assert!(workspace.store.value(&id).is_none());

    click_secret_action(cx);
    workspace.update(cx, |view, window, cx| {
        assert!(!view.secret_value_dialog.as_ref().unwrap().replacing);
        view.close_secret_value_dialog(window, cx);
    });
}

fn assert_delete_marks_placeholder_unresolved(cx: &mut TestAppContext, native: Option<&str>) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    if let Some(value) = native {
        workspace.store.set(&id, value).unwrap();
    }
    workspace.mark_stored(cx, &id);
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        assert_eq!(view.variable_context(cx).status("secretToken"), Resolved);
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
        assert_eq!(
            view.variable_context(cx).status("secretToken"),
            Resolved,
            "presence stays stored until the native delete finishes"
        );
        view.close_environment_manager_dialog(window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        assert_eq!(presence(view, &id), Some(false));
        let context = view.variable_context(cx);
        assert_eq!(context.status("secretToken"), SecretWithoutValue);
        assert_eq!(context.status("baseUrl"), Resolved);
        assert_secret_stays_out_of_editor_context(&context);
    });
    assert!(workspace.store.value(&id).is_none());
}

#[gpui::test]
fn deleting_a_stored_secret_marks_its_placeholder_unresolved(cx: &mut TestAppContext) {
    assert_delete_marks_placeholder_unresolved(cx, Some(SECRET_SENTINEL));
}

#[gpui::test]
fn deleting_an_already_absent_secret_still_marks_it_missing(cx: &mut TestAppContext) {
    assert_delete_marks_placeholder_unresolved(cx, None);
}

#[gpui::test]
fn secret_placeholder_status_follows_presence_without_reading_the_store(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "editor-secret-a");
    let other = writable_environment_fixture("editor-secret-b")
        .canonicalize()
        .unwrap();
    workspace.update(cx, |view, _, cx| {
        let request_key = view.loaded_workspace.as_ref().unwrap().requests()[0].key();
        view.select_request(request_key, cx);
        view.edit_request(
            request_key,
            |request| {
                request.headers = (0..8)
                    .map(|index| probe_core::Header {
                        name: format!("X-{index}"),
                        value: "{{secretToken}} {{token}}".to_owned(),
                        disabled: false,
                    })
                    .collect();
            },
            cx,
        );
        view.select_environment(Some("development".into()), cx);
        let context = view.variable_context(cx);
        assert_unknown_secret(&context, "secretToken");
        assert_eq!(context.status("token"), Resolved);
    });
    cx.run_until_parked();

    workspace.mark_stored(cx, &workspace.development_secret());
    workspace.update(cx, |view, _, cx| {
        let context = view.variable_context(cx);
        assert_eq!(context.status("secretToken"), Resolved);
        assert_secret_stays_out_of_editor_context(&context);
        cx.notify();
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, cx| {
        view.select_environment(Some("base".into()), cx);
        let base = view.variable_context(cx);
        assert_unknown_secret(&base, "secretToken");
        assert_eq!(base.status("host"), Resolved);

        view.select_environment(Some("development".into()), cx);
        assert_eq!(view.variable_context(cx).status("secretToken"), Resolved);

        let other_workspace = probe_opencollection::load_workspace(&other).unwrap();
        view.set_workspace(other.clone(), other_workspace);
        view.select_environment(Some("development".into()), cx);
        assert_unknown_secret(&view.variable_context(cx), "secretToken");
    });
    cx.run_until_parked();

    let reloaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
    workspace.update(cx, |view, _, cx| {
        view.set_workspace(workspace.path.clone(), reloaded);
        view.select_environment(Some("development".into()), cx);
        assert_eq!(view.variable_context(cx).status("secretToken"), Resolved);
    });
    cx.run_until_parked();
    assert_eq!(
        workspace.store.get_calls(),
        0,
        "rendering placeholders must not query the credential store"
    );
    fs::remove_file(other).unwrap();
}

#[gpui::test]
fn restored_session_presence_highlights_without_reading_the_store(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let session_store = crate::session::SessionStore::at(std::env::temp_dir().join(format!(
        "probe-presence-{unique}-{}/desktop-session.json",
        std::process::id()
    )));
    let mut saved = crate::session::SessionState::default();
    saved
        .presence
        .stored_credentials
        .insert(id.persistence_key().to_owned());
    session_store.save(&saved).unwrap();

    workspace.update(cx, |view, window, cx| {
        view.session_store = Some(session_store.clone());
        view.restore_session(window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        assert_eq!(presence(view, &id), Some(true));
        view.select_environment(Some("development".into()), cx);
        assert_eq!(view.variable_context(cx).status("secretToken"), Resolved);
        view.session_store = None;
    });
    let _ = fs::remove_dir_all(session_store.path().parent().unwrap());
}
