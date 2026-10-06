use std::path::Path;

use crate::CommandOutput;

use probe_core::{
    AuthenticationValue, Body, Documentation, GraphqlOperation, GraphqlRequestError,
    MultipartPartKind, MultipartValue, RawBodyKind, Request, RequestBody, VariableValueType,
};
use probe_http::{HttpResponse, MAX_IN_MEMORY_RESPONSE_BYTES};
use serde_json::{Map, Value, json};

pub(super) fn response_human(
    method: &str,
    url: &str,
    response: &HttpResponse,
    output: Option<&Path>,
) -> String {
    let mut rendered = format!(
        "{method} {url}\n\n{} {}\n{} ms\n{}\nFinal URL: {}\nHeaders:\n",
        response.status,
        response.reason,
        response.duration.as_millis(),
        human_size(response.size),
        response.url,
    );
    if response.headers.is_empty() {
        rendered.push_str("  (none)\n");
    } else {
        for header in &response.headers {
            rendered.push_str(&format!("  {}: {}\n", header.name, header.value));
        }
    }
    rendered.push('\n');
    if let Some(output) = output {
        rendered.push_str(&format!("Response body written to {}\n", output.display()));
    } else if !response.body_complete {
        rendered.push_str(&format!(
            "Response body omitted because it exceeds {} bytes; use --output <file>.\n",
            MAX_IN_MEMORY_RESPONSE_BYTES
        ));
    } else if let Ok(body) = std::str::from_utf8(&response.body) {
        let pretty = pretty_json_body(response, body);
        let body = pretty.as_deref().unwrap_or(body);
        rendered.push_str(body);
        if !body.ends_with('\n') {
            rendered.push('\n');
        }
    } else {
        rendered.push_str("Binary response body omitted; use --output <file>.\n");
    }
    rendered
}

fn pretty_json_body(response: &HttpResponse, body: &str) -> Option<String> {
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))?
        .value
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase();
    let (_, subtype) = content_type.split_once('/')?;
    if subtype != "json" && !subtype.ends_with("+json") {
        return None;
    }
    pretty_json_text(body)
}

fn pretty_json_text(body: &str) -> Option<String> {
    // Validate without interpreting values: preserve duplicate keys, key order,
    // number spelling/precision, and string escapes exactly as received.
    let raw: &serde_json::value::RawValue = serde_json::from_str(body).ok()?;
    let mut chars = raw.get().chars().peekable();
    let mut pretty = String::with_capacity(body.len().min(MAX_IN_MEMORY_RESPONSE_BYTES));
    let mut depth = 0;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                json_char(&mut pretty, ch)?;
                let mut escaped = false;
                for ch in chars.by_ref() {
                    json_char(&mut pretty, ch)?;
                    if ch == '"' && !escaped {
                        break;
                    }
                    escaped = ch == '\\' && !escaped;
                }
            }
            '{' | '[' => {
                json_char(&mut pretty, ch)?;
                depth += 1;
                // Bound indentation expansion for deeply nested response bodies.
                if depth > 128 {
                    return None;
                }
                while chars.next_if(|ch| ch.is_ascii_whitespace()).is_some() {}
                if !matches!(chars.peek(), Some('}' | ']')) {
                    json_newline(&mut pretty, depth)?;
                }
            }
            '}' | ']' => {
                depth -= 1;
                if !pretty.ends_with(['{', '[']) {
                    json_newline(&mut pretty, depth)?;
                }
                json_char(&mut pretty, ch)?;
            }
            ',' => {
                json_char(&mut pretty, ch)?;
                json_newline(&mut pretty, depth)?;
            }
            ':' => {
                json_char(&mut pretty, ':')?;
                json_char(&mut pretty, ' ')?;
            }
            ch if ch.is_ascii_whitespace() => {}
            ch => json_char(&mut pretty, ch)?,
        }
    }
    Some(pretty)
}

// Apply the response retention limit to formatted output too, before growing it.
fn json_char(pretty: &mut String, ch: char) -> Option<()> {
    if ch.len_utf8() > MAX_IN_MEMORY_RESPONSE_BYTES - pretty.len() {
        return None;
    }
    pretty.push(ch);
    Some(())
}

