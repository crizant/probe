use probe_core::{
    AuthenticationKind, AuthenticationValue, Body, HttpRequest, MultipartPartKind, MultipartValue,
    RawBodyKind, RequestBody, VariableValueType,
};
use serde_json::{Map, Value, json};

pub(super) fn request_human(
    selector: &str,
    environment: Option<&str>,
    request: &HttpRequest,
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "Name: {}\nSelector: {selector}\nEnvironment: {}\nMethod: {}\nURL: {}\n",
        request.metadata.name.as_deref().unwrap_or("<unnamed>"),
        environment.unwrap_or("<none>"),
        request.method.as_deref().unwrap_or("<unset>"),
        request.url.as_deref().unwrap_or("<unset>"),
    ));

    output.push_str("Headers:\n");
    if request.headers.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for header in &request.headers {
            let state = if header.disabled { " [disabled]" } else { "" };
            output.push_str(&format!("  {}: {}{state}\n", header.name, header.value));
        }
    }

    output.push_str("Query parameters:\n");
    if request.query_parameters.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for parameter in &request.query_parameters {
            let state = if parameter.disabled {
                " [disabled]"
            } else {
                ""
            };
            output.push_str(&format!(
                "  {}={}{state}\n",
                parameter.name, parameter.value
            ));
        }
    }

    output.push_str(&format!(
        "Body: {}\nAuthentication: {}\n",
        body_summary(request.body.as_ref()),
        request
            .authentication
            .as_ref()
            .map(|auth| authentication_kind(&auth.kind))
            .unwrap_or("<unset>"),
    ));
    output
}

pub(super) fn request_json(
    selector: &str,
    environment: Option<&str>,
    request: &HttpRequest,
) -> Value {
    let headers: Vec<_> = request
        .headers
        .iter()
        .map(|header| {
            json!({
                "disabled": header.disabled,
                "name": header.name,
                "value": header.value,
            })
        })
        .collect();
    let query_parameters: Vec<_> = request
        .query_parameters
        .iter()
        .map(|parameter| {
            json!({
                "disabled": parameter.disabled,
                "name": parameter.name,
                "value": parameter.value,
            })
        })
        .collect();
    let authentication = request.authentication.as_ref().map(|auth| {
        let properties: Map<_, _> = auth
            .properties
            .iter()
            .map(|(name, value)| (name.clone(), authentication_value(value)))
            .collect();
        json!({
            "properties": properties,
            "type": authentication_kind(&auth.kind),
        })
    });

    json!({
        "authentication": authentication,
        "body": request.body.as_ref().map(request_body_json),
        "environment": environment,
        "headers": headers,
        "method": request.method,
        "name": request.metadata.name,
        "queryParameters": query_parameters,
        "selector": selector,
        "url": request.url,
    })
}

fn body_summary(body: Option<&RequestBody>) -> &'static str {
    match body {
        None => "<unset>",
        Some(RequestBody::Single(Body::Raw(raw))) => raw_body_kind(&raw.kind),
        Some(RequestBody::Single(Body::FormUrlEncoded(_))) => "form-urlencoded",
        Some(RequestBody::Single(Body::Multipart(_))) => "multipart-form",
        Some(RequestBody::Single(Body::File(_))) => "file",
        Some(RequestBody::Variants(_)) => "variants",
    }
}

fn request_body_json(body: &RequestBody) -> Value {
    match body {
        RequestBody::Single(body) => json!({
            "mode": "single",
            "value": body_json(body),
        }),
        RequestBody::Variants(variants) => json!({
            "mode": "variants",
            "variants": variants.iter().map(|variant| json!({
                "body": body_json(&variant.body),
                "selected": variant.selected,
                "title": variant.title,
            })).collect::<Vec<_>>(),
        }),
    }
}

fn body_json(body: &Body) -> Value {
    match body {
        Body::Raw(body) => json!({
            "data": body.data,
            "type": raw_body_kind(&body.kind),
        }),
        Body::FormUrlEncoded(fields) => json!({
            "data": fields.iter().map(|field| json!({
                "disabled": field.disabled,
                "name": field.name,
                "value": field.value,
            })).collect::<Vec<_>>(),
            "type": "form-urlencoded",
        }),
        Body::Multipart(parts) => json!({
            "data": parts.iter().map(|part| json!({
                "contentType": part.content_type,
                "disabled": part.disabled,
                "name": part.name,
                "type": match part.kind {
                    MultipartPartKind::Text => "text",
                    MultipartPartKind::File => "file",
                },
                "value": match &part.value {
                    MultipartValue::Single(value) => json!(value),
                    MultipartValue::Multiple(values) => json!(values),
                },
            })).collect::<Vec<_>>(),
            "type": "multipart-form",
        }),
        Body::File(files) => json!({
            "data": files.iter().map(|file| json!({
                "contentType": file.content_type,
                "filePath": file.file_path,
                "selected": file.selected,
            })).collect::<Vec<_>>(),
            "type": "file",
        }),
    }
}

const fn raw_body_kind(kind: &RawBodyKind) -> &'static str {
    match kind {
        RawBodyKind::Json => "json",
        RawBodyKind::Text => "text",
        RawBodyKind::Xml => "xml",
        RawBodyKind::Sparql => "sparql",
    }
}

fn authentication_kind(kind: &AuthenticationKind) -> &str {
    match kind {
        AuthenticationKind::Inherit => "inherit",
        AuthenticationKind::AwsV4 => "awsv4",
        AuthenticationKind::Basic => "basic",
        AuthenticationKind::Wsse => "wsse",
        AuthenticationKind::Bearer => "bearer",
        AuthenticationKind::Digest => "digest",
        AuthenticationKind::Ntlm => "ntlm",
        AuthenticationKind::ApiKey => "apikey",
        AuthenticationKind::OAuth1 => "oauth1",
        AuthenticationKind::OAuth2 => "oauth2",
        AuthenticationKind::Other(kind) => kind,
    }
}

fn authentication_value(value: &AuthenticationValue) -> Value {
    match value {
        AuthenticationValue::String(value) => json!(value),
        AuthenticationValue::Boolean(value) => json!(value),
        AuthenticationValue::Number(value) => json!({
            "data": value,
            "type": variable_value_type(&VariableValueType::Number),
        }),
        AuthenticationValue::Null => Value::Null,
        AuthenticationValue::Sequence(values) => {
            Value::Array(values.iter().map(authentication_value).collect())
        }
        AuthenticationValue::Object(values) => Value::Object(
            values
                .iter()
                .map(|(name, value)| (name.clone(), authentication_value(value)))
                .collect(),
        ),
    }
}

const fn variable_value_type(value_type: &VariableValueType) -> &'static str {
    match value_type {
        VariableValueType::String => "string",
        VariableValueType::Number => "number",
        VariableValueType::Boolean => "boolean",
        VariableValueType::Null => "null",
        VariableValueType::Object => "object",
    }
}
