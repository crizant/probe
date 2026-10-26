use super::*;

fn about_app_icon() -> Arc<gpui_kit::Image> {
    static ICON: std::sync::OnceLock<Arc<gpui_kit::Image>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            include_bytes!(
                "../../../assets/app-icon/linux/hicolor/128x128/apps/dev.probe.desktop.png"
            )
            .to_vec(),
        ))
    })
    .clone()
}

impl ProbeApp {
    pub(super) fn render_application_dialog_actions(
        &self,
        theme: Theme,
        specs: &[DialogActionSpec],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let settings_theme = match self.application_dialog {
            Some(ApplicationDialog::Settings { theme }) => Some(theme),
            _ => None,
        };
        let mut actions = components::dialog_actions(theme);
        for spec in specs.iter().copied() {
            let view = cx.weak_entity();
            let shortcut_hint = match spec.style {
                components::DialogActionStyle::Primary => {
                    components::shortcut_label_for_action_in_context(
                        window,
                        &SubmitApplicationDialog,
                        if settings_theme.is_some() {
                            "SettingsDialog"
                        } else {
                            "ApplicationDialog"
                        },
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
                if self.saving_user_config && spec.id == "settings-dialog-save" {
                    "Saving…"
                } else {
                    spec.label
                },
                spec.style,
                shortcut_hint,
                settings_theme.is_some_and(|mode| {
                    self.saving_user_config
                        || (spec.action == ApplicationDialogAction::Save
                            && mode == self.user_config.theme)
                }),
                move |_, window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.handle_application_dialog_action(spec.action, window, cx);
                    });
                },
            ));
        }
        actions
            .when(settings_theme.is_some(), |actions| {
                actions.mt(px(theme.metrics.spacing_4 * 1.5))
            })
            .when(
                matches!(self.application_dialog, Some(ApplicationDialog::About)),
                |actions| actions.mt(px(theme.metrics.spacing_4 + theme.metrics.spacing_1)),
            )
    }

    pub(in crate::app) fn render_application_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(dialog) = self.application_dialog.as_ref() else {
            return div().into_any_element();
        };
        let description = if let ApplicationDialog::DeleteStoredSecret {
            name, environment, ..
        } = dialog
        {
            div()
                .id("application-dialog-description")
                .mt(px(theme.metrics.spacing_2))
                .max_h(px(280.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_2))
                .child(
                    div()
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
                        .child(
                            components::truncated_label(name.clone())
                                .font_family(theme.typography.monospace_family)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.colors.text.primary),
                        )
                        .child(div().flex_none().text_color(theme.colors.text.muted).child("in"))
                        .child(
                            components::truncated_label(environment.clone())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.colors.actions.accent),
                        ),
                )
                .child(
                    components::dialog_description(
                        theme,
                        "Requests using this secret will fail until another value is stored. The secret declaration will remain.",
                    )
                    .line_height(relative(theme.typography.body_line_height)),
                )
        } else {
            components::dialog_description(theme, dialog.description())
                .id("application-dialog-description")
                .mt(px(theme.metrics.spacing_2))
                .max_h(px(280.0))
                .overflow_y_scroll()
                .line_height(relative(theme.typography.body_line_height))
        };
        let mut content = components::dialog_surface(theme, "application-dialog", dialog.width());
        if matches!(dialog, ApplicationDialog::About) {
            content = content
                .p(px(theme.metrics.spacing_4 + theme.metrics.spacing_1))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_3))
                        .child(
                            img(about_app_icon())
                                .size(px(48.0))
                                .flex_none()
                                .rounded(px(theme.metrics.radius_medium)),
                        )
                        .child(
                            div()
                                .min_w(px(0.0))
                                .flex()
                                .flex_col()
                                .gap(px(theme.metrics.spacing_1))
                                .child(components::dialog_title(theme, dialog.title()))
                                .child(
                                    div()
                                        .text_size(px(theme.typography.caption_size))
                                        .text_color(theme.colors.text.muted)
                                        .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                                ),
                        ),
                )
                .child(
                    div()
                        .mt(px(theme.metrics.spacing_3))
                        .text_size(px(theme.typography.body_size))
                        .text_color(theme.colors.text.primary)
                        .line_height(relative(theme.typography.body_line_height))
                        .child(dialog.description().into_owned()),
                );
        } else {
            content = content
                .child(components::dialog_title(theme, dialog.title()))
                .when(!dialog.description().is_empty(), |content| {
                    content.child(description)
                });
        }

        if let ApplicationDialog::Settings { theme: draft_theme } = dialog {
            content = content.p(px(theme.metrics.spacing_4 + theme.metrics.spacing_1));
            let view = cx.weak_entity();
            content = content.child(
                div()
                    .mt(px(theme.metrics.spacing_4))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(theme.metrics.spacing_3))
                    .child(
                        div()
                            .text_size(px(theme.typography.body_size))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.colors.text.secondary)
                            .child("Theme mode"),
                    )
                    .child(
                        div().w(px(180.0)).flex_none().child(
                            components::dropdown(
                                theme,
                                "settings-theme-mode",
                                "Theme mode",
                                Some(*draft_theme),
                                vec![
                                    (ThemeMode::System, "System".to_owned()),
                                    (ThemeMode::Light, "Light".to_owned()),
                                    (ThemeMode::Dark, "Dark".to_owned()),
                                ],
                                180.0,
                                move |mode, _, cx| {
                                    if let Some(mode) = mode.copied() {
                                        let _ = view.update(cx, |view, cx| {
                                            if !view.saving_user_config
                                                && let Some(ApplicationDialog::Settings { theme }) =
                                                    &mut view.application_dialog
                                            {
                                                *theme = mode;
                                                cx.notify();
                                            }
                                        });
                                    }
                                },
                            )
                            .disabled(self.saving_user_config),
                        ),
                    ),
            );
        }

        if let Some(specs) = dialog.action_specs() {
            content =
                content.child(self.render_application_dialog_actions(theme, specs, window, cx));
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
                .child(self.render_application_dialog_actions(
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
                .child(self.render_application_dialog_actions(
                    theme,
                    &[CANCEL_DIALOG_ACTION],
                    window,
                    cx,
                ));
        }

        components::dialog_layer(
            theme,
            &self.application_dialog_focus,
            if matches!(dialog, ApplicationDialog::Settings { .. }) {
                "SettingsDialog"
            } else {
                "ApplicationDialog"
            },
            content,
        )
        .into_any_element()
    }

    pub(in crate::app) fn render_structure_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(dialog) = self.structure_dialog.as_ref() else {
            return div().into_any_element();
        };
        if dialog.curl.is_some() {
            return self.render_curl_import_dialog(theme, window, cx);
        }
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
                        if view
                            .structure_dialog
                            .as_ref()
                            .is_some_and(|dialog| !dialog.edits_destination())
                        {
                            view.submit_structure_dialog(window, cx);
                        }
                    });
                },
            ),
        ));
        if dialog.edits_destination() {
            form = form.child(self.render_destination_picker(theme, cx));
        }
        let destination_required = dialog.destination_required;
        if destination_required {
            form = form.child(
                div()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.status.error)
                    .child(DESTINATION_RESELECTION_MESSAGE),
            );
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
                                dialog.key_context(),
                            ),
                            save_busy || destination_required,
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
            dialog.key_context(),
            content,
        )
        .into_any_element()
    }

    pub(in crate::app) fn render_destination_picker(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let Some(dialog) = self.structure_dialog.as_ref() else {
            return div();
        };
        let parent = dialog.parent.clone();
        let expanded = dialog.expanded_folders.clone();
        let locked = self.destination_picker_locked();
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
        let select_view = cx.weak_entity();
        let toggle_view = cx.weak_entity();
        let tree = super::super::tree::destination_tree(
            theme,
            &rows,
            (!(dialog.destination_required
                || dialog
                    .curl
                    .as_ref()
                    .is_some_and(|curl| curl.destination_required)))
            .then_some(parent.as_str()),
            locked,
            &self.destination_scroll,
            move |selector, cx| {
                let _ = select_view.update(cx, |view, cx| {
                    view.select_destination_parent(selector, cx);
                });
            },
            move |selector, cx| {
                let _ = toggle_view.update(cx, |view, cx| {
                    view.toggle_destination_folder(selector, cx);
                });
            },
        );
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
                    .when(
                        matches!(dialog.mode, StructureDialogMode::SaveDetachedRequest { .. }),
                        |header| {
                            header.child(
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
                                .when(locked, |button| {
                                    button
                                        .disabled(true)
                                        .text_color(theme.colors.actions.disabled_foreground)
                                }),
                            )
                        },
                    ),
            )
            .child(tree)
    }

    pub(in crate::app) fn render_save_folder_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
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
        let destination_required = self
            .structure_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.destination_required);
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
        .when(destination_required, |content| {
            content.child(
                div()
                    .mt(px(theme.metrics.spacing_3))
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.status.error)
                    .child(DESTINATION_RESELECTION_MESSAGE),
            )
        })
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
                    busy || destination_required,
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
    ) -> gpui_kit::AnyElement {
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
