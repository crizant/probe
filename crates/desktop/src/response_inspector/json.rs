//! In-memory and streaming JSON inspection.

use std::{fmt, fs::File, io::BufReader, path::Path};

use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};

use super::findings::{inspect_jwt, inspect_jwt_text, inspect_timestamp, inspect_timestamp_text};
use super::{INSPECT_MAX_VALUES, InspectionRange, InspectionTargets, ResponseInspection};

const INSPECTION_LIMIT_REACHED: &str = "probe inspection value limit reached";

/// Inspects a complete JSON response without retaining its document tree.
pub(crate) fn inspect_json_file(path: &Path) -> ResponseInspection {
    let Ok(file) = File::open(path) else {
        return ResponseInspection {
            skipped: Some("Could not read the retained response body.".to_owned()),
            ..ResponseInspection::default()
        };
    };
    let mut inspector = StreamingJsonInspector::default();
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    let result = StreamingJsonSeed {
        inspector: &mut inspector,
        path: Vec::new(),
        key: None,
    }
    .deserialize(&mut deserializer);
    if inspector.limit_reached {
        inspector.inspection.skipped =
            Some("Inspection stopped after the first 10000 response values.".to_owned());
    } else if result.is_err() || deserializer.end().is_err() {
        return ResponseInspection {
            skipped: Some("Response is not valid JSON.".to_owned()),
            ..ResponseInspection::default()
        };
    }
    inspector.inspection
}

pub(super) fn inspect_json_bytes(body: &[u8]) -> Option<ResponseInspection> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let mut inspector = JsonInspector::default();
    inspector.visit(&value, &mut Vec::new(), None);
    if inspector.visited >= INSPECT_MAX_VALUES {
        inspector.inspection.skipped =
            Some("Inspection stopped after the first 10000 response values.".to_owned());
    }
    Some(inspector.inspection)
}

pub(super) fn collect_value_ranges(
    value: &serde_json::Value,
    pretty_text: &str,
    targets: &InspectionTargets,
    ranges: &mut Vec<InspectionRange>,
) {
    walk_value_ranges(value, pretty_text, targets, &mut Vec::new(), &mut 0, ranges);
}

fn walk_value_ranges(
    value: &serde_json::Value,
    pretty_text: &str,
    targets: &InspectionTargets,
    path: &mut Vec<PathSegment>,
    cursor: &mut usize,
    ranges: &mut Vec<InspectionRange>,
) {
    match value {
        serde_json::Value::Object(object) => {
            for (name, child) in object {
                path.push(PathSegment::Key(name.clone()));
                walk_value_ranges(child, pretty_text, targets, path, cursor, ranges);
                path.pop();
            }
        }
        serde_json::Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                path.push(PathSegment::Index(index));
                walk_value_ranges(child, pretty_text, targets, path, cursor, ranges);
                path.pop();
            }
        }
        _ => {
            let path_text = json_path(path);
            let Ok(rendered) = serde_json::to_string_pretty(value) else {
                return;
            };
            let Some(relative_start) = pretty_text[*cursor..].find(&rendered) else {
                return;
            };
            let start = *cursor + relative_start;
            let end = start + rendered.len();
            *cursor = end;
            if let Some(selection) = targets.selection_for_path(&path_text) {
                ranges.push(InspectionRange {
                    range: start..end,
                    selection,
                });
            }
        }
    }
}

#[derive(Default)]
struct JsonInspector {
    inspection: ResponseInspection,
    visited: usize,
}

