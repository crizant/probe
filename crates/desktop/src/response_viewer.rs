//! Read-only response presentation: Pretty, Raw, Headers, and Search.
//!
//! gpui-base `Input` shapes a single line and is an editor, not a viewer. A whole
//! response body in one GPUI text node would layout every glyph. This module therefore
//! prepares bounded display rows so the desktop adapter can virtualize them with
//! `uniform_list`, pretty-print JSON off the UI thread when needed, and search without
//! scanning the original bytes on every frame.

use std::ops::Range;

use probe_http::{HttpResponse, ResponseHeader};

/// Longest display row, in Unicode scalars. Keeps virtualized rows bounded even when a
/// minified body is a single line.
pub(crate) const MAX_LINE_COLUMNS: usize = 256;

/// JSON pretty-print larger than this runs on a background executor.
pub(crate) const SYNC_PRETTY_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ResponseViewerTab {
    #[default]
    Pretty,
    Raw,
    Headers,
}

impl ResponseViewerTab {
    pub(crate) const ALL: [Self; 3] = [Self::Pretty, Self::Raw, Self::Headers];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Pretty => "Pretty",
            Self::Raw => "Raw",
            Self::Headers => "Headers",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyntaxRole {
    Property,
    String,
    Number,
    Boolean,
    Null,
    Punctuation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SearchColumn {
    Body,
    HeaderName,
    HeaderValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResponseLine {
    pub text: String,
    pub syntax: Vec<(Range<usize>, SyntaxRole)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SearchMatch {
    pub row: usize,
    pub range: Range<usize>,
    pub column: SearchColumn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedDocument {
    pub generation: u64,
    pub raw_lines: Vec<ResponseLine>,
    pub pretty_lines: Vec<ResponseLine>,
    pub pretty_pending: bool,
    pub pretty_notice: Option<String>,
    pub binary: bool,
    pub truncated: bool,
    pub headers: Vec<ResponseHeader>,
}

#[derive(Debug, Default)]
pub(crate) struct ResponseViewerState {
    tab: ResponseViewerTab,
    search: String,
    active_match: usize,
    documents: std::collections::BTreeMap<probe_core::RequestKey, PreparedDocument>,
    next_generation: u64,
}

impl ResponseViewerState {
    pub(crate) fn tab(&self) -> ResponseViewerTab {
        self.tab
    }

    pub(crate) fn search(&self) -> &str {
        &self.search
    }

    pub(crate) fn active_match(&self) -> usize {
        self.active_match
    }

    pub(crate) fn document(&self, key: probe_core::RequestKey) -> Option<&PreparedDocument> {
        self.documents.get(&key)
    }

    pub(crate) fn allocate_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1);
        self.next_generation
    }

    pub(crate) fn insert(&mut self, key: probe_core::RequestKey, document: PreparedDocument) {
        self.documents.insert(key, document);
        self.active_match = 0;
    }

    pub(crate) fn remove(&mut self, key: probe_core::RequestKey) {
        self.documents.remove(&key);
        self.active_match = 0;
    }

    pub(crate) fn clear(&mut self) {
        self.documents.clear();
        self.search.clear();
        self.active_match = 0;
        self.tab = ResponseViewerTab::default();
    }

    pub(crate) fn set_tab(&mut self, tab: ResponseViewerTab) {
        if self.tab != tab {
            self.tab = tab;
            self.active_match = 0;
        }
    }

    pub(crate) fn set_search(&mut self, query: String) {
        if self.search != query {
            self.search = query;
            self.active_match = 0;
        }
    }

    pub(crate) fn apply_pretty(
        &mut self,
        key: probe_core::RequestKey,
        generation: u64,
        pretty: PrettyBody,
    ) {
        let Some(document) = self.documents.get_mut(&key) else {
            return;
        };
        if document.generation != generation || !document.pretty_pending {
            return;
        }
        document.pretty_lines = pretty.lines;
        document.pretty_notice = pretty.notice;
        document.pretty_pending = false;
        self.active_match = 0;
    }

    pub(crate) fn visible_lines(&self, key: probe_core::RequestKey) -> &[ResponseLine] {
        let Some(document) = self.documents.get(&key) else {
            return &[];
        };
        match self.tab {
            ResponseViewerTab::Pretty => &document.pretty_lines,
            ResponseViewerTab::Raw => &document.raw_lines,
            ResponseViewerTab::Headers => &[],
        }
    }

    pub(crate) fn matches(&self, key: probe_core::RequestKey) -> Vec<SearchMatch> {
        let Some(document) = self.documents.get(&key) else {
            return Vec::new();
        };
        match self.tab {
            ResponseViewerTab::Headers => search_headers(&document.headers, &self.search),
            ResponseViewerTab::Pretty | ResponseViewerTab::Raw => {
                search_lines(self.visible_lines(key), &self.search)
            }
        }
    }

    pub(crate) fn step_match(
        &mut self,
        key: probe_core::RequestKey,
        delta: isize,
    ) -> Option<usize> {
        let count = self.matches(key).len();
        if count == 0 {
            self.active_match = 0;
            return None;
        }
        let next = (self.active_match as isize + delta).rem_euclid(count as isize) as usize;
        self.active_match = next;
        Some(next)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PrettyBody {
    pub lines: Vec<ResponseLine>,
    pub notice: Option<String>,
}

pub(crate) fn prepare_document(
    response: &HttpResponse,
    generation: u64,
) -> (PreparedDocument, bool) {
    let truncated = !response.body_complete;
    if response.body.is_empty() {
        return (
            PreparedDocument {
                generation,
                raw_lines: Vec::new(),
                pretty_lines: Vec::new(),
                pretty_pending: false,
                pretty_notice: None,
                binary: false,
                truncated,
                headers: response.headers.clone(),
            },
            false,
        );
    }
    if std::str::from_utf8(&response.body).is_err() {
        return (
            PreparedDocument {
                generation,
                raw_lines: Vec::new(),
                pretty_lines: Vec::new(),
                pretty_pending: false,
                pretty_notice: Some(format!("Binary response body ({} bytes).", response.size)),
                binary: true,
                truncated,
                headers: response.headers.clone(),
            },
            false,
        );
    }

    let text = String::from_utf8_lossy(&response.body);
    let raw_lines = display_lines(&text, &[], MAX_LINE_COLUMNS);
    let json_candidate = looks_like_json(response);
    let pending = json_candidate && response.body.len() > SYNC_PRETTY_BYTES;
    let (pretty_lines, pretty_notice, pretty_pending) = if pending {
        (raw_lines.clone(), Some("Formatting JSON…".to_owned()), true)
    } else if json_candidate {
        let pretty = pretty_json_body(&response.body);
        (pretty.lines, pretty.notice, false)
    } else {
        (
            raw_lines.clone(),
            Some("Pretty formatting is available for JSON responses.".to_owned()),
            false,
        )
    };

    (
        PreparedDocument {
            generation,
            raw_lines,
            pretty_lines,
            pretty_pending,
            pretty_notice,
            binary: false,
            truncated,
            headers: response.headers.clone(),
        },
        pretty_pending,
    )
}

pub(crate) fn pretty_json_body(body: &[u8]) -> PrettyBody {
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(pretty) => {
                let highlights = highlight_json(&pretty);
                PrettyBody {
                    lines: display_lines(&pretty, &highlights, MAX_LINE_COLUMNS),
                    notice: None,
                }
            }
            Err(_) => PrettyBody {
                lines: display_lines(&String::from_utf8_lossy(body), &[], MAX_LINE_COLUMNS),
                notice: Some("Could not pretty-print this JSON response.".to_owned()),
            },
        },
        Err(_) => PrettyBody {
            lines: display_lines(&String::from_utf8_lossy(body), &[], MAX_LINE_COLUMNS),
            notice: Some("Response is not valid JSON.".to_owned()),
        },
    }
}

pub(crate) fn looks_like_json(response: &HttpResponse) -> bool {
    if let Some(content_type) = content_type(response)
        && content_type.to_ascii_lowercase().contains("json")
    {
        return true;
    }
    let trimmed = trim_ascii_start(&response.body);
    matches!(trimmed.first(), Some(b'{' | b'['))
}

fn content_type(response: &HttpResponse) -> Option<&str> {
    response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let index = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[index..]
}

pub(crate) fn display_lines(
    text: &str,
    highlights: &[(Range<usize>, SyntaxRole)],
    max_cols: usize,
) -> Vec<ResponseLine> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_start = 0;
    let mut cols = 0;

    let flush = |current: &mut String,
                 current_start: usize,
                 lines: &mut Vec<ResponseLine>,
                 highlights: &[(Range<usize>, SyntaxRole)]| {
        let end = current_start + current.len();
        lines.push(ResponseLine {
            text: std::mem::take(current),
            syntax: clip_highlights(highlights, current_start..end),
        });
    };

    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            flush(&mut current, current_start, &mut lines, highlights);
            current_start = idx + ch.len_utf8();
            cols = 0;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        if max_cols > 0 && cols == max_cols && !current.is_empty() {
            flush(&mut current, current_start, &mut lines, highlights);
            current_start = idx;
            cols = 0;
        }
        current.push(ch);
        cols += 1;
    }
    if !current.is_empty() || lines.is_empty() || text.ends_with('\n') {
        flush(&mut current, current_start, &mut lines, highlights);
    }
    lines
}

fn clip_highlights(
    highlights: &[(Range<usize>, SyntaxRole)],
    span: Range<usize>,
) -> Vec<(Range<usize>, SyntaxRole)> {
    highlights
        .iter()
        .filter_map(|(range, role)| {
            let start = range.start.max(span.start);
            let end = range.end.min(span.end);
            (start < end).then(|| (start - span.start..end - span.start, *role))
        })
        .collect()
}

pub(crate) fn highlight_json(text: &str) -> Vec<(Range<usize>, SyntaxRole)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut spans = Vec::new();
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\n' | b'\r' | b'\t' => index += 1,
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                spans.push((index..index + 1, SyntaxRole::Punctuation));
                index += 1;
            }
            b'"' => {
                let start = index;
                index += 1;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => index = (index + 2).min(bytes.len()),
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
                let role = if followed_by_colon(bytes, index) {
                    SyntaxRole::Property
                } else {
                    SyntaxRole::String
                };
                spans.push((start..index, role));
            }
            b't' if keyword_at(text, index, "true") => {
                spans.push((index..index + 4, SyntaxRole::Boolean));
                index += 4;
            }
            b'f' if keyword_at(text, index, "false") => {
                spans.push((index..index + 5, SyntaxRole::Boolean));
                index += 5;
            }
            b'n' if keyword_at(text, index, "null") => {
                spans.push((index..index + 4, SyntaxRole::Null));
                index += 4;
            }
            b'-' | b'0'..=b'9' => {
                let start = index;
                index = scan_json_number(bytes, index);
                spans.push((start..index, SyntaxRole::Number));
            }
            _ => index += 1,
        }
    }
    spans
}

