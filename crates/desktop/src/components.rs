//! Probe-styled compositions over headless base-gpui behavior.

use base_gpui::{
    button::ButtonRoot,
    switch::{SwitchRoot, SwitchThumb},
};
use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Styled as _, Window, prelude::FluentBuilder as _, px,
};

use crate::theme::Theme;

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
