use super::*;
use probe_core::{WebSocketMessageKind as Kind, WebSocketMessageSet};
fn message_tab_choices(
    variants: bool,
) -> impl Iterator<Item = (&'static str, Option<Kind>, usize, usize)> {
    let choices = [
        ("None", None),
        ("JSON", Some(Kind::Json)),
        ("Text", Some(Kind::Text)),
        ("XML", Some(Kind::Xml)),
        ("Binary", Some(Kind::Binary)),
    ]
    .into_iter()
    .filter(|(_, kind)| !variants || kind.is_some())
    .collect::<Vec<_>>();
    let size = choices.len();
    choices
        .into_iter()
        .enumerate()
        .map(move |(index, (label, kind))| (label, kind, index + 1, size))
}

impl ProbeApp {
    pub(in crate::app) fn render_websocket_message_editor(
        &self,
        key: RequestKey,
        request: &Request,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Ok(selected) = request.selected_websocket_message() else {
            return div().debug_selector(|| "websocket-invalid-selection".into()).text_color(theme.colors.text.secondary).child("Message variants require exactly one selected variant. The saved variants have not been changed.").into_any_element();
        };
        let variants = matches!(
            request.websocket_message(),
            Some(WebSocketMessageSet::Variants(_))
        );
        let mut panel = div()
            .size_full()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2));
        if let Some(WebSocketMessageSet::Variants(entries)) = request.websocket_message()
            && let Some(variant) = entries.iter().find(|v| v.selected)
        {
            panel = panel.child(
                div()
                    .text_color(theme.colors.text.secondary)
                    .child(format!("Selected message: {}", variant.title)),
            );
        }
        let mut tabs = div().flex().flex_wrap().gap(px(theme.metrics.spacing_1));
        for (label, kind, position, size) in message_tab_choices(variants) {
            let view = cx.weak_entity();
            let active = selected.map(|m| m.kind) == kind;
            tabs = tabs.child(
                components::editor_subtab(
                    theme,
                    ("websocket-message-kind", position - 1),
                    label,
                    active,
                    position,
                    size,
                    move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.edit_request(
                                key,
                                |request| {
                                    crate::request_editor::set_websocket_message(
                                        request, kind, None,
                                    )
                                },
                                cx,
                            )
                        });
                    },
                )
                .debug_selector(move || {
                    format!(
                        "websocket-kind-{label}{}",
                        if active { "-selected" } else { "" }
                    )
                }),
            );
        }
        panel = panel.child(tabs);
        if let Some(message) = selected {
            if message.kind == Kind::Binary {
                panel = panel.child(
                    div()
                        .debug_selector(|| "websocket-binary-warning".into())
                        .p(px(theme.metrics.spacing_3))
                        .rounded(px(theme.metrics.radius_small))
                        .bg(theme.colors.surfaces.window)
                        .text_color(theme.colors.text.secondary)
                        .child("Configured binary messages can be stored, but Probe cannot send them yet."),
                );
            }
            let view = cx.weak_entity();
            let kind = message.kind;
            panel = panel.child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(components::body_text_input(
                        theme,
                        ("websocket-message", key.slot()),
                        message.data.clone(),
                        match kind {
                            Kind::Json => components::BodySyntax::Json,
                            Kind::Xml => components::BodySyntax::Xml,
                            _ => components::BodySyntax::Plain,
                        },
                        self.variable_context(cx),
                        move |value, _, cx| {
                            let _ = view.update(cx, |view, cx| {
                                view.edit_request(
                                    key,
                                    |request| {
                                        crate::request_editor::set_websocket_message(
                                            request,
                                            Some(kind),
                                            Some(value.to_string()),
                                        )
                                    },
                                    cx,
                                )
                            });
                        },
                    )),
            );
        } else {
            panel = panel.child(
                div()
                    .debug_selector(|| "websocket-no-initial-message".into())
                    .text_color(theme.colors.text.muted)
                    .child("No initial message"),
            );
        }
        panel.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::message_tab_choices;

    #[test]
    fn message_tab_aria_metadata_uses_only_visible_choices() {
        let metadata = |variants| {
            message_tab_choices(variants)
                .map(|(label, _, position, size)| (label, position, size))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            metadata(true),
            vec![
                ("JSON", 1, 4),
                ("Text", 2, 4),
                ("XML", 3, 4),
                ("Binary", 4, 4)
            ]
        );
        assert_eq!(
            metadata(false),
            vec![
                ("None", 1, 5),
                ("JSON", 2, 5),
                ("Text", 3, 5),
                ("XML", 4, 5),
                ("Binary", 5, 5)
            ]
        );
    }
}
