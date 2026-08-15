//! Multi-line text editor for request bodies.
//!
//! gpui-base `Input` shapes a single line and panics when the value contains `\n`.
//! This editor splits on newlines and shapes each line with `shape_line`.

use std::{ops::Range, rc::Rc, sync::Arc};

use base_gpui::primitives::input::{
    InputBackspace, InputCopy, InputCut, InputDelete, InputEnd, InputEnter, InputHome, InputLeft,
    InputPaste, InputRight, InputSelectAll, InputSelectLeft, InputSelectRight,
};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Div, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, GlobalElementId, Hsla, InspectorElementId,
    InteractiveElement as _, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, ParentElement as _, Pixels, Point, RenderOnce, ScrollStrategy,
    ShapedLine, SharedString, StatefulInteractiveElement as _, Style, StyleRefinement, Styled,
    TextAlign, TextRun, UTF16Selection, UniformListScrollHandle, Window, actions, div, fill, point,
    px, relative, rgba, size, uniform_list,
};

const KEY_CONTEXT: &str = "MultilineInput";

#[derive(Clone, Debug)]
pub(crate) struct TextHighlight {
    pub range: Range<usize>,
    pub color: Option<Hsla>,
    pub background: Option<Hsla>,
}

actions!(
    probe_multiline_input,
    [
        MultilineUp,
        MultilineDown,
        MultilineSelectUp,
        MultilineSelectDown
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("backspace", InputBackspace, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("shift-backspace", InputBackspace, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("delete", InputDelete, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("shift-delete", InputDelete, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("left", InputLeft, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("right", InputRight, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("shift-left", InputSelectLeft, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("shift-right", InputSelectRight, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("up", MultilineUp, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("down", MultilineDown, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("shift-up", MultilineSelectUp, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("shift-down", MultilineSelectDown, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("home", InputHome, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("end", InputEnd, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-left", InputHome, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-right", InputEnd, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-a", InputSelectAll, Some(KEY_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-a", InputSelectAll, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-v", InputPaste, Some(KEY_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-v", InputPaste, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-c", InputCopy, Some(KEY_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-c", InputCopy, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-x", InputCut, Some(KEY_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-x", InputCut, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("enter", InputEnter, Some(KEY_CONTEXT)),
    ]);
}

type ValueChangeHandler =
    Rc<dyn Fn(SharedString, &mut Window, &mut Context<MultilineRuntime>) + 'static>;

pub(crate) struct MultilineRuntime {
    focus_handle: FocusHandle,
    value: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_lines: Vec<PaintedLine>,
    last_line_height: Pixels,
    list_generation: u64,
    hit_generation: u64,
    selecting: bool,
    read_only: bool,
    scroll_handle: Option<UniformListScrollHandle>,
    on_value_change: Option<ValueChangeHandler>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl MultilineRuntime {
    fn new(value: SharedString, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus_handle, window, |_, _, cx| cx.notify()),
            cx.on_blur(&focus_handle, window, |this, _, cx| {
                this.selecting = false;
                cx.notify();
            }),
        ];
        let cursor = value.len();
        Self {
            focus_handle,
            value,
            selected_range: cursor..cursor,
            selection_reversed: false,
            marked_range: None,
            last_lines: Vec::new(),
            last_line_height: px(0.0),
            list_generation: 0,
            hit_generation: 0,
            selecting: false,
            read_only: false,
            scroll_handle: None,
            on_value_change: None,
            _subscriptions: subscriptions,
        }
    }

    fn sync_props(
        &mut self,
        value: SharedString,
        read_only: bool,
        scroll_handle: Option<UniformListScrollHandle>,
        on_value_change: Option<ValueChangeHandler>,
        cx: &mut Context<Self>,
    ) {
        self.read_only = read_only;
        self.scroll_handle = scroll_handle;
        self.on_value_change = on_value_change;
        if self.value != value {
            self.value = value;
            self.clamp_selection_to_value();
            self.marked_range = None;
            cx.notify();
        }
    }

    fn can_edit(&self) -> bool {
        !self.read_only
    }

    fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn backspace(&mut self, _: &InputBackspace, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if self.selected_range.is_empty() {
            let previous = previous_boundary(&self.value, self.cursor_offset());
            if previous == self.cursor_offset() {
                window.play_system_bell();
                return;
            }
            self.select_to(previous, cx);
        }
        self.replace_text(None, "", window, cx);
    }

    fn delete(&mut self, _: &InputDelete, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if self.selected_range.is_empty() {
            let next = next_boundary(&self.value, self.cursor_offset());
            if next == self.cursor_offset() {
                window.play_system_bell();
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text(None, "", window, cx);
    }

    fn left(&mut self, _: &InputLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(previous_boundary(&self.value, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &InputRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(next_boundary(&self.value, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &InputSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(previous_boundary(&self.value, self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &InputSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_boundary(&self.value, self.cursor_offset()), cx);
    }

    fn up(&mut self, _: &MultilineUp, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.offset_for_vertical_move(-1);
        self.move_to(offset, cx);
    }

    fn down(&mut self, _: &MultilineDown, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.offset_for_vertical_move(1);
        self.move_to(offset, cx);
    }

    fn select_up(&mut self, _: &MultilineSelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.offset_for_vertical_move(-1);
        self.select_to(offset, cx);
    }

    fn select_down(&mut self, _: &MultilineSelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.offset_for_vertical_move(1);
        self.select_to(offset, cx);
    }

    fn select_all(&mut self, _: &InputSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.value.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &InputHome, _: &mut Window, cx: &mut Context<Self>) {
        let spans = line_spans(&self.value);
        let (line, _) = cursor_line_and_column(&spans, self.cursor_offset());
        self.move_to(spans[line].start, cx);
    }

    fn end(&mut self, _: &InputEnd, _: &mut Window, cx: &mut Context<Self>) {
        let spans = line_spans(&self.value);
        let (line, _) = cursor_line_and_column(&spans, self.cursor_offset());
        self.move_to(spans[line].end, cx);
    }

    fn copy(&mut self, _: &InputCopy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.value[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &InputCut, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() || !self.can_edit() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.value[self.selected_range.clone()].to_string(),
        ));
        self.replace_text(None, "", window, cx);
    }

    fn paste(&mut self, _: &InputPaste, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text(None, &normalize_newlines(&text), window, cx);
        }
    }

    fn enter(&mut self, _: &InputEnter, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        self.replace_text(None, "\n", window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window, cx);
        self.selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.selecting = false;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp_offset(offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.reveal_cursor();
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp_offset(offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.reveal_cursor();
        cx.notify();
    }

    fn reveal_cursor(&self) {
        let Some(handle) = &self.scroll_handle else {
            return;
        };
        let spans = line_spans(&self.value);
        let (line, _) = cursor_line_and_column(&spans, self.cursor_offset());
        handle.scroll_to_item(line, ScrollStrategy::Nearest);
    }

    fn offset_for_vertical_move(&self, delta: isize) -> usize {
        let spans = line_spans(&self.value);
        let cursor = self.cursor_offset();
        let (line, _) = cursor_line_and_column(&spans, cursor);
        let target = line.saturating_add_signed(delta);
        if target >= spans.len() {
            return self.value.len();
        }
        if target == line {
            return cursor;
        }
        let column_x = self.cursor_x();
        if let Some(painted) = self
            .last_lines
            .iter()
            .find(|line| line.span == spans[target])
        {
            let index = painted.line.closest_index_for_x(column_x);
            return (spans[target].start + index).min(spans[target].end);
        }
        let column = cursor.saturating_sub(spans[line].start);
        (spans[target].start + column).min(spans[target].end)
    }

    fn cursor_x(&self) -> Pixels {
        let cursor = self.cursor_offset();
        self.last_lines
            .iter()
            .find(|line| cursor >= line.span.start && cursor <= line.span.end)
            .map(|line| line.line.x_for_index(cursor - line.span.start))
            .unwrap_or(px(0.0))
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.last_lines.is_empty() || self.last_line_height <= px(0.0) {
            return self.value.len();
        }
        if position.y < self.last_lines[0].origin.y {
            return self.last_lines[0].span.start;
        }
        for painted in &self.last_lines {
            if position.y < painted.origin.y + self.last_line_height {
                let index = painted
                    .line
                    .closest_index_for_x(position.x - painted.origin.x);
                return (painted.span.start + index).min(painted.span.end);
            }
        }
        self.last_lines
            .last()
            .map(|line| line.span.end)
            .unwrap_or(self.value.len())
    }

    fn begin_list_frame(&mut self) -> u64 {
        self.list_generation = self.list_generation.wrapping_add(1);
        self.list_generation
    }

    fn begin_hit_frame(&mut self, generation: u64) {
        if self.hit_generation != generation {
            self.hit_generation = generation;
            self.last_lines.clear();
        }
    }

    fn replace_text(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace_selected_text(range, new_text, None, window, cx);
    }

    fn replace_selected_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        mark: Option<Option<Range<usize>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut next = String::new();
        next.push_str(&self.value[..range.start]);
        next.push_str(new_text);
        next.push_str(&self.value[range.end..]);
        let next_value = SharedString::from(next);
        let next_offset = range.start + new_text.len();

        if let Some(on_value_change) = self.on_value_change.as_ref() {
            on_value_change(next_value.clone(), window, cx);
        }

        self.value = next_value;
        match mark {
            Some(new_selected_range_utf16) => {
                if new_text.is_empty() {
                    self.marked_range = None;
                    self.selected_range = range.start..range.start;
                } else {
                    let marked_range = range.start..range.start + new_text.len();
                    self.marked_range = Some(marked_range.clone());
                    self.selected_range = new_selected_range_utf16
                        .as_ref()
                        .map(|range_utf16| self.range_from_utf16(range_utf16))
                        .map(|new_range| {
                            new_range.start + marked_range.start..new_range.end + marked_range.start
                        })
                        .unwrap_or_else(|| marked_range.end..marked_range.end);
                }
                self.selection_reversed = false;
            }
            None => {
                self.marked_range = None;
                self.selected_range = next_offset..next_offset;
                self.selection_reversed = false;
            }
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.value.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.value.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn clamp_offset(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.value.len());
        while offset > 0 && !self.value.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn clamp_selection_to_value(&mut self) {
        let start = self.clamp_offset(self.selected_range.start);
        let end = self.clamp_offset(self.selected_range.end);
        self.selected_range = start.min(end)..start.max(end);
    }
}

impl EntityInputHandler for MultilineRuntime {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.value[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        self.replace_text(range_utf16, &normalize_newlines(text), window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace_selected_text(
            range,
            &normalize_newlines(new_text),
            Some(new_selected_range),
            window,
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let painted = self
            .last_lines
            .iter()
            .find(|line| range.start >= line.span.start && range.start <= line.span.end)?;
        let start = painted
            .line
            .x_for_index(range.start.saturating_sub(painted.span.start));
        let end = painted.line.x_for_index(
            range
                .end
                .saturating_sub(painted.span.start)
                .min(painted.span.len()),
        );
        Some(Bounds::from_corners(
            point(painted.origin.x + start, painted.origin.y),
            point(
                painted.origin.x + end,
                painted.origin.y + self.last_line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(point)))
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.can_edit()
    }
}

struct LineLayout {
    span: Range<usize>,
    line: ShapedLine,
}

struct PaintedLine {
    span: Range<usize>,
    line: ShapedLine,
    origin: Point<Pixels>,
}

struct MultilineTextElement {
    state: Entity<MultilineRuntime>,
    placeholder: SharedString,
    highlights: Vec<TextHighlight>,
}

struct MultilinePrepaintState {
    lines: Vec<LineLayout>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    line_height: Pixels,
}

impl IntoElement for MultilineTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for MultilineTextElement {
    type RequestLayoutState = ();
    type PrepaintState = MultilinePrepaintState;

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
        let value = self.state.read(cx).value.clone();
        let line_count = line_spans(&value).len().max(1);
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = (window.line_height() * line_count as f32).into();
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
        let runtime = self.state.read(cx);
        let value = runtime.value.clone();
        let selected_range = runtime.selected_range.clone();
        let cursor = runtime.cursor_offset();
        let focused = runtime.focus_handle.is_focused(window);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let empty = value.is_empty();

        let (display, display_color) = if empty {
            (self.placeholder.clone(), style.color.opacity(0.35))
        } else {
            (value.clone(), style.color)
        };

        let spans = line_spans(&display);

        let mut lines = Vec::with_capacity(spans.len());
        for span in &spans {
            let line_text = SharedString::from(display[span.clone()].to_string());
            let base = TextRun {
                len: line_text.len(),
                font: style.font(),
                color: display_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line_highlights = if empty {
                Vec::new()
            } else {
                clip_text_highlights(&self.highlights, span.clone())
            };
            let runs = runs_from_highlights(&line_text, &line_highlights, &base);
            let shaped = window
                .text_system()
                .shape_line(line_text, font_size, &runs, None);
            lines.push(LineLayout {
                span: span.clone(),
                line: shaped,
            });
        }

        let mut selections = Vec::new();
        let mut cursor_quad = None;
        if focused && !empty {
            if selected_range.is_empty() {
                if let Some((index, layout)) = lines
                    .iter()
                    .enumerate()
                    .find(|(_, layout)| cursor >= layout.span.start && cursor <= layout.span.end)
                {
                    let cursor_x = layout.line.x_for_index(cursor - layout.span.start);
                    cursor_quad = Some(fill(
                        Bounds::new(
                            point(
                                bounds.left() + cursor_x,
                                bounds.top() + line_height * index as f32,
                            ),
                            size(px(1.0), line_height),
                        ),
                        style.color,
                    ));
                }
            } else {
                for (index, layout) in lines.iter().enumerate() {
                    let start = selected_range.start.max(layout.span.start);
                    let end = selected_range.end.min(layout.span.end);
                    if start >= end
                        && !(layout.span.start >= selected_range.start
                            && layout.span.start < selected_range.end
                            && layout.span.is_empty())
                    {
                        continue;
                    }
                    let start_x = layout
                        .line
                        .x_for_index(start.saturating_sub(layout.span.start));
                    let end_x = if start >= end {
                        layout.line.width.max(px(4.0))
                    } else {
                        layout
                            .line
                            .x_for_index(end.saturating_sub(layout.span.start))
                    };
                    let top = bounds.top() + line_height * index as f32;
                    selections.push(fill(
                        Bounds::from_corners(
                            point(bounds.left() + start_x, top),
                            point(
                                bounds.left() + end_x.max(start_x + px(1.0)),
                                top + line_height,
                            ),
                        ),
                        rgba(0x335b9dff),
                    ));
                }
            }
        } else if focused && empty {
            cursor_quad = Some(fill(
                Bounds::new(
                    point(bounds.left(), bounds.top()),
                    size(px(1.0), line_height),
                ),
                style.color,
            ));
        }

        MultilinePrepaintState {
            lines,
            cursor: cursor_quad,
            selections,
            line_height,
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
        let focus_handle = self.state.read(cx).focus_handle();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.state.clone()),
            cx,
        );

        let line_height = prepaint.line_height;
        for selection in prepaint.selections.drain(..) {
            window.paint_quad(selection);
        }
        for (index, layout) in prepaint.lines.iter().enumerate() {
            layout
                .line
                .paint(
                    point(bounds.left(), bounds.top() + line_height * index as f32),
                    line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .expect("multiline input text should paint");
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }

        let stored_lines = prepaint
            .lines
            .drain(..)
            .enumerate()
            .map(|(index, layout)| PaintedLine {
                span: layout.span,
                line: layout.line,
                origin: point(bounds.left(), bounds.top() + line_height * index as f32),
            })
            .collect();
        self.state.update(cx, |runtime, _| {
            runtime.last_lines = stored_lines;
            runtime.last_line_height = line_height;
        });
    }
}

type VisibleRangeHandler = Rc<dyn Fn(Range<usize>, &mut App) + 'static>;

#[derive(IntoElement)]
pub(crate) struct MultilineInput {
    id: ElementId,
    base: Div,
    value: SharedString,
    placeholder: SharedString,
    highlights: Vec<TextHighlight>,
    read_only: bool,
    scroll_handle: Option<UniformListScrollHandle>,
    on_visible_range: Option<VisibleRangeHandler>,
    on_value_change: Option<ValueChangeHandler>,
    style_with_state: Option<Rc<dyn Fn(bool, Div) -> Div + 'static>>,
}

impl Default for MultilineInput {
    fn default() -> Self {
        Self {
            id: ElementId::from("multiline-input"),
            base: div(),
            value: SharedString::default(),
            placeholder: SharedString::default(),
            highlights: Vec::new(),
            read_only: false,
            scroll_handle: None,
            on_visible_range: None,
            on_value_change: None,
            style_with_state: None,
        }
    }
}

impl Styled for MultilineInput {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for MultilineInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state_id =
            ElementId::NamedChild(Arc::new(self.id.clone()), SharedString::from("state"));
        let initial = self.value.clone();
        let state: Entity<MultilineRuntime> = window.use_keyed_state(state_id, cx, |window, cx| {
            MultilineRuntime::new(initial, window, cx)
        });
        state.update(cx, |runtime, cx| {
            runtime.sync_props(
                self.value.clone(),
                self.read_only,
                self.scroll_handle.clone(),
                self.on_value_change.clone(),
                cx,
            );
        });
        let focus_handle = state.read(cx).focus_handle();
        let focused = focus_handle.is_focused(window);
        let highlights = self.highlights.clone();
        let base = match self.style_with_state {
            Some(style) => style(focused, self.base),
            None => self.base,
        }
        .id(self.id.clone())
        .track_focus(&focus_handle.tab_stop(true))
        .key_context(KEY_CONTEXT)
        .focusable()
        .cursor(CursorStyle::IBeam)
        .on_action(window.listener_for(&state, MultilineRuntime::left))
        .on_action(window.listener_for(&state, MultilineRuntime::right))
        .on_action(window.listener_for(&state, MultilineRuntime::select_left))
        .on_action(window.listener_for(&state, MultilineRuntime::select_right))
        .on_action(window.listener_for(&state, MultilineRuntime::up))
        .on_action(window.listener_for(&state, MultilineRuntime::down))
        .on_action(window.listener_for(&state, MultilineRuntime::select_up))
        .on_action(window.listener_for(&state, MultilineRuntime::select_down))
        .on_action(window.listener_for(&state, MultilineRuntime::select_all))
        .on_action(window.listener_for(&state, MultilineRuntime::home))
        .on_action(window.listener_for(&state, MultilineRuntime::end))
        .on_action(window.listener_for(&state, MultilineRuntime::copy))
        .on_action(window.listener_for(&state, MultilineRuntime::enter))
        .on_action(window.listener_for(&state, MultilineRuntime::backspace))
        .on_action(window.listener_for(&state, MultilineRuntime::delete))
        .on_action(window.listener_for(&state, MultilineRuntime::paste))
        .on_action(window.listener_for(&state, MultilineRuntime::cut));
        if self.read_only {
            base.child(read_only_list(
                self.id,
                state,
                self.value,
                self.placeholder,
                highlights,
                self.scroll_handle,
                self.on_visible_range,
                window,
            ))
        } else {
            base.overflow_scroll()
                .on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(&state, MultilineRuntime::on_mouse_down),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    window.listener_for(&state, MultilineRuntime::on_mouse_up),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    window.listener_for(&state, MultilineRuntime::on_mouse_up),
                )
                .on_mouse_move(window.listener_for(&state, MultilineRuntime::on_mouse_move))
                .child(MultilineTextElement {
                    state,
                    placeholder: self.placeholder,
                    highlights,
                })
        }
    }
}

impl MultilineInput {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub(crate) fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    pub(crate) fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub(crate) fn highlights(mut self, highlights: Vec<TextHighlight>) -> Self {
        self.highlights = highlights;
        self
    }

    pub(crate) fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub(crate) fn track_scroll(mut self, handle: UniformListScrollHandle) -> Self {
        self.scroll_handle = Some(handle);
        self
    }

    pub(crate) fn on_visible_range(
        mut self,
        on_visible_range: impl Fn(Range<usize>, &mut App) + 'static,
    ) -> Self {
        self.on_visible_range = Some(Rc::new(on_visible_range));
        self
    }

    pub(crate) fn on_value_change(
        mut self,
        on_value_change: impl Fn(SharedString, &mut Window, &mut Context<MultilineRuntime>) + 'static,
    ) -> Self {
        self.on_value_change = Some(Rc::new(on_value_change));
        self
    }

    pub(crate) fn style_with_state(mut self, style: impl Fn(bool, Div) -> Div + 'static) -> Self {
        self.style_with_state = Some(Rc::new(style));
        self
    }
}

/// Byte ranges of each line, excluding the `\n` terminator.
pub(crate) fn line_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            spans.push(start..index);
            start = index + 1;
        }
    }
    spans.push(start..text.len());
    spans
}

fn cursor_line_and_column(spans: &[Range<usize>], cursor: usize) -> (usize, usize) {
    for (index, span) in spans.iter().enumerate() {
        if cursor <= span.end {
            return (index, cursor.saturating_sub(span.start));
        }
    }
    let last = spans.len().saturating_sub(1);
    (last, spans.get(last).map(|span| span.len()).unwrap_or(0))
}

struct ReadOnlyLineElement {
    state: Entity<MultilineRuntime>,
    text: SharedString,
    span: Range<usize>,
    highlights: Vec<TextHighlight>,
    generation: u64,
    placeholder: bool,
}

struct ReadOnlyLinePrepaint {
    line: ShapedLine,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    line_height: Pixels,
}

impl IntoElement for ReadOnlyLineElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ReadOnlyLineElement {
    type RequestLayoutState = ();
    type PrepaintState = ReadOnlyLinePrepaint;

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
        let runtime = self.state.read(cx);
        let selected_range = runtime.selected_range.clone();
        let cursor = runtime.cursor_offset();
        let focused = runtime.focus_handle.is_focused(window);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let color = if self.placeholder {
            style.color.opacity(0.35)
        } else {
            style.color
        };
        let base = TextRun {
            len: self.text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = runs_from_highlights(&self.text, &self.highlights, &base);
        let shaped = window
            .text_system()
            .shape_line(self.text.clone(), font_size, &runs, None);

        let mut selection = None;
        let mut cursor_quad = None;
        if focused && !self.placeholder {
            if selected_range.is_empty() {
                if cursor >= self.span.start && cursor <= self.span.end {
                    let cursor_x = shaped.x_for_index(cursor - self.span.start);
                    cursor_quad = Some(fill(
                        Bounds::new(
                            point(bounds.left() + cursor_x, bounds.top()),
                            size(px(1.0), line_height),
                        ),
                        style.color,
                    ));
                }
            } else {
                let start = selected_range.start.max(self.span.start);
                let end = selected_range.end.min(self.span.end);
                let selected_empty_line = self.span.is_empty()
                    && self.span.start >= selected_range.start
                    && self.span.start < selected_range.end;
                if start < end || selected_empty_line {
                    let start_x = shaped.x_for_index(start.saturating_sub(self.span.start));
                    let end_x = if start >= end {
                        shaped.width.max(px(4.0))
                    } else {
                        shaped.x_for_index(end.saturating_sub(self.span.start))
                    };
                    selection = Some(fill(
                        Bounds::from_corners(
                            point(bounds.left() + start_x, bounds.top()),
                            point(
                                bounds.left() + end_x.max(start_x + px(1.0)),
                                bounds.top() + line_height,
                            ),
                        ),
                        rgba(0x335b9dff),
                    ));
                }
            }
        } else if focused && self.placeholder {
            cursor_quad = Some(fill(
                Bounds::new(
                    point(bounds.left(), bounds.top()),
                    size(px(1.0), line_height),
                ),
                style.color,
            ));
        }

        ReadOnlyLinePrepaint {
            line: shaped,
            cursor: cursor_quad,
            selection,
            line_height,
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
        let focus_handle = self.state.read(cx).focus_handle();
        let cursor = self.state.read(cx).cursor_offset();
        if cursor >= self.span.start && cursor <= self.span.end {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.state.clone()),
                cx,
            );
        }

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        prepaint
            .line
            .paint(
                point(bounds.left(), bounds.top()),
                prepaint.line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .expect("read-only response line should paint");
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }

        let painted = PaintedLine {
            span: self.span.clone(),
            line: prepaint.line.clone(),
            origin: bounds.origin,
        };
        let generation = self.generation;
        let line_height = prepaint.line_height;
        self.state.update(cx, |runtime, _| {
            runtime.begin_hit_frame(generation);
            runtime.last_lines.push(painted);
            runtime.last_line_height = line_height;
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn read_only_list(
    id: ElementId,
    state: Entity<MultilineRuntime>,
    value: SharedString,
    placeholder: SharedString,
    highlights: Vec<TextHighlight>,
    scroll_handle: Option<UniformListScrollHandle>,
    on_visible_range: Option<VisibleRangeHandler>,
    window: &mut Window,
) -> gpui::UniformList {
    let empty = value.is_empty();
    let spans = Arc::new(line_spans(&value));
    let highlights = Arc::new(highlights);
    let row_count = if empty { 1 } else { spans.len() };
    let list_id = ElementId::NamedChild(Arc::new(id), SharedString::from("list"));
    let mut list = uniform_list(list_id, row_count, {
        let value = value.clone();
        let state = state.clone();
        move |range, _, cx| {
            if let Some(on_visible_range) = &on_visible_range {
                on_visible_range(range.clone(), cx);
            }
            // uniform_list rebuilds this closure every frame. A local generation
            // cell would reset to 0 and never invalidate the previous frame's
            // hit targets, so clicks after scroll would land on pre-scroll lines.
            let frame = state.update(cx, |runtime, _| runtime.begin_list_frame());
            range
                .map(|index| {
                    let (line_text, line_span, placeholder) = if empty {
                        (placeholder.clone(), 0..0, true)
                    } else {
                        let span = spans.get(index).cloned().unwrap_or(0..0);
                        (
                            SharedString::from(value[span.clone()].to_string()),
                            span,
                            false,
                        )
                    };
                    let line_highlights = if placeholder {
                        Vec::new()
                    } else {
                        clip_text_highlights(&highlights, line_span.clone())
                    };
                    ReadOnlyLineElement {
                        state: state.clone(),
                        text: line_text,
                        span: line_span,
                        highlights: line_highlights,
                        generation: frame,
                        placeholder,
                    }
                })
                .collect::<Vec<_>>()
        }
    })
    .size_full()
    .on_mouse_down(
        MouseButton::Left,
        window.listener_for(&state, MultilineRuntime::on_mouse_down),
    )
    .on_mouse_up(
        MouseButton::Left,
        window.listener_for(&state, MultilineRuntime::on_mouse_up),
    )
    .on_mouse_up_out(
        MouseButton::Left,
        window.listener_for(&state, MultilineRuntime::on_mouse_up),
    )
    .on_mouse_move(window.listener_for(&state, MultilineRuntime::on_mouse_move));
    if let Some(handle) = scroll_handle {
        list = list.track_scroll(&handle);
    }
    list
}

fn runs_from_highlights(value: &str, highlights: &[TextHighlight], base: &TextRun) -> Vec<TextRun> {
    let mut points = vec![0, value.len()];
    for highlight in highlights {
        points.push(highlight.range.start.min(value.len()));
        points.push(highlight.range.end.min(value.len()));
    }
    points.sort_unstable();
    points.dedup();
    let mut runs = Vec::new();
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start >= end {
            continue;
        }
        let mut color = base.color;
        let mut background = base.background_color;
        for highlight in highlights {
            if highlight.range.start <= start && highlight.range.end >= end {
                if let Some(highlight_color) = highlight.color {
                    color = highlight_color;
                }
                if let Some(highlight_background) = highlight.background {
                    background = Some(highlight_background);
                }
            }
        }
        runs.push(TextRun {
            len: end - start,
            color,
            background_color: background,
            ..base.clone()
        });
    }
    runs.retain(|run| run.len > 0);
    if runs.is_empty() {
        runs.push(base.clone());
    }
    runs
}

fn clip_text_highlights(highlights: &[TextHighlight], span: Range<usize>) -> Vec<TextHighlight> {
    highlights
        .iter()
        .filter_map(|highlight| {
            let start = highlight.range.start.max(span.start);
            let end = highlight.range.end.min(span.end);
            (start < end).then(|| TextHighlight {
                range: start - span.start..end - span.start,
                color: highlight.color,
                background: highlight.background,
            })
        })
        .collect()
}

fn previous_boundary(value: &str, offset: usize) -> usize {
    if offset == 0 {
        0
    } else {
        value.floor_char_boundary(offset - 1)
    }
}

fn next_boundary(value: &str, offset: usize) -> usize {
    if offset >= value.len() {
        value.len()
    } else {
        value.ceil_char_boundary(offset + 1)
    }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use gpui::{
        AppContext as _, Entity, IntoElement, Modifiers, Render, ScrollStrategy, SharedString,
        TestAppContext, UniformListScrollHandle, VisualTestContext, div, hsla, point, prelude::*,
        px, size,
    };

    use super::{
        MultilineRuntime, MultilineTextElement, TextHighlight, clip_text_highlights,
        cursor_line_and_column, line_spans, normalize_newlines,
    };

    #[test]
    fn line_spans_split_on_newlines_and_keep_a_trailing_empty_line() {
        assert_eq!(line_spans(""), vec![0..0]);
        assert_eq!(line_spans("abc"), vec![0..3]);
        assert_eq!(line_spans("a\nb"), vec![0..1, 2..3]);
        assert_eq!(line_spans("a\nb\n"), vec![0..1, 2..3, 4..4]);
        assert_eq!(line_spans("\n"), vec![0..0, 1..1]);
    }

    #[test]
    fn cursor_on_a_newline_belongs_to_the_preceding_line_end() {
        let spans = line_spans("ab\ncd");
        assert_eq!(cursor_line_and_column(&spans, 0), (0, 0));
        assert_eq!(cursor_line_and_column(&spans, 2), (0, 2));
        assert_eq!(cursor_line_and_column(&spans, 3), (1, 0));
        assert_eq!(cursor_line_and_column(&spans, 5), (1, 2));
    }

    #[test]
    fn normalize_newlines_unifies_carriage_returns() {
        assert_eq!(normalize_newlines("a\r\nb\rc"), "a\nb\nc");
    }

    struct MultilineHarness {
        input: Entity<MultilineRuntime>,
    }

    impl Render for MultilineHarness {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn multiline_text_shapes_json_without_panicking(cx: &mut TestAppContext) {
        let value = SharedString::from("{\n  \"name\": \"Milo\"\n}");
        let window = cx.open_window(size(px(240.0), px(80.0)), |window, cx| MultilineHarness {
            input: cx.new(|cx| MultilineRuntime::new(value.clone(), window, cx)),
        });
        let input = window
            .update(cx, |harness, _window, _cx| harness.input.clone())
            .expect("multiline test window should be open");
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let _ = visual.draw(
            point(px(0.0), px(0.0)),
            size(px(200.0), px(60.0)),
            |_, _| MultilineTextElement {
                state: input,
                placeholder: SharedString::from("Body content"),
                highlights: Vec::new(),
            },
        );
    }

    #[test]
    fn clip_text_highlights_shifts_ranges_into_line_space() {
        let highlights = vec![TextHighlight {
            range: 2..8,
            color: Some(hsla(0.1, 1.0, 0.5, 1.0)),
            background: None,
        }];
        let clipped = clip_text_highlights(&highlights, 4..10);
        assert_eq!(clipped.len(), 1);
        assert_eq!(clipped[0].range, 0..4);
    }

    struct ReadOnlyScrollHarness {
        state: Entity<MultilineRuntime>,
        scroll: UniformListScrollHandle,
        value: SharedString,
    }

    impl Render for ReadOnlyScrollHarness {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let scroll = self.scroll.clone();
            self.state.update(cx, |runtime, cx| {
                runtime.sync_props(self.value.clone(), true, Some(scroll), None, cx);
            });
            div()
                .id("readonly-host")
                .debug_selector(|| "readonly-host".into())
                .size_full()
                .child(super::read_only_list(
                    "readonly-body".into(),
                    self.state.clone(),
                    self.value.clone(),
                    SharedString::default(),
                    Vec::new(),
                    Some(self.scroll.clone()),
                    None,
                    window,
                ))
        }
    }

    #[gpui::test]
    fn read_only_click_after_scroll_hits_the_visible_line(cx: &mut TestAppContext) {
        let value = SharedString::from(
            (0..200)
                .map(|index| format!("line-{index:03}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let scroll = UniformListScrollHandle::new();
        let window = cx.open_window(size(px(320.0), px(160.0)), |window, cx| {
            let state = cx.new(|cx| MultilineRuntime::new(value.clone(), window, cx));
            ReadOnlyScrollHarness {
                state,
                scroll: scroll.clone(),
                value,
            }
        });
        cx.run_until_parked();

        let first_visible = window
            .update(cx, |harness, _, cx| harness.state.read(cx).last_lines.len())
            .expect("readonly test window should be open");
        assert!(
            first_visible > 0,
            "read-only viewer should paint visible lines"
        );

        const SCROLLED_LINE: usize = 80;
        scroll.scroll_to_item(SCROLLED_LINE, ScrollStrategy::Top);
        window
            .update(cx, |_, window, cx| {
                window.refresh();
                cx.notify();
            })
            .expect("readonly test window should remain open");
        cx.run_until_parked();

        let (visible_after_scroll, line_height) = window
            .update(cx, |harness, _, cx| {
                let runtime = harness.state.read(cx);
                (runtime.last_lines.len(), runtime.last_line_height)
            })
            .expect("readonly test window should remain open");
        assert!(
            visible_after_scroll <= first_visible + 4,
            "hit targets should be replaced after scroll, not appended ({visible_after_scroll} vs {first_visible})"
        );

        window
            .update(cx, |harness, _, cx| {
                harness.state.update(cx, |runtime, _| {
                    runtime.selected_range = 0..0;
                });
            })
            .expect("readonly test window should remain open");

        let click = {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let bounds = visual
                .debug_bounds("readonly-host")
                .expect("read-only host should render");
            point(bounds.left() + px(16.0), bounds.top() + line_height / 2.0)
        };
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_click(click, Modifiers::default());
            visual.run_until_parked();
        }

        let (cursor, last_spans) = window
            .update(cx, |harness, _, cx| {
                let runtime = harness.state.read(cx);
                (
                    runtime.cursor_offset(),
                    runtime
                        .last_lines
                        .iter()
                        .map(|line| line.span.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .expect("readonly test window should remain open");
        let scrolled_line_start = (0..SCROLLED_LINE)
            .map(|index| format!("line-{index:03}\n").len())
            .sum::<usize>();
        assert!(
            last_spans
                .iter()
                .any(|span| cursor >= span.start && cursor <= span.end),
            "click should land on a currently painted line (cursor {cursor})"
        );
        assert!(
            cursor >= scrolled_line_start,
            "click after scroll should land in the visible lines (cursor {cursor}, expected at least {scrolled_line_start})"
        );
    }
}
