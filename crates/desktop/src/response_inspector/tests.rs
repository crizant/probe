use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    InspectSelection, ResponseInspection, inspect_json_file, inspect_response_body,
    inspect_xml_file, inspection_detail_text, inspection_selection_at_offset, inspection_text,
    inspection_value_ranges,
};

fn inspect_temp_source(
    suffix: &str,
    source: impl AsRef<[u8]>,
    inspect: fn(&Path) -> ResponseInspection,
) -> ResponseInspection {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "probe-streaming-inspection-{}-{unique}.{suffix}",
        std::process::id()
    ));
    std::fs::write(&path, source).unwrap();
    let inspection = inspect(&path);
    std::fs::remove_file(path).unwrap();
    inspection
}

#[test]
fn inspection_detects_structurally_valid_jwts() {
    let token = concat!(
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.",
        "eyJzdWIiOiIxMjMiLCJpYXQiOjE3ODc0ODI4MDAsImV4cCI6MTc4NzQ4NjQwMH0.",
        "signature"
    );
    let source = format!(r#"{{"authResponse":{{"accessToken":"{token}"}}}}"#);
    let inspection = inspect_response_body(source.as_bytes());

    assert_eq!(inspection.jwts.len(), 1);
    assert_eq!(inspection.jwts[0].path, "authResponse.accessToken");
    assert!(inspection.jwts[0].header_json.contains("\"alg\""));
    assert!(
        inspection.jwts[0]
            .claims
            .iter()
            .any(|claim| claim.name == "exp" && claim.timestamp.is_some())
    );
}

#[test]
fn streaming_inspection_reaches_findings_after_the_memory_preview() {
    let padding = "x".repeat(1024 * 1024);
    let json = inspect_temp_source(
        "json",
        format!(r#"{{"padding":"{padding}","createdAt":1787482800}}"#),
        inspect_json_file,
    );
    assert_eq!(json.timestamps.len(), 1);
    assert_eq!(json.timestamps[0].path, "createdAt");

    let xml = inspect_temp_source(
        "xml",
        format!(r#"<root><padding>{padding}</padding><item createdAt="1787482800"/></root>"#),
        inspect_xml_file,
    );
    assert_eq!(xml.timestamps.len(), 1);
    assert_eq!(xml.timestamps[0].path, "/root/item/@createdAt");
}

#[test]
fn streaming_inspection_rejects_invalid_input_without_partial_findings() {
    let json = inspect_temp_source(
        "json",
        r#"{"createdAt":1787482800} trailing"#,
        inspect_json_file,
    );
    assert!(json.timestamps.is_empty());
    assert_eq!(json.skipped.as_deref(), Some("Response is not valid JSON."));

    let xml = inspect_temp_source(
        "xml",
        r#"<root createdAt="1787482800"><broken></root>"#,
        inspect_xml_file,
    );
    assert!(xml.timestamps.is_empty());
    assert_eq!(xml.skipped.as_deref(), Some("Response is not valid XML."));
}

#[test]
fn streaming_json_inspection_stops_at_the_value_limit() {
    let source = format!(
        "[{},{{\"createdAt\":1787482800}}]",
        std::iter::repeat_n("0", 10_000)
            .collect::<Vec<_>>()
            .join(",")
    );
    let inspection = inspect_temp_source("json", source, inspect_json_file);

    assert!(inspection.timestamps.is_empty());
    assert_eq!(
        inspection.skipped.as_deref(),
        Some("Inspection stopped after the first 10000 response values.")
    );
}

#[test]
fn streaming_xml_inspection_reads_cdata_values() {
    let inspection = inspect_temp_source(
        "xml",
        r#"<root><createdAt><![CDATA[1787482800]]></createdAt></root>"#,
        inspect_xml_file,
    );

    assert_eq!(inspection.timestamps.len(), 1);
    assert_eq!(inspection.timestamps[0].path, "/root/createdAt");
}

#[test]
fn xml_inspection_detects_attribute_and_element_values() {
    let token = concat!(
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.",
        "eyJzdWIiOiIxMjMiLCJpYXQiOjE3ODc0ODI4MDAsImV4cCI6MTc4NzQ4NjQwMH0.",
        "signature"
    );
    let source = format!(
        r#"<response createdAt="1787482800"><accessToken>{token}</accessToken><item updated_at="1787482800123"/></response>"#
    );
    let inspection = inspect_response_body(source.as_bytes());

    assert_eq!(inspection.jwts.len(), 1);
    assert_eq!(inspection.jwts[0].path, "/response/accessToken");
    assert_eq!(inspection.timestamps.len(), 2);
    assert!(
        inspection
            .timestamps
            .iter()
            .any(|finding| finding.path == "/response/@createdAt")
    );
    assert!(
        inspection
            .timestamps
            .iter()
            .any(|finding| finding.path == "/response/item/@updated_at")
    );

    let ranges = inspection_value_ranges(&source, &inspection);
    assert_eq!(ranges.len(), 3);
    for range in ranges {
        let selected = &source[range.range];
        assert!(selected == token || selected.starts_with("1787482800"));
    }
}

#[test]
fn xml_inspection_paths_include_namespaces_and_repeated_sibling_indexes() {
    let source = r#"<n:response xmlns:n="urn:test"><n:item createdAt="1787482800"/><n:item createdAt="1787486400"/></n:response>"#;
    let inspection = inspect_response_body(source.as_bytes());
    let paths = inspection
        .timestamps
        .iter()
        .map(|finding| finding.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![
            "/n:response/n:item[1]/@createdAt",
            "/n:response/n:item[2]/@createdAt"
        ]
    );
}

#[test]
fn inspection_rejects_jwt_shaped_strings_that_do_not_decode() {
    let inspection = inspect_response_body(br#"{"token":"abc.def.ghi"}"#);
    assert!(inspection.jwts.is_empty());
}

#[test]
fn timestamp_inspection_uses_field_semantics() {
    let inspection = inspect_response_body(
        br#"{"createdAt":1787482800,"userId":1787482800,"updated_at":1787482800123}"#,
    );

    let paths: Vec<_> = inspection
        .timestamps
        .iter()
        .map(|timestamp| timestamp.path.as_str())
        .collect();
    assert!(paths.contains(&"createdAt"));
    assert!(paths.contains(&"updated_at"));
    assert!(!paths.contains(&"userId"));
    assert!(inspection.timestamps.iter().any(
        |timestamp| timestamp.path == "updated_at" && timestamp.timestamp.millisecond_precision
    ));
}

#[test]
fn inspection_skips_very_large_bodies() {
    let body = format!(
        r#"{{"createdAt":{},"padding":"{}"}}"#,
        1_787_482_800,
        "x".repeat(600 * 1024)
    );
    let inspection = inspect_response_body(body.as_bytes());

    assert!(inspection.timestamps.is_empty());
    assert!(inspection.skipped.is_some());
}

#[test]
fn inspection_text_keeps_jwt_times_close_to_claim_values() {
    let token = concat!(
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.",
        "eyJzdWIiOiIxMjMiLCJpYXQiOjE3ODc0ODI4MDAsImV4cCI6MTc4NzQ4NjQwMH0.",
        "signature"
    );
    let source = format!(r#"{{"accessToken":"{token}"}}"#);
    let report = inspection_text(&inspect_response_body(source.as_bytes()));

    assert!(report.contains("JWT [1]"));
    assert!(report.contains("exp: 1787486400  Local:"));
    assert!(report.contains("Signature not verified"));
}

#[test]
fn inspection_detail_text_renders_one_selected_finding() {
    let inspection = inspect_response_body(br#"{"createdAt":1787482800,"updatedAt":1787486400}"#);
    let detail = inspection_detail_text(&inspection, Some(InspectSelection::Timestamp(1)));

    assert!(detail.starts_with("Timestamp"));
    assert!(detail.contains("updatedAt"));
    assert!(detail.contains("Raw: 1787486400"));
    assert!(!detail.contains("createdAt"));
}

#[test]
fn inspection_ranges_follow_paths_not_duplicate_values() {
    let inspection = inspect_response_body(br#"{"createdAt":1787482800,"updatedAt":1787482800}"#);
    let pretty = serde_json::to_string_pretty(
        &serde_json::json!({"createdAt": 1787482800, "updatedAt": 1787482800}),
    )
    .unwrap();
    let ranges = inspection_value_ranges(&pretty, &inspection);

    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].selection, InspectSelection::Timestamp(0));
    assert_eq!(ranges[1].selection, InspectSelection::Timestamp(1));
    assert_eq!(
        inspection_selection_at_offset(&ranges, ranges[1].range.start + 2),
        Some(InspectSelection::Timestamp(1))
    );
}

#[test]
fn inspection_ranges_do_not_match_substrings_in_other_values() {
    let inspection = inspect_response_body(br#"{"createdAt":1787482800,"label":"1787482800"}"#);
    let pretty = serde_json::to_string_pretty(
        &serde_json::json!({"createdAt": 1787482800, "label": "1787482800"}),
    )
    .unwrap();
    let ranges = inspection_value_ranges(&pretty, &inspection);

    assert_eq!(ranges.len(), 1);
    let label_offset = pretty.find("\"1787482800\"").unwrap() + 1;
    assert_eq!(inspection_selection_at_offset(&ranges, label_offset), None);
}

#[test]
fn inspection_ranges_skip_duplicate_non_targets_before_targets() {
    let inspection = inspect_response_body(br#"{"label":"1787482800","createdAt":1787482800}"#);
    let pretty = serde_json::to_string_pretty(
        &serde_json::json!({"label": "1787482800", "createdAt": 1787482800}),
    )
    .unwrap();
    let ranges = inspection_value_ranges(&pretty, &inspection);
    let label_offset = pretty.find("\"1787482800\"").unwrap() + 1;
    let timestamp_offset = pretty.rfind("1787482800").unwrap() + 1;

    assert_eq!(ranges.len(), 1);
    assert_eq!(inspection_selection_at_offset(&ranges, label_offset), None);
    assert_eq!(
        inspection_selection_at_offset(&ranges, timestamp_offset),
        Some(InspectSelection::Timestamp(0))
    );
}
