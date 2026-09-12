use super::*;
use probe_core::{GraphqlBody, GraphqlOperation, GraphqlRequest};

pub(super) fn convert_request(
    workspace: &YaakWorkspace,
    request: &YaakHttpRequest,
    folders: &BTreeMap<&str, &YaakFolder>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<CollectionItem, YaakImportError> {
    diagnose_extra_fields(
        "http_request",
        Some(&request.id),
        &request.extra,
        diagnostics,
    );
    if !request.description.trim().is_empty() {
        diagnostics.push(lossy(
            "unsupported_field",
            "http_request",
            Some(&request.id),
            Some("description"),
            "request descriptions cannot be represented by the current Probe domain",
        ));
    }
    let ancestors = folder_ancestors(request.folder_id.as_deref(), folders);
    let headers = effective_headers(workspace, &ancestors, request, diagnostics);
    let authentication = effective_authentication(workspace, &ancestors, request, diagnostics);
    let settings = effective_settings(workspace, &ancestors, request, diagnostics);
    let mut query_parameters = Vec::new();
    let mut path_parameters = Vec::new();
    for parameter in &request.url_parameters {
        let name = convert_templates(
            &parameter.name,
            "http_request",
            &request.id,
            "urlParameters.name",
            diagnostics,
        );
        let converted = QueryParameter {
            name: name.strip_prefix(':').unwrap_or(&name).to_owned(),
            value: convert_templates(
                &parameter.value,
                "http_request",
                &request.id,
                "urlParameters.value",
                diagnostics,
            ),
            disabled: !parameter.enabled.unwrap_or(true),
        };
        if name.starts_with(':') {
            path_parameters.push(converted);
        } else {
            query_parameters.push(converted);
        }
        diagnose_extra_fields(
            "http_request",
            Some(&request.id),
            &parameter.extra,
            diagnostics,
        );
    }
    let metadata = ItemMetadata {
        name: nonempty(&request.name),
        sequence: Some(request.sort_priority),
    };
    let method = nonempty(&request.method);
    let url = Some(convert_templates(
        &request.url,
        "http_request",
        &request.id,
        "url",
        diagnostics,
    ));
    if request.body_type.as_deref() == Some("graphql") {
        return Ok(CollectionItem::GraphqlRequest(GraphqlRequest {
            metadata,
            method,
            url,
            headers,
            query_parameters,
            path_parameters,
            body: convert_graphql_body(request, diagnostics)?,
            authentication,
            settings,
        }));
    }
    Ok(CollectionItem::HttpRequest(HttpRequest {
        metadata,
        method,
        url,
        headers,
        query_parameters,
        path_parameters,
        body: convert_http_body(request, diagnostics),
        authentication,
        settings,
        protocol: probe_core::RequestProtocol::Http,
    }))
}

fn folder_ancestors<'a>(
    folder_id: Option<&str>,
    folders: &'a BTreeMap<&str, &'a YaakFolder>,
) -> Vec<&'a YaakFolder> {
    let mut chain = Vec::new();
    let mut current = folder_id;
    while let Some(id) = current {
        let folder = folders
            .get(id)
            .copied()
            .expect("folder graph was validated before conversion");
        chain.push(folder);
        current = folder.folder_id.as_deref();
    }
    chain.reverse();
    chain
}

