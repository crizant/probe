//! Probe-styled compositions over headless Longbridge gpui-base behavior.

use std::{collections::BTreeMap, ops::Range, rc::Rc, sync::Arc, time::Duration};

use crate::response_viewer::{SearchMatch, join_header_lines};
use crate::shell::PaneLayout;
use crate::theme::Theme;
use gpui::{
    App, AppContext as _, Bounds, ClickEvent, ContentMask, Context, Element, ElementId, Entity,
    Focusable, FontWeight, GlobalElementId, HighlightStyle, Hsla, InspectorElementId,
    InteractiveElement as _, IntoElement, LayoutId, MouseButton, PaintQuad, ParentElement as _,
    Pixels, Render, RenderOnce, Role, ShapedLine, SharedString, StatefulInteractiveElement as _,
    Style, Styled as _, Subscription, TextAlign, TextRun, TransformationMatrix, Window, canvas,
    div, fill, point, prelude::FluentBuilder as _, px, relative, size, transparent_black,
};
use gpui_base::{
    Button, Editor, Input, InputBase, Popover, Switch, SwitchThumb, SwitchTrack, Toggle,
    ToggleGroup,
    actions::{Confirm, SelectDown, SelectUp},
    input::{
        EditorState, InputEditorStyle, InputEvent, InputState, TextDecoration,
        TextDecorationCollection,
    },
};

/// Single-line label that shows an ellipsis when the available width is too small.
pub(crate) fn truncated_label(text: impl Into<String>) -> gpui::Div {
    div().min_w(px(0.0)).truncate().child(text.into())
}

const CHEVRON_DOWN_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M3 5.5 8 11 13 5.5" stroke="black" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
const CHEVRON_UP_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M3 10.5 8 5 13 10.5" stroke="black" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
const CHEVRON_RIGHT_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M5.5 3 11 8 5.5 13" stroke="black" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
const PLUS_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M8 3v10M3 8h10" stroke="black" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
const SAVE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M3 2.5h8l2 2v9H3v-11Z" stroke="black" stroke-width="1.5" stroke-linejoin="round"/><path d="M5 2.5v4h6v-4M5 13.5v-4h6v4" stroke="black" stroke-width="1.5" stroke-linejoin="round"/></svg>"#;

fn painted_svg_icon(path: &'static str, svg: &'static [u8], size: f32) -> gpui::Div {
    let size = px(size);
    div()
        .flex_none()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    let _ = window.paint_svg(
                        bounds,
                        SharedString::from(path),
                        Some(svg),
                        TransformationMatrix::default(),
                        window.text_style().color,
                        cx,
                    );
                },
            )
            .size(size),
        )
}

pub(crate) fn chevron_icon(theme: Theme, expanded: bool) -> gpui::Div {
    let icon = if expanded {
        painted_svg_icon("probe-chevron-up", CHEVRON_UP_SVG, theme.metrics.icon_small)
    } else {
        painted_svg_icon(
            "probe-chevron-down",
            CHEVRON_DOWN_SVG,
            theme.metrics.icon_small,
        )
    };
    icon.text_color(theme.colors.text.muted)
}

pub(crate) fn tree_chevron_icon(theme: Theme, expanded: bool) -> gpui::Div {
    let icon = if expanded {
        painted_svg_icon(
            "probe-chevron-down",
            CHEVRON_DOWN_SVG,
            theme.metrics.icon_small,
        )
    } else {
        painted_svg_icon(
            "probe-chevron-right",
            CHEVRON_RIGHT_SVG,
            theme.metrics.icon_small,
        )
    };
    icon.text_color(theme.colors.text.muted)
}

fn plus_icon(theme: Theme) -> gpui::Div {
    painted_svg_icon("probe-plus", PLUS_SVG, theme.metrics.icon_small)
}

pub(crate) fn save_icon(theme: Theme) -> gpui::Div {
    painted_svg_icon("probe-save", SAVE_SVG, theme.metrics.icon_standard)
        .text_color(theme.colors.text.primary)
}

pub(crate) fn close_icon(theme: Theme) -> gpui::Div {
    div()
        .flex_none()
        .w(px(theme.metrics.icon_standard))
        .h(px(theme.metrics.icon_standard))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(theme.metrics.icon_standard))
        .line_height(relative(1.0))
        .child("×")
}

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
            .px(px(self.theme.metrics.spacing_3))
            .py(px(self.theme.metrics.spacing_2))
            .flex()
            .flex_col()
            .gap(px(self.theme.metrics.spacing_1))
            .rounded(px(self.theme.metrics.radius_medium))
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
                    .items_center()
                    .gap(px(self.theme.metrics.spacing_2))
                    .child(
                        truncated_label(format!("{{{{{name}}}}}"))
                            .flex_none()
                            .max_w(px(160.0))
                            .text_color(self.theme.colors.syntax.string),
                    )
                    .child(truncated_label(value.clone()).flex_1()),
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
    Button::new(id)
        .h(px(theme.metrics.control_height))
        .px(px(theme.metrics.spacing_3))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme.metrics.radius_small))
        .font_family(theme.typography.interface_family)
        .text_size(px(theme.typography.body_size))
        .text_color(theme.colors.text.inverse)
        .bg(theme.colors.actions.accent)
        .border_1()
        .border_color(theme.colors.actions.accent)
        .cursor_pointer()
        .hover(move |button| {
            button
                .bg(theme.colors.actions.hover)
                .border_color(theme.colors.actions.hover)
        })
        .focus(move |button| button.border_color(theme.colors.borders.focused))
        .styles(move |styles| {
            styles.disabled(move |button| {
                button
                    .bg(theme.colors.actions.disabled)
                    .text_color(theme.colors.actions.disabled_foreground)
                    .border_color(theme.colors.actions.disabled)
            })
        })
        .on_click(on_click)
        .child(label)
}

type InputChangeHandler = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
type DropdownChangeHandler<T> = Rc<dyn Fn(Option<&T>, &mut Window, &mut App)>;
type VisibleRangeHandler = Rc<dyn Fn(Range<usize>, &mut App)>;

struct FieldInput {
    state: Entity<InputState>,
    on_change: InputChangeHandler,
    on_enter: Option<InputChangeHandler>,
    _subscription: Subscription,
}

impl FieldInput {
    fn on_event(
        this: &mut Self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let value = input.read(cx).value();
                (this.on_change)(value, window, cx);
            }
            InputEvent::PressEnter { .. } => {
                if let Some(on_enter) = this.on_enter.clone() {
                    let value = input.read(cx).value();
                    on_enter(value, window, cx);
                }
            }
            _ => {}
        }
    }
}

