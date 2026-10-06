use super::*;
use crate::app::dialogs::{EnvironmentFieldKind, EnvironmentVariableRowId};

const ENABLED_COLUMN_WIDTH: f32 = 44.0;

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
                    .flex_none()
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

    fn environment_field_focus(
        dialog: &EnvironmentManagerDialog,
        row_id: EnvironmentVariableRowId,
        kind: EnvironmentFieldKind,
        cx: &Context<Self>,
    ) -> impl Fn(gpui::Entity<components::FieldInput>, bool, &mut App) + 'static {
        let view = cx.weak_entity();
        let environment = dialog.original_name.clone();
        move |field, focused, cx| {
            let _ = view.update(cx, |view, cx| {
                let Some(dialog) = view.environment_manager_dialog.as_mut() else {
                    return;
                };
                if dialog.original_name != environment {
                    return;
                }
                if let EnvironmentVariableRowId::Direct(id) = &row_id
                    && !dialog.variable_row_ids.contains(id)
                {
                    return;
                }
                if focused {
                    dialog.active_field = Some((row_id.clone(), kind, field));
                } else if dialog
                    .active_field
                    .as_ref()
                    .is_some_and(|(_, _, active)| active == &field)
                {
                    dialog.active_field = None;
                }
                cx.notify();
            });
        }
    }

    fn render_environment_variable_row(
        &self,
        theme: Theme,
        dialog: &EnvironmentManagerDialog,
        row: probe_core::EffectiveEnvironmentVariable,
        busy: bool,
        dirty: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let row_index = row.direct_index.unwrap_or(0);
        let stable_row_id = match row.direct_index {
            Some(index) => EnvironmentVariableRowId::Direct(dialog.variable_row_ids[index]),
            None => EnvironmentVariableRowId::Inherited {
                defined_in: row.defined_in.clone(),
                name: row.variable.name().unwrap_or_default().to_owned(),
            },
        };
        let name_row_id = stable_row_id.clone();
        let value_row_id = stable_row_id.clone();
        let row_id = match row.direct_index {
            Some(direct_index) => format!(
                "direct-{}-{}",
                dialog.original_name,
                dialog
                    .variable_row_ids
                    .get(direct_index)
                    .copied()
                    .unwrap_or(direct_index as u64)
            ),
            None => format!(
                "inherited-{}-{}-{}",
                dialog.original_name,
                row.defined_in,
                row.variable.name().unwrap_or("")
            ),
        };
        let (value, editable) = match &row.variable {
            EnvironmentVariable::Plain(variable) => environment_variable_text(variable),
            EnvironmentVariable::Secret(_) => (String::new(), false),
        };
        let secret = row.variable.is_secret();
        let direct_index = row.direct_index;
        let inherited = direct_index.is_none();
        let toggle_variable = row.variable.clone();
        let value_variable = row.variable.clone();
        let toggle_view = cx.weak_entity();
        let value_view = cx.weak_entity();
        let remove_view = cx.weak_entity();
        let variable_name_view = cx.weak_entity();
        let name = row.variable.name().unwrap_or_default().to_owned();
        let enabled = !row.variable.is_disabled();
        let value_selector = if name.is_empty() {
            format!("environment-variable-value-{row_index}")
        } else {
            format!("environment-variable-value-{name}")
        };
        let mut row_element = div()
            .id(format!("environment-manager-variable-row-{row_id}"))
            .w_full()
            .h(px(theme.metrics.control_height + theme.metrics.spacing_2))
            .flex_none()
            .overflow_hidden()
            .px(px(theme.metrics.spacing_2))
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_2))
            .border_b_1()
            .border_color(theme.colors.borders.subtle)
            .when(inherited, |row| row.text_color(theme.colors.text.muted))
            .child(div().w(px(ENABLED_COLUMN_WIDTH)).child(components::switch(
                theme,
                format!("environment-variable-enabled-{row_id}"),
                format!("Enable {name}"),
                enabled,
                busy,
                move |enabled, _, cx| {
                    let mut variable = toggle_variable.clone();
                    variable.set_disabled(!enabled);
                    let _ = toggle_view.update(cx, |view, cx| {
                        view.apply_environment_manager_draft(cx, |dialog| {
                            if let Some(index) = direct_index {
                                dialog.draft_mut().variables[index] = variable;
                            } else {
                                dialog.add_variable(variable);
                            }
                        });
                    });
                },
            )))
            .child(
                div()
                    .id(format!("environment-variable-type-{row_id}"))
                    .debug_selector({
                        let selector = format!("environment-variable-type-{name}");
                        move || selector.clone()
                    })
                    .w(px(theme.metrics.icon_standard))
                    .flex_none()
                    .when(secret, |slot| slot.child(components::lock_icon(theme))),
            )
            .child(if inherited {
                div()
                    .w(px(155.0))
                    .flex_none()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .child(
                        components::truncated_label(name.clone())
                            .min_w(px(0.0))
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
                let list_scroll = self.environment_list_scroll.clone();
                div()
                    .id(format!("environment-variable-name-{row_id}"))
                    .debug_selector({
                        let selector = name_selector.clone();
                        move || selector
                    })
                    .w(px(155.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(
                        div().flex_1().min_w(px(0.0)).child(
                            components::dialog_text_input(
                                theme,
                                format!("environment-variable-name-input-{row_id}"),
                                name.clone(),
                                "Name",
                                name.is_empty() && !busy && dialog.active_field.is_none(),
                                move |value, _, cx| {
                                    let _ = variable_name_view.update(cx, |view, cx| {
                                        view.apply_environment_manager_draft(cx, |dialog| {
                                            if let EnvironmentVariableRowId::Direct(id) = name_row_id
                                                && let Some(index) = dialog.variable_row_ids.iter().position(|row| *row == id)
                                                && let Some(variable) = dialog.draft_mut().variables.get_mut(index)
                                            {
                                                *variable.name_mut() = Some(value.to_string());
                                            }
                                        });
                                    });
                                },
                                |_, _, _| {},
                            )
                            .list_scroll(Some(&list_scroll))
                            .persistent_field(
                                dialog
                                    .active_field
                                    .as_ref()
                                    .filter(|(id, kind, _)| {
                                        *id == stable_row_id
                                            && *kind == EnvironmentFieldKind::Name
                                    })
                                    .map(|(_, _, field)| field.clone()),
                                Self::environment_field_focus(
                                    dialog,
                                    stable_row_id.clone(),
                                    EnvironmentFieldKind::Name,
                                    cx,
                                ),
                            )
                            .disabled(busy),
                        ),
                    )
                    .into_any_element()
            })
            .child(if editable {
                let input_id = format!("environment-variable-value-input-{row_id}");
                let list_scroll = self.environment_list_scroll.clone();
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
                                let _ = value_view.update(cx, |view, cx| {
                                    let mut promoted_index = None;
                                    view.apply_environment_manager_draft(cx, |dialog| {
                                        let index = match &value_row_id {
                                            EnvironmentVariableRowId::Direct(id) => {
                                                dialog.variable_row_ids.iter().position(|row| row == id)
                                            },
                                            EnvironmentVariableRowId::Inherited { name, .. } => {
                                                dialog.draft().variables.iter().position(|variable| {
                                                    matches!(variable, EnvironmentVariable::Plain(variable) if variable.name.as_ref() == Some(name))
                                                })
                                            },
                                        };
                                        if let Some(index) = index {
                                            if let EnvironmentVariable::Plain(variable) = &mut dialog.draft_mut().variables[index] {
                                                set_environment_variable_text(variable, value.to_string());
                                            }
                                        } else if matches!(value_row_id, EnvironmentVariableRowId::Inherited { .. })
                                            && let EnvironmentVariable::Plain(mut variable) = value_variable.clone()
                                        {
                                            set_environment_variable_text(&mut variable, value.to_string());
                                            let direct_id = dialog.next_variable_row_id;
                                            let index = dialog.draft().variables.len();
                                            dialog.add_variable(EnvironmentVariable::Plain(variable));
                                            // The inherited row disappears when its direct override is added.
                                            // Move ownership to the new identity before the next render.
                                            if let Some((id, _, _)) = &mut dialog.active_field
                                                && *id == value_row_id
                                            {
                                                *id = EnvironmentVariableRowId::Direct(direct_id);
                                                promoted_index = Some(index);
                                            }
                                        }
                                    });
                                    if let Some(index) = promoted_index {
                                        view.environment_variables_scroll.scroll_to_item_strict(index, ScrollStrategy::Top);
                                    }
                                });
                            },
                            |_, _, _| {},
                        )
                        .list_scroll(Some(&list_scroll))
                        .persistent_field(
                            dialog
                                .active_field
                                .as_ref()
                                .filter(|(id, kind, _)| {
                                    *id == stable_row_id
                                        && *kind == EnvironmentFieldKind::Value
                                })
                                .map(|(_, _, field)| field.clone()),
                            Self::environment_field_focus(
                                dialog,
                                stable_row_id.clone(),
                                EnvironmentFieldKind::Value,
                                cx,
                            ),
                        )
                        .disabled(busy),
                    )
                    .into_any_element()
            } else if secret {
                self.render_secret_variable_value_cell(theme, dialog, name.clone(), busy, dirty, cx)
            } else {
                environment_variant_value(theme, &name, row_index, value, inherited)
            })
            .child(
                div()
                    .id(format!("environment-variable-defined-in-{row_id}"))
                    .debug_selector({
                        let selector = format!("environment-variable-defined-in-{name}");
                        move || selector.clone()
                    })
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
                            components::truncated_label(format!("Value for {}", dialog.draft().name))
                                .text_size(px(theme.typography.caption_size))
                                .text_color(theme.colors.text.muted),
                        )
                    }),
            );
        row_element = if let Some(index) = direct_index {
            row_element.child(
                components::remove_row_button(
                    theme,
                    format!("environment-variable-delete-{row_id}"),
                    format!("Remove {name} from this environment"),
                    move |_, _, cx| {
                        let _ = remove_view.update(cx, |view, cx| {
                            view.apply_environment_manager_draft(cx, |dialog| {
                                dialog.remove_variable(index);
                            });
                        });
                    },
                )
                .disabled(busy),
            )
        } else {
            row_element.child(div().w(px(32.0)))
        };
        row_element.into_any_element()
    }

    fn render_secret_variable_value_cell(
        &self,
        theme: Theme,
        dialog: &EnvironmentManagerDialog,
        name: String,
        busy: bool,
        dirty: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let credential_ready = !dirty && !busy && !name.is_empty();
        let current_status = dialog.secret_statuses.get(&name).copied();
        let status = if dirty {
            "Save changes to manage secret"
        } else if busy {
            "Saving changes…"
        } else {
            match current_status.unwrap_or(SecretUiStatus::Unknown) {
                SecretUiStatus::Loading => "Removing stored secret…",
                SecretUiStatus::Stored => "● Stored securely",
                SecretUiStatus::NotStored => "○ Not set",
                SecretUiStatus::Unknown => "Not verified",
            }
        };
        let actionable = credential_ready && !self.secret_write_in_progress;
        let set_view = cx.weak_entity();
        let set_name = name.clone();
        let button_label = match current_status {
            Some(SecretUiStatus::Stored) => "Replace",
            _ => "Set",
        };
        let set_selector = match current_status {
            Some(SecretUiStatus::Stored) => {
                format!("environment-secret-replace-{name}")
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
            .min_w(px(0.0))
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_2))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    // Match the value field, whose text sits inside a 1px border and spacing_2 padding.
                    .px(px(theme.metrics.spacing_2 + 1.0))
                    .text_size(px(theme.typography.caption_size))
                    .text_color(match current_status {
                        Some(SecretUiStatus::Stored) if credential_ready => {
                            theme.colors.status.success
                        }
                        _ => theme.colors.text.muted,
                    })
                    .child(status),
            )
            .when(
                credential_ready
                    && matches!(
                        current_status,
                        Some(
                            SecretUiStatus::Stored
                                | SecretUiStatus::NotStored
                                | SecretUiStatus::Unknown
                        )
                    ),
                |row| {
                    row.child(
                        components::editor_action_button(
                            theme,
                            set_selector,
                            button_label,
                            !actionable,
                            move |_, window, cx| {
                                let _ = set_view.update(cx, |view, cx| {
                                    view.open_secret_value_dialog(set_name.clone(), window, cx);
                                });
                            },
                        )
                        .flex_none(),
                    )
                },
            )
            .into_any_element()
    }

    fn render_environment_variable_actions(
        &self,
        theme: Theme,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let add_variable_view = cx.weak_entity();
        let add_secret_view = cx.weak_entity();
        div()
            .w_full()
            .flex_none()
            .px(px(theme.metrics.spacing_2))
            .py(px(theme.metrics.spacing_1))
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_2))
            .border_t_1()
            .border_color(theme.colors.borders.subtle)
            .child(
                components::editor_add_button(
                    theme,
                    "environment-manager-add-variable",
                    "Add variable",
                    move |_, _, cx| {
                        let _ = add_variable_view.update(cx, |view, cx| {
                            view.add_environment_manager_variable(
                                cx,
                                EnvironmentVariable::Plain(Variable {
                                    name: Some(String::new()),
                                    value: Some(VariableValueSet::Single(VariableValue::String(
                                        String::new(),
                                    ))),
                                    disabled: false,
                                }),
                            );
                        });
                    },
                )
                .flex_none()
                .disabled(busy),
            )
            .child(
                components::editor_add_button(
                    theme,
                    "environment-manager-add-secret",
                    "Add secret",
                    move |_, _, cx| {
                        let _ = add_secret_view.update(cx, |view, cx| {
                            view.add_environment_manager_variable(
                                cx,
                                EnvironmentVariable::Secret(SecretVariable {
                                    name: Some(String::new()),
                                    value_type: None,
                                    disabled: false,
                                }),
                            );
                        });
                    },
                )
                .flex_none()
                .disabled(busy),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_environment_manager_dialog(
        &mut self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // Focus-out is based on rendered ancestry and cannot observe an already offscreen editor.
        if let Some(dialog) = self.environment_manager_dialog.as_mut() {
            dialog.sync_variable_row_ids();
            if dialog
                .active_field
                .as_ref()
                .is_some_and(|(_, _, field)| !field.read(cx).is_focused(window, cx))
            {
                dialog.active_field = None;
            }
        }
        let Some(loaded) = self.loaded_workspace.as_ref() else {
            return div().into_any_element();
        };
        let environments = loaded.workspace().environments();
        let Some(dialog) = self.environment_manager_dialog.as_ref() else {
            return div().into_any_element();
        };
        let rows = dialog.effective_rows(environments);
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
        let selected_parent = dialog.draft().extends.clone().unwrap_or_default();
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
            .child(div().w(px(ENABLED_COLUMN_WIDTH)).child("ON"))
            .child(div().w(px(theme.metrics.icon_standard)))
            .child(div().w(px(155.0)).child("NAME"))
            .child(div().flex_1().child("VALUE"))
            .child(div().w(px(140.0)).child("DEFINED IN"))
            .child(div().w(px(32.0)));
        let table_body = div()
            .id("environment-manager-variables")
            .debug_selector(|| "environment-manager-variables".into())
            .flex_1()
            .min_h(px(0.0))
            .relative();
        let row_count = rows.len() + usize::from(rows_empty);
        let list = uniform_list("environment-manager-variable-list", row_count, {
            let rows = rows.clone();
            cx.processor(move |view, range: std::ops::Range<usize>, _, cx| {
                #[cfg(test)]
                {
                    view.rendered_environment_variable_rows =
                        range.clone().filter(|index| *index < rows.len()).count();
                }
                let Some(dialog) = view.environment_manager_dialog.as_ref() else {
                    return Vec::new();
                };
                range
                    .filter_map(|index| {
                        if rows_empty && index == 0 {
                            return Some(
                                div()
                                    .w_full()
                                    .h(px(theme.metrics.control_height + theme.metrics.spacing_2))
                                    .px(px(theme.metrics.spacing_2))
                                    .flex()
                                    .items_center()
                                    .border_b_1()
                                    .border_color(theme.colors.borders.subtle)
                                    .text_color(theme.colors.text.muted)
                                    .child("No variables in this environment.")
                                    .into_any_element(),
                            );
                        }
                        let row = rows.get(index)?.clone();
                        Some(
                            view.render_environment_variable_row(
                                theme, dialog, row, busy, dirty, cx,
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            })
        })
        .size_full()
        .track_scroll(&self.environment_variables_scroll);
        let list = components::list_scroll_region(
            list,
            &self.environment_variables_scroll.0.borrow().base_handle,
            &self.environment_list_scroll,
        )
        .fill_height();
        let table_body = table_body.child(list).child(
            Scrollbar::vertical(&self.environment_variables_scroll)
                .id("environment-manager-variables-scrollbar")
                .mode(ScrollbarMode::Scrolling),
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
            .child(table_body)
            .child(self.render_environment_variable_actions(theme, busy, cx));
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
                                dialog.draft().name.clone(),
                                "Environment name",
                                false,
                                move |value, _, cx| {
                                    let _ = name_view.update(cx, |view, cx| {
                                        view.apply_environment_manager_draft(cx, |dialog| {
                                            dialog.draft_mut().name = value.to_string();
                                        });
                                    });
                                },
                                move |value, _, cx| {
                                    let _ = name_enter_view.update(cx, |view, cx| {
                                        view.apply_environment_manager_draft(cx, |dialog| {
                                            dialog.draft_mut().name = value.to_string();
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
                                                dialog.draft_mut().extends =
                                                    (!value.is_empty()).then_some(value);
                                            });
                                        });
                                    },
                                )
                                .disabled(busy),
                            ),
                    ),
            )
            .child({
                let description_view = cx.weak_entity();
                let description_id = gpui::ElementId::Name(
                    format!("environment-description-{}", dialog.original_name).into(),
                );
                super::super::render::documentation::documentation_text_field(
                    theme,
                    "Description",
                    crate::app::documentation::documentation_text(
                        dialog.draft().description.as_ref(),
                    ),
                    description_id,
                    "environment-manager-description",
                    false,
                    move |value, _, cx| {
                        let _ = description_view.update(cx, |view, cx| {
                            view.apply_environment_manager_draft(cx, |dialog| {
                                crate::app::documentation::edit_documentation(
                                    &mut dialog.draft_mut().description,
                                    value.to_string(),
                                );
                            });
                        });
                    },
                )
                .mt(px(theme.metrics.spacing_2))
            })
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(dialog) = &self.secret_value_dialog else {
            return div().into_any_element();
        };
        let cancel_view = cx.weak_entity();
        let save_view = cx.weak_entity();
        let delete_view = cx.weak_entity();
        let delete_name = dialog.target.name.clone();
        let input = dialog.input.clone();
        let stored = self.secret_is_stored(&dialog.target, dialog.from_manager);
        let content = components::dialog_surface(theme, "secret-value-dialog", components::COMPACT_DIALOG_WIDTH)
            .debug_selector(|| "secret-value-dialog".into())
            .child(components::dialog_title(theme, if dialog.replacing { "Replace secret" } else { "Set secret" }))
            .child(div()
                .id("secret-value-identity")
                .debug_selector(|| "secret-value-identity".into())
                .mt(px(theme.metrics.spacing_2))
                .w_full()
                .min_w(px(0.0))
                .px(px(theme.metrics.spacing_2))
                .py(px(theme.metrics.spacing_2))
                .flex()
                .items_center()
                .gap(px(theme.metrics.spacing_2))
                .rounded(px(theme.metrics.radius_small))
                .border_1()
                .border_color(theme.colors.borders.subtle)
                .bg(theme.colors.surfaces.raised)
                .child(components::lock_icon(theme).flex_none())
                .child(components::truncated_label(dialog.target.name.clone())
                    .font_family(theme.typography.monospace_family)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.colors.text.primary))
                .child(div()
                    .flex_none()
                    .text_color(theme.colors.text.muted)
                    .child("in"))
                .child(components::truncated_label(dialog.target.environment.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.colors.actions.accent)))
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
                .cursor_text()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    input.update(cx, |input, cx| input.focus(window, cx));
                })
                .child(Input::new(&dialog.input)))
            .child(components::dialog_description(theme, "Stored in your system credential store. The value is never saved to this collection or displayed by Probe.")
                .mt(px(theme.metrics.spacing_2)))
            .when_some(dialog.error, |content, error| content.child(
                components::dialog_description(theme, error).mt(px(theme.metrics.spacing_2)).text_color(theme.colors.status.error)
            ))
            .child(components::dialog_actions(theme)
                .when(dialog.replacing && stored, |actions| actions.child(
                    components::dialog_action_button(theme, "secret-value-delete-stored", "Delete Stored Value", components::DialogActionStyle::Destructive, None, dialog.busy || self.secret_write_in_progress, move |_, window, cx| {
                        let _ = delete_view.update(cx, |view, cx| view.confirm_delete_stored_secret(delete_name.clone(), window, cx));
                    })
                ))
                .child(components::dialog_action_button(theme, "secret-value-cancel", "Cancel", components::DialogActionStyle::Secondary, None, false, move |_, window, cx| {
                    let _ = cancel_view.update(cx, |view, cx| view.close_secret_value_dialog(window, cx));
                }))
                .child(components::dialog_action_button(theme, "secret-value-save", if dialog.busy { "Saving…" } else { "Save Secret" }, components::DialogActionStyle::Primary, components::shortcut_label_for_action_in_context(window, &SubmitSecretValueDialog, "SecretValueDialog"), dialog.busy || dialog.input.read(cx).value().is_empty(), move |_, window, cx| {
                    let _ = save_view.update(cx, |view, cx| view.save_secret_value(window, cx));
                })));
        components::dialog_layer(theme, &dialog.restore_focus, "SecretValueDialog", content)
            .into_any_element()
    }
}
