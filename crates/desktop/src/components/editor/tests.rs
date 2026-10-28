use std::collections::BTreeMap;

use gpui_kit::base::{Input, InputBase, input::InputState};
use gpui_kit::{
    Context, Entity, IntoElement, Render, SharedString, TestAppContext, VisualTestContext, hsla,
    point, prelude::*, px, size, transparent_black,
};

use super::core::detect_indentation;
use super::*;
use crate::theme::Theme;
use probe_core::VariableStatus;

#[test]
fn editor_paint_style_uses_visible_caret_and_selection() {
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
    assert_eq!(&value[ranges[0].range.clone()], "{{host}}");
    assert_eq!(ranges[0].name(value), "host");
    assert_eq!(ranges[0].kind, ReferenceKind::Environment);
    assert_eq!(&value[ranges[1].range.clone()], "{{id}}");
    assert_eq!(ranges[1].name(value), "id");
}

#[test]
fn variable_ranges_trim_names_and_find_placeholders_in_json() {
    let value = "{\n  \"tenant\": \"{{ tenant }}\"\n}";
    let ranges = variable_ranges(value);
    assert_eq!(ranges.len(), 1);
    assert_eq!(&value[ranges[0].range.clone()], "{{ tenant }}");
    assert_eq!(ranges[0].name(value), "tenant");
}

#[test]
fn variable_tooltip_presentation_handles_variable_and_secret_states() {
    let mut values = BTreeMap::new();
    values.insert("host".to_owned(), "api.example".to_owned());
    values.insert("empty".to_owned(), String::new());
    let variables = VariableContext {
        values,
        secrets: ["token".to_owned()].into_iter().collect(),
        unavailable_message: "unavailable".to_owned(),
        on_change: Some(std::rc::Rc::new(|_, _, _, _| {})),
        ..VariableContext::default()
    };
    let existing = variable_tooltip_presentation("host", &variables);
    assert_eq!(existing.value, "api.example");
    assert!(existing.editable);
    assert!(existing.hint.is_none());

    let missing = variable_tooltip_presentation("created", &variables);
    assert_eq!(missing.value, "");
    assert_eq!(missing.placeholder, "Enter a value to create");
    assert!(missing.editable);
    assert_eq!(missing.hint, Some("Not defined in this environment"));

    let empty = variable_tooltip_presentation("empty", &variables);
    assert_eq!(empty.value, "");
    assert!(empty.editable);
    assert!(empty.hint.is_none());

    let secret = variable_tooltip_presentation("token", &variables);
    assert!(secret.value.is_empty());
    assert!(!secret.editable);
    assert_eq!(secret.secret, Some(SecretTooltipState::NotStored));
    assert!(secret.hint.is_none());
    assert_eq!(secret.secret.unwrap().status_text(), "○ Not set");
    assert_eq!(secret.secret.unwrap().action_label(), "Set Secret…");

    let stored = VariableContext {
        resolved_secrets: ["apiKey".to_owned()].into_iter().collect(),
        on_change: Some(std::rc::Rc::new(|_, _, _, _| {})),
        ..VariableContext::default()
    };
    assert_eq!(stored.status("apiKey"), VariableStatus::Resolved);
    assert_eq!(
        reference_status(&stored, ReferenceKind::Environment, "apiKey"),
        VariableStatus::Resolved
    );
    let stored_tooltip = variable_tooltip_presentation("apiKey", &stored);
    assert!(stored_tooltip.value.is_empty());
    assert!(!stored_tooltip.editable);
    assert_eq!(stored_tooltip.secret, Some(SecretTooltipState::Stored));
    assert!(stored_tooltip.hint.is_none());
    assert_eq!(
        stored_tooltip.secret.unwrap().status_text(),
        "● Stored securely"
    );
    assert_eq!(
        stored_tooltip.secret.unwrap().action_label(),
        "Replace Secret…"
    );
    for theme in [Theme::light(), Theme::dark()] {
        assert_eq!(
            stored_tooltip.secret.unwrap().status_color(theme),
            theme.colors.status.success
        );
        assert_eq!(
            secret.secret.unwrap().status_color(theme),
            theme.colors.text.muted
        );
    }
    assert!(!stored.values.contains_key("apiKey"));
    let palette = variable_highlight_palette(Theme::light());
    let (color, underline) =
        super::variables::placeholder_paint(PlaceholderTone::Resolved, palette);
    assert!(underline.is_none());
    assert_eq!(color, palette.resolved);

    let unknown = VariableContext {
        unknown_secrets: ["secretToken".to_owned()].into_iter().collect(),
        ..VariableContext::default()
    };
    let unknown_tooltip = variable_tooltip_presentation("secretToken", &unknown);
    assert!(unknown_tooltip.value.is_empty());
    assert!(!unknown_tooltip.editable);
    assert_eq!(unknown_tooltip.secret, Some(SecretTooltipState::Unknown));
    assert!(unknown_tooltip.hint.is_none());
    assert_eq!(
        unknown_tooltip.secret.unwrap().status_text(),
        "Not verified"
    );
    assert_eq!(
        unknown_tooltip.secret.unwrap().action_label(),
        "Set Secret…"
    );
    let identity_error = VariableContext {
        values: [("host".to_owned(), "api.example.com".to_owned())]
            .into_iter()
            .collect(),
        secret_identity_errors: [(String::new(), "invalid credential identity".to_owned())]
            .into_iter()
            .collect(),
        on_change: Some(std::rc::Rc::new(|_, _, _, _| {})),
        ..VariableContext::default()
    };
    let host = variable_tooltip_presentation("host", &identity_error);
    assert_eq!(host.value, "api.example.com");
    assert!(host.editable);
    assert!(host.secret.is_none());
    let failed_secret = variable_tooltip_presentation("", &identity_error);
    assert_eq!(failed_secret.value, "invalid credential identity");
    assert!(!failed_secret.editable);
    assert_eq!(failed_secret.secret, Some(SecretTooltipState::Unknown));
    for theme in [Theme::light(), Theme::dark()] {
        assert_eq!(
            unknown_tooltip.secret.unwrap().status_color(theme),
            theme.colors.text.muted
        );
    }
    assert_ne!(
        unknown.status("secretToken"),
        VariableStatus::SecretWithoutValue
    );
    let (color, underline) = super::variables::placeholder_paint(
        placeholder_tone(&unknown, ReferenceKind::Environment, "secretToken"),
        palette,
    );
    assert!(underline.is_none());
    assert_eq!(color, palette.neutral);
}