#[derive(IntoElement)]
struct ProbeTextInput {
    theme: Theme,
    id: ElementId,
    value: SharedString,
    placeholder: SharedString,
    variables: VariableContext,
    font_family: &'static str,
    text_size: f32,
    height: f32,
    width: Option<f32>,
    debug_selector: Option<&'static str>,
    on_change: InputChangeHandler,
    on_enter: Option<InputChangeHandler>,
}

impl RenderOnce for ProbeTextInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let placeholder = self.placeholder.clone();
        let on_change = self.on_change.clone();
        let on_enter = self.on_enter.clone();
        let field = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            let state = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(placeholder.clone());
                state.set_editor_style(editor_paint_style(self.theme));
                state
            });
            let subscription = cx.subscribe_in(&state, window, FieldInput::on_event);
            FieldInput {
                state,
                on_change: on_change.clone(),
                on_enter: on_enter.clone(),
                _subscription: subscription,
            }
        });
        field.update(cx, |field, cx| {
            field.on_change = self.on_change.clone();
            field.on_enter = self.on_enter.clone();
            field.state.update(cx, |input, cx| {
                input.set_editor_style(editor_paint_style(self.theme));
                if input.value() != self.value {
                    input.set_value(self.value.clone(), window, cx);
                }
            });
        });
        let state = field.read(cx).state.clone();
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        let tooltip_id = ElementId::NamedChild(
            Arc::new(self.id.clone()),
            SharedString::from("variable-tooltip"),
        );
        let theme = self.theme;
        let input = InputBase::new(self.id.clone())
            .h(px(self.height))
            .when_some(self.width, |input, width| input.w(px(width)))
            .when(self.width.is_none(), |input| input.min_w(px(0.0)).w_full())
            .px(px(theme.metrics.spacing_2))
            .flex()
            .items_center()
            .rounded(px(theme.metrics.radius_small))
            .font_family(self.font_family)
            .text_size(px(self.text_size))
            .text_color(theme.colors.text.primary)
            .bg(theme.colors.surfaces.raised)
            .border_1()
            .border_color(if focused {
                theme.colors.borders.focused
            } else {
                theme.colors.borders.standard
            })
            .focused(focused)
            .styles(move |styles| {
                styles.focused(move |input| input.border_color(theme.colors.borders.focused))
            })
            .when_some(self.debug_selector, |input, selector| {
                input.debug_selector(move || selector.into())
            })
            .on_mouse_down(MouseButton::Left, {
                let state = state.clone();
                move |_, window, cx| {
                    state.update(cx, |input, cx| input.focus(window, cx));
                }
            })
            .child(Input::new(&state));
        variable_input_overlay(
            self.theme,
            state,
            tooltip_id,
            input,
            self.value,
            self.variables,
        )
    }
}

pub(crate) fn variable_text_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    variables: VariableContext,
    on_value_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    ProbeTextInput {
        theme,
        id: id.into(),
        value: single_line(value),
        placeholder: placeholder.into(),
        variables,
        font_family: theme.typography.monospace_family,
        text_size: theme.typography.body_size,
        height: theme.metrics.control_height,
        width: None,
        debug_selector: None,
        on_change: Rc::new(on_value_change),
        on_enter: None,
    }
    .into_any_element()
}

pub(crate) fn search_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    on_value_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    on_enter: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    ProbeTextInput {
        theme,
        id: id.into(),
        value: single_line(value),
        placeholder: placeholder.into(),
        variables: VariableContext::default(),
        font_family: theme.typography.interface_family,
        text_size: theme.typography.body_size,
        height: theme.metrics.control_height - 2.0,
        width: Some(180.0),
        debug_selector: Some("response-search"),
        on_change: Rc::new(on_value_change),
        on_enter: Some(Rc::new(on_enter)),
    }
}

fn editor_button_base(
    theme: Theme,
    id: impl Into<ElementId>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .selected(selected)
        .h(px(theme.metrics.control_height))
        .px(px(theme.metrics.spacing_3))
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
        .bg(if selected {
            theme.colors.selection.active_background
        } else {
            theme.colors.surfaces.window
        })
        .border_1()
        .border_color(theme.colors.borders.standard)
        .cursor_pointer()
        .hover(move |button| {
            if selected {
                button
            } else {
                button.bg(theme.colors.surfaces.raised)
            }
        })
        .focus(move |button| button.border_color(theme.colors.borders.focused))
        .on_click(on_click)
}

pub(crate) fn editor_button(
    theme: Theme,
    id: impl Into<ElementId>,
    label: impl Into<String>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    editor_button_base(theme, id, selected, on_click).child(label.into())
}

pub(crate) fn editor_add_button(
    theme: Theme,
    id: impl Into<ElementId>,
    label: impl Into<String>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    editor_button_base(theme, id, false, on_click)
        .gap(px(theme.metrics.spacing_1))
        .child(plus_icon(theme))
        .child(label.into())
}

pub(crate) fn text_tab(
    theme: Theme,
    id: impl Into<ElementId>,
    label: impl Into<String>,
    selected: bool,
    position: usize,
    size: usize,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let label = label.into();
    Button::new(id)
        .role(Role::Tab)
        .selected(selected)
        .aria_selected(selected)
        .aria_position_in_set(position)
        .aria_size_of_set(size)
        .h(px(theme.metrics.control_height - 2.0))
        .px(px(theme.metrics.spacing_2))
        .flex()
        .items_center()
        .rounded(px(theme.metrics.radius_medium))
        .text_size(px(theme.typography.caption_size))
        .text_color(if selected {
            theme.colors.text.primary
        } else {
            theme.colors.text.secondary
        })
        .when(selected, |tab| {
            tab.bg(theme.colors.selection.inactive_background)
                .font_weight(FontWeight::SEMIBOLD)
        })
        .when(!selected, |tab| {
            tab.hover(move |tab| tab.bg(theme.colors.surfaces.raised))
        })
        .cursor_pointer()
        .on_click(on_click)
        .child(label)
}

pub(crate) fn remove_row_button(
    theme: Theme,
    id: impl Into<ElementId>,
    aria_label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let color = theme.colors.text.secondary;
    Button::new(id)
        .accessibility_label(aria_label)
        .w(px(30.0))
        .h(px(theme.metrics.control_height))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme.metrics.radius_small))
        .bg(theme.colors.surfaces.raised)
        .border_1()
        .border_color(theme.colors.borders.standard)
        .cursor_pointer()
        .hover(move |button| button.bg(theme.colors.selection.inactive_background))
        .focus(move |button| button.border_color(theme.colors.borders.focused))
        .on_click(on_click)
        .child(trash_icon(color))
}

