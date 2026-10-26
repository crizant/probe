use super::*;
use probe_application::parse_curl;

use crate::structure_editor::CurlImport;

impl CurlImport {
    fn contains_credentials(&self) -> bool {
        self.parsed
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .is_some_and(|request| {
                request.authentication.is_some()
                    || request.headers.iter().any(|header| {
                        [
                            "authorization",
                            "proxy-authorization",
                            "cookie",
                            "set-cookie",
                            "x-api-key",
                            "api-key",
                        ]
                        .iter()
                        .any(|name| header.name.eq_ignore_ascii_case(name))
                    })
            })
    }
}

impl ProbeApp {
    pub(super) fn curl_import_in_progress(&self) -> bool {
        self.structure_task.is_some()
            && self
                .structure_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.curl.is_some())
    }

    pub(super) fn open_curl_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loaded_workspace.is_none() || self.loading || self.has_active_workspace_write() {
            return;
        }
        self.curl_parse_task = None;
        self.create_environment_dialog = None;
        self.structure_dialog = Some(StructureDialog::import_curl(
            self.selected_parent_selector(),
        ));
        self.expand_destination_ancestors();
        self.destination_scroll = ScrollHandle::new();
        self.revealed_destination = None;
        self.structure_dialog_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn edit_curl_command(&mut self, command: String, cx: &mut Context<Self>) {
        if self.structure_task.is_some() {
            return;
        }
        let Some(curl) = self
            .structure_dialog
            .as_mut()
            .and_then(|dialog| dialog.curl.as_mut())
        else {
            return;
        };
        if curl.committed {
            return;
        }
        curl.command = command.clone();
        curl.parsed = None;
        curl.error = None;
        // Replacing the task cancels delivery of an obsolete preview, including
        // after closing and reopening the dialog with the same command.
        self.curl_parse_task = Some(cx.spawn(async move |view, cx| {
            let parsed = cx
                .background_spawn(async move { parse_curl(&command) })
                .await;
            let _ = view.update(cx, |view, cx| {
                if let Some(curl) = view
                    .structure_dialog
                    .as_mut()
                    .and_then(|dialog| dialog.curl.as_mut())
                {
                    curl.parsed = Some(parsed);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(super) fn set_curl_import_error(&mut self, message: String, cx: &mut Context<Self>) {
        if let Some(curl) = self
            .structure_dialog
            .as_mut()
            .and_then(|dialog| dialog.curl.as_mut())
        {
            curl.error = Some(message);
            cx.notify();
        }
    }

    pub(super) fn curl_import_disabled(&self) -> bool {
        self.loading
            || self.has_active_workspace_write()
            || self.structure_dialog.as_ref().is_none_or(|dialog| {
                dialog.destination_required
                    || dialog.name.trim().is_empty()
                    || dialog.curl.as_ref().is_none_or(|curl| {
                        curl.committed
                            || curl.destination_required
                            || !matches!(curl.parsed, Some(Ok(_)))
                    })
            })
    }

    pub(super) fn render_curl_import_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let dialog = self
            .structure_dialog
            .as_ref()
            .expect("import dialog exists");
        let curl = dialog.curl.as_ref().expect("import state exists");
        let busy = self.structure_task.is_some();
        let locked = busy || curl.committed;
        let name_view = cx.weak_entity();
        let command_view = cx.weak_entity();
        let cancel_view = cx.weak_entity();
        let submit_view = cx.weak_entity();
        let mut form = div()
            .mt(px(theme.metrics.spacing_4))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_3))
            .child(components::dialog_field(
                theme,
                "Request name",
                components::dialog_text_input(
                    theme,
                    "curl-name",
                    dialog.name.clone(),
                    "",
                    true,
                    move |value, _, cx| {
                        let _ = name_view.update(cx, |view, cx| {
                            if !locked && let Some(dialog) = view.structure_dialog.as_mut() {
                                dialog.name = value.to_string();
                            }
                            cx.notify();
                        });
                    },
                    |_, _, _| {},
                )
                .disabled(locked),
            ))
            .child(self.render_destination_picker(theme, cx))
            .child(components::dialog_field(
                theme,
                "cURL command",
                div()
                    .h(px(140.0))
                    .capture_action(|_: &gpui_kit::base::input::Escape, window, cx| {
                        window.dispatch_action(Box::new(CancelStructureDialog), cx);
                        cx.stop_propagation();
                    })
                    .child(components::curl_command_input(
                        theme,
                        "curl-command".into(),
                        curl.command.clone(),
                        locked,
                        move |value, _, cx| {
                            let _ = command_view.update(cx, |view, cx| {
                                view.edit_curl_command(value.to_string(), cx);
                            });
                        },
                    )),
            ));
        if let Some(Ok(request)) = &curl.parsed {
            form = form.child(
                div()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.text.secondary)
                    .whitespace_normal()
                    .child(format!(
                        "{} {}\n{} headers · {} query parameters · {} path parameters · {} body",
                        request.method.as_deref().unwrap_or("GET"),
                        request.url.as_deref().unwrap_or(""),
                        request.headers.len(),
                        request.query_parameters.len(),
                        request.path_parameters.len(),
                        usize::from(request.http_body().is_some())
                    )),
            );
        } else if !curl.command.is_empty() {
            let message = match &curl.parsed {
                Some(Err(error)) => format!(
                    "Cannot import: {error}. Use one supported POSIX cURL command with inline values."
                ),
                _ => "Parsing command…".into(),
            };
            form = form.child(
                div()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.status.error)
                    .child(message),
            );
        }
        if curl.contains_credentials() {
            form = form.child(div().text_size(px(theme.typography.caption_size)).text_color(theme.colors.status.warning)
                .child("Authentication headers or cookies will be persisted in plaintext in the collection. Remove sensitive values before importing if needed."));
        }
        if dialog.name.trim().is_empty() && !curl.command.is_empty() {
            form = form.child(
                div()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.status.error)
                    .child("Request name is required."),
            );
        }
        if dialog.destination_required || curl.destination_required {
            form = form.child(
                div()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.status.error)
                    .child(DESTINATION_RESELECTION_MESSAGE),
            );
        }
        if let Some(error) = &curl.error {
            form = form.child(
                div()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.status.error)
                    .child(error.clone()),
            );
        }
        let content = components::dialog_surface(
            theme,
            "curl-import-dialog",
            components::COMPACT_DIALOG_WIDTH,
        )
        .max_h(window.viewport_size().height - px(theme.metrics.spacing_4 * 2.0))
        .overflow_y_scroll()
        .child(components::dialog_title(theme, "Import from cURL"))
        .child(form)
        .child(
            components::dialog_actions(theme)
                .child(components::dialog_action_button(
                    theme,
                    "curl-cancel",
                    "Cancel",
                    components::DialogActionStyle::Secondary,
                    None,
                    busy,
                    move |_, window, cx| {
                        let _ = cancel_view.update(cx, |view, cx| {
                            if view.structure_task.is_some() {
                                return;
                            }
                            view.curl_parse_task = None;
                            view.structure_dialog = None;
                            view.tree_focus_handle.focus(window, cx);
                            cx.notify();
                        });
                    },
                ))
                .child(components::dialog_action_button(
                    theme,
                    "curl-import",
                    if busy {
                        "Importing…"
                    } else {
                        "Import Request"
                    },
                    components::DialogActionStyle::Primary,
                    components::shortcut_label_for_action_in_context(
                        window,
                        &SubmitStructureDialog,
                        "CurlImportDialog",
                    ),
                    self.curl_import_disabled(),
                    move |_, window, cx| {
                        let _ = submit_view
                            .update(cx, |view, cx| view.submit_structure_dialog(window, cx));
                    },
                )),
        );
        components::dialog_layer(
            theme,
            &self.structure_dialog_focus,
            "CurlImportDialog",
            content,
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_for_literal_authentication_and_cookies_without_exposing_values() {
        for command in [
            "curl https://example.test -u user:password",
            "curl https://example.test -H 'aUtHoRiZaTiOn: Bearer token'",
            "curl https://example.test -H 'Proxy-Authorization: Basic token'",
            "curl https://example.test -b 'session=token'",
            "curl https://example.test -H 'X-Api-Key: token'",
        ] {
            let curl = CurlImport {
                parsed: Some(parse_curl(command)),
                ..Default::default()
            };
            assert!(curl.contains_credentials());
        }
        let curl = CurlImport {
            parsed: Some(parse_curl(
                "curl https://example.test -H 'Accept: application/json'",
            )),
            ..Default::default()
        };
        assert!(!curl.contains_credentials());
    }
}
