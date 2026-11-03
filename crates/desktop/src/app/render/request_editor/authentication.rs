use super::*;

impl ProbeApp {
    pub(super) fn render_authentication_editor(
        &self,
        key: RequestKey,
        request: &Request,
        theme: Theme,
        list_scroll: Option<&components::ListScroll>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let active = request
            .authentication
            .as_ref()
            .map(|auth| auth_label(&auth.kind));
        let choices = [
            ("None", None),
            ("Basic", Some(AuthenticationKind::Basic)),
            ("Bearer", Some(AuthenticationKind::Bearer)),
            ("API Key", Some(AuthenticationKind::ApiKey)),
        ];
        let choice_count = choices.len();
        let mut kind_buttons = div().flex().flex_wrap().gap(px(theme.metrics.spacing_1));
        for (index, (label, kind)) in choices.into_iter().enumerate() {
            let kind_view = cx.weak_entity();
            kind_buttons = kind_buttons.child(components::editor_subtab(
                theme,
                ("authentication-kind", index),
                label,
                active == Some(label) || (active.is_none() && label == "None"),
                index + 1,
                choice_count,
                move |_, _, cx| {
                    let kind = kind.clone();
                    let _ = kind_view.update(cx, |view, cx| {
                        view.edit_request(key, |request| set_authentication(request, kind), cx);
                    });
                },
            ));
        }

        let mut editor = div()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2))
            .child(kind_buttons);
        if let Some(authentication) = &request.authentication {
            let fields: &[(&str, &str)] = match authentication.kind {
                AuthenticationKind::Basic => &[("username", "Username"), ("password", "Password")],
                AuthenticationKind::Bearer => &[("token", "Token")],
                AuthenticationKind::ApiKey => &[("key", "Key"), ("value", "Value")],
                _ => &[],
            };
            for &(property, label) in fields {
                let value_view = cx.weak_entity();
                let value = authentication
                    .properties
                    .get(property)
                    .map(auth_value)
                    .unwrap_or_default();
                editor = editor.child(
                    div()
                        .id(property)
                        .debug_selector(move || format!("authentication-{property}"))
                        .flex()
                        .flex_col()
                        .gap(px(theme.metrics.spacing_1))
                        .child(label)
                        .child(
                            components::variable_text_input(
                                theme,
                                SharedString::from(format!("authentication-{key:?}-{property}")),
                                value,
                                label,
                                self.variable_context(cx),
                                move |value, _, input_cx| {
                                    let _ = value_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                set_auth_property(
                                                    request,
                                                    property.to_owned(),
                                                    value.to_string(),
                                                )
                                            },
                                            cx,
                                        );
                                    });
                                },
                            )
                            .password(property == "password")
                            .list_scroll(list_scroll),
                        ),
                );
            }
            if authentication.kind == AuthenticationKind::ApiKey {
                let placement = authentication.properties.get("placement").map(auth_value);
                let mut options = vec![
                    ("header".to_owned(), "Header".to_owned()),
                    ("query".to_owned(), "Query Params".to_owned()),
                ];
                if let Some(value) = &placement
                    && !matches!(value.as_str(), "header" | "query")
                {
                    options.push((value.clone(), value.clone()));
                }
                let placement_view = cx.weak_entity();
                editor = editor.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(theme.metrics.spacing_1))
                        .child("Add to")
                        .child(
                            components::dropdown(
                                theme,
                                "authentication-placement",
                                "Add to",
                                placement,
                                options,
                                180.0,
                                move |placement, _, cx| {
                                    if let Some(placement) = placement {
                                        let _ = placement_view.update(cx, |view, cx| {
                                            view.edit_request(
                                                key,
                                                |request| {
                                                    set_auth_property(
                                                        request,
                                                        "placement".to_owned(),
                                                        placement.clone(),
                                                    )
                                                },
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )
                            .placeholder("Not set"),
                        ),
                );
            }
            if fields.is_empty() {
                editor = editor.child(div().text_color(theme.colors.text.muted).child(format!(
                    "{} authentication is not supported for execution. Imported configuration is preserved.",
                    auth_label(&authentication.kind),
                )));
            }
        } else {
            editor = editor.child(
                div()
                    .text_color(theme.colors.text.muted)
                    .child("This request does not use authentication."),
            );
        }
        editor.into_any_element()
    }
}