#[test]
fn variable_tooltip_presentation_keeps_unavailable_when_not_writable() {
    let variables = VariableContext {
        unavailable_message: "Select an environment".to_owned(),
        ..VariableContext::default()
    };
    let missing = variable_tooltip_presentation("created", &variables);
    assert!(missing.value.is_empty());
    assert_eq!(
        missing.unavailable_message.as_deref(),
        Some("Select an environment")
    );
    assert!(!missing.editable);
    assert!(missing.hint.is_none());
}

#[test]
fn body_text_highlights_overlay_mustache_variables() {
    let value = "{\"host\":\"{{host}}\"}";
    let ranges = variable_ranges(value);
    let palette = stand_in_palette();
    let highlights =
        body_text_highlights(value, &ranges, palette, |_, _| PlaceholderTone::Resolved);
    assert_eq!(highlights.len(), 1);
    assert_eq!(&value[highlights[0].range.clone()], "{{host}}");
    assert_eq!(highlights[0].style.color, Some(palette.resolved));
    assert!(highlights[0].style.underline.is_none());
}

#[test]
fn body_text_highlights_underline_unresolved_mustache_spans() {
    let value = "{\"host\":\"{{host}}\",\"missing\":\"{{missing}}\"}";
    let ranges = variable_ranges(value);
    let palette = stand_in_palette();
    let highlights = body_text_highlights(value, &ranges, palette, |_, name| {
        if name == "host" {
            PlaceholderTone::Resolved
        } else {
            PlaceholderTone::Unresolved
        }
    });
    assert_eq!(highlights.len(), 2);
    assert_eq!(highlights[0].style.color, Some(palette.resolved));
    assert!(highlights[0].style.underline.is_none());
    assert_eq!(&value[highlights[1].range.clone()], "{{missing}}");
    assert_eq!(highlights[1].style.color, Some(palette.unresolved));
    let underline = highlights[1]
        .style
        .underline
        .expect("unresolved placeholders keep a non-color cue");
    assert_eq!(underline.thickness, px(1.0));
    assert_eq!(underline.color, Some(palette.unresolved));
}

