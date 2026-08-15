//! Probe-styled compositions over headless base-gpui behavior.

use std::{collections::BTreeMap, ops::Range, rc::Rc, sync::Arc, time::Duration};

use base_gpui::{
    button::ButtonRoot,
    primitives::input::{Input, InputRuntime},
    select::{
        SelectIcon, SelectItem, SelectItemIndicator, SelectItemText, SelectList, SelectPopup,
        SelectPortal, SelectPositioner, SelectRoot, SelectTrigger, SelectValue,
    },
    switch::{SwitchRoot, SwitchThumb},
    toggle::Toggle,
    toggle_group::ToggleGroup,
};
use gpui::{
    App, AppContext as _, ClickEvent, Context, ElementId, HighlightStyle, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Window, div,
    prelude::FluentBuilder as _, px, transparent_black,
};

use crate::theme::Theme;

use crate::shell::PaneLayout;

#[derive(Clone, Debug, Default)]
pub(crate) struct VariableContext {
    pub(crate) values: BTreeMap<String, String>,
    pub(crate) unavailable_message: String,
}

struct VariableTooltip {
    theme: Theme,
    rows: Vec<(String, String)>,
}

impl Render for VariableTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div()
            .debug_selector(|| "variable-input-tooltip-popup".into())
            .max_w(px(360.0))
            .px(px(self.theme.metrics.spacing_2))
            .py(px(self.theme.metrics.spacing_1))
            .flex()
            .flex_col()
            .gap(px(self.theme.metrics.spacing_1))
            .rounded(px(self.theme.metrics.radius_small))
            .bg(self.theme.colors.surfaces.overlay)
            .border_1()
            .border_color(self.theme.colors.borders.standard)
            .font_family(self.theme.typography.monospace_family)
            .text_size(px(self.theme.typography.caption_size))
            .text_color(self.theme.colors.text.primary);
        for (name, value) in &self.rows {
            content = content.child(
                div()
                    .flex()
                    .gap(px(self.theme.metrics.spacing_2))
                    .child(
                        div()
                            .text_color(self.theme.colors.syntax.string)
                            .child(format!("{{{{{name}}}}}")),
                    )
                    .child(value.clone()),
            );
        }
        content
    }
}

pub fn primary_button(
    theme: Theme,
    id: impl Into<ElementId>,
    label: impl Into<String>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    ButtonRoot::new()
        .id(id)
        .h(px(theme.metrics.control_height))
        .px(px(theme.metrics.spacing_3))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme.metrics.radius_small))
        .font_family(theme.typography.interface_family)
        .text_size(px(theme.typography.body_size))
        .on_click(on_click)
        .style_with_state(move |state, button| {
            let background = if state.disabled {
                theme.colors.actions.disabled
            } else {
                theme.colors.actions.accent
            };
            let foreground = if state.disabled {
                theme.colors.actions.disabled_foreground
            } else {
                theme.colors.text.inverse
            };

            button
                .bg(background)
                .text_color(foreground)
                .border_1()
                .border_color(if state.focused {
                    theme.colors.borders.focused
                } else {
                    background
                })
                .when(!state.disabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |button| button.bg(theme.colors.actions.hover))
                })
        })
        .child(label)
}

pub(crate) fn variable_text_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    variables: VariableContext,
    on_value_change: impl Fn(SharedString, &mut Window, &mut gpui::Context<InputRuntime>) + 'static,
) -> gpui::AnyElement {
    text_input_with_variables(theme, id, value, placeholder, variables, on_value_change)
}

