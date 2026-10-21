use super::*;
use crate::request_editor::{ASSERTION_OPERATORS, AssertionEdit, AssertionField};
use gpui_kit::base::{Checkbox, CheckboxState};

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
        if assertions.is_empty() {
            rows = rows.child(
                div()
                    .text_color(theme.colors.text.muted)
                    .child("Add assertions to check the response when this request is sent."),
            );
        }
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
            let mut row = div()
                .id(("assertion-row", index))
                .debug_selector(move || format!("assertion-row-{index}"))
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_1))
                .p(px(theme.metrics.spacing_2))
                .border_1()
                .border_color(theme.colors.borders.subtle)
                .rounded(px(theme.metrics.radius_small));
            let controls = components::editor_key_value_row(theme)
                .child(
                    Checkbox::new("assertion-enabled")
                        .debug_selector(move || format!("assertion-enabled-{index}"))
                        .accessibility_label(format!("Enable assertion {}", index + 1))
                        .checked(enabled)
                        .size(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_1()
                        .border_color(theme.colors.borders.standard)
                        .rounded(px(theme.metrics.radius_small))
                        .cursor_pointer()
                        .text_color(theme.colors.text.primary)
                        .bg(theme.colors.surfaces.raised)
                        .focus_visible(move |control| {
                            control.border_color(theme.colors.borders.focused)
                        })
                        .child(if enabled { "✓" } else { "" })
                        .on_change(move |state, _, _, cx| {
                            let _ = enabled_view.update(cx, |view, cx| {
                                view.edit_request(
                                    key,
                                    |request| {
                                        AssertionEdit::Enabled(
                                            index,
                                            state == CheckboxState::Checked,
                                        )
                                        .apply(request)
                                    },
                                    cx,
                                );
                            });
                        }),
                )
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
                    "Assertion operator",
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
            let mut actions = components::editor_key_value_row(theme);
            for (label, delta, disabled) in [
                ("Move Up", -1, index == 0),
                ("Move Down", 1, index + 1 == assertions.len()),
            ] {
                let view = cx.weak_entity();
                actions = actions.child(
                    components::editor_action_button(
                        theme,
                        (label, index),
                        label,
                        disabled,
                        move |_, window, cx| {
                            let _ = view.update(cx, |view, cx| {
                                view.edit_request(
                                    key,
                                    |request| AssertionEdit::Move(index, delta).apply(request),
                                    cx,
                                );
                                view.focus_handle.focus(window, cx);
                            });
                        },
                    )
                    .debug_selector(move || {
                        format!(
                            "assertion-{}-{index}",
                            if delta < 0 { "up" } else { "down" }
                        )
                    }),
                );
            }
            actions = actions.child(
                components::remove_row_button(
                    theme,
                    ("assertion-delete", index),
                    "Delete Assertion",
                    move |_, window, cx| {
                        let _ = remove_view.update(cx, |view, cx| {
                            view.edit_request(
                                key,
                                |request| AssertionEdit::Remove(index).apply(request),
                                cx,
                            );
                            view.focus_handle.focus(window, cx);
                        });
                    },
                )
                .debug_selector(move || format!("assertion-delete-{index}")),
            );
            row = row
                .child(controls)
                .child(self.assertion_text_field(
                    key,
                    index,
                    AssertionField::Value,
                    assertion.value.as_deref().unwrap_or_default(),
                    if matches!(assertion.operator.as_str(), "isNull" | "isNotNull") {
                        "Expected Value (Unused for This Operator)"
                    } else {
                        "Expected Value"
                    },
                    theme,
                    list_scroll,
                    cx,
                ))
                .child(
                    components::editor_key_value_row(theme)
                        .child(
                            div().flex_1().min_w(px(0.0)).child(
                                self.assertion_text_field(
                                    key,
                                    index,
                                    AssertionField::Description,
                                    documentation_text(assertion.description.as_ref())
                                        .unwrap_or_default(),
                                    "Description (Optional)",
                                    theme,
                                    list_scroll,
                                    cx,
                                ),
                            ),
                        )
                        .child(actions),
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
        div()
            .id((field.id(), index))
            .debug_selector(move || format!("{}-{index}", field.id()))
            .child(
                components::dialog_text_input(
                    theme,
                    (field.id(), index),
                    value.to_owned(),
                    placeholder,
                    false,
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
                    |_, _, _| {},
                )
                .list_scroll(list_scroll),
            )
            .into_any_element()
    }
}
