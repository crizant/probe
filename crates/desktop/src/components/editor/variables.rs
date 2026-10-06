use super::*;
use gpui::UnderlineStyle;
use probe_core::VariableStatus;

#[allow(clippy::too_many_arguments)]
pub(in crate::components) fn variable_input_overlay(
    theme: Theme,
    state: Entity<InputState>,
    tooltip_id: ElementId,
    input: impl IntoElement,
    value: SharedString,
    variables: VariableContext,
    highlight_path_variables: bool,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyElement {
    let ranges = input_variable_ranges(&value, highlight_path_variables);
    let tooltip_ranges = variable_ranges(&value);
    let tooltip_geometry = (!tooltip_ranges.is_empty()).then(|| {
        let hover = window.use_keyed_state(
            ElementId::NamedChild(Arc::new(tooltip_id.clone()), SharedString::from("hover")),
            cx,
            VariableHoverState::new,
        );
        let origin = hover.read(cx).overlay_origin;
        InputTooltipGeometry {
            hover,
            ranges: tooltip_ranges
                .iter()
                .map(|reference| {
                    (
                        reference.range.clone(),
                        input_tooltip_bounds(state.read(cx), &reference.range, origin),
                    )
                })
                .collect(),
        }
    });
    // Input paints first so it keeps native caret, selection, and scroll.
    // The overlay sits on top and recolors supported variable spans.
    let mut wrapper = div()
        .id(tooltip_id.clone())
        .relative()
        .debug_selector(|| "variable-input-tooltip-trigger".into())
        .w_full()
        .child(input)
        .child(variable_highlight_layer(
            theme,
            state.clone(),
            ranges.is_empty(),
            theme.typography.monospace_family,
            theme.typography.body_size,
            highlight_path_variables,
            variables.clone(),
            tooltip_geometry.clone(),
        ));
    let Some(geometry) = tooltip_geometry else {
        return wrapper.into_any_element();
    };
    let hover = geometry.hover;
    let mut hits = div()
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .overflow_hidden()
        .on_prepaint({
            let hover = hover.clone();
            move |bounds, _, cx| {
                hover.update(cx, |state, _| state.overlay_origin = Some(bounds.origin));
            }
        });
    for (index, (reference, (_, bounds))) in tooltip_ranges.iter().zip(&geometry.ranges).enumerate()
    {
        let Some(bounds) = bounds else { continue };
        hits = hits.child(variable_hover_hit(
            ("variable-hover", index),
            index,
            reference.name(&value).to_owned(),
            hover.clone(),
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width.max(px(1.0)),
            Some(bounds.size.height.max(px(1.0))),
            if index == 0 {
                "variable-hover-trigger".into()
            } else {
                format!("variable-hover-trigger-{index}")
            },
        ));
    }
    wrapper = wrapper.child(hits);

    with_variable_tooltip(
        wrapper,
        theme,
        hover,
        variables,
        state.read(cx).focus_handle(cx),
        cx,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn variable_editor_overlay(
    theme: Theme,
    state: Entity<EditorState>,
    tooltip_id: ElementId,
    editor: impl IntoElement,
    value: SharedString,
    variables: VariableContext,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyElement {
    let ranges = variable_ranges(&value);
    let mut wrapper = div()
        .id(tooltip_id.clone())
        .relative()
        .size_full()
        .min_h(px(0.0))
        .child(editor);
    if ranges.is_empty() {
        return wrapper.into_any_element();
    }

    let hover = window.use_keyed_state(
        ElementId::NamedChild(Arc::new(tooltip_id), SharedString::from("hover")),
        cx,
        VariableHoverState::new,
    );
    let overlay_origin = hover.read(cx).overlay_origin;
    let mut hits = div()
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .overflow_hidden()
        .on_prepaint({
            let hover = hover.clone();
            move |bounds, window, cx| {
                let origin = bounds.origin;
                let changed = hover.update(cx, |state, _| {
                    let changed = state.overlay_origin != Some(origin);
                    state.overlay_origin = Some(origin);
                    changed
                });
                if changed {
                    window.request_animation_frame();
                }
            }
        });
    if let Some(origin) = overlay_origin {
        let editor = state.read(cx);
        if editor.visible_row_range().is_none() {
            window.request_animation_frame();
        }
        for (index, reference) in ranges.iter().enumerate() {
            let Some(bounds) = editor.range_to_bounds(&reference.range) else {
                continue;
            };
            hits = hits.child(variable_hover_hit(
                ("body-variable-hover", index),
                index,
                reference.name(&value).to_owned(),
                hover.clone(),
                bounds.origin.x - origin.x,
                bounds.origin.y - origin.y,
                bounds.size.width.max(px(1.0)),
                Some(bounds.size.height.max(px(1.0))),
                if index == 0 {
                    "body-variable-hover-trigger".into()
                } else {
                    format!("body-variable-hover-trigger-{index}")
                },
            ));
        }
    } else {
        window.request_animation_frame();
    }
    wrapper = wrapper.child(hits);
    with_variable_tooltip(
        wrapper,
        theme,
        hover,
        variables,
        state.read(cx).focus_handle(cx),
        cx,
    )
}

#[allow(clippy::too_many_arguments)]
fn variable_hover_hit(
    id: impl Into<ElementId>,
    index: usize,
    name: String,
    hover: Entity<VariableHoverState>,
    left: Pixels,
    top: Pixels,
    width: Pixels,
    height: Option<Pixels>,
    debug_selector: String,
) -> gpui::Stateful<gpui::Div> {
    let hover_trigger = hover.clone();
    div()
        .id(id)
        .absolute()
        .left(left)
        .top(top)
        .w(width)
        .when_some(height, |hit, height| hit.h(height))
        .debug_selector(move || debug_selector.clone())
        .on_hover({
            let hover = hover_trigger.clone();
            move |hovered, _, cx| {
                hover.update(cx, |state, cx| {
                    state.on_trigger_hover(index, name.clone(), *hovered, cx);
                });
            }
        })
        .on_prepaint({
            let hover = hover_trigger;
            move |bounds, window, cx| {
                let changed = hover.update(cx, |state, _| {
                    if state
                        .active
                        .as_ref()
                        .is_none_or(|(active, _)| *active != index)
                    {
                        return false;
                    }
                    let changed = state.trigger_bounds != bounds;
                    state.trigger_bounds = bounds;
                    changed
                });
                if changed {
                    window.request_animation_frame();
                }
            }
        })
}

fn with_variable_tooltip(
    wrapper: gpui::Stateful<gpui::Div>,
    theme: Theme,
    hover: Entity<VariableHoverState>,
    variables: VariableContext,
    editor_focus: FocusHandle,
    cx: &App,
) -> gpui::AnyElement {
    let (open, active, bounds) = {
        let state = hover.read(cx);
        (state.open, state.active.clone(), state.trigger_bounds)
    };
    if !open {
        return wrapper.into_any_element();
    }
    let Some((_, name)) = active else {
        return wrapper.into_any_element();
    };
    if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
        return wrapper.into_any_element();
    }
    let presentation = variable_tooltip_presentation(&name, &variables);
    let value_input = hover.read(cx).value_input.clone();
    *hover.read(cx).on_value_change.borrow_mut() = variables.on_change.clone();
    wrapper
        .child(
            deferred(
                Positioner::side(bounds)
                    .placement(Placement::Bottom)
                    .align(Align::Start)
                    .offset(px(4.0))
                    .margin(px(4.0))
                    .child(variable_tooltip_popup(
                        theme,
                        name,
                        presentation,
                        hover,
                        value_input,
                        variables,
                        editor_focus,
                    )),
            )
            .with_priority(POPUP_PRIORITY + 1),
        )
        .into_any_element()
}

#[derive(Clone)]
pub(in crate::components) struct InputTooltipGeometry {
    hover: Entity<VariableHoverState>,
    ranges: Vec<(Range<usize>, Option<Bounds<Pixels>>)>,
}

fn input_tooltip_bounds(
    input: &InputState,
    range: &Range<usize>,
    origin: Option<Point<Pixels>>,
) -> Option<Bounds<Pixels>> {
    let origin = origin?;
    let mut bounds = input.range_to_bounds(range)?;
    bounds.origin -= origin;
    Some(bounds)
}

#[allow(clippy::too_many_arguments)]
fn variable_highlight_layer(
    theme: Theme,
    state: Entity<InputState>,
    ranges_empty: bool,
    font_family: &'static str,
    text_size: f32,
    highlight_path_variables: bool,
    variables: VariableContext,
    tooltip_geometry: Option<InputTooltipGeometry>,
) -> impl IntoElement {
    let base_color = if ranges_empty {
        transparent_black()
    } else {
        theme.colors.text.primary.into()
    };
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
        .when(!ranges_empty, |layer| {
            layer.debug_selector(|| "variable-highlight-overlay".into())
        })
        .child(VariableHighlightElement {
            state,
            base_color,
            palette: variable_highlight_palette(theme),
            highlight_path_variables,
            variables,
            tooltip_geometry,
        })
}

pub(in crate::components) struct VariableHighlightElement {
    pub(in crate::components) state: Entity<InputState>,
    pub(in crate::components) base_color: Hsla,
    pub(in crate::components) palette: VariableHighlightPalette,
    pub(in crate::components) highlight_path_variables: bool,
    pub(in crate::components) variables: VariableContext,
    pub(in crate::components) tooltip_geometry: Option<InputTooltipGeometry>,
}

pub(in crate::components) struct VariableHighlightPrepaintState {
    line: Option<ShapedLine>,
    pub(in crate::components) scroll_offset: Pixels,
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
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self.state.clone();
        let value = single_line(state.read(cx).value());
        let references = input_variable_ranges(&value, self.highlight_path_variables);
        let style = window.text_style();
        let run = TextRun {
            len: value.len(),
            font: style.font(),
            color: self.base_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let variables = &self.variables;
        let runs =
            variable_highlight_runs(&value, &references, &run, self.palette, |kind, name| {
                placeholder_tone(variables, kind, name)
            });
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(value, font_size, &runs, None);
        VariableHighlightPrepaintState {
            line: Some(line),
            scroll_offset: px(0.0),
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
        // Native Input paints first and owns caret following and clamping. Manual
        // scrolling may intentionally leave the caret outside the visible viewport.
        let scroll_offset = self.state.read(cx).scroll_offset().x;
        // Input publishes layout during paint. Compare native ranges relative to
        // the overlay so moving a row does not itself schedule another render.
        if let Some(geometry) = &self.tooltip_geometry {
            let origin = geometry.hover.read(cx).overlay_origin;
            if geometry.ranges.iter().any(|(range, bounds)| {
                input_tooltip_bounds(self.state.read(cx), range, origin) != *bounds
            }) {
                let view = window.current_view();
                cx.defer(move |cx| cx.notify(view));
            }
        }
        prepaint.scroll_offset = scroll_offset;
        let line = prepaint.line.take();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(line) = line {
                line.paint(
                    bounds.origin + point(scroll_offset, px(0.0)),
                    window.line_height(),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .expect("variable highlight text should paint");
            }
        });
    }
}

/// Recolor supported variable spans while preserving the native text runs.
pub(in crate::components) fn variable_highlight_runs(
    value: &str,
    references: &[VariableReference],
    base: &TextRun,
    palette: VariableHighlightPalette,
    mut tone_for: impl FnMut(ReferenceKind, &str) -> PlaceholderTone,
) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut ix = 0;
    for reference in references {
        // URL highlights can nest, as in `{{a:b}}` plus `:b`. The earlier span
        // keeps the overlap; a fully covered span is not painted.
        let start = reference.range.start.min(value.len()).max(ix);
        let end = reference.range.end.min(value.len());
        if ix < start {
            runs.push(TextRun {
                len: start - ix,
                color: base.color,
                underline: base.underline,
                ..base.clone()
            });
        }
        if start >= end {
            continue;
        }
        let (color, underline) =
            placeholder_paint(tone_for(reference.kind, reference.name(value)), palette);
        runs.push(TextRun {
            len: end - start,
            color,
            underline,
            ..base.clone()
        });
        ix = end;
    }
    if ix < value.len() {
        runs.push(TextRun {
            len: value.len() - ix,
            color: base.color,
            underline: base.underline,
            ..base.clone()
        });
    }
    runs.retain(|run| run.len > 0);
    if runs.is_empty() {
        runs.push(base.clone());
    }
    runs
}

pub(crate) fn single_line(value: impl Into<SharedString>) -> SharedString {
    let value = value.into();
    if value.find(['\n', '\r']).is_none() {
        value
    } else {
        SharedString::from(value.replace(['\n', '\r'], " "))
    }
}

pub(in crate::components) fn variable_tooltip_presentation(
    name: &str,
    variables: &VariableContext,
) -> VariableTooltipPresentation {
    if variables.unknown_secrets.contains(name) {
        return VariableTooltipPresentation::secret(SecretTooltipState::Unknown);
    }
    match variables.status(name) {
        VariableStatus::Resolved if variables.resolved_secrets.contains(name) => {
            VariableTooltipPresentation::secret(SecretTooltipState::Stored)
        }
        VariableStatus::Resolved => VariableTooltipPresentation {
            value: variables
                .values
                .get(name)
                .cloned()
                .unwrap_or_else(String::new),
            placeholder: "Variable value",
            editable: variables.on_change.is_some(),
            hint: None,
            secret: None,
        },
        VariableStatus::SecretWithoutValue => {
            VariableTooltipPresentation::secret(SecretTooltipState::NotStored)
        }
        VariableStatus::Missing if variables.on_change.is_some() => VariableTooltipPresentation {
            value: String::new(),
            placeholder: "Enter a value to create",
            editable: true,
            hint: Some("Not defined in this environment"),
            secret: None,
        },
        VariableStatus::Missing => unavailable_variable_tooltip(&variables.unavailable_message),
    }
}

fn unavailable_variable_tooltip(message: &str) -> VariableTooltipPresentation {
    VariableTooltipPresentation {
        value: message.to_owned(),
        placeholder: "Variable value",
        editable: false,
        hint: None,
        secret: None,
    }
}

pub(in crate::components) struct VariableTooltipPresentation {
    pub(in crate::components) value: String,
    pub(in crate::components) placeholder: &'static str,
    pub(in crate::components) editable: bool,
    pub(in crate::components) hint: Option<&'static str>,
    pub(in crate::components) secret: Option<SecretTooltipState>,
}

impl VariableTooltipPresentation {
    fn secret(state: SecretTooltipState) -> Self {
        Self {
            value: String::new(),
            placeholder: "Secret value",
            editable: false,
            hint: None,
            secret: Some(state),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::components) enum SecretTooltipState {
    Stored,
    NotStored,
    Unknown,
}

impl SecretTooltipState {
    pub(in crate::components) fn status_text(self) -> &'static str {
        match self {
            Self::Stored => "● Stored securely",
            Self::NotStored => "○ Not set",
            Self::Unknown => "Not verified",
        }
    }

    pub(in crate::components) fn status_color(self, theme: Theme) -> gpui::Rgba {
        match self {
            Self::Stored => theme.colors.status.success,
            Self::NotStored | Self::Unknown => theme.colors.text.muted,
        }
    }

    pub(in crate::components) fn action_label(self) -> &'static str {
        match self {
            Self::Stored => "Replace Secret…",
            Self::NotStored | Self::Unknown => "Set Secret…",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::components) enum ReferenceKind {
    Environment,
    Path,
}

/// A placeholder whose highlight range and lookup name are spans of the scanned value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::components) struct VariableReference {
    pub(in crate::components) range: Range<usize>,
    /// Trimmed `{{ name }}` lookup, or the path-parameter name after `:`.
    pub(in crate::components) name: Range<usize>,
    pub(in crate::components) kind: ReferenceKind,
}

impl VariableReference {
    pub(in crate::components) fn name<'a>(&self, value: &'a str) -> &'a str {
        &value[self.name.clone()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::components) struct VariableHighlightPalette {
    pub(in crate::components) resolved: Hsla,
    pub(in crate::components) unresolved: Hsla,
    pub(in crate::components) neutral: Hsla,
}

pub(in crate::components) fn variable_highlight_palette(theme: Theme) -> VariableHighlightPalette {
    VariableHighlightPalette {
        resolved: theme.colors.syntax.string.into(),
        unresolved: theme.colors.status.error.into(),
        neutral: theme.colors.text.secondary.into(),
    }
}

pub(in crate::components) fn reference_status(
    variables: &VariableContext,
    kind: ReferenceKind,
    name: &str,
) -> VariableStatus {
    match kind {
        ReferenceKind::Environment => variables.status(name),
        ReferenceKind::Path => variables.path_status(name),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::components) enum PlaceholderTone {
    Resolved,
    Unresolved,
    Neutral,
}

pub(in crate::components) fn placeholder_tone(
    variables: &VariableContext,
    kind: ReferenceKind,
    name: &str,
) -> PlaceholderTone {
    if kind == ReferenceKind::Environment && variables.unknown_secrets.contains(name) {
        return PlaceholderTone::Neutral;
    }
    if reference_status(variables, kind, name).is_resolved() {
        PlaceholderTone::Resolved
    } else {
        PlaceholderTone::Unresolved
    }
}

pub(in crate::components) fn placeholder_paint(
    tone: PlaceholderTone,
    palette: VariableHighlightPalette,
) -> (Hsla, Option<UnderlineStyle>) {
    match tone {
        PlaceholderTone::Resolved => (palette.resolved, None),
        PlaceholderTone::Neutral => (palette.neutral, None),
        PlaceholderTone::Unresolved => (
            palette.unresolved,
            Some(UnderlineStyle {
                thickness: px(1.0),
                color: Some(palette.unresolved),
                wavy: false,
            }),
        ),
    }
}

pub(in crate::components) fn variable_ranges(value: &str) -> Vec<VariableReference> {
    let mut ranges = Vec::new();
    let mut offset = 0;
    let mut remaining = value;
    while let Some(start) = remaining.find("{{") {
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("}}") else {
            break;
        };
        let raw_name = &after_start[..end];
        let name = raw_name.trim();
        if !name.is_empty() && !name.contains("{{") {
            let range_start = offset + start;
            let leading = raw_name.len() - raw_name.trim_start().len();
            let name_start = range_start + 2 + leading;
            ranges.push(VariableReference {
                range: range_start..range_start + 2 + end + 2,
                name: name_start..name_start + name.len(),
                kind: ReferenceKind::Environment,
            });
        }
        let consumed = start + 2 + end + 2;
        offset += consumed;
        remaining = &remaining[consumed..];
    }
    ranges
}

pub(in crate::components) fn input_variable_ranges(
    value: &str,
    highlight_path_variables: bool,
) -> Vec<VariableReference> {
    let mut ranges = variable_ranges(value);
    if highlight_path_variables {
        ranges.extend(
            path_variable_spans(value)
                .into_iter()
                .map(|span| VariableReference {
                    range: span.range,
                    name: span.name,
                    kind: ReferenceKind::Path,
                }),
        );
        ranges.sort_by_key(|reference| reference.range.start);
    }
    ranges
}
