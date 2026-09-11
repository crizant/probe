use super::*;

impl ProbeApp {
    pub(in crate::app) fn render_graphql_query_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let operation = request.selected_graphql().ok().flatten();
        let query = operation
            .and_then(|op| op.query.as_ref())
            .map(String::as_str)
            .unwrap_or("");
        let view = cx.weak_entity();

        div().size_full().flex().flex_col().child(
            components::multiline_text_input(
                theme,
                ("graphql-query", key.slot()),
                query.to_owned(),
                "query { viewer { login } }",
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
            )
            .flex_1()
            .min_h(px(200.0)),
        )
    }

    pub(in crate::app) fn render_graphql_variables_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let operation = request.selected_graphql().ok().flatten();
        let variables = operation
            .and_then(|op| op.variables.as_ref())
            .map(|vars| serde_json::to_string_pretty(vars).unwrap_or_default())
            .unwrap_or_default();
        let view = cx.weak_entity();

        div().size_full().flex().flex_col().child(
            components::multiline_text_input(
                theme,
                ("graphql-variables", key.slot()),
                variables.clone(),
                r#"{ "login": "octocat" }"#,
                self.variable_context(cx),
                move |value, _, cx| {
                    let _ = view.update(cx, |view, cx| {
                        let variables = if value.trim().is_empty() {
                            None
                        } else {
                            serde_json::from_str(&value).ok()
                        };
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
            )
            .flex_1()
            .min_h(px(200.0)),
        )
    }

    pub(in crate::app) fn render_graphql_operation_name_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
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
    }

    pub(in crate::app) fn render_graphql_extensions_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let operation = request.selected_graphql().ok().flatten();
        let extensions = operation
            .and_then(|op| op.extensions.as_ref())
            .map(|ext| serde_json::to_string_pretty(ext).unwrap_or_default())
            .unwrap_or_default();
        let view = cx.weak_entity();

        div().size_full().flex().flex_col().child(
            components::multiline_text_input(
                theme,
                ("graphql-extensions", key.slot()),
                extensions.clone(),
                r#"{ "persistedQuery": { "version": 1 } }"#,
                self.variable_context(cx),
                move |value, _, cx| {
                    let _ = view.update(cx, |view, cx| {
                        let extensions = if value.trim().is_empty() {
                            None
                        } else {
                            serde_json::from_str(&value).ok()
                        };
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
            )
            .flex_1()
            .min_h(px(200.0)),
        )
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
