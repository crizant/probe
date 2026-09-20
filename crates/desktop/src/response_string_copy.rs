//! Context menu support for copying scalar values from response viewers.
//!
//! Both formats use cursor-local lexical detection so partially valid responses work
//! without parsing the full document on the UI thread.

use std::ops::Range;

/// Maximum characters to show in the copy menu preview label.
const PREVIEW_MAX_CHARS: usize = 24;
const EAGER_PREVIEW_MAX_BYTES: usize = 4 * 1024;

/// Extract and unescape a JSON string using serde_json for proper UTF-16 surrogate handling.
/// The range should include the surrounding quotes.
/// Returns the unescaped string content without quotes, or None if extraction fails.
pub(crate) fn extract_json_string(text: &str, range: Range<usize>) -> Option<String> {
    let json_literal = text.get(range)?;

    // Use serde_json to parse the string literal properly (handles UTF-16 surrogates)
    serde_json::from_str::<String>(json_literal).ok()
}

/// Generate a preview label for a quoted value.
/// Format: `Copy value "preview…"` for long strings.
/// Escapes any double quotes in the preview to avoid breaking menu layout.
/// Sanitizes control characters for safe display.
fn quoted_copy_menu_label(prefix: &str, unescaped: &str) -> String {
    let mut sanitized = unescaped.chars().map(sanitize_preview_char);
    let truncated: String = sanitized.by_ref().take(PREVIEW_MAX_CHARS).collect();
    let preview = if sanitized.next().is_some() {
        format!("{}…", escape_preview_quotes(&truncated))
    } else {
        escape_preview_quotes(&truncated)
    };

    if preview.is_empty() {
        format!("{prefix} \"\"")
    } else {
        format!("{prefix} \"{preview}\"")
    }
}

fn sanitize_preview_char(ch: char) -> char {
    if ch.is_control() && ch != '\t' {
        '�'
    } else {
        ch
    }
}

/// Escape double quotes in the preview text for safe menu label display.
fn escape_preview_quotes(text: &str) -> String {
    text.replace('"', "\\\"")
}

#[derive(Clone)]
pub(crate) struct ValueAtCursor {
    pub label: String,
    source: ValueSource,
}

#[derive(Clone)]
enum ValueSource {
    JsonString(Range<usize>),
    Raw(Range<usize>),
    XmlEscaped(Range<usize>),
}

impl ValueAtCursor {
    pub fn extract(text: &str, language: &str, offset: usize) -> Option<Self> {
        match language {
            "json" => extract_json_value(text, offset),
            "xml" => extract_xml_value(text, offset),
            _ => None,
        }
    }

    pub fn clipboard_value(&self, text: &str) -> Option<String> {
        match &self.source {
            ValueSource::JsonString(range) => extract_json_string(text, range.clone()),
            ValueSource::Raw(range) => text.get(range.clone()).map(str::to_owned),
            ValueSource::XmlEscaped(range) => {
                let raw = text.get(range.clone())?;
                Some(
                    quick_xml::escape::unescape(raw)
                        .map(|value| value.into_owned())
                        .unwrap_or_else(|_| raw.to_owned()),
                )
            }
        }
    }
}

fn extract_json_value(text: &str, offset: usize) -> Option<ValueAtCursor> {
    if offset >= text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    if let Some(value) = extract_json_quoted_value(text, offset) {
        return Some(value);
    }

    let bytes = text.as_bytes();
    let token_byte = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.');
    if !token_byte(bytes[offset]) {
        return None;
    }
    let start = bytes[..offset]
        .iter()
        .rposition(|byte| !token_byte(*byte))
        .map_or(0, |index| index + 1);
    let end = bytes[offset..]
        .iter()
        .position(|byte| !token_byte(*byte))
        .map_or(text.len(), |index| offset + index);
    if !valid_json_scalar_boundaries(bytes, start, end) {
        return None;
    }
    let value = text.get(start..end)?;
    if !matches!(value, "true" | "false" | "null") && !is_json_number(value) {
        return None;
    }
    let label = scalar_copy_menu_label(value);
    Some(ValueAtCursor {
        label,
        source: ValueSource::Raw(start..end),
    })
}

