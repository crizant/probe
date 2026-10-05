use super::*;
use gpui::SharedString;
use probe_core::FolderKey;

mod authentication;
mod body;
mod file;
mod form;
mod graphql;
mod headers;
mod multipart;
mod parameters;

impl ProbeApp {
    pub(super) fn render_editor_breadcrumb(
        &self,
        folders: &[FolderKey],
        request_name: Option<&str>,
        id: &'static str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut path = div()
            .id(SharedString::from(format!("{id}-path")))
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1))
            .overflow_x_scroll()
            .text_size(px(theme.typography.caption_size))
            .text_color(theme.colors.text.muted);
        for (index, key) in folders.iter().copied().enumerate() {
            let Some(folder) = self
                .loaded_workspace
                .as_ref()
                .and_then(|loaded| loaded.workspace().folder(key))
            else {
                continue;
            };
            if index > 0 {
                path = path.child(div().flex_none().child("›"));
            }
            let label = folder.metadata.name.as_deref().unwrap_or("Untitled folder");
            if request_name.is_none() && index + 1 == folders.len() {
                path = path.child(
                    components::truncated_label(label.to_owned())
                        .debug_selector(move || format!("{id}-folder-{index}"))
                        .max_w(px(220.0))
                        .flex_none()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.colors.text.primary),
                );
                continue;
            }
            let select_view = cx.weak_entity();
            path = path.child(
                Button::new((id, index))
                    .debug_selector(move || format!("{id}-folder-{index}"))
                    .accessibility_label(format!("Open {label} folder overview"))
                    .flex_none()
                    .max_w(px(220.0))
                    .cursor_pointer()
                    .hover(move |segment| segment.text_color(theme.colors.text.primary))
                    .child(components::truncated_label(label.to_owned()))
                    .on_click(move |_, _, cx| {
                        let _ = select_view.update(cx, |view, cx| {
                            view.select_open_tab(crate::shell::OverviewTab::Folder(key).into(), cx);
                        });
                    }),
            );
        }
        if let Some(name) = request_name {
            if !folders.is_empty() {
                path = path.child(div().flex_none().child("›"));
            }
            path = path.child(
                components::truncated_label(name.to_owned())
                    .debug_selector(|| "request-breadcrumb-request".into())
                    .max_w(px(220.0))
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.colors.text.primary),
            );
        }
        path
    }

    pub(super) fn render_request_editor(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let Some(key) = self.shell.active_tab() else {
            return div()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.colors.surfaces.editor)
                .text_color(theme.colors.text.muted)
                .child("Select a request from the collection sidebar.");
        };
        let Some(request) = self.active_request().cloned() else {
            return div().flex_1();
        };
        let method = request.method.as_deref().unwrap_or("GET").to_uppercase();
        let url = url_bar_value(&request);
        let request_dirty = self.persistence.is_dirty(key, &request);
        let folders = self
            .loaded_workspace
            .as_ref()
            .and_then(|loaded| loaded.workspace().request_ancestor_folders(key))
            .unwrap_or_default();
        let breadcrumb_path = self.render_editor_breadcrumb(
            folders,
            Some(
                request
                    .metadata
                    .name
                    .as_deref()
                    .unwrap_or("Untitled request"),
            ),
            "request-breadcrumb",
            theme,
            cx,
        );
        let breadcrumb = components::breadcrumb_header(
            theme,
            components::request_icon(
                theme,
                &components::RequestIcon::from_request(&request.kind, request.method.as_deref()),
            )
            .id("request-protocol-label")
            .debug_selector(|| "request-protocol-label".into()),
            breadcrumb_path,
            self.render_save_button(theme, "Save request", request_dirty, false, cx),
        )
        .id("request-breadcrumb")
        .debug_selector(|| "request-breadcrumb".into());
        let url_view = cx.weak_entity();
        let execution_view = cx.weak_entity();
        let request_running = self
            .execution
            .response(key)
            .is_some_and(ResponseState::is_running);
        let sections = EditorSection::for_protocol(request.kind.protocol());
        let mut section_tabs = Tabs::new("request-editor-sections")
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1));
        for (index, section) in sections.iter().copied().enumerate() {
            let section_view = cx.weak_entity();
            section_tabs = section_tabs.child(components::text_tab(
                theme,
                ("request-editor-section", index),
                format!(
                    "{}{}",
                    section.label(),
                    match section {
                        EditorSection::Query => format!("  {}", request.query_parameters.len()),
                        EditorSection::Path => format!("  {}", request.path_parameters.len()),
                        EditorSection::Headers => format!("  {}", request.headers.len()),
                        EditorSection::Docs
                        | EditorSection::Body
                        | EditorSection::Authentication
                        | EditorSection::GraphqlQuery
                        | EditorSection::GraphqlVariables
                        | EditorSection::GraphqlOperationName
                        | EditorSection::GraphqlExtensions => String::new(),
                    }
                ),
                self.request_editor.section(key) == section,
                index + 1,
                sections.len(),
                move |_, _, cx| {
                    let _ = section_view.update(cx, |view, cx| {
                        view.request_editor.set_section(key, section);
                        cx.notify();
                    });
                },
            ));
        }

        let section_kind = self.request_editor.section(key);
        let section_scrolls = !matches!(section_kind, EditorSection::Body | EditorSection::Docs)
            && !section_kind.is_graphql();
        if section_scrolls && self.request_section_scroll_owner.get() != Some((key, section_kind)) {
            self.request_section_scroll
                .set_offset(point(px(0.0), px(0.0)));
            self.request_section_scroll_owner
                .set(Some((key, section_kind)));
        }
        let section_scroll = self.request_section_scroll.clone();
        let list_scroll = section_scrolls.then_some(&section_scroll);
        let section = self.render_request_section(key, &request, theme, list_scroll, cx);

        div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(120.0))
            .flex()
            .flex_col()
            .bg(theme.colors.surfaces.editor)
            .child(
                div()
                    .p(px(theme.metrics.spacing_2))
                    .pb(px(theme.metrics.spacing_2))
                    .flex()
                    .flex_col()
                    .gap(px(theme.metrics.spacing_2))
                    .child(breadcrumb)
                    .child(
                        div()
                            .id("request-url-bar")
                            .debug_selector(|| "request-url-bar".into())
                            .h(px(theme.metrics.control_height))
                            .w_full()
                            .flex()
                            .items_center()
                            .child(div().w(px(108.0)).mr(px(theme.metrics.spacing_1)).child(
                                components::dropdown_with_option_colors(
                                    theme,
                                    "request-method",
                                    "HTTP method",
                                    Some(method.clone()),
                                    request_method_options(theme, &method),
                                    108.0,
                                    {
                                        let method_view = cx.weak_entity();
                                        move |value, _, cx| {
                                            let Some(value) = value.cloned() else {
                                                return;
                                            };
                                            let _ = method_view.update(cx, |view, cx| {
                                                view.edit_request(
                                                    key,
                                                    |request| request.method = Some(value),
                                                    cx,
                                                );
                                            });
                                        }
                                    },
                                ),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(components::url_text_input(
                                        theme,
                                        ("request-url", key.slot()),
                                        url.clone(),
                                        "https://api.example.com/users/:userId",
                                        self.variable_context(cx)
                                            .with_path_values(&request.path_parameters),
                                        move |value, _, input_cx| {
                                            let _ = url_view.update(input_cx, |view, cx| {
                                                view.edit_request(
                                                    key,
                                                    |request| apply_url_bar_value(request, &value),
                                                    cx,
                                                );
                                            });
                                        },
                                    )),
                            )
                            .child(div().ml(px(theme.metrics.spacing_1)).flex_none().child(
                                if request_running {
                                    components::primary_button(
                                        theme,
                                        "request-execution",
                                        "Cancel",
                                        move |_, _, cx| {
                                            let _ = execution_view.update(cx, |view, cx| {
                                                view.cancel_request(key, cx);
                                            });
                                        },
                                    )
                                    .into_any_element()
                                } else {
                                    let send_view = execution_view.clone();
                                    let menu_state_view = cx.weak_entity();
                                    let download_view = cx.weak_entity();
                                    let copy_view = cx.weak_entity();
                                    let popup = components::popup_surface(
                                        theme,
                                        "request-execution-menu-popup",
                                        180.0,
                                    )
                                    .child(
                                        components::menu_button(
                                            theme,
                                            "request-send-and-save",
                                            "Send and Save Body…",
                                            None,
                                            move |window, cx| {
                                                let _ = download_view.update(cx, |view, cx| {
                                                    view.choose_send_and_save(key, window, cx);
                                                });
                                            },
                                        ),
                                    );
                                    let popup = popup.when(
                                        matches!(
                                            request.kind.protocol(),
                                            probe_core::RequestProtocol::Http
                                                | probe_core::RequestProtocol::Graphql
                                        ),
                                        |popup| {
                                            popup.child(components::menu_button(
                                                theme,
                                                "request-copy-as-curl",
                                                "Copy as cURL",
                                                None,
                                                move |_, cx| {
                                                    let _ = copy_view.update(cx, |view, cx| {
                                                        view.copy_as_curl(key, cx)
                                                    });
                                                },
                                            ))
                                        },
                                    );
                                    components::DropdownButton::new(
                                        theme,
                                        "request-execution",
                                        "Send",
                                        move |_, _, cx| {
                                            let _ = send_view.update(cx, |view, cx| {
                                                view.send_request(key, cx);
                                            });
                                        },
                                    )
                                    .menu_trigger("request-execution-menu-trigger", "Send options")
                                    .open(self.transient.request_execution_menu_open)
                                    .on_open_change(move |open, _, cx| {
                                        let _ = menu_state_view.update(cx, |view, cx| {
                                            view.transient.request_execution_menu_open = *open;
                                            cx.notify();
                                        });
                                    })
                                    .menu("request-execution-menu", popup)
                                    .into_any_element()
                                },
                            )),
                    )
                    .child(section_tabs),
            )
            .child(
                div()
                    .id("request-editor-section-content")
                    .flex_1()
                    .min_h(px(0.0))
                    .px(px(theme.metrics.spacing_2))
                    .pb(px(theme.metrics.spacing_2))
                    .when(section_scrolls, |content| {
                        content
                            .overflow_y_scroll()
                            .track_scroll(&self.request_section_scroll)
                    })
                    .child(section),
            )
    }

    fn render_request_section(
        &self,
        key: RequestKey,
        request: &Request,
        theme: Theme,
        list_scroll: Option<&ScrollHandle>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match self.request_editor.section(key) {
            EditorSection::Docs => {
                let view = cx.weak_entity();
                documentation_sections(
                    theme,
                    "Description",
                    [
                        documentation_text(request.metadata.description.as_ref()),
                        request.docs.as_deref(),
                    ],
                    [
                        ("request-description", key.slot()).into(),
                        ("request-docs", key.slot()).into(),
                    ],
                    move |docs, value, _, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.edit_request(
                                key,
                                |request| {
                                    if docs {
                                        request.docs = Some(value.to_string());
                                    } else {
                                        crate::app::documentation::edit_documentation(
                                            &mut request.metadata.description,
                                            value.to_string(),
                                        );
                                    }
                                },
                                cx,
                            )
                        });
                    },
                )
                .into_any_element()
            }
            EditorSection::Query => self.render_parameter_editor(
                key,
                request,
                ParameterEditorKind::Query,
                theme,
                list_scroll,
                cx,
            ),
            EditorSection::Path => self.render_parameter_editor(
                key,
                request,
                ParameterEditorKind::Path,
                theme,
                list_scroll,
                cx,
            ),
            EditorSection::Headers => {
                self.render_header_editor(key, request, theme, list_scroll, cx)
            }
            EditorSection::Body => self.render_body_editor(key, request, theme, cx),
            EditorSection::Authentication => {
                self.render_authentication_editor(key, request, theme, list_scroll, cx)
            }
            EditorSection::GraphqlQuery => {
                self.render_graphql_query_editor(key, request, theme, cx)
            }
            EditorSection::GraphqlVariables => {
                self.render_graphql_variables_editor(key, request, theme, cx)
            }
            EditorSection::GraphqlOperationName => {
                self.render_graphql_operation_name_editor(key, request, theme, cx)
            }
            EditorSection::GraphqlExtensions => {
                self.render_graphql_extensions_editor(key, request, theme, cx)
            }
        }
    }
}

