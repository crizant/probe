use std::time::{Duration, Instant};

use super::*;
use crate::components::surfaces::TextContextLabelHandler;
use gpui::TouchPhase;

#[derive(Clone, Copy)]
pub(in crate::components) struct EditorInsets {
    pub(in crate::components) top: f32,
    pub(in crate::components) right: f32,
    pub(in crate::components) bottom: f32,
    pub(in crate::components) left: f32,
}

impl EditorInsets {
    pub(in crate::components) fn standard(theme: Theme) -> Self {
        Self {
            top: theme.metrics.spacing_2,
            right: theme.metrics.spacing_2,
            bottom: theme.metrics.spacing_2,
            left: theme.metrics.spacing_2,
        }
    }

    pub(in crate::components) fn response(theme: Theme) -> Self {
        Self {
            top: theme.metrics.spacing_2,
            right: 2.0,
            bottom: theme.metrics.spacing_2,
            left: theme.metrics.spacing_1,
        }
    }

    pub(in crate::components) fn edges(self) -> Edges<Pixels> {
        Edges {
            top: px(self.top),
            right: px(self.right),
            bottom: px(self.bottom),
            left: px(self.left),
        }
    }
}

#[derive(Clone)]
pub(in crate::components) struct TextContextMenuExtraAction {
    pub(in crate::components) id: &'static str,
    pub(in crate::components) label: TextContextMenuLabel,
    pub(in crate::components) requires_selection: bool,
    pub(in crate::components) is_enabled: TextContextEnableHandler,
    pub(in crate::components) on_click: TextContextActionHandler,
}

#[derive(Clone)]
pub(in crate::components) enum TextContextMenuLabel {
    Static(&'static str),
    Dynamic(TextContextLabelHandler),
}

pub(crate) struct ResponseBodyInputOptions<'a> {
    pub(in crate::components) matches: &'a [SearchMatch],
    pub(in crate::components) active_match: usize,
    pub(in crate::components) inspection_reveal: Option<(Range<usize>, bool)>,
    pub(in crate::components) language: SharedString,
    pub(in crate::components) soft_wrap: bool,
    pub(in crate::components) on_visible_range: VisibleRangeHandler,
    pub(in crate::components) on_mouse_down: EditorMouseDownHandler,
    pub(in crate::components) inspect_enabled: TextContextEnableHandler,
    pub(in crate::components) on_inspect: TextContextActionHandler,
}

impl<'a> ResponseBodyInputOptions<'a> {
    pub(crate) fn new(
        matches: &'a [SearchMatch],
        active_match: usize,
        language: impl Into<SharedString>,
        on_visible_range: impl Fn(Range<usize>, &mut App) + 'static,
        on_mouse_down: impl Fn(&mut Window, &mut App) + 'static,
        inspect_enabled: impl Fn(Option<&str>, usize) -> bool + 'static,
        on_inspect: impl Fn(Option<String>, usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            matches,
            active_match,
            inspection_reveal: None,
            language: language.into(),
            soft_wrap: true,
            on_visible_range: Rc::new(on_visible_range),
            on_mouse_down: Rc::new(on_mouse_down),
            inspect_enabled: Rc::new(inspect_enabled),
            on_inspect: Rc::new(on_inspect),
        }
    }

    pub(crate) fn inspection_reveal(
        mut self,
        inspection_reveal: Option<(Range<usize>, bool)>,
    ) -> Self {
        self.inspection_reveal = inspection_reveal;
        self
    }

    pub(crate) fn soft_wrap(mut self, soft_wrap: bool) -> Self {
        self.soft_wrap = soft_wrap;
        self
    }
}

pub(in crate::components) type VisibleRangeHandler = Rc<dyn Fn(Range<usize>, &mut App)>;
type FieldFocusHandler = Rc<dyn Fn(Entity<FieldInput>, bool, &mut App)>;
type FocusChangeHandler = Rc<dyn Fn(bool, &mut App)>;

pub(crate) struct FieldInput {
    state: Entity<InputState>,
    on_change: Option<InputChangeHandler>,
    on_enter: Option<InputChangeHandler>,
    on_focus: Option<FocusChangeHandler>,
    on_field_focus: Option<FieldFocusHandler>,
    autofocused: bool,
    _subscription: Subscription,
}

