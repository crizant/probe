//! Response document preparation, pretty-printing, and image detection.

use std::sync::Arc;

use gpui::{Image, ImageFormat, SharedString};
use quick_xml::{Reader, events::Event, writer::Writer};

use super::{PreparedDocument, ResponseBodySyntax, ResponseImagePreview, SYNC_PRETTY_BYTES};
use crate::response_inspector::{
    INSPECT_MAX_BYTES, ResponseInspection, first_inspection_selection, inspect_response_body,
    inspection_value_ranges,
};
use probe_http::HttpResponse;

#[derive(Clone, Debug, Eq, PartialEq)]
enum DetectedImage {
    Supported(ImageFormat),
    UnsupportedMediaType(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PrettyBody {
    pub text: String,
    pub notice: Option<String>,
}

fn document_from_response(response: &HttpResponse, generation: u64) -> PreparedDocument {
    PreparedDocument {
        generation,
        raw_text: SharedString::default(),
        pretty_text: String::new(),
        pretty_pending: false,
        pretty_notice: None,
        page_body: Vec::new(),
        base64_text: String::new(),
        base64_pending: false,
        hex_text: String::new(),
        hex_pending: false,
        image_preview: None,
        syntax: ResponseBodySyntax::Plain,
        binary: false,
        file_backed: response.body_file.is_some(),
        truncated: !response.body_complete,
        retention_notice: response.body_retention_error.clone(),
        page_offset: 0,
        page_len: response.body.len(),
        page_revision: 0,
        total_size: response.size,
        page_pending: false,
        headers: response.headers.clone(),
        inspection: ResponseInspection::default(),
        inspection_pending: false,
        inspection_ranges: Vec::new(),
        inspection_selection: None,
    }
}

pub(crate) fn prepare_document(
    response: &HttpResponse,
    generation: u64,
) -> (PreparedDocument, bool, bool) {
    if let Some(detected) = detect_response_image(response) {
        let mut document = document_from_response(response, generation);
        document.image_preview = Some(if !response.body_complete {
            ResponseImagePreview::Unavailable(
                "The complete image response is unavailable for preview.".to_owned(),
            )
        } else {
            match detected {
                DetectedImage::Supported(format) => ResponseImagePreview::Ready(Arc::new(
                    Image::from_bytes(format, response.body.clone()),
                )),
                DetectedImage::UnsupportedMediaType(media_type) => {
                    ResponseImagePreview::Unavailable(format!(
                        "The {media_type} image format cannot be previewed."
                    ))
                }
            }
        });
        document.binary = body_is_binary(&response.body, document.file_backed);
        if document.binary {
            if !matches!(
                document.image_preview.as_ref(),
                Some(ResponseImagePreview::Ready(_))
            ) {
                document.page_body = response.body.clone();
            }
        } else {
            document.raw_text = String::from_utf8_lossy(&response.body).into_owned().into();
        }
        return (document, false, false);
    }
    if response.body.is_empty() {
        return (document_from_response(response, generation), false, false);
    }
    let file_backed = response.body_file.is_some();
    if body_is_binary(&response.body, file_backed) {
        let mut document = document_from_response(response, generation);
        document.pretty_notice = Some(format!("Binary response body ({} bytes).", response.size));
        document.page_body = response.body.clone();
        document.binary = true;
        return (document, false, false);
    }

    let raw_text = String::from_utf8_lossy(&response.body).into_owned();
    let syntax = response_body_syntax(response);
    let truncated = !response.body_complete;
    let pretty_candidate = matches!(syntax, ResponseBodySyntax::Json | ResponseBodySyntax::Xml);
    let inspection_candidate = pretty_candidate;
    let pretty_pending = !truncated && pretty_candidate && response.body.len() > SYNC_PRETTY_BYTES;
    let inspection_pending = (file_backed && inspection_candidate)
        || (!truncated && inspection_candidate && response.body.len() <= INSPECT_MAX_BYTES);
    let inspection = if truncated && !file_backed {
        ResponseInspection {
            skipped: Some(
                response
                    .body_retention_error
                    .clone()
                    .unwrap_or_else(|| "The complete response body was not retained.".to_owned()),
            ),
            ..ResponseInspection::default()
        }
    } else if !inspection_candidate || inspection_pending {
        ResponseInspection::default()
    } else {
        inspect_response_body(&response.body)
    };
    let (pretty_text, pretty_notice, pretty_pending, inspection) = if truncated {
        (String::new(), None, false, inspection)
    } else if pretty_pending {
        let notice = match syntax {
            ResponseBodySyntax::Json => "Formatting JSON…",
            ResponseBodySyntax::Xml => "Formatting XML…",
            ResponseBodySyntax::Plain => "Formatting…",
        };
        (raw_text.clone(), Some(notice.to_owned()), true, inspection)
    } else if syntax == ResponseBodySyntax::Json {
        let pretty = pretty_json_body(&response.body);
        (pretty.text, pretty.notice, false, inspection)
    } else if syntax == ResponseBodySyntax::Xml {
        let pretty = pretty_xml_body(&response.body);
        let inspection = if pretty.notice.is_none() {
            inspect_response_body(pretty.text.as_bytes())
        } else {
            inspection
        };
        (pretty.text, pretty.notice, false, inspection)
    } else {
        (
            raw_text.clone(),
            Some("Pretty formatting is available for JSON and XML responses.".to_owned()),
            false,
            inspection,
        )
    };
    let inspection_ranges = inspection_value_ranges(&pretty_text, &inspection);
    let inspection_selection = first_inspection_selection(&inspection);

    let mut document = document_from_response(response, generation);
    document.raw_text = raw_text.into();
    document.pretty_text = pretty_text;
    document.pretty_pending = pretty_pending;
    document.pretty_notice = pretty_notice;
    document.syntax = syntax;
    document.inspection = inspection;
    document.inspection_pending = inspection_pending;
    document.inspection_ranges = inspection_ranges;
    document.inspection_selection = inspection_selection;
    (document, pretty_pending, inspection_pending)
}

pub(super) fn body_is_binary(body: &[u8], file_backed: bool) -> bool {
    match std::str::from_utf8(body) {
        Ok(_) => false,
        // A bounded prefix can end partway through an otherwise valid UTF-8
        // scalar. Invalid bytes before the end still identify a binary body.
        Err(error) => !file_backed || error.error_len().is_some(),
    }
}

pub(crate) fn pretty_body(body: &[u8], syntax: ResponseBodySyntax) -> PrettyBody {
    match syntax {
        ResponseBodySyntax::Json => pretty_json_body(body),
        ResponseBodySyntax::Xml => pretty_xml_body(body),
        ResponseBodySyntax::Plain => PrettyBody {
            text: String::from_utf8_lossy(body).into_owned(),
            notice: None,
        },
    }
}

pub(crate) fn pretty_json_body(body: &[u8]) -> PrettyBody {
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(pretty) => PrettyBody {
                text: pretty,
                notice: None,
            },
            Err(_) => PrettyBody {
                text: String::from_utf8_lossy(body).into_owned(),
                notice: Some("Could not pretty-print this JSON response.".to_owned()),
            },
        },
        Err(_) => PrettyBody {
            text: String::from_utf8_lossy(body).into_owned(),
            notice: Some("Response is not valid JSON.".to_owned()),
        },
    }
}

