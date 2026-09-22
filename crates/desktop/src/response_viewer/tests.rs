use std::time::Duration;

use gpui::ImageFormat;
use probe_http::{HttpResponse, ResponseHeader};

use super::prepare::{body_is_binary, pretty_json_body, pretty_xml_body, response_body_syntax};
use super::search::{join_header_lines, search_headers, search_text};
use super::{
    PageDirection, PreparedDocument, RESPONSE_PAGE_BYTES, RawBodyView, ResponseBodySyntax,
    ResponseImagePreview, ResponseViewerTab, encode_base64, encode_hex, prepare_document,
    pretty_body,
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
        body_file: None,
        body_retention_error: None,
    }
}

fn request_key() -> probe_core::RequestKey {
    let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
        items: vec![probe_core::CollectionItem::HttpRequest(
            probe_core::HttpRequest::default(),
        )],
        ..probe_core::Collection::default()
    });
    let probe_core::WorkspaceItemRef::Request(key) = workspace.root_items()[0] else {
        panic!("expected request key");
    };
    key
}

fn viewer_with(document: PreparedDocument) -> (probe_core::RequestKey, super::ResponseViewerState) {
    let key = request_key();
    let mut viewer = super::ResponseViewerState::default();
    viewer.insert(key, document);
    (key, viewer)
}

fn file_backed_document(body: &[u8], trailing: usize, content_type: &str) -> PreparedDocument {
    let mut large = response(body, content_type);
    large.size = body.len() + trailing;
    large.body_complete = false;
    let (mut document, _, _) = prepare_document(&large, 7);
    document.file_backed = true;
    document
}

const BINARY_BODY: &[u8] = &[0, 159, 146, 150];

