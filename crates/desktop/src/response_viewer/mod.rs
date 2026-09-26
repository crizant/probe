//! Read-only response presentation: Pretty/Preview, Raw (Text/Base64), Headers, Inspect, and
//! Search.
//!
//! This module retains response text without altering it, searches the active
//! representation, and pretty-prints JSON and XML off the UI thread. Syntax coloring is
//! applied by the gpui-base `Editor` highlighter.

mod encode;
mod prepare;
mod search;

use std::{ops::Range, sync::Arc};

use gpui::{Image, ScrollHandle, SharedString};

use crate::response_inspector::{
    InspectSelection, InspectionRange, ResponseInspection, first_inspection_selection,
    inspect_response_body, inspection_has_selection, inspection_selection_at_offset,
    inspection_value_ranges,
};
use encode::page_bytes;
pub(crate) use encode::{encode_base64, encode_hex};
pub(crate) use prepare::{PrettyBody, prepare_document, pretty_body};
use probe_http::ResponseHeader;
pub(crate) use search::join_header_lines;

/// JSON/XML pretty-print and Base64 encoding larger than this run on a background executor.
pub(crate) const SYNC_PRETTY_BYTES: usize = 64 * 1024;
pub(crate) const RESPONSE_PAGE_BYTES: usize = probe_http::MAX_IN_MEMORY_RESPONSE_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PageDirection {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ResponseViewerTab {
    #[default]
    Pretty,
    Raw,
    Headers,
    Inspect,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ResponseBodySyntax {
    #[default]
    Plain,
    Json,
    Xml,
}

impl ResponseBodySyntax {
    pub(crate) const fn language(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Json => "json",
            Self::Xml => "xml",
        }
    }
}

impl ResponseViewerTab {
    pub(crate) const ALL: [Self; 4] = [Self::Pretty, Self::Raw, Self::Headers, Self::Inspect];
    pub(crate) const TRUNCATED: [Self; 3] = [Self::Raw, Self::Headers, Self::Inspect];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Pretty => "Pretty",
            Self::Raw => "Raw",
            Self::Headers => "Headers",
            Self::Inspect => "Inspect",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum RawBodyView {
    #[default]
    Text,
    Base64,
    Hex,
}

impl RawBodyView {
    pub(crate) const ALL: [Self; 3] = [Self::Text, Self::Hex, Self::Base64];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Base64 => "Base64",
            Self::Hex => "Hex",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SearchMatch {
    pub range: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedDocument {
    pub generation: u64,
    pub raw_text: SharedString,
    pub pretty_text: String,
    pub pretty_pending: bool,
    pub pretty_notice: Option<String>,
    pub page_body: Vec<u8>,
    pub base64_text: String,
    pub base64_pending: bool,
    pub hex_text: String,
    pub hex_pending: bool,
    pub image_preview: Option<ResponseImagePreview>,
    pub syntax: ResponseBodySyntax,
    pub binary: bool,
    pub file_backed: bool,
    pub truncated: bool,
    pub retention_notice: Option<String>,
    pub page_offset: usize,
    pub page_len: usize,
    pub page_revision: u64,
    pub total_size: usize,
    pub page_pending: bool,
    pub headers: Vec<ResponseHeader>,
    pub inspection: ResponseInspection,
    pub inspection_pending: bool,
    pub inspection_ranges: Vec<InspectionRange>,
    pub inspection_selection: Option<InspectSelection>,
}

impl PreparedDocument {
    pub(crate) fn is_image(&self) -> bool {
        self.image_preview.is_some()
    }

    pub(crate) fn can_load_previous_page(&self) -> bool {
        self.file_backed && self.page_offset > 0 && !self.page_pending
    }

    pub(crate) fn can_load_next_page(&self) -> bool {
        self.file_backed
            && !self.page_pending
            && self
                .page_offset
                .checked_add(self.page_len)
                .is_some_and(|end| end < self.total_size)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResponseImagePreview {
    Ready(Arc<Image>),
    Unavailable(String),
}

#[derive(Debug, Default)]
pub(crate) struct ResponseViewerState {
    selections: std::collections::HashMap<probe_core::RequestKey, ResponseSelection>,
    documents: std::collections::BTreeMap<probe_core::RequestKey, PreparedDocument>,
    image_scrolls: std::collections::BTreeMap<probe_core::RequestKey, ScrollHandle>,
    next_generation: u64,
}

#[derive(Debug, Default)]
struct ResponseSelection {
    tab: ResponseViewerTab,
    raw_view: RawBodyView,
}

impl ResponseViewerState {
    pub(crate) fn tab(&self, key: probe_core::RequestKey) -> ResponseViewerTab {
        self.selections
            .get(&key)
            .map(|selection| selection.tab)
            .unwrap_or_default()
    }

    pub(crate) fn raw_view(&self, key: probe_core::RequestKey) -> RawBodyView {
        self.selections
            .get(&key)
            .map(|selection| selection.raw_view)
            .unwrap_or_default()
    }

    pub(crate) fn document(&self, key: probe_core::RequestKey) -> Option<&PreparedDocument> {
        self.documents.get(&key)
    }

    pub(crate) fn image_scroll(&self, key: probe_core::RequestKey) -> Option<&ScrollHandle> {
        self.image_scrolls.get(&key)
    }

    pub(crate) fn inspection_selection(
        &self,
        key: probe_core::RequestKey,
    ) -> Option<InspectSelection> {
        let document = self.documents.get(&key)?;
        document
            .inspection_selection
            .filter(|selection| inspection_has_selection(&document.inspection, *selection))
            .or_else(|| first_inspection_selection(&document.inspection))
    }

    pub(crate) fn allocate_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1);
        self.next_generation
    }

    pub(crate) fn insert(&mut self, key: probe_core::RequestKey, document: PreparedDocument) {
        if document.is_image() {
            self.image_scrolls.insert(key, ScrollHandle::new());
        } else {
            self.image_scrolls.remove(&key);
        }
        self.documents.insert(key, document);
    }

    pub(crate) fn ensure_available_tab(&mut self, key: probe_core::RequestKey) {
        let Some(document) = self.documents.get(&key) else {
            return;
        };
        let (truncated, binary) = (document.truncated, document.binary);
        if self.tab(key) == ResponseViewerTab::Pretty && truncated {
            self.set_tab(key, ResponseViewerTab::Raw);
        }
        if binary && self.raw_view(key) == RawBodyView::Text {
            self.set_raw_view(key, RawBodyView::Hex);
        }
    }

    pub(crate) fn remove(&mut self, key: probe_core::RequestKey) {
        self.documents.remove(&key);
        self.image_scrolls.remove(&key);
    }

    pub(crate) fn remove_selection(&mut self, key: probe_core::RequestKey) {
        self.selections.remove(&key);
    }

    pub(crate) fn clear(&mut self) {
        self.selections.clear();
        self.documents.clear();
        self.image_scrolls.clear();
    }

    pub(crate) fn remap_requests(
        &mut self,
        key_remaps: &std::collections::BTreeMap<probe_core::RequestKey, probe_core::RequestKey>,
    ) {
        self.selections = std::mem::take(&mut self.selections)
            .into_iter()
            .filter_map(|(key, selection)| key_remaps.get(&key).map(|new| (*new, selection)))
            .collect();
        self.documents = std::mem::take(&mut self.documents)
            .into_iter()
            .filter_map(|(key, document)| key_remaps.get(&key).map(|new| (*new, document)))
            .collect();
        self.image_scrolls = std::mem::take(&mut self.image_scrolls)
            .into_iter()
            .filter_map(|(key, scroll)| key_remaps.get(&key).map(|new| (*new, scroll)))
            .collect();
    }

    pub(crate) fn set_tab(&mut self, key: probe_core::RequestKey, tab: ResponseViewerTab) {
        self.selections.entry(key).or_default().tab = tab;
    }

    pub(crate) fn set_raw_view(&mut self, key: probe_core::RequestKey, view: RawBodyView) {
        self.selections.entry(key).or_default().raw_view = view;
    }

    pub(crate) fn take_base64_job(
        &mut self,
        key: probe_core::RequestKey,
    ) -> Option<(u64, Vec<u8>, u64)> {
        if self.tab(key) != ResponseViewerTab::Raw || self.raw_view(key) != RawBodyView::Base64 {
            return None;
        }
        let document = self.documents.get_mut(&key)?;
        if document.base64_pending || !document.base64_text.is_empty() {
            return None;
        }
        let bytes = page_bytes(document).to_vec();
        if bytes.is_empty() {
            return None;
        }
        let page_revision = document.page_revision;
        if bytes.len() <= SYNC_PRETTY_BYTES {
            document.base64_text = encode_base64(&bytes);
            None
        } else {
            document.base64_pending = true;
            Some((document.generation, bytes, page_revision))
        }
    }

    pub(crate) fn take_hex_job(
        &mut self,
        key: probe_core::RequestKey,
    ) -> Option<(u64, Vec<u8>, usize, u64)> {
        if self.tab(key) != ResponseViewerTab::Raw || self.raw_view(key) != RawBodyView::Hex {
            return None;
        }
        let document = self.documents.get_mut(&key)?;
        if document.hex_pending || !document.hex_text.is_empty() {
            return None;
        }
        let bytes = page_bytes(document).to_vec();
        if bytes.is_empty() {
            return None;
        }
        let offset = document.page_offset;
        let page_revision = document.page_revision;
        if bytes.len() <= SYNC_PRETTY_BYTES {
            document.hex_text = encode_hex(&bytes, offset);
            None
        } else {
            document.hex_pending = true;
            Some((document.generation, bytes, offset, page_revision))
        }
    }

    pub(crate) fn apply_base64(
        &mut self,
        key: probe_core::RequestKey,
        generation: u64,
        page_revision: u64,
        encoded: String,
    ) {
        let Some(document) = self.documents.get_mut(&key) else {
            return;
        };
        if document.generation != generation
            || !document.base64_pending
            || document.page_revision != page_revision
        {
            return;
        }
        document.base64_text = encoded;
        document.base64_pending = false;
    }

    pub(crate) fn apply_hex(
        &mut self,
        key: probe_core::RequestKey,
        generation: u64,
        offset: usize,
        page_revision: u64,
        encoded: String,
    ) {
        let Some(document) = self.documents.get_mut(&key) else {
            return;
        };
        if document.generation != generation
            || !document.hex_pending
            || document.page_offset != offset
            || document.page_revision != page_revision
        {
            return;
        }
        document.hex_text = encoded;
        document.hex_pending = false;
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
        let pretty_succeeded = pretty.notice.is_none();
        document.pretty_text = pretty.text;
        document.pretty_notice = pretty.notice;
        document.pretty_pending = false;
        if pretty_succeeded && document.syntax == ResponseBodySyntax::Xml {
            document.inspection = inspect_response_body(document.pretty_text.as_bytes());
            document.inspection_selection = first_inspection_selection(&document.inspection);
        }
        document.inspection_ranges =
            inspection_value_ranges(&document.pretty_text, &document.inspection);
    }

    pub(crate) fn apply_inspection(
        &mut self,
        key: probe_core::RequestKey,
        generation: u64,
        inspection: ResponseInspection,
    ) {
        let Some(document) = self.documents.get_mut(&key) else {
            return;
        };
        if document.generation != generation || !document.inspection_pending {
            return;
        }
        document.inspection = inspection;
        document.inspection_pending = false;
        document.inspection_selection = first_inspection_selection(&document.inspection);
        document.inspection_ranges =
            inspection_value_ranges(&document.pretty_text, &document.inspection);
    }

    pub(crate) fn begin_page(
        &mut self,
        key: probe_core::RequestKey,
        direction: PageDirection,
    ) -> Option<(u64, usize)> {
        let document = self.documents.get_mut(&key)?;
        if !document.file_backed || document.page_pending {
            return None;
        }
        let offset = match direction {
            PageDirection::Previous if document.can_load_previous_page() => {
                document.page_offset.saturating_sub(RESPONSE_PAGE_BYTES)
            }
            PageDirection::Previous => return None,
            PageDirection::Next => document
                .can_load_next_page()
                .then_some(document.page_offset)
                .and_then(|offset| offset.checked_add(RESPONSE_PAGE_BYTES))
                .filter(|offset| *offset < document.total_size)?,
        };
        if offset == document.page_offset {
            return None;
        }
        document.page_pending = true;
        Some((document.generation, offset))
    }

    pub(crate) fn apply_page(
        &mut self,
        key: probe_core::RequestKey,
        generation: u64,
        offset: usize,
        body: Vec<u8>,
    ) {
        let Some(document) = self.documents.get_mut(&key) else {
            return;
        };
        if document.generation != generation || !document.page_pending {
            return;
        }
        document.page_offset = offset;
        document.page_len = body.len();
        document.page_revision = document.page_revision.wrapping_add(1);
        if document.binary {
            document.page_body = body;
            document.raw_text = SharedString::default();
        } else {
            document.raw_text = String::from_utf8_lossy(&body).into_owned().into();
            document.page_body.clear();
        }
        document.base64_text.clear();
        document.base64_pending = false;
        document.hex_text.clear();
        document.hex_pending = false;
        document.pretty_text.clear();
        document.pretty_notice = None;
        document.inspection_ranges.clear();
        document.page_pending = false;
    }

    pub(crate) fn fail_page(
        &mut self,
        key: probe_core::RequestKey,
        generation: u64,
        message: String,
    ) {
        let Some(document) = self.documents.get_mut(&key) else {
            return;
        };
        if document.generation == generation && document.page_pending {
            document.page_pending = false;
            document.pretty_notice = Some(message);
        }
    }

    pub(crate) fn select_inspection_at_offset(
        &mut self,
        key: probe_core::RequestKey,
        offset: usize,
    ) -> Option<InspectSelection> {
        let document = self.documents.get_mut(&key)?;
        let selection = inspection_selection_at_offset(&document.inspection_ranges, offset)?;
        document.inspection_selection = Some(selection);
        self.set_tab(key, ResponseViewerTab::Inspect);
        Some(selection)
    }

    pub(crate) fn select_inspection(
        &mut self,
        key: probe_core::RequestKey,
        selection: InspectSelection,
    ) {
        if let Some(document) = self.documents.get_mut(&key) {
            document.inspection_selection = Some(selection);
        }
    }

    pub(crate) fn inspection_range_for_selection(
        &self,
        key: probe_core::RequestKey,
        selection: InspectSelection,
    ) -> Option<Range<usize>> {
        let document = self.documents.get(&key)?;
        document
            .inspection_ranges
            .iter()
            .find(|range| range.selection == selection)
            .map(|range| range.range.clone())
    }

    pub(crate) fn reveal_inspection_in_pretty(
        &mut self,
        key: probe_core::RequestKey,
    ) -> Option<InspectSelection> {
        let selection = self.inspection_selection(key)?;
        self.inspection_range_for_selection(key, selection)?;
        if let Some(document) = self.documents.get_mut(&key) {
            document.inspection_selection = Some(selection);
        }
        self.set_tab(key, ResponseViewerTab::Pretty);
        Some(selection)
    }

    pub(crate) fn visible_text(&self, key: probe_core::RequestKey) -> SharedString {
        let Some(document) = self.documents.get(&key) else {
            return SharedString::default();
        };
        match self.tab(key) {
            ResponseViewerTab::Pretty => SharedString::from(document.pretty_text.as_str()),
            ResponseViewerTab::Raw => match self.raw_view(key) {
                RawBodyView::Text => document.raw_text.clone(),
                RawBodyView::Base64 => SharedString::from(document.base64_text.as_str()),
                RawBodyView::Hex => SharedString::from(document.hex_text.as_str()),
            },
            ResponseViewerTab::Headers | ResponseViewerTab::Inspect => SharedString::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn visible_line_count(&self, key: probe_core::RequestKey) -> usize {
        let text = self.visible_text(key);
        if text.is_empty() {
            0
        } else {
            text.lines().count() + usize::from(text.ends_with('\n'))
        }
    }

    #[cfg(test)]
    fn show_raw_base64(&mut self, key: probe_core::RequestKey) {
        self.set_tab(key, ResponseViewerTab::Raw);
        self.set_raw_view(key, RawBodyView::Base64);
        if let Some((generation, bytes, page_revision)) = self.take_base64_job(key) {
            self.apply_base64(key, generation, page_revision, encode_base64(&bytes));
        }
    }

    #[cfg(test)]
    fn show_raw_hex(&mut self, key: probe_core::RequestKey) {
        self.set_tab(key, ResponseViewerTab::Raw);
        self.set_raw_view(key, RawBodyView::Hex);
        if let Some((generation, bytes, offset, page_revision)) = self.take_hex_job(key) {
            self.apply_hex(
                key,
                generation,
                offset,
                page_revision,
                encode_hex(&bytes, offset),
            );
        }
    }
}

#[cfg(test)]
mod tests;