pub(crate) fn pretty_xml_body(body: &[u8]) -> PrettyBody {
    let source = match std::str::from_utf8(body) {
        Ok(source) => source,
        Err(_) => {
            return PrettyBody {
                text: String::from_utf8_lossy(body).into_owned(),
                notice: Some("Could not pretty-print this XML response.".to_owned()),
            };
        }
    };
    match pretty_xml_text(source) {
        Ok(pretty) => PrettyBody {
            text: pretty,
            notice: None,
        },
        Err(PrettyXmlError::Invalid) => PrettyBody {
            text: source.to_owned(),
            notice: Some("Response is not valid XML.".to_owned()),
        },
        Err(PrettyXmlError::Write) => PrettyBody {
            text: source.to_owned(),
            notice: Some("Could not pretty-print this XML response.".to_owned()),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrettyXmlError {
    Invalid,
    Write,
}

fn pretty_xml_text(source: &str) -> Result<String, PrettyXmlError> {
    let mut reader = Reader::from_str(source);
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Eof) => break,
            Ok(event) => {
                writer
                    .write_event(event.borrow())
                    .map_err(|_| PrettyXmlError::Write)?;
            }
            Err(_) => return Err(PrettyXmlError::Invalid),
        }
        buffer.clear();
    }
    String::from_utf8(writer.into_inner()).map_err(|_| PrettyXmlError::Write)
}

pub(crate) fn response_body_syntax(response: &HttpResponse) -> ResponseBodySyntax {
    if let Some(content_type) = content_type(response)
        && content_type.to_ascii_lowercase().contains("json")
    {
        return ResponseBodySyntax::Json;
    }
    if let Some(content_type) = content_type(response)
        && content_type.to_ascii_lowercase().contains("xml")
    {
        return ResponseBodySyntax::Xml;
    }
    let trimmed = trim_ascii_start(&response.body);
    if matches!(trimmed.first(), Some(b'{' | b'[')) {
        ResponseBodySyntax::Json
    } else if trimmed.first() == Some(&b'<') {
        ResponseBodySyntax::Xml
    } else {
        ResponseBodySyntax::Plain
    }
}

fn content_type(response: &HttpResponse) -> Option<&str> {
    response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
}

fn detect_response_image(response: &HttpResponse) -> Option<DetectedImage> {
    let media_type = content_type(response)?
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if media_type.starts_with("image/") {
        return Some(ImageFormat::from_mime_type(&media_type).map_or_else(
            || DetectedImage::UnsupportedMediaType(media_type),
            DetectedImage::Supported,
        ));
    }
    if media_type == "application/octet-stream" {
        sniff_image_format(&response.body).map(DetectedImage::Supported)
    } else {
        None
    }
}

fn sniff_image_format(body: &[u8]) -> Option<ImageFormat> {
    if body.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if body.starts_with(b"\xff\xd8\xff") {
        Some(ImageFormat::Jpeg)
    } else if body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if body.starts_with(b"GIF87a") || body.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if body.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else if body.starts_with(b"II*\0") || body.starts_with(b"MM\0*") {
        Some(ImageFormat::Tiff)
    } else if body.starts_with(b"\0\0\x01\0") {
        Some(ImageFormat::Ico)
    } else if body.len() >= 3
        && body[0] == b'P'
        && matches!(body[1], b'1'..=b'7')
        && body[2].is_ascii_whitespace()
    {
        Some(ImageFormat::Pnm)
    } else {
        None
    }
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let index = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[index..]
}