impl FieldInput {
    pub(crate) fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.state.read(cx).focus_handle(cx).is_focused(window)
    }

    fn on_event(
        this: &mut Self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                if let Some(on_change) = this.on_change.clone() {
                    let value = input.read(cx).value();
                    on_change(value, window, cx);
                }
            }
            InputEvent::PressEnter { .. } => {
                if let Some(on_enter) = this.on_enter.clone() {
                    let value = input.read(cx).value();
                    on_enter(value, window, cx);
                }
            }
            InputEvent::Focus => {
                if let Some(handler) = &this.on_field_focus {
                    handler(cx.entity(), true, cx);
                }
                if let Some(on_focus) = &this.on_focus {
                    on_focus(true, cx);
                }
            }
            InputEvent::Blur => {
                // A virtualized editor leaves the dispatch tree without losing its focus ID.
                if input.read(cx).focus_handle(cx).is_focused(window) {
                    return;
                }
                if let Some(handler) = &this.on_field_focus {
                    handler(cx.entity(), false, cx);
                }
                if let Some(on_focus) = &this.on_focus {
                    on_focus(false, cx);
                }
            }
        }
    }
}

#[derive(IntoElement)]
pub(crate) struct ProbeTextInput {
    pub(in crate::components) theme: Theme,
    pub(in crate::components) id: ElementId,
    pub(in crate::components) value: SharedString,
    pub(in crate::components) placeholder: SharedString,
    pub(in crate::components) variables: VariableContext,
    pub(in crate::components) highlight_path_variables: bool,
    pub(in crate::components) variable_overlay: bool,
    pub(in crate::components) font_family: &'static str,
    pub(in crate::components) text_size: f32,
    pub(in crate::components) height: f32,
    pub(in crate::components) width: Option<f32>,
    pub(in crate::components) debug_selector: Option<&'static str>,
    pub(in crate::components) on_change: Option<InputChangeHandler>,
    pub(in crate::components) on_enter: Option<InputChangeHandler>,
    pub(in crate::components) on_focus: Option<FocusChangeHandler>,
    pub(in crate::components) autofocus: bool,
    pub(in crate::components) readonly: bool,
    persistent_field: Option<Entity<FieldInput>>,
    on_field_focus: Option<FieldFocusHandler>,
    pub(in crate::components) shared_input: Option<Entity<InputState>>,
    pub(in crate::components) flat: bool,
    pub(in crate::components) leading_icon: Option<gpui::Div>,
    pub(in crate::components) content_gap: f32,
    pub(in crate::components) quiet_focus: bool,
    pub(in crate::components) focus_on_render: bool,
    pub(in crate::components) list_scroll: Option<ListScroll>,
}

