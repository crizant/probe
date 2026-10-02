use super::*;
use crate::app::chrome::environment_variable_text;
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
    let mut dialog = crate::app::EnvironmentManagerDialog::new(&environment);
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
fn environment_switcher_is_available_without_a_request_and_offers_actions(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, _, cx| {
        assert!(view.shell.active_tab().is_none());
        cx.notify();
    });
    cx.run_until_parked();

    let mut visual = workspace.visual(cx);
    let trigger = visual
        .debug_bounds("request-environment-trigger")
        .expect("environment switcher should render without a selected request");
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
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "base");

    {
        let mut visual = workspace.visual(cx);
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
            .expect("add-environment control should render");
        assert!(
            visual.debug_bounds("environment-manager-delete").is_none(),
            "delete should live in the environment context menu, not the sidebar"
        );
        visual
            .debug_bounds("environment-manager-add-variable")
            .expect("add-variable action should render");
        visual
            .debug_bounds("environment-manager-add-secret")
            .expect("add-secret action should render");
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

    workspace.update(cx, |view, _, cx| {
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
    });
    cx.run_until_parked();

    let mut visual = workspace.visual(cx);
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
fn environment_manager_scrolls_variables_when_the_pointer_is_over_a_field(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "base");
    workspace.update(cx, |view, _, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            for index in 0..40 {
                dialog.add_variable(EnvironmentVariable::Plain(Variable {
                    name: Some(format!("scroll-{index}")),
                    value: Some(VariableValueSet::Single(VariableValue::String(format!(
                        "value-{index}"
                    )))),
                    disabled: false,
                }));
            }
        });
    });
    cx.run_until_parked();

    let offset_y = |workspace: &EnvironmentWorkspace, cx: &mut TestAppContext| {
        workspace.update(cx, |view, _, _| {
            view.environment_variables_scroll
                .0
                .borrow()
                .base_handle
                .offset()
                .y
        })
    };
    let mut visual = workspace.visual(cx);
    let field = visual
        .debug_bounds("environment-variable-value-host")
        .expect("a value field should accept the pointer");
    // Keep the pointer stationary while rows and their gaps pass underneath it.
    for tick in 0..48 {
        let delta_y = if tick >= 24 && tick % 2 == 0 {
            px(8.0)
        } else {
            px(-8.0)
        };
        let before = offset_y(&workspace, cx);
        visual.simulate_event(gpui::ScrollWheelEvent {
            position: field.center(),
            delta: gpui::ScrollDelta::Pixels(if tick % 3 == 0 {
                point(px(0.0), delta_y)
            } else {
                point(px(2.0), delta_y)
            }),
            modifiers: Modifiers::default(),
            touch_phase: if tick == 0 {
                gpui::TouchPhase::Started
            } else {
                gpui::TouchPhase::Moved
            },
        });
        assert_eq!(offset_y(&workspace, cx), before + delta_y);
        // Burst several events between frames, including rapid reversals.
        if tick % 4 == 3 {
            visual.run_until_parked();
        }
    }
    let before = offset_y(&workspace, cx);
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: field.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
    let after = offset_y(&workspace, cx);
    assert!(
        after < before,
        "wheeling over a value field should scroll the list, before={before:?} after={after:?}"
    );

    workspace.update(cx, |view, _, cx| {
        view.environment_variables_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.0), px(0.0)));
        cx.notify();
    });
    visual.run_until_parked();
    let field = visual
        .debug_bounds("environment-variable-value-host")
        .expect("the value field should return to the top of the list");
    visual.simulate_click(field.center(), Modifiers::default());
    visual.run_until_parked();
    let focused_before = offset_y(&workspace, cx);
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: field.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(12.0), px(-80.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
    let focused = offset_y(&workspace, cx);
    assert!(
        focused < focused_before,
        "a focused field should still let a vertical wheel scroll the list, before={focused_before:?} after={focused:?}"
    );
}

