use super::*;

fn resolved_variable(view: &ProbeApp, name: &str) -> Option<String> {
    let environment = view.shell.selected_environment()?;
    probe_core::resolve_environment(
        view.loaded_workspace.as_ref()?.workspace().environments(),
        environment,
    )
    .ok()
    .and_then(|resolved| resolved.variable(name).map(str::to_owned))
}

fn saved_development_variable(workspace: &EnvironmentWorkspace, name: &str) -> Option<String> {
    let reloaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
    probe_core::resolve_environment(reloaded.workspace().environments(), "development")
        .unwrap()
        .variable(name)
        .map(str::to_owned)
}

fn select_first_request(
    workspace: &EnvironmentWorkspace,
    cx: &mut TestAppContext,
) -> probe_core::RequestKey {
    workspace.update(cx, |view, _, cx| {
        let key = view.loaded_workspace.as_ref().unwrap().requests()[0].key();
        view.select_request(key, cx);
        view.select_environment(Some("development".to_owned()), cx);
        key
    })
}

#[gpui_kit::test]
fn request_variables_render_inline_and_show_resolved_tooltips(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "tooltip");
    select_first_request(&workspace, cx);
    cx.run_until_parked();

    let (variable_point, input_point) = {
        let mut visual = workspace.visual(cx);
        let variable = visual
            .debug_bounds("variable-hover-trigger")
            .expect("variable hover trigger should render");
        let url_input = visual
            .debug_bounds("request-url-input")
            .expect("request URL input should render");
        (variable.center(), url_input.center())
    };
    hover_and_wait(cx, workspace.window, variable_point);
    let popup_point = workspace
        .visual(cx)
        .debug_bounds("variable-input-tooltip-popup")
        .expect("hovered variable tooltip should render")
        .center();
    hover_and_wait(cx, workspace.window, popup_point);
    {
        let mut visual = workspace.visual(cx);
        assert!(
            visual
                .debug_bounds("variable-input-tooltip-popup")
                .is_some(),
            "tooltip should stay visible while moving from the variable onto it"
        );
        let value_input = visual
            .debug_bounds("variable-tooltip-value-input")
            .expect("tooltip value input should render");
        visual.simulate_click(value_input.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(
            visual
                .debug_bounds("variable-input-tooltip-popup")
                .is_some(),
            "tooltip should stay visible while interacting with its value field"
        );
    }
    workspace.update(cx, |view, window, cx| {
        view.update_environment_variable(
            "baseUrl",
            "https://changed.example".to_owned(),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let updated = workspace.update(cx, |view, _, _| resolved_variable(view, "baseUrl"));
    assert_eq!(updated.as_deref(), Some("https://changed.example"));
    assert_eq!(
        saved_development_variable(&workspace, "baseUrl").as_deref(),
        Some("https://changed.example")
    );

    let button_point = workspace
        .visual(cx)
        .debug_bounds("variable-tooltip-manage-environments")
        .expect("tooltip should include Manage environments")
        .center();
    hover_and_wait(cx, workspace.window, button_point);
    {
        let mut visual = workspace.visual(cx);
        visual.simulate_click(button_point, Modifiers::default());
        visual.run_until_parked();
    }
    cx.run_until_parked();
    {
        let mut visual = workspace.visual(cx);
        visual
            .debug_bounds("environment-manager-dialog")
            .expect("clicking Manage environments should open the environment manager");
        assert!(
            visual
                .debug_bounds("variable-input-tooltip-popup")
                .is_none(),
            "opening the environment manager should dismiss the variable tooltip"
        );
    }
    workspace.update(cx, |view, window, cx| {
        view.request_close_environment_manager_dialog(window, cx)
    });
    cx.run_until_parked();
    {
        let mut visual = workspace.visual(cx);
        visual.simulate_click(input_point, Modifiers::default());
        visual.run_until_parked();
    }
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    cx.simulate_keystrokes(workspace.window.into(), select_all);
    cx.simulate_input(workspace.window.into(), "https://url.example");
    cx.run_until_parked();
    let edited_url = workspace.update(cx, |view, _, _| {
        view.active_request()
            .and_then(|request| request.url.clone())
    });
    assert_eq!(edited_url.as_deref(), Some("https://url.example"));
}

#[gpui_kit::test]
fn missing_url_variable_tooltip_creates_the_variable(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "create-var");
    let request_key = select_first_request(&workspace, cx);
    workspace.update(cx, |view, _, cx| {
        view.edit_request(
            request_key,
            |request| request.url = Some("https://{{created}}/users".to_owned()),
            cx,
        );
    });
    cx.run_until_parked();

    let variable_point = workspace
        .visual(cx)
        .debug_bounds("variable-hover-trigger")
        .expect("missing variable hover trigger should render")
        .center();
    hover_and_wait(cx, workspace.window, variable_point);
    let popup_point = {
        let mut visual = workspace.visual(cx);
        assert!(
            visual
                .debug_bounds("variable-tooltip-create-hint")
                .is_some(),
            "missing variable tooltip should invite creating the variable"
        );
        visual
            .debug_bounds("variable-input-tooltip-popup")
            .expect("create-variable tooltip should render")
            .center()
    };
    hover_and_wait(cx, workspace.window, popup_point);
    {
        let mut visual = workspace.visual(cx);
        let value_point = visual
            .debug_bounds("variable-tooltip-value-input")
            .expect("create-variable value input should render")
            .center();
        visual.simulate_mouse_move(value_point, None, Modifiers::default());
        visual.simulate_click(value_point, Modifiers::default());
        visual.run_until_parked();
        assert!(
            visual
                .debug_bounds("variable-input-tooltip-popup")
                .is_some(),
            "create-variable tooltip should stay open while focusing its value field"
        );
    }
    cx.simulate_input(workspace.window.into(), "createdhost");
    cx.run_until_parked();
    let created = workspace.update(cx, |view, _, _| {
        let url = view
            .active_request()
            .and_then(|request| request.url.clone());
        assert_eq!(url.as_deref(), Some("https://{{created}}/users"));
        resolved_variable(view, "created")
    });
    assert_eq!(created.as_deref(), Some("createdhost"));
    cx.run_until_parked();
    assert_eq!(
        saved_development_variable(&workspace, "created").as_deref(),
        Some("createdhost")
    );
}

#[gpui_kit::test]
fn json_body_variables_show_resolved_tooltips(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let request_key = select_first_request(&workspace, cx);
    workspace.update(cx, |view, _, cx| {
        view.request_editor
            .set_section(request_key, EditorSection::Body);
        view.edit_request(
            request_key,
            |request| {
                request.kind = probe_core::RequestKind::Http {
                    body: Some(probe_core::RequestBody::Single(probe_core::Body::Raw(
                        probe_core::RawBody {
                            kind: probe_core::RawBodyKind::Json,
                            data: "{\n  \"tenant\": \"{{tenant}}\"\n}".to_owned(),
                        },
                    ))),
                };
            },
            cx,
        );
    });
    cx.run_until_parked();
    // Hits are placed after the editor reports its overlay origin on a later frame.
    workspace.update(cx, |_, _, cx| cx.notify());
    cx.run_until_parked();

    let variable_point = {
        let mut visual = workspace.visual(cx);
        let editor = visual
            .debug_bounds("request-body-editor")
            .expect("JSON body editor should render");
        visual
            .debug_bounds("body-variable-hover-trigger")
            .unwrap_or_else(|| {
                panic!(
                    "body variable hover trigger should render inside the JSON editor, editor={editor:?}"
                )
            })
            .center()
    };
    hover_and_wait(cx, workspace.window, variable_point);
    let mut visual = workspace.visual(cx);
    visual
        .debug_bounds("variable-input-tooltip-popup")
        .expect("hovered JSON body variable tooltip should render");
    visual
        .debug_bounds("variable-tooltip-value-input")
        .expect("tooltip should offer the resolved value");
}

#[gpui_kit::test]
fn variable_context_resolves_once_per_frame_for_many_headers(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let request_key = select_first_request(&workspace, cx);
    workspace.update(cx, |view, _, cx| {
        view.request_editor
            .set_section(request_key, EditorSection::Headers);
        view.edit_request(
            request_key,
            |request| {
                request.headers = (0..12)
                    .map(|index| probe_core::Header {
                        name: format!("X-{index}"),
                        value: format!("{{{{name{index}}}}}"),
                        disabled: false,
                    })
                    .collect();
            },
            cx,
        );
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        view.variable_context_frames.set(0);
        view.environment_resolution_count.set(0);
        cx.notify();
    });
    cx.run_until_parked();
    let (frames, resolutions) = workspace.update(cx, |view, _, _| {
        (
            view.variable_context_frames.get(),
            view.environment_resolution_count.get(),
        )
    });
    assert!(
        frames >= 1,
        "rendering the request should paint at least one frame"
    );
    assert_eq!(
        resolutions, frames,
        "each frame should resolve the environment once, frames={frames} resolutions={resolutions}"
    );
    assert!(
        resolutions < 12,
        "many header fields should share one resolution, frames={frames} resolutions={resolutions}"
    );
}

#[gpui_kit::test]
fn variable_context_reclassifies_when_the_selected_environment_changes(cx: &mut TestAppContext) {
    use probe_core::VariableStatus::{Missing, Resolved};
    let workspace = EnvironmentWorkspace::open(cx);
    select_first_request(&workspace, cx);
    workspace.update(cx, |_, _, cx| cx.notify());
    cx.run_until_parked();

    workspace.update(cx, |view, _, cx| {
        let resolutions_before = view.environment_resolution_count.get();
        let development = view.variable_context(cx);
        assert!(
            view.environment_resolution_count.get() > resolutions_before,
            "variable_context outside a render pass should resolve again"
        );
        assert!(development.on_manage_environments.is_some());
        assert_eq!(development.status("baseUrl"), Resolved);
        assert_eq!(
            development.values.get("baseUrl").map(String::as_str),
            Some("https://dev.example.com")
        );
        assert_eq!(development.status("token"), Resolved);
        assert_unknown_secret(&development, "secretToken");
        assert_eq!(development.status("disabledValue"), Missing);
        assert_eq!(development.status("missing"), Missing);

        view.select_environment(Some("base".to_owned()), cx);
        let base = view.variable_context(cx);
        assert_eq!(base.status("host"), Resolved);
        assert_eq!(
            base.values.get("host").map(String::as_str),
            Some("api.example.com")
        );
        assert_eq!(base.status("token"), Missing);
        assert_unknown_secret(&base, "secretToken");
        assert_eq!(base.status("disabledValue"), Missing);
        assert_eq!(
            base.values.get("baseUrl").map(String::as_str),
            Some("https://api.example.com")
        );

        view.shell.select_environment(None);
        let unselected = view.variable_context(cx);
        for name in ["baseUrl", "secretToken", "host", "token"] {
            assert_eq!(unselected.status(name), Missing, "{name}");
        }
    });
}

#[gpui_kit::test]
fn invalid_secret_identity_keeps_a_plain_variable_resolvable(cx: &mut TestAppContext) {
    use probe_core::VariableStatus::Resolved;

    let workspace = EnvironmentWorkspace::writable_source(
        cx,
        "invalid-secret-identity",
        r#"opencollection: 1.0.0
info:
  name: Invalid secret identity
bundled: true
config:
  environments:
    - name: development
      variables:
        - name: host
          value: api.example.com
        - name: ""
          secret: true
items:
  - info:
      name: Users
      type: http
      seq: 1
    http:
      method: GET
      url: "https://{{host}}"
"#,
    );
    workspace.select_environment(cx, "development");
    workspace.update(cx, |view, _, cx| {
        let context = view.variable_context(cx);
        assert_eq!(context.status("host"), Resolved);
        assert_eq!(
            context.values.get("host").map(String::as_str),
            Some("api.example.com")
        );
        assert!(
            context.on_change.is_some(),
            "plain variables stay editable when a secret identity fails"
        );
        assert_eq!(
            context.secret_identity_errors.get("").map(String::as_str),
            Some("invalid credential identity")
        );
    });
}

#[gpui_kit::test]
fn watcher_defers_a_committed_environment_save_and_preserves_queued_tooltip_edits(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "watcher-environment-save");
    select_first_request(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        let loaded = view.loaded_workspace.as_mut().unwrap();
        let baseline = loaded.baseline();
        loaded
            .set_environment_variable("development", "host", "first.example".into())
            .unwrap();
        let saved = loaded
            .prepare_environment_variable_save("development", "host")
            .unwrap()
            .execute()
            .unwrap();
        // Hold the completion callback after the atomic write has reached disk.
        view.environment_save_task = Some(cx.spawn(async |_, _| std::future::pending().await));
        let snapshot = probe_opencollection::load_workspace(&workspace.path).unwrap();
        assert!(
            !view.apply_watched_workspace(baseline, Ok(snapshot), BTreeMap::new(), window, cx,)
        );
        view.update_environment_variable("host", "latest.example".into(), window, cx);
        view.update_environment_variable("token", "latest-token".into(), window, cx);
        view.environment_save_task = None;
        view.loaded_workspace
            .as_mut()
            .unwrap()
            .complete_environment_save(saved)
            .unwrap();

        assert_eq!(
            resolved_variable(view, "host").as_deref(),
            Some("latest.example")
        );
        assert_eq!(
            resolved_variable(view, "token").as_deref(),
            Some("latest-token")
        );
        view.pump_workspace_writes(window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        assert!(view.environment_save_task.is_none());
        assert!(view.pending_environment_saves.is_empty());
        let baseline = view.loaded_workspace.as_ref().unwrap().baseline();
        assert!(view.apply_watched_workspace(
            baseline,
            probe_opencollection::load_workspace(&workspace.path),
            BTreeMap::new(),
            window,
            cx,
        ));
        assert_eq!(
            resolved_variable(view, "host").as_deref(),
            Some("latest.example")
        );
        assert_eq!(
            resolved_variable(view, "token").as_deref(),
            Some("latest-token")
        );
        assert!(view.application_dialog.is_none());
        assert!(view.toasts.is_empty(), "{:?}", toast_debug(view));
    });
    assert_eq!(
        saved_development_variable(&workspace, "host").as_deref(),
        Some("latest.example")
    );
    assert_eq!(
        saved_development_variable(&workspace, "token").as_deref(),
        Some("latest-token")
    );
}

#[gpui_kit::test]
fn watcher_retries_a_snapshot_loaded_before_a_save_and_still_reports_external_conflicts(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "watcher-stale-snapshot");
    let key = select_first_request(&workspace, cx);
    // Capture a snapshot before a save starts, then deliver it after completion.
    let (baseline, stale) = workspace.update(cx, |view, window, cx| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let baseline = loaded.baseline();
        let stale = probe_opencollection::load_workspace(&workspace.path).unwrap();
        view.update_environment_variable("host", "newest.example".into(), window, cx);
        (baseline, stale)
    });
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        assert!(view.environment_save_task.is_none());
        assert!(!view.apply_watched_workspace(baseline, Ok(stale), BTreeMap::new(), window, cx,));
        assert_eq!(
            resolved_variable(view, "host").as_deref(),
            Some("newest.example")
        );
        view.edit_request(key, |request| request.method = Some("POST".into()), cx);
        let source = fs::read_to_string(&workspace.path).unwrap();
        fs::write(
            &workspace.path,
            source.replacen("method: GET", "method: PATCH", 1),
        )
        .unwrap();
        let baseline = view.loaded_workspace.as_ref().unwrap().baseline();
        assert!(view.apply_watched_workspace(
            baseline,
            probe_opencollection::load_workspace(&workspace.path),
            BTreeMap::new(),
            window,
            cx,
        ));
        assert!(matches!(
            view.application_dialog,
            Some(ApplicationDialog::FilesystemConflict { .. })
        ));
        assert_eq!(
            view.loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(key)
                .unwrap()
                .method
                .as_deref(),
            Some("POST")
        );
        assert!(view.toasts.is_empty(), "{:?}", toast_debug(view));
    });
}

