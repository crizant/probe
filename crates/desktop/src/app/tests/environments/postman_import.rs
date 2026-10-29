use super::*;
use probe_opencollection::load_workspace;

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

#[gpui_kit::test]
fn postman_environment_import_persists_complete_environment_without_activating(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "postman-import-success");
    workspace.open_manager(cx, "development");
    let before = load_workspace(&workspace.path)
        .unwrap()
        .workspace()
        .environments()
        .to_vec();
    let source = export("Imported");
    let before_yaml = workspace.yaml();
    choose_source(&workspace, cx, &source);
    assert_eq!(workspace.yaml(), before_yaml);
    workspace.update(cx, |view, _, _| {
        let dialog = view.application_dialog.as_ref().unwrap();
        let detail = dialog.description();
        assert!(detail.contains("secret values"));
        assert!(detail.contains("plaintext"));
        assert!(detail.contains("YAML"));
        assert!(detail.contains("not be stored in Probe's credential store"));
        assert!(!detail.contains(SECRET_SENTINEL));
        assert!(view.environment_save_task.is_none());
    });
    confirm_import(&workspace, cx);
    let reloaded = load_workspace(&workspace.path).unwrap();
    let environments = reloaded.workspace().environments();
    assert_eq!(&environments[..before.len()], before.as_slice());
    assert_eq!(
        environments.last().unwrap(),
        &probe_postman::import_postman_environment(&source)
            .unwrap()
            .environment
    );
    workspace.update(cx, |view, _, _| {
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .original_name,
            "Imported"
        );
        assert_eq!(view.shell.selected_environment(), Some("development"));
        assert_eq!(
            view.session.selected_environment_for(&workspace.path),
            Some("development")
        );
        assert_no_leak(view);
    });
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
        if label != "invalid" {
            confirm_import(&workspace, cx);
        }
        assert_eq!(workspace.yaml(), before);
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
        view.import_postman_environment(
            probe_postman::import_postman_environment(&export("Imported"))
                .unwrap()
                .environment,
            window,
            cx,
        );
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
        view.import_postman_environment(
            probe_postman::import_postman_environment(&export("Imported"))
                .unwrap()
                .environment,
            window,
            cx,
        );
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
        choose_source(&workspace, cx, &export("Imported"));
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
fn postman_environment_import_surfaces_compatibility_warnings_without_secrets(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "postman-import-diagnostics");
    workspace.open_manager(cx, "development");
    let before = workspace.yaml();
    let mut source: serde_json::Value = serde_json::from_str(&export("Imported")).unwrap();
    source["values"][0]["type"] = serde_json::json!("default");
    source["values"][0]["description"] = serde_json::json!("PRIVATE_DESCRIPTION_SENTINEL");
    source["values"][1]["value"] = serde_json::json!("{{$randomUUID}}");
    choose_source(&workspace, cx, &source.to_string());
    workspace.update(cx, |view, window, cx| {
        let detail = view.application_dialog.as_ref().unwrap().description();
        assert!(detail.contains("descriptions cannot be represented"));
        assert!(detail.contains("dynamic variables"));
        assert!(!detail.contains(SECRET_SENTINEL));
        assert!(!detail.contains("PRIVATE_DESCRIPTION_SENTINEL"));
        view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
    });
    assert_eq!(workspace.yaml(), before);
}