fn trash_icon(color: gpui::Rgba) -> gpui::Div {
    div()
        .w(px(12.0))
        .h(px(13.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(1.0))
        .child(div().w(px(4.0)).h(px(1.5)).rounded(px(1.0)).bg(color))
        .child(div().w(px(12.0)).h(px(1.5)).rounded(px(0.5)).bg(color))
        .child(
            div()
                .w(px(10.0))
                .h(px(8.0))
                .rounded(px(1.5))
                .border_1()
                .border_color(color)
                .overflow_hidden()
                .flex()
                .justify_center()
                .gap(px(1.5))
                .pt(px(1.5))
                .child(div().w(px(1.0)).h(px(4.5)).bg(color))
                .child(div().w(px(1.0)).h(px(4.5)).bg(color)),
        )
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
    dropdown_with_option_colors(
        theme,
        id,
        aria_label,
        value,
        options
            .into_iter()
            .map(|(value, label)| (value, label, None))
            .collect(),
        width,
        on_value_change,
    )
}

pub(crate) fn dropdown_with_option_colors<T: Clone + Eq + 'static>(
    theme: Theme,
    id: &'static str,
    aria_label: &'static str,
    value: Option<T>,
    options: Vec<(T, String, Option<gpui::Rgba>)>,
    width: f32,
    on_value_change: impl Fn(Option<&T>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    ProbeDropdown {
        theme,
        id,
        aria_label,
        value,
        options,
        width,
        on_value_change: Rc::new(on_value_change),
    }
}

#[derive(IntoElement)]
struct ProbeDropdown<T: Clone + Eq + 'static> {
    theme: Theme,
    id: &'static str,
    aria_label: &'static str,
    value: Option<T>,
    options: Vec<(T, String, Option<gpui::Rgba>)>,
    width: f32,
    on_value_change: DropdownChangeHandler<T>,
}

#[derive(Debug)]
struct DropdownState {
    open: bool,
    highlighted: usize,
}