fn extract_json_quoted_value(text: &str, offset: usize) -> Option<ValueAtCursor> {
    let bytes = text.as_bytes();
    let at_quote = bytes[offset] == b'"' && is_unescaped_quote(bytes, offset);
    let before_or_at = (0..=offset)
        .rev()
        .find(|index| bytes[*index] == b'"' && is_unescaped_quote(bytes, *index));

    let mut candidates = Vec::with_capacity(2);
    if at_quote
        && let Some(start) = (0..offset)
            .rev()
            .find(|index| bytes[*index] == b'"' && is_unescaped_quote(bytes, *index))
    {
        candidates.push((start, offset));
    }
    if let Some(start) = before_or_at
        && let Some(end) = (start + 1..text.len())
            .find(|index| bytes[*index] == b'"' && is_unescaped_quote(bytes, *index))
    {
        candidates.push((start, end));
    }

    candidates.into_iter().find_map(|(start, end)| {
        if offset < start || offset > end || !valid_json_string_prefix(bytes, start) {
            return None;
        }
        let next = bytes[end + 1..]
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace());
        let prefix = if next == Some(b':') {
            "Copy key"
        } else if next.is_none() || next.is_some_and(|byte| matches!(byte, b',' | b'}' | b']')) {
            "Copy value"
        } else {
            return None;
        };
        let range = start..end + 1;
        let literal = text.get(range.clone())?;
        if !is_valid_json_string_literal(literal) {
            return None;
        }
        let label = if literal.len() <= EAGER_PREVIEW_MAX_BYTES {
            quoted_copy_menu_label(prefix, &extract_json_string(text, range.clone())?)
        } else if literal.as_bytes()[1..literal.len() - 1].contains(&b'\\') {
            prefix.to_owned()
        } else {
            quoted_copy_menu_label(prefix, literal.get(1..literal.len() - 1)?)
        };
        Some(ValueAtCursor {
            label,
            source: ValueSource::JsonString(range),
        })
    })
}

fn is_valid_json_string_literal(literal: &str) -> bool {
    let bytes = literal.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return false;
    }
    let mut index = 1;
    while index + 1 < bytes.len() {
        match bytes[index] {
            0x00..=0x1f => return false,
            b'\\' => {
                index += 1;
                match bytes.get(index) {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => index += 1,
                    Some(b'u') => {
                        let Some(code) = json_hex_escape(bytes, index + 1) else {
                            return false;
                        };
                        index += 5;
                        if (0xd800..=0xdbff).contains(&code) {
                            if bytes.get(index..index + 2) != Some(br"\u") {
                                return false;
                            }
                            let Some(low) = json_hex_escape(bytes, index + 2) else {
                                return false;
                            };
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return false;
                            }
                            index += 6;
                        } else if (0xdc00..=0xdfff).contains(&code) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            _ => index += 1,
        }
    }
    index + 1 == bytes.len()
}

fn json_hex_escape(bytes: &[u8], start: usize) -> Option<u16> {
    let digits = bytes.get(start..start + 4)?;
    digits.iter().try_fold(0_u16, |value, digit| {
        Some(value * 16 + u16::from((*digit as char).to_digit(16)? as u8))
    })
}

fn is_unescaped_quote(bytes: &[u8], quote: usize) -> bool {
    let slashes = bytes[..quote]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count();
    slashes % 2 == 0
}

fn valid_json_string_prefix(bytes: &[u8], start: usize) -> bool {
    bytes[..start]
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_none_or(|byte| matches!(byte, b'{' | b'[' | b',' | b':'))
}

fn valid_json_scalar_boundaries(bytes: &[u8], start: usize, end: usize) -> bool {
    let before = bytes[..start]
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace());
    let after = bytes[end..]
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace());
    before.is_none_or(|byte| matches!(byte, b'[' | b',' | b':'))
        && after.is_none_or(|byte| matches!(byte, b',' | b']' | b'}'))
}

fn is_json_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len()
}

fn scalar_copy_menu_label(value: &str) -> String {
    if value.chars().count() > PREVIEW_MAX_CHARS {
        format!(
            "Copy value {}…",
            value.chars().take(PREVIEW_MAX_CHARS).collect::<String>()
        )
    } else {
        format!("Copy value {value}")
    }
}

