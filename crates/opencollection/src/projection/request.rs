use probe_core::{CollectionItem, Folder, QueryParameter, Request, RequestKind};

use crate::{
    document::{optional_documentation, request_docs_from_yaml},
    native_item::NativeItemType,
};
use serde_yaml_ng::Value;

use super::{
    authentication::project_authentication,
    body::{project_graphql_body, project_request_body, project_websocket_message},
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
    let item_name = value
        .get("info")
        .and_then(|info| info.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let start = diagnostics.len();
    let item = project_item_contents(value, path, diagnostics)?;
    for diagnostic in &mut diagnostics[start..] {
        if diagnostic.item_name.is_none() {
            diagnostic.item_name = item_name.clone();
        }
    }
    Ok(item)
}

fn project_item_contents(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Option<CollectionItem>, serde_yaml_ng::Error> {
    let description =
        optional_documentation(value.get("info").and_then(|info| info.get("description")))?;
    let docs = value.get("docs").cloned();
    let kind: ItemKindDocument = serde_yaml_ng::from_value(value.clone())?;

    match kind
        .info
        .item_type
        .as_deref()
        .and_then(NativeItemType::from_name)
    {
        Some(NativeItemType::Folder) => {
            let docs = optional_documentation(docs.as_ref())?;
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            let mut metadata = item.info.into_domain();
            metadata.description = description;
            Ok(Some(CollectionItem::Folder(Folder {
                metadata,
                docs,
                items: project_items(item.items, &format!("{path}/items"), diagnostics)?,
            })))
        }
        Some(NativeItemType::Http) => {
            let docs = request_docs_from_yaml(docs.as_ref())?;
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
            let mut metadata = item.info.into_domain();
            metadata.description = description;
            Ok(Some(CollectionItem::Request(Request {
                metadata,
                docs,
                method: http.method,
                url: http.url,
                headers: http
                    .headers
                    .into_iter()
                    .map(HeaderDocument::into_domain)
                    .collect(),
                query_parameters,
                path_parameters,
                authentication,
                settings,
                kind: RequestKind::Http { body },
            })))
        }
        Some(NativeItemType::Graphql) => {
            let docs = request_docs_from_yaml(docs.as_ref())?;
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
            let mut metadata = item.info.into_domain();
            metadata.description = description;
            Ok(Some(CollectionItem::Request(Request {
                metadata,
                docs,
                method: graphql.method,
                url: graphql.url,
                headers: graphql
                    .headers
                    .into_iter()
                    .map(HeaderDocument::into_domain)
                    .collect(),
                query_parameters,
                path_parameters,
                authentication,
                settings,
                kind: RequestKind::Graphql { body },
            })))
        }
        Some(NativeItemType::WebSocket) => {
            let docs = request_docs_from_yaml(docs.as_ref())?;
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            let settings = item.settings.into_websocket_domain()?;
            let websocket = item.websocket.unwrap_or_default();
            let message = websocket
                .message
                .map(|value| {
                    project_websocket_message(
                        value,
                        &format!("{path}/websocket/message"),
                        diagnostics,
                    )
                })
                .transpose()?
                .flatten();
            let authentication = websocket
                .auth
                .map(|value| {
                    project_authentication(value, &format!("{path}/websocket/auth"), diagnostics)
                })
                .transpose()?;
            let mut metadata = item.info.into_domain();
            metadata.description = description;
            Ok(Some(CollectionItem::Request(Request {
                metadata,
                docs,
                method: None,
                url: websocket.url,
                headers: websocket
                    .headers
                    .into_iter()
                    .map(HeaderDocument::into_domain)
                    .collect(),
                query_parameters: Vec::new(),
                path_parameters: Vec::new(),
                authentication,
                settings,
                kind: RequestKind::WebSocket { message },
            })))
        }
        None => {
            diagnostic(
                diagnostics,
                format!("{path}/info/type"),
                ProjectionDiagnosticKind::ItemType,
                kind.info.item_type.as_deref().unwrap_or("<missing>"),
            );
            Ok(None)
        }
    }
}
