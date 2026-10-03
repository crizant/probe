//! JSON values for `request set` and `request create` field writes.

use std::collections::BTreeMap;

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, FieldPatch, FileReference,
    FormField, Header, MultipartPart, MultipartPartKind, MultipartValue, QueryParameter, RawBody,
    RawBodyKind,
};
use serde_json::{Map, Value};

use crate::CliError;

const BODY_TYPE: &str =
    "HTTP body type must be json, text, xml, sparql, form-urlencoded, multipart-form, or file";
const AUTHENTICATION_TYPE: &str = "authentication type must be inherit, basic, bearer, or apikey";
const AUTHENTICATION_VALUE: &str =
    "authentication must be a JSON object, the string \"inherit\", or null";

struct NamedList {
    list: &'static str,
    item: &'static str,
}

const HEADERS: NamedList = NamedList {
    list: "headers",
    item: "header",
};

const QUERY_PARAMETERS: NamedList = NamedList {
    list: "query parameters",
    item: "query parameter",
};

const PATH_PARAMETERS: NamedList = NamedList {
    list: "path parameters",
    item: "path parameter",
};

pub(crate) fn parse_headers(source: &str) -> Result<Vec<Header>, CliError> {
    parse_named_list(source, HEADERS).map(|entries| {
        entries
            .into_iter()
            .map(|(name, value, disabled)| Header {
                name,
                value,
                disabled,
            })
            .collect()
    })
}

pub(crate) fn parse_query_parameters(source: &str) -> Result<Vec<QueryParameter>, CliError> {
    parse_named_parameters(source, QUERY_PARAMETERS)
}

pub(crate) fn parse_path_parameters(source: &str) -> Result<Vec<QueryParameter>, CliError> {
    parse_named_parameters(source, PATH_PARAMETERS)
}

#[derive(Debug)]
pub(crate) enum HttpBodyWrite {
    Clear,
    Content(Body),
}

pub(crate) fn parse_http_body(source: &str) -> Result<HttpBodyWrite, CliError> {
    let value = parse_json(source, "HTTP body must be a JSON object or null")?;
    match value {
        Value::Null => Ok(HttpBodyWrite::Clear),
        Value::Object(object) => Ok(HttpBodyWrite::Content(parse_body_object(&object)?)),
        _ => Err(CliError::invalid_arguments(
            "HTTP body must be a JSON object or null",
        )),
    }
}

pub(crate) fn parse_authentication(source: &str) -> Result<FieldPatch<Authentication>, CliError> {
    let value = parse_json(source, AUTHENTICATION_VALUE)?;
    match value {
        Value::Null => Ok(FieldPatch::Clear),
        Value::String(kind) if kind == "inherit" => Ok(FieldPatch::Set(Authentication {
            kind: AuthenticationKind::Inherit,
            properties: BTreeMap::new(),
        })),
        Value::String(_) => Err(CliError::invalid_arguments(AUTHENTICATION_VALUE)),
        Value::Object(object) => Ok(FieldPatch::Set(parse_auth_object(&object)?)),
        _ => Err(CliError::invalid_arguments(AUTHENTICATION_VALUE)),
    }
}

fn parse_auth_object(object: &Map<String, Value>) -> Result<Authentication, CliError> {
    let kind_name = object.get("type").and_then(Value::as_str).ok_or_else(|| {
        CliError::invalid_arguments("authentication type must be a non-empty string")
    })?;
    if kind_name == "inherit" {
        return Err(CliError::invalid_arguments(
            "inherit authentication must be the JSON string \"inherit\"",
        ));
    }
    let (kind, properties) = match kind_name {
        "basic" => (
            AuthenticationKind::Basic,
            string_properties(object, &["username", "password"], "basic authentication")?,
        ),
        "bearer" => (
            AuthenticationKind::Bearer,
            string_properties(object, &["token"], "bearer authentication")?,
        ),
        "apikey" => (AuthenticationKind::ApiKey, api_key_properties(object)?),
        _ => return Err(CliError::invalid_arguments(AUTHENTICATION_TYPE)),
    };
    Ok(Authentication { kind, properties })
}

fn string_properties(
    object: &Map<String, Value>,
    fields: &[&str],
    label: &str,
) -> Result<BTreeMap<String, AuthenticationValue>, CliError> {
    let mut allowed = vec!["type"];
    allowed.extend_from_slice(fields);
    reject_unknown_fields(object, &allowed, label)?;
    let mut properties = BTreeMap::new();
    for field in fields {
        insert_string(&mut properties, object, field, label)?;
    }
    Ok(properties)
}