#[gpui::test]
fn environment_manager_retains_only_the_focused_virtualized_field(cx: &mut TestAppContext) {
    for field_kind in ["name", "value"] {
        let workspace = EnvironmentWorkspace::open(cx);
        workspace.open_manager(cx, "base");
        workspace.update(cx, |_, window, _| window.activate_window());
        workspace.update(cx, |view, _, cx| {
            view.apply_environment_manager_draft(cx, |dialog| {
                for index in 0..80 {
                    dialog.add_variable(EnvironmentVariable::Plain(Variable {
                        name: Some(format!("focus-{index}")),
                        value: Some(VariableValueSet::Single(VariableValue::String(format!(
                            "value-{index}"
                        )))),
                        disabled: false,
                    }));
                }
            });
        });
        workspace.update(cx, |view, _, cx| {
            view.environment_variables_scroll
                .scroll_to_item_strict(4, ScrollStrategy::Top);
            cx.notify();
        });
        cx.run_until_parked();
        let mut visual = workspace.visual(cx);
        let selector = if field_kind == "name" {
            "environment-variable-name-focus-0"
        } else {
            "environment-variable-value-focus-0"
        };
        let field = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(field.center(), Modifiers::default());
        visual.run_until_parked();
        let (focus, controller, row_id) = workspace.update(cx, |view, window, cx| {
            let (id, _, field) = view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .active_field
                .as_ref()
                .unwrap();
            (window.focused(cx).unwrap(), field.entity_id(), id.clone())
        });
        let scroll = |index, workspace: &EnvironmentWorkspace, cx: &mut TestAppContext| {
            workspace.update(cx, |view, _, cx| {
                view.environment_variables_scroll
                    .scroll_to_item_strict(index, ScrollStrategy::Top);
                cx.notify();
            });
        };
        scroll(70, &workspace, cx);
        visual.run_until_parked();
        assert!(
            visual.debug_bounds(selector).is_none(),
            "active row must actually be virtualized away"
        );
        workspace.update(cx, |view, window, cx| {
            assert_eq!(window.focused(cx), Some(focus.clone()));
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .active_field
                    .as_ref()
                    .unwrap()
                    .2
                    .entity_id(),
                controller
            );
            // Remove a preceding row while the editor is offscreen. Its callback must follow the stable ID.
            view.apply_environment_manager_draft(cx, |dialog| dialog.remove_variable(0));
        });
        visual.run_until_parked();
        scroll(0, &workspace, cx);
        visual.run_until_parked();
        visual.debug_bounds(selector).unwrap();
        workspace.update(cx, |view, window, cx| {
            assert_eq!(window.focused(cx), Some(focus.clone()));
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .active_field
                    .as_ref()
                    .unwrap()
                    .2
                    .entity_id(),
                controller
            );
        });
        cx.simulate_input(workspace.window.into(), "edited");
        visual.run_until_parked();
        workspace.update(cx, |view, _, _| {
            let dialog = view.environment_manager_dialog.as_ref().unwrap();
            let index = dialog
                .variable_row_ids
                .iter()
                .position(|id| crate::app::dialogs::EnvironmentVariableRowId::Direct(*id) == row_id)
                .unwrap();
            let EnvironmentVariable::Plain(variable) = &dialog.draft.variables[index] else {
                panic!("plain variable")
            };
            if field_kind == "name" {
                assert!(variable.name.as_ref().unwrap().contains("edited"));
            } else {
                assert!(environment_variable_text(variable).0.contains("edited"));
            }
            let EnvironmentVariable::Plain(next) = &dialog.draft.variables[index + 1] else {
                panic!("plain variable")
            };
            assert_eq!(next.name.as_deref(), Some("focus-1"));
            assert_eq!(environment_variable_text(next).0, "value-1");
        });
        scroll(70, &workspace, cx);
        visual.run_until_parked();
        // The name may have changed, so check the value cell as the row's visibility marker.
        let updated_name = workspace.update(cx, |view, _, _| {
            let dialog = view.environment_manager_dialog.as_ref().unwrap();
            let index = dialog
                .variable_row_ids
                .iter()
                .position(|id| crate::app::dialogs::EnvironmentVariableRowId::Direct(*id) == row_id)
                .unwrap();
            let EnvironmentVariable::Plain(variable) = &dialog.draft.variables[index] else {
                panic!("plain variable")
            };
            variable.name.clone().unwrap()
        });
        let updated_selector: &'static str =
            Box::leak(format!("environment-variable-value-{updated_name}").into_boxed_str());
        assert!(visual.debug_bounds(updated_selector).is_none());
        let other = visual
            .debug_bounds("environment-variable-value-focus-68")
            .unwrap();
        visual.simulate_click(other.center(), Modifiers::default());
        visual.run_until_parked();
        let (other_focus, other_id, weak_field) = workspace.update(cx, |view, window, cx| {
            let (id, _, field) = view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .active_field
                .as_ref()
                .unwrap();
            assert_ne!(field.entity_id(), controller);
            (window.focused(cx).unwrap(), id.clone(), field.downgrade())
        });
        scroll(0, &workspace, cx);
        visual.run_until_parked();
        visual.debug_bounds(updated_selector).unwrap();
        workspace.update(cx, |view, window, cx| {
            assert_eq!(window.focused(cx), Some(other_focus));
            view.apply_environment_manager_draft(cx, |dialog| {
                let index = dialog
                    .variable_row_ids
                    .iter()
                    .position(|id| {
                        crate::app::dialogs::EnvironmentVariableRowId::Direct(*id) == other_id
                    })
                    .unwrap();
                dialog.remove_variable(index);
                assert!(dialog.active_field.is_none());
            });
        });
        visual.run_until_parked();
        assert!(
            weak_field.upgrade().is_none(),
            "removed offscreen field must be released"
        );
        let field = visual.debug_bounds(updated_selector).unwrap();
        visual.simulate_click(field.center(), Modifiers::default());
        visual.run_until_parked();
        let blurred_field = workspace.update(cx, |view, _, _| {
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .active_field
                .as_ref()
                .unwrap()
                .2
                .downgrade()
        });
        scroll(70, &workspace, cx);
        visual.run_until_parked();
        workspace.update(cx, |view, window, cx| window.focus(&view.focus_handle, cx));
        visual.run_until_parked();
        workspace.update(cx, |view, _, _| {
            assert!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .active_field
                    .is_none()
            )
        });
        assert!(
            blurred_field.upgrade().is_none(),
            "ending an offscreen edit must release its controller"
        );
        scroll(0, &workspace, cx);
        visual.run_until_parked();
        let field = visual.debug_bounds(updated_selector).unwrap();
        visual.simulate_click(field.center(), Modifiers::default());
        visual.run_until_parked();
        workspace.update(cx, |view, _, cx| {
            assert!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .active_field
                    .is_some()
            );
            // Discard the test draft so switching does not open a dirty-draft prompt.
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
            view.select_environment_manager_environment("development", cx);
            assert!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .active_field
                    .is_none()
            );
        });
        visual.run_until_parked();
        let field = visual
            .debug_bounds("environment-variable-value-host")
            .unwrap();
        visual.simulate_click(field.center(), Modifiers::default());
        visual.run_until_parked();
        workspace.update(cx, |view, window, cx| {
            assert!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .active_field
                    .is_some()
            );
            view.close_environment_manager_dialog(window, cx);
            assert!(view.environment_manager_dialog.is_none());
        });
        visual.run_until_parked();
    }
}

