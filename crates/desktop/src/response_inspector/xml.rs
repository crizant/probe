//! In-memory and streaming XML inspection.

use std::{fs::File, io::BufReader, ops::Range, path::Path};

use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};

use super::findings::{inspect_jwt_text, inspect_timestamp_text};
use super::{INSPECT_MAX_VALUES, ResponseInspection};

/// Inspects a complete XML response using a bounded event buffer.
pub(crate) fn inspect_xml_file(path: &Path) -> ResponseInspection {
    let Ok(file) = File::open(path) else {
        return ResponseInspection {
            skipped: Some("Could not read the retained response body.".to_owned()),
            ..ResponseInspection::default()
        };
    };
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut path = Vec::<String>::new();
    let mut inspection = ResponseInspection::default();
    let mut visited = 0_usize;
    let mut limit_reached = false;
    let mut invalid = false;
    let mut root_seen = false;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => {
                if path.is_empty() {
                    if root_seen {
                        invalid = true;
                    }
                    root_seen = true;
                }
                let name = element.name().as_ref().to_owned();
                path.push(name);
                match inspect_streaming_xml_attributes(
                    &element,
                    &path,
                    &mut inspection,
                    &mut visited,
                ) {
                    StreamingXmlStatus::Continue => {}
                    StreamingXmlStatus::LimitReached => limit_reached = true,
                    StreamingXmlStatus::Invalid => invalid = true,
                }
            }
            Ok(Event::Empty(element)) => {
                if path.is_empty() {
                    if root_seen {
                        invalid = true;
                    }
                    root_seen = true;
                }
                let name = element.name().as_ref().to_owned();
                path.push(name);
                match inspect_streaming_xml_attributes(
                    &element,
                    &path,
                    &mut inspection,
                    &mut visited,
                ) {
                    StreamingXmlStatus::Continue => {}
                    StreamingXmlStatus::LimitReached => limit_reached = true,
                    StreamingXmlStatus::Invalid => invalid = true,
                }
                path.pop();
            }
            Ok(Event::Text(text)) => {
                if path.is_empty() {
                    invalid = !text.xml10_content().trim().is_empty();
                } else {
                    match quick_xml::escape::unescape(&text.xml10_content()) {
                        Ok(value) => {
                            limit_reached = inspect_streaming_xml_value(
                                value.as_ref(),
                                &format!("/{}", path.join("/")),
                                path.last().map(String::as_str),
                                &mut inspection,
                                &mut visited,
                            );
                        }
                        Err(_) => invalid = true,
                    }
                }
            }
            Ok(Event::CData(text)) => {
                if path.is_empty() {
                    invalid = true;
                } else {
                    limit_reached = inspect_streaming_xml_value(
                        &text.xml10_content(),
                        &format!("/{}", path.join("/")),
                        path.last().map(String::as_str),
                        &mut inspection,
                        &mut visited,
                    );
                }
            }
            Ok(Event::End(_)) => invalid |= path.pop().is_none(),
            Ok(Event::Eof) => {
                invalid |= !root_seen || !path.is_empty();
                break;
            }
            Ok(Event::GeneralRef(_)) if path.is_empty() => invalid = true,
            Err(_) => {
                invalid = true;
                break;
            }
            _ => {}
        }
        if invalid || limit_reached {
            break;
        }
        buffer.clear();
    }
    if invalid {
        return ResponseInspection {
            skipped: Some("Response is not valid XML.".to_owned()),
            ..ResponseInspection::default()
        };
    }
    if limit_reached {
        inspection.skipped =
            Some("Inspection stopped after the first 10000 response values.".to_owned());
    }
    inspection
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamingXmlStatus {
    Continue,
    LimitReached,
    Invalid,
}

fn inspect_streaming_xml_attributes(
    element: &BytesStart<'_>,
    path: &[String],
    inspection: &mut ResponseInspection,
    visited: &mut usize,
) -> StreamingXmlStatus {
    for attribute in element.attributes() {
        let Ok(attribute) = attribute else {
            return StreamingXmlStatus::Invalid;
        };
        let name = attribute.key.as_ref().to_owned();
        let Ok(value) = attribute.normalized_value(XmlVersion::Implicit1_0) else {
            return StreamingXmlStatus::Invalid;
        };
        if inspect_streaming_xml_value(
            value.as_ref(),
            &format!("/{}/@{name}", path.join("/")),
            Some(&name),
            inspection,
            visited,
        ) {
            return StreamingXmlStatus::LimitReached;
        }
    }
    StreamingXmlStatus::Continue
}