#[test]
fn variable_highlight_runs_color_only_mustache_spans() {
    let value = "{{host}}/users";
    let ranges = variable_ranges(value);
    let palette = stand_in_palette();
    let base = gpui_kit::TextRun {
        len: value.len(),
        font: gpui_kit::Font::default(),
        color: transparent_black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs = variable_highlight_runs(value, &ranges, &base, palette, |_, _| {
        PlaceholderTone::Resolved
    });
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].color, palette.resolved);
    assert!(runs[0].underline.is_none());
    assert_eq!(runs[0].len, "{{host}}".len());
    assert_eq!(runs[1].color, transparent_black());
    assert_eq!(runs[1].len, "/users".len());
}

#[test]
fn variable_highlight_runs_underline_unresolved_mustache_spans() {
    let value = "{{host}}/{{missing}}";
    let ranges = variable_ranges(value);
    let palette = stand_in_palette();
    let base = gpui_kit::TextRun {
        len: value.len(),
        font: gpui_kit::Font::default(),
        color: hsla(0.0, 0.0, 0.2, 1.0),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs = variable_highlight_runs(value, &ranges, &base, palette, |_, name| {
        if name == "host" {
            PlaceholderTone::Resolved
        } else {
            PlaceholderTone::Unresolved
        }
    });
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].color, palette.resolved);
    assert!(runs[0].underline.is_none());
    assert_eq!(runs[1].color, base.color);
    assert!(runs[1].underline.is_none());
    assert_eq!(runs[2].color, palette.unresolved);
    assert_eq!(runs[2].len, "{{missing}}".len());
    let underline = runs[2]
        .underline
        .expect("unresolved placeholders keep a non-color cue");
    assert_eq!(underline.thickness, px(1.0));
    assert_eq!(underline.color, Some(palette.unresolved));
}

#[test]
fn variable_highlight_runs_clamp_overlapping_url_placeholders() {
    let value = "{{a:b}}";
    let ranges = input_variable_ranges(value, true);
    let palette = stand_in_palette();
    let base = gpui_kit::TextRun {
        len: value.len(),
        font: gpui_kit::Font::default(),
        color: transparent_black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut classified = Vec::new();
    let runs = variable_highlight_runs(value, &ranges, &base, palette, |kind, name| {
        classified.push((kind, name.to_owned()));
        PlaceholderTone::Resolved
    });
    assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), value.len());
    assert_eq!(
        classified,
        vec![(ReferenceKind::Environment, "a:b".to_owned())]
    );
}

#[test]
fn path_placeholders_are_classified_apart_from_matching_environment_names() {
    let value = "https://{{host}}/users/:host";
    let references = input_variable_ranges(value, true);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].kind, ReferenceKind::Environment);
    assert_eq!(references[0].name(value), "host");
    assert_eq!(&value[references[0].range.clone()], "{{host}}");
    assert_eq!(references[1].kind, ReferenceKind::Path);
    assert_eq!(references[1].name(value), "host");
    assert_eq!(&value[references[1].range.clone()], ":host");

    let environment_only = VariableContext {
        values: BTreeMap::from([("host".to_owned(), "api.example".to_owned())]),
        ..VariableContext::default()
    };
    assert_eq!(
        reference_status(&environment_only, ReferenceKind::Environment, "host"),
        VariableStatus::Resolved
    );
    assert_eq!(
        reference_status(&environment_only, ReferenceKind::Path, "host"),
        VariableStatus::Missing
    );

    let path_only = VariableContext::default().with_path_values(&[probe_core::QueryParameter {
        name: "host".to_owned(),
        value: "42".to_owned(),
        disabled: false,
    }]);
    assert_eq!(
        reference_status(&path_only, ReferenceKind::Environment, "host"),
        VariableStatus::Missing
    );
    assert_eq!(
        reference_status(&path_only, ReferenceKind::Path, "host"),
        VariableStatus::Resolved
    );

    let blank_or_disabled = VariableContext::default().with_path_values(&[
        probe_core::QueryParameter {
            name: "blank".to_owned(),
            value: String::new(),
            disabled: false,
        },
        probe_core::QueryParameter {
            name: "off".to_owned(),
            value: "kept".to_owned(),
            disabled: true,
        },
    ]);
    assert_eq!(
        reference_status(&blank_or_disabled, ReferenceKind::Path, "blank"),
        VariableStatus::Missing
    );
    assert_eq!(
        reference_status(&blank_or_disabled, ReferenceKind::Path, "off"),
        VariableStatus::Missing
    );
}