fn api_key_properties(
    object: &Map<String, Value>,
) -> Result<BTreeMap<String, AuthenticationValue>, CliError> {
    reject_unknown_fields(
        object,
        &["type", "key", "value", "placement"],
        "apikey authentication",
    )?;
    let mut properties = BTreeMap::new();
    insert_string(&mut properties, object, "key", "apikey authentication")?;
    insert_string(&mut properties, object, "value", "apikey authentication")?;
    insert_enum(
        &mut properties,
        object,
        "placement",
        &["header", "query"],
        "apikey authentication",
    )?;
    Ok(properties)
}

fn parse_named_parameters(source: &str, list: NamedList) -> Result<Vec<QueryParameter>, CliError> {
    parse_named_list(source, list).map(|entries| {
        entries
            .into_iter()
            .map(|(name, value, disabled)| QueryParameter {
                name,
                value,
                disabled,
            })
            .collect()
    })
}

fn parse_named_list(
    source: &str,
    list: NamedList,
) -> Result<Vec<(String, String, bool)>, CliError> {
    let value = parse_json(
        source,
        &format!("{} must be a JSON array or null", list.list),
    )?;
    let items = match value {
        Value::Null => return Ok(Vec::new()),
        Value::Array(items) => items,
        _ => {
            return Err(CliError::invalid_arguments(format!(
                "{} must be a JSON array or null",
                list.list
            )));
        }
    };
    items
        .iter()
        .map(|item| parse_named_entry(item, list.item))
        .collect()
}

fn parse_named_entry(value: &Value, item: &str) -> Result<(String, String, bool), CliError> {
    let Some(object) = value.as_object() else {
        return Err(named_entry_error(item));
    };
    let Some(name) = object.get("name").and_then(Value::as_str) else {
        return Err(named_entry_error(item));
    };
    let Some(entry_value) = object.get("value").and_then(Value::as_str) else {
        return Err(named_entry_error(item));
    };
    let disabled = optional_bool(
        object,
        "disabled",
        &format!("{item} disabled must be a boolean"),
    )?;
    reject_unknown(object, &["name", "value", "disabled"], item)?;
    Ok((name.to_owned(), entry_value.to_owned(), disabled))
}

fn named_entry_error(item: &str) -> CliError {
    CliError::invalid_arguments(format!(
        "{item} must be a JSON object with string name and value"
    ))
}

fn parse_body_object(object: &Map<String, Value>) -> Result<Body, CliError> {
    let Some(kind) = object.get("type").and_then(Value::as_str) else {
        return Err(CliError::invalid_arguments(BODY_TYPE));
    };
    match kind {
        "json" => parse_raw_body(object, RawBodyKind::Json, "json"),
        "text" => parse_raw_body(object, RawBodyKind::Text, "text"),
        "xml" => parse_raw_body(object, RawBodyKind::Xml, "xml"),
        "sparql" => parse_raw_body(object, RawBodyKind::Sparql, "sparql"),
        "form-urlencoded" => parse_form_body(object),
        "multipart-form" => parse_multipart_body(object),
        "file" => parse_file_body(object),
        _ => Err(CliError::invalid_arguments(BODY_TYPE)),
    }
}

fn parse_raw_body(
    object: &Map<String, Value>,
    kind: RawBodyKind,
    name: &str,
) -> Result<Body, CliError> {
    reject_unknown(object, &["type", "data"], "HTTP body")?;
    let Some(data) = object.get("data").and_then(Value::as_str) else {
        return Err(CliError::invalid_arguments(format!(
            "{name} body data must be a string"
        )));
    };
    Ok(Body::Raw(RawBody {
        kind,
        data: data.to_owned(),
    }))
}