fn text_input_with_variables(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    variables: VariableContext,
    on_value_change: impl Fn(SharedString, &mut Window, &mut gpui::Context<InputRuntime>) + 'static,
) -> gpui::AnyElement {
    let id = id.into();
    let tooltip_id =
        ElementId::NamedChild(Arc::new(id.clone()), SharedString::from("variable-tooltip"));
    let value = value.into();
    let input = Input::new()
        .id(id)
        .value(value.clone())
        .placeholder(placeholder)
        .on_value_change_with_context(on_value_change)
        .h(px(theme.metrics.control_height))
        .min_w(px(0.0))
        .px(px(theme.metrics.spacing_2))
        .flex()
        .items_center()
        .rounded(px(theme.metrics.radius_small))
        .font_family(theme.typography.monospace_family)
        .text_size(px(theme.typography.body_size))
        .text_color(theme.colors.text.primary)
        .style_with_state(move |state, input| {
            input
                .bg(theme.colors.surfaces.window)
                .border_1()
                .border_color(if state.focused {
                    theme.colors.borders.focused
                } else {
                    theme.colors.borders.standard
                })
        });
    variable_input_overlay(theme, tooltip_id, input, value, variables, false)
}

pub(crate) fn editor_button(
    theme: Theme,
    id: impl Into<ElementId>,
    label: impl Into<String>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    ButtonRoot::new()
        .id(id)
        .h(px(30.0))
        .px(px(theme.metrics.spacing_2))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme.metrics.radius_small))
        .text_size(px(theme.typography.caption_size))
        .text_color(if selected {
            theme.colors.selection.active_foreground
        } else {
            theme.colors.text.secondary
        })
        .on_click(on_click)
        .style_with_state(move |state, button| {
            button
                .bg(if selected {
                    theme.colors.selection.active_background
                } else {
                    theme.colors.surfaces.window
                })
                .border_1()
                .border_color(if state.focused {
                    theme.colors.borders.focused
                } else {
                    theme.colors.borders.standard
                })
                .cursor_pointer()
                .when(!selected, |button| {
                    button.hover(move |button| button.bg(theme.colors.surfaces.raised))
                })
        })
        .child(label)
}

pub(crate) fn dropdown<T: Clone + Eq + 'static>(
    theme: Theme,
    id: &'static str,
    aria_label: &'static str,
    value: Option<T>,
    options: Vec<(T, String)>,
    width: f32,
    on_value_change: impl Fn(Option<&T>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut list = SelectList::new().flex().flex_col().gap(px(2.0));
    for (index, (value, label)) in options.into_iter().enumerate() {
        list = list.child(
            SelectItem::new()
                .id(format!("{id}-item-{index}"))
                .value(value)
                .label(label.clone())
                .h(px(30.0))
                .px(px(theme.metrics.spacing_2))
                .flex()
                .items_center()
                .gap(px(theme.metrics.spacing_2))
                .rounded(px(theme.metrics.radius_small))
                .text_color(theme.colors.text.primary)
                .style_with_state(move |state, item| {
                    item.when(state.highlighted, |item| {
                        item.bg(theme.colors.surfaces.sidebar)
                    })
                })
                .child(
                    SelectItemIndicator::new()
                        .keep_mounted(true)
                        .w(px(14.0))
                        .style_with_state(|state, indicator| {
                            if state.selected {
                                indicator
                            } else {
                                indicator.invisible()
                            }
                        })
                        .child("✓"),
                )
                .child(SelectItemText::new().text(label)),
        );
    }

    SelectRoot::<T>::new()
        .id(id)
        .value(value)
        .on_value_change(move |value, _, window, cx| on_value_change(value, window, cx))
        .w(px(width))
        .child(
            SelectTrigger::new()
                .id(format!("{id}-trigger"))
                .aria_label(aria_label)
                .w_full()
                .h(px(30.0))
                .px(px(theme.metrics.spacing_2))
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(theme.metrics.radius_small))
                .bg(theme.colors.surfaces.window)
                .border_1()
                .border_color(theme.colors.borders.standard)
                .text_color(theme.colors.text.primary)
                .style_with_state(move |state, trigger| {
                    trigger
                        .debug_selector(move || format!("{id}-trigger"))
                        .border_color(if state.root.focused {
                            theme.colors.borders.focused
                        } else {
                            theme.colors.borders.standard
                        })
                        .when(!state.root.open, |trigger| {
                            trigger.hover(move |trigger| trigger.bg(theme.colors.surfaces.raised))
                        })
                })
                .child(SelectValue::new().placeholder("None"))
                .child(
                    SelectIcon::new()
                        .text_color(theme.colors.text.muted)
                        .child("▾"),
                ),
        )
        .child(
            SelectPortal::<T>::new().child(
                SelectPositioner::new()
                    .side_offset(px(theme.metrics.spacing_1))
                    .child(
                        SelectPopup::new()
                            .w(px(width.max(160.0)))
                            .p(px(theme.metrics.spacing_1))
                            .rounded(px(theme.metrics.radius_medium))
                            .bg(theme.colors.surfaces.overlay)
                            .border_1()
                            .border_color(theme.colors.borders.standard)
                            .child(list),
                    ),
            ),
        )
}