fn keyword_at(text: &str, index: usize, keyword: &str) -> bool {
    text[index..].starts_with(keyword)
        && !text
            .as_bytes()
            .get(index + keyword.len())
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn followed_by_colon(bytes: &[u8], mut index: usize) -> bool {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    bytes.get(index) == Some(&b':')
}

fn scan_json_number(bytes: &[u8], mut index: usize) -> usize {
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    index
}

pub(crate) fn search_lines(lines: &[ResponseLine], query: &str) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        for range in find_ignore_case(&line.text, query) {
            matches.push(SearchMatch {
                row,
                range,
                column: SearchColumn::Body,
            });
        }
    }
    matches
}

pub(crate) fn search_headers(headers: &[ResponseHeader], query: &str) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for (row, header) in headers.iter().enumerate() {
        for range in find_ignore_case(&header.name, query) {
            matches.push(SearchMatch {
                row,
                range,
                column: SearchColumn::HeaderName,
            });
        }
        for range in find_ignore_case(&header.value, query) {
            matches.push(SearchMatch {
                row,
                range,
                column: SearchColumn::HeaderValue,
            });
        }
    }
    matches
}

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

fn chars_eq_ignore_case(haystack: &[(usize, char)], start: usize, needle: &[char]) -> bool {
    if start + needle.len() > haystack.len() {
        return false;
    }
    haystack[start..start + needle.len()]
        .iter()
        .zip(needle)
        .all(|((_, haystack_char), needle_char)| equal_ignore_case(*haystack_char, *needle_char))
}

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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use probe_http::{HttpResponse, ResponseHeader};

    use super::{
        MAX_LINE_COLUMNS, PreparedDocument, ResponseViewerTab, SearchColumn, SyntaxRole,
        display_lines, highlight_json, looks_like_json, prepare_document, pretty_json_body,
        search_headers, search_lines,
    };

    fn response(body: &[u8], content_type: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            reason: "OK".to_owned(),
            url: String::new(),
            duration: Duration::ZERO,
            size: body.len(),
            headers: vec![ResponseHeader {
                name: "content-type".to_owned(),
                value: content_type.to_owned(),
            }],
            body: body.to_vec(),
            body_complete: true,
        }
    }

    #[test]
    fn pretty_json_indents_and_highlights_tokens() {
        let pretty = pretty_json_body(br#"{"ok":true,"n":1}"#);
        assert!(pretty.notice.is_none());
        let text = pretty
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains('\n'));
        assert!(text.contains("\"ok\""));
        let roles: Vec<SyntaxRole> = pretty
            .lines
            .iter()
            .flat_map(|line| line.syntax.iter().map(|(_, role)| *role))
            .collect();
        assert!(roles.contains(&SyntaxRole::Property));
        assert!(roles.contains(&SyntaxRole::Boolean));
        assert!(roles.contains(&SyntaxRole::Number));
    }

    #[test]
    fn long_lines_are_chunked_for_virtualized_rows() {
        let line = "x".repeat(MAX_LINE_COLUMNS * 3 + 10);
        let lines = display_lines(&line, &[], MAX_LINE_COLUMNS);
        assert_eq!(lines.len(), 4);
        assert!(
            lines
                .iter()
                .all(|row| row.text.chars().count() <= MAX_LINE_COLUMNS)
        );
    }

    #[test]
    fn search_is_case_insensitive_and_records_byte_ranges() {
        let lines = display_lines("Alpha\nbeta ALPHA", &[], MAX_LINE_COLUMNS);
        let matches = search_lines(&lines, "alpha");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].row, 0);
        assert_eq!(&lines[0].text[matches[0].range.clone()], "Alpha");
        assert_eq!(matches[1].row, 1);
    }

    #[test]
    fn header_search_covers_names_and_values() {
        let headers = [
            ResponseHeader {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            },
            ResponseHeader {
                name: "x-request-id".to_owned(),
                value: "abc".to_owned(),
            },
        ];
        let matches = search_headers(&headers, "json");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].column, SearchColumn::HeaderValue);
        assert_eq!(matches[0].row, 0);
    }

    #[test]
    fn binary_and_json_sniffing_prepare_the_expected_document() {
        let json = response(br#"{"ok":true}"#, "application/json");
        assert!(looks_like_json(&json));
        let (document, pending) = prepare_document(&json, 1);
        assert!(!pending);
        assert!(!document.binary);
        assert!(document.pretty_notice.is_none());
        assert!(document.pretty_lines.len() > 1);

        let binary = response(&[0, 159, 146, 150], "application/octet-stream");
        let (document, pending) = prepare_document(&binary, 2);
        assert!(!pending);
        assert!(document.binary);
        assert!(document.raw_lines.is_empty());
    }

    #[test]
    fn invalid_json_keeps_raw_text_and_explains_pretty_failure() {
        let response = response(b"{not json", "application/json");
        let (PreparedDocument { pretty_notice, .. }, pending) = prepare_document(&response, 1);
        assert!(!pending);
        assert_eq!(
            pretty_notice.as_deref(),
            Some("Response is not valid JSON.")
        );
    }

    #[test]
    fn highlight_json_treats_object_keys_as_properties() {
        let source = "{\n  \"name\": \"Ada\"\n}";
        let highlights = highlight_json(source);
        let property = highlights
            .iter()
            .find(|(_, role)| *role == SyntaxRole::Property)
            .expect("property span");
        assert_eq!(&source[property.0.clone()], "\"name\"");
        let string = highlights
            .iter()
            .find(|(_, role)| *role == SyntaxRole::String)
            .expect("string span");
        assert_eq!(&source[string.0.clone()], "\"Ada\"");
    }

    #[test]
    fn viewer_tabs_are_stable() {
        assert_eq!(ResponseViewerTab::ALL.len(), 3);
        assert_eq!(ResponseViewerTab::Pretty.label(), "Pretty");
    }
}