#[test]
fn pretty_json_indents_object_fields() {
    let pretty = pretty_json_body(br#"{"ok":true,"n":1}"#);
    assert!(pretty.notice.is_none());
    let text = pretty.text;
    assert!(text.contains('\n'));
    assert!(text.contains("\"ok\""));
}

#[test]
fn long_lines_are_preserved_for_the_virtualized_editor() {
    let line = "x".repeat(1_000);
    let response = response(line.as_bytes(), "text/plain");
    let (document, pending, inspection_pending) = prepare_document(&response, 1);
    assert!(!pending);
    assert!(!inspection_pending);
    assert_eq!(document.raw_text.as_ref(), line);
}

#[test]
fn search_is_case_insensitive_and_records_byte_ranges() {
    let text = "Alpha\nbeta ALPHA";
    let matches = search_text(text, "alpha");
    assert_eq!(matches.len(), 2);
    assert_eq!(&text[matches[0].range.clone()], "Alpha");
    assert_eq!(&text[matches[1].range.clone()], "ALPHA");
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
    let joined = join_header_lines(&headers);
    assert_eq!(&joined.text[matches[0].range.clone()], "json");
}

#[test]
fn join_header_lines_keeps_name_and_value_offsets() {
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
    let joined = join_header_lines(&headers);
    assert_eq!(
        joined.text,
        "content-type: application/json\nx-request-id: abc"
    );
    assert_eq!(
        &joined.text[joined.line_offsets[0]..joined.line_offsets[0] + joined.name_lens[0]],
        "content-type"
    );
    let value_start = joined.line_offsets[1] + joined.name_lens[1] + 2;
    assert_eq!(&joined.text[value_start..], "abc");
}

#[test]
fn binary_and_json_sniffing_prepare_the_expected_document() {
    let json = response(br#"{"ok":true}"#, "application/json");
    assert_eq!(response_body_syntax(&json), ResponseBodySyntax::Json);
    let (document, pending, inspection_pending) = prepare_document(&json, 1);
    assert!(!pending);
    assert!(inspection_pending);
    assert!(!document.binary);
    assert!(document.pretty_notice.is_none());
    assert!(document.pretty_text.contains('\n'));

    let binary = response(BINARY_BODY, "application/octet-stream");
    let (document, pending, inspection_pending) = prepare_document(&binary, 2);
    assert!(!pending);
    assert!(!inspection_pending);
    assert!(document.binary);
    assert!(document.raw_text.is_empty());
    assert_eq!(document.page_body, BINARY_BODY);
}

#[test]
fn image_content_type_prepares_a_supported_preview() {
    let image = response(BINARY_BODY, " Image/PNG ; charset=binary");
    let (document, pretty_pending, inspection_pending) = prepare_document(&image, 3);

    assert!(!pretty_pending);
    assert!(!inspection_pending);
    assert!(document.is_image());
    assert!(document.binary);
    assert!(matches!(
        document.image_preview,
        Some(ResponseImagePreview::Ready(ref image)) if image.format() == ImageFormat::Png
    ));
    assert!(document.page_body.is_empty());

    let (key, mut viewer) = viewer_with(document);
    viewer.ensure_available_tab(key);
    viewer.show_raw_base64(key);
    assert_eq!(viewer.visible_text(key), encode_base64(BINARY_BODY));
}

#[test]
fn unsupported_image_content_type_still_uses_preview_presentation() {
    let image = response(BINARY_BODY, "image/avif");
    let (document, pretty_pending, inspection_pending) = prepare_document(&image, 4);

    assert!(!pretty_pending);
    assert!(!inspection_pending);
    assert!(document.is_image());
    assert!(matches!(
        document.image_preview,
        Some(ResponseImagePreview::Unavailable(ref message))
            if message == "The image/avif image format cannot be previewed."
    ));
}

#[test]
fn generic_binary_content_type_sniffs_supported_image_signatures() {
    let cases: &[(&[u8], ImageFormat)] = &[
        (b"\x89PNG\r\n\x1a\nrest", ImageFormat::Png),
        (b"\xff\xd8\xffrest", ImageFormat::Jpeg),
        (b"RIFF\x04\0\0\0WEBPrest", ImageFormat::Webp),
        (b"GIF87arest", ImageFormat::Gif),
        (b"GIF89arest", ImageFormat::Gif),
        (b"BMrest", ImageFormat::Bmp),
        (b"II*\0rest", ImageFormat::Tiff),
        (b"MM\0*rest", ImageFormat::Tiff),
        (b"\0\0\x01\0rest", ImageFormat::Ico),
        (b"P6\nrest", ImageFormat::Pnm),
    ];

    for (body, expected) in cases {
        let response = response(body, "Application/Octet-Stream; charset=binary");
        let (document, pretty_pending, inspection_pending) = prepare_document(&response, 5);
        assert!(!pretty_pending);
        assert!(!inspection_pending);
        assert!(matches!(
            document.image_preview,
            Some(ResponseImagePreview::Ready(ref image)) if image.format() == *expected
        ));
    }
}

#[test]
fn magic_bytes_do_not_override_an_explicit_non_image_content_type() {
    let response = response(b"\x89PNG\r\n\x1a\nrest", "text/plain");
    let (document, _, _) = prepare_document(&response, 6);

    assert!(!document.is_image());
}

#[test]
fn encode_base64_matches_rfc4648_and_wraps_at_76_columns() {
    let cases: &[(&[u8], &str)] = &[
        (b"", ""),
        (b"f", "Zg=="),
        (b"fo", "Zm8="),
        (b"foo", "Zm9v"),
        (b"foob", "Zm9vYg=="),
        (b"fooba", "Zm9vYmE="),
        (b"foobar", "Zm9vYmFy"),
        (BINARY_BODY, "AJ+Slg=="),
    ];
    for (input, expected) in cases {
        assert_eq!(encode_base64(input), *expected, "input={input:?}");
    }

    let wrapped = encode_base64(&[b'a'; 60]);
    let lines: Vec<&str> = wrapped.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].len(), 76);
    assert!(lines[1].len() < 76);
    assert!(!wrapped.ends_with('\n'));
}

#[test]
fn encode_hex_produces_classic_hex_dump_format() {
    let empty = encode_hex(b"", 0);
    assert_eq!(empty, "");

    let single = encode_hex(b"A", 0);
    assert_eq!(
        single,
        "00000000  41                                                |A|"
    );

    let short = encode_hex(b"Hello", 0);
    assert_eq!(
        short,
        "00000000  48 65 6c 6c 6f                                    |Hello|"
    );

    let sixteen = encode_hex(b"0123456789abcdef", 0);
    assert_eq!(
        sixteen,
        "00000000  30 31 32 33 34 35 36 37  38 39 61 62 63 64 65 66  |0123456789abcdef|"
    );

    let multiline = encode_hex(b"0123456789abcdef0123456789", 0);
    let lines: Vec<&str> = multiline.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("00000000"));
    assert!(lines[1].starts_with("00000010"));
    assert!(lines[0].ends_with("|0123456789abcdef|"));
    assert!(lines[1].ends_with("|0123456789|"));

    let binary = encode_hex(BINARY_BODY, 0);
    assert!(binary.contains("00 9f 92 96"));
    assert!(binary.ends_with("|....|"));
}

#[test]
fn encode_hex_with_non_zero_base_offset() {
    let page_offset = 0x1000;
    let hex = encode_hex(b"Test", page_offset);
    assert!(hex.starts_with("00001000"));

    let multiline = encode_hex(b"0123456789abcdef0123456789", page_offset);
    let lines: Vec<&str> = multiline.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("00001000"));
    assert!(lines[1].starts_with("00001010"));

    let large_offset = 0xdeadbe00;
    let hex = encode_hex(b"Hello", large_offset);
    assert!(hex.starts_with("deadbe00"));
}