pub(crate) fn body_text_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    variables: VariableContext,
    on_value_change: impl Fn(SharedString, &mut Window, &mut gpui::Context<InputRuntime>) + 'static,
) -> gpui::AnyElement {
    let id = id.into();
    let tooltip_id =
        ElementId::NamedChild(Arc::new(id.clone()), SharedString::from("variable-tooltip"));
    let value = value.into();
    let input = Input::new()
        .id(id)
        .value(value.clone())
        .placeholder("Body content")
        .on_value_change_with_context(on_value_change)
        .size_full()
        .min_h(px(120.0))
        .p(px(theme.metrics.spacing_3))
        .flex()
        .items_start()
        .rounded(px(theme.metrics.radius_small))
        .font_family(theme.typography.monospace_family)
        .text_size(px(theme.typography.body_size))
        .text_color(theme.colors.text.primary)
        .style_with_state(move |state, input| {
            input
                .bg(theme.colors.surfaces.window)
                .border_1()
                .border_color(if state.focused {
                    theme.colors.borders.focused
                } else {
                    theme.colors.borders.standard
                })
        });
    variable_input_overlay(theme, tooltip_id, input, value, variables, true)
}

fn variable_input_overlay(
    theme: Theme,
    tooltip_id: ElementId,
    input: Input,
    value: SharedString,
    variables: VariableContext,
    body: bool,
) -> gpui::AnyElement {
    let ranges = variable_ranges(&value);
    let wrapper = div()
        .id(tooltip_id)
        .relative()
        .debug_selector(|| "variable-input-tooltip-trigger".into())
        .when(body, |wrapper| wrapper.size_full())
        .when(!body, |wrapper| wrapper.w_full());
    if ranges.is_empty() {
        return wrapper.child(input).into_any_element();
    }

    let highlights = ranges.iter().map(|(range, _)| {
        (
            range.clone(),
            HighlightStyle {
                color: Some(theme.colors.syntax.string.into()),
                ..HighlightStyle::default()
            },
        )
    });
    let wrapper = wrapper.child(input).child(
        div()
            .absolute()
            .top(px(0.0))
            .bottom(px(0.0))
            .left(px(0.0))
            .right(px(0.0))
            .when(body, |overlay| {
                overlay.p(px(theme.metrics.spacing_3)).items_start()
            })
            .when(!body, |overlay| {
                overlay.px(px(theme.metrics.spacing_2)).items_center()
            })
            .flex()
            .overflow_hidden()
            .font_family(theme.typography.monospace_family)
            .text_size(px(theme.typography.body_size))
            .text_color(transparent_black())
            .debug_selector(|| "variable-highlight-overlay".into())
            .child(StyledText::new(value.clone()).with_highlights(highlights)),
    );

    let rows = ranges
        .into_iter()
        .map(|(_, name)| {
            let resolved = variables
                .values
                .get(&name)
                .cloned()
                .unwrap_or_else(|| variables.unavailable_message.clone());
            (name, resolved)
        })
        .collect::<Vec<_>>();
    wrapper
        .tooltip(move |_, cx| {
            cx.new(|_| VariableTooltip {
                theme,
                rows: rows.clone(),
            })
            .into()
        })
        .tooltip_show_delay(Duration::from_millis(200))
        .into_any_element()
}