fn extract_xml_value(text: &str, offset: usize) -> Option<ValueAtCursor> {
    if offset >= text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let bytes = text.as_bytes();
    match xml_cursor_state(text, offset) {
        XmlCursorState::Text { value_start } => extract_xml_text_value(text, value_start, offset),
        XmlCursorState::Tag {
            tag_start,
            quote,
            copy_attributes,
        } => {
            if !copy_attributes {
                return None;
            }
            if let Some((delimiter, value_start)) = quote {
                let value_end = if bytes[offset] == delimiter {
                    offset
                } else {
                    bytes[offset..]
                        .iter()
                        .position(|byte| *byte == delimiter)
                        .map(|end| offset + end)?
                };
                return xml_value(text, value_start..value_end, true);
            }
            let delimiter = bytes[offset];
            if !matches!(delimiter, b'\'' | b'"') {
                return None;
            }
            let before_quote = bytes[tag_start + 1..offset]
                .iter()
                .rev()
                .copied()
                .find(|byte| !byte.is_ascii_whitespace());
            if before_quote != Some(b'=') {
                return None;
            }
            let value_end = bytes[offset + 1..]
                .iter()
                .position(|byte| *byte == delimiter)
                .map(|end| offset + 1 + end)?;
            xml_value(text, offset + 1..value_end, true)
        }
        XmlCursorState::Cdata { content_start } => {
            if offset < content_start {
                return None;
            }
            let content_end = text[content_start..]
                .find("]]>")
                .map(|end| content_start + end)?;
            if offset >= content_end {
                return None;
            }
            xml_value(text, content_start..content_end, false)
        }
        XmlCursorState::Comment
        | XmlCursorState::ProcessingInstruction
        | XmlCursorState::DeclarationComment { .. }
        | XmlCursorState::DeclarationProcessingInstruction { .. }
        | XmlCursorState::Declaration { .. } => None,
    }
}

fn extract_xml_text_value(text: &str, value_start: usize, offset: usize) -> Option<ValueAtCursor> {
    let bytes = text.as_bytes();
    let value_end = bytes[offset..]
        .iter()
        .position(|byte| *byte == b'<')
        .map_or(text.len(), |end| offset + end);
    if !(value_start..value_end).contains(&offset) {
        return None;
    }
    let raw = text.get(value_start..value_end)?;
    if raw
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
    {
        return None;
    }
    xml_value(text, value_start..value_end, true)
}

#[derive(Clone, Copy)]
enum XmlCursorState {
    Text {
        value_start: usize,
    },
    Tag {
        tag_start: usize,
        quote: Option<(u8, usize)>,
        copy_attributes: bool,
    },
    Comment,
    Cdata {
        content_start: usize,
    },
    ProcessingInstruction,
    Declaration {
        quote: Option<u8>,
        subset_depth: usize,
    },
    DeclarationComment {
        subset_depth: usize,
    },
    DeclarationProcessingInstruction {
        subset_depth: usize,
    },
}