#[gpui::test]
fn inherited_value_keeps_its_controller_when_virtualized_and_promoted_to_an_override(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        window.activate_window();
        view.apply_environment_manager_draft(cx, |dialog| {
            for index in 0..80 {
                dialog.add_variable(EnvironmentVariable::Plain(Variable {
                    name: Some(format!("inherited-focus-{index}")),
                    value: Some(VariableValueSet::Single(VariableValue::String(
                        "filler".into(),
                    ))),
                    disabled: false,
                }));
            }
        });
    });
    let inherited_index = workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft
            .variables
            .len()
    });
    let scroll = |index, workspace: &EnvironmentWorkspace, cx: &mut TestAppContext| {
        workspace.update(cx, |view, _, cx| {
            view.environment_variables_scroll
                .scroll_to_item_strict(index, ScrollStrategy::Top);
            cx.notify();
        });
    };
    scroll(inherited_index, &workspace, cx);
    cx.run_until_parked();
    let mut visual = workspace.visual(cx);
    assert!(
        visual
            .debug_bounds("environment-variable-name-baseUrl")
            .is_none(),
        "inherited names stay read-only"
    );
    let field = visual
        .debug_bounds("environment-variable-value-baseUrl")
        .unwrap();
    visual.simulate_click(field.center(), Modifiers::default());
    visual.run_until_parked();
    let (focus, controller) = workspace.update(cx, |view, window, cx| {
        let active = view
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .active_field
            .as_ref()
            .expect("inherited value must retain its controller");
        assert!(matches!(&active.0, crate::app::dialogs::EnvironmentVariableRowId::Inherited { defined_in, name } if defined_in == "base" && name == "baseUrl"));
        (window.focused(cx).unwrap(), active.2.entity_id())
    });
    scroll(0, &workspace, cx);
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds("environment-variable-value-baseUrl")
            .is_none(),
        "inherited row must leave the rendered range"
    );
    scroll(inherited_index, &workspace, cx);
    visual.run_until_parked();
    visual
        .debug_bounds("environment-variable-value-baseUrl")
        .unwrap();
    workspace.update(cx, |view, window, cx| {
        assert_eq!(window.focused(cx), Some(focus.clone()));
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .active_field
                .as_ref()
                .unwrap()
                .2
                .entity_id(),
            controller
        );
    });
    cx.simulate_input(workspace.window.into(), "!");
    visual.run_until_parked();
    // The first edit changes the row's identity and makes its name directly editable.
    visual
        .debug_bounds("environment-variable-name-baseUrl")
        .unwrap();
    workspace.update(cx, |view, window, cx| {
        assert_eq!(window.focused(cx), Some(focus.clone()));
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .active_field
                .as_ref()
                .unwrap()
                .2
                .entity_id(),
            controller
        );
    });
    cx.simulate_input(workspace.window.into(), "continued");
    visual.run_until_parked();
    let direct_index = workspace.update(cx, |view, window, cx| {
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        let matches: Vec<_> = dialog
            .draft
            .variables
            .iter()
            .enumerate()
            .filter_map(|(index, variable)| match variable {
                EnvironmentVariable::Plain(variable)
                    if variable.name.as_deref() == Some("baseUrl") =>
                {
                    Some((index, variable))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "continued typing must update one override"
        );
        assert_eq!(
            dialog.active_field.as_ref().unwrap().0,
            crate::app::dialogs::EnvironmentVariableRowId::Direct(
                dialog.variable_row_ids[matches[0].0]
            )
        );
        let value = environment_variable_text(matches[0].1).0;
        assert!(
            value.contains('!') && value.contains("continued"),
            "override value: {value}"
        );
        assert_eq!(window.focused(cx), Some(focus.clone()));
        assert_eq!(
            dialog.active_field.as_ref().unwrap().2.entity_id(),
            controller
        );
        let parent = view
            .loaded_workspace
            .as_ref()
            .unwrap()
            .workspace()
            .environments()
            .iter()
            .find(|environment| environment.name == "base")
            .unwrap();
        let parent_variable = parent
            .variables
            .iter()
            .find_map(|variable| match variable {
                EnvironmentVariable::Plain(variable)
                    if variable.name.as_deref() == Some("baseUrl") =>
                {
                    Some(variable)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            environment_variable_text(parent_variable).0,
            "https://{{host}}"
        );
        matches[0].0
    });
    scroll(0, &workspace, cx);
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds("environment-variable-value-baseUrl")
            .is_none()
    );
    scroll(direct_index, &workspace, cx);
    visual.run_until_parked();
    visual
        .debug_bounds("environment-variable-value-baseUrl")
        .unwrap();
    workspace.update(cx, |view, window, cx| {
        assert_eq!(window.focused(cx), Some(focus));
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .active_field
                .as_ref()
                .unwrap()
                .2
                .entity_id(),
            controller
        );
    });
}

#[gpui::test]
fn environment_manager_keeps_a_horizontal_wheel_on_a_focused_overflowing_field(
    cx: &mut TestAppContext,
) {
    let overflow_value = "x".repeat(400);
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "base");
    workspace.update(cx, |view, _, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            for variable in &mut dialog.draft.variables {
                let EnvironmentVariable::Plain(variable) = variable else {
                    continue;
                };
                if variable.name.as_deref() == Some("host") {
                    variable.value = Some(VariableValueSet::Single(VariableValue::String(
                        overflow_value.clone(),
                    )));
                }
            }
            for index in 0..40 {
                dialog.add_variable(EnvironmentVariable::Plain(Variable {
                    name: Some(format!("scroll-{index}")),
                    value: Some(VariableValueSet::Single(VariableValue::String(format!(
                        "value-{index}"
                    )))),
                    disabled: false,
                }));
            }
        });
    });
    cx.run_until_parked();

    let mut visual = workspace.visual(cx);
    let field = visual
        .debug_bounds("environment-variable-value-host")
        .expect("the overflowing value field should accept the pointer");
    visual.simulate_click(field.center(), Modifiers::default());
    visual.run_until_parked();

    let list_before = workspace.update(cx, |view, _, _| {
        view.environment_variables_scroll
            .0
            .borrow()
            .base_handle
            .offset()
            .y
    });
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: field.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(-160.0), px(-6.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
    let list_after = workspace.update(cx, |view, _, _| {
        view.environment_variables_scroll
            .0
            .borrow()
            .base_handle
            .offset()
            .y
    });
    assert_eq!(
        list_after, list_before,
        "a horizontal wheel on a focused overflowing field should leave the list in place"
    );
}