fn effective_headers(
    workspace: &YaakWorkspace,
    ancestors: &[&YaakFolder],
    request: &YaakHttpRequest,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Vec<Header> {
    let mut headers = Vec::<Header>::new();
    let iter = workspace
        .headers
        .iter()
        .chain(ancestors.iter().flat_map(|folder| folder.headers.iter()))
        .chain(request.headers.iter());
    for header in iter {
        let converted = Header {
            name: convert_templates(
                &header.name,
                "http_request",
                &request.id,
                "headers.name",
                diagnostics,
            ),
            value: convert_templates(
                &header.value,
                "http_request",
                &request.id,
                "headers.value",
                diagnostics,
            ),
            disabled: !header.enabled.unwrap_or(true),
        };
        if let Some(existing) = headers
            .iter_mut()
            .find(|existing| existing.name.eq_ignore_ascii_case(&converted.name))
        {
            *existing = converted;
        } else {
            headers.push(converted);
        }
        diagnose_extra_fields("header", header.id.as_deref(), &header.extra, diagnostics);
    }
    headers
}

fn effective_authentication(
    workspace: &YaakWorkspace,
    ancestors: &[&YaakFolder],
    request: &YaakHttpRequest,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<Authentication> {
    let mut owner_type = workspace.authentication_type.as_deref();
    let mut owner = &workspace.authentication;
    for folder in ancestors {
        if folder.authentication_type.is_some() {
            owner_type = folder.authentication_type.as_deref();
            owner = &folder.authentication;
        }
    }
    if request.authentication_type.is_some() {
        owner_type = request.authentication_type.as_deref();
        owner = &request.authentication;
    }
    let auth_type = owner_type?;
    if auth_type == "none" {
        return None;
    }
    let kind = match auth_type {
        "awsv4" => AuthenticationKind::AwsV4,
        "basic" => AuthenticationKind::Basic,
        "bearer" => AuthenticationKind::Bearer,
        "digest" => AuthenticationKind::Digest,
        "ntlm" => AuthenticationKind::Ntlm,
        "apikey" => AuthenticationKind::ApiKey,
        "oauth1" => AuthenticationKind::OAuth1,
        "oauth2" => AuthenticationKind::OAuth2,
        other => {
            diagnostics.push(lossy(
                "unsupported_authentication",
                "http_request",
                Some(&request.id),
                Some("authenticationType"),
                &format!("Yaak authentication type '{other}' is not defined by OpenCollection"),
            ));
            return None;
        }
    };
    if !matches!(kind, AuthenticationKind::Basic | AuthenticationKind::Bearer) {
        diagnostics.push(warning(
            "execution_unsupported",
            "http_request",
            Some(&request.id),
            Some("authenticationType"),
            &format!(
                "authentication type '{}' is preserved but the current Probe HTTP engine cannot execute it",
                kind.as_str()
            ),
        ));
    }
    let properties = owner
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                json_auth_value(
                    value,
                    request,
                    &format!("authentication.{name}"),
                    diagnostics,
                ),
            )
        })
        .collect();
    Some(Authentication { kind, properties })
}

fn json_auth_value(
    value: &Value,
    request: &YaakHttpRequest,
    field: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> AuthenticationValue {
    match value {
        Value::Null => AuthenticationValue::Null,
        Value::Bool(value) => AuthenticationValue::Boolean(*value),
        Value::Number(value) => AuthenticationValue::Number(value.to_string()),
        Value::String(value) => AuthenticationValue::String(convert_templates(
            value,
            "http_request",
            &request.id,
            field,
            diagnostics,
        )),
        Value::Array(values) => AuthenticationValue::Sequence(
            values
                .iter()
                .map(|value| json_auth_value(value, request, field, diagnostics))
                .collect(),
        ),
        Value::Object(values) => AuthenticationValue::Object(
            values
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        json_auth_value(value, request, field, diagnostics),
                    )
                })
                .collect(),
        ),
    }
}

