use super::*;
use crate::request_editor::{ASSERTION_OPERATORS, AssertionEdit, AssertionField};

impl ProbeApp {
    pub(super) fn render_assertions_editor(
        &self,
        key: RequestKey,
        request: &Request,
        theme: Theme,
        list_scroll: Option<&components::ListScroll>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let assertions = request.runtime.assertions.as_deref().unwrap_or_default();
        let mut rows = div().flex().flex_col().gap(px(theme.metrics.spacing_2));
        for (index, assertion) in assertions.iter().enumerate() {
            let enabled_view = cx.weak_entity();
            let operator_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            let mut operators = ASSERTION_OPERATORS
                .iter()
                .map(|operator| (operator.to_string(), operator.to_string()))
                .collect::<Vec<_>>();
            if !ASSERTION_OPERATORS.contains(&assertion.operator.as_str()) {
                operators.insert(0, (assertion.operator.clone(), assertion.operator.clone()));
            }
            let enabled = assertion.disabled != Some(true);
            let description =
                documentation_text(assertion.description.as_ref()).unwrap_or_default();
            let controls = components::editor_key_value_row(theme)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(self.assertion_text_field(
                            key,
                            index,
                            AssertionField::Expression,
                            &assertion.expression,
                            "Expression",
                            theme,
                            list_scroll,
                            cx,
                        )),
                )
                .child(components::dropdown(
                    theme,
                    "assertion-operator",
                    "Assertion Operator",
                    Some(assertion.operator.clone()),
                    operators,
                    145.0,
                    move |operator, _, cx| {
                        if let Some(operator) = operator {
                            let _ = operator_view.update(cx, |view, cx| {
                                view.edit_request(
                                    key,
                                    |request| {
                                        AssertionEdit::Text(
                                            index,
                                            AssertionField::Operator,
                                            operator.clone(),
                                        )
                                        .apply(request)
                                    },
                                    cx,
                                );
                            });
                        }
                    },
                ));
            let controls = controls
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(self.assertion_text_field(
                            key,
                            index,
                            AssertionField::Value,
                            assertion.value.as_deref().unwrap_or_default(),
                            if matches!(assertion.operator.as_str(), "isNull" | "isNotNull") {
                                "Not Used"
                            } else {
                                "Expected Value"
                            },
                            theme,
                            list_scroll,
                            cx,
                        )),
                )
                .child(
                    div()
                        .id(("assertion-enabled-control", index))
                        .debug_selector(move || format!("assertion-enabled-{index}"))
                        .child(components::switch(
                            theme,
                            ("assertion-enabled", index),
                            "Enable Assertion",
                            enabled,
                            false,
                            move |enabled, _, cx| {
                                let _ = enabled_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            AssertionEdit::Enabled(index, enabled).apply(request)
                                        },
                                        cx,
                                    );
                                });
                            },
                        )),
                )
                .child(
                    components::remove_row_button(
                        theme,
                        ("assertion-delete", index),
                        "Remove Assertion",
                        move |_, window, cx| {
                            let _ = remove_view.update(cx, |view, cx| {
                                view.edit_request(
                                    key,
                                    |request| AssertionEdit::Remove(index).apply(request),
                                    cx,
                                );
                                view.request_editor.assertion_removed();
                                view.focus_handle.focus(window, cx);
                            });
                        },
                    )
                    .debug_selector(move || format!("assertion-delete-{index}")),
                );
            let row = div()
                .id(("assertion-row", index))
                .debug_selector(move || format!("assertion-row-{index}"))
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_2))
                .child(controls)
                .child(
                    components::editor_key_value_row(theme)
                        .pl(px(theme.metrics.spacing_2))
                        .text_color(theme.colors.text.muted)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(self.assertion_text_field(
                                    key,
                                    index,
                                    AssertionField::Description,
                                    description,
                                    "Description",
                                    theme,
                                    list_scroll,
                                    cx,
                                )),
                        ),
                );
            rows = rows.child(row);
        }
        let view = cx.weak_entity();
        rows.child(components::editor_add_button(
            theme,
            "add-assertion",
            "Add Assertion",
            move |_, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.edit_request(key, |request| AssertionEdit::Add.apply(request), cx)
                });
            },
        ))
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn assertion_text_field(
        &self,
        key: RequestKey,
        index: usize,
        field: AssertionField,
        value: &str,
        placeholder: &'static str,
        theme: Theme,
        list_scroll: Option<&components::ListScroll>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let view = cx.weak_entity();
        let focus = matches!(field, AssertionField::Expression)
            && self.request_editor.take_assertion_focus(key, index);
        let unary = matches!(field, AssertionField::Value)
            && self
                .loaded_workspace
                .as_ref()
                .and_then(|loaded| loaded.workspace().request(key))
                .and_then(|request| request.runtime.assertions.as_ref())
                .and_then(|rows| rows.get(index))
                .is_some_and(|row| matches!(row.operator.as_str(), "isNull" | "isNotNull"));
        div()
            .id((field.id(), index))
            .debug_selector(move || format!("{}-{index}", field.id()))
            .child(
                components::variable_text_input(
                    theme,
                    (field.id(), index),
                    value.to_owned(),
                    placeholder,
                    self.variable_context(cx),
                    move |value, _, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.edit_request(
                                key,
                                |request| {
                                    AssertionEdit::Text(index, field, value.to_string())
                                        .apply(request)
                                },
                                cx,
                            )
                        });
                    },
                )
                .list_scroll(list_scroll)
                .muted(matches!(field, AssertionField::Description))
                .disabled(unary)
                .focus_on_render(focus),
            )
            .into_any_element()
    }
}
