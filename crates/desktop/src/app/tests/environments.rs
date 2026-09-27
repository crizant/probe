use super::*;
use crate::app::chrome::environment_variable_text;
use crate::credentials::CredentialStore;
use gpui::ScrollStrategy;
use std::rc::Rc;

#[test]
fn environment_manager_row_ids_follow_insertions_and_removals() {
    let environment = Environment {
        name: "empty".to_owned(),
        color: None,
        extends: None,
        dot_env_file_path: None,
        variables: Vec::new(),
    };
    let variable = || {
        EnvironmentVariable::Plain(Variable {
            name: Some("name".to_owned()),
            value: None,
            disabled: false,
        })
    };
    let mut dialog = super::super::EnvironmentManagerDialog::new(&environment);
    dialog.add_variable(variable());
    dialog.add_variable(variable());
    assert_eq!(dialog.variable_row_ids, [0, 1]);

    dialog.draft.variables.push(variable());
    dialog.draft.variables.push(variable());
    dialog.sync_variable_row_ids();
    assert_eq!(dialog.variable_row_ids, [0, 1, 2, 3]);
    assert_eq!(dialog.next_variable_row_id, 4);

    dialog.remove_variable(1);
    assert_eq!(dialog.variable_row_ids, [0, 2, 3]);
    assert_eq!(dialog.draft.variables.len(), 3);
    dialog.remove_variable(dialog.draft.variables.len());
    dialog.remove_variable(99);
    assert_eq!(dialog.variable_row_ids, [0, 2, 3]);
}

#[gpui::test]
fn environment_switcher_is_visible_without_a_selected_request(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            assert!(view.shell.active_tab().is_none());
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("request-environment-trigger").is_some());
}

#[gpui::test]
fn environment_switcher_includes_environment_actions(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("request-environment-trigger")
        .expect("environment switcher should render");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    visual
        .debug_bounds("request-environment-action-0")
        .expect("environment switcher should include Create environment");
    visual
        .debug_bounds("request-environment-action-1")
        .expect("environment switcher should include Manage environments");
}

