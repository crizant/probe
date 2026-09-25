use probe_core::{
    Body, BodyVariant, GraphqlBody, GraphqlBodyVariant, GraphqlOperation, RawBody, RequestBody,
};
use serde_yaml_ng::Value;

use super::diagnostic;
use crate::{ProjectionDiagnostic, ProjectionDiagnosticKind, document::*};

pub(super) fn project_request_body(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Option<RequestBody>, serde_yaml_ng::Error> {
    if value.is_sequence() {
        let variants: Vec<BodyVariantDocument> = serde_yaml_ng::from_value(value)?;
        let mut projected = Vec::with_capacity(variants.len());

        for (index, variant) in variants.into_iter().enumerate() {
            if let Some(body) =
                project_body(variant.body, &format!("{path}/{index}/body"), diagnostics)?
            {
                projected.push(BodyVariant {
                    title: variant.title,
                    selected: variant.selected,
                    body,
                });
            }
        }

        Ok(Some(RequestBody::Variants(projected)))
    } else {
        Ok(project_body(value, path, diagnostics)?.map(RequestBody::Single))
    }
}

pub(super) fn project_graphql_body(value: Value) -> Result<GraphqlBody, serde_yaml_ng::Error> {
    if value.is_sequence() {
        let variants: Vec<GraphqlBodyVariantDocument> = serde_yaml_ng::from_value(value)?;
        Ok(GraphqlBody::Variants(
            variants
                .into_iter()
                .map(|variant| {
                    project_graphql_operation(variant.body).map(|body| GraphqlBodyVariant {
                        title: variant.title,
                        selected: variant.selected,
                        body,
                    })
                })
                .collect::<Result<_, _>>()?,
        ))
    } else {
        let body: GraphqlBodyDocument = serde_yaml_ng::from_value(value)?;
        Ok(GraphqlBody::Single(project_graphql_operation(body)?))
    }
}

fn project_graphql_operation(
    body: GraphqlBodyDocument,
) -> Result<GraphqlOperation, serde_yaml_ng::Error> {
    Ok(GraphqlOperation {
        query: body.query,
        variables: body
            .variables
            .map(|value| project_graphql_object(value, "variables"))
            .transpose()?,
        operation_name: body.operation_name,
        extensions: body
            .extensions
            .map(|value| project_graphql_object(value, "extensions"))
            .transpose()?,
    })
}

fn project_graphql_object(
    value: Value,
    field: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, serde_yaml_ng::Error> {
    let value = match value {
        Value::String(source) => serde_json::from_str(&source).map_err(|error| {
            <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
                "GraphQL {field} must contain a JSON object: {error}"
            ))
        })?,
        value => serde_json::to_value(value).map_err(|error| {
            <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
                "GraphQL {field} must be a JSON object: {error}"
            ))
        })?,
    };
    value.as_object().cloned().ok_or_else(|| {
        <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
            "GraphQL {field} must be a JSON object"
        ))
    })
}

fn project_body(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Option<Body>, serde_yaml_ng::Error> {
    let kind: BodyKindDocument = serde_yaml_ng::from_value(value.clone())?;

    match kind.body_type.as_str() {
        "json" | "text" | "xml" | "sparql" => {
            let body: RawBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::Raw(RawBody {
                kind: body.body_type.into_domain(),
                data: body.data,
            })))
        }
        "form-urlencoded" => {
            let body: FormBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::FormUrlEncoded(
                body.data
                    .into_iter()
                    .map(FormFieldDocument::into_domain)
                    .collect(),
            )))
        }
        "multipart-form" => {
            let body: MultipartBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::Multipart(
                body.data
                    .into_iter()
                    .map(MultipartPartDocument::into_domain)
                    .collect(),
            )))
        }
        "file" => {
            let body: FileBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::File(
                body.data
                    .into_iter()
                    .map(FileReferenceDocument::into_domain)
                    .collect(),
            )))
        }
        other => {
            diagnostic(
                diagnostics,
                format!("{path}/type"),
                ProjectionDiagnosticKind::BodyType,
                other,
            );
            Ok(None)
        }
    }
}