#[test]
fn raw_base64_view_encodes_the_response_body() {
    let json = br#"{"ok":true}"#;
    let (key, mut viewer) = viewer_with(prepare_document(&response(json, "application/json"), 1).0);
    viewer.show_raw_base64(key);
    assert_eq!(viewer.visible_text(key), encode_base64(json));

    viewer.insert(
        key,
        prepare_document(&response(BINARY_BODY, "application/octet-stream"), 2).0,
    );
    viewer.ensure_available_tab(key);
    assert_eq!(viewer.raw_view(), RawBodyView::Base64);
    viewer.show_raw_base64(key);
    assert_eq!(viewer.visible_text(key), encode_base64(BINARY_BODY));
}

#[test]
fn ensure_available_tab_switches_binary_from_text_to_hex() {
    let (key, mut viewer) =
        viewer_with(prepare_document(&response(BINARY_BODY, "application/octet-stream"), 1).0);
    assert_eq!(viewer.raw_view(), RawBodyView::Text);
    viewer.ensure_available_tab(key);
    assert_eq!(viewer.raw_view(), RawBodyView::Hex);
}

#[test]
fn ensure_available_tab_preserves_base64_for_binary() {
    let (key, mut viewer) =
        viewer_with(prepare_document(&response(BINARY_BODY, "application/octet-stream"), 1).0);
    viewer.set_raw_view(RawBodyView::Base64);
    assert_eq!(viewer.raw_view(), RawBodyView::Base64);
    viewer.ensure_available_tab(key);
    assert_eq!(viewer.raw_view(), RawBodyView::Base64);
}

#[test]
fn raw_hex_view_encodes_the_response_body() {
    let json = br#"{"ok":true}"#;
    let (key, mut viewer) = viewer_with(prepare_document(&response(json, "application/json"), 1).0);
    viewer.show_raw_hex(key);
    let hex = viewer.visible_text(key);
    assert!(hex.contains("7b 22 6f 6b 22"));
    assert!(hex.contains("{\"ok\":true}"));

    viewer.insert(
        key,
        prepare_document(&response(BINARY_BODY, "application/octet-stream"), 2).0,
    );
    viewer.ensure_available_tab(key);
    assert_eq!(viewer.raw_view(), RawBodyView::Hex);
    viewer.show_raw_hex(key);
    let hex = viewer.visible_text(key);
    assert!(hex.contains("00 9f 92 96"));
}

#[test]
fn paging_a_binary_body_replaces_bytes_and_invalidates_base64_and_hex() {
    let first_page = vec![0xFF; RESPONSE_PAGE_BYTES];
    let (key, mut viewer) = viewer_with(file_backed_document(
        &first_page,
        4,
        "application/octet-stream",
    ));
    viewer.ensure_available_tab(key);
    assert_eq!(viewer.tab(), ResponseViewerTab::Raw);
    assert_eq!(viewer.raw_view(), RawBodyView::Hex);
    assert!(viewer.take_hex_job(key).is_some());
    assert!(viewer.document(key).unwrap().hex_pending);

    let (generation, offset) = viewer.begin_page(key, PageDirection::Next).unwrap();
    viewer.apply_page(key, generation, offset, vec![1, 2, 3, 4]);

    let document = viewer.document(key).unwrap();
    assert_eq!(document.page_offset, RESPONSE_PAGE_BYTES);
    assert_eq!(document.page_body, [1, 2, 3, 4]);
    assert!(document.base64_text.is_empty());
    assert!(!document.base64_pending);
    assert!(document.hex_text.is_empty());
    assert!(!document.hex_pending);

    viewer.show_raw_base64(key);
    assert_eq!(viewer.visible_text(key), encode_base64(&[1, 2, 3, 4]));

    viewer.show_raw_hex(key);
    let hex = viewer.visible_text(key);
    assert!(hex.contains("01 02 03 04"));
    assert!(hex.starts_with(&format!("{:08x}", RESPONSE_PAGE_BYTES)));
}

#[test]
fn invalid_json_keeps_raw_text_and_explains_pretty_failure() {
    let response = response(b"{not json", "application/json");
    let (PreparedDocument { pretty_notice, .. }, pending, inspection_pending) =
        prepare_document(&response, 1);
    assert!(!pending);
    assert!(inspection_pending);
    assert_eq!(
        pretty_notice.as_deref(),
        Some("Response is not valid JSON.")
    );
}

#[test]
fn pretty_xml_indents_elements() {
    let source = r#"<?xml version="1.0"?><root id="1"><item/></root>"#;
    let pretty = pretty_xml_body(source.as_bytes());
    assert!(pretty.notice.is_none());
    assert!(pretty.text.contains('\n'));
    assert!(pretty.text.contains("<root"));
    assert!(pretty.text.contains("<item"));
}