#[gpui_kit::test]
fn watcher_surfaces_blocked_environment_saves_without_discarding_edits(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "watcher-blocked-environment");
    // In-memory sources cannot prepare saves. Keep a real path for watcher reloads.
    let source = fs::read_to_string(&workspace.path).unwrap();
    let loaded = probe_opencollection::load_workspace_from_str(&source).unwrap();
    workspace.update(cx, |view, _, _| {
        view.set_workspace(workspace.path.clone(), loaded)
    });
    select_first_request(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        view.update_environment_variable("host", "unsaved.example".into(), window, cx);
        assert!(view.environment_save_task.is_none());
        assert!(view.pending_environment_saves.contains(&("development".into(), "host".into())));
        assert!(toast_debug(view).iter().any(|toast| toast.contains("Could not save environment variable")));
        fs::write(&workspace.path, source.replacen("method: GET", "method: PATCH", 1)).unwrap();
        let baseline = view.loaded_workspace.as_ref().unwrap().baseline();
        // This event is handled, rather than retried indefinitely while idle.
        assert!(view.apply_watched_workspace(
            baseline, probe_opencollection::load_workspace(&workspace.path), BTreeMap::new(), window, cx,
        ));
        assert!(matches!(&view.application_dialog, Some(ApplicationDialog::FilesystemConflict { detail, .. }) if detail.contains("unsaved edits to host")));
        assert_eq!(resolved_variable(view, "host").as_deref(), Some("unsaved.example"));
        view.handle_application_dialog_action(ApplicationDialogAction::KeepLocal, window, cx);
        assert_eq!(resolved_variable(view, "host").as_deref(), Some("unsaved.example"));
        assert!(!view.pending_environment_saves.is_empty());
        // Later external changes still reach the existing conflict UI.
        fs::write(&workspace.path, source.replacen("method: GET", "method: PUT", 1)).unwrap();
        assert!(view.apply_watched_workspace(
            baseline, probe_opencollection::load_workspace(&workspace.path), BTreeMap::new(), window, cx,
        ));
        assert!(matches!(view.application_dialog, Some(ApplicationDialog::FilesystemConflict { .. })));
        view.handle_application_dialog_action(ApplicationDialogAction::UseDisk, window, cx);
    });
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        assert!(view.pending_environment_saves.is_empty());
        assert_eq!(
            resolved_variable(view, "host").as_deref(),
            Some("dev.example.com")
        );
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let key = loaded.requests()[0].key();
        assert_eq!(
            loaded.workspace().request(key).unwrap().method.as_deref(),
            Some("PUT")
        );
        let baseline = loaded.baseline();
        assert!(view.apply_watched_workspace(
            baseline,
            probe_opencollection::load_workspace(&workspace.path),
            BTreeMap::new(),
            window,
            cx,
        ));
        assert!(view.application_dialog.is_none());
    });
}