fn parse_form_body(object: &Map<String, Value>) -> Result<Body, CliError> {
    reject_unknown(object, &["type", "data"], "HTTP body")?;
    let fields = object_list(object, "form-urlencoded body data must be a JSON array")?;
    let fields = fields
        .iter()
        .map(|field| {
            let Some(field) = field.as_object() else {
                return Err(form_field_error());
            };
            let Some(name) = field.get("name").and_then(Value::as_str) else {
                return Err(form_field_error());
            };
            let Some(value) = field.get("value").and_then(Value::as_str) else {
                return Err(form_field_error());
            };
            let disabled =
                optional_bool(field, "disabled", "form field disabled must be a boolean")?;
            reject_unknown(field, &["name", "value", "disabled"], "form field")?;
            Ok(FormField {
                name: name.to_owned(),
                value: value.to_owned(),
                disabled,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Body::FormUrlEncoded(fields))
}

fn parse_multipart_body(object: &Map<String, Value>) -> Result<Body, CliError> {
    reject_unknown(object, &["type", "data"], "HTTP body")?;
    let parts = object_list(object, "multipart-form body data must be a JSON array")?;
    let parts = parts
        .iter()
        .map(|part| {
            let Some(part) = part.as_object() else {
                return Err(multipart_part_error());
            };
            let Some(name) = part.get("name").and_then(Value::as_str) else {
                return Err(multipart_part_error());
            };
            let kind = match part.get("type").and_then(Value::as_str) {
                Some("text") => MultipartPartKind::Text,
                Some("file") => MultipartPartKind::File,
                Some(_) => {
                    return Err(CliError::invalid_arguments(
                        "multipart part type must be text or file",
                    ));
                }
                None => return Err(multipart_part_error()),
            };
            let value = multipart_value(part.get("value"))?;
            let content_type = optional_string(
                part,
                "contentType",
                "multipart part contentType must be a string or null",
            )?;
            let disabled = optional_bool(
                part,
                "disabled",
                "multipart part disabled must be a boolean",
            )?;
            reject_unknown(
                part,
                &["name", "type", "value", "contentType", "disabled"],
                "multipart part",
            )?;
            Ok(MultipartPart {
                name: name.to_owned(),
                kind,
                value,
                content_type,
                disabled,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Body::Multipart(parts))
}

fn parse_file_body(object: &Map<String, Value>) -> Result<Body, CliError> {
    reject_unknown(object, &["type", "data"], "HTTP body")?;
    let files = object_list(object, "file body data must be a JSON array")?;
    let files = files
        .iter()
        .map(|file| {
            let Some(file) = file.as_object() else {
                return Err(file_entry_error());
            };
            let Some(file_path) = file.get("filePath").and_then(Value::as_str) else {
                return Err(file_entry_error());
            };
            let Some(content_type) = file.get("contentType").and_then(Value::as_str) else {
                return Err(file_entry_error());
            };
            let Some(Value::Bool(selected)) = file.get("selected") else {
                return Err(file_entry_error());
            };
            reject_unknown(
                file,
                &["filePath", "contentType", "selected"],
                "file body entry",
            )?;
            Ok(FileReference {
                file_path: file_path.to_owned(),
                content_type: content_type.to_owned(),
                selected: *selected,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Body::File(files))
}

fn multipart_value(value: Option<&Value>) -> Result<MultipartValue, CliError> {
    match value {
        Some(Value::String(value)) => Ok(MultipartValue::Single(value.clone())),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(multipart_value_error)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(MultipartValue::Multiple),
        _ => Err(multipart_value_error()),
    }
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    label: &str,
) -> Result<(), CliError> {
    if let Some(name) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(CliError::invalid_arguments(format!(
            "{label} contains unsupported field '{name}'"
        )));
    }
    Ok(())
}

fn insert_string(
    properties: &mut BTreeMap<String, AuthenticationValue>,
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<(), CliError> {
    match object.get(field) {
        None => Ok(()),
        Some(Value::String(value)) => {
            properties.insert(field.to_owned(), AuthenticationValue::String(value.clone()));
            Ok(())
        }
        Some(_) => Err(CliError::invalid_arguments(format!(
            "{label} {field} must be a string"
        ))),
    }
}

fn insert_enum(
    properties: &mut BTreeMap<String, AuthenticationValue>,
    object: &Map<String, Value>,
    field: &str,
    allowed: &[&str],
    label: &str,
) -> Result<(), CliError> {
    let Some(value) = required_enum_value(object, field, allowed, label)? else {
        return Ok(());
    };
    properties.insert(field.to_owned(), AuthenticationValue::String(value));
    Ok(())
}

fn required_enum_value(
    object: &Map<String, Value>,
    field: &str,
    allowed: &[&str],
    label: &str,
) -> Result<Option<String>, CliError> {
    match object.get(field) {
        None => Ok(None),
        Some(Value::String(value)) if allowed.contains(&value.as_str()) => Ok(Some(value.clone())),
        Some(Value::String(_)) => Err(CliError::invalid_arguments(format!(
            "{label} {field} must be {}",
            english_list(allowed)
        ))),
        Some(_) => Err(CliError::invalid_arguments(format!(
            "{label} {field} must be a string"
        ))),
    }
}

fn english_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [first, second] => format!("{first} or {second}"),
        _ => {
            let (last, rest) = items.split_last().expect("list has at least three items");
            format!("{}, or {last}", rest.join(", "))
        }
    }
}

fn object_list<'a>(
    object: &'a Map<String, Value>,
    message: &str,
) -> Result<&'a Vec<Value>, CliError> {
    match object.get("data") {
        Some(Value::Array(items)) => Ok(items),
        _ => Err(CliError::invalid_arguments(message)),
    }
}

fn optional_bool(
    object: &Map<String, Value>,
    field: &str,
    message: &str,
) -> Result<bool, CliError> {
    match object.get(field) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(CliError::invalid_arguments(message)),
    }
}

fn optional_string(
    object: &Map<String, Value>,
    field: &str,
    message: &str,
) -> Result<Option<String>, CliError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(CliError::invalid_arguments(message)),
    }
}

fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    item: &str,
) -> Result<(), CliError> {
    if let Some(name) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(CliError::invalid_arguments(format!(
            "{item} contains unsupported field '{name}'"
        )));
    }
    Ok(())
}

