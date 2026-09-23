use super::*;

impl ProbeApp {
    pub(super) fn render_application_dialog_actions(
        theme: Theme,
        specs: &[DialogActionSpec],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut actions = components::dialog_actions(theme);
        for spec in specs.iter().copied() {
            let view = cx.weak_entity();
            let shortcut_hint = match spec.style {
                components::DialogActionStyle::Primary => {
                    components::shortcut_label_for_action_in_context(
                        window,
                        &SubmitApplicationDialog,
                        "ApplicationDialog",
                    )
                }
                components::DialogActionStyle::Secondary => None,
                components::DialogActionStyle::Destructive => {
                    components::shortcut_label_for_action_in_context(
                        window,
                        &SubmitApplicationDialogDestructive,
                        "ApplicationDialog",
                    )
                }
            };
            actions = actions.child(components::dialog_action_button(
                theme,
                spec.id,
                spec.label,
                spec.style,
                shortcut_hint,
                false,
                move |_, window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.handle_application_dialog_action(spec.action, window, cx);
                    });
                },
            ));
        }
        actions
    }

    pub(in crate::app) fn render_application_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(dialog) = self.application_dialog.as_ref() else {
            return div().into_any_element();
        };
        let mut content = components::dialog_surface(theme, "application-dialog", dialog.width())
            .child(components::dialog_title(theme, dialog.title()))
            .child(
                components::dialog_description(theme, dialog.description())
                    .id("application-dialog-description")
                    .mt(px(theme.metrics.spacing_2))
                    .max_h(px(280.0))
                    .overflow_y_scroll()
                    .line_height(relative(theme.typography.body_line_height)),
            );

        if let Some(specs) = dialog.action_specs() {
            content = content.child(Self::render_application_dialog_actions(
                theme, specs, window, cx,
            ));
        } else if let ApplicationDialog::SelectYaakWorkspace { workspaces, .. } = dialog {
            let mut choices = div()
                .id("application-dialog-workspaces")
                .mt(px(theme.metrics.spacing_3))
                .max_h(px(320.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_2));
            for (index, workspace) in workspaces.iter().enumerate() {
                let choice_view = cx.weak_entity();
                choices = choices.child(components::dialog_choice_button(
                    theme,
                    format!("application-dialog-workspace-{index}"),
                    format!("{} — {}", workspace.name, workspace.id),
                    move |_, window, cx| {
                        let _ = choice_view.update(cx, |view, cx| {
                            view.handle_application_dialog_action(
                                ApplicationDialogAction::SelectWorkspace(index),
                                window,
                                cx,
                            );
                        });
                    },
                ));
            }
            content = content
                .child(choices)
                .child(Self::render_application_dialog_actions(
                    theme,
                    &[CANCEL_DIALOG_ACTION],
                    window,
                    cx,
                ));
        } else if let ApplicationDialog::SelectCollectionFile { candidates } = dialog {
            let mut choices = div()
                .id("application-dialog-collections")
                .mt(px(theme.metrics.spacing_3))
                .max_h(px(320.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_2));
            for (index, candidate) in candidates.iter().enumerate() {
                let choice_view = cx.weak_entity();
                choices = choices.child(components::dialog_choice_button(
                    theme,
                    format!("application-dialog-collection-{index}"),
                    candidate.display().to_string(),
                    move |_, window, cx| {
                        let _ = choice_view.update(cx, |view, cx| {
                            view.handle_application_dialog_action(
                                ApplicationDialogAction::SelectCollectionFile(index),
                                window,
                                cx,
                            );
                        });
                    },
                ));
            }
            content = content
                .child(choices)
                .child(Self::render_application_dialog_actions(
                    theme,
                    &[CANCEL_DIALOG_ACTION],
                    window,
                    cx,
                ));
        }

        components::dialog_layer(
            theme,
            &self.application_dialog_focus,
            "ApplicationDialog",
            content,
        )
        .into_any_element()
    }

    pub(in crate::app) fn render_structure_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(dialog) = self.structure_dialog.as_ref() else {
            return div().into_any_element();
        };
        let name_view = cx.weak_entity();
        let name_enter_view = cx.weak_entity();
        let cancel_view = cx.weak_entity();
        let submit_view = cx.weak_entity();
        let save_destination =
            matches!(dialog.mode, StructureDialogMode::SaveDetachedRequest { .. });
        let save_busy = save_destination
            && (self.structure_task.is_some()
                || self.request_save_task.is_some()
                || self.environment_save_task.is_some());
        let mut form = div()
            .mt(px(theme.metrics.spacing_4))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_3));
        if dialog.edits_name() {
            form = form.child(components::dialog_field(
                theme,
                "Name",
                components::dialog_text_input(
                    theme,
                    "structure-name",
                    dialog.name.clone(),
                    "",
                    true,
                    move |value, _, cx| {
                        let _ = name_view.update(cx, |view, cx| {
                            if let Some(dialog) = view.structure_dialog.as_mut() {
                                dialog.name = value.to_string();
                            }
                            cx.notify();
                        });
                    },
                    move |value, window, cx| {
                        let _ = name_enter_view.update(cx, |view, cx| {
                            if let Some(dialog) = view.structure_dialog.as_mut() {
                                dialog.name = value.to_string();
                            }
                            view.submit_structure_dialog(window, cx);
                        });
                    },
                ),
            ));
        }
        if save_destination {
            form = form.child(self.render_save_destination(theme, cx));
        } else if dialog.edits_destination() {
            let parent_view = cx.weak_entity();
            let index_view = cx.weak_entity();
            let index_enter_view = cx.weak_entity();
            let mut options = vec![(ROOT_PARENT.to_owned(), "Collection root".to_owned())];
            if let Some(loaded) = &self.loaded_workspace {
                options.extend(loaded.folders().iter().filter_map(|located| {
                    let name = loaded
                        .workspace()
                        .folder(located.key())?
                        .metadata
                        .name
                        .as_deref()
                        .unwrap_or("Untitled folder");
                    Some((
                        located.selector().to_owned(),
                        format!("{name} — {}", located.selector()),
                    ))
                }));
            }
            form = form
                .child(components::dialog_field(
                    theme,
                    "Destination",
                    components::dropdown(
                        theme,
                        "structure-parent",
                        "Destination folder",
                        Some(dialog.parent.clone()),
                        options,
                        388.0,
                        move |value, _, cx| {
                            let Some(value) = value else {
                                return;
                            };
                            let value = value.clone();
                            let _ = parent_view.update(cx, |view, cx| {
                                if let Some(dialog) = view.structure_dialog.as_mut() {
                                    dialog.parent = value;
                                    dialog.index.clear();
                                }
                                cx.notify();
                            });
                        },
                    ),
                ))
                .child(components::dialog_field(
                    theme,
                    "Position",
                    components::dialog_text_input(
                        theme,
                        "structure-index",
                        dialog.index.clone(),
                        "Append",
                        false,
                        move |value, _, cx| {
                            let _ = index_view.update(cx, |view, cx| {
                                if let Some(dialog) = view.structure_dialog.as_mut() {
                                    dialog.index = value.to_string();
                                }
                                cx.notify();
                            });
                        },
                        move |value, window, cx| {
                            let _ = index_enter_view.update(cx, |view, cx| {
                                if let Some(dialog) = view.structure_dialog.as_mut() {
                                    dialog.index = value.to_string();
                                }
                                view.submit_structure_dialog(window, cx);
                            });
                        },
                    ),
                ));
        }
        let submit_label = dialog.submit_label();
        let content =
            components::dialog_surface(theme, "structure-dialog", components::COMPACT_DIALOG_WIDTH)
                .child(components::dialog_title(theme, dialog.title()))
                .child(form)
                .child(
                    components::dialog_actions(theme)
                        .child(components::dialog_action_button(
                            theme,
                            "structure-cancel",
                            "Cancel",
                            components::DialogActionStyle::Secondary,
                            None,
                            false,
                            move |_, window, cx| {
                                let _ = cancel_view.update(cx, |view, cx| {
                                    if matches!(
                                        view.structure_dialog.as_ref().map(|dialog| &dialog.mode),
                                        Some(StructureDialogMode::SaveDetachedRequest { .. })
                                    ) {
                                        view.pending_close = None;
                                    }
                                    view.structure_dialog = None;
                                    view.focus_handle.focus(window, cx);
                                    cx.notify();
                                });
                            },
                        ))
                        .child(components::dialog_action_button(
                            theme,
                            "structure-submit",
                            submit_label,
                            components::DialogActionStyle::Primary,
                            components::shortcut_label_for_action_in_context(
                                window,
                                &SubmitStructureDialog,
                                "StructureDialog",
                            ),
                            save_busy,
                            move |_, window, cx| {
                                let _ = submit_view.update(cx, |view, cx| {
                                    view.submit_structure_dialog(window, cx);
                                });
                            },
                        )),
                );

        components::dialog_layer(
            theme,
            &self.structure_dialog_focus,
            "StructureDialog",
            content,
        )
        .into_any_element()
    }

    fn render_save_destination(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let Some(dialog) = self.structure_dialog.as_ref() else {
            return div();
        };
        let parent = dialog.parent.clone();
        let expanded = dialog.expanded_folders.clone();
        let busy = self.structure_task.is_some() || dialog.new_folder_name.is_some();
        let rows = self
            .loaded_workspace
            .as_ref()
            .map(|loaded| save_destination_rows(loaded, &expanded))
            .unwrap_or_else(|| {
                vec![SaveDestinationRow {
                    selector: ROOT_PARENT.to_owned(),
                    name: "Collection root".to_owned(),
                    depth: 0,
                    expandable: false,
                    expanded: true,
                }]
            });
        let mut tree = div()
            .id("save-destination-tree")
            .w_full()
            .max_h(px(theme.metrics.tree_row_height * 8.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .p(px(theme.metrics.spacing_1))
            .rounded(px(theme.metrics.radius_small))
            .border_1()
            .border_color(theme.colors.borders.standard)
            .bg(theme.colors.surfaces.sidebar);
        for (index, row) in rows.into_iter().enumerate() {
            tree = tree.child(self.render_save_destination_row(theme, index, &row, &parent, cx));
        }
        let open_view = cx.weak_entity();
        let open_action = cx.weak_entity();
        div()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_1))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(components::dialog_field_label(theme, "Destination"))
                    .child(
                        components::text_button(
                            theme,
                            "save-new-folder",
                            "New Folder",
                            move |_, window, cx| {
                                let _ = open_view.update(cx, |view, cx| {
                                    view.open_save_folder_dialog(window, cx);
                                });
                            },
                        )
                        .key_context("SaveDestination")
                        .on_action(move |_: &ActivateSaveDestination, window, cx| {
                            let _ = open_action.update(cx, |view, cx| {
                                view.open_save_folder_dialog(window, cx);
                            });
                        })
                        .when(busy, |button| {
                            button
                                .disabled(true)
                                .text_color(theme.colors.actions.disabled_foreground)
                        }),
                    ),
            )
            .child(tree)
    }

    fn render_save_destination_row(
        &self,
        theme: Theme,
        index: usize,
        row: &SaveDestinationRow,
        parent: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = row.selector == parent;
        let depth = row.depth;
        let select_id = if row.selector.is_empty() {
            "save-destination-root".to_owned()
        } else {
            format!("save-destination-{}", row.selector)
        };
        let selector = row.selector.clone();
        let select_view = cx.weak_entity();
        let select_action = cx.weak_entity();
        let action_selector = selector.clone();
        let mut select = Button::new(select_id.clone())
            .debug_selector({
                let select_id = select_id.clone();
                move || select_id
            })
            .accessibility_label(if row.selector.is_empty() {
                "Collection root".to_owned()
            } else {
                format!("Folder {}", row.name)
            })
            .key_context("SaveDestination")
            .flex_1()
            .min_w(px(0.0))
            .h(px(theme.metrics.tree_row_height))
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1))
            .overflow_hidden()
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let selector = selector.clone();
                let _ = select_view.update(cx, |view, cx| {
                    view.select_save_dialog_parent(selector, cx);
                });
            })
            .on_action(move |_: &ActivateSaveDestination, _, cx| {
                let selector = action_selector.clone();
                let _ = select_action.update(cx, |view, cx| {
                    view.select_save_dialog_parent(selector, cx);
                });
            });
        if !row.expandable {
            select = select.child(components::tree_folder_icon(theme, depth == 0, selected));
        }
        select = select.child(
            components::truncated_label(row.name.clone())
                .flex_1()
                .font_weight(FontWeight::SEMIBOLD)
                .when(selected, |label| {
                    label.text_color(theme.colors.selection.active_foreground)
                }),
        );

        let mut line = div()
            .id(("save-destination-row", index))
            .relative()
            .w_full()
            .h(px(theme.metrics.tree_row_height))
            .pl(px(tree_level_indent(theme, depth)))
            .pr(px(theme.metrics.spacing_1))
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1))
            .rounded(px(theme.metrics.radius_small))
            .when(selected, |line| {
                line.bg(theme.colors.selection.active_background)
                    .text_color(theme.colors.selection.active_foreground)
            })
            .when(!selected, |line| {
                line.hover(move |line| line.bg(theme.colors.surfaces.window))
            });
        if row.expandable {
            let toggle_view = cx.weak_entity();
            let toggle_action = cx.weak_entity();
            let toggle_selector = row.selector.clone();
            let action_selector = toggle_selector.clone();
            let expanded = row.expanded;
            line = line.child(
                Button::new(format!("save-destination-toggle-{}", row.selector))
                    .accessibility_label(if expanded {
                        format!("Collapse {}", row.name)
                    } else {
                        format!("Expand {}", row.name)
                    })
                    .key_context("SaveDestination")
                    .flex_none()
                    .w(px(theme.metrics.icon_standard))
                    .h(px(theme.metrics.tree_row_height))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(components::tree_folder_icon(theme, expanded, selected))
                    .on_click(move |_, _, cx| {
                        let toggle_selector = toggle_selector.clone();
                        let _ = toggle_view.update(cx, |view, cx| {
                            view.toggle_save_dialog_folder(toggle_selector, cx);
                        });
                    })
                    .on_action(move |_: &ActivateSaveDestination, _, cx| {
                        let selector = action_selector.clone();
                        let _ = toggle_action.update(cx, |view, cx| {
                            view.toggle_save_dialog_folder(selector, cx);
                        });
                    }),
            );
        }
        line = line.child(select);
        if depth > 0 {
            line = line.child(tree_hierarchy_guides(theme, depth, selected));
        }
        line.into_any_element()
    }

    pub(in crate::app) fn render_save_folder_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(name) = self.structure_dialog.as_ref().and_then(|dialog| {
            matches!(dialog.mode, StructureDialogMode::SaveDetachedRequest { .. })
                .then(|| dialog.new_folder_name.clone())
                .flatten()
        }) else {
            return div().into_any_element();
        };
        let name_view = cx.weak_entity();
        let name_enter_view = cx.weak_entity();
        let cancel_view = cx.weak_entity();
        let submit_view = cx.weak_entity();
        let busy = self.structure_task.is_some();
        let content = components::dialog_surface(
            theme,
            "save-folder-dialog",
            components::COMPACT_DIALOG_WIDTH,
        )
        .child(components::dialog_title(theme, "New Folder"))
        .child(
            div()
                .mt(px(theme.metrics.spacing_4))
                .child(components::dialog_field(
                    theme,
                    "Name",
                    components::dialog_text_input(
                        theme,
                        "save-folder-name",
                        name,
                        "",
                        true,
                        move |value, _, cx| {
                            let _ = name_view.update(cx, |view, cx| {
                                if let Some(dialog) = view.structure_dialog.as_mut() {
                                    dialog.new_folder_name = Some(value.to_string());
                                }
                                cx.notify();
                            });
                        },
                        move |value, _, cx| {
                            let _ = name_enter_view.update(cx, |view, _| {
                                if let Some(dialog) = view.structure_dialog.as_mut() {
                                    dialog.new_folder_name = Some(value.to_string());
                                }
                            });
                        },
                    )
                    .disabled(busy),
                )),
        )
        .child(
            components::dialog_actions(theme)
                .child(components::dialog_action_button(
                    theme,
                    "save-folder-cancel",
                    "Cancel",
                    components::DialogActionStyle::Secondary,
                    None,
                    busy,
                    move |_, window, cx| {
                        let _ = cancel_view.update(cx, |view, cx| {
                            view.close_save_folder_dialog(window, cx);
                        });
                    },
                ))
                .child(components::dialog_action_button(
                    theme,
                    "save-folder-create",
                    "Create",
                    components::DialogActionStyle::Primary,
                    components::shortcut_label_for_action_in_context(
                        window,
                        &SubmitSaveFolderDialog,
                        "SaveFolderDialog",
                    ),
                    busy,
                    move |_, window, cx| {
                        let _ = submit_view.update(cx, |view, cx| {
                            view.create_folder_from_save_dialog(window, cx);
                        });
                    },
                )),
        );
        components::dialog_layer(
            theme,
            &self.save_folder_dialog_focus,
            "SaveFolderDialog",
            content,
        )
        .into_any_element()
    }

    pub(in crate::app) fn render_create_environment_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(name) = self.create_environment_dialog.as_ref() else {
            return div().into_any_element();
        };
        let name_view = cx.weak_entity();
        let name_enter_view = cx.weak_entity();
        let cancel_view = cx.weak_entity();
        let submit_view = cx.weak_entity();
        let busy = self.environment_save_task.is_some();
        let mut content = components::dialog_surface(
            theme,
            "create-environment-dialog",
            components::COMPACT_DIALOG_WIDTH,
        )
        .child(components::dialog_title(theme, "New Environment"));
        content = content
            .child(
                div()
                    .mt(px(theme.metrics.spacing_4))
                    .flex()
                    .flex_col()
                    .gap(px(theme.metrics.spacing_3))
                    .child(components::dialog_field(
                        theme,
                        "Name",
                        components::dialog_text_input(
                            theme,
                            "create-environment-name",
                            name.clone(),
                            "",
                            true,
                            move |value, _, cx| {
                                let _ = name_view.update(cx, |view, cx| {
                                    if let Some(name) = view.create_environment_dialog.as_mut() {
                                        *name = value.to_string();
                                    }
                                    cx.notify();
                                });
                            },
                            move |value, window, cx| {
                                let _ = name_enter_view.update(cx, |view, cx| {
                                    if let Some(name) = view.create_environment_dialog.as_mut() {
                                        *name = value.to_string();
                                    }
                                    view.submit_create_environment_dialog(window, cx);
                                });
                            },
                        )
                        .disabled(busy),
                    )),
            )
            .child(
                components::dialog_actions(theme)
                    .child(components::dialog_action_button(
                        theme,
                        "create-environment-cancel",
                        "Cancel",
                        components::DialogActionStyle::Secondary,
                        None,
                        busy,
                        move |_, window, cx| {
                            let _ = cancel_view.update(cx, |view, cx| {
                                view.close_create_environment_dialog(window, cx);
                            });
                        },
                    ))
                    .child(components::dialog_action_button(
                        theme,
                        "create-environment-submit",
                        "Create",
                        components::DialogActionStyle::Primary,
                        components::shortcut_label_for_action_in_context(
                            window,
                            &SubmitCreateEnvironmentDialog,
                            "CreateEnvironmentDialog",
                        ),
                        busy,
                        move |_, window, cx| {
                            let _ = submit_view.update(cx, |view, cx| {
                                view.submit_create_environment_dialog(window, cx);
                            });
                        },
                    )),
            );

        components::dialog_layer(
            theme,
            &self.create_environment_dialog_focus,
            "CreateEnvironmentDialog",
            content,
        )
        .into_any_element()
    }
}
