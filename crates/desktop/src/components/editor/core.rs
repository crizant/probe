use super::*;

struct EditorField {
    state: Entity<EditorState>,
    decorations: TextDecorationCollection,
    last_decorations: Vec<TextDecoration>,
    last_value: SharedString,
    on_change: Option<InputChangeHandler>,
    last_scroll_range: Option<Range<usize>>,
    language: SharedString,
    soft_wrap: bool,
    readonly: bool,
    _subscription: Subscription,
}

pub(in crate::components) fn editor_value_needs_refresh(
    language_changed: bool,
    current_value: &SharedString,
    next_value: &SharedString,
) -> bool {
    language_changed
        || (!std::ptr::eq(
            current_value.as_ref() as *const str,
            next_value.as_ref() as *const str,
        ) && current_value != next_value)
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
            let selection = input.read(cx).selected_range();

            // Auto-pairing: check if we should insert a closing character
            if !this.readonly
                && let Some(pair_result) = detect_auto_pair(&this.last_value, &value, selection)
            {
                input.update(cx, |editor, cx| {
                    // Save current selection
                    let saved_selection = editor.selected_range();
                    // Set selection to insertion point
                    editor.set_selected_range(pair_result.cursor..pair_result.cursor, cx);
                    // Insert the closing character
                    editor.insert(pair_result.closing_char.to_string(), window, cx);
                    // Restore cursor position (between the pair)
                    editor.set_selected_range(saved_selection, cx);
                });
                // Update last_value to the new text with the pair
                this.last_value = input.read(cx).value();

                if let Some(on_change) = this.on_change.clone() {
                    let current_value = input.read(cx).value();
                    on_change(current_value, window, cx);
                }
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
pub(in crate::components) fn editor_paint_style(theme: Theme) -> InputEditorStyle {
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
    pub(in crate::components) min_height: Option<f32>,
    pub(in crate::components) padding: EditorInsets,
    pub(in crate::components) soft_wrap: bool,
    pub(in crate::components) text_color: gpui::Rgba,
    pub(in crate::components) scroll_to_range: Option<Range<usize>>,
    pub(in crate::components) search_matches: Vec<(Range<usize>, bool)>,
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
                    .folding(false)
                    .searchable(true)
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
                last_value: self.value.clone(),
                on_change: on_change.clone(),
                last_scroll_range: None,
                language: language.clone(),
                soft_wrap,
                readonly: self.readonly,
                _subscription: subscription,
            }
        });
        field.update(cx, |field, cx| {
            field.on_change = self.on_change.clone();
            field.readonly = self.readonly;
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
            let value_changed =
                editor_value_needs_refresh(language_changed, &field.last_value, &self.value);
            if value_changed {
                field.last_value = self.value.clone();
            }
            field.state.update(cx, |editor, cx| {
                editor.set_editor_style(editor_paint_style(self.theme));
                editor.set_editor_paddings(self.padding.edges());
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
                }
                // gpui-base clears its parser when the language changes and
                // rebuilds it on the next text update. Pretty and Raw XML are
                // often byte-identical, so force that update when only the
                // language changed as well.
                if value_changed {
                    editor.set_value(self.value.clone(), window, cx);
                }
                if editor.search_session().open {
                    let query = editor.search_session().query.clone();
                    let active_match = editor.search_session().matcher.current_match_index();
                    search_state.update(cx, |search, cx| {
                        search.open();
                        if !query.is_empty() {
                            search.set_query(query);
                        }
                        search.set_active_match(active_match);
                        cx.notify();
                    });
                    editor.close_search(cx);
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
        let (matches, active_match) = if search_state.read(cx).is_open() {
            let matches = state.read(cx).search_session().matcher.matched_ranges();
            let active_match = search_state
                .read(cx)
                .active_match()
                .min(matches.len().saturating_sub(1));
            if active_match != search_state.read(cx).active_match() {
                search_state.update(cx, |search, _| search.set_active_match(active_match));
            }
            (matches, active_match)
        } else {
            (Rc::new(Vec::new()), 0)
        };
        let local_search_matches = matches
            .iter()
            .enumerate()
            .map(|(index, range)| (range.clone(), index == active_match))
            .collect::<Vec<_>>();
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
            .key_context("ProbeEditor")
            .on_action({
                let search_state = search_state.clone();
                move |_: &Search, window, cx| {
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
                        state.update(cx, |editor, cx| editor.focus(window, cx));
                        window.refresh();
                    } else {
                        // Blur the editor by clearing window focus
                        let focus_handle = state.read(cx).focus_handle(cx);
                        if focus_handle.is_focused(window) {
                            window.blur();
                        }
                    }
                }
            })
            .on_action({
                let field = field.clone();
                let state = state.clone();
                move |_: &crate::app::IndentLine, window, cx| {
                    let readonly = field.read(cx).readonly;
                    if readonly {
                        // In readonly mode, Tab should navigate focus
                        window.focus_next(cx);
                        return;
                    }

                    state.update(cx, |editor, cx| {
                        apply_indent(editor, window, cx);
                    });
                }
            })
            .on_action({
                let field = field.clone();
                let state = state.clone();
                move |_: &crate::app::OutdentLine, window, cx| {
                    let readonly = field.read(cx).readonly;
                    if readonly {
                        // In readonly mode, Shift+Tab should navigate focus backwards
                        window.focus_prev(cx);
                        return;
                    }

                    state.update(cx, |editor, cx| {
                        apply_outdent(editor, window, cx);
                    });
                }
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
            .child(div().size_full().child(Editor::new(&state)));
        let editor = response_search_highlight_overlay(
            self.theme,
            state.clone(),
            editor,
            self.value.clone(),
            [self.search_matches, local_search_matches].concat(),
        );
        let editor = editor_search_card_overlay(
            self.theme,
            editor,
            ElementId::NamedChild(
                Arc::new(editor_id.clone()),
                SharedString::from("search-card"),
            ),
            search_state,
            matches,
            active_match,
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

/// Apply indent to the current selection or line using undo-preserving edit.
pub(super) fn apply_indent(
    editor: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    let value = editor.value();
    let indent_str = detect_indentation(&value);
    let selection = editor.selected_range();

    // Expand to cover full lines
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
        return;
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

    // Apply the edit: select the range and replace it
    editor.set_selected_range(line_start..line_end, cx);
    editor.replace(new_text, window, cx);
    // Restore selection
    editor.set_selected_range(new_start..new_end, cx);
}

/// Apply outdent to the current selection or line using undo-preserving edit.
pub(super) fn apply_outdent(
    editor: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    let value = editor.value();
    let indent_str = detect_indentation(&value);
    let selection = editor.selected_range();

    // Expand to cover full lines
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
        return;
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

    // Apply the edit: select the range and replace it
    editor.set_selected_range(line_start..line_end, cx);
    editor.replace(new_text, window, cx);
    // Restore selection
    editor.set_selected_range(new_start..new_end, cx);
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

pub(super) struct AutoPairResult {
    pub(super) closing_char: char,
    pub(super) cursor: usize,
}

/// Detect if auto-pairing should be applied.
/// Returns Some with the closing character and cursor position if pairing should happen.
pub(super) fn detect_auto_pair(
    old_value: &SharedString,
    new_value: &SharedString,
    selection: Range<usize>,
) -> Option<AutoPairResult> {
    // Only auto-pair on single character insertion with empty selection
    if old_value.len() + 1 != new_value.len() || selection.start != selection.end {
        return None;
    }

    let cursor = selection.start;
    if cursor == 0 {
        return None;
    }

    // Get the inserted character by finding the difference
    // We need to be careful with UTF-8 byte offsets
    let old_bytes = old_value.as_bytes();
    let new_bytes = new_value.as_bytes();

    // Find where the insertion happened by comparing bytes
    let mut insert_pos = 0;
    while insert_pos < old_bytes.len().min(new_bytes.len())
        && old_bytes[insert_pos] == new_bytes[insert_pos]
    {
        insert_pos += 1;
    }

    // The inserted character starts at insert_pos in new_bytes
    if insert_pos >= new_bytes.len() {
        return None;
    }

    // Decode the character at insert_pos
    let remaining = &new_bytes[insert_pos..];
    let inserted_char = std::str::from_utf8(remaining).ok()?.chars().next()?;

    let closing_char = match inserted_char {
        '"' => '"',
        '\'' => '\'',
        '`' => '`',
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return None,
    };

    // Check if the next character is already the closing character
    // cursor is the byte offset after insertion
    if cursor < new_value.len()
        && let Some(next_char) = new_value[cursor..].chars().next()
        && next_char == closing_char
    {
        return None;
    }

    Some(AutoPairResult {
        closing_char,
        cursor,
    })
}
