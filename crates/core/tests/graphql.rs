use probe_core::{
    Body, Environment, EnvironmentVariable, GraphqlRequest, HttpRequest, RawBody, RawBodyKind,
    RequestBody, RequestUpdate, Variable, VariableValue, VariableValueSet, resolve_environment,
    resolve_request,
};

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

#[test]
fn graphql_json_envelopes_have_a_typed_domain_view_and_interpolate() {
    let request = HttpRequest {
        body: Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: r#"{"query":"query Viewer($login: String!) { viewer(login: $login) { login } }","variables":{"login":"{{login}}"},"operationName":"Viewer"}"#.to_owned(),
        }))),
        ..HttpRequest::default()
    };

    let graphql = request.graphql().unwrap().expect("GraphQL envelope");
    assert_eq!(graphql.operation_name.as_deref(), Some("Viewer"));
    assert_eq!(graphql.variables.unwrap()["login"], "{{login}}");

    let resolved = resolve_request(
        &request,
        &resolve_environment(&[environment()], "local").unwrap(),
    )
    .expect("request should resolve");
    let graphql = resolved
        .graphql()
        .unwrap()
        .expect("resolved GraphQL envelope");
    assert_eq!(graphql.variables.unwrap()["login"], "octocat");
}

#[test]
fn graphql_updates_write_canonical_json_bodies_without_reclassifying_extensions() {
    let graphql = GraphqlRequest::from_parts(
        "query Health { health }".to_owned(),
        Some(r#"{"region":"au"}"#),
        None,
    )
    .unwrap();
    let mut request = HttpRequest::default();
    RequestUpdate {
        body: Some(Some(RequestBody::Single(Body::Raw(graphql.as_raw_body())))),
        ..RequestUpdate::default()
    }
    .apply(&mut request);

    assert_eq!(request.graphql().unwrap(), Some(graphql));
    let extended = HttpRequest {
        body: Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: r#"{"query":"query Health { health }","extensions":{"persistedQuery":{}}}"#
                .to_owned(),
        }))),
        ..HttpRequest::default()
    };
    assert_eq!(extended.graphql().unwrap(), None);
}

#[test]
fn graphql_variables_must_be_a_json_object() {
    let error = GraphqlRequest::from_parts("query Health { health }".to_owned(), Some("[]"), None)
        .unwrap_err();
    assert_eq!(error.to_string(), "GraphQL variables must be a JSON object");
}
