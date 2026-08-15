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
    App, AppContext as _, Bounds, ClickEvent, ContentMask, Context, Element, ElementId, Entity,
    GlobalElementId, Hsla, InspectorElementId, InteractiveElement as _, IntoElement, LayoutId,
    MouseButton, PaintQuad, ParentElement as _, Pixels, Render, ShapedLine, SharedString,
    StatefulInteractiveElement as _, Style, Styled as _, TextAlign, TextRun,
    UniformListScrollHandle, Window, div, fill, point, prelude::FluentBuilder as _, px, relative,
    size, transparent_black,
};

use crate::multiline_input::{MultilineInput, TextHighlight};
use crate::response_viewer::{
    HEADER_SEPARATOR, ResponseLine, SearchColumn, SearchMatch, SyntaxRole, highlight_json,
    join_display_lines, join_header_lines,
};
use crate::shell::PaneLayout;
use crate::theme::Theme;

/// Single-line label that shows an ellipsis when the available width is too small.
pub(crate) fn truncated_label(text: impl Into<String>) -> gpui::Div {
    div().min_w(px(0.0)).truncate().child(text.into())
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
        .text_color(theme.colors.selection.active_foreground)
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
                theme.colors.selection.active_foreground
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

pub(crate) fn search_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    on_value_change: impl Fn(SharedString, &mut Window, &mut gpui::Context<InputRuntime>) + 'static,
    on_enter: impl Fn(SharedString, &mut Window, &mut gpui::Context<InputRuntime>) + 'static,
) -> impl IntoElement {
    Input::new()
        .id(id)
        .value(single_line(value))
        .placeholder(placeholder)
        .on_value_change_with_context(on_value_change)
        .on_enter_with_context(on_enter)
        .h(px(theme.metrics.control_height - 4.0))
        .w(px(180.0))
        .px(px(theme.metrics.spacing_2))
        .flex()
        .items_center()
        .rounded(px(theme.metrics.radius_small))
        .font_family(theme.typography.interface_family)
        .text_size(px(theme.typography.caption_size))
        .text_color(theme.colors.text.primary)
        .style_with_state(move |state, input| {
            input
                .debug_selector(|| "response-search".into())
                .bg(theme.colors.surfaces.window)
                .border_1()
                .border_color(if state.focused {
                    theme.colors.borders.focused
                } else {
                    theme.colors.borders.standard
                })
        })
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
    let value = single_line(value);
    let input = Input::new()
        .id(id.clone())
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
    variable_input_overlay(theme, id, tooltip_id, input, value, variables)
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
                .overflow_hidden()
                .rounded(px(theme.metrics.radius_small))
                .text_color(theme.colors.text.primary)
                .style_with_state(move |state, item| {
                    item.debug_selector(move || format!("{id}-item-{index}"))
                        .when(state.highlighted, |item| {
                            item.bg(theme.colors.surfaces.sidebar)
                        })
                })
                .child(
                    SelectItemIndicator::new()
                        .keep_mounted(true)
                        .flex_none()
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
                .child(
                    SelectItemText::new()
                        .text(label)
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate(),
                ),
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
                .gap(px(theme.metrics.spacing_1))
                .overflow_hidden()
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
                .child(
                    SelectValue::new()
                        .placeholder("None")
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate(),
                )
                .child(
                    SelectIcon::new()
                        .flex_none()
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
                            .style_with_state(|_, popup| popup.occlude())
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
    json: bool,
    on_value_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let id = id.into();
    let tooltip_id =
        ElementId::NamedChild(Arc::new(id.clone()), SharedString::from("variable-tooltip"));
    let value = value.into();
    let ranges = variable_ranges(&value);
    let highlights = body_text_highlights(theme, &value, json, &ranges);
    let input = MultilineInput::new()
        .id(id)
        .value(value)
        .placeholder("Body content")
        .highlights(highlights)
        .on_value_change(move |value, window, cx| on_value_change(value, window, cx))
        .size_full()
        .min_h(px(120.0))
        .p(px(theme.metrics.spacing_3))
        .rounded(px(theme.metrics.radius_small))
        .font_family(theme.typography.monospace_family)
        .text_size(px(theme.typography.body_size))
        .text_color(theme.colors.text.primary)
        .style_with_state(move |focused, input| {
            input
                .bg(theme.colors.surfaces.window)
                .border_1()
                .border_color(if focused {
                    theme.colors.borders.focused
                } else {
                    theme.colors.borders.standard
                })
        });

    let wrapper = div()
        .id(tooltip_id)
        .relative()
        .size_full()
        .debug_selector(|| "variable-input-tooltip-trigger".into())
        .child(input);
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
    lines: &[ResponseLine],
    matches: &[SearchMatch],
    active_match: usize,
    scroll: UniformListScrollHandle,
    on_visible_range: impl Fn(std::ops::Range<usize>, &mut App) + 'static,
) -> gpui::AnyElement {
    let joined = join_display_lines(lines);
    let mut highlights = joined
        .syntax
        .into_iter()
        .map(|(range, role)| TextHighlight {
            range,
            color: Some(syntax_color(theme, role)),
            background: None,
        })
        .collect::<Vec<_>>();
    for (index, found) in matches.iter().enumerate() {
        if found.column != SearchColumn::Body {
            continue;
        }
        let Some(&offset) = joined.line_offsets.get(found.row) else {
            continue;
        };
        let active = index == active_match;
        highlights.push(TextHighlight {
            range: offset + found.range.start..offset + found.range.end,
            color: active.then(|| theme.colors.selection.active_foreground.into()),
            background: Some(if active {
                theme.colors.selection.active_background.into()
            } else {
                theme.colors.selection.inactive_background.into()
            }),
        });
    }
    MultilineInput::new()
        .id(id)
        .value(joined.text)
        .read_only()
        .highlights(highlights)
        .track_scroll(scroll)
        .on_visible_range(on_visible_range)
        .size_full()
        .min_h(px(0.0))
        .font_family(theme.typography.monospace_family)
        .text_size(px(theme.typography.body_size))
        .text_color(theme.colors.syntax.plain)
        .into_any_element()
}

pub(crate) fn response_headers_input(
    theme: Theme,
    id: impl Into<ElementId>,
    headers: &[probe_http::ResponseHeader],
    matches: &[SearchMatch],
    active_match: usize,
    scroll: UniformListScrollHandle,
    on_visible_range: impl Fn(std::ops::Range<usize>, &mut App) + 'static,
) -> gpui::AnyElement {
    let joined = join_header_lines(headers);
    let mut highlights = Vec::new();
    for (offset, name_len) in joined.line_offsets.iter().zip(&joined.name_lens) {
        highlights.push(TextHighlight {
            range: *offset..*offset + name_len,
            color: Some(theme.colors.text.secondary.into()),
            background: None,
        });
    }
    for (index, found) in matches.iter().enumerate() {
        let Some(&line_start) = joined.line_offsets.get(found.row) else {
            continue;
        };
        let range = match found.column {
            SearchColumn::HeaderName => {
                line_start + found.range.start..line_start + found.range.end
            }
            SearchColumn::HeaderValue => {
                let value_start = line_start + joined.name_lens[found.row] + HEADER_SEPARATOR.len();
                value_start + found.range.start..value_start + found.range.end
            }
            SearchColumn::Body => continue,
        };
        let active = index == active_match;
        highlights.push(TextHighlight {
            range,
            color: active.then(|| theme.colors.selection.active_foreground.into()),
            background: Some(if active {
                theme.colors.selection.active_background.into()
            } else {
                theme.colors.selection.inactive_background.into()
            }),
        });
    }
    MultilineInput::new()
        .id(id)
        .value(joined.text)
        .read_only()
        .highlights(highlights)
        .track_scroll(scroll)
        .on_visible_range(on_visible_range)
        .size_full()
        .min_h(px(0.0))
        .font_family(theme.typography.monospace_family)
        .text_size(px(theme.typography.body_size))
        .text_color(theme.colors.text.primary)
        .into_any_element()
}

fn body_text_highlights(
    theme: Theme,
    value: &str,
    json: bool,
    variables: &[(std::ops::Range<usize>, String)],
) -> Vec<TextHighlight> {
    let mut highlights = Vec::new();
    if json {
        highlights.extend(
            highlight_json(value)
                .into_iter()
                .map(|(range, role)| TextHighlight {
                    range,
                    color: Some(syntax_color(theme, role)),
                    background: None,
                }),
        );
    }
    for (range, _) in variables {
        highlights.push(TextHighlight {
            range: range.clone(),
            color: Some(theme.colors.syntax.string.into()),
            background: None,
        });
    }
    highlights
}

fn syntax_color(theme: Theme, role: SyntaxRole) -> Hsla {
    let color = match role {
        SyntaxRole::Property => theme.colors.syntax.property,
        SyntaxRole::String => theme.colors.syntax.string,
        SyntaxRole::Number => theme.colors.syntax.number,
        SyntaxRole::Boolean => theme.colors.syntax.boolean,
        SyntaxRole::Null => theme.colors.syntax.null,
        SyntaxRole::Punctuation => theme.colors.syntax.punctuation,
    };
    color.into()
}

fn variable_input_overlay(
    theme: Theme,
    input_id: ElementId,
    tooltip_id: ElementId,
    input: Input,
    value: SharedString,
    variables: VariableContext,
) -> gpui::AnyElement {
    let ranges = variable_ranges(&value);
    let wrapper = div()
        .id(tooltip_id)
        .relative()
        .debug_selector(|| "variable-input-tooltip-trigger".into())
        .w_full();
    if ranges.is_empty() {
        return wrapper.child(input).into_any_element();
    }

    // Input paints first so it keeps native caret, selection, and scroll.
    // The overlay sits on top and recolors only `{{variable}}` spans; the
    // previous behind-the-input layer was covered by the field background.
    let wrapper = wrapper.child(input).child(variable_highlight_layer(
        theme,
        input_id,
        value,
        ranges.clone(),
    ));

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

/// Must match the keyed-state child id used by `base_gpui` `Input`.
const INPUT_RUNTIME_STATE_KEY: &str = "state";

fn variable_highlight_layer(
    theme: Theme,
    input_id: ElementId,
    value: SharedString,
    ranges: Vec<(Range<usize>, String)>,
) -> impl IntoElement {
    let highlight_color = theme.colors.syntax.string.into();
    let caret_color = theme.colors.text.primary.into();
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
        .font_family(theme.typography.monospace_family)
        .text_size(px(theme.typography.body_size))
        .debug_selector(|| "variable-highlight-overlay".into())
        .child(VariableHighlightElement {
            input_id: Some(input_id),
            state: None,
            value,
            ranges: ranges.into_iter().map(|(range, _)| range).collect(),
            highlight_color,
            caret_color,
        })
}

struct VariableHighlightElement {
    input_id: Option<ElementId>,
    state: Option<Entity<InputRuntime>>,
    value: SharedString,
    ranges: Vec<Range<usize>>,
    highlight_color: Hsla,
    caret_color: Hsla,
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
        let state = self.input_runtime(window, cx);
        let cursor = state.read(cx).cursor_offset();
        let selected_range = state.read(cx).selected_range();
        let focused = state.read(cx).is_focused(window);
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
            Some(fill(
                Bounds::new(
                    point(bounds.left() + scroll_offset + cursor_x, bounds.top()),
                    size(px(1.0), bounds.bottom() - bounds.top()),
                ),
                self.caret_color,
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

impl VariableHighlightElement {
    fn input_runtime(&self, window: &mut Window, cx: &mut App) -> Entity<InputRuntime> {
        if let Some(state) = self.state.clone() {
            return state;
        }
        let input_id = self
            .input_id
            .clone()
            .expect("variable highlight needs the input id or a runtime");
        let value = self.value.clone();
        // `Input::render` looks up keyed state from inside ViewElement's
        // `type_name::<Input>()` namespace. Replay that namespace so this
        // sibling reads the same `InputRuntime` the field is editing.
        window.with_id(
            ElementId::Name(std::any::type_name::<Input>().into()),
            |window| {
                let state_id = ElementId::NamedChild(
                    Arc::new(input_id),
                    SharedString::from(INPUT_RUNTIME_STATE_KEY),
                );
                window.use_keyed_state(state_id, cx, |window, cx| {
                    InputRuntime::new(value, window, cx)
                })
            },
        )
    }
}

/// Matches `InputTextElement` in gpui-base: shift the painted line left when the
/// caret would otherwise sit past the visible width.
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
            ButtonRoot::new()
                .id(id)
                .w_full()
                .h(px(32.0))
                .px(px(theme.metrics.spacing_3))
                .flex()
                .items_center()
                .justify_start()
                .overflow_hidden()
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
    use base_gpui::{
        popover::{PopoverPopup, PopoverPortal, PopoverPositioner, PopoverRoot, PopoverTrigger},
        primitives::input::{InputHome, InputRuntime},
    };
    use gpui::{
        AppContext as _, Context, Entity, IntoElement, Modifiers, Render, SharedString,
        TestAppContext, VisualTestContext, div, hsla, point, prelude::*, px, size,
        transparent_black,
    };

    use super::{
        VariableHighlightElement, body_text_highlights, dropdown, input_text_scroll_offset,
        menu_button, single_line, variable_highlight_runs, variable_ranges,
    };
    use crate::response_viewer::{SyntaxRole, highlight_json};
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
                                        popup.debug_selector(|| "menu-test-popup".into()).occlude()
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
        cx.update(base_gpui::init);
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
    fn json_body_highlights_tokens_and_lets_variables_override() {
        let theme = Theme::light();
        let value = "{\"host\":\"{{host}}\"}";
        let ranges = variable_ranges(value);
        let highlights = body_text_highlights(theme, value, true, &ranges);
        assert!(highlights.iter().any(|highlight| {
            highlight.range == (1..7)
                && highlight.color == Some(theme.colors.syntax.property.into())
        }));
        let variable = highlights
            .iter()
            .rev()
            .find(|highlight| &value[highlight.range.clone()] == "{{host}}")
            .expect("variable highlight");
        assert_eq!(variable.color, Some(theme.colors.syntax.string.into()));
        let roles_from_json = highlight_json(value);
        assert!(
            roles_from_json
                .iter()
                .any(|(_, role)| *role == SyntaxRole::Property)
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
        input: Entity<InputRuntime>,
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
        let value = long_variable_url();
        let window = cx.open_window(size(px(240.0), px(48.0)), |window, cx| HighlightHarness {
            input: cx.new(|cx| InputRuntime::new(value.clone(), window, cx)),
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
                input_id: None,
                state: Some(input.clone()),
                value: value.clone(),
                ranges,
                highlight_color: hsla(0.33, 0.6, 0.5, 1.0),
                caret_color: hsla(0.0, 0.0, 1.0, 1.0),
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
        let value = long_variable_url();
        let window = cx.open_window(size(px(240.0), px(48.0)), |window, cx| HighlightHarness {
            input: cx.new(|cx| InputRuntime::new(value.clone(), window, cx)),
        });
        let input = window
            .update(cx, |harness, window, cx| {
                harness.input.update(cx, |input, cx| {
                    input.home(&InputHome, window, cx);
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
                input_id: None,
                state: Some(input),
                value,
                ranges,
                highlight_color: hsla(0.33, 0.6, 0.5, 1.0),
                caret_color: hsla(0.0, 0.0, 1.0, 1.0),
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
        let value = SharedString::from("{{host}}\n/users");
        let window = cx.open_window(size(px(240.0), px(48.0)), |window, cx| HighlightHarness {
            input: cx.new(|cx| InputRuntime::new(single_line(value.clone()), window, cx)),
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
                input_id: None,
                state: Some(input),
                value,
                ranges,
                highlight_color: hsla(0.33, 0.6, 0.5, 1.0),
                caret_color: hsla(0.0, 0.0, 1.0, 1.0),
            },
        );
    }
}