fn json_newline(pretty: &mut String, depth: usize) -> Option<()> {
    if 1 + depth * 2 > MAX_IN_MEMORY_RESPONSE_BYTES - pretty.len() {
        return None;
    }
    pretty.push('\n');
    pretty.extend(std::iter::repeat_n(' ', depth * 2));
    Some(())
}

pub(super) fn response_json(
    request: Value,
    response: &HttpResponse,
    output: Option<&Path>,
) -> Value {
    let output_path = output.map(|path| path.to_string_lossy().into_owned());
    let (content, encoding, omitted, omission_reason) = if output.is_some() {
        (None, None, false, None)
    } else if !response.body_complete {
        (None, None, true, Some("too_large"))
    } else if let Ok(body) = std::str::from_utf8(&response.body) {
        (Some(body), Some("utf8"), false, None)
    } else {
        (None, None, true, Some("binary"))
    };
    json!({
        "request": request,
        "response": {
            "body": {
                "content": content,
                "encoding": encoding,
                "omissionReason": omission_reason,
                "omitted": omitted,
                "outputPath": output_path,
            },
            "durationMs": response.duration.as_millis(),
            "headers": response.headers.iter().map(|header| json!({
                "name": header.name,
                "value": header.value,
            })).collect::<Vec<_>>(),
            "reason": response.reason,
            "sizeBytes": response.size,
            "status": response.status,
            "url": response.url,
        }
    })
}

pub(super) fn dry_run_human(request: &Request) -> String {
    format!(
        "{} {}\n",
        request.method.as_deref().unwrap_or("<unset>"),
        request.url.as_deref().unwrap_or("<unset>"),
    )
}

pub(super) fn dry_run_json(request: &Request) -> Result<Value, GraphqlRequestError> {
    Ok(json!({
        "dryRun": true,
        "request": run_request_json(request)?,
    }))
}

pub(super) fn run_request_json(request: &Request) -> Result<Value, GraphqlRequestError> {
    Ok(json!({
        "type": request.kind.as_str(),
        "graphql": request.selected_graphql()?.map(graphql_json),
        "method": request.method,
        "url": request.url,
    }))
}

fn human_size(size: usize) -> String {
    if size < 1024 {
        format!("{size} B")
    } else if size < 1024 * 1024 {
        format!("{:.1} KiB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", size as f64 / (1024.0 * 1024.0))
    }
}

pub(super) fn request_human(
    selector: &str,
    environment: Option<&str>,
    request: &Request,
) -> Result<String, GraphqlRequestError> {
    let mut output = String::new();
    output.push_str(&format!(
        "Name: {}\nSelector: {selector}\nType: {}\nEnvironment: {}\nMethod: {}\nURL: {}\n",
        request.metadata.name.as_deref().unwrap_or("<unnamed>"),
        request.kind.as_str(),
        environment.unwrap_or("<none>"),
        request.method.as_deref().unwrap_or("<unset>"),
        request.url.as_deref().unwrap_or("<unset>"),
    ));
    append_documentation(
        &mut output,
        "Description",
        request.metadata.description.as_ref(),
    );
    append_text(&mut output, "Docs", request.docs.as_deref());

    output.push_str("Headers:\n");
    if request.headers.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for header in &request.headers {
            let state = if header.disabled { " [disabled]" } else { "" };
            output.push_str(&format!("  {}: {}{state}\n", header.name, header.value));
        }
    }

    output.push_str("Path parameters:\n");
    if request.path_parameters.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for parameter in &request.path_parameters {
            let state = if parameter.disabled {
                " [disabled]"
            } else {
                ""
            };
            output.push_str(&format!(
                "  :{}={}{state}\n",
                parameter.name, parameter.value
            ));
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

    if request.kind.is_graphql() {
        output.push_str("Body: graphql\n");
        match request.selected_graphql()? {
            None => output.push_str("GraphQL: <unset>\n"),
            Some(graphql) => {
                output.push_str(&format!(
                    "GraphQL query: {}\nGraphQL variables: {}\nGraphQL operation name: {}\nGraphQL extensions: {}\n",
                    graphql.query.as_deref().unwrap_or("<unset>"),
                    graphql_object_human(graphql.variables.as_ref()),
                    graphql.operation_name.as_deref().unwrap_or("<unset>"),
                    graphql_object_human(graphql.extensions.as_ref()),
                ));
            }
        }
    } else {
        output.push_str(&format!("Body: {}\n", body_summary(request.http_body())));
    }
    output.push_str(&format!(
        "Authentication: {}\n",
        request
            .authentication
            .as_ref()
            .map(|auth| auth.kind.as_str())
            .unwrap_or("<unset>"),
    ));
    Ok(output)
}