#[test]
fn variable_highlight_runs_paint_non_variable_text_with_the_base_color() {
    let value = "https://{{host}}/users";
    let ranges = variable_ranges(value);
    let base_color = hsla(0.0, 0.0, 0.25, 1.0);
    let palette = stand_in_palette();
    let base = gpui_kit::TextRun {
        len: value.len(),
        font: gpui_kit::Font::default(),
        color: base_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let runs = variable_highlight_runs(value, &ranges, &base, palette, |_, _| {
        PlaceholderTone::Resolved
    });

    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].color, base_color);
    assert_eq!(runs[1].color, palette.resolved);
    assert!(runs[1].underline.is_none());
    assert_eq!(runs[2].color, base_color);
}

fn stand_in_palette() -> VariableHighlightPalette {
    VariableHighlightPalette {
        resolved: hsla(0.33, 0.6, 0.5, 1.0),
        unresolved: hsla(0.02, 0.75, 0.46, 1.0),
        neutral: hsla(0.0, 0.0, 0.45, 1.0),
    }
}

struct HighlightHarness {
    input: Entity<InputState>,
}

impl Render for HighlightHarness {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        InputBase::new("variable-highlight-harness-input")
            .w(px(160.0))
            .h(px(24.0))
            .child(Input::new(&self.input))
    }
}

fn long_variable_url() -> SharedString {
    SharedString::from(format!(
        "{{{{sdfsdfsd}}}}{}",
        "kjlkjlkjlkjlkjlkjlkjflsdjflkjsdlfkjsldkjflskdjflkjlfjlsdj".repeat(2)
    ))
}

#[gpui_kit::test]
fn variable_highlight_scrolls_with_caret_at_end_of_long_url(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let value = long_variable_url();
    let window = cx.open_window(size(px(240.0), px(48.0)), |window, cx| HighlightHarness {
        input: cx.new(|cx| InputState::new(window, cx)),
    });
    cx.run_until_parked();
    let input = window
        .update(cx, |harness, window, cx| {
            harness.input.update(cx, |input, cx| {
                input.focus(window, cx);
            });
            harness.input.clone()
        })
        .expect("highlight test window should be open");
    cx.simulate_input(window.into(), value.as_ref());
    cx.run_until_parked();
    let native_scroll_offset = input.read_with(cx, |input, _| input.scroll_offset().x);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let visible = size(px(160.0), px(24.0));
    let (_, prepaint) = visual.draw(point(px(0.0), px(0.0)), visible, |_, _| {
        VariableHighlightElement {
            tooltip_geometry: None,
            state: input.clone(),
            base_color: transparent_black(),
            palette: stand_in_palette(),
            highlight_path_variables: false,
            variables: VariableContext::default(),
        }
    });

    assert!(
        prepaint.scroll_offset < px(0.0),
        "long URL with caret at the end should scroll highlights left, got {:?}",
        prepaint.scroll_offset
    );
    assert_eq!(
        prepaint.scroll_offset, native_scroll_offset,
        "the variable overlay must use gpui-base's finalized scroll offset"
    );
}

#[gpui_kit::test]
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
    cx.run_until_parked();
    let input = window
        .update(cx, |harness, _window, cx| {
            harness.input.update(cx, |input, cx| {
                input.set_selected_range(0..0, cx);
            });
            harness.input.clone()
        })
        .expect("highlight test window should be open");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let (_, prepaint) = visual.draw(
        point(px(0.0), px(0.0)),
        size(px(160.0), px(24.0)),
        |_, _| VariableHighlightElement {
            tooltip_geometry: None,
            state: input.clone(),
            base_color: transparent_black(),
            palette: stand_in_palette(),
            highlight_path_variables: false,
            variables: VariableContext::default(),
        },
    );

    assert_eq!(
        prepaint.scroll_offset,
        px(0.0),
        "caret at the start should keep the variable highlight at its origin"
    );
    window
        .update(cx, |_, _, cx| {
            input.update(cx, |state, cx| {
                state.set_scroll_offset(point(px(-40.0), px(0.0)), cx)
            });
        })
        .unwrap();
    visual.run_until_parked();
    let native = input.read_with(cx, |state, _| state.scroll_offset().x);
    assert_eq!(native, px(-40.0));
    let (_, prepaint) = visual.draw(
        point(px(0.0), px(0.0)),
        size(px(160.0), px(24.0)),
        |_, _| VariableHighlightElement {
            tooltip_geometry: None,
            state: input.clone(),
            base_color: transparent_black(),
            palette: stand_in_palette(),
            highlight_path_variables: false,
            variables: VariableContext::default(),
        },
    );
    assert_eq!(
        prepaint.scroll_offset, native,
        "manual scrolling may leave the caret offscreen; highlights must follow the native offset"
    );
}

