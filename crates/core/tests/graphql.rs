use probe_core::{
    Body, BodyVariant, Environment, EnvironmentVariable, FieldPatch, GraphqlBody,
    GraphqlBodyVariant, GraphqlOperation, GraphqlRequest, GraphqlRequestError, GraphqlUpdate,
    HttpRequest, QueryParameter, RawBody, RawBodyKind, RequestBody, RequestProtocol, RequestUpdate,
    Variable, VariableValue, VariableValueSet, resolve_environment, resolve_request,
};
use serde_json::{Map, Value, json};

fn object(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .cloned()
        .expect("test value must be an object")
}

fn environment() -> Environment {
    Environment {
        name: "local".to_owned(),
        color: None,
        extends: None,
        dot_env_file_path: None,
        variables: vec![EnvironmentVariable::Plain(Variable {
            name: Some("login".to_owned()),
            value: Some(VariableValueSet::Single(VariableValue::String(
                "octocat".to_owned(),
            ))),
            disabled: false,
        })],
    }
}

fn native_request(method: &str) -> HttpRequest {
    GraphqlRequest {
        method: Some(method.to_owned()),
        url: Some("https://example.com/graphql".to_owned()),
        body: Some(GraphqlBody::Single(GraphqlOperation {
            query: Some(
                "query {{operation}}($login: String!) { viewer(login: $login) { login } }"
                    .to_owned(),
            ),
            variables: Some(object(json!({ "login": "{{login}}" }))),
            operation_name: Some("{{operation}}".to_owned()),
            extensions: Some(object(json!({ "trace": "{{login}}" }))),
        })),
        ..GraphqlRequest::default()
    }
    .into_request()
}

#[test]
fn native_graphql_fields_interpolate_and_prepare_post_through_http() {
    let mut environment = environment();
    environment
        .variables
        .push(EnvironmentVariable::Plain(Variable {
            name: Some("operation".to_owned()),
            value: Some(VariableValueSet::Single(VariableValue::String(
                "Viewer".to_owned(),
            ))),
            disabled: false,
        }));
    let resolved = resolve_request(
        &native_request("POST"),
        &resolve_environment(&[environment], "local").unwrap(),
    )
    .expect("request should resolve");
    let graphql = resolved.selected_graphql().unwrap().unwrap();
    assert_eq!(graphql.operation_name.as_deref(), Some("Viewer"));
    assert_eq!(graphql.variables.as_ref().unwrap()["login"], "octocat");
    assert_eq!(graphql.extensions.as_ref().unwrap()["trace"], "octocat");

    let prepared = resolved.into_http().unwrap();
    assert_eq!(prepared.protocol, RequestProtocol::Http);
    let Some(RequestBody::Single(Body::Raw(body))) = prepared.body else {
        panic!("POST GraphQL request should prepare a raw JSON body");
    };
    let envelope: Value = serde_json::from_str(&body.data).unwrap();
    assert_eq!(envelope["operationName"], "Viewer");
    assert_eq!(envelope["variables"]["login"], "octocat");
    assert_eq!(envelope["extensions"]["trace"], "octocat");
}

#[test]
fn native_graphql_get_prepares_graphql_query_parameters() {
    let prepared = native_request("GET").into_http().unwrap();
    assert!(prepared.body.is_none());
    assert_eq!(prepared.query_parameters[0].name, "query");
    assert_eq!(prepared.query_parameters[1].name, "variables");
    assert_eq!(prepared.query_parameters[2].name, "operationName");
    assert_eq!(prepared.query_parameters[3].name, "extensions");
}

#[test]
fn native_graphql_get_replaces_existing_graphql_query_parameters() {
    let mut request = native_request("GET");
    request.query_parameters = vec![
        QueryParameter {
            name: "query".to_owned(),
            value: "stale".to_owned(),
            disabled: false,
        },
        QueryParameter {
            name: "keep".to_owned(),
            value: "1".to_owned(),
            disabled: false,
        },
    ];
    let prepared = request.into_http().unwrap();
    assert_eq!(prepared.query_parameters[0].name, "keep");
    assert_eq!(prepared.query_parameters[1].name, "query");
    assert_ne!(prepared.query_parameters[1].value, "stale");
    assert_eq!(prepared.query_parameters[2].name, "variables");
    assert_eq!(prepared.query_parameters[3].name, "operationName");
    assert_eq!(prepared.query_parameters[4].name, "extensions");
}

#[test]
fn owned_http_preparation_preserves_existing_body_variants() {
    let request = HttpRequest {
        method: Some("POST".to_owned()),
        body: Some(RequestBody::Variants(vec![BodyVariant {
            title: "ordinary HTTP".to_owned(),
            selected: true,
            body: Body::Raw(RawBody {
                kind: RawBodyKind::Json,
                data: "{\"query\":\"ordinary HTTP\"}".to_owned(),
            }),
        }])),
        ..HttpRequest::default()
    };
    assert_eq!(request.clone().into_http().unwrap(), request);
}

