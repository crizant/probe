use std::collections::BTreeMap;

use probe_core::{
    Authentication, AuthenticationKind, Body, GraphqlBody, GraphqlBodyVariant, GraphqlOperation,
    GraphqlRequest, GraphqlRequestError, Header, HttpRequest, ItemMetadata, QueryParameter,
    RawBody, RawBodyKind, RequestBody, RequestUpdate,
};
use serde_json::{Map, Value, json};

fn parameter(name: &str, value: &str) -> QueryParameter {
    QueryParameter {
        name: name.to_owned(),
        value: value.to_owned(),
        disabled: false,
    }
}

fn body(text: &str) -> RequestBody {
    RequestBody::Single(Body::Raw(RawBody {
        kind: RawBodyKind::Json,
        data: text.to_owned(),
    }))
}

fn authentication() -> Authentication {
    Authentication {
        kind: AuthenticationKind::Basic,
        properties: BTreeMap::new(),
    }
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn graphql(operation: GraphqlOperation) -> HttpRequest {
    GraphqlRequest {
        body: Some(GraphqlBody::Single(operation)),
        ..GraphqlRequest::default()
    }
    .into_request()
}

#[test]
fn unchanged_request_has_no_patch() {
    let request = HttpRequest::default();
    assert!(
        RequestUpdate::between(Some(&request), &request)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn new_graphql_request_includes_its_operation_in_the_patch() {
    let request = graphql(GraphqlOperation {
        query: Some("query Viewer { viewer }".to_owned()),
        ..GraphqlOperation::default()
    });
    let update = RequestUpdate::between(None, &request).unwrap();
    assert_eq!(
        update.graphql.unwrap().query.as_deref(),
        Some("query Viewer { viewer }")
    );
}

#[test]
fn diff_tracks_http_fields_and_applies_the_patch() {
    let base = HttpRequest {
        metadata: ItemMetadata {
            name: Some("Old".to_owned()),
            sequence: None,
        },
        method: Some("GET".to_owned()),
        url: Some("https://old.example".to_owned()),
        headers: vec![Header {
            name: "X-Old".to_owned(),
            value: "1".to_owned(),
            disabled: false,
        }],
        query_parameters: vec![parameter("old", "1")],
        path_parameters: vec![parameter("id", "1")],
        body: Some(body("old")),
        authentication: Some(authentication()),
        ..HttpRequest::default()
    };
    let mut current = base.clone();
    current.metadata.name = Some("New".to_owned());
    current.method = Some("PATCH".to_owned());
    current.url = Some("https://new.example".to_owned());
    current.headers.clear();
    current.query_parameters = vec![parameter("new", "2")];
    current.path_parameters.clear();
    current.body = Some(body("new"));
    current.authentication = None;

    let update = RequestUpdate::between(Some(&base), &current).unwrap();
    assert_eq!(update.name.as_deref(), Some("New"));
    assert_eq!(update.method.as_deref(), Some("PATCH"));
    assert_eq!(update.url.as_deref(), Some("https://new.example"));
    assert_eq!(update.headers, Some(Vec::new()));
    assert_eq!(
        update.query_parameters,
        Some(current.query_parameters.clone())
    );
    assert_eq!(update.path_parameters, Some(Vec::new()));
    assert_eq!(update.body, Some(current.body.clone()));
    assert_eq!(update.authentication, Some(None));
    assert!(update.graphql.is_none());
    let mut patched = base;
    update.apply(&mut patched).unwrap();
    assert_eq!(patched, current);
}

#[test]
fn diff_distinguishes_cleared_body_and_authentication() {
    let base = HttpRequest {
        body: Some(body("old")),
        authentication: Some(authentication()),
        ..HttpRequest::default()
    };
    let current = HttpRequest::default();
    let update = RequestUpdate::between(Some(&base), &current).unwrap();
    assert_eq!(update.body, Some(None));
    assert_eq!(update.authentication, Some(None));
    let mut patched = base;
    update.apply(&mut patched).unwrap();
    assert_eq!(patched, current);
}

#[test]
fn diff_tracks_graphql_operation_changes_and_optional_clears() {
    let base = graphql(GraphqlOperation {
        query: Some("query Old { old }".to_owned()),
        variables: Some(object(json!({ "id": 1 }))),
        operation_name: Some("Old".to_owned()),
        extensions: Some(object(json!({ "trace": true }))),
    });
    let current = graphql(GraphqlOperation {
        query: Some("query New { new }".to_owned()),
        ..GraphqlOperation::default()
    });
    let update = RequestUpdate::between(Some(&base), &current).unwrap();
    let graphql = update.graphql.as_ref().unwrap();
    assert_eq!(graphql.query.as_deref(), Some("query New { new }"));
    assert_eq!(graphql.variables, Some(None));
    assert_eq!(graphql.operation_name, Some(None));
    assert_eq!(graphql.extensions, Some(None));
    let mut patched = base;
    update.apply(&mut patched).unwrap();
    assert_eq!(patched, current);
}

#[test]
fn diff_uses_the_selected_graphql_variant_and_rejects_invalid_selection() {
    let base = GraphqlRequest {
        body: Some(GraphqlBody::Variants(vec![
            GraphqlBodyVariant {
                title: "first".to_owned(),
                selected: true,
                body: GraphqlOperation {
                    query: Some("first".to_owned()),
                    ..GraphqlOperation::default()
                },
            },
            GraphqlBodyVariant {
                title: "second".to_owned(),
                selected: false,
                body: GraphqlOperation {
                    query: Some("second".to_owned()),
                    ..GraphqlOperation::default()
                },
            },
        ])),
        ..GraphqlRequest::default()
    }
    .into_request();
    let mut current = base.clone();
    let probe_core::RequestProtocol::Graphql(Some(GraphqlBody::Variants(variants))) =
        &mut current.protocol
    else {
        unreachable!()
    };
    variants[0].body.query = Some("changed".to_owned());
    assert_eq!(
        RequestUpdate::between(Some(&base), &current)
            .unwrap()
            .graphql
            .unwrap()
            .query
            .as_deref(),
        Some("changed")
    );

    for (first, second, message) in [
        (false, false, "no selected value"),
        (true, true, "multiple selected values"),
    ] {
        let mut invalid = current.clone();
        let probe_core::RequestProtocol::Graphql(Some(GraphqlBody::Variants(variants))) =
            &mut invalid.protocol
        else {
            unreachable!()
        };
        variants[0].selected = first;
        variants[1].selected = second;
        assert!(
            matches!(RequestUpdate::between(Some(&base), &invalid), Err(GraphqlRequestError::InvalidBodySelection(error)) if error.contains(message))
        );
    }
}