#[gpui::test]
fn environment_manager_scrolls_a_new_row_into_view(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "base");
    workspace.update(cx, |view, _, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            for index in 0..40 {
                dialog.add_variable(EnvironmentVariable::Plain(Variable {
                    name: Some(format!("scroll-{index}")),
                    value: Some(VariableValueSet::Single(VariableValue::String(format!(
                        "value-{index}"
                    )))),
                    disabled: false,
                }));
            }
        });
    });
    cx.run_until_parked();

    let variable_index = workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft
            .variables
            .len()
    });
    let mut visual = workspace.visual(cx);
    let add_variable = visual
        .debug_bounds("environment-manager-add-variable")
        .expect("add variable should render");
    visual.simulate_click(add_variable.center(), Modifiers::default());
    visual.run_until_parked();
    let variable_name: &'static str =
        Box::leak(format!("environment-variable-name-{variable_index}").into_boxed_str());
    visual
        .debug_bounds(variable_name)
        .expect("the new variable row should scroll into view");
    let offset_y = workspace.update(cx, |view, _, _| {
        view.environment_variables_scroll
            .0
            .borrow()
            .base_handle
            .offset()
            .y
    });
    assert!(
        offset_y < px(0.0),
        "adding a variable below the fold should scroll the list, offset={offset_y:?}"
    );

    let add_secret = visual
        .debug_bounds("environment-manager-add-secret")
        .expect("add secret should render");
    visual.simulate_click(add_secret.center(), Modifiers::default());
    visual.run_until_parked();
    let secret_name: &'static str =
        Box::leak(format!("environment-variable-name-{}", variable_index + 1).into_boxed_str());
    visual
        .debug_bounds(secret_name)
        .expect("the new secret row should scroll into view");
    workspace.update(cx, |view, _, _| {
        assert!(matches!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .draft
                .variables
                .last(),
            Some(EnvironmentVariable::Secret(_))
        ));
    });
}