fn variable_ranges(value: &str) -> Vec<(Range<usize>, String)> {
    let mut ranges = Vec::new();
    let mut offset = 0;
    let mut remaining = value;
    while let Some(start) = remaining.find("{{") {
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("}}") else {
            break;
        };
        let name = &after_start[..end];
        if !name.is_empty() && !name.contains("{{") {
            let range_start = offset + start;
            let range_end = range_start + 2 + end + 2;
            ranges.push((range_start..range_end, name.to_owned()));
        }
        let consumed = start + 2 + end + 2;
        offset += consumed;
        remaining = &remaining[consumed..];
    }
    ranges
}

pub fn menu_button(
    theme: Theme,
    id: impl Into<ElementId>,
    label: impl Into<String>,
    on_activate: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_activate = Rc::new(on_activate);
    let pointer_activate = on_activate.clone();
    let keyboard_activate = on_activate;

    // A controlled popover can rerender after its button receives focus on
    // mouse-down. Activate before that rerender so the subsequent click is not
    // lost when the popup is unmounted. Keyboard activation still follows the
    // headless button's normal action path.
    gpui::div()
        .w_full()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            pointer_activate(window, cx);
        })
        .child(
            ButtonRoot::new()
                .id(id)
                .w_full()
                .h(px(32.0))
                .px(px(theme.metrics.spacing_3))
                .flex()
                .items_center()
                .justify_start()
                .rounded(px(theme.metrics.radius_small))
                .font_family(theme.typography.interface_family)
                .text_size(px(theme.typography.body_size))
                .text_color(theme.colors.text.primary)
                .on_click(move |event, window, cx| {
                    if !matches!(event, ClickEvent::Mouse(_)) {
                        keyboard_activate(window, cx);
                    }
                })
                .style_with_state(move |state, button| {
                    button
                        .border_1()
                        .border_color(if state.focused {
                            theme.colors.borders.focused
                        } else {
                            theme.colors.surfaces.overlay
                        })
                        .when(!state.disabled, |button| {
                            button
                                .cursor_pointer()
                                .hover(move |button| button.bg(theme.colors.surfaces.sidebar))
                        })
                })
                .child(label.into()),
        )
}

pub fn switch(
    theme: Theme,
    id: impl Into<ElementId>,
    label: impl Into<String>,
    checked: bool,
    on_checked_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    SwitchRoot::new()
        .id(id)
        .checked(Some(checked))
        .aria_label(label)
        .w(px(38.0))
        .h(px(22.0))
        .p(px(2.0))
        .flex()
        .items_center()
        .rounded(px(11.0))
        .on_checked_change(move |value, _, window, cx| on_checked_change(value, window, cx))
        .style_with_state(move |state, root| {
            root.bg(if state.checked {
                theme.colors.actions.accent
            } else {
                theme.colors.actions.disabled
            })
            .border_1()
            .border_color(if state.focused {
                theme.colors.borders.focused
            } else {
                theme.colors.borders.standard
            })
            .when(!state.disabled, |root| root.cursor_pointer())
        })
        .child(
            SwitchThumb::new()
                .size(px(16.0))
                .rounded(px(8.0))
                .style_with_state(move |state, thumb| {
                    thumb
                        .bg(theme.colors.surfaces.raised)
                        .when(state.root.checked, |thumb| thumb.ml(px(16.0)))
                }),
        )
}