#[test]
fn detect_indentation_uses_existing_indent_or_defaults_to_two_spaces() {
    for (value, expected) in [
        ("\tindented", "\t"),
        ("    indented", "    "),
        ("  indented", "  "),
        ("no indentation", "  "),
    ] {
        assert_eq!(detect_indentation(value), expected, "{value:?}");
    }
}

#[test]
fn compute_indent_on_single_line_with_empty_caret() {
    let value = "hello";
    let selection = 2..2; // Caret at 'l'
    let (new_text, replace_start, replace_end, new_selection) =
        super::core::compute_indent(value, selection);
    assert_eq!(new_text, "  ");
    assert_eq!(replace_start, 2);
    assert_eq!(replace_end, 2);
    assert_eq!(new_selection, 4..4); // Caret moved by 2
}

#[test]
fn compute_indent_on_multiline_selection() {
    let value = "line1\nline2\nline3";
    let selection = 3..10; // From "e1" to "e2"
    let (new_text, line_start, line_end, new_selection) =
        super::core::compute_indent(value, selection);
    // Only returns affected lines (line1 and line2, not line3)
    assert_eq!(new_text, "  line1\n  line2");
    assert_eq!(line_start, 0);
    assert_eq!(line_end, 11); // "line1\nline2".len()
    // Selection expands by 2 spaces per line (2 lines affected)
    assert_eq!(new_selection.start, 5); // 3 + 2
    assert_eq!(new_selection.end, 14); // 10 + 2*2
}

#[test]
fn compute_indent_on_partial_line_selection() {
    let value = "abc\ndef\nghi";
    let selection = 5..9; // From "ef" to "gh"
    let (new_text, line_start, line_end, new_selection) =
        super::core::compute_indent(value, selection);
    // Should indent both lines that touch the selection
    assert_eq!(new_text, "  def\n  ghi");
    assert_eq!(line_start, 4);
    assert_eq!(line_end, 11);
    assert_eq!(new_selection.start, 7); // 5 + 2
    assert_eq!(new_selection.end, 13); // 9 + 2*2
}

#[test]
fn compute_outdent_on_single_line_with_empty_caret() {
    let value = "  hello";
    let selection = 2..2; // Caret after "  " (only whitespace before)
    let (new_text, replace_start, replace_end, new_selection) =
        super::core::compute_outdent(value, selection);
    // Hybrid: caret with only whitespace before → whole-line outdent
    assert_eq!(new_text, "hello");
    assert_eq!(replace_start, 0);
    assert_eq!(replace_end, 7);
    assert_eq!(new_selection, 0..0); // Caret at start after outdent
}

#[test]
fn compute_outdent_on_multiline_with_partial_selection() {
    let value = "  line1\n  line2\n  line3";
    let selection = 5..14; // From "e1" to "e2"
    let (new_text, line_start, line_end, new_selection) =
        super::core::compute_outdent(value, selection);
    // Only returns affected lines (line1 and line2, not line3)
    assert_eq!(new_text, "line1\nline2");
    assert_eq!(line_start, 0);
    assert_eq!(line_end, 15); // "  line1\n  line2".len()
    // Selection shrinks by removed indents
    assert_eq!(new_selection.start, 3); // 5 - 2 (from line1)
    assert_eq!(new_selection.end, 10); // 14 - 2 (line1) - 2 (line2)
}

#[test]
fn compute_outdent_on_mixed_indentation() {
    let value = "  line1\n    line2";
    let selection = 3..12; // Spans both lines
    let (new_text, line_start, line_end, new_selection) =
        super::core::compute_outdent(value, selection);
    // Second line has 4 spaces, but indent_str is "  " (2 spaces based on first line)
    // So it removes 2 spaces from both lines
    assert_eq!(new_text, "line1\n  line2");
    assert_eq!(line_start, 0);
    assert_eq!(line_end, value.len());
    // Both lines lose 2 spaces each
    assert_eq!(new_selection.start, 1); // 3 - 2
    assert_eq!(new_selection.end, 8); // 12 - 2 (line1) - 2 (line2)
}

#[test]
fn compute_outdent_on_lines_with_no_indentation() {
    let value = "line1\nline2";
    let selection = 2..8;
    let (new_text, _, _, new_selection) = super::core::compute_outdent(value, selection);
    assert_eq!(new_text, "line1\nline2");
    // No change since there's no indentation
    assert_eq!(new_selection, 2..8);
}