#[gpui::test]
fn environment_manager_renders_editable_and_readonly_variable_fields(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("base".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .expect("test window should be open");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual
            .debug_bounds("environment-manager-dialog")
            .expect("environment manager should render");
        visual
            .debug_bounds("environment-manager-parent-trigger")
            .expect("extends dropdown should render");
        visual
            .debug_bounds("environment-manager-variables")
            .expect("variable table should render");
        visual
            .debug_bounds("environment-manager-add")
            .expect("compact add-environment control should render");
        assert!(
            visual.debug_bounds("environment-manager-delete").is_none(),
            "delete should live in the environment context menu, not the sidebar"
        );
        visual
            .debug_bounds("environment-manager-add-variable")
            .expect("inline add-variable action should render");
        assert!(
            visual
                .debug_bounds("environment-manager-save-status")
                .is_none(),
            "save status should not render in the environment manager footer"
        );
        visual
            .debug_bounds("environment-variable-value-host")
            .expect("string values should remain editable");
        visual
            .debug_bounds("environment-variable-variant-tenant")
            .expect("direct selectable-variant values should render as read-only");
        assert!(
            visual
                .debug_bounds("environment-variable-value-tenant")
                .is_none(),
            "direct selectable-variant values must not use an editable input"
        );
    }

    window
        .update(cx, |view, _, cx| {
            view.select_environment_manager_environment("development", cx);
            let dialog = view
                .environment_manager_dialog
                .as_mut()
                .expect("manager should remain open");
            assert_eq!(dialog.original_name, "development");
            dialog
                .draft
                .variables
                .push(EnvironmentVariable::Plain(Variable {
                    name: Some("retries".to_owned()),
                    value: Some(VariableValueSet::Single(VariableValue::Typed {
                        kind: probe_core::VariableValueType::Number,
                        data: "3".to_owned(),
                    })),
                    disabled: false,
                }));
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual
        .debug_bounds("environment-variable-name-host")
        .expect("direct variable names should be editable");
    assert!(
        visual
            .debug_bounds("environment-variable-name-baseUrl")
            .is_none(),
        "inherited variable names should remain read-only"
    );
    visual
        .debug_bounds("environment-variable-value-host")
        .expect("string values should remain editable");
    visual
        .debug_bounds("environment-variable-value-retries")
        .expect("typed single values should remain editable");
    visual
        .debug_bounds("environment-variable-variant-tenant")
        .expect("inherited selectable-variant values should render as read-only");
    assert!(
        visual
            .debug_bounds("environment-variable-value-tenant")
            .is_none(),
        "inherited selectable-variant values must not use an editable input"
    );
    visual
        .debug_bounds("environment-manager-dirty")
        .expect("unsaved environment changes should show a dirty indicator");
}

#[gpui::test]
fn environment_manager_virtualizes_variables_and_preserves_row_identity(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("base".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                for index in 0..500 {
                    dialog.add_variable(EnvironmentVariable::Plain(Variable {
                        name: Some(format!("virtual-{index}")),
                        value: Some(VariableValueSet::Single(VariableValue::String(format!(
                            "value-{index}"
                        )))),
                        disabled: false,
                    }));
                }
            });
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let first_value = visual
        .debug_bounds("environment-variable-value-virtual-0")
        .expect("first variable should render");
    visual.simulate_click(first_value.center(), Modifiers::default());
    cx.simulate_input(window.into(), "draft");
    visual.run_until_parked();
    let first_draft = window
        .update(cx, |view, _, _| {
            let dialog = view.environment_manager_dialog.as_ref().unwrap();
            environment_variable_text(
                match &dialog.draft.variables[dialog.draft.variables.len() - 500] {
                    EnvironmentVariable::Plain(variable) => variable,
                    EnvironmentVariable::Secret(_) => panic!("expected a plain variable"),
                },
            )
            .0
        })
        .unwrap();
    assert!(first_draft.contains("draft"));
    assert!(
        visual
            .debug_bounds("environment-variable-value-virtual-499")
            .is_none(),
        "offscreen rows should not render"
    );
    assert!(
        visual
            .debug_bounds("environment-manager-add-variable")
            .is_none(),
        "the add action should scroll with the rows"
    );
    let rendered = window
        .update(cx, |view, _, _| view.rendered_environment_variable_rows)
        .unwrap();
    assert!(rendered > 0 && rendered < 40, "rendered {rendered} rows");

    window
        .update(cx, |view, _, cx| {
            let add_row_index = view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .draft
                .variables
                .len();
            view.environment_variables_scroll
                .scroll_to_item_strict(add_row_index, ScrollStrategy::Bottom);
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    visual
        .debug_bounds("environment-variable-value-virtual-499")
        .expect("scrolling should render the last variable");
    visual
        .debug_bounds("environment-manager-add-variable")
        .expect("scrolling should reveal the add action");
    assert!(
        visual
            .debug_bounds("environment-variable-value-virtual-0")
            .is_none()
    );

    window
        .update(cx, |view, _, cx| {
            let dialog = view.environment_manager_dialog.as_mut().unwrap();
            let retained_id = dialog.variable_row_ids[2];
            dialog.remove_variable(1);
            assert_eq!(dialog.variable_row_ids[1], retained_id);
            view.environment_variables_scroll
                .scroll_to_item_strict(0, ScrollStrategy::Top);
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    let first_value = visual
        .debug_bounds("environment-variable-value-virtual-0")
        .expect("remaining first variable should render");
    let retained_draft = window
        .update(cx, |view, _, _| {
            let dialog = view.environment_manager_dialog.as_ref().unwrap();
            let variable = dialog
                .draft
                .variables
                .iter()
                .find(|variable| matches!(variable, EnvironmentVariable::Plain(variable) if variable.name.as_deref() == Some("virtual-0")))
                .unwrap();
            let EnvironmentVariable::Plain(variable) = variable else {
                unreachable!()
            };
            environment_variable_text(variable).0
        })
        .unwrap();
    assert_eq!(retained_draft, first_draft);
    visual.simulate_click(first_value.center(), Modifiers::default());
    cx.simulate_input(window.into(), "more");
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let dialog = view.environment_manager_dialog.as_ref().unwrap();
            let values = dialog
                .draft
                .variables
                .iter()
                .filter_map(|variable| match variable {
                    EnvironmentVariable::Plain(variable) => Some((
                        variable.name.as_deref().unwrap_or(""),
                        environment_variable_text(variable).0,
                    )),
                    EnvironmentVariable::Secret(_) => None,
                })
                .collect::<Vec<_>>();
            assert!(
                values
                    .iter()
                    .find(|(name, _)| *name == "virtual-0")
                    .unwrap()
                    .1
                    .contains("draft")
            );
            assert!(
                values
                    .iter()
                    .find(|(name, _)| *name == "virtual-0")
                    .unwrap()
                    .1
                    .contains("more")
            );
            assert_eq!(
                values
                    .iter()
                    .find(|(name, _)| *name == "virtual-1")
                    .unwrap()
                    .1,
                "value-1"
            );
        })
        .unwrap();
    visual
        .debug_bounds("environment-variable-value-virtual-1")
        .expect("remaining second variable should render");

    window
        .update(cx, |view, _, cx| {
            let original = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .environments()
                .iter()
                .find(|environment| environment.name == "base")
                .unwrap()
                .clone();
            view.environment_manager_dialog.as_mut().unwrap().draft = original;
            let previous_scroll = view.environment_variables_scroll.0.clone();
            view.select_environment_manager_environment("development", cx);
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .original_name,
                "development"
            );
            assert!(!Rc::ptr_eq(
                &previous_scroll,
                &view.environment_variables_scroll.0
            ));
            assert_eq!(
                view.environment_variables_scroll
                    .0
                    .borrow()
                    .base_handle
                    .offset()
                    .y,
                px(0.0)
            );
        })
        .unwrap();
}

#[gpui::test]
fn environment_manager_protects_dirty_draft_and_restores_create_focus(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            assert_eq!(
                window.focused(cx),
                Some(view.environment_manager_dialog_focus.clone())
            );
            view.open_create_environment_dialog(window, cx);
            assert!(view.create_environment_dialog.is_some());
            assert_eq!(
                window.focused(cx),
                Some(view.create_environment_dialog_focus.clone())
            );
            view.close_create_environment_dialog(window, cx);
            assert!(view.create_environment_dialog.is_none());
            assert_eq!(
                window.focused(cx),
                Some(view.environment_manager_dialog_focus.clone())
            );
            let dialog = view
                .environment_manager_dialog
                .as_mut()
                .expect("manager should remain open");
            dialog.draft.name = "renamed-development".to_owned();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let add = visual
            .debug_bounds("environment-manager-add")
            .expect("add-environment control should render");
        visual.simulate_click(add.center(), Modifiers::default());
        visual.run_until_parked();
    }

    window
        .update(cx, |view, window, cx| {
            assert!(view.create_environment_dialog.is_none());
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.draft.name.as_str()),
                Some("renamed-development")
            );
            view.open_create_environment_dialog(window, cx);
            assert!(view.create_environment_dialog.is_none());
            assert!(
                has_active_toast(
                    view,
                    ToastIntent::Error,
                    "Save or discard unsaved environment changes first."
                ),
                "{:?}",
                toast_debug(view)
            );
            view.create_named_environment("staging".to_owned(), window, cx);
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.draft.name.as_str()),
                Some("renamed-development")
            );
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn environment_manager_validation_errors_are_scoped_and_dismissible(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.show_toast(ToastIntent::Error, "App-level error", cx);
            view.environment_manager_dialog
                .as_mut()
                .expect("manager should open")
                .draft
                .name = "  ".to_owned();
            view.save_environment_manager_dialog(window, cx);
            assert!(
                has_active_toast(
                    view,
                    ToastIntent::Error,
                    "Environment and variable names are required."
                ),
                "{:?}",
                toast_debug(view)
            );
            assert!(
                has_active_toast(view, ToastIntent::Error, "App-level error"),
                "{:?}",
                toast_debug(view)
            );
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    visual.run_until_parked();
    let close = visual
        .debug_bounds("toast-close-1")
        .expect("the validation toast should expose a close action");
    visual.simulate_click(close.center(), Modifiers::default());
    visual.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.environment_dialog_error.is_none());
            assert!(
                !has_active_toast(
                    view,
                    ToastIntent::Error,
                    "Environment and variable names are required."
                ),
                "{:?}",
                toast_debug(view)
            );
            assert!(
                has_active_toast(view, ToastIntent::Error, "App-level error"),
                "{:?}",
                toast_debug(view)
            );
            assert!(view.environment_manager_dialog.is_some());
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn environment_manager_routes_blocked_save_and_create_failures_to_its_error(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);

            view.pending_environment_saves
                .insert(("development".to_owned(), "host".to_owned()));
            view.save_environment_manager_dialog(window, cx);
            assert!(
                has_active_toast(
                    view,
                    ToastIntent::Error,
                    "Wait for the current save to finish."
                ),
                "{:?}",
                toast_debug(view)
            );
            view.pending_environment_saves.clear();

            view.environment_manager_dialog
                .as_mut()
                .expect("manager should remain open")
                .draft
                .name = "base".to_owned();
            view.save_environment_manager_dialog(window, cx);
            assert!(
                has_active_toast(view, ToastIntent::Error, "Could not save environment:"),
                "{:?}",
                toast_debug(view)
            );

            view.environment_manager_dialog
                .as_mut()
                .expect("manager should remain open")
                .draft
                .name = "development".to_owned();
            view.open_create_environment_dialog(window, cx);
            *view
                .create_environment_dialog
                .as_mut()
                .expect("create dialog should open") = "base".to_owned();
            view.submit_create_environment_dialog(window, cx);
            assert!(view.create_environment_dialog.is_some());
            assert!(
                has_active_toast(view, ToastIntent::Error, "Could not create environment:"),
                "{:?}",
                toast_debug(view)
            );
        })
        .expect("test window should be open");
}

