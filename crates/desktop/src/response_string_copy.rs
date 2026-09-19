//! Context menu support for copying JSON string values from response viewers.
//!
//! This module provides token detection and string extraction for the "Copy string"
//! context menu action in Pretty and JSON response views.

use std::ops::Range;

/// Maximum characters to show in the "Copy string" menu preview label.
const PREVIEW_MAX_CHARS: usize = 24;

/// Detect if the cursor/offset is within a JSON string token in the highlighted text.
/// Expands across contiguous `string` and `string.escape` roles to find the complete string.
/// Returns the byte range of the complete string literal (between quotes) if found.
pub(crate) fn find_string_token_at_offset(
    text: &str,
    highlights: &[(Range<usize>, &'static str)],
    offset: usize,
) -> Option<Range<usize>> {
    // Find if cursor is in a string or string.escape token
    let cursor_span = highlights.iter().find(|(range, role)| {
        (*role == "string" || *role == "string.escape") && range.contains(&offset)
    })?;

    // Expand to cover all contiguous string/string.escape spans
    let mut start = cursor_span.0.start;
    let mut end = cursor_span.0.end;

    // Expand backwards
    for (range, role) in highlights.iter().rev() {
        if range.end == start && (*role == "string" || *role == "string.escape") {
            start = range.start;
        } else if range.end < start {
            break;
        }
    }

    // Expand forwards
    for (range, role) in highlights {
        if range.start == end && (*role == "string" || *role == "string.escape") {
            end = range.end;
        } else if range.start > end {
            break;
        }
    }

    // Now we have the contiguous string content range (may or may not include quotes)
    // Check for surrounding quotes and expand to include them
    let bytes = text.as_bytes();
    if start > 0
        && end < text.len()
        && bytes.get(start.saturating_sub(1)) == Some(&b'"')
        && bytes.get(end) == Some(&b'"')
    {
        // Quotes surround the content, include them
        return Some(start.saturating_sub(1)..end + 1);
    }

    // Check if the range already includes quotes
    if let Some(token) = text.get(start..end)
        && token.starts_with('"')
        && token.ends_with('"')
        && token.len() >= 2
    {
        return Some(start..end);
    }

    None
}

/// Extract and unescape a JSON string using serde_json for proper UTF-16 surrogate handling.
/// The range should include the surrounding quotes.
/// Returns the unescaped string content without quotes, or None if extraction fails.
pub(crate) fn extract_json_string(text: &str, range: Range<usize>) -> Option<String> {
    let json_literal = text.get(range)?;

    // Use serde_json to parse the string literal properly (handles UTF-16 surrogates)
    serde_json::from_str::<String>(json_literal).ok()
}

/// Generate a preview label for the "Copy string" menu item.
/// Format: `Copy "preview…"` for long strings, `Copy "short"` for short strings.
/// Escapes any double quotes in the preview to avoid breaking menu layout.
/// Sanitizes control characters for safe display.
pub(crate) fn string_copy_menu_label(unescaped: &str) -> String {
    let sanitized = sanitize_preview(unescaped);
    let preview = if sanitized.chars().count() > PREVIEW_MAX_CHARS {
        let truncated: String = sanitized.chars().take(PREVIEW_MAX_CHARS).collect();
        format!("{}…", escape_preview_quotes(&truncated))
    } else {
        escape_preview_quotes(&sanitized)
    };

    if preview.is_empty() {
        "Copy string".to_owned()
    } else {
        format!("Copy \"{}\"", preview)
    }
}

/// Sanitize control characters in preview text for safe menu display.
fn sanitize_preview(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_control() && ch != '\t' {
                '�' // Unicode replacement character
            } else {
                ch
            }
        })
        .collect()
}

/// Escape double quotes in the preview text for safe menu label display.
fn escape_preview_quotes(text: &str) -> String {
    text.replace('"', "\\\"")
}

/// Extract string at cursor position with caching to avoid triple parsing.
/// Returns both the extracted string and a suitable menu label.
pub(crate) struct StringAtCursor {
    pub unescaped: String,
    pub label: String,
}

