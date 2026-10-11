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
        let status = state.map_or_else(
            || "Disconnected".to_owned(),
            |s| {
                format!(
                    "{} · {} messages{}{}",
                    s.state.label(),
                    s.sent + s.received,
                    s.close
                        .as_ref()
                        .and_then(|(_, code, _)| *code)
                        .map(|c| format!(" · {c}"))
                        .unwrap_or_default(),
                    if connected {
                        s.started
                            .map(|start| format!(" · {} s", start.elapsed().as_secs()))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                )
            },
        );
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
                    .p(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Messages"))
                    .child(div().text_color(theme.colors.text.secondary).child(status))
                    .child(components::primary_button(
                        theme,
                        "websocket-clear",
                        "Clear",
                        move |_, _, cx| {
                            let _ = clear.update(cx, |view, cx| {
                                if let Some(state) = view.websocket.sessions.get_mut(&key) {
                                    state.clear_timeline();
                                }
                                cx.notify();
                            });
                        },
                    )),
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
                        let label = match &entry.event {
                            SessionEvent::Sent(_) => "↑ Sent",
                            SessionEvent::Received(_) => "↓ Received",
                            SessionEvent::Error(_) => "Error",
                            _ => "Session",
                        };
                        let mut header = div()
                            .flex()
                            .justify_between()
                            .text_color(theme.colors.text.secondary)
                            .child(label)
                            .child(entry.time.clone());
                        if let Some(original) = entry.original.clone() {
                            header = header.child(components::primary_button(
                                theme,
                                "websocket-copy",
                                "Copy",
                                move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        original.clone(),
                                    ))
                                },
                            ));
                        }
                        div()
                            .p(px(theme.metrics.spacing_2))
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
                .flex_1()
                .min_h(px(0.0)),
            );
        } else {
            panel = panel.child(div().flex_1().child("Connect to start a session."));
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
                .p(px(theme.metrics.spacing_2))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
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
                    components::primary_button(theme, "websocket-send", "Send", move |_, _, cx| {
                        let _ = send.update(cx, |view, cx| view.send_websocket_message(key, cx));
                    })
                    .disabled(!connected || state.is_some_and(|s| s.sending)),
                ),
        )
    }
}