#[gpui::test]
fn environment_dialog_auto_dismisses_errors_when_their_condition_resolves(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.environment_manager_dialog
                .as_mut()
                .expect("manager should open")
                .draft
                .name = "  ".to_owned();
            view.save_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.name = "development".to_owned();
            });
        })
        .expect("test window should be open");
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.environment_dialog_error.is_none());
        })
        .expect("test window should remain open");

    window
        .update(cx, |view, window, cx| {
            view.pending_environment_saves
                .insert(("development".to_owned(), "host".to_owned()));
            view.save_environment_manager_dialog(window, cx);
            view.pending_environment_saves.clear();
            cx.notify();
        })
        .expect("test window should remain open");
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.environment_dialog_error.is_none());
        })
        .expect("test window should remain open");

    window
        .update(cx, |view, window, cx| {
            view.environment_manager_dialog
                .as_mut()
                .expect("manager should remain open")
                .draft
                .name = "base".to_owned();
            view.save_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.name = "development".to_owned();
            });
        })
        .expect("test window should remain open");
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(
                has_active_toast(view, ToastIntent::Error, "Could not save environment:"),
                "{:?}",
                toast_debug(view)
            );
            view.open_create_environment_dialog(window, cx);
            view.submit_create_environment_dialog(window, cx);
            *view
                .create_environment_dialog
                .as_mut()
                .expect("create dialog should remain open") = "staging".to_owned();
            cx.notify();
        })
        .expect("test window should remain open");
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.environment_dialog_error.is_none());
            assert_eq!(view.create_environment_dialog.as_deref(), Some("staging"));
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn environment_manager_saves_plain_variables_and_parent(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-save")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            let dialog = view
                .environment_manager_dialog
                .as_mut()
                .expect("manager should open");
            dialog.draft.extends = None;
            dialog
                .draft
                .variables
                .push(EnvironmentVariable::Plain(Variable {
                    name: Some("region".to_owned()),
                    value: Some(VariableValueSet::Single(VariableValue::String(
                        "ap-southeast-2".to_owned(),
                    ))),
                    disabled: false,
                }));
            view.save_environment_manager_dialog(window, cx);
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            assert!(
                has_active_toast(view, ToastIntent::Success, "Environment saved."),
                "{:?}",
                toast_debug(view)
            );
        })
        .expect("test window should remain open");
    let reloaded = probe_opencollection::load_workspace(&fixture).expect("saved env should load");
    let development = reloaded
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == "development")
        .unwrap();
    assert_eq!(development.extends, None);
    assert!(development.variables.iter().any(|variable| matches!(
        variable,
        EnvironmentVariable::Plain(variable) if variable.name.as_deref() == Some("region")
    )));
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn platform_save_hotkey_saves_dirty_environment_manager(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-save-hotkey")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.extends = None;
            });
            assert!(!view.environment_manager_save_disabled());
        })
        .expect("test window should be open");
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), save_shortcut());
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            assert!(
                has_active_toast(view, ToastIntent::Success, "Environment saved."),
                "{:?}",
                toast_debug(view)
            );
            assert!(view.environment_manager_save_disabled());
        })
        .expect("test window should remain open");
    let reloaded = probe_opencollection::load_workspace(&fixture).expect("saved env should load");
    let development = reloaded
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == "development")
        .unwrap();
    assert_eq!(development.extends, None);
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn platform_save_hotkey_is_disabled_when_environment_manager_has_nothing_to_save(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-save-hotkey-clean")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request_key, cx);
            view.edit_request(
                request_key,
                |request| request.url = Some("https://dirty.example".to_owned()),
                cx,
            );
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            assert!(view.environment_manager_save_disabled());
            assert!(view.request_is_dirty(request_key));
        })
        .expect("test window should be open");
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), save_shortcut());
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.environment_manager_dialog.is_some());
            assert!(view.environment_manager_save_disabled());
            assert!(view.request_is_dirty(request_key));
            assert!(
                !has_active_toast(view, ToastIntent::Success, "Environment saved."),
                "{:?}",
                toast_debug(view)
            );
            assert!(
                !has_active_toast(view, ToastIntent::Success, "Request saved."),
                "{:?}",
                toast_debug(view)
            );
        })
        .expect("test window should remain open");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn platform_save_hotkey_is_disabled_while_environment_manager_save_is_busy(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-save-hotkey-busy")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.extends = None;
            });
            view.save_environment_manager_dialog(window, cx);
            assert!(view.environment_save_task.is_some());
            assert!(view.environment_manager_save_disabled());
            window.dispatch_action(Box::new(SubmitEnvironmentManagerDialog), cx);
            assert!(
                !has_active_toast(
                    view,
                    ToastIntent::Error,
                    "Wait for the current save to finish."
                ),
                "{:?}",
                toast_debug(view)
            );
            assert!(view.environment_save_task.is_some());
        })
        .expect("test window should be open");
    cx.run_until_parked();
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_manager_save_ignores_edits_made_while_busy(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-save-busy")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.save_environment_manager_dialog(window, cx);
            assert!(view.environment_save_task.is_some());
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.name = "hijacked".to_owned();
            });
            view.environment_manager_dialog
                .as_mut()
                .expect("manager should stay open during save")
                .draft
                .name = "hijacked".to_owned();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            let dialog = view
                .environment_manager_dialog
                .as_ref()
                .expect("manager should rebind to the saved environment");
            assert_eq!(dialog.original_name, "development");
            assert_eq!(dialog.draft.name, "development");
        })
        .expect("test window should remain open");
    let reloaded = probe_opencollection::load_workspace(&fixture).expect("saved env should load");
    assert!(
        reloaded
            .workspace()
            .environments()
            .iter()
            .any(|environment| environment.name == "development")
    );
    assert!(
        reloaded
            .workspace()
            .environments()
            .iter()
            .all(|environment| environment.name != "hijacked")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_manager_deletes_a_leaf_environment(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-delete")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.delete_environment("development".to_owned(), window, cx);
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.original_name.as_str()),
                Some("base")
            );
            assert_eq!(
                window.focused(cx),
                Some(view.environment_manager_dialog_focus.clone())
            );
        })
        .expect("test window should remain open");
    let reloaded = probe_opencollection::load_workspace(&fixture).expect("saved env should load");
    assert!(
        reloaded
            .workspace()
            .environments()
            .iter()
            .all(|environment| environment.name != "development")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_manager_delete_preserves_a_dirty_draft_for_another_environment(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-delete-other")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.create_named_environment("staging".to_owned(), window, cx);
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            view.select_environment_manager_environment("development", cx);
            let dialog = view
                .environment_manager_dialog
                .as_mut()
                .expect("manager should be editing development");
            assert_eq!(dialog.original_name, "development");
            dialog.draft.name = "renamed-development".to_owned();
            view.delete_environment("staging".to_owned(), window, cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            let dialog = view
                .environment_manager_dialog
                .as_ref()
                .expect("manager should keep the current draft");
            assert_eq!(dialog.original_name, "development");
            assert_eq!(dialog.draft.name, "renamed-development");
        })
        .expect("test window should remain open");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_manager_delete_selects_a_neighbor(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-delete-neighbor")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.create_named_environment("staging".to_owned(), window, cx);
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            view.select_environment_manager_environment("development", cx);
            view.delete_environment("development".to_owned(), window, cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.original_name.as_str()),
                Some("staging")
            );
        })
        .expect("test window should remain open");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_manager_delete_of_the_last_environment_closes(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-delete-last")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.delete_environment("development".to_owned(), window, cx);
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.original_name.as_str()),
                Some("base")
            );
            view.delete_environment("base".to_owned(), window, cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            assert!(view.environment_manager_dialog.is_none());
            assert_eq!(window.focused(cx), Some(view.focus_handle.clone()));
        })
        .expect("test window should remain open");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_manager_closes_when_the_workspace_resets(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let other = http_environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let other_workspace =
        probe_opencollection::load_workspace(&other).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.open_environment_manager_dialog(window, cx);
            view.environment_manager_dialog
                .as_mut()
                .expect("manager should open")
                .draft
                .name = "renamed-development".to_owned();
            view.set_workspace(other, other_workspace);
            assert!(view.environment_manager_dialog.is_none());
            let reloaded =
                probe_opencollection::load_workspace(&fixture).expect("fixture should reload");
            view.set_workspace(fixture, reloaded);
            view.open_environment_manager_dialog(window, cx);
            view.close_workspace_now(cx);
            assert!(view.environment_manager_dialog.is_none());
        })
        .expect("test window should be open");
}