impl<T: Clone + Eq + 'static> RenderOnce for ProbeDropdown<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let selected_index = self
            .value
            .as_ref()
            .and_then(|selected| {
                self.options
                    .iter()
                    .position(|(value, _, _)| value == selected)
            })
            .unwrap_or(0);
        let state =
            window.use_keyed_state(ElementId::from(format!("{}-state", self.id)), cx, |_, _| {
                DropdownState {
                    open: false,
                    highlighted: selected_index,
                }
            });
        state.update(cx, |state, _| {
            if state.highlighted >= self.options.len() {
                state.highlighted = selected_index;
            }
        });
        let open = state.read(cx).open;
        let parent = window.current_view();
        let selected_label = self
            .value
            .as_ref()
            .and_then(|selected| {
                self.options
                    .iter()
                    .find(|(value, _, _)| value == selected)
                    .map(|(_, label, _)| label.clone())
            })
            .unwrap_or_else(|| "None".to_owned());
        let selected_color = self
            .value
            .as_ref()
            .and_then(|selected| {
                self.options
                    .iter()
                    .find(|(value, _, _)| value == selected)
                    .and_then(|(_, _, color)| *color)
            })
            .unwrap_or(self.theme.colors.text.primary);
        let theme = self.theme;
        let id = self.id;
        let option_count = self.options.len();
        let option_focus_handles = (0..option_count)
            .map(|index| {
                window
                    .use_keyed_state(format!("{id}-item-{index}-focus"), cx, |_, cx| {
                        cx.focus_handle()
                    })
                    .read(cx)
                    .clone()
            })
            .collect::<Vec<_>>();
        let trigger = Button::new(format!("{id}-trigger"))
            .role(Role::ComboBox)
            .accessibility_label(self.aria_label)
            .selected(open)
            .aria_expanded(open)
            .w_full()
            .h(px(theme.metrics.control_height))
            .px(px(theme.metrics.spacing_2))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(theme.metrics.spacing_1))
            .overflow_hidden()
            .rounded(px(theme.metrics.radius_small))
            .bg(theme.colors.surfaces.raised)
            .border_1()
            .border_color(theme.colors.borders.standard)
            .text_color(selected_color)
            .debug_selector(move || format!("{id}-trigger"))
            .hover(move |trigger| trigger.bg(theme.colors.selection.inactive_background))
            .focus(move |trigger| trigger.border_color(theme.colors.borders.focused))
            .child(truncated_label(selected_label).min_w(px(0.0)).flex_1())
            .child(chevron_icon(theme, open));
        let options = Rc::new(self.options);
        let selected_value = self.value;
        let on_value_change = self.on_value_change;
        Popover::new(id)
            .open(open)
            .on_open_change({
                let state = state.clone();
                move |next, _, cx| {
                    state.update(cx, |state, _| {
                        state.open = *next;
                        if *next {
                            state.highlighted = selected_index;
                        }
                    });
                    cx.notify(parent);
                }
            })
            .trigger(trigger)
            .content(move |popover_state, _window, cx| {
                let list_focus = popover_state.focus_handle(cx);
                let popover = cx.entity();
                let values = options.clone();
                let confirm_handler = on_value_change.clone();
                let confirm_state = state.clone();
                let mut list = div()
                    .track_focus(&list_focus)
                    .key_context("Select")
                    .on_action({
                        let state = state.clone();
                        let handles = option_focus_handles.clone();
                        move |_: &SelectDown, window, cx| {
                            if handles.is_empty() {
                                return;
                            }
                            let next = state.update(cx, |state, _| {
                                state.highlighted = (state.highlighted + 1) % handles.len();
                                state.highlighted
                            });
                            handles[next].focus(window, cx);
                        }
                    })
                    .on_action({
                        let state = state.clone();
                        let handles = option_focus_handles.clone();
                        move |_: &SelectUp, window, cx| {
                            if handles.is_empty() {
                                return;
                            }
                            let next = state.update(cx, |state, _| {
                                state.highlighted =
                                    (state.highlighted + handles.len() - 1) % handles.len();
                                state.highlighted
                            });
                            handles[next].focus(window, cx);
                        }
                    })
                    .on_action(move |_: &Confirm, window, cx| {
                        let index = confirm_state.read(cx).highlighted;
                        if let Some((value, _, _)) = values.get(index) {
                            confirm_handler(Some(value), window, cx);
                            popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                            cx.notify(parent);
                        }
                    })
                    .flex()
                    .flex_col()
                    .gap(px(theme.metrics.spacing_1))
                    .w(px(self.width.max(160.0)))
                    .p(px(theme.metrics.spacing_2))
                    .rounded(px(theme.metrics.radius_medium))
                    .bg(theme.colors.surfaces.overlay)
                    .border_1()
                    .border_color(theme.colors.borders.standard);
                for (index, (value, label, color)) in options.iter().enumerate() {
                    let color = color.unwrap_or(theme.colors.text.primary);
                    let selected = selected_value.as_ref() == Some(value);
                    let on_value_change = on_value_change.clone();
                    let value = value.clone();
                    let popover = cx.entity();
                    let focus_handle = option_focus_handles[index].clone();
                    list = list.child(
                        Button::new(format!("{id}-item-{index}"))
                            .role(Role::ListBoxOption)
                            .aria_selected(selected)
                            .track_focus(&focus_handle)
                            .w_full()
                            .h(px(theme.metrics.control_height))
                            .px(px(theme.metrics.spacing_3))
                            .flex()
                            .items_center()
                            .gap(px(theme.metrics.spacing_2))
                            .overflow_hidden()
                            .rounded(px(theme.metrics.radius_small))
                            .text_color(color)
                            .debug_selector(move || format!("{id}-item-{index}"))
                            .hover(move |item| {
                                item.bg(theme.colors.surfaces.sidebar).cursor_pointer()
                            })
                            .on_click(move |_, window, cx| {
                                on_value_change(Some(&value), window, cx);
                                popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                                cx.notify(parent);
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(14.0))
                                    .when(!selected, |marker| marker.invisible())
                                    .child("✓"),
                            )
                            .child(truncated_label(label.clone()).min_w(px(0.0)).flex_1()),
                    );
                }
                list
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BodySyntax {
    Plain,
    Json,
    Xml,
}

pub(crate) fn body_text_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    variables: VariableContext,
    syntax: BodySyntax,
    on_value_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let id = id.into();
    let tooltip_id =
        ElementId::NamedChild(Arc::new(id.clone()), SharedString::from("variable-tooltip"));
    let value = value.into();
    let ranges = variable_ranges(&value);
    let decorations = body_text_highlights(theme, &ranges);
    let editor = ProbeEditor {
        theme,
        id,
        value: value.clone(),
        placeholder: SharedString::from("Body content"),
        decorations,
        language: match syntax {
            BodySyntax::Json => "json".into(),
            BodySyntax::Xml => "xml".into(),
            BodySyntax::Plain => SharedString::default(),
        },
        readonly: false,
        min_height: Some(120.0),
        padded: true,
        soft_wrap: true,
        text_color: theme.colors.text.primary,
        scroll_to_offset: None,
        on_change: Some(Rc::new(on_value_change)),
        on_visible_range: None,
        debug_selector: None,
    };
    let wrapper = div()
        .id(tooltip_id)
        .relative()
        .size_full()
        .debug_selector(|| "variable-input-tooltip-trigger".into())
        .child(editor);
    if ranges.is_empty() {
        return wrapper.into_any_element();
    }

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

pub(crate) fn response_body_input(
    theme: Theme,
    id: impl Into<ElementId>,
    text: &str,
    matches: &[SearchMatch],
    active_match: usize,
    language: impl Into<SharedString>,
    on_visible_range: impl Fn(Range<usize>, &mut App) + 'static,
) -> gpui::AnyElement {
    let mut decorations = Vec::new();
    let mut scroll_to_offset = None;
    for (index, found) in matches.iter().enumerate() {
        let active = index == active_match;
        if active {
            scroll_to_offset = Some(found.range.start);
        }
        decorations.push(search_match_decoration(theme, found.range.clone(), active));
    }
    ProbeEditor {
        theme,
        id: id.into(),
        value: text.to_owned().into(),
        placeholder: SharedString::default(),
        decorations,
        language: language.into(),
        readonly: true,
        min_height: None,
        padded: true,
        soft_wrap: false,
        text_color: theme.colors.syntax.plain,
        scroll_to_offset,
        on_change: None,
        on_visible_range: Some(Rc::new(on_visible_range)),
        debug_selector: None,
    }
    .into_any_element()
}

pub(crate) fn response_headers_input(
    theme: Theme,
    id: impl Into<ElementId>,
    headers: &[probe_http::ResponseHeader],
    matches: &[SearchMatch],
    active_match: usize,
    on_visible_range: impl Fn(Range<usize>, &mut App) + 'static,
) -> gpui::AnyElement {
    let joined = join_header_lines(headers);
    let mut decorations = Vec::new();
    for (offset, name_len) in joined.line_offsets.iter().zip(&joined.name_lens) {
        decorations.push(text_decoration(
            *offset..*offset + name_len,
            Some(theme.colors.text.secondary.into()),
            None,
        ));
    }
    let mut scroll_to_offset = None;
    for (index, found) in matches.iter().enumerate() {
        let active = index == active_match;
        if active {
            scroll_to_offset = Some(found.range.start);
        }
        decorations.push(search_match_decoration(theme, found.range.clone(), active));
    }
    ProbeEditor {
        theme,
        id: id.into(),
        value: joined.text.into(),
        placeholder: SharedString::default(),
        decorations,
        language: SharedString::default(),
        readonly: true,
        min_height: None,
        padded: true,
        soft_wrap: false,
        text_color: theme.colors.text.primary,
        scroll_to_offset,
        on_change: None,
        on_visible_range: Some(Rc::new(on_visible_range)),
        debug_selector: None,
    }
    .into_any_element()
}

struct EditorField {
    state: Entity<EditorState>,
    decorations: TextDecorationCollection,
    last_decorations: Vec<TextDecoration>,
    on_change: Option<InputChangeHandler>,
    last_scroll_offset: Option<usize>,
    language: SharedString,
    _subscription: Subscription,
}

impl EditorField {
    fn on_event(
        this: &mut Self,
        input: &Entity<EditorState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change)
            && let Some(on_change) = this.on_change.clone()
        {
            on_change(input.read(cx).value(), window, cx);
        }
    }
}

/// gpui-base paints caret, selection, and gutter from `InputEditorStyle`.
/// Its `Default` is fully transparent, so Probe must supply visible tokens.
fn editor_paint_style(theme: Theme) -> InputEditorStyle {
    InputEditorStyle {
        foreground: theme.colors.text.primary.into(),
        muted_foreground: theme.colors.text.muted.into(),
        background: theme.colors.surfaces.raised.into(),
        border: theme.colors.borders.standard.into(),
        selection: theme.editor_selection(),
        caret: theme.colors.text.primary.into(),
        highlight_styles: Arc::new(crate::syntax::ProbeHighlightStyles::new(theme)),
        ..Default::default()
    }
}

#[derive(IntoElement)]
struct ProbeEditor {
    theme: Theme,
    id: ElementId,
    value: SharedString,
    placeholder: SharedString,
    decorations: Vec<TextDecoration>,
    language: SharedString,
    readonly: bool,
    min_height: Option<f32>,
    padded: bool,
    soft_wrap: bool,
    text_color: gpui::Rgba,
    scroll_to_offset: Option<usize>,
    on_change: Option<InputChangeHandler>,
    on_visible_range: Option<VisibleRangeHandler>,
    debug_selector: Option<&'static str>,
}

impl RenderOnce for ProbeEditor {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let placeholder = self.placeholder.clone();
        let on_change = self.on_change.clone();
        let soft_wrap = self.soft_wrap;
        let language = self.language.clone();
        let field = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            let state = cx.new(|cx| {
                let mut editor = EditorState::new(window, cx)
                    .placeholder(placeholder.clone())
                    .folding(false)
                    .searchable(false)
                    .soft_wrap(soft_wrap)
                    .language(language.clone());
                editor.set_highlighter_factory(crate::syntax::factory(), cx);
                editor.set_editor_style(editor_paint_style(self.theme));
                editor
            });
            state.update(cx, |editor, cx| {
                editor.set_readonly(self.readonly, cx);
                editor.set_value(self.value.clone(), window, cx);
            });
            let decorations = state.update(cx, |editor, cx| {
                editor.create_decorations_collection(Vec::new(), cx)
            });
            let subscription = cx.subscribe_in(&state, window, EditorField::on_event);
            EditorField {
                state,
                decorations,
                last_decorations: Vec::new(),
                on_change: on_change.clone(),
                last_scroll_offset: None,
                language: language.clone(),
                _subscription: subscription,
            }
        });
        field.update(cx, |field, cx| {
            field.on_change = self.on_change.clone();
            let language_changed = field.language != self.language;
            if language_changed {
                field.language = self.language.clone();
            }
            field.state.update(cx, |editor, cx| {
                editor.set_editor_style(editor_paint_style(self.theme));
                editor.set_readonly(self.readonly, cx);
                if language_changed {
                    editor.set_highlighter(self.language.clone(), cx);
                }
                if editor.value() != self.value {
                    editor.set_value(self.value.clone(), window, cx);
                }
            });
            if field.last_decorations != self.decorations {
                field.last_decorations = self.decorations.clone();
                field.decorations.set(self.decorations.clone(), cx);
            }
            if self.scroll_to_offset != field.last_scroll_offset {
                if let Some(offset) = self.scroll_to_offset {
                    let laid_out = field.state.read(cx).visible_row_range().is_some();
                    field.state.update(cx, |editor, cx| {
                        editor.set_selected_range(offset..offset, cx);
                    });
                    if laid_out {
                        field.last_scroll_offset = self.scroll_to_offset;
                    }
                } else {
                    field.last_scroll_offset = None;
                }
            }
        });
        let state = field.read(cx).state.clone();
        if let Some(on_visible_range) = self.on_visible_range {
            match state.read(cx).visible_row_range() {
                Some(range) => on_visible_range(range, cx),
                None => window.request_animation_frame(),
            }
        }
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        let theme = self.theme;
        InputBase::new(self.id)
            .size_full()
            .when_some(self.min_height, |editor, height| editor.min_h(px(height)))
            .when(self.padded, |editor| editor.p(px(theme.metrics.spacing_2)))
            .overflow_hidden()
            .rounded(px(theme.metrics.radius_small))
            .font_family(theme.typography.monospace_family)
            .text_size(px(theme.typography.body_size))
            .text_color(self.text_color)
            .bg(theme.colors.surfaces.raised)
            .border_1()
            .border_color(if focused {
                theme.colors.borders.focused
            } else {
                theme.colors.borders.standard
            })
            .focused(focused)
            .styles(move |styles| {
                styles.focused(move |editor| editor.border_color(theme.colors.borders.focused))
            })
            .when_some(self.debug_selector, |editor, selector| {
                editor.debug_selector(move || selector.into())
            })
            .on_mouse_down(MouseButton::Left, {
                let state = state.clone();
                move |_, window, cx| {
                    state.update(cx, |editor, cx| editor.focus(window, cx));
                }
            })
            .child(div().size_full().child(Editor::new(&state)))
    }
}