impl RenderOnce for ProbeTextInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let component_id = self.id.clone();
        let context_menu = window.use_keyed_state(
            text_context_menu_id(&component_id, "context-menu-state"),
            cx,
            |_, _| TextContextMenuState::default(),
        );
        let overlay_paints_text = self.variable_overlay
            && !input_variable_ranges(&self.value, self.highlight_path_variables).is_empty();
        let placeholder = self.placeholder.clone();
        let on_change = self.on_change.clone();
        let on_enter = self.on_enter.clone();
        let state = if let Some(state) = self.shared_input.clone() {
            state
        } else {
            let field = self.persistent_field.clone().unwrap_or_else(|| {
                window.use_keyed_state(self.id.clone(), cx, |window, cx| {
                    let state = cx.new(|cx| {
                        let mut state =
                            InputState::new(window, cx).placeholder(placeholder.clone());
                        state.set_editor_style(editor_paint_style(self.theme));
                        state
                    });
                    let subscription = cx.subscribe_in(&state, window, FieldInput::on_event);
                    FieldInput {
                        state,
                        on_change: on_change.clone(),
                        on_enter: on_enter.clone(),
                        on_focus: None,
                        on_field_focus: None,
                        autofocused: false,
                        _subscription: subscription,
                    }
                })
            });
            field.update(cx, |field, _| {
                field.on_change = self.on_change.clone();
                field.on_enter = self.on_enter.clone();
                field.on_focus = self.on_focus.clone();
                field.on_field_focus = self.on_field_focus.clone();
            });
            if self.autofocus && !field.read(cx).autofocused {
                field.update(cx, |field, _| field.autofocused = true);
                let focus_state = field.read(cx).state.clone();
                window.defer(cx, move |window, cx| {
                    focus_state.update(cx, |input, cx| input.focus(window, cx));
                });
            }
            if self.focus_on_render
                && !field
                    .read(cx)
                    .state
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            {
                let focus_state = field.read(cx).state.clone();
                window.defer(cx, move |window, cx| {
                    focus_state.update(cx, |input, cx| input.focus(window, cx));
                });
            }
            field.read(cx).state.clone()
        };
        // The pinned setter always notifies. Cache the applied placeholder because
        // reading presentation() on every frame would also copy the entire value.
        let applied_placeholder = window.use_keyed_state(
            text_context_menu_id(&component_id, "applied-placeholder"),
            cx,
            |_, cx| {
                (
                    state.entity_id(),
                    state.read(cx).presentation().placeholder().clone(),
                )
            },
        );
        let placeholder_changed = applied_placeholder.update(cx, |applied, cx| {
            if applied.0 != state.entity_id() {
                *applied = (
                    state.entity_id(),
                    state.read(cx).presentation().placeholder().clone(),
                );
            }
            if applied.1 == placeholder {
                return false;
            }
            applied.1 = placeholder.clone();
            true
        });
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        let context_focus = state.read(cx).focus_handle(cx);
        let open_context_menu = context_menu.clone();
        state.update(cx, |input, cx| {
            input.set_editor_style(editor_paint_style(self.theme));
            input.set_readonly(self.readonly, cx);
            if placeholder_changed {
                input.set_placeholder(placeholder, window, cx);
            }
            input.on_context_menu(Rc::new(move |_, capabilities, position, window, cx| {
                context_focus.focus(window, cx);
                open_context_menu.update(cx, |state, cx| {
                    state.position = Some(position);
                    state.capabilities = capabilities;
                    state.target_focus = Some(context_focus.clone());
                    cx.notify();
                });
            }));
            if !focused && input.value() != self.value {
                input.set_value(self.value.clone(), window, cx);
            }
        });
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
            .gap(px(self.content_gap))
            .rounded(px(theme.metrics.radius_small))
            .font_family(self.font_family)
            .text_size(px(self.text_size))
            // gpui-base's single-line Input takes glyph color from the
            // enclosing GPUI text style, not InputEditorStyle::foreground.
            // Hide that native glyph copy when the variable layer paints the
            // complete value, otherwise both layers visibly diverge on scroll.
            .text_color(if overlay_paints_text {
                transparent_black()
            } else {
                theme.colors.text.primary.into()
            })
            .bg(if self.flat {
                theme.colors.surfaces.sidebar
            } else {
                theme.colors.surfaces.raised
            })
            .when(!self.flat, |input| {
                input.border_1().border_color(if focused {
                    if self.quiet_focus {
                        theme.colors.borders.strong
                    } else {
                        theme.colors.borders.focused
                    }
                } else {
                    theme.colors.borders.standard
                })
            })
            .focused(focused)
            .styles(move |styles| {
                styles.focused(move |input| {
                    if self.flat {
                        input.bg(hover_fill(theme.colors.surfaces.sidebar))
                    } else if self.quiet_focus {
                        input.border_color(theme.colors.borders.strong)
                    } else {
                        input.border_color(theme.colors.borders.focused)
                    }
                })
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
            .when_some(self.leading_icon, |input, icon| input.child(icon))
            .child(Input::new(&state));
        let input = if self.variable_overlay {
            variable_input_overlay(
                self.theme,
                state.clone(),
                tooltip_id,
                input,
                self.value,
                self.variables,
                self.highlight_path_variables,
                window,
                cx,
            )
        } else {
            input.into_any_element()
        };
        if let Some(scroll) = self.list_scroll.as_ref() {
            scroll.update(cx, |list, _| list.fields.push(state));
        }
        with_text_context_menu(
            self.theme,
            &component_id,
            context_menu,
            input,
            Vec::new(),
            false,
            window,
            cx,
        )
    }
}