#[gpui::test]
fn environment_manager_rebinds_after_workspace_reload(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-reload")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.apply_reconciled_workspace(
                reconciled_workspace(
                    probe_opencollection::load_workspace(&fixture).expect("fixture should reload"),
                ),
                cx,
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.original_name.as_str()),
                Some("development")
            );
            view.environment_manager_dialog
                .as_mut()
                .expect("manager should remain open")
                .draft
                .name = "renamed-development".to_owned();
            view.apply_reconciled_workspace(
                reconciled_workspace(
                    probe_opencollection::load_workspace(&fixture).expect("fixture should reload"),
                ),
                cx,
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.draft.name.as_str()),
                Some("renamed-development")
            );
            assert!(view.toasts.is_empty(), "{:?}", toast_debug(view));
        })
        .expect("test window should be open");

    let mut changed = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let mut replacement = changed.workspace().environments()[1].clone();
    replacement.extends = None;
    let saved = changed
        .prepare_environment_replace("development", replacement)
        .unwrap()
        .execute()
        .unwrap();
    changed.complete_environment_replace(saved);

    window
        .update(cx, |view, _, cx| {
            view.apply_reconciled_workspace(reconciled_workspace(changed), cx);
            let dialog = view
                .environment_manager_dialog
                .as_ref()
                .expect("manager should rebind to disk");
            assert_eq!(dialog.original_name, "development");
            assert_eq!(dialog.draft.name, "development");
            assert_eq!(dialog.draft.extends, None);
            assert!(
                has_active_toast(
                    view,
                    ToastIntent::Error,
                    "This environment changed on disk. Unsaved environment edits were discarded."
                ),
                "{:?}",
                toast_debug(view)
            );
        })
        .expect("test window should remain open");

    let mut remaining =
        probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let saved = remaining
        .prepare_environment_delete("development")
        .unwrap()
        .execute()
        .unwrap();
    remaining.complete_environment_delete(saved);

    window
        .update(cx, |view, _, cx| {
            view.apply_reconciled_workspace(reconciled_workspace(remaining), cx);
            assert!(view.environment_manager_dialog.is_none());
        })
        .expect("test window should remain open");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_manager_cancel_with_unsaved_changes_prompts(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
            let dialog = view
                .environment_manager_dialog
                .as_mut()
                .expect("manager should open");
            dialog.draft.name = "renamed-development".to_owned();
            view.request_close_environment_manager_dialog(window, cx);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::UnsavedEnvironment)
            ));
            assert!(view.environment_manager_dialog.is_some());
        })
        .expect("test window should be open");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let cancel = visual
            .debug_bounds("application-dialog-cancel")
            .expect("unsaved environment warning should render Cancel");
        visual.simulate_click(cancel.center(), Modifiers::default());
        visual.run_until_parked();
    }

    window
        .update(cx, |view, window, cx| {
            assert!(view.application_dialog.is_none());
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .map(|dialog| dialog.draft.name.as_str()),
                Some("renamed-development")
            );
            view.request_close_environment_manager_dialog(window, cx);
            view.handle_application_dialog_action(ApplicationDialogAction::Discard, window, cx);
            assert!(view.application_dialog.is_none());
            assert!(view.environment_manager_dialog.is_none());
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn environment_manager_context_menu_deletes_an_environment(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("manager-context-delete")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .expect("test window should be open");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let dialog = visual
            .debug_bounds("environment-manager-dialog")
            .expect("environment manager should render");
        window
            .update(cx, |view, _, cx| {
                view.open_environment_manager_context_menu(
                    "development".to_owned(),
                    dialog.center(),
                    cx,
                );
            })
            .expect("test window should remain open");
        visual.run_until_parked();
        let delete = visual
            .debug_bounds("environment-manager-delete")
            .expect("environment context menu should include Delete");
        visual.simulate_click(delete.center(), Modifiers::default());
        visual.run_until_parked();
    }

    window
        .update(cx, |view, _, _| {
            assert!(matches!(
                view.application_dialog.as_ref(),
                Some(ApplicationDialog::DeleteEnvironment { name, .. }) if name == "development"
            ));
        })
        .expect("test window should remain open");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn create_environment_dialog_cannot_close_while_persistence_is_running(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("create-env-cancel-busy")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.open_create_environment_dialog(window, cx);
            *view
                .create_environment_dialog
                .as_mut()
                .expect("create dialog should open") = "staging".to_owned();
            view.submit_create_environment_dialog(window, cx);
            assert!(view.environment_save_task.is_some());

            view.close_create_environment_dialog(window, cx);
            assert!(view.create_environment_dialog.is_some());
            assert_eq!(
                window.focused(cx),
                Some(view.create_environment_dialog_focus.clone())
            );
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.environment_save_task.is_none());
            assert!(view.create_environment_dialog.is_none());
            assert_eq!(view.shell.selected_environment(), Some("staging"));
        })
        .expect("test window should remain open");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn creating_an_environment_from_the_switcher_persists_and_selects_it(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("create-env")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let trigger = visual
            .debug_bounds("request-environment-trigger")
            .expect("environment switcher should render");
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
        let create = visual
            .debug_bounds("request-environment-action-0")
            .expect("Create environment action should render");
        visual.simulate_click(create.center(), Modifiers::default());
        visual.run_until_parked();
    }
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert!(view.create_environment_dialog.is_some());
            if let Some(name) = view.create_environment_dialog.as_mut() {
                *name = "staging".to_owned();
            }
            view.submit_create_environment_dialog(window, cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(
                view.environment_save_task.is_none(),
                "{:?}",
                toast_debug(view)
            );
            assert_eq!(view.shell.selected_environment(), Some("staging"));
            let loaded = view.loaded_workspace.as_ref().expect("workspace");
            assert!(
                loaded
                    .workspace()
                    .environments()
                    .iter()
                    .any(|environment| environment.name == "staging")
            );
        })
        .expect("test window should remain open");

    let reloaded = probe_opencollection::load_workspace(&fixture).expect("created env should load");
    assert!(
        reloaded
            .workspace()
            .environments()
            .iter()
            .any(|environment| environment.name == "staging")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn create_environment_dialog_rejects_an_empty_name(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("create-env-empty")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.open_create_environment_dialog(window, cx);
            view.submit_create_environment_dialog(window, cx);
            assert!(
                has_active_toast(view, ToastIntent::Error, "Environment name is required."),
                "{:?}",
                toast_debug(view)
            );
            assert!(view.create_environment_dialog.is_some());
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual
        .debug_bounds("toast-0")
        .expect("validation error should render as a toast");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn environment_selection_is_shared_when_opening_another_request(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let first = workspace.requests()[0].key();
    let second = workspace.requests()[1].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(first, cx);
            view.shell
                .select_environment(Some("development".to_owned()));
            view.select_request(second, cx);
            assert_eq!(view.shell.selected_environment(), Some("development"));
        })
        .expect("test window should be open");
}