fn xml_cursor_state(text: &str, offset: usize) -> XmlCursorState {
    let bytes = text.as_bytes();
    let mut state = XmlCursorState::Text { value_start: 0 };
    let mut index = 0;
    while index < offset {
        match state {
            XmlCursorState::Text { .. } => {
                if bytes[index..].starts_with(b"<!--") {
                    state = XmlCursorState::Comment;
                    index += 4;
                } else if bytes[index..].starts_with(b"<![CDATA[") {
                    let content_start = index + 9;
                    state = XmlCursorState::Cdata { content_start };
                    index = content_start;
                } else if bytes[index..].starts_with(b"<?") {
                    state = XmlCursorState::ProcessingInstruction;
                    index += 2;
                } else if bytes[index..].starts_with(b"<!") {
                    state = XmlCursorState::Declaration {
                        quote: None,
                        subset_depth: 0,
                    };
                    index += 2;
                } else if bytes[index] == b'<' {
                    state = XmlCursorState::Tag {
                        tag_start: index,
                        quote: None,
                        copy_attributes: !bytes[index..].starts_with(b"</"),
                    };
                    index += 1;
                } else {
                    index += 1;
                }
            }
            XmlCursorState::Tag {
                tag_start,
                quote,
                copy_attributes,
            } => match (quote, bytes[index]) {
                (None, b'\'' | b'"') => {
                    state = XmlCursorState::Tag {
                        tag_start,
                        quote: Some((bytes[index], index + 1)),
                        copy_attributes,
                    };
                    index += 1;
                }
                (Some((open, _)), close) if open == close => {
                    state = XmlCursorState::Tag {
                        tag_start,
                        quote: None,
                        copy_attributes,
                    };
                    index += 1;
                }
                (None, b'>') => {
                    state = XmlCursorState::Text {
                        value_start: index + 1,
                    };
                    index += 1;
                }
                _ => index += 1,
            },
            XmlCursorState::Comment => {
                if bytes[index..].starts_with(b"-->") {
                    state = XmlCursorState::Text {
                        value_start: index + 3,
                    };
                    index += 3;
                } else {
                    index += 1;
                }
            }
            XmlCursorState::Cdata { .. } => {
                if bytes[index..].starts_with(b"]]>") {
                    state = XmlCursorState::Text {
                        value_start: index + 3,
                    };
                    index += 3;
                } else {
                    index += 1;
                }
            }
            XmlCursorState::ProcessingInstruction => {
                if bytes[index..].starts_with(b"?>") {
                    state = XmlCursorState::Text {
                        value_start: index + 2,
                    };
                    index += 2;
                } else {
                    index += 1;
                }
            }
            XmlCursorState::Declaration {
                mut quote,
                mut subset_depth,
            } => {
                if quote.is_none() && bytes[index..].starts_with(b"<!--") {
                    state = XmlCursorState::DeclarationComment { subset_depth };
                    index += 4;
                    continue;
                }
                if quote.is_none() && bytes[index..].starts_with(b"<?") {
                    state = XmlCursorState::DeclarationProcessingInstruction { subset_depth };
                    index += 2;
                    continue;
                }
                match (quote, bytes[index]) {
                    (None, b'\'' | b'"') => quote = Some(bytes[index]),
                    (Some(open), close) if open == close => quote = None,
                    (None, b'[') => subset_depth += 1,
                    (None, b']') => subset_depth = subset_depth.saturating_sub(1),
                    (None, b'>') if subset_depth == 0 => {
                        state = XmlCursorState::Text {
                            value_start: index + 1,
                        };
                        index += 1;
                        continue;
                    }
                    _ => {}
                }
                state = XmlCursorState::Declaration {
                    quote,
                    subset_depth,
                };
                index += 1;
            }
            XmlCursorState::DeclarationComment { subset_depth } => {
                if bytes[index..].starts_with(b"-->") {
                    state = XmlCursorState::Declaration {
                        quote: None,
                        subset_depth,
                    };
                    index += 3;
                } else {
                    index += 1;
                }
            }
            XmlCursorState::DeclarationProcessingInstruction { subset_depth } => {
                if bytes[index..].starts_with(b"?>") {
                    state = XmlCursorState::Declaration {
                        quote: None,
                        subset_depth,
                    };
                    index += 2;
                } else {
                    index += 1;
                }
            }
        }
    }
    state
}