impl JsonInspector {
    fn visit(&mut self, value: &serde_json::Value, path: &mut Vec<PathSegment>, key: Option<&str>) {
        if self.visited >= INSPECT_MAX_VALUES {
            return;
        }
        self.visited += 1;

        let path_text = json_path(path);
        if let Some(finding) = inspect_jwt(value, &path_text) {
            self.inspection.jwts.push(finding);
        } else if let Some(finding) = inspect_timestamp(value, &path_text, key, false) {
            self.inspection.timestamps.push(finding);
        }

        match value {
            serde_json::Value::Object(object) => {
                for (name, child) in object {
                    path.push(PathSegment::Key(name.clone()));
                    self.visit(child, path, Some(name));
                    path.pop();
                }
            }
            serde_json::Value::Array(array) => {
                for (index, child) in array.iter().enumerate() {
                    path.push(PathSegment::Index(index));
                    self.visit(child, path, key);
                    path.pop();
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
enum PathSegment {
    Key(String),
    Index(usize),
}

#[derive(Default)]
struct StreamingJsonInspector {
    inspection: ResponseInspection,
    visited: usize,
    limit_reached: bool,
}

impl StreamingJsonInspector {
    fn string(&mut self, value: &str, path: &[PathSegment], key: Option<&str>) {
        if self.visited >= INSPECT_MAX_VALUES {
            return;
        }
        self.visited += 1;
        let path = json_path(path);
        if let Some(finding) = inspect_jwt_text(value, &path) {
            self.inspection.jwts.push(finding);
        } else if let Some(finding) = inspect_timestamp_text(value, &path, key, false) {
            self.inspection.timestamps.push(finding);
        }
    }

    fn scalar(&mut self, value: serde_json::Value, path: &[PathSegment], key: Option<&str>) {
        if self.visited >= INSPECT_MAX_VALUES {
            return;
        }
        self.visited += 1;
        let path = json_path(path);
        if let Some(finding) = inspect_jwt(&value, &path) {
            self.inspection.jwts.push(finding);
        } else if let Some(finding) = inspect_timestamp(&value, &path, key, false) {
            self.inspection.timestamps.push(finding);
        }
    }
}

struct StreamingJsonSeed<'a> {
    inspector: &'a mut StreamingJsonInspector,
    path: Vec<PathSegment>,
    key: Option<String>,
}

impl<'de> DeserializeSeed<'de> for StreamingJsonSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.inspector.visited >= INSPECT_MAX_VALUES {
            self.inspector.limit_reached = true;
            return Err(D::Error::custom(INSPECTION_LIMIT_REACHED));
        }
        deserializer.deserialize_any(StreamingJsonVisitor(self))
    }
}

struct StreamingJsonVisitor<'a>(StreamingJsonSeed<'a>);

impl<'de> Visitor<'de> for StreamingJsonVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_map<A>(self, mut map: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        self.0.inspector.visited = self.0.inspector.visited.saturating_add(1);
        while let Some(key) = map.next_key::<String>()? {
            let mut path = self.0.path.clone();
            path.push(PathSegment::Key(key.clone()));
            map.next_value_seed(StreamingJsonSeed {
                inspector: self.0.inspector,
                path,
                key: Some(key),
            })?;
        }
        Ok(())
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        self.0.inspector.visited = self.0.inspector.visited.saturating_add(1);
        let mut index = 0;
        loop {
            let mut path = self.0.path.clone();
            path.push(PathSegment::Index(index));
            if sequence
                .next_element_seed(StreamingJsonSeed {
                    inspector: self.0.inspector,
                    path,
                    key: self.0.key.clone(),
                })?
                .is_none()
            {
                break;
            }
            index += 1;
        }
        Ok(())
    }

    fn visit_str<E>(self, value: &str) -> Result<(), E> {
        self.0
            .inspector
            .string(value, &self.0.path, self.0.key.as_deref());
        Ok(())
    }

    fn visit_string<E>(self, value: String) -> Result<(), E>
    where
        E: serde::de::Error,
    {
        self.visit_str(&value)
    }

    fn visit_i64<E>(self, value: i64) -> Result<(), E> {
        self.0.inspector.scalar(
            serde_json::Value::Number(value.into()),
            &self.0.path,
            self.0.key.as_deref(),
        );
        Ok(())
    }

    fn visit_u64<E>(self, value: u64) -> Result<(), E> {
        self.0.inspector.scalar(
            serde_json::Value::Number(value.into()),
            &self.0.path,
            self.0.key.as_deref(),
        );
        Ok(())
    }

    fn visit_f64<E>(self, value: f64) -> Result<(), E> {
        if let Some(number) = serde_json::Number::from_f64(value) {
            self.0.inspector.scalar(
                serde_json::Value::Number(number),
                &self.0.path,
                self.0.key.as_deref(),
            );
        }
        Ok(())
    }

    fn visit_bool<E>(self, value: bool) -> Result<(), E> {
        self.0.inspector.scalar(
            serde_json::Value::Bool(value),
            &self.0.path,
            self.0.key.as_deref(),
        );
        Ok(())
    }

    fn visit_none<E>(self) -> Result<(), E>
    where
        E: serde::de::Error,
    {
        self.visit_unit()
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        self.0
            .inspector
            .scalar(serde_json::Value::Null, &self.0.path, self.0.key.as_deref());
        Ok(())
    }
}

fn json_path(path: &[PathSegment]) -> String {
    let mut text = String::new();
    for segment in path {
        match segment {
            PathSegment::Key(key) => {
                if text.is_empty() {
                    text.push_str(key);
                } else {
                    text.push('.');
                    text.push_str(key);
                }
            }
            PathSegment::Index(index) => {
                text.push('[');
                text.push_str(&index.to_string());
                text.push(']');
            }
        }
    }
    if text.is_empty() {
        "$".to_owned()
    } else {
        text
    }
}
