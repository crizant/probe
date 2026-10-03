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
const AUTHENTICATION_TYPE: &str = "authentication type must be inherit, awsv4, basic, wsse, bearer, digest, ntlm, apikey, oauth1, or oauth2";
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
    let kind = authentication_kind(kind_name)?;
    let properties = match kind {
        AuthenticationKind::AwsV4 => string_properties(
            object,
            &[
                "accessKeyId",
                "secretAccessKey",
                "sessionToken",
                "service",
                "region",
                "profileName",
            ],
            "awsv4 authentication",
        )?,
        AuthenticationKind::Basic => {
            string_properties(object, &["username", "password"], "basic authentication")?
        }
        AuthenticationKind::Wsse => {
            string_properties(object, &["username", "password"], "wsse authentication")?
        }
        AuthenticationKind::Bearer => {
            string_properties(object, &["token"], "bearer authentication")?
        }
        AuthenticationKind::Digest => {
            string_properties(object, &["username", "password"], "digest authentication")?
        }
        AuthenticationKind::Ntlm => string_properties(
            object,
            &["username", "password", "domain"],
            "ntlm authentication",
        )?,
        AuthenticationKind::ApiKey => api_key_properties(object)?,
        AuthenticationKind::OAuth1 => oauth1_properties(object)?,
        AuthenticationKind::OAuth2 => oauth2_properties(object)?,
        AuthenticationKind::Inherit | AuthenticationKind::Other(_) => {
            return Err(CliError::invalid_arguments(AUTHENTICATION_TYPE));
        }
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

fn oauth1_properties(
    object: &Map<String, Value>,
) -> Result<BTreeMap<String, AuthenticationValue>, CliError> {
    reject_unknown_fields(
        object,
        &[
            "type",
            "consumerKey",
            "consumerSecret",
            "accessToken",
            "accessTokenSecret",
            "callbackUrl",
            "verifier",
            "signatureMethod",
            "privateKey",
            "timestamp",
            "nonce",
            "version",
            "realm",
            "placement",
            "includeBodyHash",
        ],
        "oauth1 authentication",
    )?;
    let mut properties = BTreeMap::new();
    for field in [
        "consumerKey",
        "consumerSecret",
        "accessToken",
        "accessTokenSecret",
        "callbackUrl",
        "verifier",
        "timestamp",
        "nonce",
        "version",
        "realm",
    ] {
        insert_string(&mut properties, object, field, "oauth1 authentication")?;
    }
    insert_enum(
        &mut properties,
        object,
        "signatureMethod",
        &[
            "HMAC-SHA1",
            "HMAC-SHA256",
            "HMAC-SHA512",
            "RSA-SHA1",
            "RSA-SHA256",
            "RSA-SHA512",
            "PLAINTEXT",
        ],
        "oauth1 authentication",
    )?;
    insert_enum(
        &mut properties,
        object,
        "placement",
        &["header", "query", "body"],
        "oauth1 authentication",
    )?;
    insert_bool(
        &mut properties,
        object,
        "includeBodyHash",
        "oauth1 authentication",
    )?;
    if let Some(value) = object.get("privateKey") {
        properties.insert("privateKey".to_owned(), oauth1_private_key(value)?);
    }
    Ok(properties)
}

fn oauth1_private_key(value: &Value) -> Result<AuthenticationValue, CliError> {
    let Some(object) = value.as_object() else {
        return Err(CliError::invalid_arguments(
            "oauth1 privateKey must be an object with type and value",
        ));
    };
    reject_unknown_fields(object, &["type", "value"], "oauth1 privateKey")?;
    let source = match object.get("type").and_then(Value::as_str) {
        Some(source @ ("file" | "text")) => source.to_owned(),
        Some(_) => {
            return Err(CliError::invalid_arguments(
                "oauth1 privateKey type must be file or text",
            ));
        }
        None => {
            return Err(CliError::invalid_arguments(
                "oauth1 privateKey must be an object with type and value",
            ));
        }
    };
    let Some(key) = object.get("value").and_then(Value::as_str) else {
        return Err(CliError::invalid_arguments(
            "oauth1 privateKey must be an object with type and value",
        ));
    };
    let mut properties = BTreeMap::new();
    properties.insert("type".to_owned(), AuthenticationValue::String(source));
    properties.insert(
        "value".to_owned(),
        AuthenticationValue::String(key.to_owned()),
    );
    Ok(AuthenticationValue::Object(properties))
}

fn oauth2_properties(
    object: &Map<String, Value>,
) -> Result<BTreeMap<String, AuthenticationValue>, CliError> {
    let flow = required_enum(
        object,
        "flow",
        &[
            "client_credentials",
            "resource_owner_password_credentials",
            "authorization_code",
            "implicit",
        ],
        "oauth2 authentication",
    )?;
    let allowed: &[&str] = match flow.as_str() {
        "client_credentials" => &[
            "type",
            "flow",
            "accessTokenUrl",
            "refreshTokenUrl",
            "credentials",
            "scope",
            "additionalParameters",
            "tokenConfig",
            "settings",
        ],
        "resource_owner_password_credentials" => &[
            "type",
            "flow",
            "accessTokenUrl",
            "refreshTokenUrl",
            "credentials",
            "resourceOwner",
            "scope",
            "additionalParameters",
            "tokenConfig",
            "settings",
        ],
        "authorization_code" => &[
            "type",
            "flow",
            "authorizationUrl",
            "accessTokenUrl",
            "refreshTokenUrl",
            "callbackUrl",
            "credentials",
            "scope",
            "state",
            "pkce",
            "additionalParameters",
            "tokenConfig",
            "settings",
        ],
        "implicit" => &[
            "type",
            "flow",
            "authorizationUrl",
            "callbackUrl",
            "credentials",
            "scope",
            "state",
            "additionalParameters",
            "tokenConfig",
            "settings",
        ],
        _ => unreachable!("oauth2 flow was validated"),
    };
    reject_unknown_fields(object, allowed, "oauth2 authentication")?;
    let mut properties = BTreeMap::new();
    properties.insert("flow".to_owned(), AuthenticationValue::String(flow.clone()));
    for field in [
        "accessTokenUrl",
        "refreshTokenUrl",
        "authorizationUrl",
        "callbackUrl",
        "scope",
        "state",
    ] {
        insert_string(&mut properties, object, field, "oauth2 authentication")?;
    }
    if let Some(value) = object.get("credentials") {
        let credentials = if flow == "implicit" {
            implicit_credentials(value)?
        } else {
            client_credentials(value)?
        };
        properties.insert("credentials".to_owned(), credentials);
    }
    if let Some(value) = object.get("resourceOwner") {
        properties.insert("resourceOwner".to_owned(), resource_owner(value)?);
    }
    if let Some(value) = object.get("pkce") {
        properties.insert("pkce".to_owned(), pkce(value)?);
    }
    if let Some(value) = object.get("additionalParameters") {
        properties.insert(
            "additionalParameters".to_owned(),
            additional_parameters(value, &flow)?,
        );
    }
    if let Some(value) = object.get("tokenConfig") {
        properties.insert("tokenConfig".to_owned(), token_config(value)?);
    }
    if let Some(value) = object.get("settings") {
        properties.insert("settings".to_owned(), oauth2_settings(value)?);
    }
    Ok(properties)
}

fn client_credentials(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 credentials")?;
    reject_unknown_fields(
        object,
        &["clientId", "clientSecret", "placement"],
        "oauth2 credentials",
    )?;
    let mut properties = BTreeMap::new();
    insert_string(&mut properties, object, "clientId", "oauth2 credentials")?;
    insert_string(
        &mut properties,
        object,
        "clientSecret",
        "oauth2 credentials",
    )?;
    insert_enum(
        &mut properties,
        object,
        "placement",
        &["basic_auth_header", "body"],
        "oauth2 credentials",
    )?;
    Ok(AuthenticationValue::Object(properties))
}

fn implicit_credentials(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 credentials")?;
    reject_unknown_fields(object, &["clientId"], "oauth2 credentials")?;
    let mut properties = BTreeMap::new();
    insert_string(&mut properties, object, "clientId", "oauth2 credentials")?;
    Ok(AuthenticationValue::Object(properties))
}

fn resource_owner(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 resourceOwner")?;
    reject_unknown_fields(object, &["username", "password"], "oauth2 resourceOwner")?;
    let mut properties = BTreeMap::new();
    insert_string(&mut properties, object, "username", "oauth2 resourceOwner")?;
    insert_string(&mut properties, object, "password", "oauth2 resourceOwner")?;
    Ok(AuthenticationValue::Object(properties))
}

fn pkce(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 pkce")?;
    reject_unknown_fields(object, &["disabled", "method"], "oauth2 pkce")?;
    let mut properties = BTreeMap::new();
    insert_bool(&mut properties, object, "disabled", "oauth2 pkce")?;
    insert_enum(
        &mut properties,
        object,
        "method",
        &["S256", "plain"],
        "oauth2 pkce",
    )?;
    Ok(AuthenticationValue::Object(properties))
}

fn additional_parameters(value: &Value, flow: &str) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 additionalParameters")?;
    let allowed: &[&str] = match flow {
        "implicit" => &["authorizationRequest"],
        "authorization_code" => &[
            "authorizationRequest",
            "accessTokenRequest",
            "refreshTokenRequest",
        ],
        _ => &["accessTokenRequest", "refreshTokenRequest"],
    };
    reject_unknown_fields(object, allowed, "oauth2 additionalParameters")?;
    let mut properties = BTreeMap::new();
    for field in allowed {
        if let Some(value) = object.get(*field) {
            properties.insert((*field).to_owned(), parameter_list(value, field)?);
        }
    }
    Ok(AuthenticationValue::Object(properties))
}

