use super::*;
use crate::request_editor::{AssertionEdit, AssertionField};
use probe_application::{AssertionError, AssertionOutcome, AssertionResult};

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
fn assertion_controls_edit_http_and_graphql_drafts_and_keep_execution_snapshots(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1500.0), px(1000.0)), ProbeApp::new);
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/opencollection/assertions-bundled.yml");
    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let http = loaded.requests()[0].key();
    let graphql = loaded.requests()[1].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, loaded);
            view.select_request(http, cx);
            // Start compact so every action is reachable without layout assumptions.
            view.edit_request(http, |r| r.runtime.assertions = Some(Vec::new()), cx);
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "request-tab-assertions");
    click(&mut visual, "add-assertion");
    click(&mut visual, "assertion-expression-0");
    cx.simulate_keystrokes(
        window.into(),
        if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        },
    );
    cx.simulate_input(window.into(), "response.body.{{editable}}");
    visual.run_until_parked();
    click(&mut visual, "assertion-operator-trigger");
    cx.simulate_keystrokes(window.into(), "down enter");
    visual.run_until_parked();
    click(&mut visual, "assertion-enabled-0");
    click(&mut visual, "add-assertion");
    click(&mut visual, "assertion-up-1");
    window
        .update(cx, |view, _, _| {
            let request = view.active_request().unwrap();
            let rows = request.runtime.assertions.as_ref().unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[1].expression, "response.body.{{editable}}");
            assert_eq!(rows[1].disabled, Some(true));
            assert_eq!(rows[1].operator, "notEquals");
            assert!(view.persistence.is_dirty(http, request));
        })
        .unwrap();
    click(&mut visual, "assertion-delete-0");
    window
        .update(cx, |view, _, cx| {
            let (cancel, _) = oneshot::channel();
            let generation = view.execution.begin(http, cancel);
            let mut response = super::workspace::tab_response();
            response.body_complete = false;
            view.complete_execution(
                http,
                generation,
                Ok((
                    response,
                    vec![
                        AssertionResult {
                            index: 0,
                            outcome: AssertionOutcome::Passed,
                        },
                        AssertionResult {
                            index: 1,
                            outcome: AssertionOutcome::Failed,
                        },
                        AssertionResult {
                            index: 2,
                            outcome: AssertionOutcome::Error(AssertionError::VariableResolution),
                        },
                        AssertionResult {
                            index: 3,
                            outcome: AssertionOutcome::Disabled,
                        },
                    ],
                )),
                None,
                cx,
            );
            view.edit_request(http, |r| AssertionEdit::Remove(0).apply(r), cx);
        })
        .unwrap();
    visual.run_until_parked();
    click(&mut visual, "response-tab-assertions");
    for selector in [
        "assertion-result-0",
        "assertion-result-1",
        "assertion-result-2",
        "assertion-result-3",
    ] {
        assert!(visual.debug_bounds(selector).is_some());
    }
    // Existing response remains usable despite failed/error outcomes and truncation.
    for theme in [
        crate::user_config::ThemeMode::Light,
        crate::user_config::ThemeMode::Dark,
    ] {
        window
            .update(cx, |view, _, cx| {
                view.user_config.theme = theme;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("response-assertions").is_some());
        click(&mut visual, "response-tab-headers");
        click(&mut visual, "response-tab-assertions");
    }
    click(&mut visual, "response-tab-raw");
    click(&mut visual, "response-tab-headers");
    window
        .update(cx, |view, _, cx| {
            view.select_request(graphql, cx);
            view.edit_request(graphql, |r| r.runtime.assertions = Some(Vec::new()), cx);
        })
        .unwrap();
    visual.run_until_parked();
    click(&mut visual, "request-tab-assertions");
    click(&mut visual, "add-assertion");
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.active_request()
                    .unwrap()
                    .runtime
                    .assertions
                    .as_ref()
                    .unwrap()
                    .len(),
                1
            );
            assert!(
                view.persistence
                    .is_dirty(graphql, view.active_request().unwrap())
            );
        })
        .unwrap();
}