pub(in crate::components) fn text_input_base(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
) -> ProbeTextInput {
    ProbeTextInput {
        theme,
        id: id.into(),
        value: single_line(value),
        placeholder: placeholder.into(),
        variables: VariableContext::default(),
        highlight_path_variables: false,
        variable_overlay: false,
        font_family: theme.typography.interface_family,
        text_size: theme.typography.body_size,
        height: theme.metrics.control_height,
        width: None,
        debug_selector: None,
        on_change: None,
        on_enter: None,
        on_focus: None,
        autofocus: false,
        readonly: false,
        shared_input: None,
        persistent_field: None,
        on_field_focus: None,
        flat: false,
        leading_icon: None,
        content_gap: theme.metrics.spacing_1,
        quiet_focus: false,
        focus_on_render: false,
        list_scroll: None,
    }
}

const SCROLL_GESTURE_GAP: Duration = Duration::from_millis(28);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListedFieldAxis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListedFieldWheel {
    /// Scroll the surrounding list and keep the field from seeing the wheel.
    List { delta_y: Pixels },
    /// Shift maps a vertical wheel onto the field's horizontal text offset.
    ShiftText { delta_x: Pixels },
    /// A horizontal gesture belongs to the field's own wheel handler, which
    /// clamps against the current scroll range.
    Field,
}

#[derive(Clone, Copy, Debug)]
struct ListedFieldWheelSample {
    at: Instant,
    phase: TouchPhase,
    precise: bool,
    focused: bool,
    shift: bool,
    delta_x: Pixels,
    delta_y: Pixels,
}

/// Axis chosen for one precise trackpad gesture over a single-line field.
///
/// The first non-zero sample picks vertical or horizontal, and that choice
/// lasts until the gesture ends. A later frame does not retarget the wheel.
/// Updating this state must not notify: it changes on every trackpad tick.
/// The container redraws only when its scroll offset changes.
#[derive(Default)]
struct ListedFieldGesture {
    axis: Option<ListedFieldAxis>,
    last_event: Option<Instant>,
    has_start_phase: bool,
    /// This wheel frame was handed to the field. The wrapper stops it in the
    /// bubble phase when the field's own handler leaves it unconsumed.
    yielded_to_field: bool,
}

impl ListedFieldGesture {
    fn reset(&mut self) {
        self.axis = None;
        self.last_event = None;
        self.has_start_phase = false;
    }

    fn track_precise(&mut self, at: Instant, phase: TouchPhase, delta_x: Pixels, delta_y: Pixels) {
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.reset();
            return;
        }
        if phase == TouchPhase::Started {
            self.reset();
            self.has_start_phase = true;
        }
        let x = delta_x.abs();
        let y = delta_y.abs();
        if x == px(0.0) && y == px(0.0) {
            return;
        }
        // Explicit phases define the physical gesture even across slow frames.
        // Devices that send only Moved events still need an idle-gap fallback.
        let starts_new_gesture = !self.has_start_phase
            && self
                .last_event
                .is_none_or(|last_event| at.duration_since(last_event) >= SCROLL_GESTURE_GAP);
        if starts_new_gesture {
            self.axis = None;
        }
        self.last_event = Some(at);
        if self.axis.is_none() {
            self.axis = Some(if x <= y {
                ListedFieldAxis::Vertical
            } else {
                ListedFieldAxis::Horizontal
            });
        }
    }
}

/// A precise gesture scrolls field text when its first non-zero sample is
/// horizontal and the focused value overflows. Shift still maps one vertical
/// wheel tick onto that text. Every other sample scrolls the list by its
/// vertical component.
fn classify_listed_field_wheel(
    gesture: &mut ListedFieldGesture,
    sample: ListedFieldWheelSample,
    overflows: impl FnOnce() -> bool,
) -> ListedFieldWheel {
    if sample.precise {
        gesture.track_precise(sample.at, sample.phase, sample.delta_x, sample.delta_y);
    } else {
        gesture.reset();
    }
    let horizontal = if sample.precise {
        gesture.axis == Some(ListedFieldAxis::Horizontal)
    } else {
        sample.delta_x.abs() > sample.delta_y.abs()
    };
    if sample.focused && (horizontal || sample.shift) && overflows() {
        if horizontal {
            ListedFieldWheel::Field
        } else {
            ListedFieldWheel::ShiftText {
                delta_x: sample.delta_y,
            }
        }
    } else {
        ListedFieldWheel::List {
            delta_y: sample.delta_y,
        }
    }
}