pub(super) fn request_json(
    selector: &str,
    environment: Option<&str>,
    request: &Request,
) -> Result<Value, GraphqlRequestError> {
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
    let path_parameters: Vec<_> = request
        .path_parameters
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
            "type": auth.kind.as_str(),
        })
    });

    Ok(json!({
        "authentication": authentication,
        "body": request.http_body().map(request_body_json),
        "description": documentation_json(request.metadata.description.as_ref()),
        "docs": request.docs,
        "environment": environment,
        "graphql": request.selected_graphql()?.map(graphql_json),
        "headers": headers,
        "method": request.method,
        "name": request.metadata.name,
        "pathParameters": path_parameters,
        "queryParameters": query_parameters,
        "selector": selector,
        "type": request.kind.as_str(),
        "url": request.url,
    }))
}

pub(super) fn unset_documentation(
    kind: &str,
    selector: Option<&str>,
    fields: &[&str],
) -> CommandOutput {
    let mut human = String::new();
    for field in fields {
        match selector {
            Some(selector) => human.push_str(&format!("Unset {kind} {selector} {field}\n")),
            None => human.push_str(&format!("Unset {kind} {field}\n")),
        }
    }
    let mut value = serde_json::Map::new();
    value.insert("operation".to_owned(), json!("unset"));
    if let Some(selector) = selector {
        value.insert("selector".to_owned(), json!(selector));
    }
    value.insert("fields".to_owned(), json!(fields));
    CommandOutput {
        human,
        json: Value::Object(value),
    }
}

pub(super) fn documentation_json(value: Option<&Documentation>) -> Value {
    match value {
        None | Some(Documentation::Null) => Value::Null,
        Some(Documentation::Text(text)) => json!(text),
        Some(Documentation::Content {
            content,
            media_type,
        }) => json!({
            "content": content,
            "type": media_type,
        }),
    }
}

pub(super) fn append_documentation(
    output: &mut String,
    label: &str,
    value: Option<&Documentation>,
) {
    match value {
        None => output.push_str(&format!("{label}: <unset>\n")),
        Some(Documentation::Null) => output.push_str(&format!("{label}: null\n")),
        Some(Documentation::Text(text)) => append_text(output, label, Some(text)),
        Some(Documentation::Content {
            content,
            media_type,
        }) => {
            output.push_str(&format!("{label} type: {media_type}\n"));
            append_text(output, label, Some(content));
        }
    }
}

