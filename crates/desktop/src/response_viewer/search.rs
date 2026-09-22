//! Header line joining and response text search.

#[cfg(test)]
use std::ops::Range;

use probe_http::ResponseHeader;

#[cfg(test)]
use super::SearchMatch;

pub(crate) const HEADER_SEPARATOR: &str = ": ";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JoinedHeaders {
    pub text: String,
    pub line_offsets: Vec<usize>,
    pub name_lens: Vec<usize>,
}

pub(crate) fn join_header_lines(headers: &[ResponseHeader]) -> JoinedHeaders {
    let mut text = String::new();
    let mut line_offsets = Vec::with_capacity(headers.len());
    let mut name_lens = Vec::with_capacity(headers.len());
    for (index, header) in headers.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        line_offsets.push(text.len());
        name_lens.push(header.name.len());
        text.push_str(&header.name);
        text.push_str(HEADER_SEPARATOR);
        text.push_str(&header.value);
    }
    JoinedHeaders {
        text,
        line_offsets,
        name_lens,
    }
}

#[cfg(test)]
pub(crate) fn search_text(text: &str, query: &str) -> Vec<SearchMatch> {
    find_ignore_case(text, query)
        .into_iter()
        .map(|range| SearchMatch { range })
        .collect()
}

#[cfg(test)]
pub(crate) fn search_headers(headers: &[ResponseHeader], query: &str) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut line_start = 0;
    for header in headers {
        for range in find_ignore_case(&header.name, query) {
            matches.push(SearchMatch {
                range: line_start + range.start..line_start + range.end,
            });
        }
        let value_start = line_start + header.name.len() + HEADER_SEPARATOR.len();
        for range in find_ignore_case(&header.value, query) {
            matches.push(SearchMatch {
                range: value_start + range.start..value_start + range.end,
            });
        }
        line_start = value_start + header.value.len() + 1;
    }
    matches
}

#[cfg(test)]
fn find_ignore_case(haystack: &str, needle: &str) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let needle_chars: Vec<char> = needle.chars().collect();
    let haystack_chars: Vec<(usize, char)> = haystack.char_indices().collect();
    let mut matches = Vec::new();
    let mut index = 0;
    while index < haystack_chars.len() {
        if chars_eq_ignore_case(&haystack_chars, index, &needle_chars) {
            let start = haystack_chars[index].0;
            let end = haystack_chars
                .get(index + needle_chars.len())
                .map(|(next, _)| *next)
                .unwrap_or(haystack.len());
            matches.push(start..end);
            index += needle_chars.len();
        } else {
            index += 1;
        }
    }
    matches
}

#[cfg(test)]
fn chars_eq_ignore_case(haystack: &[(usize, char)], start: usize, needle: &[char]) -> bool {
    if start + needle.len() > haystack.len() {
        return false;
    }
    haystack[start..start + needle.len()]
        .iter()
        .zip(needle)
        .all(|((_, haystack_char), needle_char)| equal_ignore_case(*haystack_char, *needle_char))
}

#[cfg(test)]
fn equal_ignore_case(left: char, right: char) -> bool {
    if left.eq_ignore_ascii_case(&right) {
        return true;
    }
    let mut left_lower = left.to_lowercase();
    let mut right_lower = right.to_lowercase();
    loop {
        match (left_lower.next(), right_lower.next()) {
            (Some(left_ch), Some(right_ch)) if left_ch == right_ch => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}
