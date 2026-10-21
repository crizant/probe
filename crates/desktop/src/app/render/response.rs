use super::*;

impl ProbeApp {
    pub(super) fn render_response_panel(
        &self,
        theme: Theme,
        window_height: f32,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        if self.active_request().is_some_and(|r| r.kind.is_websocket()) {
            return self.render_websocket_panel(theme, window_height, cx);
        }
        let active_key = self.shell.active_tab();
        let state = active_key.and_then(|key| self.execution.response(key));
        let shows_response_tabs = matches!(state, Some(ResponseState::Complete { .. }))
            && active_key.is_some_and(|key| self.response_viewer.document(key).is_some());
        let header_vertical_padding = if shows_response_tabs {
            theme.metrics.spacing_1
        } else {
            theme.metrics.spacing_2
        };
        let (header_leading, header_trailing, content) = match state {
            Some(
                state @ ResponseState::Running {
                    progress: Some(progress),
                    ..
                },
            ) => {
                let status = format!("{} {}", progress.status, progress.reason);
                let transfer =
                    format_transfer_progress(progress.received_bytes, progress.content_length);
                (
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Receiving…")
                        .into_any_element(),
                    div()
                        .min_w(px(0.0))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(theme.metrics.spacing_1))
                        .child(
                            components::truncated_label(status.trim_end().to_owned())
                                .id("response-status-code")
                                .debug_selector(|| "response-status-code".into())
                                .flex_none()
                                .max_w(px(220.0))
                                .text_color(response_status_color(theme, progress.status)),
                        )
                        .child(
                            components::truncated_label(format!(
                                "• {} • {}",
                                transfer,
                                format_duration(state.elapsed().unwrap_or_default())
                            ))
                            .id("response-metadata")
                            .debug_selector(|| "response-metadata".into())
                            .flex_none()
                            .font_family(theme.typography.monospace_family)
                            .text_color(theme.colors.text.muted),
                        )
                        .into_any_element(),
                    div()
                        .id("response-receiving-body")
                        .debug_selector(|| "response-receiving-body".into())
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme.colors.text.muted)
                        .child("Receiving the response body…")
                        .into_any_element(),
                )
            }
            Some(state @ ResponseState::Running { .. }) => (
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Sending…")
                    .into_any_element(),
                div()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        components::truncated_label(format_duration(
                            state.elapsed().unwrap_or_default(),
                        ))
                        .text_color(theme.colors.text.muted),
                    )
                    .into_any_element(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child("Waiting for the server…")
                    .into_any_element(),
            ),
            Some(ResponseState::Cancelled) => (
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Response")
                    .into_any_element(),
                div()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        components::truncated_label("Cancelled")
                            .text_color(theme.colors.text.muted),
                    )
                    .into_any_element(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child("Request cancelled.")
                    .into_any_element(),
            ),
            Some(ResponseState::Failed(error)) => (
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_1))
                    .children(
                        [ResponseViewerTab::Pretty, ResponseViewerTab::Assertions]
                            .into_iter()
                            .enumerate()
                            .map(|(index, tab)| {
                                let view = cx.weak_entity();
                                components::text_tab(
                                    theme,
                                    ("failed-response-tab", index),
                                    if index == 0 { "Response" } else { "Assertions" },
                                    active_key
                                        .is_some_and(|key| self.response_viewer.tab(key) == tab),
                                    index + 1,
                                    2,
                                    move |_, _, cx| {
                                        let _ = view
                                            .update(cx, |view, cx| view.set_response_tab(tab, cx));
                                    },
                                )
                                .debug_selector(move || format!("failed-response-tab-{index}"))
                            }),
                    )
                    .into_any_element(),
                div()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        components::truncated_label("Failed").text_color(theme.colors.status.error),
                    )
                    .into_any_element(),
                if active_key.is_some_and(|key| {
                    self.response_viewer.tab(key) == ResponseViewerTab::Assertions
                }) {
                    div()
                        .id("assertions-not-evaluated")
                        .debug_selector(|| "assertions-not-evaluated".into())
                        .child(placeholder_message(theme, "Not evaluated — request failed"))
                        .into_any_element()
                } else {
                    div()
                        .id("response-error-scroll")
                        .flex_1()
                        .p(px(theme.metrics.spacing_3))
                        .overflow_y_scroll()
                        .text_color(theme.colors.status.error)
                        .child(error.clone())
                        .into_any_element()
                },
            ),
            Some(ResponseState::Complete {
                response, saved_to, ..
            }) => {
                let status = format!("{} {}", response.status, response.reason);
                let metadata = format!(
                    "• {} • {}",
                    format_duration(response.duration),
                    format_size(response.size as u64),
                );
                let document = active_key.and_then(|key| self.response_viewer.document(key));
                let can_save = response.size > 0
                    && (response.body_complete
                        || response.body_file.is_some()
                        || saved_to.is_some());
                let save_view = cx.weak_entity();
                (
                    document.map_or_else(
                        || {
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Response")
                                .into_any_element()
                        },
                        |document| self.render_response_tabs(theme, document, cx),
                    ),
                    div()
                        .min_w(px(0.0))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(theme.metrics.spacing_1))
                        .child(
                            components::truncated_label(status.trim_end().to_owned())
                                .id("response-status-code")
                                .debug_selector(|| "response-status-code".into())
                                .flex_none()
                                .max_w(px(220.0))
                                .text_color(response_status_color(theme, response.status)),
                        )
                        .child(
                            div()
                                .id("response-metadata")
                                .debug_selector(|| "response-metadata".into())
                                .flex_none()
                                .text_color(theme.colors.text.muted)
                                .child(metadata),
                        )
                        .when(can_save, |header| {
                            header.child(components::icon_button(
                                theme,
                                "response-save-body",
                                "Save response body",
                                components::download_icon(theme),
                                move |_, window, cx| {
                                    if let Some(key) = active_key {
                                        let _ = save_view.update(cx, |view, cx| {
                                            view.choose_response_save(key, window, cx);
                                        });
                                    }
                                },
                            ))
                        })
                        .into_any_element(),
                    self.render_response_document(theme, document, cx),
                )
            }
            None => (
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Response")
                    .into_any_element(),
                div().into_any_element(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child("Send a request to see its response.")
                    .into_any_element(),
            ),
        };

        div()
            .when(self.shell.pane_layout == PaneLayout::Vertical, |panel| {
                panel
                    .h(px(self.shell.response_height_for_window(window_height)))
                    .w_full()
            })
            .when(self.shell.pane_layout == PaneLayout::Horizontal, |panel| {
                panel.w(px(self.shell.response_width)).h_full()
            })
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.colors.surfaces.raised)
            .child(
                div()
                    .py(px(header_vertical_padding))
                    .px(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(theme.metrics.spacing_2))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .items_center()
                            .child(header_leading),
                    )
                    .child(
                        div()
                            .id("response-status")
                            .debug_selector(|| "response-status".into())
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(theme.typography.caption_size))
                            .child(header_trailing),
                    ),
            )
            .child(content)
    }

    pub(super) fn render_response_tabs(
        &self,
        theme: Theme,
        document: &PreparedDocument,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let key = self
            .shell
            .active_tab()
            .expect("response tabs require an active request");
        let mut tabs = Tabs::new("response-view-tabs")
            .w_full()
            .min_w(px(0.0))
            .overflow_x_scroll()
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1));
        let mut available_tabs = if document.truncated {
            ResponseViewerTab::TRUNCATED.to_vec()
        } else {
            ResponseViewerTab::ALL.to_vec()
        };
        available_tabs.push(ResponseViewerTab::Assertions);
        for (index, tab) in available_tabs.iter().copied().enumerate() {
            let tab_view = cx.weak_entity();
            let selected = self.response_viewer.tab(key) == tab;
            let label = if tab == ResponseViewerTab::Pretty && document.is_image() {
                "Preview".to_owned()
            } else if tab == ResponseViewerTab::Inspect {
                let count = document.inspection.count();
                if count > 0 {
                    format!("Inspect [{count}]")
                } else {
                    tab.label().to_owned()
                }
            } else {
                tab.label().to_owned()
            };
            let debug_label = label.to_ascii_lowercase();
            tabs = tabs.child(
                components::text_tab(
                    theme,
                    ("response-view-tab", index),
                    label,
                    selected,
                    index + 1,
                    available_tabs.len(),
                    move |_, _, cx| {
                        let _ = tab_view.update(cx, |view, cx| {
                            view.set_response_tab(tab, cx);
                        });
                    },
                )
                .when(tab == ResponseViewerTab::Assertions, |tab| {
                    let failures = match self.execution.response(key) {
                        Some(ResponseState::Complete { assertions, .. }) => {
                            crate::response_viewer::assertions::counts(assertions)[0]
                        }
                        _ => 0,
                    };
                    tab.accessibility_label(if failures > 0 {
                        format!("Assertions, {failures} failed")
                    } else {
                        "Assertions".into()
                    })
                    .when(failures > 0, |tab| {
                        tab.child(
                            div()
                                .ml(px(theme.metrics.spacing_1))
                                .text_color(theme.colors.status.error)
                                .debug_selector(move || {
                                    format!("assertion-failure-count-{failures}")
                                })
                                .child(failures.to_string()),
                        )
                    })
                })
                .flex_none()
                .debug_selector(move || format!("response-tab-{debug_label}")),
            );
        }
        tabs.into_any_element()
    }

    fn render_assertion_results(
        &self,
        key: RequestKey,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use crate::response_viewer::assertions::{counts, ordered, summary};
        let (assertions, declarations) = match self.execution.response(key) {
            Some(ResponseState::Complete {
                assertions,
                assertion_declarations,
                ..
            }) => (assertions.as_slice(), assertion_declarations.as_slice()),
            Some(ResponseState::Failed(_)) => {
                return div()
                    .id("assertions-not-evaluated")
                    .debug_selector(|| "assertions-not-evaluated".into())
                    .child(placeholder_message(theme, "Not evaluated — request failed"))
                    .into_any_element();
            }
            _ => return placeholder_message(theme, "Send a request to see its response."),
        };
        let container = Self::response_tab_content_spacing(theme)
            .id("response-assertions")
            .debug_selector(|| "response-assertions".into())
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .line_height(relative(1.25))
            .text_color(theme.colors.text.primary);
        if assertions.is_empty() {
            let view = cx.weak_entity();
            return container
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(placeholder_message(
                            theme,
                            "No assertions. Add one in the request's Assertions tab.",
                        ))
                        .child(components::editor_add_button(
                            theme,
                            "open-request-assertions",
                            "Add Assertion",
                            move |_, _, cx| {
                                let _ = view.update(cx, |view, cx| {
                                    view.request_editor
                                        .set_section(key, EditorSection::Assertions);
                                    cx.notify();
                                });
                            },
                        )),
                )
                .into_any_element();
        }
        let [_, _, _, skipped] = counts(assertions);
        let rows = ordered(assertions);
        let selected = self
            .response_viewer
            .assertion_selection(key)
            .or_else(|| rows.first().map(|result| result.index));
        let mut content = div()
            .id("assertion-result-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .when_some(
                self.response_viewer.assertion_scroll(key),
                |content, scroll| content.track_scroll(scroll),
            )
            .child(div().when(skipped != assertions.len(), |content| {
                content.child(self.render_assertion_summary(assertions, theme))
            }));
        let container = container.when(skipped == assertions.len(), |container| {
            container.child(
                div()
                    .id("assertions-all-disabled")
                    .debug_selector(|| "assertions-all-disabled".into())
                    .child(placeholder_message(theme, &summary(assertions)[0])),
            )
        });
        for result in rows {
            content = content.child(self.render_assertion_result_row(
                key,
                result,
                declarations.get(result.index),
                selected,
                theme,
                cx,
            ));
        }
        container
            .track_focus(&self.assertion_results_focus)
            .tab_stop(true)
            .child(content)
            .on_key_down(
                cx.listener(move |view, event: &gpui_kit::KeyDownEvent, _, cx| {
                    view.handle_assertion_result_key(key, event, cx);
                }),
            )
            .into_any_element()
    }

    fn render_assertion_summary(
        &self,
        assertions: &[probe_application::AssertionResult],
        theme: Theme,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        use crate::response_viewer::assertions::{counts, summary};
        let [failed, errors, _, skipped] = counts(assertions);
        let summary_color = if skipped == assertions.len() {
            theme.colors.text.muted
        } else if failed > 0 {
            theme.colors.status.error
        } else if errors > 0 {
            theme.colors.status.warning
        } else {
            theme.colors.status.success
        };
        let mut summary_line = div()
            .id("assertion-summary")
            .debug_selector(|| "assertion-summary".into())
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1))
            .line_height(relative(1.25));
        for (index, part) in summary(assertions).into_iter().enumerate() {
            if index > 0 {
                summary_line =
                    summary_line.child(div().text_color(theme.colors.text.muted).child("·"));
            }
            summary_line = summary_line.child(
                div()
                    .text_color(if index > 0 {
                        theme.colors.text.muted
                    } else {
                        summary_color
                    })
                    .child(part),
            );
        }
        summary_line
    }

    fn render_assertion_result_row(
        &self,
        key: RequestKey,
        result: &probe_application::AssertionResult,
        declaration: Option<&probe_core::Assertion>,
        selected: Option<usize>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        use probe_application::AssertionOutcome;
        let index = result.index;
        let disabled = result.outcome == AssertionOutcome::Disabled;
        let failed = result.outcome == AssertionOutcome::Failed;
        let color = match result.outcome {
            AssertionOutcome::Passed => theme.colors.status.success,
            AssertionOutcome::Failed => theme.colors.status.error,
            AssertionOutcome::Error(_) => theme.colors.status.warning,
            AssertionOutcome::Disabled => theme.colors.text.muted,
        };
        let text_color = if disabled {
            theme.colors.text.muted
        } else {
            theme.colors.text.primary
        };
        let description = declaration
            .and_then(|row| documentation_text(row.description.as_ref()))
            .filter(|text| !text.is_empty());
        let mono = |text: String| {
            div()
                .font_family(theme.typography.monospace_family)
                .min_w(px(0.0))
                .overflow_hidden()
                .text_ellipsis()
                .child(text)
        };
        let mut primary = div()
            .min_h(px(theme.metrics.tree_row_height))
            .px(px(theme.metrics.spacing_1))
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_2))
            .line_height(relative(1.25))
            .child(components::assertion_status_icon(theme, &result.outcome).text_color(color));
        if let Some(description) = description {
            primary = primary.child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(components::truncated_label(description.to_owned()))
                    .child(
                        mono(
                            declaration
                                .map(|row| row.expression.clone())
                                .unwrap_or_default(),
                        )
                        .text_color(theme.colors.text.muted),
                    ),
            );
        } else {
            primary = primary.child(
                mono(
                    declaration
                        .map(|row| row.expression.clone())
                        .unwrap_or_default(),
                )
                .flex_1(),
            );
        }
        primary = primary
            .child(
                div().text_color(theme.colors.text.muted).child(
                    declaration
                        .map(|row| row.operator.clone())
                        .unwrap_or_default(),
                ),
            )
            .child(
                mono(
                    declaration
                        .filter(|row| !matches!(row.operator.as_str(), "isNull" | "isNotNull"))
                        .and_then(|row| row.value.clone())
                        .unwrap_or_default(),
                )
                .flex_1(),
            )
            .when(disabled, |line| line.child(div().child("Skipped")));
        let view = cx.weak_entity();
        let mut row = div()
            .id(("assertion-result", index))
            .debug_selector(move || format!("assertion-result-{index}"))
            .flex()
            .flex_col()
            .line_height(relative(1.25))
            .text_color(text_color)
            .border_l_2()
            .border_color(if failed {
                theme.colors.status.error
            } else {
                transparent_black().into()
            })
            .when(selected == Some(index), |row| {
                row.bg(theme.colors.selection.inactive_background)
            })
            .cursor(CursorStyle::PointingHand)
            .child(primary)
            .on_click(move |event, window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.response_viewer.select_assertion(key, index);
                    view.assertion_results_focus.focus(window, cx);
                    if event.click_count() > 1 {
                        view.jump_to_assertion(key, index, cx);
                    }
                    cx.notify();
                });
            });
        if failed && let Some(actual) = &result.actual {
            row = row.child(
                mono(format!(
                    "got {}",
                    crate::response_viewer::assertions::single_line_actual(actual)
                ))
                .id(("assertion-actual", index))
                .debug_selector(move || format!("assertion-actual-{index}"))
                .pl(px(theme.metrics.spacing_3)),
            );
        }
        if let AssertionOutcome::Error(error) = result.outcome {
            row = row.child(
                div()
                    .id(("assertion-error", index))
                    .debug_selector(move || format!("assertion-error-{index}"))
                    .pl(px(theme.metrics.spacing_3))
                    .text_color(theme.colors.text.secondary)
                    .line_height(relative(1.25))
                    .child(error.to_string()),
            );
        }
        row
    }

    fn handle_assertion_result_key(
        &mut self,
        key: RequestKey,
        event: &gpui_kit::KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        use crate::response_viewer::assertions::ordered;
        let indices = match self.execution.response(key) {
            Some(ResponseState::Complete { assertions, .. }) => ordered(assertions)
                .iter()
                .map(|r| r.index)
                .collect::<Vec<_>>(),
            _ => return,
        };
        let selected = self
            .response_viewer
            .assertion_selection(key)
            .or_else(|| indices.first().copied());
        match event.keystroke.key.as_str() {
            "up" | "down" => self.response_viewer.move_assertion(
                key,
                &indices,
                if event.keystroke.key == "up" { -1 } else { 1 },
            ),
            "enter" => {
                if let Some(index) = selected {
                    self.jump_to_assertion(key, index, cx);
                }
            }
            "c" if if cfg!(target_os = "macos") {
                event.keystroke.modifiers.platform
            } else {
                event.keystroke.modifiers.control
            } =>
            {
                if let Some(index) = selected {
                    self.copy_assertion_result(key, index, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_raw_response_tabs(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let key = self
            .shell
            .active_tab()
            .expect("response tabs require an active request");
        let mut raw_views = Tabs::new("response-raw-view-tabs")
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1));
        for (index, view) in RawBodyView::ALL.iter().copied().enumerate() {
            let view_entity = cx.weak_entity();
            let selected = self.response_viewer.raw_view(key) == view;
            raw_views = raw_views.child(
                components::editor_subtab(
                    theme,
                    ("response-raw-view", index),
                    view.label(),
                    selected,
                    index + 1,
                    RawBodyView::ALL.len(),
                    move |_, _, cx| {
                        let _ = view_entity.update(cx, |app, cx| {
                            app.set_raw_body_view(view, cx);
                        });
                    },
                )
                .debug_selector(move || {
                    format!("response-raw-view-{}", view.label().to_ascii_lowercase())
                }),
            );
        }
        raw_views.into_any_element()
    }

    pub(super) fn render_response_document(
        &self,
        theme: Theme,
        document: Option<&PreparedDocument>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(key) = self.shell.active_tab() else {
            return div().into_any_element();
        };
        if self.response_viewer.tab(key) == ResponseViewerTab::Assertions {
            return self.render_assertion_results(key, theme, cx);
        }
        let Some(document) = document else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.colors.text.muted)
                .child("Preparing response…")
                .into_any_element();
        };

        let mut banners = div()
            .px(px(theme.metrics.spacing_2))
            .pb(px(theme.metrics.spacing_1))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_1));
        let mut has_banner = false;
        if document.file_backed {
            has_banner = true;
            let previous_view = cx.weak_entity();
            let next_view = cx.weak_entity();
            let can_previous = document.can_load_previous_page();
            let can_next = document.can_load_next_page();
            let first_byte = document.page_offset.saturating_add(1);
            let last_byte = document.page_offset.saturating_add(document.page_len);
            banners = banners.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(theme.metrics.spacing_2))
                    .text_color(theme.colors.status.warning)
                    .text_size(px(theme.typography.caption_size))
                    .child(
                        div().flex_1().min_w(px(0.0)).child(format!(
                            "The complete response is retained on disk. Showing bytes {first_byte}–{last_byte} of {}; search covers this page only.",
                            document.total_size
                        )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(theme.metrics.spacing_1))
                            .child(
                                response_page_button(
                                    theme,
                                    "response-page-previous",
                                    "Previous",
                                    !can_previous,
                                    move |_, _, cx| {
                                        let _ = previous_view.update(cx, |view, cx| {
                                            view.load_response_page(
                                                key,
                                                PageDirection::Previous,
                                                cx,
                                            );
                                        });
                                    },
                                ),
                            )
                            .child(
                                response_page_button(
                                    theme,
                                    "response-page-next",
                                    "Next",
                                    !can_next,
                                    move |_, _, cx| {
                                        let _ = next_view.update(cx, |view, cx| {
                                            view.load_response_page(
                                                key,
                                                PageDirection::Next,
                                                cx,
                                            );
                                        });
                                    },
                                ),
                            ),
                    ),
            );
        } else if document.truncated {
            has_banner = true;
            banners = banners.child(
                div()
                    .text_color(theme.colors.status.warning)
                    .text_size(px(theme.typography.caption_size))
                    .child(document.retention_notice.clone().unwrap_or_else(|| {
                        "The response exceeds the in-memory limit and the complete body was not retained."
                            .to_owned()
                    })),
            );
        }
        if let Some(notice) = &document.pretty_notice
            && !matches!(
                self.response_viewer.tab(key),
                ResponseViewerTab::Headers
                    | ResponseViewerTab::Inspect
                    | ResponseViewerTab::Assertions
            )
        {
            has_banner = true;
            banners = banners.child(
                div()
                    .text_color(theme.colors.text.muted)
                    .text_size(px(theme.typography.caption_size))
                    .child(notice.clone()),
            );
        }

        let list = match self.response_viewer.tab(key) {
            ResponseViewerTab::Assertions => self.render_assertion_results(key, theme, cx),
            ResponseViewerTab::Headers => self.render_response_headers(theme, document, cx),
            ResponseViewerTab::Inspect => self.render_response_inspector(theme, key, document, cx),
            ResponseViewerTab::Pretty if document.is_image() => {
                self.render_response_image(theme, key, document)
            }
            ResponseViewerTab::Pretty | ResponseViewerTab::Raw => {
                self.render_response_body(theme, key, document, cx)
            }
        };

        div()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .when(
                self.response_viewer.tab(key) == ResponseViewerTab::Raw,
                |panel| {
                    panel.child(
                        div()
                            .h(px(theme.metrics.control_height))
                            .px(px(theme.metrics.spacing_2))
                            .mb(px(theme.metrics.spacing_1))
                            .flex()
                            .items_center()
                            .child(self.render_raw_response_tabs(theme, cx)),
                    )
                },
            )
            .when(has_banner, |panel| panel.child(banners))
            .child(list)
            .into_any_element()
    }

    fn render_response_image(
        &self,
        theme: Theme,
        key: probe_core::RequestKey,
        document: &PreparedDocument,
    ) -> gpui_kit::AnyElement {
        match document.image_preview.as_ref() {
            Some(ResponseImagePreview::Ready(image)) => {
                let Some(scroll) = self.response_viewer.image_scroll(key) else {
                    return placeholder_message(theme, "Image preview is unavailable.");
                };
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .id("response-image-preview")
                            .debug_selector(|| "response-image-preview".into())
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(scroll)
                            .p(px(theme.metrics.spacing_2))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                img(image.clone())
                                    .id("response-preview-image")
                                    .debug_selector(|| "response-preview-image".into())
                                    .max_w_full()
                                    .flex_none()
                                    .with_loading(move || {
                                        placeholder_message(theme, "Loading image…")
                                    })
                                    .with_fallback(move || {
                                        placeholder_message(
                                            theme,
                                            "The image could not be decoded.",
                                        )
                                    }),
                            ),
                    )
                    .child(
                        Scrollbar::vertical(scroll)
                            .id("response-image-scrollbar")
                            .mode(ScrollbarMode::Always),
                    )
                    .into_any_element()
            }
            Some(ResponseImagePreview::Unavailable(message)) => placeholder_message(theme, message),
            None => placeholder_message(theme, "Image preview is unavailable."),
        }
    }

    pub(super) fn response_tab_content_spacing(theme: Theme) -> gpui_kit::Div {
        div()
            .px(px(theme.metrics.spacing_2))
            .pb(px(theme.metrics.spacing_2))
    }

    pub(super) fn inspect_tab_content_boundary(
        theme: Theme,
        content: impl gpui_kit::IntoElement,
    ) -> gpui_kit::Div {
        div().flex_1().min_h(px(0.0)).flex().flex_col().child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(theme.colors.borders.subtle)
                .child(content),
        )
    }

    pub(super) fn render_response_body(
        &self,
        theme: Theme,
        key: probe_core::RequestKey,
        document: &PreparedDocument,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if document.binary
            && (self.response_viewer.tab(key) == ResponseViewerTab::Pretty
                || self.response_viewer.raw_view(key) == RawBodyView::Text)
        {
            return placeholder_message(theme, "Binary response body cannot be displayed as text.");
        }
        if self.response_viewer.tab(key) == ResponseViewerTab::Raw
            && self.response_viewer.raw_view(key) == RawBodyView::Base64
            && document.base64_pending
        {
            return placeholder_message(theme, "Encoding Base64…");
        }
        if self.response_viewer.tab(key) == ResponseViewerTab::Raw
            && self.response_viewer.raw_view(key) == RawBodyView::Hex
            && document.hex_pending
        {
            return placeholder_message(theme, "Encoding Hex…");
        }
        let text = self.response_viewer.visible_text(key);
        if text.is_empty() {
            return placeholder_message(theme, "Empty response body.");
        }
        let view = cx.weak_entity();
        let body_mouse_view = cx.weak_entity();
        let inspect_view = cx.weak_entity();
        let inspect_ranges = document.inspection_ranges.clone();
        let inspect_context_enabled = self.response_viewer.tab(key) == ResponseViewerTab::Pretty
            && matches!(
                document.syntax,
                ResponseBodySyntax::Json | ResponseBodySyntax::Xml
            )
            && document.pretty_notice.is_none();
        let pretty_reveal = if self.response_viewer.tab(key) == ResponseViewerTab::Pretty {
            self.pretty_reveal.get()
        } else {
            None
        };
        let inspection_reveal = pretty_reveal
            .and_then(|reveal| {
                self.response_viewer
                    .inspection_range_for_selection(key, reveal.selection)
                    .map(|range| (range, reveal.scroll_pending))
            })
            .and_then(|reveal| {
                (self.response_viewer.tab(key) == ResponseViewerTab::Pretty).then_some(reveal)
            });
        if let Some(reveal) = pretty_reveal
            && reveal.scroll_pending
            && self.response_viewer.tab(key) == ResponseViewerTab::Pretty
        {
            self.pretty_reveal.set(Some(PrettyRevealState {
                selection: reveal.selection,
                scroll_pending: false,
            }));
        }
        Self::response_tab_content_spacing(theme)
            .id("response-body")
            .debug_selector(|| "response-body".into())
            .flex_1()
            .min_h(px(0.0))
            .child(components::response_body_input(
                theme,
                "response-body-editor",
                text,
                components::ResponseBodyInputOptions::new(
                    if self.response_viewer.tab(key) == ResponseViewerTab::Pretty {
                        document.syntax.language()
                    } else {
                        ""
                    },
                    move |range, cx| {
                        #[cfg(test)]
                        {
                            let _ = view.update(cx, |this, _| {
                                this.rendered_response_rows = range.len();
                            });
                        }
                        #[cfg(not(test))]
                        {
                            let _ = (&view, range, cx);
                        }
                    },
                    move |_, cx| {
                        let _ = body_mouse_view.update(cx, |view, cx| {
                            if view.response_viewer.tab(key) == ResponseViewerTab::Pretty
                                && view.pretty_reveal.take().is_some()
                            {
                                cx.notify();
                            }
                        });
                    },
                    move |_, offset| {
                        inspect_context_enabled
                            && inspect_ranges
                                .iter()
                                .any(|entry| entry.range.contains(&offset))
                    },
                    move |_, offset, _, cx| {
                        let _ = inspect_view.update(cx, |view, cx| {
                            if view.response_viewer.tab(key) != ResponseViewerTab::Pretty {
                                view.show_toast(
                                    ToastIntent::Info,
                                    "Inspect from the Pretty tab to select a response value.",
                                    cx,
                                );
                            } else if view
                                .response_viewer
                                .document(key)
                                .is_some_and(|document| document.inspection_pending)
                            {
                                view.response_viewer
                                    .set_tab(key, ResponseViewerTab::Inspect);
                                view.show_toast(
                                    ToastIntent::Info,
                                    "Inspection is still running.",
                                    cx,
                                );
                            } else if let Some(selection) = view
                                .response_viewer
                                .select_inspection_at_offset(key, offset)
                            {
                                view.pending_inspector_reveal.set(Some(selection));
                                view.pretty_reveal.set(None);
                            } else {
                                view.show_toast(
                                    ToastIntent::Info,
                                    "No inspected JWT or timestamp found at that value.",
                                    cx,
                                );
                            }
                            cx.notify();
                        });
                    },
                )
                .soft_wrap(
                    self.response_viewer.tab(key) == ResponseViewerTab::Pretty
                        || self.response_viewer.raw_view(key) == RawBodyView::Base64,
                )
                .inspection_reveal(inspection_reveal),
            ))
            .into_any_element()
    }

    pub(super) fn render_response_headers(
        &self,
        theme: Theme,
        document: &PreparedDocument,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if document.headers.is_empty() {
            return placeholder_message(theme, "No response headers");
        }
        let view = cx.weak_entity();
        Self::response_tab_content_spacing(theme)
            .id("response-headers")
            .debug_selector(|| "response-headers".into())
            .flex_1()
            .min_h(px(0.0))
            .child(components::response_headers_input(
                theme,
                "response-headers-editor",
                &document.headers,
                move |range, cx| {
                    #[cfg(test)]
                    {
                        let _ = view.update(cx, |this, _| {
                            this.rendered_response_rows = range.len();
                        });
                    }
                    #[cfg(not(test))]
                    {
                        let _ = (&view, range, cx);
                    }
                },
            ))
            .into_any_element()
    }

    pub(super) fn render_response_inspector(
        &self,
        theme: Theme,
        key: probe_core::RequestKey,
        document: &PreparedDocument,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let rows = inspect_list_rows(document);
        if let Some(selection) = self.pending_inspector_reveal.take()
            && let Some(index) = inspect_row_index(&rows, selection)
        {
            self.inspector_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
        let selected = self.response_viewer.inspection_selection(key);
        let detail = if document.inspection_pending {
            "Inspecting response…".to_owned()
        } else {
            inspection_detail_text(&document.inspection, selected)
        };
        let revealable = selected.is_some_and(|selection| {
            self.response_viewer
                .inspection_range_for_selection(key, selection)
                .is_some()
        }) && document.pretty_notice.is_none();
        let view = cx.weak_entity();
        let row_count = rows.len();
        let rows_for_list = rows.clone();
        let list = uniform_list("response-inspector-list", row_count, {
            cx.processor(move |view, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        let document = view.response_viewer.document(key)?;
                        let selected = view.response_viewer.inspection_selection(key);
                        rows_for_list.get(index).copied().map(|row| {
                            view.render_inspector_list_row(theme, key, row, document, selected, cx)
                        })
                    })
                    .collect::<Vec<_>>()
            })
        })
        .size_full()
        .track_scroll(&self.inspector_scroll);

        if row_count == 0 && !document.inspection_pending {
            return Self::inspect_tab_content_boundary(
                theme,
                div()
                    .id("response-inspector-empty")
                    .debug_selector(|| "response-inspector-empty".into())
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child(document.inspection.skipped.clone().unwrap_or_else(|| {
                        "JWTs and Unix timestamps are detected automatically.".to_owned()
                    })),
            )
            .into_any_element();
        }

        let divider_view = cx.weak_entity();
        let divider =
            components::pane_splitter(theme, "response-inspector-divider", Axis::Horizontal)
                .debug_selector("response-inspector-divider")
                .on_mouse_down(move |event, _, cx| {
                    let _ = divider_view.update(cx, |view, cx| {
                        view.inspector_resize_start =
                            Some((f32::from(event.position.x), view.inspector_list_width));
                        cx.notify();
                    });
                });

        Self::inspect_tab_content_boundary(
            theme,
            div()
                .id("response-inspector")
                .debug_selector(|| "response-inspector".into())
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .child(
                    Self::response_tab_content_spacing(theme)
                        .pr(px(theme.metrics.spacing_2))
                        .w(px(self.inspector_list_width))
                        .flex_none()
                        .min_h(px(0.0))
                        .child(list),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .child(
                            Self::response_tab_content_spacing(theme)
                                .pl(px(theme.metrics.spacing_2))
                                .pt(px(theme.metrics.spacing_2))
                                .size_full()
                                .child(
                                    div()
                                        .relative()
                                        .size_full()
                                        .child(components::response_inspector_input(
                                            theme,
                                            "response-inspector-editor",
                                            detail,
                                            move |range, cx| {
                                                #[cfg(test)]
                                                {
                                                    let _ = view.update(cx, |this, _| {
                                                        this.rendered_response_rows = range.len();
                                                    });
                                                }
                                                #[cfg(not(test))]
                                                {
                                                    let _ = (&view, range, cx);
                                                }
                                            },
                                        ))
                                        .when(revealable, |detail| {
                                            let reveal_view = cx.weak_entity();
                                            detail.child(
                                                div()
                                                    .absolute()
                                                    .top(px(theme.metrics.spacing_3))
                                                    .right(px(theme.metrics.spacing_3))
                                                    .child(components::compact_icon_button(
                                                        theme,
                                                        "response-inspector-reveal-pretty",
                                                        "Reveal in Pretty",
                                                        components::locate_icon(theme),
                                                        move |_, _, cx| {
                                                            let _ = reveal_view.update(
                                                                cx,
                                                                |view, cx| {
                                                                    if let Some(selection) = view
                                                                        .response_viewer
                                                                        .reveal_inspection_in_pretty(
                                                                            key,
                                                                        )
                                                                    {
                                                                        view.pretty_reveal.set(
                                                                            Some(
                                                                                PrettyRevealState {
                                                                                    selection,
                                                                                    scroll_pending: true,
                                                                                },
                                                                            ),
                                                                        );
                                                                    } else {
                                                                        view.show_toast(
                                                                            ToastIntent::Warning,
                                                                            "Pretty source is unavailable.",
                                                                            cx,
                                                                        );
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            );
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        )
                        .child(divider),
                ),
        )
        .into_any_element()
    }

    pub(super) fn render_inspector_list_row(
        &self,
        theme: Theme,
        key: probe_core::RequestKey,
        row: InspectListRow,
        document: &PreparedDocument,
        selected: Option<InspectSelection>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        match row {
            InspectListRow::Group { label, count } => div()
                .w_full()
                .h(px(theme.metrics.tree_row_height))
                .px(px(theme.metrics.spacing_1))
                .flex()
                .items_center()
                .gap(px(theme.metrics.spacing_1))
                .text_size(px(theme.typography.caption_size))
                .text_color(theme.colors.text.secondary)
                .font_weight(FontWeight::SEMIBOLD)
                .child(label)
                .child(
                    div()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme.colors.text.muted)
                        .child(format!("[{count}]")),
                )
                .into_any_element(),
            InspectListRow::Item { selection } => {
                let label = inspect_row_label(document, selection);
                let row_view = cx.weak_entity();
                let is_selected = selected == Some(selection);
                div()
                    .id("response-inspector-list-row")
                    .debug_selector(|| "response-inspector-list-row".into())
                    .w_full()
                    .h(px(theme.metrics.tree_row_height))
                    .px(px(theme.metrics.spacing_1))
                    .flex()
                    .items_center()
                    .rounded(px(theme.metrics.radius_small))
                    .cursor(CursorStyle::PointingHand)
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.text.primary)
                    .bg(if is_selected {
                        theme.colors.selection.inactive_background
                    } else {
                        theme.colors.surfaces.raised
                    })
                    .hover(move |row| {
                        if is_selected {
                            row
                        } else {
                            row.bg(theme.colors.surfaces.editor)
                        }
                    })
                    .child(components::truncated_label(label).min_w(px(0.0)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = row_view.update(cx, |view, cx| {
                            view.response_viewer.select_inspection(key, selection);
                            view.pretty_reveal.set(None);
                            cx.notify();
                        });
                    })
                    .into_any_element()
            }
        }
    }

    pub(super) fn render_editor_response(
        &self,
        theme: Theme,
        window: &Window,
        window_height: f32,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        if let Some(tab) = self.shell.active_overview() {
            return self.render_overview(tab, theme, cx);
        }
        let response_view = cx.weak_entity();
        let horizontal = self.shell.pane_layout == PaneLayout::Horizontal;
        let splitter = components::pane_splitter(
            theme,
            "response-resize-handle",
            if horizontal {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
        )
        .debug_selector("response-resize-handle")
        .on_mouse_down(move |_, _, cx| {
            let _ = response_view.update(cx, |view, cx| {
                view.shell.resizing = Some(ResizePane::Response);
                cx.notify();
            });
        });

        div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .flex()
            .when(horizontal, |work_area| work_area.flex_row())
            .when(!horizontal, |work_area| work_area.flex_col())
            .child(self.render_request_editor(theme, window, cx))
            .child(
                self.render_response_panel(theme, window_height, cx)
                    .relative()
                    .child(splitter),
            )
    }

    pub(in crate::app) fn active_request(&self) -> Option<&Request> {
        let key = self.shell.active_tab()?;
        self.loaded_workspace.as_ref()?.workspace().request(key)
    }

    pub(in crate::app) fn variable_context(
        &self,
        cx: &mut Context<Self>,
    ) -> components::VariableContext {
        if let Some(context) = &self.frame_variable_context {
            return context.clone();
        }
        self.resolve_variable_context(cx)
    }

    pub(super) fn resolve_variable_context(
        &self,
        cx: &mut Context<Self>,
    ) -> components::VariableContext {
        #[cfg(test)]
        self.environment_resolution_count
            .set(self.environment_resolution_count.get() + 1);
        let on_manage_environments = self.loaded_workspace.is_some().then(|| {
            let view = cx.weak_entity();
            Rc::new(move |window: &mut Window, cx: &mut gpui_kit::App| {
                let view = view.clone();
                window.defer(cx, move |window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.open_environment_manager_dialog(window, cx);
                    });
                });
            }) as Rc<dyn Fn(&mut Window, &mut gpui_kit::App)>
        });
        let Some(selected) = self.shell.selected_environment() else {
            return components::VariableContext {
                unavailable_message: "Select an environment to resolve this variable".to_owned(),
                on_manage_environments,
                ..components::VariableContext::default()
            };
        };
        let Some(loaded) = &self.loaded_workspace else {
            return components::VariableContext::default();
        };
        match resolve_environment(loaded.workspace().environments(), selected) {
            Ok(environment) => {
                let sets = self.editor_secret_sets(selected, environment.secrets_without_values());
                let view = cx.weak_entity();
                let selected_environment = selected.to_owned();
                let selected_workspace = self.workspace_path.clone();
                components::VariableContext {
                    values: environment.variables().clone(),
                    secrets: sets.missing,
                    resolved_secrets: sets.resolved,
                    unknown_secrets: sets.unknown,
                    secret_identity_errors: sets.identity_errors,
                    unavailable_message: "Variable value is unavailable".to_owned(),
                    on_change: Some(Rc::new(move |name, value, window, cx| {
                        let name = name.to_owned();
                        let view = view.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = view.update(cx, |view, cx| {
                                view.update_environment_variable(&name, value, window, cx);
                            });
                        });
                    })),
                    on_manage_secret: Some({
                        let view = cx.weak_entity();
                        let selected_environment = selected_environment.clone();
                        let selected_workspace = selected_workspace.clone();
                        Rc::new(
                            move |name: &str,
                                  focus: FocusHandle,
                                  window: &mut Window,
                                  cx: &mut gpui_kit::App| {
                                let name = name.to_owned();
                                let view = view.clone();
                                let selected_environment = selected_environment.clone();
                                let selected_workspace = selected_workspace.clone();
                                window.defer(cx, move |window, cx| {
                                    let _ = view.update(cx, |view, cx| {
                                        if let Some(workspace) = selected_workspace {
                                            view.open_editor_secret_value_dialog(
                                                workspace,
                                                selected_environment,
                                                name,
                                                focus,
                                                window,
                                                cx,
                                            );
                                        }
                                    });
                                });
                            },
                        )
                    }),
                    on_manage_environments,
                    ..components::VariableContext::default()
                }
            }
            Err(error) => components::VariableContext {
                unavailable_message: error.to_string(),
                on_manage_environments,
                ..components::VariableContext::default()
            },
        }
    }
}