#[gpui::test]
fn environment_selection_is_restored_when_reopening_a_workspace(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request_key, cx);
            view.select_environment(Some("development".to_owned()), cx);
            view.close_workspace_now(cx);
        })
        .expect("test window should be open");

    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should reload");
    window
        .update(cx, |view, _, _| {
            view.set_workspace(fixture, workspace);
            assert_eq!(view.shell.selected_environment(), Some("development"));
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn environment_selection_is_remembered_per_workspace(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let first_fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let second_fixture = http_environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let first_workspace =
        probe_opencollection::load_workspace(&first_fixture).expect("fixture should load");
    let second_workspace =
        probe_opencollection::load_workspace(&second_fixture).expect("fixture should load");
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(first_fixture.clone(), first_workspace);
            view.select_environment(Some("development".to_owned()), cx);
            view.capture_selected_environment();
            view.set_workspace(second_fixture.clone(), second_workspace);
            view.select_environment(Some("local".to_owned()), cx);
            view.capture_selected_environment();
        })
        .expect("test window should be open");

    let first_workspace =
        probe_opencollection::load_workspace(&first_fixture).expect("fixture should reload");
    let second_workspace =
        probe_opencollection::load_workspace(&second_fixture).expect("fixture should reload");
    window
        .update(cx, |view, _, _| {
            view.set_workspace(first_fixture, first_workspace);
            assert_eq!(view.shell.selected_environment(), Some("development"));
            view.capture_selected_environment();
            view.set_workspace(second_fixture, second_workspace);
            assert_eq!(view.shell.selected_environment(), Some("local"));
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn missing_environment_is_not_restored(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.session.remember_selected_environment(
                fixture.clone(),
                Some("missing-environment".to_owned()),
            );
            view.set_workspace(fixture, workspace);
            assert_eq!(view.shell.selected_environment(), None);
            cx.notify();
        })
        .expect("test window should be open");
}

#[gpui::test]
fn request_variables_render_inline_and_show_resolved_tooltips(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("tooltip")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request_key, cx);
            view.shell
                .select_environment(Some("development".to_owned()));
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let (variable_point, input_point, trigger_left) = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let variable = visual
            .debug_bounds("variable-hover-trigger")
            .expect("variable hover trigger should render");
        let url_input = visual
            .debug_bounds("request-url-input")
            .expect("request URL input should render");
        (variable.center(), url_input.center(), variable.left())
    };
    hover_and_wait(cx, window, variable_point);
    let popup_point = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let popup = visual
            .debug_bounds("variable-input-tooltip-popup")
            .expect("hovered variable tooltip should render");
        assert!(
            (popup.left() - trigger_left).abs() < px(1.0),
            "tooltip left edge should align to the variable, popup={:?} trigger_left={:?}",
            popup,
            trigger_left
        );
        popup.center()
    };
    hover_and_wait(cx, window, popup_point);
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
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
    window
        .update(cx, |view, window, cx| {
            view.update_environment_variable(
                "baseUrl",
                "https://changed.example".to_owned(),
                window,
                cx,
            );
        })
        .expect("test window should remain open");
    cx.run_until_parked();
    let updated = window
        .update(cx, |view, _, _| {
            let environment = view.shell.selected_environment()?.to_owned();
            probe_core::resolve_environment(
                view.loaded_workspace.as_ref()?.workspace().environments(),
                &environment,
            )
            .ok()
            .and_then(|resolved| resolved.variable("baseUrl").map(str::to_owned))
        })
        .expect("test window should remain open");
    assert_eq!(updated.as_deref(), Some("https://changed.example"));
    let reloaded = probe_opencollection::load_workspace(&fixture).expect("saved env should load");
    assert_eq!(
        probe_core::resolve_environment(reloaded.workspace().environments(), "development")
            .unwrap()
            .variable("baseUrl"),
        Some("https://changed.example")
    );
    let button_point = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual
            .debug_bounds("variable-tooltip-manage-environments")
            .expect("tooltip should include Manage environments")
            .center()
    };
    hover_and_wait(cx, window, button_point);
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(button_point, Modifiers::default());
        visual.run_until_parked();
    }
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
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
    window
        .update(cx, |view, window, cx| {
            view.request_close_environment_manager_dialog(window, cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(input_point, Modifiers::default());
        visual.run_until_parked();
    }
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    cx.simulate_keystrokes(window.into(), select_all);
    cx.simulate_input(window.into(), "https://url.example");
    cx.run_until_parked();
    let edited_url = window
        .update(cx, |view, _, _| {
            view.active_request()
                .and_then(|request| request.url.clone())
        })
        .expect("test window should remain open");
    assert_eq!(edited_url.as_deref(), Some("https://url.example"));
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn missing_url_variable_tooltip_creates_the_variable(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("create-var")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request_key, cx);
            view.shell
                .select_environment(Some("development".to_owned()));
            view.edit_request(
                request_key,
                |request| request.url = Some("https://{{created}}/users".to_owned()),
                cx,
            );
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let variable_point = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual
            .debug_bounds("variable-hover-trigger")
            .expect("missing variable hover trigger should render")
            .center()
    };
    hover_and_wait(cx, window, variable_point);
    let popup_point = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
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
    hover_and_wait(cx, window, popup_point);
    let value_point = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual
            .debug_bounds("variable-tooltip-value-input")
            .expect("create-variable value input should render")
            .center()
    };
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
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
    cx.simulate_input(window.into(), "createdhost");
    cx.run_until_parked();
    let created = window
        .update(cx, |view, _, _| {
            let url = view
                .active_request()
                .and_then(|request| request.url.clone());
            assert_eq!(url.as_deref(), Some("https://{{created}}/users"));
            let environment = view.shell.selected_environment()?.to_owned();
            probe_core::resolve_environment(
                view.loaded_workspace.as_ref()?.workspace().environments(),
                &environment,
            )
            .ok()
            .and_then(|resolved| resolved.variable("created").map(str::to_owned))
        })
        .expect("test window should remain open");
    assert_eq!(created.as_deref(), Some("createdhost"));
    cx.run_until_parked();
    let reloaded = probe_opencollection::load_workspace(&fixture).expect("saved env should load");
    assert_eq!(
        probe_core::resolve_environment(reloaded.workspace().environments(), "development")
            .unwrap()
            .variable("created"),
        Some("createdhost")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn json_body_variables_show_resolved_tooltips(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("body-tooltip")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request_key, cx);
            view.shell
                .select_environment(Some("development".to_owned()));
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
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();
    // Hits are placed after the editor reports its overlay origin on a later frame.
    window
        .update(cx, |_, _, cx| cx.notify())
        .expect("test window should remain open");
    cx.run_until_parked();

    let (variable_point, trigger_left) = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let editor = visual
            .debug_bounds("request-body-editor")
            .expect("JSON body editor should render");
        let variable = visual
                .debug_bounds("body-variable-hover-trigger")
                .unwrap_or_else(|| {
                    panic!(
                        "body variable hover trigger should render inside the JSON editor, editor={editor:?}"
                    )
                });
        (variable.center(), variable.left())
    };
    hover_and_wait(cx, window, variable_point);
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let popup = visual
            .debug_bounds("variable-input-tooltip-popup")
            .expect("hovered JSON body variable tooltip should render");
        assert!(
            (popup.left() - trigger_left).abs() < px(8.0),
            "tooltip should appear near the body variable, popup={:?} trigger_left={:?}",
            popup,
            trigger_left
        );
        let value_input = visual
            .debug_bounds("variable-tooltip-value-input")
            .expect("tooltip value input should render");
        assert!(
            value_input.size.width > px(0.0),
            "resolved variable value should be visible"
        );
    }
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn variable_context_resolves_once_per_frame_for_many_headers(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("variable-resolution")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request_key, cx);
            view.select_environment(Some("development".to_owned()), cx);
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
        })
        .expect("test window should be open");
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            view.variable_context_frames.set(0);
            view.environment_resolution_count.set(0);
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();
    let (frames, resolutions) = window
        .update(cx, |view, _, _| {
            (
                view.variable_context_frames.get(),
                view.environment_resolution_count.get(),
            )
        })
        .expect("test window should be open");
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
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn variable_context_reclassifies_when_the_selected_environment_changes(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            view.select_environment(Some("development".to_owned()), cx);
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, cx| {
            let resolutions_before = view.environment_resolution_count.get();
            let development = view.variable_context(cx);
            assert!(
                view.environment_resolution_count.get() > resolutions_before,
                "variable_context outside a render pass should resolve again"
            );
            assert!(development.on_manage_environments.is_some());
            assert_eq!(
                development.status("baseUrl"),
                probe_core::VariableStatus::Resolved
            );
            assert_eq!(
                development.values.get("baseUrl").map(String::as_str),
                Some("https://dev.example.com")
            );
            assert_eq!(
                development.status("token"),
                probe_core::VariableStatus::Resolved
            );
            assert_eq!(
                development.status("secretToken"),
                probe_core::VariableStatus::SecretWithoutValue
            );
            assert_eq!(
                development.status("disabledValue"),
                probe_core::VariableStatus::Missing
            );
            assert_eq!(
                development.status("missing"),
                probe_core::VariableStatus::Missing
            );

            view.select_environment(Some("base".to_owned()), cx);
            let base = view.variable_context(cx);
            assert_eq!(base.status("host"), probe_core::VariableStatus::Resolved);
            assert_eq!(
                base.values.get("host").map(String::as_str),
                Some("api.example.com")
            );
            assert_eq!(base.status("token"), probe_core::VariableStatus::Missing);
            assert_eq!(
                base.status("secretToken"),
                probe_core::VariableStatus::SecretWithoutValue
            );
            assert_eq!(
                base.status("disabledValue"),
                probe_core::VariableStatus::Missing
            );
            assert_eq!(
                base.values.get("baseUrl").map(String::as_str),
                Some("https://api.example.com")
            );

            view.shell.select_environment(None);
            let unselected = view.variable_context(cx);
            assert_eq!(
                unselected.status("baseUrl"),
                probe_core::VariableStatus::Missing
            );
            assert_eq!(
                unselected.status("secretToken"),
                probe_core::VariableStatus::Missing
            );
            assert_eq!(
                unselected.status("host"),
                probe_core::VariableStatus::Missing
            );
            assert_eq!(
                unselected.status("token"),
                probe_core::VariableStatus::Missing
            );
        })
        .expect("test window should be open");
}

