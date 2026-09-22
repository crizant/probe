use std::ops::Range;

use gpui::{Context, EntityInputHandler as _, Window};
use gpui_base::input::EditorState;

/// A local edit decided from the text before and after a collapsed caret change.
/// Ranges and `cursor` are UTF-8 byte offsets into the post-change text.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum AutoEdit {
    /// Insert `closer` at `cursor` and leave the caret there, before the closer.
    InsertCloser { closer: char, cursor: usize },
    /// Replace `range` in the post-change text. The caret ends at the end of
    /// `replacement` (the start of the range when `replacement` is empty).
    Splice {
        range: Range<usize>,
        replacement: String,
    },
}

/// Decide a pair insertion, closer overtype, or empty-pair backspace.
///
/// `selection` is the collapsed caret after the user's edit, in UTF-8 bytes.
/// Only a single inserted or deleted character is considered. Quote parity
/// counts unescaped quotes of the same kind on the current line, strictly
/// before the quote that was just typed.
pub(super) fn detect_auto_edit(
    old_value: &str,
    new_value: &str,
    selection: Range<usize>,
) -> Option<AutoEdit> {
    if selection.start != selection.end {
        return None;
    }
    let caret = selection.start;
    if new_value.len() > old_value.len() {
        detect_insertion(old_value, new_value, caret)
    } else if new_value.len() < old_value.len() {
        detect_empty_pair_backspace(old_value, new_value, caret)
    } else {
        None
    }
}

/// Apply `edit` to `value` (the post-change text) and return the text and caret.
pub(super) fn apply_auto_edit(value: &str, edit: &AutoEdit) -> (String, usize) {
    match edit {
        AutoEdit::InsertCloser { closer, cursor } => {
            let mut text = String::with_capacity(value.len() + closer.len_utf8());
            text.push_str(&value[..*cursor]);
            text.push(*closer);
            text.push_str(&value[*cursor..]);
            (text, *cursor)
        }
        AutoEdit::Splice { range, replacement } => {
            let mut text = String::with_capacity(value.len() + replacement.len());
            text.push_str(&value[..range.start]);
            text.push_str(replacement);
            text.push_str(&value[range.end..]);
            (text, range.start + replacement.len())
        }
    }
}

/// `selected_range` / `set_selected_range` are UTF-8 byte offsets.
/// `replace_text_in_range` takes UTF-16 code units, matching indent edits.
pub(super) fn apply_editor_auto_edit(
    editor: &mut EditorState,
    edit: &AutoEdit,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    match edit {
        AutoEdit::InsertCloser { closer, cursor } => {
            editor.set_selected_range(*cursor..*cursor, cx);
            editor.insert(closer.to_string(), window, cx);
            editor.set_selected_range(*cursor..*cursor, cx);
        }
        AutoEdit::Splice { range, replacement } => {
            let value = editor.value();
            let range_utf16 = byte_range_to_utf16(&value, range.start..range.end);
            editor.replace_text_in_range(Some(range_utf16), replacement, window, cx);
        }
    }
}

fn byte_range_to_utf16(value: &str, range: Range<usize>) -> Range<usize> {
    value[..range.start].encode_utf16().count()..value[..range.end].encode_utf16().count()
}

fn detect_insertion(old_value: &str, new_value: &str, caret: usize) -> Option<AutoEdit> {
    let (inserted, inserted_at) = single_char_insert(old_value, new_value, caret)?;
    let next = new_value[caret..].chars().next();
    match inserted {
        '(' | '[' | '{' => {
            let closer = matching_closer(inserted)?;
            if next == Some(closer) {
                None
            } else {
                Some(AutoEdit::InsertCloser {
                    closer,
                    cursor: caret,
                })
            }
        }
        ')' | ']' | '}' => overtype_splice(new_value, inserted_at, caret, inserted),
        '"' | '\'' | '`' => quote_edit(new_value, inserted, inserted_at, caret, next),
        _ => None,
    }
}