struct SaveTooltip {
    theme: Theme,
    label: String,
}

impl Render for SaveTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_base::Tooltip::new("editor-save-tooltip")
            .px(px(self.theme.metrics.spacing_2))
            .py(px(self.theme.metrics.spacing_1))
            .rounded(px(self.theme.metrics.radius_small))
            .border_1()
            .border_color(self.theme.colors.borders.standard)
            .bg(self.theme.colors.surfaces.overlay)
            .text_size(px(self.theme.typography.caption_size))
            .text_color(self.theme.colors.text.primary)
            .child(self.label.clone())
    }
}

impl ProbeApp {
    pub(super) fn render_save_button(
        &self,
        theme: Theme,
        label: &'static str,
        dirty: bool,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let view = cx.weak_entity();
        let enabled = dirty && !busy;
        Button::new("editor-save")
            .accessibility_label(label)
            .debug_selector(|| "editor-save".into())
            .disabled(!enabled)
            .tooltip(move |_, cx| {
                let shortcut = if cfg!(target_os = "macos") {
                    "⌘S"
                } else {
                    "Ctrl+S"
                };
                cx.new(|_| SaveTooltip {
                    theme,
                    label: format!("{label} ({shortcut})"),
                })
                .into()
            })
            .ml(px(theme.metrics.spacing_2))
            .flex_none()
            .w(px(theme.metrics.control_height))
            .h(px(theme.metrics.control_height))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme.metrics.radius_small))
            .border_1()
            .border_color(theme.colors.borders.standard)
            .bg(theme.colors.surfaces.raised)
            .hover(move |button| button.bg(theme.colors.selection.inactive_background))
            .focus_visible(move |button| button.border_color(theme.colors.borders.focused))
            .styles(move |styles| {
                styles.disabled(move |button| {
                    button
                        .bg(theme.colors.selection.inactive_background)
                        .border_color(theme.colors.selection.inactive_background)
                        .text_color(theme.colors.actions.disabled_foreground)
                })
            })
            .child(components::save_icon(theme).text_color(if enabled {
                theme.colors.actions.accent
            } else {
                theme.colors.actions.disabled_foreground
            }))
            .on_click(move |_, window, cx| {
                let _ = view.update(cx, |view, cx| view.save_active_editor(window, cx));
            })
    }
}