#[derive(Default)]
struct FakeManagerCredentials {
    values: std::sync::Mutex<std::collections::HashMap<crate::credentials::CredentialId, String>>,
    status_calls: std::sync::atomic::AtomicUsize,
    get_calls: std::sync::atomic::AtomicUsize,
    fail_status: std::sync::atomic::AtomicBool,
    fail_set: std::sync::atomic::AtomicBool,
    slow_status: std::sync::atomic::AtomicBool,
    slow_set: std::sync::atomic::AtomicBool,
    hold_delete_until_status_reads: std::sync::atomic::AtomicBool,
    status_read_during_hold: std::sync::atomic::AtomicBool,
}

impl crate::credentials::CredentialStore for FakeManagerCredentials {
    fn status(
        &self,
        id: &crate::credentials::CredentialId,
    ) -> Result<crate::credentials::CredentialStatus, crate::credentials::CredentialStoreError>
    {
        self.status_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.slow_status.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if self.fail_status.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::credentials::CredentialStoreError::Unavailable);
        }
        let stored = self.values.lock().unwrap().contains_key(id);
        if self
            .hold_delete_until_status_reads
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.status_read_during_hold
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(if stored {
            crate::credentials::CredentialStatus::Stored
        } else {
            crate::credentials::CredentialStatus::NotStored
        })
    }
    fn set(
        &self,
        id: &crate::credentials::CredentialId,
        value: &str,
    ) -> Result<(), crate::credentials::CredentialStoreError> {
        if self.fail_set.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::credentials::CredentialStoreError::BackendFailure);
        }
        if self.slow_set.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.values
            .lock()
            .unwrap()
            .insert(id.clone(), value.to_owned());
        Ok(())
    }
    fn delete(
        &self,
        id: &crate::credentials::CredentialId,
    ) -> Result<(), crate::credentials::CredentialStoreError> {
        if self
            .hold_delete_until_status_reads
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let started = std::time::Instant::now();
            while !self
                .status_read_during_hold
                .load(std::sync::atomic::Ordering::Relaxed)
                && started.elapsed() < std::time::Duration::from_millis(500)
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        self.values
            .lock()
            .unwrap()
            .remove(id)
            .map(|_| ())
            .ok_or(crate::credentials::CredentialStoreError::NotFound)
    }
    fn get(
        &self,
        _: &crate::credentials::CredentialId,
    ) -> Result<Option<probe_core::SecretValue>, crate::credentials::CredentialStoreError> {
        self.get_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        panic!("Environment Manager must use status, never get")
    }
}

#[gpui::test]
fn manager_secret_status_uses_effective_environment_and_never_gets_value(cx: &mut TestAppContext) {
    use std::sync::{Arc, atomic::Ordering};
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    let development_id =
        crate::credentials::CredentialId::for_workspace(&fixture, "development", "secretToken")
            .unwrap();
    store
        .set(&development_id, "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
        .unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window.update(cx, |view, _, cx| {
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert_eq!(dialog.secret_statuses.get("secretToken"), Some(&super::super::SecretUiStatus::Stored));
        assert_eq!(store.get_calls.load(Ordering::Relaxed), 0);
        assert_eq!(store.status_calls.load(Ordering::Relaxed), 1);
        assert_eq!(view.loaded_workspace.as_ref().unwrap().workspace().effective_environment_variables(&dialog.draft).iter().find(|row| matches!(&row.variable, EnvironmentVariable::Secret(secret) if secret.name.as_deref() == Some("secretToken"))).unwrap().defined_in, "base");
        cx.notify();
    }).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("environment-secret-status-secretToken")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("environment-secret-replace-secretToken")
            .is_some()
    );
    visual.run_until_parked();
    assert_eq!(
        store.status_calls.load(Ordering::Relaxed),
        1,
        "rerender should not query the store"
    );
    window
        .update(cx, |view, _, cx| {
            view.select_environment_manager_environment("base", cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::NotStored)
            );
        })
        .unwrap();
}