fn parse_json(source: &str, message: &str) -> Result<Value, CliError> {
    serde_json::from_str(source).map_err(|_| CliError::invalid_arguments(message))
}

fn form_field_error() -> CliError {
    CliError::invalid_arguments("form field must be a JSON object with string name and value")
}

fn multipart_part_error() -> CliError {
    CliError::invalid_arguments(
        "multipart part must be a JSON object with string name, type, and value",
    )
}

fn multipart_value_error() -> CliError {
    CliError::invalid_arguments("multipart part value must be a string or array of strings")
}

fn file_entry_error() -> CliError {
    CliError::invalid_arguments(
        "file body entry must be a JSON object with string filePath, string contentType, and boolean selected",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        HttpBodyWrite, parse_authentication, parse_headers, parse_http_body, parse_path_parameters,
        parse_query_parameters,
    };
    use probe_core::{
        AuthenticationKind, AuthenticationValue, Body, FieldPatch, MultipartPartKind,
        MultipartValue, RawBodyKind,
    };

    fn assert_invalid(error: crate::CliError, message: &str) {
        assert_eq!(error.message, message);
    }

    #[test]
    fn writable_auth_forms_are_inherit_basic_bearer_and_apikey() {
        let FieldPatch::Set(inherit) = parse_authentication("\"inherit\"").unwrap() else {
            panic!("inherit should be the JSON string");
        };
        assert_eq!(inherit.kind, AuthenticationKind::Inherit);
        assert!(inherit.properties.is_empty());
        for (kind, source) in [
            ("basic", r#"{"type":"basic","username":"demo"}"#),
            ("bearer", r#"{"type":"bearer","token":"abc"}"#),
            ("apikey", r#"{"type":"apikey","placement":"header"}"#),
        ] {
            let FieldPatch::Set(auth) = parse_authentication(source).unwrap() else {
                panic!("{kind} authentication should parse");
            };
            assert_eq!(auth.kind.as_str(), kind);
        }
        assert!(matches!(
            parse_authentication("null").unwrap(),
            FieldPatch::Clear
        ));
    }

    #[test]
    fn parsers_reject_malformed_field_values() {
        assert_invalid(
            parse_headers(r#"[{"name":"A","value":"B","disabled":"yes"}]"#).unwrap_err(),
            "header disabled must be a boolean",
        );
        assert!(parse_query_parameters("null").unwrap().is_empty());
        assert!(parse_path_parameters("[]").unwrap().is_empty());
        assert_invalid(
            parse_http_body(r#"{"type":"json","data":1}"#).unwrap_err(),
            "json body data must be a string",
        );
        assert_invalid(
            parse_http_body(r#"{"type":"form-urlencoded","data":{}}"#).unwrap_err(),
            "form-urlencoded body data must be a JSON array",
        );
        assert_invalid(
            parse_http_body(
                r#"{"type":"form-urlencoded","data":[{"name":"a","value":"b","disabled":1}]}"#,
            )
            .unwrap_err(),
            "form field disabled must be a boolean",
        );
        assert_invalid(
            parse_http_body(
                r#"{"type":"multipart-form","data":[{"name":"a","type":"blob","value":"x"}]}"#,
            )
            .unwrap_err(),
            "multipart part type must be text or file",
        );
        assert_invalid(
            parse_http_body(
                r#"{"type":"multipart-form","data":[{"name":"a","type":"text","value":1}]}"#,
            )
            .unwrap_err(),
            "multipart part value must be a string or array of strings",
        );
        assert_invalid(
            parse_http_body(r#"{"type":"multipart-form","data":[{"name":"a","type":"text","value":"x","contentType":1}]}"#).unwrap_err(),
            "multipart part contentType must be a string or null",
        );
        assert_invalid(
            parse_http_body(r#"{"type":"file","data":[{"filePath":"./a","contentType":"text/plain","selected":false,"extra":1}]}"#).unwrap_err(),
            "file body entry contains unsupported field 'extra'",
        );
        assert_invalid(
            parse_http_body(r#"{"type":"text","data":"hi","extra":1}"#).unwrap_err(),
            "HTTP body contains unsupported field 'extra'",
        );
        assert_invalid(
            parse_authentication("\"\"").unwrap_err(),
            "authentication must be a JSON object, the string \"inherit\", or null",
        );
        assert_invalid(
            parse_authentication(r#"{"type":"whatever"}"#).unwrap_err(),
            "authentication type must be inherit, basic, bearer, or apikey",
        );
        assert_invalid(
            parse_authentication(r#""custom""#).unwrap_err(),
            "authentication must be a JSON object, the string \"inherit\", or null",
        );
        assert_invalid(
            parse_authentication("[]").unwrap_err(),
            "authentication must be a JSON object, the string \"inherit\", or null",
        );
        assert_invalid(
            parse_path_parameters(r#"[{"name":"id","value":"1","disabled":null}]"#).unwrap_err(),
            "path parameter disabled must be a boolean",
        );
    }

    #[test]
    fn multipart_text_part_without_content_type_is_unset() {
        let HttpBodyWrite::Content(Body::Multipart(parts)) = parse_http_body(
            r#"{"type":"multipart-form","data":[{"name":"caption","type":"text","value":"Summer"},{"name":"files","type":"file","value":["./a.png"]}]}"#,
        )
        .unwrap()
        else {
            panic!("multipart body should parse");
        };
        assert_eq!(parts[0].kind, MultipartPartKind::Text);
        assert_eq!(parts[0].value, MultipartValue::Single("Summer".to_owned()));
        assert_eq!(parts[0].content_type, None);
        assert!(!parts[0].disabled);
        assert_eq!(parts[1].kind, MultipartPartKind::File);
        assert_eq!(
            parts[1].value,
            MultipartValue::Multiple(vec!["./a.png".to_owned()])
        );

        let HttpBodyWrite::Content(Body::Raw(raw)) =
            parse_http_body(r#"{"type":"xml","data":"<a/>"}"#).unwrap()
        else {
            panic!("xml body should parse");
        };
        assert_eq!(raw.kind, RawBodyKind::Xml);

        let FieldPatch::Set(auth) =
            parse_authentication(r#"{"type":"basic","username":"demo","password":"secret"}"#)
                .unwrap()
        else {
            panic!("basic authentication should parse");
        };
        assert_eq!(auth.kind, AuthenticationKind::Basic);
        assert_eq!(
            auth.properties.get("username"),
            Some(&AuthenticationValue::String("demo".to_owned()))
        );
    }

    #[test]
    fn supported_auth_objects_reject_unknown_fields_and_other_schemes() {
        let rejected = [
            (
                r#"{"type":"inherit"}"#,
                "inherit authentication must be the JSON string \"inherit\"",
            ),
            (
                r#"{"type":"basic","username":"a","token":"nope"}"#,
                "basic authentication contains unsupported field 'token'",
            ),
            (
                r#"{"type":"bearer","token":1}"#,
                "bearer authentication token must be a string",
            ),
            (
                r#"{"type":"apikey","placement":"body"}"#,
                "apikey authentication placement must be header or query",
            ),
            (
                r#"{"type":"apikey","key":"X","extra":true}"#,
                "apikey authentication contains unsupported field 'extra'",
            ),
            (
                r#"{"type":"oauth2","flow":"client_credentials"}"#,
                "authentication type must be inherit, basic, bearer, or apikey",
            ),
            (
                r#"{"type":"awsv4","region":"us-east-1"}"#,
                "authentication type must be inherit, basic, bearer, or apikey",
            ),
        ];
        for (source, message) in rejected {
            assert_invalid(parse_authentication(source).unwrap_err(), message);
        }
    }
}