#[gpui::test]
fn environment_manager_virtualizes_variables_and_preserves_row_identity(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, window, cx| {
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
    });
    cx.run_until_parked();

    let plain_value = |view: &ProbeApp, name: &str| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft
            .variables
            .iter()
            .find_map(|variable| match variable {
                EnvironmentVariable::Plain(variable) if variable.name.as_deref() == Some(name) => {
                    Some(environment_variable_text(variable).0)
                }
                _ => None,
            })
            .unwrap()
    };

    let mut visual = workspace.visual(cx);
    let first_value = visual
        .debug_bounds("environment-variable-value-virtual-0")
        .expect("first variable should render");
    visual.simulate_click(
        point(first_value.right() - px(8.0), first_value.center().y),
        Modifiers::default(),
    );
    cx.simulate_input(workspace.window.into(), "draft");
    visual.run_until_parked();
    let first_draft = workspace.update(cx, |view, _, _| plain_value(view, "virtual-0"));
    assert!(first_draft.contains("draft"));
    assert!(
        visual
            .debug_bounds("environment-variable-value-virtual-499")
            .is_none(),
        "offscreen rows should not render"
    );
    visual
        .debug_bounds("environment-manager-add-variable")
        .expect("add variable should stay available while the list is scrolled");
    visual
        .debug_bounds("environment-manager-add-secret")
        .expect("add secret should stay available while the list is scrolled");
    let rendered = workspace.update(cx, |view, _, _| view.rendered_environment_variable_rows);
    assert!(rendered > 0 && rendered < 40, "rendered {rendered} rows");

    workspace.update(cx, |view, _, cx| {
        let last_index = view
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft
            .variables
            .len()
            .saturating_sub(1);
        view.environment_variables_scroll
            .scroll_to_item_strict(last_index, ScrollStrategy::Bottom);
        cx.notify();
    });
    visual.run_until_parked();
    visual
        .debug_bounds("environment-variable-value-virtual-499")
        .expect("scrolling should render the last variable");
    visual
        .debug_bounds("environment-manager-add-variable")
        .expect("add variable should stay available after scrolling to the end");
    visual
        .debug_bounds("environment-manager-add-secret")
        .expect("add secret should stay available after scrolling to the end");
    assert!(
        visual
            .debug_bounds("environment-variable-value-virtual-0")
            .is_none()
    );

    workspace.update(cx, |view, _, cx| {
        let dialog = view.environment_manager_dialog.as_mut().unwrap();
        let retained_id = dialog.variable_row_ids[2];
        dialog.remove_variable(1);
        assert_eq!(dialog.variable_row_ids[1], retained_id);
        view.environment_variables_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        cx.notify();
    });
    visual.run_until_parked();
    let first_value = visual
        .debug_bounds("environment-variable-value-virtual-0")
        .expect("remaining first variable should render");
    let retained_draft = workspace.update(cx, |view, _, _| plain_value(view, "virtual-0"));
    assert_eq!(retained_draft, first_draft);
    visual.simulate_click(
        point(first_value.right() - px(8.0), first_value.center().y),
        Modifiers::default(),
    );
    cx.simulate_input(workspace.window.into(), "more");
    visual.run_until_parked();
    workspace.update(cx, |view, _, _| {
        let restored = plain_value(view, "virtual-0");
        assert!(
            restored.contains("draft") && restored.contains("more"),
            "value after restoring virtualized row: {restored:?}"
        );
        assert_eq!(plain_value(view, "virtual-1"), "value-1");
    });
    visual
        .debug_bounds("environment-variable-value-virtual-1")
        .expect("remaining second variable should render");

    workspace.update(cx, |view, _, cx| {
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
    });
}

#[gpui::test]
fn environment_manager_protects_dirty_draft_and_restores_create_focus(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, window, cx| {
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
        view.environment_manager_dialog.as_mut().unwrap().draft.name =
            "renamed-development".to_owned();
    });
    cx.run_until_parked();

    {
        let mut visual = workspace.visual(cx);
        let add = visual
            .debug_bounds("environment-manager-add")
            .expect("add-environment control should render");
        visual.simulate_click(add.center(), Modifiers::default());
        visual.run_until_parked();
    }

    workspace.update(cx, |view, window, cx| {
        let draft_name = |view: &ProbeApp| {
            view.environment_manager_dialog
                .as_ref()
                .map(|dialog| dialog.draft.name.clone())
        };
        assert!(view.create_environment_dialog.is_none());
        assert_eq!(draft_name(view).as_deref(), Some("renamed-development"));
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
        assert_eq!(draft_name(view).as_deref(), Some("renamed-development"));
    });
}

#[gpui::test]
fn environment_manager_validation_errors_are_scoped_and_dismissible(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        view.show_toast(ToastIntent::Error, "App-level error", cx);
        view.environment_manager_dialog.as_mut().unwrap().draft.name = "  ".to_owned();
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
    });
    cx.run_until_parked();

    let mut visual = workspace.visual(cx);
    visual.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    visual.run_until_parked();
    let close = visual
        .debug_bounds("toast-close-1")
        .expect("the validation toast should expose a close action");
    visual.simulate_click(close.center(), Modifiers::default());
    visual.run_until_parked();

    workspace.update(cx, |view, _, _| {
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
    });
}

#[gpui::test]
fn environment_manager_routes_blocked_save_and_create_failures_to_its_error(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, window, cx| {
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

        view.environment_manager_dialog.as_mut().unwrap().draft.name = "base".to_owned();
        view.save_environment_manager_dialog(window, cx);
        assert!(
            has_active_toast(view, ToastIntent::Error, "Could not save environment:"),
            "{:?}",
            toast_debug(view)
        );

        view.environment_manager_dialog.as_mut().unwrap().draft.name = "development".to_owned();
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
    });
}

#[gpui::test]
fn environment_dialog_auto_dismisses_errors_when_their_condition_resolves(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        view.environment_manager_dialog.as_mut().unwrap().draft.name = "  ".to_owned();
        view.save_environment_manager_dialog(window, cx);
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft.name = "development".to_owned();
        });
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(view.environment_dialog_error.is_none())
    });

    workspace.update(cx, |view, window, cx| {
        view.pending_environment_saves
            .insert(("development".to_owned(), "host".to_owned()));
        view.save_environment_manager_dialog(window, cx);
        view.pending_environment_saves.clear();
        cx.notify();
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(view.environment_dialog_error.is_none())
    });

    workspace.update(cx, |view, window, cx| {
        view.environment_manager_dialog.as_mut().unwrap().draft.name = "base".to_owned();
        view.save_environment_manager_dialog(window, cx);
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft.name = "development".to_owned();
        });
    });
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
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
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(view.environment_dialog_error.is_none());
        assert_eq!(view.create_environment_dialog.as_deref(), Some("staging"));
    });
}