#[gpui::test]
fn manager_new_secret_must_be_saved_before_credential_can_be_set(cx: &mut TestAppContext) {
    use std::sync::Arc;
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("new-secret")
        .canonicalize()
        .unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.variables.push(EnvironmentVariable::Secret(
                    probe_core::SecretVariable {
                        name: Some(String::new()),
                        value_type: None,
                        disabled: false,
                    },
                ))
            });
            view.save_environment_manager_dialog(window, cx);
            assert!(
                view.environment_save_task.is_none(),
                "blank secret names must be rejected"
            );
            assert!(!view.environment_manager_draft_has_required_names());
            view.apply_environment_manager_draft(cx, |dialog| {
                if let Some(EnvironmentVariable::Secret(secret)) = dialog.draft.variables.last_mut()
                {
                    secret.name = Some("newToken".into());
                }
            });
            assert!(!view.can_manage_secret("newToken"));
            view.open_secret_value_dialog("newToken".into(), window, cx);
            assert!(view.secret_value_dialog.is_none());
            view.save_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let yaml = fs::read_to_string(&fixture).unwrap();
    assert!(yaml.contains("secret: true"));
    assert!(
        yaml.contains("newToken"),
        "{}",
        window
            .update(cx, |view, _, _| format!("{:?}", toast_debug(view)))
            .unwrap()
    );
    assert!(!yaml.contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    window
        .update(cx, |view, window, cx| {
            assert!(view.can_manage_secret("newToken"));
            view.open_secret_value_dialog("newToken".into(), window, cx);
            let dialog = view.secret_value_dialog.as_ref().unwrap();
            assert!(dialog.input.read(cx).value().is_empty());
            assert!(
                !format!("{:?}", view.environment_manager_dialog.as_ref().unwrap())
                    .contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
            );
            dialog.input.update(cx, |input, cx| {
                input.set_value("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR", window, cx)
            });
            assert!(
                !format!("{:?}", view.session)
                    .contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
            );
            assert!(
                !serde_json::to_string(&view.session)
                    .unwrap()
                    .contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
            );
            view.save_secret_value(window, cx);
            assert!(
                view.secret_value_dialog
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .value()
                    .is_empty()
            );
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.secret_value_dialog.is_none());
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("newToken"),
                Some(&super::super::SecretUiStatus::Stored)
            );
        })
        .unwrap();
    assert!(
        !fs::read_to_string(&fixture)
            .unwrap()
            .contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn manager_replaces_and_deletes_native_value_without_changing_declaration(cx: &mut TestAppContext) {
    use std::sync::Arc;
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("replace-delete-secret")
        .canonicalize()
        .unwrap();
    let before = fs::read_to_string(&fixture).unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    let id =
        crate::credentials::CredentialId::for_workspace(&fixture, "development", "secretToken")
            .unwrap();
    store.set(&id, "old-value").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::Stored)
            );
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            let input = view.secret_value_dialog.as_ref().unwrap().input.clone();
            assert!(
                input.read(cx).value().is_empty(),
                "Replace must never prefill the old value"
            );
            input.update(cx, |input, cx| {
                input.set_value("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR", window, cx)
            });
            view.save_secret_value(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        store.values.lock().unwrap().get(&id).map(String::as_str),
        Some("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
    );
    assert_eq!(fs::read_to_string(&fixture).unwrap(), before);
    window
        .update(cx, |view, window, cx| {
            view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert!(!store.values.lock().unwrap().contains_key(&id));
    window
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::NotStored)
            );
            view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&fixture).unwrap(), before);
    assert_eq!(
        store.get_calls.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn manager_store_failure_and_unsaved_rename_never_write_collection(cx: &mut TestAppContext) {
    use std::sync::{Arc, atomic::Ordering};
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("secret-failure")
        .canonicalize()
        .unwrap();
    let before = fs::read_to_string(&fixture).unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    store.fail_status.store(true, Ordering::Relaxed);
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("environment-secret-retry-secretToken")
            .is_some()
    );
    window
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::Unavailable)
            );
            view.apply_environment_manager_draft(cx, |dialog| dialog.draft.name = "renamed".into());
            assert!(!view.can_manage_secret("secretToken"));
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            assert!(view.secret_value_dialog.is_none());
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.name = "development".into()
            });
            store.fail_status.store(false, Ordering::Relaxed);
            store.fail_set.store(true, Ordering::Relaxed);
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            let input = view.secret_value_dialog.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| {
                input.set_value("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR", window, cx)
            });
            view.save_secret_value(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.secret_value_dialog.as_ref().unwrap().error,
                Some("Could not save to the system credential store.")
            );
            assert!(
                view.secret_value_dialog
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .value()
                    .is_empty()
            );
            view.close_secret_value_dialog(window, cx);
        })
        .unwrap();
    assert_eq!(fs::read_to_string(&fixture).unwrap(), before);
    assert_eq!(store.get_calls.load(Ordering::Relaxed), 0);
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn manager_ignores_status_from_previous_environment(cx: &mut TestAppContext) {
    use std::sync::Arc;
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    let id =
        crate::credentials::CredentialId::for_workspace(&fixture, "development", "secretToken")
            .unwrap();
    store.set(&id, "private").unwrap();
    store
        .slow_status
        .store(true, std::sync::atomic::Ordering::Relaxed);
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store;
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.select_environment_manager_environment("base", cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let dialog = view.environment_manager_dialog.as_ref().unwrap();
            assert_eq!(dialog.original_name, "base");
            assert_eq!(
                dialog.secret_statuses.get("secretToken"),
                Some(&super::super::SecretUiStatus::NotStored)
            );
        })
        .unwrap();
}