fn parameter_list(value: &Value, field: &str) -> Result<AuthenticationValue, CliError> {
    let Some(items) = value.as_array() else {
        return Err(CliError::invalid_arguments(format!(
            "oauth2 additionalParameters {field} must be an array"
        )));
    };
    items
        .iter()
        .map(additional_parameter)
        .collect::<Result<Vec<_>, _>>()
        .map(AuthenticationValue::Sequence)
}

fn additional_parameter(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 additional parameter")?;
    reject_unknown_fields(
        object,
        &["name", "value", "placement"],
        "oauth2 additional parameter",
    )?;
    let mut properties = BTreeMap::new();
    insert_string(
        &mut properties,
        object,
        "name",
        "oauth2 additional parameter",
    )?;
    insert_string(
        &mut properties,
        object,
        "value",
        "oauth2 additional parameter",
    )?;
    insert_enum(
        &mut properties,
        object,
        "placement",
        &["header", "query", "body"],
        "oauth2 additional parameter",
    )?;
    Ok(AuthenticationValue::Object(properties))
}

fn token_config(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 tokenConfig")?;
    reject_unknown_fields(object, &["id", "placement", "source"], "oauth2 tokenConfig")?;
    let mut properties = BTreeMap::new();
    insert_string(&mut properties, object, "id", "oauth2 tokenConfig")?;
    insert_enum(
        &mut properties,
        object,
        "source",
        &["access_token", "id_token"],
        "oauth2 tokenConfig",
    )?;
    if let Some(value) = object.get("placement") {
        properties.insert("placement".to_owned(), token_placement(value)?);
    }
    Ok(AuthenticationValue::Object(properties))
}