fn quote_edit(
    new_value: &str,
    quote: char,
    quote_at: usize,
    caret: usize,
    next: Option<char>,
) -> Option<AutoEdit> {
    // An escaped quote is neither an opener nor a closer.
    if quote_is_escaped(new_value, quote_at) {
        return None;
    }
    let unescaped_before = unescaped_same_quotes_before(new_value, quote_at, quote);
    if unescaped_before % 2 == 1 {
        if next == Some(quote) && !quote_is_escaped(new_value, caret) {
            overtype_splice(new_value, quote_at, caret, quote)
        } else {
            None
        }
    } else if next.is_none_or(char::is_whitespace) {
        Some(AutoEdit::InsertCloser {
            closer: quote,
            cursor: caret,
        })
    } else {
        None
    }
}

fn detect_empty_pair_backspace(old_value: &str, new_value: &str, caret: usize) -> Option<AutoEdit> {
    // The removed character sits at the post-edit caret, which is backspace:
    // the deleted opener was immediately before the caret. Forward-delete of a
    // closer removes that closer instead, so it does not match here.
    let deleted = single_char_delete(old_value, new_value, caret)?;
    let closer = matching_closer(deleted)?;
    let next = new_value[caret..].chars().next()?;
    if next != closer {
        return None;
    }
    Some(AutoEdit::Splice {
        range: caret..(caret + next.len_utf8()),
        replacement: String::new(),
    })
}

fn overtype_splice(text: &str, typed_at: usize, caret: usize, ch: char) -> Option<AutoEdit> {
    let next = text[caret..].chars().next()?;
    if next != ch {
        return None;
    }
    Some(AutoEdit::Splice {
        range: typed_at..(caret + next.len_utf8()),
        replacement: ch.to_string(),
    })
}

fn matching_closer(opener: char) -> Option<char> {
    match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' | '\'' | '`' => Some(opener),
        _ => None,
    }
}

/// A quote is escaped when the backslashes immediately before it have odd length.
fn quote_is_escaped(text: &str, quote_at: usize) -> bool {
    let bytes = text.as_bytes();
    let mut index = quote_at;
    let mut slashes = 0usize;
    while index > 0 && bytes[index - 1] == b'\\' {
        slashes += 1;
        index -= 1;
    }
    slashes % 2 == 1
}

fn unescaped_same_quotes_before(text: &str, quote_at: usize, quote: char) -> usize {
    let line_start = text[..quote_at].rfind('\n').map_or(0, |index| index + 1);
    let mut count = 0usize;
    let mut chars = text[line_start..quote_at].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // The next character is escaped, so a quote there does not count.
            let _escaped = chars.next();
            continue;
        }
        if ch == quote {
            count += 1;
        }
    }
    count
}

/// The character immediately before `caret`, if `new_value` is `old_value` plus that character.
fn single_char_insert(old_value: &str, new_value: &str, caret: usize) -> Option<(char, usize)> {
    let (inserted, inserted_at) = char_ending_at(new_value, caret)?;
    if old_value.len() + inserted.len_utf8() != new_value.len() || inserted_at > old_value.len() {
        return None;
    }
    if new_value.as_bytes()[..inserted_at] != old_value.as_bytes()[..inserted_at] {
        return None;
    }
    if new_value.as_bytes()[caret..] != old_value.as_bytes()[inserted_at..] {
        return None;
    }
    Some((inserted, inserted_at))
}

/// The character removed at `caret`, if that is the only difference and the caret is the deletion point.
fn single_char_delete(old_value: &str, new_value: &str, caret: usize) -> Option<char> {
    if caret > new_value.len()
        || caret > old_value.len()
        || !new_value.is_char_boundary(caret)
        || !old_value.is_char_boundary(caret)
    {
        return None;
    }
    let deleted = old_value[caret..].chars().next()?;
    let deleted_len = deleted.len_utf8();
    if old_value.len() != new_value.len() + deleted_len {
        return None;
    }
    if old_value.as_bytes()[..caret] != new_value.as_bytes()[..caret] {
        return None;
    }
    if old_value.as_bytes()[caret + deleted_len..] != new_value.as_bytes()[caret..] {
        return None;
    }
    Some(deleted)
}

fn char_ending_at(text: &str, end: usize) -> Option<(char, usize)> {
    if end == 0 || end > text.len() || !text.is_char_boundary(end) {
        return None;
    }
    let ch = text[..end].chars().next_back()?;
    Some((ch, end - ch.len_utf8()))
}