fn body_text_highlights(theme: Theme, variables: &[(Range<usize>, String)]) -> Vec<TextDecoration> {
    variables
        .iter()
        .map(|(range, _)| {
            text_decoration(range.clone(), Some(theme.colors.syntax.string.into()), None)
        })
        .collect()
}

fn search_match_decoration(theme: Theme, range: Range<usize>, active: bool) -> TextDecoration {
    text_decoration(
        range,
        active.then(|| theme.colors.selection.active_foreground.into()),
        Some(if active {
            theme.colors.selection.active_background.into()
        } else {
            theme.colors.selection.inactive_background.into()
        }),
    )
}

fn text_decoration(
    range: Range<usize>,
    color: Option<Hsla>,
    background: Option<Hsla>,
) -> TextDecoration {
    TextDecoration::new(
        range,
        HighlightStyle {
            color,
            background_color: background,
            ..Default::default()
        },
    )
}

fn variable_input_overlay(
    theme: Theme,
    state: Entity<InputState>,
    tooltip_id: ElementId,
    input: impl IntoElement,
    value: SharedString,
    variables: VariableContext,
) -> gpui::AnyElement {
    let ranges = variable_ranges(&value);
    // Input paints first so it keeps native caret, selection, and scroll.
    // The overlay sits on top, recolors `{{variable}}` spans, and covers the
    // native caret while blink is off.
    let wrapper = div()
        .id(tooltip_id)
        .relative()
        .debug_selector(|| "variable-input-tooltip-trigger".into())
        .w_full()
        .child(input)
        .child(variable_highlight_layer(
            theme,
            state,
            value,
            ranges.clone(),
            theme.typography.monospace_family,
            theme.typography.body_size,
        ));
    if ranges.is_empty() {
        return wrapper.into_any_element();
    }

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

fn variable_highlight_layer(
    theme: Theme,
    state: Entity<InputState>,
    value: SharedString,
    ranges: Vec<(Range<usize>, String)>,
    font_family: &'static str,
    text_size: f32,
) -> impl IntoElement {
    let highlight_color = theme.colors.syntax.string.into();
    let caret_color = theme.colors.text.primary.into();
    let mask_color = theme.colors.surfaces.raised.into();
    div()
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        // Match the input's border box so highlight rects share its origin
        // and visible width.
        .border_1()
        .border_color(transparent_black())
        .px(px(theme.metrics.spacing_2))
        .items_center()
        .flex()
        .overflow_hidden()
        .font_family(font_family)
        .text_size(px(text_size))
        .when(!ranges.is_empty(), |layer| {
            layer.debug_selector(|| "variable-highlight-overlay".into())
        })
        .child(VariableHighlightElement {
            state,
            value,
            ranges: ranges.into_iter().map(|(range, _)| range).collect(),
            highlight_color,
            caret_color,
            mask_color,
        })
}

struct VariableHighlightElement {
    state: Entity<InputState>,
    value: SharedString,
    ranges: Vec<Range<usize>>,
    highlight_color: Hsla,
    caret_color: Hsla,
    mask_color: Hsla,
}

struct VariableHighlightPrepaintState {
    line: Option<ShapedLine>,
    caret: Option<PaintQuad>,
    scroll_offset: Pixels,
    #[cfg(test)]
    cursor_x: Pixels,
}

impl IntoElement for VariableHighlightElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for VariableHighlightElement {
    type RequestLayoutState = ();
    type PrepaintState = VariableHighlightPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self.state.clone();
        let cursor = state.read(cx).cursor();
        let selected_range = state.read(cx).selected_range();
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        let style = window.text_style();
        let display = single_line(self.value.clone());
        let run = TextRun {
            len: display.len(),
            font: style.font(),
            color: transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = variable_highlight_runs(&display, &self.ranges, &run, self.highlight_color);
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display, font_size, &runs, None);
        let cursor_x = line.x_for_index(cursor);
        let visible_width = bounds.right() - bounds.left();
        let scroll_offset = input_text_scroll_offset(cursor_x, visible_width);
        let caret = if focused && selected_range.is_empty() {
            let bounds = Bounds::new(
                point(bounds.left() + scroll_offset + cursor_x, bounds.top()),
                size(px(1.0), bounds.bottom() - bounds.top()),
            );
            Some(fill(
                bounds,
                if crate::caret::CaretBlink::is_visible(cx) {
                    self.caret_color
                } else {
                    self.mask_color
                },
            ))
        } else {
            None
        };
        VariableHighlightPrepaintState {
            line: Some(line),
            caret,
            scroll_offset,
            #[cfg(test)]
            cursor_x,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scroll_offset = prepaint.scroll_offset;
        let caret = prepaint.caret.take();
        let line = prepaint
            .line
            .take()
            .expect("variable highlight text should be shaped during prepaint");
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            line.paint(
                bounds.origin + point(scroll_offset, px(0.0)),
                window.line_height(),
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .expect("variable highlight text should paint");
            if let Some(caret) = caret {
                window.paint_quad(caret);
            }
        });
    }
}