fn effective_settings(
    workspace: &YaakWorkspace,
    ancestors: &[&YaakFolder],
    request: &YaakHttpRequest,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> RequestSettings {
    let mut timeout = workspace.setting_request_timeout.unwrap_or(0);
    let mut follow_redirects = workspace.setting_follow_redirects.unwrap_or(true);
    let mut validate_certificates = workspace.setting_validate_certificates.unwrap_or(true);
    let mut send_cookies = workspace.setting_send_cookies.unwrap_or(true);
    let mut store_cookies = workspace.setting_store_cookies.unwrap_or(true);
    for folder in ancestors {
        override_setting(&folder.setting_request_timeout, &mut timeout);
        override_setting(&folder.setting_follow_redirects, &mut follow_redirects);
        override_setting(
            &folder.setting_validate_certificates,
            &mut validate_certificates,
        );
        override_setting(&folder.setting_send_cookies, &mut send_cookies);
        override_setting(&folder.setting_store_cookies, &mut store_cookies);
    }
    override_setting(&request.setting_request_timeout, &mut timeout);
    override_setting(&request.setting_follow_redirects, &mut follow_redirects);
    override_setting(
        &request.setting_validate_certificates,
        &mut validate_certificates,
    );
    override_setting(&request.setting_send_cookies, &mut send_cookies);
    override_setting(&request.setting_store_cookies, &mut store_cookies);
    if !validate_certificates {
        diagnostics.push(lossy(
            "unsupported_setting",
            "http_request",
            Some(&request.id),
            Some("settingValidateCertificates"),
            "disabling certificate validation cannot be represented by the current Probe domain",
        ));
    }
    if !send_cookies || !store_cookies {
        diagnostics.push(lossy(
            "unsupported_setting",
            "http_request",
            Some(&request.id),
            Some("cookieSettings"),
            "Yaak cookie-jar settings cannot be represented by the current Probe domain",
        ));
    }
    RequestSettings {
        timeout: (timeout > 0).then(|| Duration::from_millis(timeout as u64)),
        follow_redirects: Some(follow_redirects),
        max_redirects: None,
    }
}

fn override_setting<T: Copy + Default>(setting: &Option<InheritedSetting<T>>, value: &mut T) {
    if let Some(setting) = setting
        && setting.enabled.unwrap_or(false)
    {
        *value = setting.value;
    }
}

fn convert_http_body(
    request: &YaakHttpRequest,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<RequestBody> {
    let body_type = request.body_type.as_deref()?;
    let body = match body_type {
        "application/json" => Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: body_text(request, diagnostics),
        }),
        "application/xml" | "text/xml" => Body::Raw(RawBody {
            kind: RawBodyKind::Xml,
            data: body_text(request, diagnostics),
        }),
        "application/sparql-query" => Body::Raw(RawBody {
            kind: RawBodyKind::Sparql,
            data: body_text(request, diagnostics),
        }),
        "text/plain" | "other" => Body::Raw(RawBody {
            kind: RawBodyKind::Text,
            data: body_text(request, diagnostics),
        }),
        "application/x-www-form-urlencoded" => Body::FormUrlEncoded(
            body_forms(request)
                .into_iter()
                .map(|field| FormField {
                    name: convert_templates(
                        &field.name,
                        "http_request",
                        &request.id,
                        "body.form.name",
                        diagnostics,
                    ),
                    value: convert_templates(
                        field.value.as_deref().unwrap_or_default(),
                        "http_request",
                        &request.id,
                        "body.form.value",
                        diagnostics,
                    ),
                    disabled: !field.enabled.unwrap_or(true),
                })
                .collect(),
        ),
        "multipart/form-data" => Body::Multipart(
            body_forms(request)
                .into_iter()
                .map(|field| {
                    let file = field.file.as_deref();
                    MultipartPart {
                        name: convert_templates(
                            &field.name,
                            "http_request",
                            &request.id,
                            "body.form.name",
                            diagnostics,
                        ),
                        kind: if file.is_some() {
                            MultipartPartKind::File
                        } else {
                            MultipartPartKind::Text
                        },
                        value: MultipartValue::Single(convert_templates(
                            file.or(field.value.as_deref()).unwrap_or_default(),
                            "http_request",
                            &request.id,
                            "body.form.value",
                            diagnostics,
                        )),
                        content_type: field.content_type,
                        disabled: !field.enabled.unwrap_or(true),
                    }
                })
                .collect(),
        ),
        "binary" => {
            let file_path = request
                .body
                .get("filePath")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Body::File(vec![FileReference {
                file_path: convert_templates(
                    file_path,
                    "http_request",
                    &request.id,
                    "body.filePath",
                    diagnostics,
                ),
                content_type: String::new(),
                selected: true,
            }])
        }
        other => {
            diagnostics.push(lossy(
                "unsupported_body_type",
                "http_request",
                Some(&request.id),
                Some("bodyType"),
                &format!("Yaak body type '{other}' is not supported"),
            ));
            Body::Raw(RawBody {
                kind: RawBodyKind::Text,
                data: body_text(request, diagnostics),
            })
        }
    };
    Some(RequestBody::Single(body))
}

fn body_text(request: &YaakHttpRequest, diagnostics: &mut Vec<ImportDiagnostic>) -> String {
    convert_templates(
        request
            .body
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "http_request",
        &request.id,
        "body.text",
        diagnostics,
    )
}