#[test]
fn desktop_assertion_edits_round_trip_bundled_and_unbundled_declarations() {
    for unbundled in [false, true] {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/opencollection");
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("opencollection.yml");
        if unbundled {
            for name in ["opencollection.yml", "http.yml", "graphql.yml"] {
                fs::copy(
                    source.join("assertions-unbundled").join(name),
                    directory.path().join(name),
                )
                .unwrap();
            }
        } else {
            fs::copy(source.join("assertions-bundled.yml"), &path).unwrap();
        }
        let entry = if unbundled {
            directory.path()
        } else {
            path.as_path()
        };
        let mut loaded = probe_opencollection::load_workspace(entry).unwrap();
        for reference in loaded.requests().to_vec() {
            let key = reference.key();
            let selector = reference.selector().to_owned();
            let original = loaded.workspace().request(key).unwrap().clone();
            let mut edited = original.clone();
            AssertionEdit::Text(0, AssertionField::Value, "{{ordinaryText}}".into())
                .apply(&mut edited);
            AssertionEdit::Enabled(0, false).apply(&mut edited);
            AssertionEdit::Add.apply(&mut edited);
            AssertionEdit::Move(0, 1).apply(&mut edited);
            if original.kind.protocol() == probe_core::RequestProtocol::Http {
                let rows = edited.runtime.assertions.as_ref().unwrap();
                assert_eq!(
                    rows[2].description,
                    original.runtime.assertions.as_ref().unwrap()[2].description
                );
                assert_eq!(rows[3].operator, "isDefined");
                assert_eq!(rows[3].value, None);
                assert_eq!(rows[3].description, Some(probe_core::Documentation::Null));
            }
            let update = probe_core::RequestUpdate::between(Some(&original), &edited).unwrap();
            assert!(!update.is_empty());
            loaded.update_request(&selector, &update).unwrap();
            let reloaded = probe_opencollection::load_workspace(entry).unwrap();
            let saved = reloaded
                .workspace()
                .request(reloaded.request_key(&selector).unwrap())
                .unwrap();
            assert_eq!(saved, &edited);
            assert!(
                probe_core::RequestUpdate::between(Some(saved), &edited)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[gpui_kit::test]
fn assertion_add_delete_restores_original_list_presence_and_keyboard_save(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    for explicit_empty in [false, true] {
        let path = writable_bundled_fixture(if explicit_empty {
            "assertions-original-empty"
        } else {
            "assertions-original-absent"
        });
        let mut loaded = probe_opencollection::load_workspace(&path).unwrap();
        let key = loaded.requests()[0].key();
        let selector = loaded.requests()[0].selector().to_owned();
        let original = explicit_empty.then(Vec::new);
        let saved = loaded.workspace().request(key).unwrap().clone();
        let mut request = saved.clone();
        request.runtime.assertions = original.clone();
        let update = probe_core::RequestUpdate::between(Some(&saved), &request).unwrap();
        if !update.is_empty() {
            loaded.update_request(&selector, &update).unwrap();
        }
        let window = cx.open_window(size(px(1200.0), px(850.0)), ProbeApp::new);
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(path.clone(), loaded);
                view.select_request(key, cx);
                view.request_editor
                    .set_section(key, EditorSection::Assertions);
            })
            .unwrap();
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        click(&mut visual, "add-assertion");
        window
            .update(cx, |view, _, _| {
                assert!(
                    view.persistence
                        .is_dirty(key, view.active_request().unwrap())
                );
            })
            .unwrap();
        click(&mut visual, "assertion-delete-0");
        window
            .update(cx, |view, _, cx| {
                let request = view.active_request().unwrap();
                assert_eq!(request.runtime.assertions, original);
                assert!(!view.persistence.is_dirty(key, request));
                // An unrelated edit saves without adding an assertion declaration.
                view.edit_request(
                    key,
                    |request| request.url = Some("https://changed.example/".into()),
                    cx,
                );
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), save_shortcut());
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(
                    !view
                        .persistence
                        .is_dirty(key, view.active_request().unwrap())
                );
            })
            .unwrap();
        let reloaded = probe_opencollection::load_workspace(&path).unwrap();
        assert_eq!(
            reloaded
                .workspace()
                .request(reloaded.request_key(&selector).unwrap())
                .unwrap()
                .runtime
                .assertions,
            original
        );
        fs::remove_file(path).unwrap();
    }
}