fn single_line_input_overflows(input: &InputState) -> bool {
    if input.scroll_offset().x < px(0.0) {
        return true;
    }
    let len = input.value().len();
    if len == 0 {
        return false;
    }
    let Some(text) = input.range_to_bounds(&(0..len)) else {
        return false;
    };
    let viewport = input.input_bounds();
    text.left() < viewport.left() - px(0.5) || text.right() > viewport.right() + px(0.5)
}

fn scroll_list_vertically(scroll: &ScrollHandle, delta_y: Pixels) -> bool {
    let mut offset = scroll.offset();
    let next_y = (offset.y + delta_y).clamp(-scroll.max_offset().y, px(0.0));
    if next_y == offset.y {
        return false;
    }
    offset.y = next_y;
    scroll.set_offset(offset);
    true
}

pub(crate) type ListScroll = Entity<ListScrollState>;

/// One coordinator per scroll container, with registrations limited to rendered fields.
#[derive(Default)]
pub(crate) struct ListScrollState {
    gesture: ListedFieldGesture,
    fields: Vec<Entity<InputState>>,
}

#[derive(IntoElement)]
pub(crate) struct ListScrollRegion {
    child: gpui::AnyElement,
    scroll: ScrollHandle,
    state: ListScroll,
    fill_height: bool,
}

pub(crate) fn list_scroll_region(
    child: impl IntoElement,
    scroll: &ScrollHandle,
    state: &ListScroll,
) -> ListScrollRegion {
    ListScrollRegion {
        child: child.into_any_element(),
        scroll: scroll.clone(),
        state: state.clone(),
        fill_height: false,
    }
}

impl ListScrollRegion {
    pub(crate) fn fill_height(mut self) -> Self {
        self.fill_height = true;
        self
    }
}

impl RenderOnce for ListScrollRegion {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let gesture = self.state;
        // Children register during render. Offscreen virtualized fields must not retain stale bounds.
        gesture.update(cx, |list, _| list.fields.clear());
        let scroll = self.scroll;
        let yielded_gesture = gesture.clone();
        div()
            .relative()
            .w_full()
            // The wrapper is painted before the field, so this bubble listener
            // runs after the input. The input stops a wheel only when its offset
            // changes; a locked horizontal frame can still carry vertical drift
            // once the text is clamped, and that frame must not reach the list.
            .on_scroll_wheel(move |_event, _window, cx| {
                if yielded_gesture.read(cx).gesture.yielded_to_field {
                    cx.stop_propagation();
                }
            })
            .when(self.fill_height, |region| region.h_full())
            .child(self.child)
            .child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |_bounds, hitbox, window, _cx| {
                        let scroll = scroll.clone();
                        let gesture = gesture.clone();
                        let view = window.current_view();
                        window.on_mouse_event(
                            move |event: &ScrollWheelEvent, phase, window, cx| {
                                if !phase.capture() || !hitbox.should_handle_scroll(window) {
                                    return;
                                }
                                let precise = event.delta.precise();
                                let delta = event.delta.pixel_delta(window.line_height());
                                let mut state = None;
                                let target = gesture.update(cx, |list, cx| {
                                    let fields = &list.fields;
                                    let target = classify_listed_field_wheel(
                                        &mut list.gesture,
                                        ListedFieldWheelSample {
                                            at: Instant::now(),
                                            phase: event.touch_phase,
                                            precise,
                                            // Resolve the focused field lazily for horizontal candidates.
                                            focused: true,
                                            shift: event.modifiers.shift,
                                            delta_x: delta.x,
                                            delta_y: delta.y,
                                        },
                                        || {
                                            state = fields
                                                .iter()
                                                .find(|state| {
                                                    let input = state.read(cx);
                                                    input.focus_handle(cx).is_focused(window)
                                                        && input
                                                            .input_bounds()
                                                            .contains(&event.position)
                                                })
                                                .cloned();
                                            state.as_ref().is_some_and(|state| {
                                                single_line_input_overflows(state.read(cx))
                                            })
                                        },
                                    );
                                    list.gesture.yielded_to_field =
                                        matches!(target, ListedFieldWheel::Field);
                                    target
                                });
                                match target {
                                    // The field's own handler applies this delta and
                                    // clamps it against the current range immediately.
                                    ListedFieldWheel::Field => {}
                                    ListedFieldWheel::ShiftText { delta_x } => {
                                        if delta_x != px(0.0) {
                                            state.as_ref().expect("focused field").update(
                                                cx,
                                                |input, cx| {
                                                    let mut offset = input.scroll_offset();
                                                    offset.x += delta_x;
                                                    offset.y = px(0.0);
                                                    input.set_scroll_offset(offset, cx);
                                                },
                                            );
                                        }
                                        cx.stop_propagation();
                                    }
                                    ListedFieldWheel::List { delta_y } => {
                                        if scroll_list_vertically(&scroll, delta_y) {
                                            cx.notify(view);
                                        }
                                        // Stop before the input's bubble handler. That
                                        // handler repaints on every wheel tick and
                                        // swallows the event when a horizontal
                                        // component moves its own text.
                                        cx.stop_propagation();
                                    }
                                }
                            },
                        );
                    },
                )
                .absolute()
                .top(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .left(px(0.0)),
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
) -> ProbeTextInput {
    let mut input = text_input_base(theme, id, value, placeholder);
    input.variables = variables;
    input.variable_overlay = true;
    input.font_family = theme.typography.monospace_family;
    input.on_change = Some(Rc::new(on_value_change));
    input
}