fn xml_value(text: &str, range: Range<usize>, decode_entities: bool) -> Option<ValueAtCursor> {
    let raw = text.get(range.clone())?;
    let label = if decode_entities && raw.len() > EAGER_PREVIEW_MAX_BYTES && raw.contains('&') {
        "Copy value".to_owned()
    } else if decode_entities && raw.len() <= EAGER_PREVIEW_MAX_BYTES {
        let preview = quick_xml::escape::unescape(raw).unwrap_or(std::borrow::Cow::Borrowed(raw));
        quoted_copy_menu_label("Copy value", preview.as_ref())
    } else {
        quoted_copy_menu_label("Copy value", raw)
    };
    let source = if decode_entities {
        ValueSource::XmlEscaped(range)
    } else {
        ValueSource::Raw(range)
    };
    Some(ValueAtCursor { label, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copied(value: &ValueAtCursor, text: &str) -> String {
        value.clipboard_value(text).unwrap()
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
    fn quoted_copy_menu_label_formats_short_strings() {
        assert_eq!(
            quoted_copy_menu_label("Copy value", "hello"),
            r#"Copy value "hello""#
        );
        assert_eq!(quoted_copy_menu_label("Copy value", ""), r#"Copy value """#);
    }

    #[test]
    fn quoted_copy_menu_label_truncates_long_strings() {
        let long = "a".repeat(50);
        let label = quoted_copy_menu_label("Copy value", &long);
        assert!(label.starts_with(r#"Copy value "a"#));
        assert!(label.ends_with("…\""));
        assert!(label.len() < 50);
    }

    #[test]
    fn quoted_copy_menu_label_escapes_embedded_quotes() {
        assert_eq!(
            quoted_copy_menu_label("Copy value", "say \"hi\""),
            r#"Copy value "say \"hi\"""#
        );
    }

    #[test]
    fn quoted_copy_menu_label_sanitizes_control_chars() {
        // Newline should be replaced with replacement character
        assert_eq!(
            quoted_copy_menu_label("Copy value", "line\nbreak"),
            r#"Copy value "line�break""#
        );
        // Tab should appear literally in the test output
        let result = quoted_copy_menu_label("Copy value", "tab\there");
        // Check that it contains tab character (not escaped)
        assert!(result.contains('\t'), "Expected tab character in: {result}");
    }

    #[test]
    fn json_value_at_cursor_extracts_strings_keys_and_other_scalars() {
        // Test case 1: Cursor on string content
        let text = r#"{"message": "Hello, World!"}"#;
        let result = ValueAtCursor::extract(text, "json", 15); // cursor on 'Hello'
        assert!(result.is_some());
        let extracted = result.unwrap();
        assert_eq!(copied(&extracted, text), "Hello, World!");
        assert_eq!(extracted.label, r#"Copy value "Hello, World!""#);

        // Test case 2: Cursor on escape sequence
        let text = r#"{"text": "line\nbreak"}"#;
        let result = ValueAtCursor::extract(text, "json", 14); // cursor on \n
        assert!(result.is_some());
        let extracted = result.unwrap();
        assert_eq!(copied(&extracted, text), "line\nbreak");
        assert_eq!(extracted.label, r#"Copy value "line�break""#); // \n sanitized

        // Test case 3: Cursor on quote (should work if quote is part of string span)
        let text = r#"{"key": "value"}"#;
        // Try cursor near the quotes
        let result = ValueAtCursor::extract(text, "json", 9); // first char of "value"
        assert!(result.is_some());

        // Test case 4: Object keys are decoded and identified as keys.
        let result = ValueAtCursor::extract(text, "json", 2).unwrap();
        assert_eq!(copied(&result, text), "key");
        assert_eq!(result.label, r#"Copy key "key""#);

        let text = r#"{"first\u0020name": 1}"#;
        let offset = text.find("u0020").unwrap();
        let result = ValueAtCursor::extract(text, "json", offset).unwrap();
        assert_eq!(copied(&result, text), "first name");
        assert_eq!(result.label, r#"Copy key "first name""#);

        let text = r#"{"n": -12.5e2, "ok": true, "missing": null}"#;
        for (needle, expected) in [("12.5", "-12.5e2"), ("true", "true"), ("null", "null")] {
            let offset = text.find(needle).unwrap();
            let result = ValueAtCursor::extract(text, "json", offset).unwrap();
            assert_eq!(copied(&result, text), expected);
            assert_eq!(result.label, format!("Copy value {expected}"));
        }
    }

    #[test]
    fn string_at_cursor_handles_emoji_surrogates() {
        let text = r#"{"emoji": "\uD83D\uDE00"}"#;
        let result = ValueAtCursor::extract(text, "json", 15); // cursor in escape
        assert!(result.is_some());
        let extracted = result.unwrap();
        assert_eq!(copied(&extracted, text), "😀");
    }

    #[test]
    fn json_value_at_cursor_supports_empty_strings_and_bounds_number_labels() {
        let text = r#"{"": ""}"#;
        let key = ValueAtCursor::extract(text, "json", 1).unwrap();
        assert_eq!(copied(&key, text), "");
        assert_eq!(key.label, r#"Copy key """#);
        let value = ValueAtCursor::extract(text, "json", 5).unwrap();
        assert_eq!(copied(&value, text), "");
        assert_eq!(value.label, r#"Copy value """#);
        let unterminated = r#"{"value": "true"#;
        assert!(
            ValueAtCursor::extract(unterminated, "json", unterminated.find("true").unwrap())
                .is_none()
        );

        let number = "1".repeat(10_000);
        let text = format!(r#"{{"n": {number}}}"#);
        let offset = text.find(&number).unwrap();
        let value = ValueAtCursor::extract(&text, "json", offset).unwrap();
        assert_eq!(copied(&value, &text), number);
        assert!(value.label.ends_with('…'));
        assert!(value.label.len() < 64);

        let string = "x".repeat(10_000);
        let text = format!(r#"{{"value":"{string}"}}"#);
        let offset = text.find(&string).unwrap() + string.len() / 2;
        let value = ValueAtCursor::extract(&text, "json", offset).unwrap();
        assert!(value.label.len() < 64);
        assert_eq!(copied(&value, &text), string);

        let escaped = format!(r#"\u0041{}"#, "x".repeat(5_000));
        let text = format!(r#"{{"value":"{escaped}"}}"#);
        let offset = text.find('x').unwrap();
        let value = ValueAtCursor::extract(&text, "json", offset).unwrap();
        assert_eq!(value.label, "Copy value");
        assert_eq!(copied(&value, &text), format!("A{}", "x".repeat(5_000)));

        let invalid = format!(r#"{{"value":"{}\q"}}"#, "x".repeat(5_000));
        let offset = invalid.find('x').unwrap();
        assert!(ValueAtCursor::extract(&invalid, "json", offset).is_none());
    }

    #[test]
    fn xml_value_at_cursor_extracts_attributes_text_and_cdata() {
        let text =
            r#"<root id="a&amp;b"><name>Ada &amp; Co</name><raw><![CDATA[x < y]]></raw></root>"#;
        let attribute_offset = text.find("a&amp;b").unwrap();
        let attribute = ValueAtCursor::extract(text, "xml", attribute_offset).unwrap();
        assert_eq!(copied(&attribute, text), "a&b");
        assert_eq!(attribute.label, r#"Copy value "a&b""#);

        let text_offset = text.find("Ada").unwrap();
        let element_text = ValueAtCursor::extract(text, "xml", text_offset).unwrap();
        assert_eq!(copied(&element_text, text), "Ada & Co");

        let cdata_offset = text.find("x < y").unwrap();
        let cdata = ValueAtCursor::extract(text, "xml", cdata_offset).unwrap();
        assert_eq!(copied(&cdata, text), "x < y");

        let cdata_entities = "<root><![CDATA[a&amp;b]]></root>";
        let offset = cdata_entities.find("a&amp;b").unwrap();
        let cdata = ValueAtCursor::extract(cdata_entities, "xml", offset).unwrap();
        assert_eq!(copied(&cdata, cdata_entities), "a&amp;b");

        for literal in ["a <!-- b", "a <? b", "a <![CDATA[ b"] {
            let document = format!("<root><![CDATA[{literal}]]></root>");
            let offset = document.find('b').unwrap();
            let cdata = ValueAtCursor::extract(&document, "xml", offset).unwrap();
            assert_eq!(copied(&cdata, &document), literal);
        }

        let greater_than = r#"<root label="x>y">a > b</root>"#;
        let attribute_offset = greater_than.find("y\"").unwrap();
        let attribute = ValueAtCursor::extract(greater_than, "xml", attribute_offset).unwrap();
        assert_eq!(copied(&attribute, greater_than), "x>y");
        let text_offset = greater_than.find("b</").unwrap();
        let element_text = ValueAtCursor::extract(greater_than, "xml", text_offset).unwrap();
        assert_eq!(copied(&element_text, greater_than), "a > b");

        for value in ["alpha --> beta", "alpha ?> beta"] {
            let text_document = format!("<root>{value}</root>");
            let offset = text_document.find("beta").unwrap();
            let extracted = ValueAtCursor::extract(&text_document, "xml", offset).unwrap();
            assert_eq!(copied(&extracted, &text_document), value);
            let attribute_document = format!(r#"<root label="{value}"/>"#);
            let offset = attribute_document.find("beta").unwrap();
            let extracted = ValueAtCursor::extract(&attribute_document, "xml", offset).unwrap();
            assert_eq!(copied(&extracted, &attribute_document), value);
        }

        let preserved = "<root xml:space=\"preserve\">  padded  </root>";
        let offset = preserved.find("padded").unwrap();
        let extracted = ValueAtCursor::extract(preserved, "xml", offset).unwrap();
        assert_eq!(copied(&extracted, preserved), "  padded  ");
        let non_xml_whitespace = "<root>\u{a0}padded\u{a0}</root>";
        let offset = non_xml_whitespace.find("padded").unwrap();
        let extracted = ValueAtCursor::extract(non_xml_whitespace, "xml", offset).unwrap();
        assert_eq!(copied(&extracted, non_xml_whitespace), "\u{a0}padded\u{a0}");
        let indentation = "<root>\n  <child/>\n</root>";
        let offset = indentation.find("  ").unwrap();
        assert!(ValueAtCursor::extract(indentation, "xml", offset).is_none());
        let closing_tag = "<root>value</root>";
        let offset = closing_tag.find("</root>").unwrap();
        assert!(ValueAtCursor::extract(closing_tag, "xml", offset).is_none());

        let large_text = "x".repeat(10_000);
        let document = format!("<root>{large_text}</root>");
        let offset = document.find(&large_text).unwrap() + large_text.len() / 2;
        let value = ValueAtCursor::extract(&document, "xml", offset).unwrap();
        assert!(value.label.len() < 64);
        assert_eq!(copied(&value, &document), large_text);

        let large_escaped = format!("&amp;{}", "x".repeat(5_000));
        let document = format!("<root>{large_escaped}</root>");
        let offset = document.find('x').unwrap();
        let value = ValueAtCursor::extract(&document, "xml", offset).unwrap();
        assert_eq!(value.label, "Copy value");
        assert_eq!(copied(&value, &document), format!("&{}", "x".repeat(5_000)));

        let empty_attribute = r#"<root label=""/>"#;
        let offset = empty_attribute.find("\"\"").unwrap();
        let attribute = ValueAtCursor::extract(empty_attribute, "xml", offset).unwrap();
        assert_eq!(copied(&attribute, empty_attribute), "");
        assert_eq!(attribute.label, r#"Copy value """#);
        let closing_quote = offset + 1;
        let attribute = ValueAtCursor::extract(empty_attribute, "xml", closing_quote).unwrap();
        assert_eq!(copied(&attribute, empty_attribute), "");

        // Cursor-local extraction also works before the rest of a document has arrived.
        let partial = "<root label='café'>  déjà";
        let attribute_offset = partial.find("café").unwrap();
        let attribute = ValueAtCursor::extract(partial, "xml", attribute_offset).unwrap();
        assert_eq!(copied(&attribute, partial), "café");
        let text_offset = partial.find("déjà").unwrap();
        let element_text = ValueAtCursor::extract(partial, "xml", text_offset).unwrap();
        assert_eq!(copied(&element_text, partial), "  déjà");
    }

    #[test]
    fn value_at_cursor_rejects_structure_comments_and_unknown_languages() {
        let json = r#"{"key": 1}"#;
        assert!(ValueAtCursor::extract(json, "json", 0).is_none());

        let xml = "<root><!-- note --><child/></root>";
        assert!(ValueAtCursor::extract(xml, "xml", 1).is_none());
        assert!(ValueAtCursor::extract(xml, "xml", xml.find("note").unwrap()).is_none());
        let comment = "<root><!-- a > secret --></root>";
        assert!(ValueAtCursor::extract(comment, "xml", comment.find("secret").unwrap()).is_none());
        let doctype =
            "<!DOCTYPE root [<!-- ] > secret --><!ELEMENT root (#PCDATA)>]><root>x</root>";
        assert!(ValueAtCursor::extract(doctype, "xml", doctype.find("secret").unwrap()).is_none());
        let value = ValueAtCursor::extract(doctype, "xml", doctype.rfind('x').unwrap()).unwrap();
        assert_eq!(copied(&value, doctype), "x");
        assert!(ValueAtCursor::extract(json, "plain", 2).is_none());
    }
}
