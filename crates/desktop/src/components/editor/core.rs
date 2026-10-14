use super::*;
use crate::app::{ShiftTabOrOutdent, TabOrIndent};
use gpui_kit::EntityInputHandler as _;

struct EditorField {
    state: Entity<EditorState>,
    decorations: TextDecorationCollection,
    last_decorations: Vec<TextDecoration>,
    last_value: SharedString,
    on_change: Option<InputChangeHandler>,
    last_scroll_range: Option<Range<usize>>,
    language: SharedString,
    soft_wrap: bool,
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
        if matches!(event, InputEvent::Change) {
            let value = input.read(cx).value();

            if value == this.last_value {
                return;
            }

            // Remember edits made by this EditorState before propagating them to
            // application state. The resulting render is an acknowledgement of
            // the local edit, not an external value replacement; calling
            // EditorState::set_value for it would reset the caret and undo stack.
            this.last_value = value.clone();

            if let Some(on_change) = this.on_change.clone() {
                on_change(value, window, cx);
            }
        }
    }
}

/// gpui-base paints caret, selection, and gutter from `InputEditorStyle`.
/// Its `Default` is fully transparent, so Probe must supply visible tokens.
pub(crate) fn editor_paint_style(theme: Theme) -> InputEditorStyle {
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
pub(in crate::components) struct ProbeEditor {
    pub(in crate::components) theme: Theme,
    pub(in crate::components) id: ElementId,
    pub(in crate::components) value: SharedString,
    pub(in crate::components) placeholder: SharedString,
    pub(in crate::components) decorations: Vec<TextDecoration>,
    pub(in crate::components) language: SharedString,
    pub(in crate::components) readonly: bool,
    pub(in crate::components) disabled_appearance: bool,
    pub(in crate::components) min_height: Option<f32>,
    pub(in crate::components) padding: EditorInsets,
    pub(in crate::components) soft_wrap: bool,
    pub(in crate::components) text_color: gpui_kit::Rgba,
    pub(in crate::components) scroll_to_range: Option<Range<usize>>,
    pub(in crate::components) on_change: Option<InputChangeHandler>,
    pub(in crate::components) on_mouse_down: Option<EditorMouseDownHandler>,
    pub(in crate::components) on_visible_range: Option<VisibleRangeHandler>,
    pub(in crate::components) extra_context_menu_actions: Vec<TextContextMenuExtraAction>,
    pub(in crate::components) debug_selector: Option<&'static str>,
    pub(in crate::components) variables: Option<VariableContext>,
}

impl RenderOnce for ProbeEditor {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let component_id = self.id.clone();
        let context_menu = window.use_keyed_state(
            text_context_menu_id(&component_id, "context-menu-state"),
            cx,
            |_, _| TextContextMenuState::default(),
        );
        let placeholder = self.placeholder.clone();
        let mut paint_style = editor_paint_style(self.theme);
        if self.disabled_appearance {
            paint_style.foreground = self.theme.colors.actions.disabled_foreground.into();
            paint_style.muted_foreground = self.theme.colors.actions.disabled_foreground.into();
            paint_style.background = self.theme.colors.actions.disabled.into();
            paint_style.border = self.theme.colors.borders.subtle.into();
        }
        let on_change = self.on_change.clone();
        let soft_wrap = self.soft_wrap;
        let language = self.language.clone();
        let editor_id = self.id.clone();
        let search_state = window.use_keyed_state(
            ElementId::NamedChild(
                Arc::new(editor_id.clone()),
                SharedString::from("search-state"),
            ),
            cx,
            |_, _| EditorSearchState::default(),
        );
        let scroll_to_range = self.scroll_to_range;
        let field = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            let state = cx.new(|cx| {
                let mut editor = EditorState::new(window, cx)
                    .placeholder(placeholder.clone())
                    .smart_indent(language.as_ref() == "json")
                    .folding(false)
                    // Let the shortcut reach Probe’s search card; the native
                    // session still owns matching, navigation, and painting.
                    .searchable(false)
                    .soft_wrap(soft_wrap)
                    .language(language.clone());
                editor.set_highlighter_factory(crate::syntax::factory(), cx);
                editor.set_editor_style(paint_style.clone());
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
                last_value: self.value.clone(),
                on_change: on_change.clone(),
                last_scroll_range: None,
                language: language.clone(),
                soft_wrap,
                _subscription: subscription,
            }
        });
        field.update(cx, |field, cx| {
            field.on_change = self.on_change.clone();
            let language_changed = field.language != self.language;
            if language_changed {
                field.language = self.language.clone();
            }
            let soft_wrap_changed = field.soft_wrap != self.soft_wrap;
            if soft_wrap_changed {
                field.soft_wrap = self.soft_wrap;
            }
            let context_focus = field.state.read(cx).focus_handle(cx);
            let open_context_menu = context_menu.clone();
            let context_editor = field.state.clone();
            let value_changed = field.last_value != self.value;
            if value_changed {
                field.last_value = self.value.clone();
            }
            field.state.update(cx, |editor, cx| {
                editor.set_editor_style(paint_style.clone());
                editor.set_editor_paddings(self.padding.edges());
                if editor.presentation().placeholder() != &placeholder {
                    editor.set_placeholder(placeholder.clone(), window, cx);
                }
                editor.set_readonly(self.readonly, cx);
                if soft_wrap_changed {
                    editor.set_soft_wrap(self.soft_wrap, window, cx);
                }
                editor.on_context_menu(Rc::new(move |_, capabilities, position, window, cx| {
                    context_focus.focus(window, cx);
                    open_context_menu.update(cx, |state, cx| {
                        state.position = Some(position);
                        state.capabilities = capabilities;
                        state.target_focus = Some(context_focus.clone());
                        state.target_editor = Some(context_editor.clone());
                        cx.notify();
                    });
                }));
                if language_changed {
                    editor.set_highlighter(self.language.clone(), cx);
                    editor.set_smart_indent(self.language.as_ref() == "json", window, cx);
                }
                if value_changed {
                    editor.set_value(self.value.clone(), window, cx);
                }
            });
            if field.last_decorations != self.decorations {
                field.last_decorations = self.decorations.clone();
                field.decorations.set(self.decorations.clone(), cx);
            }
            if scroll_to_range != field.last_scroll_range {
                if let Some(range) = &scroll_to_range {
                    let laid_out = field.state.read(cx).visible_row_range().is_some();
                    field.state.update(cx, |editor, cx| {
                        editor.set_selected_range(range.clone(), cx);
                        if !range.is_empty() {
                            editor.focus(window, cx);
                        }
                    });
                    if laid_out {
                        field.last_scroll_range = scroll_to_range.clone();
                    }
                } else {
                    field.last_scroll_range = None;
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
        let editor = InputBase::new(editor_id.clone())
            .size_full()
            .when_some(self.min_height, |editor, height| editor.min_h(px(height)))
            .overflow_hidden()
            .rounded(px(theme.metrics.radius_small))
            .font_family(theme.typography.monospace_family)
            .text_size(px(theme.typography.body_size))
            .text_color(if self.disabled_appearance {
                theme.colors.actions.disabled_foreground
            } else {
                self.text_color
            })
            .bg(if self.disabled_appearance {
                theme.colors.actions.disabled
            } else {
                theme.colors.surfaces.raised
            })
            .border_1()
            .border_color(if self.disabled_appearance {
                theme.colors.borders.subtle
            } else if focused {
                theme.colors.borders.focused
            } else {
                theme.colors.borders.standard
            })
            .focused(focused)
            .when(!self.disabled_appearance, |editor| {
                editor.styles(move |styles| {
                    styles.focused(move |editor| editor.border_color(theme.colors.borders.focused))
                })
            })
            .when_some(self.debug_selector, |editor, selector| {
                editor.debug_selector(move || selector.into())
            })
            .on_action({
                let search_state = search_state.clone();
                let state = state.clone();
                move |_: &Search, window, cx| {
                    state.update(cx, |editor, cx| {
                        let selected = editor.selected_text().to_string();
                        let query = if selected.is_empty() {
                            editor.search_session().query.clone()
                        } else {
                            selected
                        };
                        editor.set_search_query(query, true, cx);
                    });
                    search_state.update(cx, |search, cx| {
                        search.open();
                        cx.notify();
                    });
                    window.refresh();
                }
            })
            .on_action({
                let search_state = search_state.clone();
                let state = state.clone();
                move |_: &Escape, window, cx| {
                    if search_state.read(cx).is_open() {
                        search_state.update(cx, |search, cx| {
                            search.close();
                            cx.notify();
                        });
                        state.update(cx, |editor, cx| {
                            editor.close_search(cx);
                            editor.focus(window, cx);
                        });
                        window.refresh();
                    } else {
                        // Blur the editor by clearing window focus
                        let focus_handle = state.read(cx).focus_handle(cx);
                        if focus_handle.is_focused(window) {
                            window.blur(cx);
                        }
                    }
                }
            })
            .when(!self.readonly, |editor| {
                editor
                    .on_action({
                        let state = state.clone();
                        move |_: &TabOrIndent, window, cx| {
                            if state.read(cx).focus_handle(cx).is_focused(window) {
                                state.update(cx, |editor, cx| apply_indent(editor, window, cx));
                                cx.stop_propagation();
                            } else {
                                cx.propagate();
                            }
                        }
                    })
                    .on_action({
                        let state = state.clone();
                        move |_: &ShiftTabOrOutdent, window, cx| {
                            if state.read(cx).focus_handle(cx).is_focused(window) {
                                state.update(cx, |editor, cx| apply_outdent(editor, window, cx));
                                cx.stop_propagation();
                            } else {
                                cx.propagate();
                            }
                        }
                    })
            })
            .on_mouse_down(MouseButton::Left, {
                let state = state.clone();
                let on_mouse_down = self.on_mouse_down.clone();
                move |_, window, cx| {
                    if let Some(on_mouse_down) = &on_mouse_down {
                        on_mouse_down(window, cx);
                    }
                    state.update(cx, |editor, cx| editor.focus(window, cx));
                }
            })
            .child(Editor::new(&state));
        let editor = editor_search_card_overlay(
            self.theme,
            editor,
            ElementId::NamedChild(
                Arc::new(editor_id.clone()),
                SharedString::from("search-card"),
            ),
            search_state,
            state.clone(),
            cx,
        );
        let editor = if let Some(variables) = self.variables {
            variable_editor_overlay(
                theme,
                state,
                ElementId::NamedChild(Arc::new(editor_id), SharedString::from("variable-tooltip")),
                editor,
                self.value,
                variables,
                window,
                cx,
            )
        } else {
            editor.into_any_element()
        };
        with_text_context_menu(
            theme,
            &component_id,
            context_menu,
            editor,
            self.extra_context_menu_actions,
            true,
            window,
            cx,
        )
    }
}

fn byte_range_to_utf16(value: &str, range: Range<usize>) -> Range<usize> {
    value[..range.start].encode_utf16().count()..value[..range.end].encode_utf16().count()
}

// Kit 0.7.3 block indentation includes a line whose start ends the selection,
// and uses a fixed tab size. Probe excludes that line and detects existing
// indentation. Keep these edits until native behavior can express both rules.
/// Apply indent to the current selection or line using undo-preserving edit.
pub(super) fn apply_indent(
    editor: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    let value = editor.value();
    let original_selection = editor.selected_range();
    let (new_text, replace_start, replace_end, new_selection) =
        compute_indent(&value, original_selection.clone());
    if replace_start == replace_end && replace_start == original_selection.start {
        editor.insert(new_text, window, cx);
    } else {
        let range_utf16 = byte_range_to_utf16(&value, replace_start..replace_end);
        editor.replace_text_in_range(Some(range_utf16), &new_text, window, cx);
    }
    editor.set_selected_range(new_selection, cx);
}

/// Pure function: compute indented text and new selection.
/// Returns `(new_text, replace_start, replace_end, new_selection)`.
pub(super) fn compute_indent(
    value: &str,
    selection: Range<usize>,
) -> (String, usize, usize, Range<usize>) {
    let indent_str = detect_indentation(value);
    let has_selection = selection.start != selection.end;
    let line_start = value[..selection.start]
        .rfind('\n')
        .map_or(0, |pos| pos + 1);

    if !has_selection && !value[line_start..selection.start].trim().is_empty() {
        let new_position = selection.start + indent_str.len();
        return (
            indent_str,
            selection.start,
            selection.start,
            new_position..new_position,
        );
    }

    let line_end = if selection.end == selection.start {
        value[selection.end..]
            .find('\n')
            .map_or(value.len(), |pos| selection.end + pos)
    } else {
        // For selections, if we're at the start of a line don't include it
        if selection.end > 0 && value.as_bytes().get(selection.end - 1) == Some(&b'\n') {
            selection.end.saturating_sub(1)
        } else {
            value[selection.end..]
                .find('\n')
                .map_or(value.len(), |pos| selection.end + pos)
        }
    };

    let affected_text = &value[line_start..line_end];
    let lines: Vec<&str> = affected_text.split('\n').collect();

    if lines.is_empty() {
        return (String::new(), line_start, line_end, selection);
    }

    // Build indented text
    let mut new_text = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            new_text.push('\n');
        }
        new_text.push_str(&indent_str);
        new_text.push_str(line);
    }

    // Calculate new cursor/selection position
    let indent_len = indent_str.len();
    let new_start = selection.start + indent_len;
    let new_end = selection.end + (indent_len * lines.len());

    (new_text, line_start, line_end, new_start..new_end)
}

/// Apply outdent to the current selection or line using undo-preserving edit.
pub(super) fn apply_outdent(
    editor: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    let value = editor.value();
    let selection = editor.selected_range();
    let (new_text, replace_start, replace_end, new_selection) = compute_outdent(&value, selection);
    if new_text != value[replace_start..replace_end] {
        let range_utf16 = byte_range_to_utf16(&value, replace_start..replace_end);
        editor.replace_text_in_range(Some(range_utf16), &new_text, window, cx);
        editor.set_selected_range(new_selection, cx);
    }
}

/// Pure function: compute outdented text and new selection.
/// Returns `(new_text, replace_start, replace_end, new_selection)`.
pub(super) fn compute_outdent(
    value: &str,
    selection: Range<usize>,
) -> (String, usize, usize, Range<usize>) {
    let indent_str = detect_indentation(value);
    // Expand to cover full lines.
    let line_start = value[..selection.start]
        .rfind('\n')
        .map_or(0, |pos| pos + 1);

    let line_end = if selection.end == selection.start {
        value[selection.end..]
            .find('\n')
            .map_or(value.len(), |pos| selection.end + pos)
    } else {
        // For selections, if we're at the start of a line don't include it
        if selection.end > 0 && value.as_bytes().get(selection.end - 1) == Some(&b'\n') {
            selection.end.saturating_sub(1)
        } else {
            value[selection.end..]
                .find('\n')
                .map_or(value.len(), |pos| selection.end + pos)
        }
    };

    let affected_text = &value[line_start..line_end];
    let lines: Vec<&str> = affected_text.split('\n').collect();

    if lines.is_empty() {
        return (String::new(), line_start, line_end, selection);
    }

    // Build outdented text and track removals
    let mut new_text = String::new();
    let mut removed_before_start = 0;
    let mut removed_before_end = 0;

    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            new_text.push('\n');
        }

        // Calculate this line's byte offset in the original text
        let line_byte_start = line_start
            + if i == 0 {
                0
            } else {
                affected_text[..affected_text
                    .split('\n')
                    .take(i)
                    .map(|l| l.len() + 1)
                    .sum::<usize>()]
                    .len()
            };

        let (trimmed, removed) = if let Some(stripped) = line.strip_prefix(indent_str.as_str()) {
            (stripped, indent_str.len())
        } else if let Some(stripped) = line.strip_prefix('\t') {
            (stripped, 1)
        } else if let Some(stripped) = line.strip_prefix("    ") {
            (stripped, 4)
        } else if let Some(stripped) = line.strip_prefix("  ") {
            (stripped, 2)
        } else {
            (*line, 0)
        };

        // Track removals before selection start
        if line_byte_start < selection.start {
            removed_before_start += removed;
        }
        // Track removals before selection end (includes cursor's own line)
        if line_byte_start < selection.end {
            removed_before_end += removed;
        }

        new_text.push_str(trimmed);
    }

    // Calculate new cursor/selection position
    // For empty caret (start == end), both should move by the same amount
    let new_start = selection.start.saturating_sub(removed_before_start);
    let new_end = if selection.start == selection.end {
        new_start // Keep caret collapsed
    } else {
        selection.end.saturating_sub(removed_before_end)
    };

    (new_text, line_start, line_end, new_start..new_end)
}

/// Detect the indentation style used in the document.
pub(super) fn detect_indentation(text: &str) -> String {
    for line in text.lines() {
        if line.starts_with('\t') {
            return "\t".to_string();
        }
        if line.starts_with("    ") {
            return "    ".to_string();
        }
        if line.starts_with("  ") {
            return "  ".to_string();
        }
    }
    "  ".to_string()
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
