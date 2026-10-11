use super::*;
use crate::websocket::ConnectionState;
use probe_application::SessionEvent;
impl ProbeApp {
    pub(in crate::app) fn render_websocket_panel(
        &self,
        theme: Theme,
        height: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let key = self.shell.active_tab().unwrap();
        let state = self.websocket.sessions.get(&key);
        let connected = state.is_some_and(|s| s.state == ConnectionState::Connected);
        let connection = state.map(|s| s.state);
        let status_label = connection.map_or("Disconnected", ConnectionState::label);
        let status_color = match connection {
            Some(ConnectionState::Connected) => theme.colors.status.success,
            Some(ConnectionState::Connecting | ConnectionState::Closing) => {
                theme.colors.status.informational
            }
            Some(ConnectionState::Failed) => theme.colors.status.error,
            Some(ConnectionState::Closed) | None => theme.colors.text.secondary,
        };
        let clear = cx.weak_entity();
        let draft_view = cx.weak_entity();
        let send = cx.weak_entity();
        let mut panel = div()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.colors.surfaces.raised)
            .when(self.shell.pane_layout == PaneLayout::Vertical, |p| {
                p.h(px(self.shell.response_height_for_window(height)))
                    .w_full()
            })
            .when(self.shell.pane_layout == PaneLayout::Horizontal, |p| {
                p.w(px(self.shell.response_width)).h_full()
            })
            .child(
                div()
                    .py(px(theme.metrics.spacing_1))
                    .px(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_1))
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Messages"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_end()
                            .gap(px(theme.metrics.spacing_1))
                            .child(
                                div()
                                    .debug_selector(|| "websocket-connection-status".into())
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(theme.metrics.spacing_1))
                                    .text_size(px(theme.typography.caption_size))
                                    .text_color(status_color)
                                    .child(
                                        div()
                                            .flex_none()
                                            .size(px(theme.metrics.spacing_2))
                                            .rounded_full()
                                            .bg(status_color),
                                    )
                                    .child(status_label),
                            )
                            .when_some(state, |header, state| {
                                let elapsed = if connected {
                                    state
                                        .started
                                        .map(|start| format!(" • {} s", start.elapsed().as_secs()))
                                        .unwrap_or_default()
                                } else {
                                    String::new()
                                };
                                let close = state
                                    .close
                                    .as_ref()
                                    .and_then(|(_, code, _)| *code)
                                    .map(|code| format!(" • {code}"))
                                    .unwrap_or_default();
                                header.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(theme.typography.caption_size))
                                        .text_color(theme.colors.text.muted)
                                        .font_features(gpui::FontFeatures(std::sync::Arc::new(
                                            vec![("tnum".into(), 1)],
                                        )))
                                        .child(format!(
                                            "• {} messages{elapsed}{close}",
                                            state.sent + state.received
                                        )),
                                )
                            }),
                    )
                    .child(
                        components::icon_button(
                            theme,
                            "websocket-clear",
                            "Clear messages",
                            components::clear_icon(theme),
                            move |_, _, cx| {
                                let _ = clear.update(cx, |view, cx| {
                                    if let Some(state) = view.websocket.sessions.get_mut(&key) {
                                        state.clear_timeline();
                                    }
                                    cx.notify();
                                });
                            },
                        )
                        .flex_none(),
                    ),
            );
        if let Some(state) = state {
            if state.evicted {
                panel = panel.child(
                    div()
                        .px(px(theme.metrics.spacing_2))
                        .text_color(theme.colors.text.muted)
                        .child("Older session messages were cleared to limit memory usage."),
                );
            }
            let view = cx.weak_entity();
            panel = panel.child(
                div()
                    .id("websocket-messages-list")
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        gpui::list(state.list.clone(), move |index, _, cx| {
                            view.update(cx, |view, _| {
                                let Some(entry) = view
                                    .websocket
                                    .sessions
                                    .get(&key)
                                    .and_then(|s| s.timeline.get(index))
                                else {
                                    return div().into_any_element();
                                };
                                let (label, direction) = match &entry.event {
                                    SessionEvent::Sent(_) => {
                                        ("Sent", Some(("↑", theme.colors.status.informational)))
                                    }
                                    SessionEvent::Received(_) => {
                                        ("Received", Some(("↓", theme.colors.status.success)))
                                    }
                                    SessionEvent::Error(_) => ("Error", None),
                                    _ => ("Session", None),
                                };
                                let mut header = div()
                                    .flex()
                                    .items_center()
                                    .gap(px(theme.metrics.spacing_2))
                                    .text_size(px(theme.typography.caption_size))
                                    .text_color(theme.colors.text.secondary)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(theme.metrics.spacing_1))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .when_some(direction, |label, (arrow, color)| {
                                                label.child(div().text_color(color).child(arrow))
                                            })
                                            .child(label),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_color(theme.colors.text.muted)
                                            .child(entry.time.clone()),
                                    );
                                if let Some(original) = entry.original.clone() {
                                    header = header.child(
                                        components::editor_action_button(
                                            theme,
                                            ("websocket-copy", index),
                                            "Copy",
                                            false,
                                            move |_, _, cx| {
                                                cx.write_to_clipboard(
                                                    gpui::ClipboardItem::new_string(
                                                        original.clone(),
                                                    ),
                                                )
                                            },
                                        )
                                        .h(px(
                                            theme.metrics.control_height - theme.metrics.spacing_2
                                        )),
                                    );
                                }
                                div()
                                    .w_full()
                                    .px(px(theme.metrics.spacing_2))
                                    .py(px(theme.metrics.spacing_1))
                                    .border_b_1()
                                    .border_color(theme.colors.borders.subtle)
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme.metrics.spacing_1))
                                    .child(header)
                                    .child(
                                        div()
                                            .font_family(theme.typography.monospace_family)
                                            .child(entry.display.clone()),
                                    )
                                    .into_any_element()
                            })
                            .unwrap_or_else(|_| div().into_any_element())
                        })
                        .size_full(),
                    )
                    .child(
                        Scrollbar::vertical(&state.list)
                            .id("websocket-messages-scrollbar")
                            .mode(ScrollbarMode::Scrolling),
                    ),
            );
        } else {
            panel = panel.child(
                div()
                    .flex_1()
                    .p(px(theme.metrics.spacing_2))
                    .text_color(theme.colors.text.muted)
                    .child("Connect to start a session."),
            );
        }
        panel.child(
            div()
                .id("websocket-composer")
                .key_context("WebSocketComposer")
                .on_action(cx.listener(move |view, _: &SendWebSocketMessage, _, cx| {
                    view.send_websocket_message(key, cx)
                }))
                .h(px(100.0))
                .flex_none()
                .flex()
                .gap(px(theme.metrics.spacing_1))
                .border_t_1()
                .border_color(theme.colors.borders.subtle)
                .p(px(theme.metrics.spacing_2))
                .child(
                    div()
                        .flex_1()
                        .self_stretch()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .child(components::websocket_composer(
                            theme,
                            ("websocket-draft", key.slot()).into(),
                            state.map(|s| s.draft.clone()).unwrap_or_default(),
                            connected,
                            move |value, _, cx| {
                                let _ = draft_view.update(cx, |view, cx| {
                                    if let Some(state) = view.websocket.sessions.get_mut(&key) {
                                        state.draft = value.to_string();
                                    }
                                    cx.notify();
                                });
                            },
                        )),
                )
                .child(
                    components::primary_button_with_disabled(
                        theme,
                        "websocket-send",
                        "Send",
                        !connected || state.is_some_and(|s| s.sending),
                        move |_, _, cx| {
                            let _ =
                                send.update(cx, |view, cx| view.send_websocket_message(key, cx));
                        },
                    )
                    .self_start()
                    .tooltip(move |window, cx| {
                        let shortcut = components::shortcut_label_for_action_in_context(
                            window,
                            &SendWebSocketMessage,
                            "WebSocketComposer",
                        );
                        let label = shortcut.map_or_else(
                            || "Send message".to_owned(),
                            |shortcut| format!("Send message ({})", shortcut.replace('⏎', "↵")),
                        );
                        cx.new(|_| components::ActionTooltip {
                            id: "websocket-send-tooltip",
                            theme,
                            label,
                        })
                        .into()
                    }),
                ),
        )
    }
}