fn token_placement(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 token placement")?;
    let header = object.get("header");
    let query = object.get("query");
    match (header, query) {
        (Some(header), None) => {
            reject_unknown_fields(object, &["header"], "oauth2 token placement")?;
            let Some(header) = header.as_str() else {
                return Err(CliError::invalid_arguments(
                    "oauth2 token placement header must be a string",
                ));
            };
            let mut properties = BTreeMap::new();
            properties.insert(
                "header".to_owned(),
                AuthenticationValue::String(header.to_owned()),
            );
            Ok(AuthenticationValue::Object(properties))
        }
        (None, Some(query)) => {
            reject_unknown_fields(object, &["query"], "oauth2 token placement")?;
            let Some(query) = query.as_str() else {
                return Err(CliError::invalid_arguments(
                    "oauth2 token placement query must be a string",
                ));
            };
            let mut properties = BTreeMap::new();
            properties.insert(
                "query".to_owned(),
                AuthenticationValue::String(query.to_owned()),
            );
            Ok(AuthenticationValue::Object(properties))
        }
        _ => Err(CliError::invalid_arguments(
            "oauth2 token placement must be an object with either header or query",
        )),
    }
}

fn oauth2_settings(value: &Value) -> Result<AuthenticationValue, CliError> {
    let object = auth_object(value, "oauth2 settings")?;
    reject_unknown_fields(
        object,
        &["autoFetchToken", "autoRefreshToken"],
        "oauth2 settings",
    )?;
    let mut properties = BTreeMap::new();
    insert_bool(&mut properties, object, "autoFetchToken", "oauth2 settings")?;
    insert_bool(
        &mut properties,
        object,
        "autoRefreshToken",
        "oauth2 settings",
    )?;
    Ok(AuthenticationValue::Object(properties))
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

fn authentication_kind(kind: &str) -> Result<AuthenticationKind, CliError> {
    match kind {
        "awsv4" => Ok(AuthenticationKind::AwsV4),
        "basic" => Ok(AuthenticationKind::Basic),
        "wsse" => Ok(AuthenticationKind::Wsse),
        "bearer" => Ok(AuthenticationKind::Bearer),
        "digest" => Ok(AuthenticationKind::Digest),
        "ntlm" => Ok(AuthenticationKind::Ntlm),
        "apikey" => Ok(AuthenticationKind::ApiKey),
        "oauth1" => Ok(AuthenticationKind::OAuth1),
        "oauth2" => Ok(AuthenticationKind::OAuth2),
        _ => Err(CliError::invalid_arguments(AUTHENTICATION_TYPE)),
    }
}

fn auth_object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, CliError> {
    value
        .as_object()
        .ok_or_else(|| CliError::invalid_arguments(format!("{label} must be an object")))
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

fn insert_bool(
    properties: &mut BTreeMap<String, AuthenticationValue>,
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<(), CliError> {
    match object.get(field) {
        None => Ok(()),
        Some(Value::Bool(value)) => {
            properties.insert(field.to_owned(), AuthenticationValue::Boolean(*value));
            Ok(())
        }
        Some(_) => Err(CliError::invalid_arguments(format!(
            "{label} {field} must be a boolean"
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

fn required_enum(
    object: &Map<String, Value>,
    field: &str,
    allowed: &[&str],
    label: &str,
) -> Result<String, CliError> {
    required_enum_value(object, field, allowed, label)?
        .ok_or_else(|| CliError::invalid_arguments(format!("{label} requires {field}")))
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
    fn authentication_schemes_keep_their_opencollection_names() {
        let FieldPatch::Set(inherit) = parse_authentication("\"inherit\"").unwrap() else {
            panic!("inherit should be the JSON string");
        };
        assert_eq!(inherit.kind, AuthenticationKind::Inherit);
        assert!(inherit.properties.is_empty());
        for (kind, source) in [
            ("awsv4", r#"{"type":"awsv4","region":"us-east-1"}"#),
            ("basic", r#"{"type":"basic","username":"demo"}"#),
            ("wsse", r#"{"type":"wsse","password":"secret"}"#),
            ("bearer", r#"{"type":"bearer","token":"abc"}"#),
            ("digest", r#"{"type":"digest"}"#),
            ("ntlm", r#"{"type":"ntlm","domain":"CORP"}"#),
            ("apikey", r#"{"type":"apikey","placement":"header"}"#),
            (
                "oauth1",
                r#"{"type":"oauth1","signatureMethod":"HMAC-SHA256","placement":"query"}"#,
            ),
            (
                "oauth2",
                r#"{"type":"oauth2","flow":"client_credentials","scope":"read"}"#,
            ),
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
            "authentication type must be inherit, awsv4, basic, wsse, bearer, digest, ntlm, apikey, oauth1, or oauth2",
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
    fn auth_objects_follow_the_opencollection_schema() {
        let oauth2 = r#"{
            "type":"oauth2",
            "flow":"authorization_code",
            "credentials":{"clientId":"probe","placement":"body"},
            "pkce":{"method":"S256"},
            "additionalParameters":{"authorizationRequest":[{"name":"audience","value":"api","placement":"query"}]},
            "tokenConfig":{"source":"access_token","placement":{"header":"Authorization"}}
        }"#;
        let FieldPatch::Set(auth) = parse_authentication(oauth2).unwrap() else {
            panic!("authorization code auth should parse");
        };
        assert_eq!(auth.kind, AuthenticationKind::OAuth2);
        assert_eq!(
            auth.properties.get("flow"),
            Some(&AuthenticationValue::String(
                "authorization_code".to_owned()
            ))
        );

        let oauth1 = r#"{"type":"oauth1","signatureMethod":"RSA-SHA256","includeBodyHash":true,"privateKey":{"type":"text","value":"pem"}}"#;
        assert!(parse_authentication(oauth1).is_ok());

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
                r#"{"type":"basic","username":1}"#,
                "basic authentication username must be a string",
            ),
            (
                r#"{"type":"apikey","placement":"body"}"#,
                "apikey authentication placement must be header or query",
            ),
            (
                r#"{"type":"oauth1","signatureMethod":"MD5"}"#,
                "oauth1 authentication signatureMethod must be HMAC-SHA1, HMAC-SHA256, HMAC-SHA512, RSA-SHA1, RSA-SHA256, RSA-SHA512, or PLAINTEXT",
            ),
            (
                r#"{"type":"oauth1","privateKey":{"type":"text"}}"#,
                "oauth1 privateKey must be an object with type and value",
            ),
            (
                r#"{"type":"oauth1","privateKey":{"type":"pem","value":"x"}}"#,
                "oauth1 privateKey type must be file or text",
            ),
            (
                r#"{"type":"oauth2"}"#,
                "oauth2 authentication requires flow",
            ),
            (
                r#"{"type":"oauth2","flow":"client_credentials","authorizationUrl":"https://example.test"}"#,
                "oauth2 authentication contains unsupported field 'authorizationUrl'",
            ),
            (
                r#"{"type":"oauth2","flow":"client_credentials","credentials":{"clientId":"probe","scopes":["a"]}}"#,
                "oauth2 credentials contains unsupported field 'scopes'",
            ),
            (
                r#"{"type":"oauth2","flow":"implicit","credentials":{"clientSecret":"x"}}"#,
                "oauth2 credentials contains unsupported field 'clientSecret'",
            ),
            (
                r#"{"type":"oauth2","flow":"client_credentials","additionalParameters":{"authorizationRequest":[]}}"#,
                "oauth2 additionalParameters contains unsupported field 'authorizationRequest'",
            ),
            (
                r#"{"type":"oauth2","flow":"authorization_code","tokenConfig":{"placement":{"header":"Authorization","query":"token"}}}"#,
                "oauth2 token placement must be an object with either header or query",
            ),
            (
                r#"{"type":"oauth2","flow":"authorization_code","pkce":{"method":"plain","extra":true}}"#,
                "oauth2 pkce contains unsupported field 'extra'",
            ),
        ];
        for (source, message) in rejected {
            assert_invalid(parse_authentication(source).unwrap_err(), message);
        }
    }
}
