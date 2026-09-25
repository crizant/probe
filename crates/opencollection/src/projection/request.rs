use probe_core::{CollectionItem, Folder, GraphqlRequest, HttpRequest, QueryParameter};
use serde_yaml_ng::Value;

use super::{
    authentication::project_authentication,
    body::{project_graphql_body, project_request_body},
    diagnostic,
};
use crate::{ProjectionDiagnostic, ProjectionDiagnosticKind, document::*};

fn project_parameters(
    parameters: Vec<Value>,
    location: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<(Vec<QueryParameter>, Vec<QueryParameter>), serde_yaml_ng::Error> {
    let mut query = Vec::new();
    let mut path_parameters = Vec::new();
    for (index, value) in parameters.into_iter().enumerate() {
        let kind = value.get("type").and_then(Value::as_str);
        match kind {
            Some("query") => {
                let parameter: ParameterDocument = serde_yaml_ng::from_value(value)?;
                query.push(parameter.into_domain());
            }
            Some("path") => {
                let parameter: ParameterDocument = serde_yaml_ng::from_value(value)?;
                path_parameters.push(parameter.into_domain());
            }
            _ => diagnostic(
                diagnostics,
                format!("{location}/{index}/type"),
                ProjectionDiagnosticKind::ParameterType,
                kind.unwrap_or("<missing>"),
            ),
        }
    }
    Ok((query, path_parameters))
}

pub(crate) fn project_items(
    items: Vec<Value>,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Vec<CollectionItem>, serde_yaml_ng::Error> {
    items
        .into_iter()
        .enumerate()
        .map(|(index, value)| project_item(value, &format!("{path}/{index}"), diagnostics))
        .filter_map(Result::transpose)
        .collect()
}

pub(crate) fn project_item(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Option<CollectionItem>, serde_yaml_ng::Error> {
    let kind: ItemKindDocument = serde_yaml_ng::from_value(value.clone())?;

    match kind.info.item_type.as_deref() {
        Some("folder") => {
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(CollectionItem::Folder(Folder {
                metadata: item.info.into_domain(),
                items: project_items(item.items, &format!("{path}/items"), diagnostics)?,
            })))
        }
        Some("http") => {
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            let settings = item.settings.into_domain()?;
            let http = item.http.unwrap_or_default();
            let body = http
                .body
                .map(|value| project_request_body(value, &format!("{path}/http/body"), diagnostics))
                .transpose()?
                .flatten();
            let authentication = http
                .auth
                .map(|value| {
                    project_authentication(value, &format!("{path}/http/auth"), diagnostics)
                })
                .transpose()?;
            let (query_parameters, path_parameters) =
                project_parameters(http.params, &format!("{path}/http/params"), diagnostics)?;
            Ok(Some(CollectionItem::HttpRequest(HttpRequest {
                metadata: item.info.into_domain(),
                method: http.method,
                url: http.url,
                headers: http
                    .headers
                    .into_iter()
                    .map(HeaderDocument::into_domain)
                    .collect(),
                query_parameters,
                path_parameters,
                body,
                authentication,
                settings,
                protocol: probe_core::RequestProtocol::Http,
            })))
        }
        Some("graphql") => {
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            let settings = item.settings.into_domain()?;
            let graphql = item.graphql.unwrap_or_default();
            let body = graphql.body.map(project_graphql_body).transpose()?;
            let authentication = graphql
                .auth
                .map(|value| {
                    project_authentication(value, &format!("{path}/graphql/auth"), diagnostics)
                })
                .transpose()?;
            let (query_parameters, path_parameters) = project_parameters(
                graphql.params,
                &format!("{path}/graphql/params"),
                diagnostics,
            )?;
            Ok(Some(CollectionItem::GraphqlRequest(GraphqlRequest {
                metadata: item.info.into_domain(),
                method: graphql.method,
                url: graphql.url,
                headers: graphql
                    .headers
                    .into_iter()
                    .map(HeaderDocument::into_domain)
                    .collect(),
                query_parameters,
                path_parameters,
                body,
                authentication,
                settings,
            })))
        }
        other => {
            diagnostic(
                diagnostics,
                format!("{path}/info/type"),
                ProjectionDiagnosticKind::ItemType,
                other.unwrap_or("<missing>"),
            );
            Ok(None)
        }
    }
}
