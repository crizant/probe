//! Context menu support for copying JSON string values from response viewers.
//!
//! This module provides token detection and string extraction for the "Copy string"
//! context menu action in Pretty and JSON response views.

use std::ops::Range;

/// Maximum characters to show in the "Copy string" menu preview label.
#[cfg(test)]
const PREVIEW_MAX_CHARS: usize = 24;

/// Detect if the cursor/offset is within a JSON string token in the highlighted text.
/// Returns the byte range of the complete string token (including quotes) if found.
pub(crate) fn find_string_token_at_offset(
    highlights: &[(Range<usize>, &'static str)],
    offset: usize,
) -> Option<Range<usize>> {
    highlights
        .iter()
        .find(|(range, role)| *role == "string" && range.contains(&offset))
        .map(|(range, _)| range.clone())
}

/// Extract and unescape a JSON string from the source text given its byte range.
/// The range should include the surrounding quotes. Returns the unescaped string content
/// without the quotes, or None if the range doesn't contain a valid JSON string literal.
pub(crate) fn extract_json_string(text: &str, range: Range<usize>) -> Option<String> {
    let token = text.get(range)?;
    // JSON strings start and end with double quotes
    let quoted = token.strip_prefix('"')?.strip_suffix('"')?;
    Some(unescape_json_string(quoted))
}

/// Unescape a JSON string (without surrounding quotes).
fn unescape_json_string(escaped: &str) -> String {
    let mut result = String::with_capacity(escaped.len());
    let mut chars = escaped.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    '/' => result.push('/'),
                    'b' => result.push('\u{0008}'),
                    'f' => result.push('\u{000C}'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    'u' => {
                        // Unicode escape: \uXXXX
                        let hex: String = chars.by_ref().take(4).collect();
                        if hex.len() == 4 {
                            if let Ok(code) = u16::from_str_radix(&hex, 16) {
                                if let Some(unicode_char) = char::from_u32(u32::from(code)) {
                                    result.push(unicode_char);
                                } else {
                                    // Invalid unicode, keep the escape sequence
                                    result.push('\\');
                                    result.push('u');
                                    result.push_str(&hex);
                                }
                            } else {
                                // Invalid hex, keep the escape sequence
                                result.push('\\');
                                result.push('u');
                                result.push_str(&hex);
                            }
                        } else {
                            // Not enough characters for unicode escape
                            result.push('\\');
                            result.push('u');
                            result.push_str(&hex);
                        }
                    }
                    _ => {
                        // Unknown escape, keep both characters
                        result.push('\\');
                        result.push(next);
                    }
                }
            } else {
                result.push('\\');
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Generate a preview label for the "Copy string" menu item.
/// Format: `Copy "preview…"` for long strings, `Copy "short"` for short strings.
/// Escapes any double quotes in the preview to avoid breaking menu layout.
///
/// Note: Currently not used in production due to `TextContextMenuExtraAction` requiring
/// static label strings. This function is preserved for future enhancement and is tested.
#[cfg(test)]
pub(crate) fn string_copy_menu_label(unescaped: &str) -> String {
    let preview = if unescaped.chars().count() > PREVIEW_MAX_CHARS {
        let truncated: String = unescaped.chars().take(PREVIEW_MAX_CHARS).collect();
        format!("{}…", escape_preview_quotes(&truncated))
    } else {
        escape_preview_quotes(unescaped)
    };

    if preview.is_empty() {
        "Copy string".to_owned()
    } else {
        format!("Copy \"{}\"", preview)
    }
}

/// Escape double quotes in the preview text for safe menu label display.
#[cfg(test)]
fn escape_preview_quotes(text: &str) -> String {
    text.replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_string_token_detects_cursor_inside_string() {
        let highlights = vec![
            (0..1, "punctuation"),
            (1..8, "string"),
            (8..9, "punctuation"),
        ];
        assert_eq!(find_string_token_at_offset(&highlights, 3), Some(1..8));
        assert_eq!(find_string_token_at_offset(&highlights, 1), Some(1..8));
        assert_eq!(find_string_token_at_offset(&highlights, 7), Some(1..8));
    }

    #[test]
    fn find_string_token_returns_none_outside_string() {
        let highlights = vec![
            (0..1, "punctuation"),
            (1..8, "string"),
            (8..9, "punctuation"),
        ];
        assert_eq!(find_string_token_at_offset(&highlights, 0), None);
        assert_eq!(find_string_token_at_offset(&highlights, 8), None);
        assert_eq!(find_string_token_at_offset(&highlights, 100), None);
    }

    #[test]
    fn extract_json_string_removes_quotes_and_unescapes() {
        let text = r#""hello""#;
        assert_eq!(extract_json_string(text, 0..7), Some("hello".to_owned()));

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
    fn extract_json_string_handles_unicode_escapes() {
        let text = r#""emoji: \u263A""#;
        assert_eq!(
            extract_json_string(text, 0..15),
            Some("emoji: ☺".to_owned())
        );
    }

    #[test]
    fn extract_json_string_returns_none_for_invalid_range() {
        let text = r#""hello""#;
        assert_eq!(extract_json_string(text, 0..100), None);
        assert_eq!(extract_json_string(text, 5..7), None);
    }

    #[test]
    fn extract_json_string_returns_none_without_quotes() {
        let text = "hello";
        assert_eq!(extract_json_string(text, 0..5), None);
    }

    #[test]
    fn unescape_all_standard_json_escapes() {
        assert_eq!(unescape_json_string(r#"a\"b"#), "a\"b");
        assert_eq!(unescape_json_string(r#"a\\b"#), "a\\b");
        assert_eq!(unescape_json_string(r#"a\/b"#), "a/b");
        assert_eq!(unescape_json_string(r#"a\bb"#), "a\u{0008}b");
        assert_eq!(unescape_json_string(r#"a\fb"#), "a\u{000C}b");
        assert_eq!(unescape_json_string(r#"a\nb"#), "a\nb");
        assert_eq!(unescape_json_string(r#"a\rb"#), "a\rb");
        assert_eq!(unescape_json_string(r#"a\tb"#), "a\tb");
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
    fn escape_preview_quotes_escapes_double_quotes() {
        assert_eq!(escape_preview_quotes("say \"hello\""), r#"say \"hello\""#);
        assert_eq!(escape_preview_quotes("no quotes"), "no quotes");
    }
}