#[test]
fn invalid_xml_keeps_raw_text_and_explains_pretty_failure() {
    let response = response(
        br#"<root createdAt="1787482800"><broken></root>"#,
        "application/xml",
    );
    let (PreparedDocument { pretty_notice, .. }, pending, inspection_pending) =
        prepare_document(&response, 1);
    assert!(!pending);
    assert!(inspection_pending);
    assert_eq!(pretty_notice.as_deref(), Some("Response is not valid XML."));
}

#[test]
fn pretty_body_dispatches_by_syntax() {
    let json = pretty_body(br#"{"ok":true}"#, ResponseBodySyntax::Json);
    assert!(json.notice.is_none());
    assert!(json.text.contains('\n'));

    let xml = pretty_body(br#"<root><item/></root>"#, ResponseBodySyntax::Xml);
    assert!(xml.notice.is_none());
    assert!(xml.text.contains('\n'));
}

#[test]
fn xml_responses_select_xml_highlighting_in_the_pretty_tab() {
    let source = r#"<?xml version="1.0"?><root id="1"><item/></root>"#;
    let xml = response(source.as_bytes(), "application/problem+xml; charset=utf-8");
    assert_eq!(response_body_syntax(&xml), ResponseBodySyntax::Xml);

    let (document, pending, inspection_pending) = prepare_document(&xml, 1);
    assert!(!pending);
    assert!(inspection_pending);
    assert_eq!(document.syntax.language(), "xml");
    assert_ne!(document.pretty_text, source);
    assert!(document.pretty_text.contains('\n'));
    assert!(document.pretty_notice.is_none());
}

#[test]
fn xml_is_sniffed_when_content_type_is_not_specific() {
    let xml = response(b" \n<root><item/></root>", "text/plain");
    assert_eq!(response_body_syntax(&xml), ResponseBodySyntax::Xml);
}

#[test]
fn raw_text_does_not_insert_line_breaks() {
    let source = r#"{"value":"abcdefghij"}"#;
    let response = response(source.as_bytes(), "application/json");
    let (document, pending, inspection_pending) = prepare_document(&response, 1);
    assert!(!pending);
    assert!(inspection_pending);
    assert_eq!(document.raw_text.as_ref(), source);
}

#[test]
fn file_backed_pages_replace_only_the_bounded_view() {
    let first_page = vec![b'x'; RESPONSE_PAGE_BYTES];
    let (key, mut viewer) = viewer_with(file_backed_document(&first_page, 4, "text/plain"));
    viewer.ensure_available_tab(key);
    assert_eq!(viewer.tab(), ResponseViewerTab::Raw);
    assert!(!viewer.document(key).unwrap().can_load_previous_page());
    assert!(viewer.document(key).unwrap().can_load_next_page());
    assert_eq!(
        ResponseViewerTab::TRUNCATED,
        [
            ResponseViewerTab::Raw,
            ResponseViewerTab::Headers,
            ResponseViewerTab::Inspect,
        ]
    );

    let (generation, offset) = viewer.begin_page(key, PageDirection::Next).unwrap();
    viewer.set_tab(ResponseViewerTab::Headers);
    viewer.apply_page(key, generation, offset, b"last".to_vec());

    let document = viewer.document(key).unwrap();
    assert_eq!(document.page_offset, RESPONSE_PAGE_BYTES);
    assert_eq!(document.raw_text.as_ref(), "last");
    assert!(document.pretty_text.is_empty());
    assert!(document.can_load_previous_page());
    assert!(!document.can_load_next_page());
    assert_eq!(viewer.tab(), ResponseViewerTab::Headers);
}

#[test]
fn an_incomplete_utf8_scalar_at_a_file_preview_boundary_is_not_binary() {
    assert!(!body_is_binary(b"text\xE2\x82", true));
    assert!(body_is_binary(b"text\xE2\x82", false));
    assert!(body_is_binary(b"text\xFF", true));
}

#[test]
fn an_unretained_large_response_exposes_only_the_raw_preview() {
    let mut large = response(br#"{"createdAt":1787482800}"#, "application/json");
    large.size = RESPONSE_PAGE_BYTES + 1;
    large.body_complete = false;
    large.body_retention_error = Some("Response cache quota reached.".to_owned());

    let (document, pretty_pending, inspection_pending) = prepare_document(&large, 9);

    assert!(document.truncated);
    assert!(!document.file_backed);
    assert!(document.pretty_text.is_empty());
    assert!(!pretty_pending);
    assert!(!inspection_pending);
    assert_eq!(
        document.inspection.skipped.as_deref(),
        Some("Response cache quota reached.")
    );
    assert!(!document.can_load_next_page());
}