pub(crate) fn url_text_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    variables: VariableContext,
    on_value_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let mut input = text_input_base(theme, id, value, placeholder);
    input.debug_selector = Some("request-url-input");
    input.variables = variables;
    input.highlight_path_variables = true;
    input.variable_overlay = true;
    input.font_family = theme.typography.monospace_family;
    input.on_change = Some(Rc::new(on_value_change));
    input.into_any_element()
}

pub(crate) fn dialog_text_input(
    theme: Theme,
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    autofocus: bool,
    on_value_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    on_enter: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> ProbeTextInput {
    let mut input = text_input_base(theme, id, value, placeholder);
    input.on_change = Some(Rc::new(on_value_change));
    input.on_enter = Some(Rc::new(on_enter));
    input.autofocus = autofocus;
    input
}

impl ProbeTextInput {
    /// Defer this field's vertical wheel events to `scroll`.
    pub(crate) fn list_scroll(mut self, scroll: Option<&ListScroll>) -> Self {
        self.list_scroll = scroll.cloned();
        self
    }

    /// Reuse the active controller while its virtualized row is absent.
    pub(crate) fn persistent_field(
        mut self,
        field: Option<Entity<FieldInput>>,
        on_focus: impl Fn(Entity<FieldInput>, bool, &mut App) + 'static,
    ) -> Self {
        self.persistent_field = field;
        self.on_field_focus = Some(Rc::new(on_focus));
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.readonly = disabled;
        self.autofocus &= !disabled;
        self
    }
}

pub(crate) fn sidebar_search_input(
    theme: Theme,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    on_value_change: impl Fn(SharedString, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut input = text_input_base(theme, "tree-search-input", value, placeholder);
    input.text_size = theme.typography.caption_size;
    input.debug_selector = Some("tree-search");
    input.on_change = Some(Rc::new(on_value_change));
    input.flat = true;
    input.content_gap = theme.metrics.spacing_2;
    input.leading_icon = Some(
        library_icon("lucide-search", &SEARCH_SVG, theme.metrics.icon_small)
            .text_color(theme.colors.text.muted),
    );
    input
}

#[cfg(test)]
mod tests {
    use super::{
        ListedFieldGesture, ListedFieldWheel, ListedFieldWheelSample, classify_listed_field_wheel,
    };
    use gpui::{TouchPhase, px};
    use std::time::{Duration, Instant};

    fn sample(
        at: Instant,
        phase: TouchPhase,
        precise: bool,
        focused: bool,
        shift: bool,
        delta_x: f32,
        delta_y: f32,
    ) -> ListedFieldWheelSample {
        ListedFieldWheelSample {
            at,
            phase,
            precise,
            focused,
            shift,
            delta_x: px(delta_x),
            delta_y: px(delta_y),
        }
    }

    #[test]
    fn vertical_and_unfocused_wheels_stay_with_the_list() {
        let at = Instant::now();
        let mut gesture = ListedFieldGesture::default();
        assert_eq!(
            classify_listed_field_wheel(
                &mut gesture,
                sample(at, TouchPhase::Started, true, true, false, 2.0, -40.0),
                || panic!("vertical scrolling must not inspect text overflow"),
            ),
            ListedFieldWheel::List { delta_y: px(-40.0) }
        );
        assert_eq!(
            classify_listed_field_wheel(
                &mut ListedFieldGesture::default(),
                sample(at, TouchPhase::Moved, true, false, false, 40.0, 0.0),
                || panic!("unfocused scrolling must not inspect text overflow"),
            ),
            ListedFieldWheel::List { delta_y: px(0.0) }
        );
        assert_eq!(
            classify_listed_field_wheel(
                &mut ListedFieldGesture::default(),
                sample(at, TouchPhase::Moved, false, true, true, 0.0, -40.0),
                || false,
            ),
            ListedFieldWheel::List { delta_y: px(-40.0) }
        );
    }

    #[test]
    fn shift_wheel_scrolls_overflowing_text() {
        assert_eq!(
            classify_listed_field_wheel(
                &mut ListedFieldGesture::default(),
                sample(
                    Instant::now(),
                    TouchPhase::Moved,
                    false,
                    true,
                    true,
                    0.0,
                    -40.0
                ),
                || true,
            ),
            ListedFieldWheel::ShiftText { delta_x: px(-40.0) }
        );
    }

    #[test]
    fn gesture_boundaries_release_the_axis() {
        for (phase, elapsed, delta_x, delta_y) in [
            (TouchPhase::Ended, Duration::from_millis(4), -30.0, 4.0),
            (TouchPhase::Cancelled, Duration::from_millis(4), -30.0, 4.0),
            (TouchPhase::Started, Duration::from_millis(4), 0.0, 0.0),
        ] {
            let at = Instant::now();
            let mut gesture = ListedFieldGesture::default();
            classify_listed_field_wheel(
                &mut gesture,
                sample(at, TouchPhase::Started, true, true, false, 3.0, -40.0),
                || panic!("vertical scrolling must not inspect text overflow"),
            );
            let boundary = classify_listed_field_wheel(
                &mut gesture,
                sample(at + elapsed, phase, true, true, false, delta_x, delta_y),
                || panic!("gesture boundaries must not inspect text overflow"),
            );
            assert_eq!(
                boundary,
                ListedFieldWheel::List {
                    delta_y: px(delta_y)
                },
                "gesture boundary {phase:?} after {elapsed:?}"
            );
            assert_eq!(
                classify_listed_field_wheel(
                    &mut gesture,
                    sample(
                        at + elapsed + Duration::from_millis(1),
                        TouchPhase::Moved,
                        true,
                        true,
                        false,
                        -30.0,
                        4.0,
                    ),
                    || true,
                ),
                ListedFieldWheel::Field,
                "horizontal frame after {phase:?} boundary"
            );
        }
    }
    #[test]
    fn explicit_gestures_keep_the_axis_across_slow_frames() {
        let at = Instant::now();
        for (first_phase, expected) in [
            (
                TouchPhase::Started,
                ListedFieldWheel::List { delta_y: px(-3.0) },
            ),
            (TouchPhase::Moved, ListedFieldWheel::Field),
        ] {
            let mut gesture = ListedFieldGesture::default();
            classify_listed_field_wheel(
                &mut gesture,
                sample(at, first_phase, true, false, false, 1.0, -40.0),
                || false,
            );
            assert_eq!(
                classify_listed_field_wheel(
                    &mut gesture,
                    sample(
                        at + Duration::from_secs(1),
                        TouchPhase::Moved,
                        true,
                        true,
                        false,
                        -80.0,
                        -3.0
                    ),
                    || true
                ),
                expected
            );
        }
    }
}