fn saved_environment(workspace: &EnvironmentWorkspace, name: &str) -> Option<Environment> {
    probe_opencollection::load_workspace(&workspace.path)
        .expect("saved environments should load")
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == name)
        .cloned()
}

#[gpui::test]
fn environment_manager_saves_plain_variables_and_parent(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-save");
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        let dialog = view.environment_manager_dialog.as_mut().unwrap();
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
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
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
    });
    let development = saved_environment(&workspace, "development").unwrap();
    assert_eq!(development.extends, None);
    assert!(development.variables.iter().any(|variable| matches!(
        variable,
        EnvironmentVariable::Plain(variable) if variable.name.as_deref() == Some("region")
    )));
}

#[gpui::test]
fn platform_save_hotkey_saves_dirty_environment_manager(cx: &mut TestAppContext) {
    cx.update(bind_platform_hotkeys);
    let workspace = EnvironmentWorkspace::writable(cx, "manager-save-hotkey");
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft.extends = None;
        });
        assert!(!view.environment_manager_save_disabled());
    });
    cx.run_until_parked();

    cx.simulate_keystrokes(workspace.window.into(), save_shortcut());
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
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
    });
    assert_eq!(
        saved_environment(&workspace, "development")
            .unwrap()
            .extends,
        None
    );
}

#[gpui::test]
fn platform_save_hotkey_is_disabled_when_environment_manager_has_nothing_to_save(
    cx: &mut TestAppContext,
) {
    cx.update(bind_platform_hotkeys);
    let workspace = EnvironmentWorkspace::writable(cx, "manager-save-hotkey-clean");
    let request_key = workspace.update(cx, |view, window, cx| {
        let request_key = view.loaded_workspace.as_ref().unwrap().requests()[0].key();
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
        request_key
    });
    cx.run_until_parked();

    cx.simulate_keystrokes(workspace.window.into(), save_shortcut());
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
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
    });
}

#[gpui::test]
fn platform_save_hotkey_is_disabled_while_environment_manager_save_is_busy(
    cx: &mut TestAppContext,
) {
    cx.update(bind_platform_hotkeys);
    let workspace = EnvironmentWorkspace::writable(cx, "manager-save-hotkey-busy");
    workspace.update(cx, |view, window, cx| {
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
    });
    cx.run_until_parked();
}

#[gpui::test]
fn environment_manager_save_ignores_edits_made_while_busy(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-save-busy");
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        view.save_environment_manager_dialog(window, cx);
        assert!(view.environment_save_task.is_some());
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft.name = "hijacked".to_owned();
        });
        view.environment_manager_dialog.as_mut().unwrap().draft.name = "hijacked".to_owned();
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
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
    });
    assert!(saved_environment(&workspace, "development").is_some());
    assert!(saved_environment(&workspace, "hijacked").is_none());
}

fn manager_environment(view: &ProbeApp) -> Option<&str> {
    view.environment_manager_dialog
        .as_ref()
        .map(|dialog| dialog.original_name.as_str())
}

#[gpui::test]
fn environment_manager_deletes_a_leaf_environment(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-delete");
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        view.delete_environment("development".to_owned(), window, cx);
    });
    cx.run_until_parked();

    workspace.update(cx, |view, window, cx| {
        assert!(
            view.environment_save_task.is_none(),
            "{:?}",
            toast_debug(view)
        );
        assert_eq!(manager_environment(view), Some("base"));
        assert_eq!(
            window.focused(cx),
            Some(view.environment_manager_dialog_focus.clone())
        );
    });
    assert!(saved_environment(&workspace, "development").is_none());
}

#[gpui::test]
fn environment_manager_delete_preserves_a_dirty_draft_for_another_environment(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-delete-other");
    workspace.update(cx, |view, window, cx| {
        view.create_named_environment("staging".to_owned(), window, cx)
    });
    cx.run_until_parked();

    workspace.update(cx, |view, window, cx| {
        assert!(
            view.environment_save_task.is_none(),
            "{:?}",
            toast_debug(view)
        );
        view.select_environment_manager_environment("development", cx);
        let dialog = view.environment_manager_dialog.as_mut().unwrap();
        assert_eq!(dialog.original_name, "development");
        dialog.draft.name = "renamed-development".to_owned();
        view.delete_environment("staging".to_owned(), window, cx);
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
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
    });
}

#[gpui::test]
fn environment_manager_delete_selects_a_neighbor(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-delete-neighbor");
    workspace.update(cx, |view, window, cx| {
        view.create_named_environment("staging".to_owned(), window, cx)
    });
    cx.run_until_parked();

    let previous_scroll = workspace.update(cx, |view, window, cx| {
        assert!(
            view.environment_save_task.is_none(),
            "{:?}",
            toast_debug(view)
        );
        view.select_environment_manager_environment("development", cx);
        let previous_scroll = view.environment_variables_scroll.0.clone();
        view.delete_environment("development".to_owned(), window, cx);
        previous_scroll
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
        assert!(
            view.environment_save_task.is_none(),
            "{:?}",
            toast_debug(view)
        );
        assert_eq!(manager_environment(view), Some("staging"));
        assert!(!Rc::ptr_eq(
            &previous_scroll,
            &view.environment_variables_scroll.0
        ));
    });
}