#[test]
fn owned_graphql_preparation_selects_one_body_variant() {
    let variants = vec![
        GraphqlBodyVariant {
            title: "other".to_owned(),
            selected: false,
            body: GraphqlOperation {
                query: Some("query Other { other }".to_owned()),
                ..GraphqlOperation::default()
            },
        },
        GraphqlBodyVariant {
            title: "selected".to_owned(),
            selected: true,
            body: GraphqlOperation {
                query: Some("query Selected { selected }".to_owned()),
                ..GraphqlOperation::default()
            },
        },
    ];
    let request = GraphqlRequest {
        method: Some("POST".to_owned()),
        body: Some(GraphqlBody::Variants(variants.clone())),
        ..GraphqlRequest::default()
    }
    .into_request();
    let prepared = request.into_http().unwrap();
    let Some(RequestBody::Single(Body::Raw(body))) = prepared.body else {
        panic!("expected JSON body")
    };
    let envelope: Value = serde_json::from_str(&body.data).unwrap();
    assert_eq!(envelope["query"], "query Selected { selected }");

    for (selected, message) in [
        (vec![false, false], "no selected value"),
        (vec![true, true], "multiple selected values"),
    ] {
        let mut variants = variants.clone();
        for (variant, selected) in variants.iter_mut().zip(selected) {
            variant.selected = selected;
        }
        let request = GraphqlRequest {
            body: Some(GraphqlBody::Variants(variants)),
            ..GraphqlRequest::default()
        }
        .into_request();
        assert!(
            matches!(request.into_http(), Err(GraphqlRequestError::InvalidBodySelection(error)) if error.contains(message))
        );
    }
}

#[test]
fn graphql_variant_selection_is_shared_by_read_and_update() {
    let variants = |first_selected, second_selected| {
        GraphqlBody::Variants(vec![
            GraphqlBodyVariant {
                title: "first".to_owned(),
                selected: first_selected,
                body: GraphqlOperation {
                    query: Some("first".to_owned()),
                    ..GraphqlOperation::default()
                },
            },
            GraphqlBodyVariant {
                title: "second".to_owned(),
                selected: second_selected,
                body: GraphqlOperation {
                    query: Some("second".to_owned()),
                    ..GraphqlOperation::default()
                },
            },
        ])
    };
    let mut request = GraphqlRequest {
        body: Some(variants(false, true)),
        ..GraphqlRequest::default()
    }
    .into_request();
    assert_eq!(
        request
            .selected_graphql()
            .unwrap()
            .unwrap()
            .query
            .as_deref(),
        Some("second")
    );
    request
        .apply_graphql_update(&GraphqlUpdate {
            query: FieldPatch::Set("changed".to_owned()),
            ..GraphqlUpdate::default()
        })
        .unwrap();
    assert_eq!(
        request
            .selected_graphql()
            .unwrap()
            .unwrap()
            .query
            .as_deref(),
        Some("changed")
    );
    let RequestProtocol::Graphql(Some(GraphqlBody::Variants(selected_variants))) =
        &request.protocol
    else {
        unreachable!()
    };
    assert_eq!(selected_variants[0].body.query.as_deref(), Some("first"));

    for (first, second, message) in [
        (false, false, "no selected value"),
        (true, true, "multiple selected values"),
    ] {
        let mut request = GraphqlRequest {
            body: Some(variants(first, second)),
            ..GraphqlRequest::default()
        }
        .into_request();
        assert!(
            matches!(request.selected_graphql(), Err(GraphqlRequestError::InvalidBodySelection(error)) if error.contains(message))
        );
        assert!(
            matches!(request.apply_graphql_update(&GraphqlUpdate::default()), Err(GraphqlRequestError::InvalidBodySelection(error)) if error.contains(message))
        );
    }

    let mut empty = GraphqlRequest::default().into_request();
    empty
        .apply_graphql_update(&GraphqlUpdate {
            query: FieldPatch::Set("initialized".to_owned()),
            ..GraphqlUpdate::default()
        })
        .unwrap();
    assert_eq!(
        empty.selected_graphql().unwrap().unwrap().query.as_deref(),
        Some("initialized")
    );
}

#[test]
fn request_update_applies_graphql_fields_and_rejects_http_targets() {
    let mut request = native_request("POST");
    RequestUpdate {
        graphql: Some(GraphqlUpdate {
            variables: FieldPatch::Set(object(json!({ "page": 1 }))),
            ..GraphqlUpdate::default()
        }),
        ..RequestUpdate::default()
    }
    .apply(&mut request)
    .unwrap();
    let graphql = request.selected_graphql().unwrap().unwrap();
    assert!(graphql.query.as_ref().unwrap().starts_with("query"));
    assert_eq!(graphql.variables.as_ref().unwrap()["page"], 1);

    let mut ordinary_http = HttpRequest {
        body: Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: r#"{"query":"search term"}"#.to_owned(),
        }))),
        ..HttpRequest::default()
    };
    assert_eq!(
        RequestUpdate {
            graphql: Some(GraphqlUpdate {
                query: FieldPatch::Set("query Viewer { viewer { login } }".to_owned()),
                ..GraphqlUpdate::default()
            }),
            ..RequestUpdate::default()
        }
        .apply(&mut ordinary_http),
        Err(GraphqlRequestError::NotGraphql)
    );
    assert_eq!(ordinary_http.protocol, RequestProtocol::Http);
}
