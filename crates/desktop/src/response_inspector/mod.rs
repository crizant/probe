//! JSON and XML response inspection for JWTs, Unix timestamps, and Pretty-tab jumps.

mod findings;
mod json;
mod xml;

use std::ops::Range;

pub(crate) use json::inspect_json_file;
pub(crate) use xml::inspect_xml_file;

pub(crate) const INSPECT_MAX_BYTES: usize = 512 * 1024;
pub(super) const INSPECT_MAX_VALUES: usize = 10_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResponseInspection {
    pub jwts: Vec<JwtFinding>,
    pub timestamps: Vec<TimestampFinding>,
    pub skipped: Option<String>,
}

impl ResponseInspection {
    pub(crate) fn count(&self) -> usize {
        self.jwts.len() + self.timestamps.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JwtFinding {
    pub path: String,
    pub search: String,
    pub source_range: Option<Range<usize>>,
    pub header_json: String,
    pub payload_json: String,
    pub claims: Vec<JwtClaim>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JwtClaim {
    pub name: String,
    pub value: String,
    pub timestamp: Option<TimestampDisplay>,
    pub relative: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimestampFinding {
    pub path: String,
    pub search: String,
    pub source_range: Option<Range<usize>>,
    pub raw: String,
    pub timestamp: TimestampDisplay,
    pub confidence: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimestampDisplay {
    pub epoch_millis: i64,
    pub millisecond_precision: bool,
}

impl TimestampDisplay {
    pub(crate) fn local(&self) -> String {
        findings::format_millis_local(self.epoch_millis, self.millisecond_precision)
    }

    pub(crate) fn utc(&self) -> String {
        findings::format_millis_utc(self.epoch_millis, self.millisecond_precision)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InspectSelection {
    Jwt(usize),
    Timestamp(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InspectionRange {
    pub range: Range<usize>,
    pub selection: InspectSelection,
}

pub(crate) fn inspect_response_body(body: &[u8]) -> ResponseInspection {
    if body.len() > INSPECT_MAX_BYTES {
        return ResponseInspection {
            skipped: Some("Response is too large for automatic inspection.".to_owned()),
            ..ResponseInspection::default()
        };
    }
    if let Some(inspection) = json::inspect_json_bytes(body) {
        return inspection;
    }
    let Ok(source) = std::str::from_utf8(body) else {
        return ResponseInspection::default();
    };
    xml::inspect_xml_response(source)
}

pub(crate) fn inspection_text(inspection: &ResponseInspection) -> String {
    if inspection.is_empty() {
        return inspection
            .skipped
            .clone()
            .unwrap_or_else(|| "JWTs and Unix timestamps are detected automatically.".to_owned());
    }

    let mut text = String::new();
    if !inspection.jwts.is_empty() {
        text.push_str(&format!("JWT [{}]\n", inspection.jwts.len()));
        for jwt in &inspection.jwts {
            text.push_str(&format!("\n{}\n", jwt.path));
            text.push_str("Decoded locally. Signature not verified.\n");
            if !jwt.claims.is_empty() {
                text.push_str("Claims\n");
                for claim in &jwt.claims {
                    text.push_str("  ");
                    text.push_str(&claim.name);
                    text.push_str(": ");
                    text.push_str(&claim.value);
                    if let Some(timestamp) = &claim.timestamp {
                        text.push_str("  Local: ");
                        text.push_str(&timestamp.local());
                        text.push_str("  UTC: ");
                        text.push_str(&timestamp.utc());
                    }
                    if let Some(relative) = &claim.relative {
                        text.push_str("  ");
                        text.push_str(relative);
                    }
                    text.push('\n');
                }
            }
            text.push_str("Header\n");
            text.push_str(&jwt.header_json);
            text.push('\n');
            text.push_str("Payload\n");
            text.push_str(&jwt.payload_json);
            text.push('\n');
        }
    }

    if !inspection.timestamps.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("Timestamps [{}]\n", inspection.timestamps.len()));
        for timestamp in &inspection.timestamps {
            text.push_str(&format!("\n{}\n", timestamp.path));
            text.push_str("  Raw: ");
            text.push_str(&timestamp.raw);
            text.push('\n');
            text.push_str("  Local: ");
            text.push_str(&timestamp.timestamp.local());
            text.push('\n');
            text.push_str("  UTC: ");
            text.push_str(&timestamp.timestamp.utc());
            text.push('\n');
        }
    }

    if let Some(skipped) = &inspection.skipped {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(skipped);
        text.push('\n');
    }
    text
}

pub(crate) fn inspection_detail_text(
    inspection: &ResponseInspection,
    selection: Option<InspectSelection>,
) -> String {
    let Some(selection) = selection else {
        return inspection_text(inspection);
    };
    match selection {
        InspectSelection::Jwt(index) => inspection
            .jwts
            .get(index)
            .map(jwt_detail_text)
            .unwrap_or_else(|| inspection_text(inspection)),
        InspectSelection::Timestamp(index) => inspection
            .timestamps
            .get(index)
            .map(timestamp_detail_text)
            .unwrap_or_else(|| inspection_text(inspection)),
    }
}

pub(crate) fn first_inspection_selection(
    inspection: &ResponseInspection,
) -> Option<InspectSelection> {
    if !inspection.jwts.is_empty() {
        Some(InspectSelection::Jwt(0))
    } else if !inspection.timestamps.is_empty() {
        Some(InspectSelection::Timestamp(0))
    } else {
        None
    }
}

pub(crate) fn inspection_has_selection(
    inspection: &ResponseInspection,
    selection: InspectSelection,
) -> bool {
    match selection {
        InspectSelection::Jwt(index) => index < inspection.jwts.len(),
        InspectSelection::Timestamp(index) => index < inspection.timestamps.len(),
    }
}

pub(crate) fn inspection_selection_at_offset(
    ranges: &[InspectionRange],
    offset: usize,
) -> Option<InspectSelection> {
    ranges
        .iter()
        .find(|entry| entry.range.contains(&offset))
        .map(|entry| entry.selection)
}

pub(crate) fn inspection_value_ranges(
    pretty_text: &str,
    inspection: &ResponseInspection,
) -> Vec<InspectionRange> {
    if inspection.is_empty() {
        return Vec::new();
    }
    let source_ranges = inspection_source_ranges(inspection);
    if !source_ranges.is_empty() {
        return source_ranges;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(pretty_text) else {
        return Vec::new();
    };
    let targets = InspectionTargets::new(inspection);
    let mut ranges = Vec::with_capacity(inspection.count());
    json::collect_value_ranges(&value, pretty_text, &targets, &mut ranges);
    ranges
}

fn inspection_source_ranges(inspection: &ResponseInspection) -> Vec<InspectionRange> {
    let mut ranges = Vec::with_capacity(inspection.count());
    ranges.extend(
        inspection
            .jwts
            .iter()
            .enumerate()
            .filter_map(|(index, finding)| {
                finding.source_range.clone().map(|range| InspectionRange {
                    range,
                    selection: InspectSelection::Jwt(index),
                })
            }),
    );
    ranges.extend(
        inspection
            .timestamps
            .iter()
            .enumerate()
            .filter_map(|(index, finding)| {
                finding.source_range.clone().map(|range| InspectionRange {
                    range,
                    selection: InspectSelection::Timestamp(index),
                })
            }),
    );
    ranges.sort_by_key(|entry| entry.range.start);
    ranges
}

pub(super) struct InspectionTargets {
    entries: Vec<(String, InspectSelection)>,
}

impl InspectionTargets {
    pub(super) fn new(inspection: &ResponseInspection) -> Self {
        let mut entries = Vec::with_capacity(inspection.count());
        entries.extend(
            inspection
                .jwts
                .iter()
                .enumerate()
                .map(|(index, finding)| (finding.path.clone(), InspectSelection::Jwt(index))),
        );
        entries.extend(
            inspection
                .timestamps
                .iter()
                .enumerate()
                .map(|(index, finding)| (finding.path.clone(), InspectSelection::Timestamp(index))),
        );
        Self { entries }
    }

    pub(super) fn selection_for_path(&self, path: &str) -> Option<InspectSelection> {
        self.entries
            .iter()
            .find(|(entry_path, _)| entry_path == path)
            .map(|(_, selection)| *selection)
    }
}

fn jwt_detail_text(jwt: &JwtFinding) -> String {
    let mut text = String::new();
    text.push_str("JWT\n\n");
    text.push_str(&jwt.path);
    text.push('\n');
    text.push_str("Decoded locally. Signature not verified.\n");
    if !jwt.claims.is_empty() {
        text.push_str("\nClaims\n");
        for claim in &jwt.claims {
            text.push_str("  ");
            text.push_str(&claim.name);
            text.push_str(": ");
            text.push_str(&claim.value);
            if let Some(timestamp) = &claim.timestamp {
                text.push_str("  Local: ");
                text.push_str(&timestamp.local());
                text.push_str("  UTC: ");
                text.push_str(&timestamp.utc());
            }
            if let Some(relative) = &claim.relative {
                text.push_str("  ");
                text.push_str(relative);
            }
            text.push('\n');
        }
    }
    text.push_str("\nHeader\n");
    text.push_str(&jwt.header_json);
    text.push_str("\n\nPayload\n");
    text.push_str(&jwt.payload_json);
    text
}

fn timestamp_detail_text(timestamp: &TimestampFinding) -> String {
    let mut text = String::new();
    text.push_str("Timestamp\n\n");
    text.push_str(&timestamp.path);
    text.push('\n');
    text.push_str("Raw: ");
    text.push_str(&timestamp.raw);
    text.push('\n');
    text.push_str("Local: ");
    text.push_str(&timestamp.timestamp.local());
    text.push('\n');
    text.push_str("UTC: ");
    text.push_str(&timestamp.timestamp.utc());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests;