fn append_text(output: &mut String, label: &str, value: Option<&str>) {
    match value {
        Some(text) if text.contains('\n') => {
            output.push_str(&format!("{label}:\n{text}"));
            if !text.ends_with('\n') {
                output.push('\n');
            }
        }
        Some(text) => output.push_str(&format!("{label}: {text}\n")),
        None => output.push_str(&format!("{label}: <unset>\n")),
    }
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

fn graphql_json(graphql: &GraphqlOperation) -> Value {
    json!({
        "extensions": graphql.extensions,
        "operationName": graphql.operation_name,
        "query": graphql.query,
        "variables": graphql.variables,
    })
}

fn graphql_object_human(value: Option<&Map<String, Value>>) -> String {
    value.map_or_else(
        || "<unset>".to_owned(),
        |value| Value::Object(value.clone()).to_string(),
    )
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

fn authentication_value(value: &AuthenticationValue) -> Value {
    match value {
        AuthenticationValue::String(value) => json!(value),
        AuthenticationValue::Boolean(value) => json!(value),
        AuthenticationValue::Number(value) => json!({
            "data": value,
            "type": VariableValueType::Number.as_str(),
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

#[cfg(test)]
mod json_format_tests {
    use super::{MAX_IN_MEMORY_RESPONSE_BYTES, json_newline, pretty_json_text};

    #[test]
    fn changes_only_whitespace_outside_strings() {
        for (body, expected) in [
            (
                " \r\n { \"z\" : [ { }, [ ], { \"a\" : false } ], \"a\":null } \t",
                "{\n  \"z\": [\n    {},\n    [],\n    {\n      \"a\": false\n    }\n  ],\n  \"a\": null\n}",
            ),
            (
                r#"{"s":"雪 \u0061 \/ \" \\ \\\",:{}[] \n\t","end\\":"\\"}"#,
                "{\n  \"s\": \"雪 \\u0061 \\/ \\\" \\\\ \\\\\\\",:{}[] \\n\\t\",\n  \"end\\\\\": \"\\\\\"\n}",
            ),
            ("{ \n }", "{}"),
            ("[ \t ]", "[]"),
            (" true \r\n", "true"),
            ("null", "null"),
            (" -0.00e+9999 ", "-0.00e+9999"),
            (r#""  string  ""#, r#""  string  ""#),
        ] {
            let pretty = pretty_json_text(body).unwrap();
            assert_eq!(pretty, expected, "{body}");
            assert_eq!(pretty_json_text(&pretty).as_deref(), Some(expected));
        }
    }

    #[test]
    fn rejects_invalid_json_instead_of_formatting_a_prefix() {
        for body in [
            "",
            "{\"a\":",
            "{} trailing",
            "{} []",
            "[1,]",
            "{unquoted:1}",
            r#""\q""#,
            "\"unterminated",
            "[01]",
            "[NaN]",
        ] {
            assert_eq!(pretty_json_text(body), None, "{body}");
        }
    }

    #[test]
    fn bounds_indentation_for_deeply_nested_bodies() {
        let body = format!("{}0{}", "[".repeat(128), "]".repeat(128));
        assert!(pretty_json_text(&body).is_some());
        assert_eq!(pretty_json_text(&format!("[{body}]")), None);
    }

    #[test]
    fn bounds_output_bytes_for_indentation_and_utf8() {
        let depth = 128;
        let siblings = MAX_IN_MEMORY_RESPONSE_BYTES / (depth * 2) + 1;
        let body = format!(
            "{}{}0{}",
            "[".repeat(depth),
            "0,".repeat(siblings),
            "]".repeat(depth)
        );
        assert!(body.len() < MAX_IN_MEMORY_RESPONSE_BYTES);
        assert_eq!(pretty_json_text(&body), None);
        assert_eq!(
            pretty_json_text("[[0,0,0]]").as_deref(),
            Some("[\n  [\n    0,\n    0,\n    0\n  ]\n]")
        );
        // Check pre-append boundaries that a later character's fallback can hide.
        for (remaining, depth, fits) in [(3, 1, true), (6, 3, false)] {
            let mut pretty = " ".repeat(MAX_IN_MEMORY_RESPONSE_BYTES - remaining);
            assert_eq!(json_newline(&mut pretty, depth), fits.then_some(()));
            assert!(pretty.len() <= MAX_IN_MEMORY_RESPONSE_BYTES);
        }
        for extra_bytes in 0..=2 {
            let bytes = MAX_IN_MEMORY_RESPONSE_BYTES + extra_bytes;
            let body = format!("\"{}雪\"", "x".repeat(bytes - 5));
            assert_eq!(
                pretty_json_text(&body).map(|pretty| pretty.len()),
                (extra_bytes == 0).then_some(bytes)
            );
        }
    }
}

#[cfg(test)]
mod size_tests {
    use super::human_size;

    #[test]
    fn human_size_uses_binary_units() {
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(2048), "2.0 KiB");
        assert_eq!(human_size(1024 * 1024), "1.0 MiB");
    }
}