/// Matches Longbridge gpui-base single-line input: shift the painted line left
/// when the caret would otherwise sit past the visible width.
fn variable_highlight_runs(
    value: &str,
    ranges: &[Range<usize>],
    base: &TextRun,
    highlight_color: Hsla,
) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut ix = 0;
    for range in ranges {
        let start = range.start.min(value.len());
        let end = range.end.min(value.len());
        if ix < start {
            runs.push(TextRun {
                len: start - ix,
                color: transparent_black(),
                ..base.clone()
            });
        }
        if end > start {
            runs.push(TextRun {
                len: end - start,
                color: highlight_color,
                ..base.clone()
            });
        }
        ix = ix.max(end);
    }
    if ix < value.len() {
        runs.push(TextRun {
            len: value.len() - ix,
            color: transparent_black(),
            ..base.clone()
        });
    }
    runs.retain(|run| run.len > 0);
    if runs.is_empty() {
        runs.push(base.clone());
    }
    runs
}

fn input_text_scroll_offset(cursor_x: Pixels, visible_width: Pixels) -> Pixels {
    if cursor_x + px(2.0) > visible_width {
        visible_width - cursor_x - px(2.0)
    } else {
        px(0.0)
    }
}

pub(crate) fn single_line(value: impl Into<SharedString>) -> SharedString {
    let value = value.into();
    if value.find(['\n', '\r']).is_none() {
        value
    } else {
        SharedString::from(value.replace(['\n', '\r'], " "))
    }
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
            Button::new(id)
                .w_full()
                .h(px(theme.metrics.control_height + 4.0))
                .px(px(theme.metrics.spacing_3))
                .flex()
                .items_center()
                .justify_start()
                .overflow_hidden()
                .rounded(px(theme.metrics.radius_small))
                .font_family(theme.typography.interface_family)
                .text_size(px(theme.typography.body_size))
                .text_color(theme.colors.text.primary)
                .border_1()
                .border_color(theme.colors.surfaces.overlay)
                .cursor_pointer()
                .hover(move |button| button.bg(theme.colors.surfaces.sidebar))
                .focus(move |button| button.border_color(theme.colors.borders.focused))
                .on_click(move |event, window, cx| {
                    if !matches!(event, ClickEvent::Mouse(_)) {
                        keyboard_activate(window, cx);
                    }
                })
                .child(truncated_label(label.into()).w_full()),
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
    Switch::new(id)
        .checked(checked)
        .accessibility_label(label)
        .w(px(36.0))
        .h(px(20.0))
        .flex()
        .items_center()
        .cursor_pointer()
        .on_change(move |value, _, window, cx| on_checked_change(value, window, cx))
        .child(
            SwitchTrack::new("switch-track")
                .checked(checked)
                .w(px(36.0))
                .h(px(20.0))
                .p(px(2.0))
                .flex()
                .items_center()
                .rounded(px(9999.0))
                .bg(if checked {
                    theme.colors.actions.accent
                } else {
                    theme.colors.borders.standard
                })
                .child(
                    SwitchThumb::new(checked)
                        .size(px(16.0))
                        .rounded(px(8.0))
                        .bg(theme.colors.text.inverse)
                        .ml(if checked { px(16.0) } else { px(0.0) }),
                ),
        )
}

pub(crate) fn pane_layout_toggle(
    theme: Theme,
    layout: PaneLayout,
    on_change: impl Fn(PaneLayout, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_change = Rc::new(on_change);
    let item = |index: usize, label: &'static str, item_layout: PaneLayout| {
        let pressed = layout == item_layout;
        let color = if pressed {
            theme.colors.selection.active_foreground
        } else {
            theme.colors.text.secondary
        };
        let on_change = on_change.clone();
        Toggle::new(("pane-layout", index))
            .pressed(pressed)
            .accessibility_label(label)
            .w(px(32.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme.metrics.radius_small))
            .text_color(color)
            .when(pressed, |toggle| {
                toggle.bg(theme.colors.selection.active_background)
            })
            .when(!pressed, |toggle| {
                toggle.hover(move |toggle| toggle.bg(theme.colors.surfaces.raised))
            })
            .on_change(move |next, _, window, cx| {
                if next {
                    on_change(item_layout, window, cx);
                }
            })
            .child(pane_layout_icon(item_layout, color))
    };

    ToggleGroup::new("pane-layout-toggle")
        .p(px(3.0))
        .flex()
        .gap(px(2.0))
        .rounded(px(theme.metrics.radius_medium))
        .border_1()
        .border_color(theme.colors.borders.standard)
        .bg(theme.colors.surfaces.window)
        .child(item(
            0,
            "Stack response below request",
            PaneLayout::Vertical,
        ))
        .child(item(
            1,
            "Place response beside request",
            PaneLayout::Horizontal,
        ))
}

