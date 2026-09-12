use std::collections::BTreeMap;

use probe_core::{GraphqlBody, GraphqlOperation, ImportDiagnostic};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{
    PostmanImportError,
    diagnostics::{convert_string, extra_fields as diagnose_extra_fields, nonempty},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PostmanGraphql {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    variables: Value,
    #[serde(default)]
    operation_name: Option<String>,
    #[serde(default)]
    extensions: Value,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

pub(super) fn convert_graphql_body(
    value: Option<&Value>,
    resource_id: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<Option<GraphqlBody>, PostmanImportError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let document = PostmanGraphql::deserialize(value).map_err(|error| {
        PostmanImportError::Invalid(format!(
            "invalid Postman GraphQL body for request '{resource_id}': {error}"
        ))
    })?;
    diagnose_extra_fields("body", Some(resource_id), &document.extra, diagnostics);
    Ok(Some(GraphqlBody::Single(GraphqlOperation {
        query: document.query.as_deref().and_then(|query| {
            nonempty(&convert_string(
                query,
                "request",
                resource_id,
                "body.graphql.query",
                diagnostics,
            ))
        }),
        variables: parse_graphql_object(
            &document.variables,
            "variables",
            resource_id,
            diagnostics,
        )?,
        operation_name: document.operation_name.as_deref().and_then(|name| {
            nonempty(&convert_string(
                name,
                "request",
                resource_id,
                "body.graphql.operationName",
                diagnostics,
            ))
        }),
        extensions: parse_graphql_object(
            &document.extensions,
            "extensions",
            resource_id,
            diagnostics,
        )?,
    })))
}

fn parse_graphql_object(
    value: &Value,
    field: &str,
    resource_id: &str,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<Option<Map<String, Value>>, PostmanImportError> {
    let field_path = format!("body.graphql.{field}");
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.trim().is_empty() => Ok(None),
        Value::String(text) => parse_object_json(
            &convert_string(text, "request", resource_id, &field_path, diagnostics),
            field,
            resource_id,
        ),
        Value::Object(_) => {
            let mut converted = value.clone();
            convert_json_strings(&mut converted, &mut |text| {
                convert_string(text, "request", resource_id, &field_path, diagnostics)
            });
            match converted {
                Value::Object(object) => Ok(Some(object)),
                _ => unreachable!("GraphQL object conversion preserves objects"),
            }
        }
        _ => Err(PostmanImportError::Invalid(format!(
            "GraphQL {field} for request '{resource_id}' must be a JSON object"
        ))),
    }
}

fn parse_object_json(
    text: &str,
    field: &str,
    resource_id: &str,
) -> Result<Option<Map<String, Value>>, PostmanImportError> {
    let parsed: Value = serde_json::from_str(text).map_err(|error| {
        PostmanImportError::Invalid(format!(
            "invalid GraphQL {field} for request '{resource_id}': {error}"
        ))
    })?;
    match parsed {
        Value::Null => Ok(None),
        Value::Object(object) => Ok(Some(object)),
        _ => Err(PostmanImportError::Invalid(format!(
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
