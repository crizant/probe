use probe_core::{
    Body, Environment, EnvironmentVariable, GraphqlBody, GraphqlOperation, GraphqlRequest,
    GraphqlRequestError, GraphqlUpdate, HttpRequest, QueryParameter, RawBody, RawBodyKind,
    RequestBody, RequestProtocol, RequestUpdate, Variable, VariableValue, VariableValueSet,
    resolve_environment, resolve_request,
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

    let prepared = resolved.prepare_http().unwrap();
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
    let prepared = native_request("GET").prepare_http().unwrap();
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
    let prepared = request.prepare_http().unwrap();
    assert_eq!(prepared.query_parameters[0].name, "keep");
    assert_eq!(prepared.query_parameters[1].name, "query");
    assert_ne!(prepared.query_parameters[1].value, "stale");
    assert_eq!(prepared.query_parameters[2].name, "variables");
    assert_eq!(prepared.query_parameters[3].name, "operationName");
    assert_eq!(prepared.query_parameters[4].name, "extensions");
}

#[test]
fn request_update_applies_graphql_fields_and_rejects_http_targets() {
    let mut request = native_request("POST");
    RequestUpdate {
        graphql: Some(GraphqlUpdate {
            variables: Some(Some(object(json!({ "page": 1 })))),
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
                query: Some("query Viewer { viewer { login } }".to_owned()),
                ..GraphqlUpdate::default()
            }),
            ..RequestUpdate::default()
        }
        .apply(&mut ordinary_http),
        Err(GraphqlRequestError::NotGraphql)
    );
    assert_eq!(ordinary_http.protocol, RequestProtocol::Http);
}

#[test]
fn graphql_updates_are_partial_and_http_json_is_not_reclassified() {
    let mut request = native_request("POST");
    request
        .apply_graphql_update(&GraphqlUpdate {
            variables: Some(Some(object(json!({ "page": 1 })))),
            ..GraphqlUpdate::default()
        })
        .unwrap();
    let graphql = request.selected_graphql().unwrap().unwrap();
    assert!(graphql.query.as_ref().unwrap().starts_with("query"));
    assert_eq!(graphql.variables.as_ref().unwrap()["page"], 1);

    let ordinary_http = HttpRequest {
        body: Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: r#"{"query":"search term","variables":{"page":1}}"#.to_owned(),
        }))),
        ..HttpRequest::default()
    };
    assert_eq!(ordinary_http.protocol, RequestProtocol::Http);
    assert!(ordinary_http.graphql().is_none());
}