pub(crate) fn pane_layout_toggle(
    theme: Theme,
    layout: PaneLayout,
    on_change: impl Fn(PaneLayout, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let selected = match layout {
        PaneLayout::Vertical => "vertical",
        PaneLayout::Horizontal => "horizontal",
    };
    let item =
        move |index: usize, value: &'static str, label: &'static str, glyph: &'static str| {
            Toggle::new()
                .id(("pane-layout", index))
                .value(value)
                .aria_label(label)
                .w(px(30.0))
                .h(px(26.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme.metrics.radius_small))
                .text_size(px(theme.typography.body_size))
                .style_with_state(move |state, toggle| {
                    toggle
                        .text_color(if state.pressed {
                            theme.colors.selection.active_foreground
                        } else {
                            theme.colors.text.secondary
                        })
                        .when(state.pressed, |toggle| {
                            toggle.bg(theme.colors.selection.active_background)
                        })
                        .when(!state.pressed, |toggle| {
                            toggle.hover(move |toggle| toggle.bg(theme.colors.surfaces.raised))
                        })
                })
                .child(glyph)
        };

    ToggleGroup::<&'static str>::new()
        .id("pane-layout-toggle")
        .aria_label("Request and response layout")
        .value(vec![selected])
        .on_value_change(move |values, _, window, cx| {
            let Some(value) = values.first() else {
                return;
            };
            let layout = if *value == "horizontal" {
                PaneLayout::Horizontal
            } else {
                PaneLayout::Vertical
            };
            on_change(layout, window, cx);
        })
        .p(px(2.0))
        .flex()
        .gap(px(2.0))
        .rounded(px(theme.metrics.radius_medium))
        .border_1()
        .border_color(theme.colors.borders.standard)
        .bg(theme.colors.surfaces.window)
        .child(item(0, "vertical", "Stack response below request", "↕"))
        .child(item(1, "horizontal", "Place response beside request", "↔"))
}

#[cfg(test)]
mod tests {
    use base_gpui::popover::{
        PopoverPopup, PopoverPortal, PopoverPositioner, PopoverRoot, PopoverTrigger,
    };
    use gpui::{
        Context, IntoElement, Modifiers, Render, TestAppContext, VisualTestContext, div,
        prelude::*, px, size,
    };

    use super::menu_button;
    use crate::theme::Theme;

    struct MenuTestView {
        open: bool,
        activations: usize,
    }

    impl Render for MenuTestView {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
            let open_view = cx.weak_entity();
            let activate_view = cx.weak_entity();

            div().size_full().p(px(20.0)).child(
                PopoverRoot::<()>::new()
                    .id("menu-test-popover")
                    .open(self.open)
                    .on_open_change(move |open, _, _, cx| {
                        let _ = open_view.update(cx, |view, cx| {
                            view.open = open;
                            cx.notify();
                        });
                    })
                    .child(
                        PopoverTrigger::new()
                            .id("menu-test-trigger")
                            .w(px(100.0))
                            .h(px(28.0))
                            .style_with_state(|_, trigger| {
                                trigger.debug_selector(|| "menu-test-trigger".into())
                            })
                            .child("Open"),
                    )
                    .child(
                        PopoverPortal::new().child(
                            PopoverPositioner::new().child(
                                PopoverPopup::new()
                                    .w(px(180.0))
                                    .style_with_state(|_, popup| {
                                        popup.debug_selector(|| "menu-test-popup".into())
                                    })
                                    .child_any(menu_button(
                                        Theme::light(),
                                        "menu-test-item",
                                        "Workspace",
                                        move |_, cx| {
                                            let _ = activate_view.update(cx, |view, cx| {
                                                view.activations += 1;
                                                view.open = false;
                                                cx.notify();
                                            });
                                        },
                                    )),
                            ),
                        ),
                    ),
            )
        }
    }

    #[gpui::test]
    fn controlled_popover_menu_item_activates_on_pointer_press(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(320.0), px(180.0)), |_, _| MenuTestView {
            open: false,
            activations: 0,
        });
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let trigger = visual
            .debug_bounds("menu-test-trigger")
            .expect("trigger should be rendered");
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let popup = visual
            .debug_bounds("menu-test-popup")
            .expect("popup should be rendered");
        visual.simulate_click(popup.center(), Modifiers::default());
        visual.run_until_parked();
        cx.run_until_parked();

        let (open, activations) = window
            .update(cx, |view, _, _| (view.open, view.activations))
            .expect("test window should remain open");
        assert!(!open);
        assert_eq!(activations, 1);
    }
}