fn pane_layout_icon(layout: PaneLayout, color: gpui::Rgba) -> gpui::Div {
    let divider = match layout {
        PaneLayout::Vertical => div().w_full().h(px(1.0)).bg(color),
        PaneLayout::Horizontal => div().h_full().w(px(1.0)).bg(color),
    };
    div()
        .w(px(14.0))
        .h(px(12.0))
        .rounded(px(2.0))
        .border_1()
        .border_color(color)
        .overflow_hidden()
        .flex()
        .when(layout == PaneLayout::Vertical, |icon| icon.flex_col())
        .child(div().flex_1())
        .child(divider)
        .child(div().flex_1())
}

#[cfg(test)]
mod tests {
    use gpui::{
        AppContext as _, Context, Entity, Hsla, IntoElement, Modifiers, Render, SharedString,
        TestAppContext, VisualTestContext, div, hsla, point, prelude::*, px, size,
        transparent_black,
    };
    use gpui_base::{Button, Popover, input::InputState};

    use super::{
        VariableHighlightElement, body_text_highlights, dropdown, editor_paint_style,
        input_text_scroll_offset, menu_button, single_line, variable_highlight_runs,
        variable_ranges,
    };
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
                Popover::new("menu-test-popover")
                    .open(self.open)
                    .on_open_change(move |open, _, cx| {
                        let _ = open_view.update(cx, |view, cx| {
                            view.open = *open;
                            cx.notify();
                        });
                    })
                    .trigger(
                        Button::new("menu-test-trigger")
                            .w(px(100.0))
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .debug_selector(|| "menu-test-trigger".into())
                            .child("Open"),
                    )
                    .content(move |_, _, _| {
                        div()
                            .id("menu-test-popup")
                            .w(px(180.0))
                            .debug_selector(|| "menu-test-popup".into())
                            .child(menu_button(
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
                            ))
                    }),
            )
        }
    }

    #[gpui::test]
    fn controlled_popover_menu_item_activates_on_pointer_press(cx: &mut TestAppContext) {
        cx.update(crate::theme::Theme::init);
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

    struct DropdownHoverLeakView {
        value: Option<&'static str>,
        underlay_hovered: bool,
    }

    impl Render for DropdownHoverLeakView {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
            let select_view = cx.weak_entity();
            let underlay_view = cx.weak_entity();

            div()
                .size_full()
                .p(px(12.0))
                .flex()
                .flex_col()
                .child(dropdown(
                    Theme::light(),
                    "hover-leak-select",
                    "Method",
                    self.value,
                    vec![
                        ("GET", "GET".to_owned()),
                        ("POST", "POST".to_owned()),
                        ("PUT", "PUT".to_owned()),
                        ("PATCH", "PATCH".to_owned()),
                        ("DELETE", "DELETE".to_owned()),
                    ],
                    120.0,
                    move |value, _, cx| {
                        let value = value.copied();
                        let _ = select_view.update(cx, |view, cx| {
                            view.value = value;
                            cx.notify();
                        });
                    },
                ))
                .child(
                    div()
                        .id("dropdown-underlay")
                        .flex_1()
                        .w_full()
                        .mt(px(8.0))
                        .debug_selector(|| "dropdown-underlay".into())
                        .hover(|underlay| underlay.bg(Theme::light().colors.surfaces.raised))
                        .on_hover(move |hovered, _, cx| {
                            let hovered = *hovered;
                            let _ = underlay_view.update(cx, |view, cx| {
                                view.underlay_hovered = hovered;
                                cx.notify();
                            });
                        })
                        .child("Underlay"),
                )
        }
    }

    #[gpui::test]
    fn dropdown_menu_does_not_hover_elements_underneath(cx: &mut TestAppContext) {
        cx.update(crate::theme::Theme::init);
        let window = cx.open_window(size(px(360.0), px(280.0)), |_, _| DropdownHoverLeakView {
            value: Some("GET"),
            underlay_hovered: false,
        });
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let underlay = visual
            .debug_bounds("dropdown-underlay")
            .expect("underlay should be rendered");
        visual.simulate_mouse_move(underlay.center(), None, Modifiers::default());
        visual.run_until_parked();
        cx.run_until_parked();
        let hovered = window
            .update(cx, |view, _, _| view.underlay_hovered)
            .expect("test window should remain open");
        assert!(hovered, "underlay should hover when the menu is closed");

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let trigger = visual
            .debug_bounds("hover-leak-select-trigger")
            .expect("select trigger should be rendered");
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let item = visual
            .debug_bounds("hover-leak-select-item-3")
            .expect("select item over the underlay should be rendered");
        visual.simulate_mouse_move(item.center(), None, Modifiers::default());
        visual.run_until_parked();
        cx.run_until_parked();

        let hovered = window
            .update(cx, |view, _, _| view.underlay_hovered)
            .expect("test window should remain open");
        assert!(
            !hovered,
            "hovering a dropdown item should not hover the element underneath"
        );

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(point(px(340.0), px(260.0)), Modifiers::default());
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("hover-leak-select-item-3").is_none(),
            "clicking outside should dismiss the dropdown"
        );
    }

    #[gpui::test]
    fn dropdown_keyboard_navigation_selects_and_dismisses(cx: &mut TestAppContext) {
        cx.update(crate::theme::Theme::init);
        let window = cx.open_window(size(px(360.0), px(280.0)), |_, _| DropdownHoverLeakView {
            value: Some("GET"),
            underlay_hovered: false,
        });
        cx.run_until_parked();

        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let trigger = visual
                .debug_bounds("hover-leak-select-trigger")
                .expect("select trigger should render");
            visual.simulate_click(trigger.center(), Modifiers::default());
            visual.run_until_parked();
        }

        cx.simulate_keystrokes(window.into(), "down enter");
        cx.run_until_parked();

        let value = window
            .update(cx, |view, _, _| view.value)
            .expect("test window should remain open");
        assert_eq!(value, Some("POST"));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("hover-leak-select-item-1").is_none(),
            "keyboard selection should dismiss the dropdown"
        );
    }

    #[test]
    fn editor_paint_style_uses_visible_caret_and_theme_selection() {
        for theme in [Theme::light(), Theme::dark()] {
            let style = editor_paint_style(theme);
            assert!(
                style.caret.a > 0.0,
                "gpui-base default caret is transparent"
            );
            assert!(
                style.selection.a > 0.0,
                "gpui-base default selection is transparent"
            );
            assert_eq!(style.foreground, theme.colors.text.primary.into());
            assert_eq!(style.caret, theme.colors.text.primary.into());
            assert_eq!(style.selection, theme.editor_selection());
            assert_ne!(style.selection, Hsla::default());
        }
    }

    #[test]
    fn single_line_replaces_line_breaks_with_spaces() {
        assert_eq!(single_line("abc"), SharedString::from("abc"));
        assert_eq!(single_line("a\nb\rc"), SharedString::from("a b c"));
        assert_eq!(
            single_line("{\n  \"name\": \"Milo\"\n}"),
            SharedString::from("{   \"name\": \"Milo\" }")
        );
    }

    #[test]
    fn variable_ranges_find_mustache_placeholders() {
        let value = "{{host}}/users/{{id}}";
        let ranges = variable_ranges(value);
        assert_eq!(ranges.len(), 2);
        assert_eq!(&value[ranges[0].0.clone()], "{{host}}");
        assert_eq!(ranges[0].1, "host");
        assert_eq!(&value[ranges[1].0.clone()], "{{id}}");
        assert_eq!(ranges[1].1, "id");
    }

    #[test]
    fn body_text_highlights_overlay_mustache_variables() {
        let theme = Theme::light();
        let value = "{\"host\":\"{{host}}\"}";
        let ranges = variable_ranges(value);
        let highlights = body_text_highlights(theme, &ranges);
        assert_eq!(highlights.len(), 1);
        assert_eq!(&value[highlights[0].range.clone()], "{{host}}");
        assert_eq!(
            highlights[0].style.color,
            Some(theme.colors.syntax.string.into())
        );
    }

    #[test]
    fn variable_highlight_runs_color_only_mustache_spans() {
        let value = "{{host}}/users";
        let ranges = variable_ranges(value)
            .into_iter()
            .map(|(range, _)| range)
            .collect::<Vec<_>>();
        let highlight = hsla(0.33, 0.6, 0.5, 1.0);
        let base = gpui::TextRun {
            len: value.len(),
            font: gpui::Font::default(),
            color: transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = variable_highlight_runs(value, &ranges, &base, highlight);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].color, highlight);
        assert_eq!(runs[0].len, "{{host}}".len());
        assert_eq!(runs[1].color, transparent_black());
        assert_eq!(runs[1].len, "/users".len());
    }

    #[test]
    fn input_text_scroll_offset_shifts_left_when_caret_overflows() {
        assert_eq!(
            input_text_scroll_offset(px(50.0), px(100.0)),
            px(0.0),
            "caret inside the field should not scroll"
        );
        assert_eq!(
            input_text_scroll_offset(px(200.0), px(100.0)),
            px(-102.0),
            "caret past the right edge should match gpui-base InputTextElement"
        );
    }

    struct HighlightHarness {
        input: Entity<InputState>,
    }

    impl Render for HighlightHarness {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            _cx: &mut Context<Self>,
        ) -> impl IntoElement {
            div()
        }
    }

    fn long_variable_url() -> SharedString {
        SharedString::from(format!(
            "{{{{sdfsdfsd}}}}{}",
            "kjlkjlkjlkjlkjlkjlkjflsdjflkjsdlfkjsldkjflskdjflkjlfjlsdj".repeat(2)
        ))
    }

    #[gpui::test]
    fn variable_highlight_scrolls_with_caret_at_end_of_long_url(cx: &mut TestAppContext) {
        cx.update(crate::theme::Theme::init);
        let value = long_variable_url();
        let window = cx.open_window(size(px(240.0), px(48.0)), |window, cx| HighlightHarness {
            input: cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(value.clone(), window, cx);
                input
            }),
        });
        let input = window
            .update(cx, |harness, _window, _cx| harness.input.clone())
            .expect("highlight test window should be open");
        let ranges = variable_ranges(&value)
            .into_iter()
            .map(|(range, _)| range)
            .collect();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let visible = size(px(160.0), px(24.0));
        let (_, prepaint) = visual.draw(point(px(0.0), px(0.0)), visible, |_, _| {
            VariableHighlightElement {
                state: input.clone(),
                value: value.clone(),
                ranges,
                highlight_color: hsla(0.33, 0.6, 0.5, 1.0),
                caret_color: hsla(0.0, 0.0, 1.0, 1.0),
                mask_color: hsla(0.0, 0.0, 0.2, 1.0),
            }
        });

        assert!(
            prepaint.scroll_offset < px(0.0),
            "long URL with caret at the end should scroll highlights left, got {:?}",
            prepaint.scroll_offset
        );
        assert_eq!(
            prepaint.scroll_offset,
            input_text_scroll_offset(prepaint.cursor_x, visible.width)
        );
    }

    #[gpui::test]
    fn variable_highlight_stays_at_origin_when_caret_is_at_start(cx: &mut TestAppContext) {
        cx.update(crate::theme::Theme::init);
        let value = long_variable_url();
        let window = cx.open_window(size(px(240.0), px(48.0)), |window, cx| HighlightHarness {
            input: cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(value.clone(), window, cx);
                input
            }),
        });
        let input = window
            .update(cx, |harness, _window, cx| {
                harness.input.update(cx, |input, cx| {
                    input.set_selected_range(0..0, cx);
                });
                harness.input.clone()
            })
            .expect("highlight test window should be open");
        let ranges = variable_ranges(&value)
            .into_iter()
            .map(|(range, _)| range)
            .collect();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let (_, prepaint) = visual.draw(
            point(px(0.0), px(0.0)),
            size(px(160.0), px(24.0)),
            |_, _| VariableHighlightElement {
                state: input,
                value,
                ranges,
                highlight_color: hsla(0.33, 0.6, 0.5, 1.0),
                caret_color: hsla(0.0, 0.0, 1.0, 1.0),
                mask_color: hsla(0.0, 0.0, 0.2, 1.0),
            },
        );

        assert_eq!(
            prepaint.scroll_offset,
            px(0.0),
            "caret at the start should keep the variable highlight at its origin"
        );
    }

    #[gpui::test]
    fn variable_highlight_shapes_multiline_value_without_panicking(cx: &mut TestAppContext) {
        cx.update(crate::theme::Theme::init);
        let value = SharedString::from("{{host}}\n/users");
        let window = cx.open_window(size(px(240.0), px(48.0)), |window, cx| HighlightHarness {
            input: cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(single_line(value.clone()), window, cx);
                input
            }),
        });
        let input = window
            .update(cx, |harness, _window, _cx| harness.input.clone())
            .expect("highlight test window should be open");
        let ranges = variable_ranges(&value)
            .into_iter()
            .map(|(range, _)| range)
            .collect();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let _ = visual.draw(
            point(px(0.0), px(0.0)),
            size(px(160.0), px(24.0)),
            |_, _| VariableHighlightElement {
                state: input,
                value,
                ranges,
                highlight_color: hsla(0.33, 0.6, 0.5, 1.0),
                caret_color: hsla(0.0, 0.0, 1.0, 1.0),
                mask_color: hsla(0.0, 0.0, 0.2, 1.0),
            },
        );
    }
}