#[gpui::test]
fn environment_manager_delete_of_the_last_environment_closes(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-delete-last");
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        view.delete_environment("development".to_owned(), window, cx);
    });
    cx.run_until_parked();

    workspace.update(cx, |view, window, cx| {
        assert!(
            view.environment_save_task.is_none(),
            "{:?}",
            toast_debug(view)
        );
        assert_eq!(manager_environment(view), Some("base"));
        view.delete_environment("base".to_owned(), window, cx);
    });
    cx.run_until_parked();

    workspace.update(cx, |view, window, cx| {
        assert!(
            view.environment_save_task.is_none(),
            "{:?}",
            toast_debug(view)
        );
        assert!(view.environment_manager_dialog.is_none());
        assert_eq!(window.focused(cx), Some(view.focus_handle.clone()));
    });
}

#[gpui::test]
fn environment_manager_closes_when_the_workspace_resets(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let other = http_environment_fixture().canonicalize().unwrap();
    let other_workspace = probe_opencollection::load_workspace(&other).unwrap();
    workspace.update(cx, |view, window, cx| {
        view.open_environment_manager_dialog(window, cx);
        view.environment_manager_dialog.as_mut().unwrap().draft.name =
            "renamed-development".to_owned();
        view.set_workspace(other, other_workspace);
        assert!(view.environment_manager_dialog.is_none());
        let reloaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
        view.set_workspace(workspace.path.clone(), reloaded);
        view.open_environment_manager_dialog(window, cx);
        view.close_workspace_now(cx);
        assert!(view.environment_manager_dialog.is_none());
    });
}

#[gpui::test]
fn environment_manager_rebinds_after_workspace_reload(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-reload");
    workspace.mark_stored(cx, &workspace.development_secret());
    let reload =
        || reconciled_workspace(probe_opencollection::load_workspace(&workspace.path).unwrap());
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Stored)
        );
        view.apply_reconciled_workspace(reload(), cx);
        assert_eq!(manager_environment(view), Some("development"));
        view.environment_manager_dialog.as_mut().unwrap().draft.name =
            "renamed-development".to_owned();
        view.apply_reconciled_workspace(reload(), cx);
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .map(|dialog| dialog.draft.name.as_str()),
            Some("renamed-development")
        );
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Stored),
            "a reload must not relabel a stored secret from the unsaved environment name"
        );
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog.draft.name = "development".to_owned();
        });
        assert_eq!(
            manager_status(view, "secretToken"),
            Some(SecretUiStatus::Stored)
        );
        view.environment_manager_dialog.as_mut().unwrap().draft.name =
            "renamed-development".to_owned();
        assert!(view.toasts.is_empty(), "{:?}", toast_debug(view));
    });

    let mut changed = probe_opencollection::load_workspace(&workspace.path).unwrap();
    let mut replacement = changed.workspace().environments()[1].clone();
    replacement.extends = None;
    let saved = changed
        .prepare_environment_replace("development", replacement)
        .unwrap()
        .execute()
        .unwrap();
    changed.complete_environment_replace(saved).unwrap();

    workspace.update(cx, |view, _, cx| {
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
    });

    let mut remaining = probe_opencollection::load_workspace(&workspace.path).unwrap();
    let saved = remaining
        .prepare_environment_delete("development")
        .unwrap()
        .execute()
        .unwrap();
    remaining.complete_environment_delete(saved).unwrap();

    workspace.update(cx, |view, _, cx| {
        view.apply_reconciled_workspace(reconciled_workspace(remaining), cx);
        assert!(view.environment_manager_dialog.is_none());
    });
}

#[gpui::test]
fn environment_manager_cancel_with_unsaved_changes_prompts(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, window, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.open_environment_manager_dialog(window, cx);
        view.environment_manager_dialog.as_mut().unwrap().draft.name =
            "renamed-development".to_owned();
        view.request_close_environment_manager_dialog(window, cx);
        assert!(matches!(
            view.application_dialog,
            Some(ApplicationDialog::UnsavedEnvironment)
        ));
        assert!(view.environment_manager_dialog.is_some());
    });
    cx.run_until_parked();

    {
        let mut visual = workspace.visual(cx);
        let cancel = visual
            .debug_bounds("application-dialog-cancel")
            .expect("unsaved environment warning should render Cancel");
        visual.simulate_click(cancel.center(), Modifiers::default());
        visual.run_until_parked();
    }

    workspace.update(cx, |view, window, cx| {
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
    });
}