fn inspect_streaming_xml_value(
    value: &str,
    path: &str,
    key: Option<&str>,
    inspection: &mut ResponseInspection,
    visited: &mut usize,
) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if *visited >= INSPECT_MAX_VALUES {
        return true;
    }
    *visited += 1;
    if let Some(finding) = inspect_jwt_text(value, path) {
        inspection.jwts.push(finding);
    } else if let Some(finding) = inspect_timestamp_text(value, path, key, false) {
        inspection.timestamps.push(finding);
    }
    false
}

pub(super) fn inspect_xml_response(source: &str) -> ResponseInspection {
    let Ok(document) = roxmltree::Document::parse(source) else {
        return ResponseInspection::default();
    };
    let mut inspector = XmlInspector {
        source,
        ..XmlInspector::default()
    };
    inspector.visit_element(document.root_element(), String::new());
    if inspector.visited >= INSPECT_MAX_VALUES {
        inspector.inspection.skipped =
            Some("Inspection stopped after the first 10000 response values.".to_owned());
    }
    inspector.inspection
}

#[derive(Default)]
struct XmlInspector<'a> {
    source: &'a str,
    inspection: ResponseInspection,
    visited: usize,
}

impl XmlInspector<'_> {
    fn visit_element(&mut self, element: roxmltree::Node<'_, '_>, parent_path: String) {
        if self.visited >= INSPECT_MAX_VALUES {
            return;
        }
        let name = xml_element_name(element);
        let segment = xml_element_segment(element, &name);
        let path = if parent_path.is_empty() {
            format!("/{segment}")
        } else {
            format!("{parent_path}/{segment}")
        };

        for attribute in element.attributes() {
            if self.visited >= INSPECT_MAX_VALUES {
                return;
            }
            let attribute_name = xml_attribute_name(element, attribute);
            self.inspect_scalar(
                attribute.value(),
                format!("{path}/@{attribute_name}"),
                Some(attribute.name()),
                attribute.range_value(),
            );
        }

        let text_count = element.children().filter(|child| child.is_text()).count();
        let mut text_index = 0;
        for child in element.children() {
            if self.visited >= INSPECT_MAX_VALUES {
                return;
            }
            if child.is_element() {
                self.visit_element(child, path.clone());
            } else if child.is_text() {
                text_index += 1;
                let text_path = if text_count == 1 {
                    path.clone()
                } else {
                    format!("{path}/text()[{text_index}]")
                };
                self.inspect_scalar(
                    child.text().unwrap_or_default(),
                    text_path,
                    Some(element.tag_name().name()),
                    child.range(),
                );
            }
        }
    }

    fn inspect_scalar(
        &mut self,
        value: &str,
        path: String,
        key: Option<&str>,
        source_range: Range<usize>,
    ) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        self.visited += 1;
        let source_range = xml_scalar_source_range(self.source, source_range, value);
        if let Some(mut finding) = inspect_jwt_text(value, &path) {
            finding.source_range = Some(source_range);
            self.inspection.jwts.push(finding);
        } else if let Some(mut finding) = inspect_timestamp_text(value, &path, key, false) {
            finding.source_range = Some(source_range);
            self.inspection.timestamps.push(finding);
        }
    }
}

fn xml_element_name(element: roxmltree::Node<'_, '_>) -> String {
    let tag = element.tag_name();
    tag.namespace()
        .and_then(|namespace| element.lookup_prefix(namespace))
        .map(|prefix| format!("{prefix}:{}", tag.name()))
        .unwrap_or_else(|| tag.name().to_owned())
}

fn xml_attribute_name(
    element: roxmltree::Node<'_, '_>,
    attribute: roxmltree::Attribute<'_, '_>,
) -> String {
    attribute
        .namespace()
        .and_then(|namespace| element.lookup_prefix(namespace))
        .map(|prefix| format!("{prefix}:{}", attribute.name()))
        .unwrap_or_else(|| attribute.name().to_owned())
}

fn xml_element_segment(element: roxmltree::Node<'_, '_>, name: &str) -> String {
    let Some(parent) = element.parent().filter(|parent| parent.is_element()) else {
        return name.to_owned();
    };
    let tag = element.tag_name();
    let same_name = |sibling: &roxmltree::Node<'_, '_>| {
        sibling.is_element()
            && sibling.tag_name().name() == tag.name()
            && sibling.tag_name().namespace() == tag.namespace()
    };
    let count = parent.children().filter(same_name).count();
    if count <= 1 {
        return name.to_owned();
    }
    let index = parent
        .children()
        .take_while(|sibling| *sibling != element)
        .filter(same_name)
        .count()
        + 1;
    format!("{name}[{index}]")
}

fn xml_scalar_source_range(source: &str, range: Range<usize>, value: &str) -> Range<usize> {
    source
        .get(range.clone())
        .and_then(|raw| raw.find(value))
        .map(|offset| range.start + offset..range.start + offset + value.len())
        .unwrap_or(range)
}