#[test]
fn compute_indent_mid_line_inserts_at_caret() {
    let (text, start, end, selection) = super::core::compute_indent("hello world", 6..6);
    assert_eq!(text, "  ");
    assert_eq!((start, end, selection), (6, 6, 8..8));
}

#[test]
fn compute_indent_with_selection_indents_the_whole_line() {
    let (text, start, end, selection) = super::core::compute_indent("hello world", 3..7);
    assert_eq!(text, "  hello world");
    assert_eq!((start, end, selection), (0, 11, 5..9));
}

#[test]
fn compute_outdent_mid_line_preserves_inline_payload_whitespace() {
    let (text, start, end, selection) = super::core::compute_outdent("  hello  world", 9..9);
    assert_eq!(text, "hello  world");
    assert_eq!((start, end, selection), (0, 14, 7..7));
}

#[test]
fn compute_outdent_at_end_of_indented_line_removes_leading_indent() {
    let value = "  hello";
    let (text, start, end, selection) = super::core::compute_outdent(value, 7..7);
    assert_eq!(text, "hello");
    assert_eq!((start, end, selection), (0, value.len(), 5..5));
}

struct TooltipScrollHarness {
    input: Entity<InputState>,
    origin_shift: Pixels,
    renders: usize,
}

impl Render for TooltipScrollHarness {
    fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::light();
        self.renders += 1;
        div()
            .ml(self.origin_shift)
            .w(px(240.0))
            .h(px(24.0))
            .child(variable_input_overlay(
                theme,
                self.input.clone(),
                "tooltip-scroll".into(),
                InputBase::new("tooltip-scroll-input")
                    .w_full()
                    .child(Input::new(&self.input)),
                self.input.read(cx).value(),
                VariableContext::default(),
                false,
                window,
                cx,
            ))
    }
}

#[gpui_kit::test]
fn input_tooltip_hits_follow_native_scroll_with_an_offscreen_caret(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = cx.open_window(size(px(320.0), px(80.0)), |window, cx| {
        TooltipScrollHarness {
            origin_shift: px(0.0),
            renders: 0,
            input: cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(
                    format!("prefix-{{{{host}}}}/{{{{host}}}}{}", "x".repeat(200)),
                    window,
                    cx,
                );
                input.set_selected_range(0..0, cx);
                input
            }),
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let assert_native_hits = |visual: &mut VisualTestContext| {
        let expected = window
            .update(visual, |view, _, cx| {
                let input = view.input.read(cx);
                variable_ranges(&input.value())
                    .iter()
                    .map(|reference| input.range_to_bounds(&reference.range).unwrap())
                    .collect::<Vec<_>>()
            })
            .unwrap();
        for (selector, expected) in ["variable-hover-trigger", "variable-hover-trigger-1"]
            .into_iter()
            .zip(expected)
        {
            let actual = visual.debug_bounds(selector).unwrap();
            // GPUI snaps div hitboxes to device pixels; native glyph bounds are fractional.
            for (edge, native) in [
                (actual.left(), expected.left()),
                (actual.top(), expected.top()),
                (actual.right(), expected.right()),
                (actual.bottom(), expected.bottom()),
            ] {
                assert!(
                    (edge - native).abs() <= px(0.5),
                    "{selector}: {actual:?} vs native {expected:?}"
                );
            }
        }
    };
    assert_native_hits(&mut visual);
    window
        .update(cx, |view, _, cx| {
            view.input.update(cx, |input, cx| {
                input.set_scroll_offset(point(px(-40.0), px(0.0)), cx)
            });
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    let native = window
        .update(cx, |view, _, cx| view.input.read(cx).scroll_offset().x)
        .unwrap();
    assert_eq!(native, px(-40.0));
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.input.read(cx).cursor(),
                0,
                "wheel scrolling may leave the caret offscreen"
            );
        })
        .unwrap();
    assert_native_hits(&mut visual);
    for replacement in [None, Some(format!("prefix-{{{{host}}}}{}", "x".repeat(20)))] {
        window
            .update(cx, |view, window, cx| {
                view.input.update(cx, |input, cx| {
                    if let Some(value) = replacement {
                        input.set_value(value, window, cx);
                    }
                    let end = input.value().len();
                    input.set_selected_range(end..end, cx);
                });
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert_native_hits(&mut visual);
    }
    let before = window.update(cx, |view, _, _| view.renders).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.origin_shift = px(8.0);
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    assert_native_hits(&mut visual);
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.renders,
                before + 1,
                "moving the row must not add a tooltip geometry redraw"
            );
        })
        .unwrap();
}