impl StringAtCursor {
    pub fn extract(
        text: &str,
        highlights: &[(Range<usize>, &'static str)],
        offset: usize,
    ) -> Option<Self> {
        let range = find_string_token_at_offset(text, highlights, offset)?;
        let unescaped = extract_json_string(text, range)?;
        let label = string_copy_menu_label(&unescaped);
        Some(Self { unescaped, label })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_string_token_expands_contiguous_spans() {
        // Simulate string with escape sequence split across spans
        let text = r#"{"key": "hello\nworld"}"#;
        let highlights = vec![
            (0..1, "punctuation"),
            (1..6, "property"), // "key"
            (6..7, "punctuation"),
            (8..9, "punctuation"),     // opening "
            (9..14, "string"),         // hello
            (14..16, "string.escape"), // \n
            (16..21, "string"),        // world
            (21..22, "punctuation"),   // closing "
        ];

        // Cursor anywhere in the string should find the complete range including quotes
        let result = find_string_token_at_offset(text, &highlights, 10);
        assert_eq!(result, Some(8..22)); // Should include quotes: "hello\nworld"

        // Cursor on escape sequence
        let result = find_string_token_at_offset(text, &highlights, 15);
        assert_eq!(result, Some(8..22));
    }

    #[test]
    fn find_string_token_returns_none_outside_string() {
        let text = r#"{"key": "value"}"#;
        let highlights = vec![
            (0..1, "punctuation"),
            (9..14, "string"),
            (15..16, "punctuation"),
        ];
        assert_eq!(find_string_token_at_offset(text, &highlights, 0), None);
        assert_eq!(find_string_token_at_offset(text, &highlights, 8), None);
        assert_eq!(find_string_token_at_offset(text, &highlights, 100), None);
    }

    #[test]
    fn extract_json_string_uses_serde_for_unescaping() {
        // Basic string
        let text = r#""hello""#;
        assert_eq!(extract_json_string(text, 0..7), Some("hello".to_owned()));

        // Standard escapes
        let text = r#""line\nbreak""#;
        assert_eq!(
            extract_json_string(text, 0..13),
            Some("line\nbreak".to_owned())
        );

        let text = r#""say \"hi\"""#;
        assert_eq!(
            extract_json_string(text, 0..12),
            Some("say \"hi\"".to_owned())
        );
    }

    #[test]
    fn extract_json_string_handles_utf16_surrogates() {
        // Emoji using UTF-16 surrogate pair: 😀 = U+1F600 = \uD83D\uDE00
        let text = r#""emoji: \uD83D\uDE00""#;
        let result = extract_json_string(text, 0..text.len());
        assert_eq!(result, Some("emoji: 😀".to_owned()));

        // Another emoji: 🎉 = U+1F389 = \uD83C\uDF89
        let text = r#""party: \uD83C\uDF89""#;
        let result = extract_json_string(text, 0..text.len());
        assert_eq!(result, Some("party: 🎉".to_owned()));

        // Multiple emojis
        let text = r#""\uD83D\uDE00\uD83D\uDE01""#;
        let result = extract_json_string(text, 0..text.len());
        assert_eq!(result, Some("😀😁".to_owned()));
    }

    #[test]
    fn extract_json_string_handles_long_strings() {
        // Test a long string (>100 chars)
        let long_value = "a".repeat(150);
        let json = format!(r#""{}""#, long_value);
        let result = extract_json_string(&json, 0..json.len());
        assert_eq!(result, Some(long_value));
    }

    #[test]
    fn extract_json_string_returns_none_for_invalid_json() {
        // Missing closing quote
        let text = r#""hello"#;
        assert_eq!(extract_json_string(text, 0..6), None);

        // Not a string literal
        let text = "hello";
        assert_eq!(extract_json_string(text, 0..5), None);
    }

    #[test]
    fn string_copy_menu_label_formats_short_strings() {
        assert_eq!(string_copy_menu_label("hello"), r#"Copy "hello""#);
        assert_eq!(string_copy_menu_label(""), "Copy string");
    }

    #[test]
    fn string_copy_menu_label_truncates_long_strings() {
        let long = "a".repeat(50);
        let label = string_copy_menu_label(&long);
        assert!(label.starts_with(r#"Copy "a"#));
        assert!(label.ends_with("…\""));
        assert!(label.len() < 50);
    }

    #[test]
    fn string_copy_menu_label_escapes_embedded_quotes() {
        assert_eq!(string_copy_menu_label("say \"hi\""), r#"Copy "say \"hi\"""#);
    }

    #[test]
    fn string_copy_menu_label_sanitizes_control_chars() {
        // Newline should be replaced with replacement character
        assert_eq!(
            string_copy_menu_label("line\nbreak"),
            r#"Copy "line�break""#
        );
        // Tab should appear literally in the test output
        let result = string_copy_menu_label("tab\there");
        // Check that it contains tab character (not escaped)
        assert!(result.contains('\t'), "Expected tab character in: {result}");
    }

    #[test]
    fn string_at_cursor_end_to_end_extraction() {
        use crate::syntax::SyntectHighlighter;

        // Test case 1: Cursor on string content
        let text = r#"{"message": "Hello, World!"}"#;
        let highlights = SyntectHighlighter::parse_for_menu(text, "json");
        let result = StringAtCursor::extract(text, &highlights, 15); // cursor on 'Hello'
        assert!(result.is_some());
        let extracted = result.unwrap();
        assert_eq!(extracted.unescaped, "Hello, World!");
        assert_eq!(extracted.label, r#"Copy "Hello, World!""#);

        // Test case 2: Cursor on escape sequence
        let text = r#"{"text": "line\nbreak"}"#;
        let highlights = SyntectHighlighter::parse_for_menu(text, "json");
        let result = StringAtCursor::extract(text, &highlights, 14); // cursor on \n
        assert!(result.is_some());
        let extracted = result.unwrap();
        assert_eq!(extracted.unescaped, "line\nbreak");
        assert_eq!(extracted.label, r#"Copy "line�break""#); // \n sanitized

        // Test case 3: Cursor on quote (should work if quote is part of string span)
        let text = r#"{"key": "value"}"#;
        let highlights = SyntectHighlighter::parse_for_menu(text, "json");
        // Try cursor near the quotes
        let result = StringAtCursor::extract(text, &highlights, 9); // first char of "value"
        assert!(result.is_some());

        // Test case 4: Cursor outside string (on key)
        let result = StringAtCursor::extract(text, &highlights, 2); // cursor on 'k' in key
        assert!(result.is_none()); // keys are "property", not "string"
    }

    #[test]
    fn string_at_cursor_handles_emoji_surrogates() {
        use crate::syntax::SyntectHighlighter;

        let text = r#"{"emoji": "\uD83D\uDE00"}"#;
        let highlights = SyntectHighlighter::parse_for_menu(text, "json");
        let result = StringAtCursor::extract(text, &highlights, 15); // cursor in escape
        assert!(result.is_some());
        let extracted = result.unwrap();
        assert_eq!(extracted.unescaped, "😀");
    }
}
