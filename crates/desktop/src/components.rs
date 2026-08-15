//! Probe-styled compositions over headless base-gpui behavior.

use std::rc::Rc;

use base_gpui::{
    button::ButtonRoot,
    switch::{SwitchRoot, SwitchThumb},
    toggle::Toggle,
    toggle_group::ToggleGroup,
};
use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Styled as _, Window, prelude::FluentBuilder as _, px,
};

use crate::theme::Theme;

use crate::shell::PaneLayout;

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
