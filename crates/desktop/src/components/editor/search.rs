use super::*;

#[derive(Default)]
pub(super) struct EditorSearchState {
    open: bool,
    focus_input: bool,
}

impl EditorSearchState {
    pub(super) fn open(&mut self) {
        self.open = true;
        self.focus_input = true;
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn close(&mut self) {
        self.open = false;
    }
}

pub(super) fn editor_search_card_overlay(
    theme: Theme,
    editor: impl IntoElement,
    card_id: ElementId,
    search_state: Entity<EditorSearchState>,
    editor_state: Entity<EditorState>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    if !search_state.read(cx).is_open() {
        return editor.into_any_element();
    }

    let focus_input = search_state.update(cx, |search, _| std::mem::take(&mut search.focus_input));
    let session = editor_state.read(cx).search_session();
    let query = session.query.clone();
    let match_count = session.matcher.len();
    let count_label = session.matcher.label();
    let input_id = ElementId::NamedChild(Arc::new(card_id.clone()), SharedString::from("input"));
    let previous_id =
        ElementId::NamedChild(Arc::new(card_id.clone()), SharedString::from("previous"));
    let next_id = ElementId::NamedChild(Arc::new(card_id.clone()), SharedString::from("next"));
    let close_id = ElementId::NamedChild(Arc::new(card_id.clone()), SharedString::from("close"));
    let query_editor = editor_state.clone();
    let enter_editor = editor_state.clone();
    let previous_state = search_state.clone();
    let previous_editor = editor_state.clone();
    let next_state = search_state.clone();
    let next_editor = editor_state.clone();
    let close_state = search_state.clone();
    let close_editor = editor_state.clone();
    let escape_state = search_state.clone();
    let escape_editor = editor_state.clone();

    let mut input = text_input_base(theme, input_id, query, "Search");
    input.width = Some(150.0);
    input.height = theme.metrics.control_height - 2.0;
    input.text_size = theme.typography.caption_size;
    input.debug_selector = Some("editor-search-input");
    input.content_gap = theme.metrics.spacing_1;
    input.leading_icon = Some(
        library_icon("lucide-search", &SEARCH_SVG, theme.metrics.icon_small)
            .text_color(theme.colors.text.muted),
    );
    input.focus_on_render = focus_input;
    input.on_change = Some(Rc::new(move |value, window, cx| {
        query_editor.update(cx, |editor, cx| {
            editor.set_search_query(value.to_string(), true, cx);
            // Reveal the current match without changing the active occurrence.
            editor.previous_search_match(cx);
            editor.next_search_match(cx);
        });
        window.refresh();
    }));
    input.on_enter = Some(Rc::new(move |_, window, cx| {
        enter_editor.update(cx, |editor, cx| {
            editor.next_search_match(cx);
        });
        window.refresh();
    }));

    div()
        .relative()
        .size_full()
        .min_h(px(0.0))
        .child(editor)
        .child(
            div()
                .id(card_id)
                .debug_selector(|| "editor-search-card".into())
                .key_context("ProbeEditorSearch")
                .on_action(move |_: &Escape, window, cx| {
                    escape_state.update(cx, |search, cx| {
                        search.close();
                        cx.notify();
                    });
                    escape_editor.update(cx, |editor, cx| {
                        editor.close_search(cx);
                        editor.focus(window, cx);
                    });
                })
                .block_mouse_except_scroll()
                .absolute()
                .top(px(theme.metrics.spacing_2))
                .right(px(theme.metrics.spacing_2))
                .h(px(theme.metrics.control_height + 8.0))
                .flex()
                .items_center()
                .gap(px(theme.metrics.spacing_1))
                .pl(px(theme.metrics.spacing_1))
                .pr(px(theme.metrics.spacing_1))
                .rounded(px(theme.metrics.radius_medium))
                .bg(theme.colors.surfaces.overlay)
                .border_1()
                .border_color(theme.colors.borders.standard)
                .shadow(temporary_surface_shadow(theme, 8.0))
                .child(
                    div()
                        .w(px(150.0))
                        .flex_none()
                        .mr(px(theme.metrics.spacing_1))
                        .child(input),
                )
                .child(
                    div()
                        .flex_none()
                        .text_align(TextAlign::Center)
                        .text_size(px(theme.typography.caption_size))
                        .text_color(theme.colors.text.secondary)
                        .child(count_label),
                )
                .child(search_card_button(
                    theme,
                    previous_id,
                    "Previous search result",
                    chevron_icon(theme, true),
                    match_count == 0,
                    move |event, window, cx| {
                        previous_editor.update(cx, |editor, cx| {
                            editor.previous_search_match(cx);
                        });
                        window.refresh();
                        if matches!(event, ClickEvent::Mouse(_)) {
                            previous_state.update(cx, |search, cx| {
                                search.focus_input = true;
                                cx.notify();
                            });
                        }
                    },
                ))
                .child(search_card_button(
                    theme,
                    next_id,
                    "Next search result",
                    chevron_icon(theme, false),
                    match_count == 0,
                    move |event, window, cx| {
                        next_editor.update(cx, |editor, cx| {
                            editor.next_search_match(cx);
                        });
                        window.refresh();
                        if matches!(event, ClickEvent::Mouse(_)) {
                            next_state.update(cx, |search, cx| {
                                search.focus_input = true;
                                cx.notify();
                            });
                        }
                    },
                ))
                .child(search_card_button(
                    theme,
                    close_id,
                    "Close search",
                    close_icon(theme),
                    false,
                    move |_, window, cx| {
                        close_state.update(cx, |search, cx| {
                            search.close();
                            cx.notify();
                        });
                        close_editor.update(cx, |editor, cx| {
                            editor.close_search(cx);
                            editor.focus(window, cx);
                        });
                    },
                )),
        )
        .into_any_element()
}

fn search_card_button(
    theme: Theme,
    id: ElementId,
    aria_label: impl Into<SharedString>,
    icon: impl IntoElement,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let size = theme.metrics.control_height - 2.0;
    Button::new(id)
        .accessibility_label(aria_label)
        .disabled(disabled)
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme.metrics.radius_small))
        .bg(transparent_black())
        .border_1()
        .border_color(transparent_black())
        .cursor_pointer()
        .hover(move |button| button.bg(theme.colors.selection.inactive_background))
        .focus_visible(move |button| button.border_color(theme.colors.borders.focused))
        .on_click(on_click)
        .child(icon)
}

pub(in crate::components) fn body_text_highlights(
    value: &str,
    references: &[super::variables::VariableReference],
    palette: super::variables::VariableHighlightPalette,
    mut tone_for: impl FnMut(super::variables::ReferenceKind, &str) -> super::variables::PlaceholderTone,
) -> Vec<TextDecoration> {
    references
        .iter()
        .map(|reference| {
            let (color, underline) = super::variables::placeholder_paint(
                tone_for(reference.kind, reference.name(value)),
                palette,
            );
            TextDecoration::new(
                reference.range.clone(),
                HighlightStyle {
                    color: Some(color),
                    underline,
                    ..Default::default()
                },
            )
        })
        .collect()
}

pub(super) fn text_decoration(
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