fn body_forms(request: &YaakHttpRequest) -> Vec<YaakFormField> {
    request
        .body
        .get("form")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

const GRAPHQL_BODY_FIELDS: &[&str] = &["query", "variables", "operationName", "extensions", "text"];

fn convert_graphql_body(
    request: &YaakHttpRequest,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<Option<GraphqlBody>, YaakImportError> {
    diagnose_unknown_graphql_fields(request, diagnostics);
    let query = optional_graphql_string(request.body.get("query"), "query", &request.id)?;
    let text = optional_graphql_string(request.body.get("text"), "text", &request.id)?;
    if query.is_some() && text.is_some_and(|value| !value.trim().is_empty()) {
        diagnostics.push(lossy(
            "inactive_body_data",
            "http_request",
            Some(&request.id),
            Some("text"),
            "inactive Yaak body data cannot be represented alongside the selected GraphQL query",
        ));
    }
    let (query, variables, operation_name, extensions, query_field) = if let Some(query) = query {
        (
            Some(query.to_owned()),
            request.body.get("variables").cloned(),
            request
                .body
                .get("operationName")
                .and_then(Value::as_str)
                .map(str::to_owned),
            request.body.get("extensions").cloned(),
            "body.query",
        )
    } else if let Some(envelope) = text.and_then(parse_graphql_envelope) {
        (
            Some(envelope.query),
            envelope.variables,
            envelope.operation_name,
            envelope.extensions,
            "body.text",
        )
    } else if let Some(text) = text {
        (
            Some(text.to_owned()),
            request.body.get("variables").cloned(),
            request
                .body
                .get("operationName")
                .and_then(Value::as_str)
                .map(str::to_owned),
            request.body.get("extensions").cloned(),
            "body.text",
        )
    } else if request.body.is_empty() {
        return Ok(None);
    } else {
        (
            None,
            request.body.get("variables").cloned(),
            request
                .body
                .get("operationName")
                .and_then(Value::as_str)
                .map(str::to_owned),
            request.body.get("extensions").cloned(),
            "body.query",
        )
    };
    Ok(Some(GraphqlBody::Single(GraphqlOperation {
        query: query
            .as_deref()
            .and_then(|query| graphql_string(query, request, query_field, diagnostics)),
        variables: parse_graphql_object(variables.as_ref(), "variables", request, diagnostics)?,
        operation_name: operation_name.as_deref().and_then(|name| {
            graphql_string(
                name,
                request,
                if query_field == "body.text" {
                    "body.text"
                } else {
                    "body.operationName"
                },
                diagnostics,
            )
        }),
        extensions: parse_graphql_object(extensions.as_ref(), "extensions", request, diagnostics)?,
    })))
}

struct GraphqlEnvelope {
    query: String,
    variables: Option<Value>,
    operation_name: Option<String>,
    extensions: Option<Value>,
}

fn parse_graphql_envelope(text: &str) -> Option<GraphqlEnvelope> {
    let Value::Object(object) = serde_json::from_str(text).ok()? else {
        return None;
    };
    Some(GraphqlEnvelope {
        query: object.get("query")?.as_str()?.to_owned(),
        variables: object.get("variables").cloned(),
        operation_name: object
            .get("operationName")
            .and_then(Value::as_str)
            .map(str::to_owned),
        extensions: object.get("extensions").cloned(),
    })
}

fn diagnose_unknown_graphql_fields(
    request: &YaakHttpRequest,
    diagnostics: &mut Vec<ImportDiagnostic>,
) {
    for field in request.body.keys() {
        if !GRAPHQL_BODY_FIELDS.contains(&field.as_str()) {
            diagnostics.push(lossy(
                "unknown_field",
                "http_request",
                Some(&request.id),
                Some(field),
                &format!("unknown Yaak field '{field}' cannot be guaranteed to survive import"),
            ));
        }
    }
}

fn optional_graphql_string<'a>(
    value: Option<&'a Value>,
    field: &str,
    request_id: &str,
) -> Result<Option<&'a str>, YaakImportError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(YaakImportError::Invalid(format!(
            "GraphQL {field} for request '{request_id}' must be a string"
        ))),
    }
}

fn graphql_string(
    value: &str,
    request: &YaakHttpRequest,
    field: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Option<String> {
    nonempty(&convert_templates(
        value,
        "http_request",
        &request.id,
        field,
        diagnostics,
    ))
}

fn parse_graphql_object(
    value: Option<&Value>,
    field: &str,
    request: &YaakHttpRequest,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<Option<serde_json::Map<String, Value>>, YaakImportError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let field_path = format!("body.{field}");
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.trim().is_empty() => Ok(None),
        Value::String(text) => parse_object_json(
            &convert_templates(text, "http_request", &request.id, &field_path, diagnostics),
            field,
            &request.id,
        ),
        Value::Object(_) => {
            let mut converted = value.clone();
            convert_json_strings(&mut converted, &mut |text| {
                convert_templates(text, "http_request", &request.id, &field_path, diagnostics)
            });
            match converted {
                Value::Object(object) => Ok(Some(object)),
                _ => unreachable!("GraphQL object conversion preserves objects"),
            }
        }
        _ => Err(YaakImportError::Invalid(format!(
            "GraphQL {field} for request '{}' must be a JSON object",
            request.id
        ))),
    }
}

fn parse_object_json(
    text: &str,
    field: &str,
    resource_id: &str,
) -> Result<Option<serde_json::Map<String, Value>>, YaakImportError> {
    let parsed: Value = serde_json::from_str(text).map_err(|error| {
        YaakImportError::Invalid(format!(
            "invalid GraphQL {field} for request '{resource_id}': {error}"
        ))
    })?;
    match parsed {
        Value::Null => Ok(None),
        Value::Object(object) => Ok(Some(object)),
        _ => Err(YaakImportError::Invalid(format!(
            "GraphQL {field} for request '{resource_id}' must be a JSON object"
        ))),
    }
}

fn convert_json_strings(value: &mut Value, convert: &mut impl FnMut(&str) -> String) {
    match value {
        Value::String(text) => *text = convert(text),
        Value::Array(values) => {
            for value in values {
                convert_json_strings(value, convert);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                convert_json_strings(value, convert);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