#[gpui::test]
fn manager_renames_change_credential_identity_without_migration(cx: &mut TestAppContext) {
    use std::sync::Arc;
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("secret-identity-rename")
        .canonicalize()
        .unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    let old_id =
        crate::credentials::CredentialId::for_workspace(&fixture, "development", "secretToken")
            .unwrap();
    store.set(&old_id, "private").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.name = "production".into()
            });
            assert!(!view.can_manage_secret("secretToken"));
            view.save_environment_manager_dialog(window, cx);
            let dialog = view.application_dialog.as_ref().unwrap();
            assert_eq!(dialog.title(), "Rename environment?");
            assert_eq!(
                dialog.description(),
                "Stored secret values are associated with the environment name.\nAfter renaming, affected secrets will need to be stored again.\n\nThe existing stored credentials will not be migrated."
            );
            assert_eq!(
                dialog.primary_action(),
                Some(ApplicationDialogAction::Rename)
            );
            assert!(view.environment_save_task.is_none());
            view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
            assert!(view.application_dialog.is_none());
            assert!(view.environment_save_task.is_none());
            assert_eq!(
                view.environment_manager_dialog.as_ref().unwrap().draft.name,
                "production"
            );
        })
        .unwrap();
    assert!(
        fs::read_to_string(&fixture)
            .unwrap()
            .contains("name: development"),
        "cancel must leave the saved environment name unchanged"
    );
    assert!(
        !fs::read_to_string(&fixture)
            .unwrap()
            .contains("name: production")
    );
    window
        .update(cx, |view, window, cx| {
            view.save_environment_manager_dialog(window, cx);
            view.handle_application_dialog_action(ApplicationDialogAction::Rename, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.environment_manager_dialog.as_ref().unwrap().draft.name,
                "production"
            );
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::NotStored)
            );
        })
        .unwrap();
    assert_eq!(
        store
            .values
            .lock()
            .unwrap()
            .get(&old_id)
            .map(String::as_str),
        Some("private")
    );
    let new_id =
        crate::credentials::CredentialId::for_workspace(&fixture, "production", "secretToken")
            .unwrap();
    assert!(!store.values.lock().unwrap().contains_key(&new_id));
    window
        .update(cx, |view, window, cx| {
            view.select_environment_manager_environment("base", cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                let secret = dialog
                    .draft
                    .variables
                    .iter_mut()
                    .find_map(|variable| match variable {
                        EnvironmentVariable::Secret(secret)
                            if secret.name.as_deref() == Some("secretToken") =>
                        {
                            Some(secret)
                        }
                        _ => None,
                    })
                    .unwrap();
                secret.name = Some("renamedToken".into());
            });
            assert!(!view.can_manage_secret("renamedToken"));
            view.save_environment_manager_dialog(window, cx);
            assert!(matches!(
                &view.application_dialog,
                Some(ApplicationDialog::RenameStoredSecrets {
                    kind: StoredSecretRename::Variable { from, to },
                }) if from == "secretToken" && to == "renamedToken"
            ));
            assert_eq!(
                view.application_dialog.as_ref().unwrap().description(),
                "Stored secret values are associated with the variable name.\nAfter renaming secretToken to renamedToken, its value will need to be stored again."
            );
            view.handle_application_dialog_action(ApplicationDialogAction::Rename, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        store
            .values
            .lock()
            .unwrap()
            .get(&old_id)
            .map(String::as_str),
        Some("private")
    );
    assert!(
        fs::read_to_string(&fixture)
            .unwrap()
            .contains("renamedToken")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn manager_cancel_during_set_drops_input_and_prevents_duplicate_write(cx: &mut TestAppContext) {
    use std::sync::Arc;
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    store
        .slow_set
        .store(true, std::sync::atomic::Ordering::Relaxed);
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            let input = view.secret_value_dialog.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| {
                input.set_value("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR", window, cx)
            });
            view.save_secret_value(window, cx);
            assert!(view.secret_write_in_progress);
            view.close_secret_value_dialog(window, cx);
            assert!(view.secret_value_dialog.is_none());
            assert!(input.read(cx).value().is_empty());
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            assert!(
                view.secret_value_dialog.is_none(),
                "duplicate operation must remain blocked"
            );
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(!view.secret_write_in_progress);
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::Stored)
            );
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            assert!(
                view.secret_value_dialog
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .value()
                    .is_empty()
            );
        })
        .unwrap();
}

#[gpui::test]
fn manager_secret_keyboard_enter_submits_and_escape_discards(cx: &mut TestAppContext) {
    use std::sync::Arc;
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            let input = view.secret_value_dialog.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| input.set_value("discard-me", window, cx));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(view.secret_value_dialog.is_none());
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            let input = view.secret_value_dialog.as_ref().unwrap().input.clone();
            assert!(input.read(cx).value().is_empty());
            input.update(cx, |input, cx| {
                input.set_value("saved-by-enter", window, cx)
            });
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert!(view.secret_value_dialog.is_none());
            let path = view.workspace_path.as_ref().unwrap();
            let id =
                crate::credentials::CredentialId::for_workspace(path, "development", "secretToken")
                    .unwrap();
            assert_eq!(
                store.values.lock().unwrap().get(&id).map(String::as_str),
                Some("saved-by-enter")
            );
            assert_eq!(
                store.get_calls.load(std::sync::atomic::Ordering::Relaxed),
                0
            );
            cx.notify();
        })
        .unwrap();
}

#[gpui::test]
fn environment_manager_close_save_confirms_secret_rename_before_closing(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("secret-rename-close")
        .canonicalize()
        .unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
            view.apply_environment_manager_draft(cx, |dialog| {
                dialog.draft.name = "production".into()
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
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.environment_manager_dialog.is_none());
            assert!(view.application_dialog.is_none());
        })
        .unwrap();
    assert!(
        fs::read_to_string(&fixture)
            .unwrap()
            .contains("name: production")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn secret_save_completes_when_status_refresh_advances_generation(cx: &mut TestAppContext) {
    use std::sync::Arc;
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            view.open_secret_value_dialog("secretToken".into(), window, cx);
            let input = view.secret_value_dialog.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| {
                input.set_value("stored-during-refresh", window, cx)
            });
            view.save_secret_value(window, cx);
            assert!(view.secret_value_dialog.as_ref().unwrap().busy);
            view.refresh_secret_statuses(cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(
                view.secret_value_dialog.is_none(),
                "a successful write closes its dialog even if a status refresh advanced the generation"
            );
            assert!(!view.secret_write_in_progress);
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::Stored)
            );
            let path = view.workspace_path.as_ref().unwrap();
            let id =
                crate::credentials::CredentialId::for_workspace(path, "development", "secretToken")
                    .unwrap();
            assert_eq!(
                store.values.lock().unwrap().get(&id).map(String::as_str),
                Some("stored-during-refresh")
            );
        })
        .unwrap();
}

#[gpui::test]
fn secret_delete_refreshes_status_when_a_refresh_overlaps_deletion(cx: &mut TestAppContext) {
    use std::sync::{Arc, atomic::Ordering};
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let store = Arc::new(FakeManagerCredentials::default());
    let id =
        crate::credentials::CredentialId::for_workspace(&fixture, "development", "secretToken")
            .unwrap();
    store.set(&id, "private").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.credential_store = store.clone();
            view.set_workspace(fixture, workspace);
            view.select_environment(Some("development".into()), cx);
            view.open_environment_manager_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::Stored)
            );
            store
                .hold_delete_until_status_reads
                .store(true, Ordering::Relaxed);
            store
                .status_read_during_hold
                .store(false, Ordering::Relaxed);
            view.delete_stored_secret("secretToken".into(), "development".into(), window, cx);
            view.refresh_secret_statuses(cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(store.status_read_during_hold.load(Ordering::Relaxed));
            assert!(!store.values.lock().unwrap().contains_key(&id));
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .secret_statuses
                    .get("secretToken"),
                Some(&super::super::SecretUiStatus::NotStored)
            );
            assert_eq!(store.get_calls.load(Ordering::Relaxed), 0);
        })
        .unwrap();
}
