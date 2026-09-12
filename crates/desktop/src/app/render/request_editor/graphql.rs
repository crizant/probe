use super::*;

impl ProbeApp {
    pub(in crate::app) fn render_graphql_query_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let operation = request.selected_graphql().ok().flatten();
        let query = operation
            .and_then(|op| op.query.as_ref())
            .map(String::as_str)
            .unwrap_or("");
        let view = cx.weak_entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("graphql-query-editor")
                    .debug_selector(|| "graphql-query-editor".into())
                    .flex_1()
                    .min_h(px(0.0))
                    .child(components::body_text_input(
                        theme,
                        ("graphql-query", key.slot()),
                        query.to_owned(),
                        components::BodySyntax::Plain,
                        self.variable_context(cx),
                        move |value, _, cx| {
                            let _ = view.update(cx, |view, cx| {
                                view.edit_graphql_request(
                                    key,
                                    |_| probe_core::GraphqlUpdate {
                                        query: Some(value.to_string()),
                                        ..probe_core::GraphqlUpdate::default()
                                    },
                                    cx,
                                );
                            });
                        },
                    )),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_graphql_variables_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let operation = request.selected_graphql().ok().flatten();
        let variables = operation
            .and_then(|op| op.variables.as_ref())
            .map(|vars| serde_json::to_string_pretty(vars).unwrap_or_default())
            .unwrap_or_default();
        let view = cx.weak_entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("graphql-variables-editor")
                    .debug_selector(|| "graphql-variables-editor".into())
                    .flex_1()
                    .min_h(px(0.0))
                    .child(components::body_text_input(
                        theme,
                        ("graphql-variables", key.slot()),
                        variables,
                        components::BodySyntax::Json,
                        self.variable_context(cx),
                        move |value, _, cx| {
                            let Some(variables) = parse_optional_json_object(&value) else {
                                return;
                            };
                            let _ = view.update(cx, |view, cx| {
                                view.edit_graphql_request(
                                    key,
                                    |_| probe_core::GraphqlUpdate {
                                        variables: Some(variables),
                                        ..probe_core::GraphqlUpdate::default()
                                    },
                                    cx,
                                );
                            });
                        },
                    )),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_graphql_operation_name_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let operation = request.selected_graphql().ok().flatten();
        let operation_name = operation
            .and_then(|op| op.operation_name.as_ref())
            .map(String::as_str)
            .unwrap_or("");
        let view = cx.weak_entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2))
            .child(
                div()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.text.secondary)
                    .child("Operation name (optional)"),
            )
            .child(components::url_text_input(
                theme,
                ("graphql-operation-name", key.slot()),
                operation_name.to_owned(),
                "MyQuery",
                self.variable_context(cx),
                move |value, _, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.edit_graphql_request(
                            key,
                            |_| probe_core::GraphqlUpdate {
                                operation_name: Some(if value.trim().is_empty() {
                                    None
                                } else {
                                    Some(value.to_string())
                                }),
                                ..probe_core::GraphqlUpdate::default()
                            },
                            cx,
                        );
                    });
                },
            ))
            .into_any_element()
    }

    pub(in crate::app) fn render_graphql_extensions_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let operation = request.selected_graphql().ok().flatten();
        let extensions = operation
            .and_then(|op| op.extensions.as_ref())
            .map(|ext| serde_json::to_string_pretty(ext).unwrap_or_default())
            .unwrap_or_default();
        let view = cx.weak_entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("graphql-extensions-editor")
                    .debug_selector(|| "graphql-extensions-editor".into())
                    .flex_1()
                    .min_h(px(0.0))
                    .child(components::body_text_input(
                        theme,
                        ("graphql-extensions", key.slot()),
                        extensions,
                        components::BodySyntax::Json,
                        self.variable_context(cx),
                        move |value, _, cx| {
                            let Some(extensions) = parse_optional_json_object(&value) else {
                                return;
                            };
                            let _ = view.update(cx, |view, cx| {
                                view.edit_graphql_request(
                                    key,
                                    |_| probe_core::GraphqlUpdate {
                                        extensions: Some(extensions),
                                        ..probe_core::GraphqlUpdate::default()
                                    },
                                    cx,
                                );
                            });
                        },
                    )),
            )
            .into_any_element()
    }

    pub(in crate::app) fn edit_graphql_request(
        &mut self,
        key: RequestKey,
        update_fn: impl FnOnce(&HttpRequest) -> probe_core::GraphqlUpdate,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self
            .loaded_workspace
            .as_mut()
            .and_then(|loaded| loaded.request_mut(key))
        else {
            return;
        };
        let update = update_fn(request);
        if let Err(error) = request.apply_graphql_update(&update) {
            eprintln!("Failed to apply GraphQL update: {error}");
            return;
        }
        self.persistence.edited(key);
        cx.notify();
    }
}

/// Parses Variables/Extensions draft text.
///
/// - empty → `Some(None)` (clear)
/// - valid JSON object → `Some(Some(map))`
/// - non-empty invalid JSON → `None` (leave the stored value alone)
fn parse_optional_json_object(
    value: &str,
) -> Option<Option<serde_json::Map<String, serde_json::Value>>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(None);
    }
    serde_json::from_str(trimmed).ok().map(Some)
}

#[cfg(test)]
mod tests {
    use super::parse_optional_json_object;

    #[test]
    fn empty_json_draft_clears_the_field() {
        assert_eq!(parse_optional_json_object(""), Some(None));
        assert_eq!(parse_optional_json_object("   "), Some(None));
    }

    #[test]
    fn valid_json_object_is_applied() {
        let parsed = parse_optional_json_object(r#"{"id": 1}"#).unwrap().unwrap();
        assert_eq!(parsed.get("id").and_then(|v| v.as_i64()), Some(1));
    }

    #[test]
    fn invalid_non_empty_json_is_ignored() {
        assert_eq!(parse_optional_json_object("{"), None);
        assert_eq!(parse_optional_json_object("not-json"), None);
        assert_eq!(parse_optional_json_object("[1,2]"), None);
    }
}
