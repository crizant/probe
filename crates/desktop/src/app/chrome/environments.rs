use super::*;

impl ProbeApp {
    pub(super) fn render_environment_manager_sidebar(
        theme: Theme,
        environments: &[Environment],
        selected_name: &str,
        active_environment: Option<&str>,
        busy: bool,
        dirty: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut selected_background: Hsla = theme.colors.actions.accent.into();
        selected_background.a = 0.12;
        let mut environment_list = div()
            .id("environment-manager-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_1));
        for (index, environment) in environments.iter().enumerate() {
            let name = environment.name.clone();
            let selected = name == selected_name;
            let active = active_environment == Some(name.as_str());
            let item_dirty = selected && dirty;
            let select_name = name.clone();
            let menu_name = name;
            let select_view = cx.weak_entity();
            let context_menu_view = cx.weak_entity();
            environment_list = environment_list.child(
                Button::new(("environment-manager-environment", index))
                    .selected(selected)
                    .w_full()
                    .h(px(theme.metrics.control_height))
                    .px(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .rounded(px(theme.metrics.radius_small))
                    .text_color(theme.colors.text.secondary)
                    .when(selected, |button| {
                        button
                            .bg(selected_background)
                            .text_color(theme.colors.actions.accent)
                    })
                    .when(!selected && !busy, |button| {
                        button.hover(move |button| {
                            button.bg(theme.colors.selection.inactive_background)
                        })
                    })
                    .disabled(busy)
                    .on_click(move |_, _, cx| {
                        let _ = select_view.update(cx, |view, cx| {
                            view.select_environment_manager_environment(&select_name, cx);
                        });
                    })
                    .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let _ = context_menu_view.update(cx, |view, cx| {
                            view.open_environment_manager_context_menu(
                                menu_name.clone(),
                                event.position,
                                cx,
                            );
                        });
                    })
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(theme.metrics.spacing_2))
                            .child(components::truncated_label(environment.name.clone()).flex_1())
                            .when(item_dirty, |row| {
                                row.child(
                                    div()
                                        .id(("environment-manager-dirty", index))
                                        .debug_selector(|| "environment-manager-dirty".into())
                                        .flex_none()
                                        .w(px(6.0))
                                        .h(px(6.0))
                                        .rounded(px(3.0))
                                        .bg(theme.colors.actions.accent),
                                )
                            })
                            .when(active, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px(px(theme.metrics.spacing_1))
                                        .rounded(px(theme.metrics.radius_small))
                                        .border_1()
                                        .border_color(theme.colors.status.success)
                                        .text_size(px(theme.typography.caption_size))
                                        .text_color(theme.colors.status.success)
                                        .child("Active"),
                                )
                            }),
                    ),
            );
        }

        let add_view = cx.weak_entity();
        let add_disabled = busy || dirty;
        let add_label = if dirty {
            "Add environment. Save or discard unsaved changes first."
        } else {
            "Add environment"
        };
        div()
            .w(px(210.0))
            .h_full()
            .pr(px(theme.metrics.spacing_1))
            .border_r_1()
            .border_color(theme.colors.borders.subtle)
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(theme.metrics.spacing_2))
                    .pl(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(theme.metrics.spacing_2))
                    .child(
                        div()
                            .text_size(px(theme.typography.caption_size))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.colors.text.muted)
                            .child("ENVIRONMENTS"),
                    )
                    .child(
                        components::icon_button(
                            theme,
                            "environment-manager-add",
                            add_label,
                            components::plus_icon(theme).text_color(if add_disabled {
                                theme.colors.actions.disabled_foreground
                            } else {
                                theme.colors.text.secondary
                            }),
                            move |_, window, cx| {
                                let _ = add_view.update(cx, |view, cx| {
                                    view.open_create_environment_dialog(window, cx);
                                });
                            },
                        )
                        .flex_none()
                        .disabled(add_disabled),
                    ),
            )
            .child(environment_list)
    }

    pub(in crate::app) fn render_environment_manager_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(dialog) = self.environment_manager_dialog.as_ref() else {
            return div().into_any_element();
        };
        let Some(loaded) = self.loaded_workspace.as_ref() else {
            return div().into_any_element();
        };
        let environments = loaded.workspace().environments();
        let rows = loaded
            .workspace()
            .effective_environment_variables(&dialog.draft);
        let rows_empty = rows.is_empty();
        let busy = self.environment_save_task.is_some();
        let dirty = self.environment_manager_is_dirty();
        let sidebar = Self::render_environment_manager_sidebar(
            theme,
            environments,
            &dialog.original_name,
            self.shell.selected_environment(),
            busy,
            dirty,
            cx,
        );

        let name_view = cx.weak_entity();
        let name_enter_view = cx.weak_entity();
        let parent_view = cx.weak_entity();
        let parent_options = std::iter::once((String::new(), "None".to_owned()))
            .chain(
                environments
                    .iter()
                    .filter(|environment| environment.name != dialog.original_name)
                    .map(|environment| (environment.name.clone(), environment.name.clone())),
            )
            .collect::<Vec<_>>();
        let selected_parent = dialog.draft.extends.clone().unwrap_or_default();
        let table_header = div()
            .h(px(30.0))
            .px(px(theme.metrics.spacing_2))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_2))
            .bg(theme.colors.surfaces.raised)
            .border_b_1()
            .border_color(theme.colors.borders.subtle)
            .text_size(px(theme.typography.caption_size))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.colors.text.muted)
            .child(div().w(px(68.0)).child("ENABLED"))
            .child(div().w(px(155.0)).child("NAME"))
            .child(div().flex_1().child("VALUE"))
            .child(div().w(px(140.0)).child("DEFINED IN"))
            .child(div().w(px(32.0)));
        let mut table_body = div()
            .id("environment-manager-variables")
            .debug_selector(|| "environment-manager-variables".into())
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if rows_empty {
            table_body = table_body.child(
                div()
                    .min_h(px(theme.metrics.control_height + theme.metrics.spacing_2))
                    .px(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .text_color(theme.colors.text.muted)
                    .child("No variables in this environment."),
            );
        }
        for (row_index, row) in rows.into_iter().enumerate() {
            let (value, editable) = match &row.variable {
                EnvironmentVariable::Plain(variable) => environment_variable_text(variable),
                EnvironmentVariable::Secret(_) => (String::new(), false),
            };
            let secret = matches!(row.variable, EnvironmentVariable::Secret(_));
            let direct_index = row.direct_index;
            let inherited = direct_index.is_none();
            let toggle_variable = row.variable.clone();
            let value_variable = row.variable.clone();
            let toggle_view = cx.weak_entity();
            let value_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            let variable_name_view = cx.weak_entity();
            let name = match &row.variable {
                EnvironmentVariable::Plain(variable) => variable.name.clone(),
                EnvironmentVariable::Secret(variable) => variable.name.clone(),
            }
            .unwrap_or_default();
            let enabled = match &row.variable {
                EnvironmentVariable::Plain(variable) => !variable.disabled,
                EnvironmentVariable::Secret(variable) => !variable.disabled,
            };
            let value_selector = if name.is_empty() {
                format!("environment-variable-value-{row_index}")
            } else {
                format!("environment-variable-value-{name}")
            };
            let mut row_element = div()
                .id(("environment-manager-variable-row", row_index))
                .min_h(px(theme.metrics.control_height + theme.metrics.spacing_2))
                .px(px(theme.metrics.spacing_2))
                .flex()
                .items_center()
                .gap(px(theme.metrics.spacing_2))
                .border_b_1()
                .border_color(theme.colors.borders.subtle)
                .when(inherited, |row| row.text_color(theme.colors.text.muted))
                .child(div().w(px(68.0)).child(components::switch(
                    theme,
                    ("environment-variable-enabled", row_index),
                    format!("Enable {name}"),
                    enabled,
                    busy,
                    move |enabled, _, cx| {
                        let mut variable = toggle_variable.clone();
                        match &mut variable {
                            EnvironmentVariable::Plain(variable) => variable.disabled = !enabled,
                            EnvironmentVariable::Secret(variable) => variable.disabled = !enabled,
                        }
                        let _ = toggle_view.update(cx, |view, cx| {
                            view.apply_environment_manager_draft(cx, |dialog| {
                                if let Some(index) = direct_index {
                                    dialog.draft.variables[index] = variable;
                                } else {
                                    dialog.draft.variables.push(variable);
                                }
                            });
                        });
                    },
                )))
                .child(if inherited {
                    div()
                        .w(px(155.0))
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_1))
                        .when(secret, |row| row.child(components::lock_icon(theme)))
                        .child(
                            components::truncated_label(name.clone())
                                .font_family(theme.typography.monospace_family)
                                .text_color(theme.colors.text.muted),
                        )
                        .into_any_element()
                } else {
                    let name_selector = if name.is_empty() {
                        format!("environment-variable-name-{row_index}")
                    } else {
                        format!("environment-variable-name-{name}")
                    };
                    div()
                        .id(("environment-variable-name", row_index))
                        .debug_selector({
                            let selector = name_selector.clone();
                            move || selector
                        })
                        .w(px(155.0))
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_1))
                        .when(secret, |row| row.child(components::lock_icon(theme)))
                        .child(
                            div().flex_1().min_w(px(0.0)).child(
                                components::dialog_text_input(
                                    theme,
                                    format!("environment-variable-name-input-{row_index}"),
                                    name.clone(),
                                    "Name",
                                    name.is_empty() && !busy,
                                    move |value, _, cx| {
                                        let _ = variable_name_view.update(cx, |view, cx| {
                                            view.apply_environment_manager_draft(cx, |dialog| {
                                                if let Some(index) = direct_index {
                                                    match dialog.draft.variables.get_mut(index) {
                                                        Some(EnvironmentVariable::Plain(
                                                            variable,
                                                        )) => {
                                                            variable.name = Some(value.to_string())
                                                        }
                                                        Some(EnvironmentVariable::Secret(
                                                            variable,
                                                        )) => {
                                                            variable.name = Some(value.to_string())
                                                        }
                                                        None => {}
                                                    }
                                                }
                                            });
                                        });
                                    },
                                    |_, _, _| {},
                                )
                                .disabled(busy),
                            ),
                        )
                        .into_any_element()
                })
                .child(if editable {
                    let input_id = format!("{value_selector}-input");
                    div()
                        .id(value_selector.clone())
                        .debug_selector({
                            let selector = value_selector.clone();
                            move || selector
                        })
                        .flex_1()
                        .min_w(px(120.0))
                        .child(
                            components::dialog_text_input(
                                theme,
                                input_id,
                                value,
                                "Value",
                                false,
                                move |value, _, cx| {
                                    let EnvironmentVariable::Plain(mut variable) =
                                        value_variable.clone()
                                    else {
                                        return;
                                    };
                                    set_environment_variable_text(&mut variable, value.to_string());
                                    let _ = value_view.update(cx, |view, cx| {
                                        view.apply_environment_manager_draft(cx, |dialog| {
                                            if let Some(index) = direct_index {
                                                dialog.draft.variables[index] =
                                                    EnvironmentVariable::Plain(variable);
                                            } else {
                                                dialog
                                                    .draft
                                                    .variables
                                                    .push(EnvironmentVariable::Plain(variable));
                                            }
                                        });
                                    });
                                },
                                |_, _, _| {},
                            )
                            .disabled(busy),
                        )
                        .into_any_element()
                } else if secret {
                    let credential_ready = !dirty && !busy && !name.is_empty();
                    let status = if dirty {
                        "Save changes to manage secret"
                    } else if busy {
                        "Saving changes…"
                    } else {
                        match dialog
                            .secret_statuses
                            .get(&name)
                            .copied()
                            .unwrap_or(SecretUiStatus::Loading)
                        {
                            SecretUiStatus::Loading => "Checking credential store…",
                            SecretUiStatus::Stored => "● Stored securely",
                            SecretUiStatus::NotStored => "○ Not set",
                            SecretUiStatus::Unavailable => "⚠ Credential store unavailable",
                        }
                    };
                    let actionable = credential_ready && !self.secret_write_in_progress;
                    let current_status = dialog.secret_statuses.get(&name).copied();
                    let set_view = cx.weak_entity();
                    let delete_view = cx.weak_entity();
                    let set_name = name.clone();
                    let delete_name = name.clone();
                    let retry = current_status == Some(SecretUiStatus::Unavailable);
                    let button_label = match current_status {
                        Some(SecretUiStatus::Stored) => "Replace",
                        Some(SecretUiStatus::Unavailable) => "Retry",
                        _ => "Set",
                    };
                    let set_selector = match current_status {
                        Some(SecretUiStatus::Stored) => {
                            format!("environment-secret-replace-{name}")
                        }
                        Some(SecretUiStatus::Unavailable) => {
                            format!("environment-secret-retry-{name}")
                        }
                        _ => format!("environment-secret-set-{name}"),
                    };
                    div()
                        .id(format!("environment-secret-status-{name}"))
                        .debug_selector({
                            let selector = format!("environment-secret-status-{name}");
                            move || selector.clone()
                        })
                        .flex_1()
                        .min_w(px(120.0))
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(
                            div()
                                .text_size(px(theme.typography.caption_size))
                                .child(status),
                        )
                        .child(components::editor_action_button(
                            theme,
                            set_selector,
                            button_label,
                            !actionable
                                || !matches!(
                                    current_status,
                                    Some(
                                        SecretUiStatus::Stored
                                            | SecretUiStatus::NotStored
                                            | SecretUiStatus::Unavailable
                                    )
                                ),
                            move |_, window, cx| {
                                let _ = set_view.update(cx, |view, cx| {
                                    if retry {
                                        view.refresh_secret_statuses(cx);
                                    } else {
                                        view.open_secret_value_dialog(set_name.clone(), window, cx);
                                    }
                                });
                            },
                        ))
                        .when(current_status == Some(SecretUiStatus::Stored), |row| {
                            row.child(components::editor_action_button(
                                theme,
                                format!("environment-secret-delete-{name}"),
                                "Delete",
                                !actionable,
                                move |_, window, cx| {
                                    let _ = delete_view.update(cx, |view, cx| {
                                        view.confirm_delete_stored_secret(
                                            delete_name.clone(),
                                            window,
                                            cx,
                                        )
                                    });
                                },
                            ))
                        })
                        .into_any_element()
                } else {
                    environment_variant_value(theme, &name, row_index, value, inherited)
                })
                .child(
                    div()
                        .w(px(140.0))
                        .flex()
                        .flex_col()
                        .child(
                            components::truncated_label(row.defined_in.clone())
                                .text_size(px(theme.typography.caption_size))
                                .text_color(theme.colors.text.muted),
                        )
                        .when(secret && inherited, |cell| {
                            cell.child(
                                components::truncated_label(format!(
                                    "Value for {}",
                                    dialog.draft.name
                                ))
                                .text_size(px(theme.typography.caption_size))
                                .text_color(theme.colors.text.muted),
                            )
                        }),
                );
            row_element = if let Some(index) = direct_index {
                row_element.child(
                    components::remove_row_button(
                        theme,
                        ("environment-variable-delete", row_index),
                        format!("Delete {name}"),
                        move |_, _, cx| {
                            let _ = remove_view.update(cx, |view, cx| {
                                view.apply_environment_manager_draft(cx, |dialog| {
                                    if index < dialog.draft.variables.len() {
                                        dialog.draft.variables.remove(index);
                                    }
                                });
                            });
                        },
                    )
                    .disabled(busy),
                )
            } else {
                row_element.child(div().w(px(32.0)))
            };
            table_body = table_body.child(row_element);
        }

        let add_variable_view = cx.weak_entity();
        let add_secret_view = cx.weak_entity();
        table_body = table_body.child(
            div()
                .p(px(theme.metrics.spacing_2))
                .flex()
                .gap(px(theme.metrics.spacing_2))
                .child(
                    components::editor_add_button(
                        theme,
                        "environment-manager-add-variable",
                        "Add variable",
                        move |_, _, cx| {
                            let _ = add_variable_view.update(cx, |view, cx| {
                                view.apply_environment_manager_draft(cx, |dialog| {
                                    dialog.draft.variables.push(EnvironmentVariable::Plain(
                                        Variable {
                                            name: Some(String::new()),
                                            value: Some(VariableValueSet::Single(
                                                VariableValue::String(String::new()),
                                            )),
                                            disabled: false,
                                        },
                                    ));
                                });
                            });
                        },
                    )
                    .disabled(busy),
                )
                .child(
                    components::editor_add_button(
                        theme,
                        "environment-manager-add-secret",
                        "Add secret variable",
                        move |_, _, cx| {
                            let _ = add_secret_view.update(cx, |view, cx| {
                                view.apply_environment_manager_draft(cx, |dialog| {
                                    dialog.draft.variables.push(EnvironmentVariable::Secret(
                                        SecretVariable {
                                            name: Some(String::new()),
                                            value_type: None,
                                            disabled: false,
                                        },
                                    ));
                                });
                            });
                        },
                    )
                    .disabled(busy),
                ),
        );
        let table = div()
            .mt(px(theme.metrics.spacing_2))
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(theme.metrics.radius_small))
            .border_1()
            .border_color(theme.colors.borders.standard)
            .child(table_header)
            .child(table_body);
        let form = div()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .pl(px(theme.metrics.spacing_2))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_3))
                    .child(
                        div().flex_1().min_w(px(0.0)).child(
                            components::dialog_text_input(
                                theme,
                                "environment-manager-name",
                                dialog.draft.name.clone(),
                                "Environment name",
                                false,
                                move |value, _, cx| {
                                    let _ = name_view.update(cx, |view, cx| {
                                        view.apply_environment_manager_draft(cx, |dialog| {
                                            dialog.draft.name = value.to_string();
                                        });
                                    });
                                },
                                move |value, _, cx| {
                                    let _ = name_enter_view.update(cx, |view, cx| {
                                        view.apply_environment_manager_draft(cx, |dialog| {
                                            dialog.draft.name = value.to_string();
                                        });
                                    });
                                },
                            )
                            .disabled(busy),
                        ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(theme.metrics.spacing_2))
                            .child(components::dialog_field_label(theme, "Extends"))
                            .child(
                                components::dropdown(
                                    theme,
                                    "environment-manager-parent",
                                    "Parent environment",
                                    Some(selected_parent),
                                    parent_options,
                                    180.0,
                                    move |value, _, cx| {
                                        let value = value.cloned().unwrap_or_default();
                                        let _ = parent_view.update(cx, |view, cx| {
                                            view.apply_environment_manager_draft(cx, |dialog| {
                                                dialog.draft.extends =
                                                    (!value.is_empty()).then_some(value);
                                            });
                                        });
                                    },
                                )
                                .disabled(busy),
                            ),
                    ),
            )
            .child(table);

        let close_view = cx.weak_entity();
        let save_view = cx.weak_entity();
        let save_disabled = self.environment_manager_save_disabled();
        let mut content = components::dialog_surface(theme, "environment-manager-dialog", 900.0)
            .debug_selector(|| "environment-manager-dialog".into())
            .h(px(600.0))
            .max_h(relative(0.9))
            .child(components::dialog_title(
                theme,
                format!("Environments — {}", self.workspace_name()),
            ));
        content = content
            .child(
                div()
                    .mt(px(theme.metrics.spacing_4))
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .child(sidebar)
                    .child(form),
            )
            .child(
                components::dialog_actions(theme)
                    .child(components::dialog_action_button(
                        theme,
                        "environment-manager-close",
                        "Close",
                        components::DialogActionStyle::Secondary,
                        None,
                        false,
                        move |_, window, cx| {
                            let _ = close_view.update(cx, |view, cx| {
                                view.request_close_environment_manager_dialog(window, cx);
                            });
                        },
                    ))
                    .child(components::dialog_action_button(
                        theme,
                        "environment-manager-save",
                        "Save Changes",
                        components::DialogActionStyle::Primary,
                        components::shortcut_label_for_action_in_context(
                            window,
                            &SubmitEnvironmentManagerDialog,
                            "EnvironmentManagerDialog",
                        ),
                        save_disabled,
                        move |_, window, cx| {
                            let _ = save_view.update(cx, |view, cx| {
                                view.save_environment_manager_dialog(window, cx);
                            });
                        },
                    )),
            );

        components::dialog_layer(
            theme,
            &self.environment_manager_dialog_focus,
            "EnvironmentManagerDialog",
            content,
        )
        .into_any_element()
    }

    pub(in crate::app) fn render_environment_manager_context_menu(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(context_menu) = self.transient.environment_manager_context_menu.as_ref() else {
            return div().into_any_element();
        };
        let name = &context_menu.target;
        let position = context_menu.position;
        let busy = self.environment_save_task.is_some();
        let delete_name = name.clone();
        let delete_view = cx.weak_entity();
        let dismiss_view = cx.weak_entity();
        let menu = components::context_menu_surface(
            theme,
            "environment-manager-context-menu",
            180.0,
            move |cx| {
                let _ = dismiss_view.update(cx, |view, cx| {
                    view.close_environment_manager_context_menu(cx);
                });
            },
        )
        .child(components::destructive_menu_button(
            theme,
            "environment-manager-delete",
            "Delete",
            components::shortcut_label_for_action_in_context(
                window,
                &DeleteSelectedEnvironment,
                "EnvironmentManagerDialog",
            ),
            move |window, cx| {
                let _ = delete_view.update(cx, |view, cx| {
                    if busy {
                        return;
                    }
                    view.close_environment_manager_context_menu(cx);
                    view.confirm_delete_environment(delete_name.clone(), window, cx);
                });
            },
        ));
        deferred(
            Positioner::corner(Anchor::TopLeft, position)
                .margin(px(8.0))
                .child(menu),
        )
        .with_priority(POPUP_PRIORITY)
        .into_any_element()
    }

    pub(in crate::app) fn render_secret_value_dialog(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(dialog) = &self.secret_value_dialog else {
            return div().into_any_element();
        };
        let cancel_view = cx.weak_entity();
        let save_view = cx.weak_entity();
        let content = components::dialog_surface(theme, "secret-value-dialog", components::COMPACT_DIALOG_WIDTH)
            .debug_selector(|| "secret-value-dialog".into())
            .child(components::dialog_title(theme, "Set secret"))
            .child(components::dialog_description(theme, format!("{} in {}", dialog.name, dialog.environment))
                .mt(px(theme.metrics.spacing_2)))
            .child(div().mt(px(theme.metrics.spacing_3)).child(components::dialog_field_label(theme, "Secret value")))
            .child(div()
                .id("secret-value-input")
                .debug_selector(|| "secret-value-input".into())
                .mt(px(theme.metrics.spacing_1))
                .h(px(theme.metrics.control_height))
                .px(px(theme.metrics.spacing_2))
                .flex()
                .items_center()
                .rounded(px(theme.metrics.radius_small))
                .border_1()
                .border_color(theme.colors.borders.standard)
                .bg(theme.colors.surfaces.raised)
                .child(Input::new(&dialog.input)))
            .child(components::dialog_description(theme, "Stored in your system credential store. The value is never saved to this collection or displayed by Probe.")
                .mt(px(theme.metrics.spacing_2)))
            .when_some(dialog.error, |content, error| content.child(
                components::dialog_description(theme, error).mt(px(theme.metrics.spacing_2)).text_color(theme.colors.status.error)
            ))
            .child(components::dialog_actions(theme)
                .child(components::dialog_action_button(theme, "secret-value-cancel", "Cancel", components::DialogActionStyle::Secondary, None, false, move |_, window, cx| {
                    let _ = cancel_view.update(cx, |view, cx| view.close_secret_value_dialog(window, cx));
                }))
                .child(components::dialog_action_button(theme, "secret-value-save", if dialog.busy { "Saving…" } else { "Save Secret" }, components::DialogActionStyle::Primary, None, dialog.busy || dialog.input.read(cx).value().is_empty(), move |_, window, cx| {
                    let _ = save_view.update(cx, |view, cx| view.save_secret_value(window, cx));
                })));
        components::dialog_layer(
            theme,
            &self.environment_manager_dialog_focus,
            "EnvironmentManagerDialog",
            content,
        )
        .into_any_element()
    }
}