#[gpui::test]
fn environment_manager_context_menu_deletes_an_environment(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "manager-context-delete");
    workspace.open_manager(cx, "development");

    {
        let mut visual = workspace.visual(cx);
        let dialog = visual
            .debug_bounds("environment-manager-dialog")
            .expect("environment manager should render");
        workspace.update(cx, |view, _, cx| {
            view.open_environment_manager_context_menu(
                "development".to_owned(),
                dialog.center(),
                cx,
            );
        });
        visual.run_until_parked();
        let delete = visual
            .debug_bounds("environment-manager-delete")
            .expect("environment context menu should include Delete");
        visual.simulate_click(delete.center(), Modifiers::default());
        visual.run_until_parked();
    }

    workspace.update(cx, |view, _, _| {
        assert!(matches!(
            view.application_dialog.as_ref(),
            Some(ApplicationDialog::DeleteEnvironment { name, .. }) if name == "development"
        ));
    });
}

#[gpui::test]
fn create_environment_dialog_cannot_close_while_persistence_is_running(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "create-env-cancel-busy");
    workspace.update(cx, |view, window, cx| {
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
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
        assert!(view.environment_save_task.is_none());
        assert!(view.create_environment_dialog.is_none());
        assert_eq!(view.shell.selected_environment(), Some("staging"));
    });
}

#[gpui::test]
fn creating_an_environment_from_the_switcher_persists_and_selects_it(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "create-env");
    workspace.update(cx, |_, _, cx| cx.notify());
    cx.run_until_parked();

    {
        let mut visual = workspace.visual(cx);
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

    workspace.update(cx, |view, window, cx| {
        *view
            .create_environment_dialog
            .as_mut()
            .expect("create dialog should open") = "staging".to_owned();
        view.submit_create_environment_dialog(window, cx);
    });
    cx.run_until_parked();

    workspace.update(cx, |view, _, _| {
        assert!(
            view.environment_save_task.is_none(),
            "{:?}",
            toast_debug(view)
        );
        assert_eq!(view.shell.selected_environment(), Some("staging"));
    });
    assert!(saved_environment(&workspace, "staging").is_some());
}

#[gpui::test]
fn filesystem_reload_keeps_the_create_environment_dialog(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "create-env-reload");
    workspace.update(cx, |view, window, cx| {
        view.open_create_environment_dialog(window, cx);
        *view
            .create_environment_dialog
            .as_mut()
            .expect("create dialog should open") = "staging".to_owned();
        let fresh = reconciled_workspace(
            probe_opencollection::load_workspace(&workspace.path).expect("fixture should reload"),
        );
        view.apply_reconciled_workspace(fresh, cx);
        assert_eq!(view.create_environment_dialog.as_deref(), Some("staging"));
    });
}

#[gpui::test]
fn create_environment_dialog_rejects_an_empty_name(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "create-env-empty");
    workspace.update(cx, |view, window, cx| {
        view.open_create_environment_dialog(window, cx);
        view.submit_create_environment_dialog(window, cx);
        assert!(
            has_active_toast(view, ToastIntent::Error, "Environment name is required."),
            "{:?}",
            toast_debug(view)
        );
        assert!(view.create_environment_dialog.is_some());
    });
    cx.run_until_parked();

    let mut visual = workspace.visual(cx);
    visual.run_until_parked();
    visual
        .debug_bounds("toast-0")
        .expect("validation error should render as a toast");
}

#[gpui::test]
fn environment_selection_is_shared_when_opening_another_request(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, _, cx| {
        let requests = view.loaded_workspace.as_ref().unwrap().requests();
        let (first, second) = (requests[0].key(), requests[1].key());
        view.select_request(first, cx);
        view.shell
            .select_environment(Some("development".to_owned()));
        view.select_request(second, cx);
        assert_eq!(view.shell.selected_environment(), Some("development"));
    });
}

#[gpui::test]
fn environment_selection_is_restored_when_reopening_a_workspace(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.update(cx, |view, _, cx| {
        let request_key = view.loaded_workspace.as_ref().unwrap().requests()[0].key();
        view.select_request(request_key, cx);
        view.select_environment(Some("development".to_owned()), cx);
        view.close_workspace_now(cx);
    });

    let reloaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
    workspace.update(cx, |view, _, _| {
        view.set_workspace(workspace.path.clone(), reloaded);
        assert_eq!(view.shell.selected_environment(), Some("development"));
    });
}

#[gpui::test]
fn environment_selection_is_remembered_per_workspace(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let second = http_environment_fixture().canonicalize().unwrap();
    let load = |path: &PathBuf| probe_opencollection::load_workspace(path).unwrap();
    workspace.update(cx, |view, _, cx| {
        view.select_environment(Some("development".to_owned()), cx);
        view.capture_selected_environment();
        view.set_workspace(second.clone(), load(&second));
        view.select_environment(Some("local".to_owned()), cx);
        view.capture_selected_environment();
    });

    workspace.update(cx, |view, _, _| {
        view.set_workspace(workspace.path.clone(), load(&workspace.path));
        assert_eq!(view.shell.selected_environment(), Some("development"));
        view.capture_selected_environment();
        view.set_workspace(second.clone(), load(&second));
        assert_eq!(view.shell.selected_environment(), Some("local"));
    });
}

#[gpui::test]
fn missing_environment_is_not_restored(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = environment_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, _| {
            view.session_store = None;
            view.session.remember_selected_environment(
                fixture.clone(),
                Some("missing-environment".to_owned()),
            );
            view.set_workspace(fixture, workspace);
            assert_eq!(view.shell.selected_environment(), None);
        })
        .unwrap();
}
