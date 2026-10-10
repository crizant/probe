use std::{fs, path::PathBuf, time::Duration};

use probe_core::{
    AuthenticationKind, AuthenticationValue, Body, CollectionItem, Documentation,
    EnvironmentVariable, MultipartValue, RawBodyKind, RequestBody, VariableValue, VariableValueSet,
    VariableValueType, WebSocketMessage, WebSocketMessageKind, WebSocketMessageSet, Workspace,
    WorkspaceItemRef, resolve_environment, resolve_request,
};
use probe_opencollection::{ProjectionDiagnosticKind, parse};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/opencollection")
        .join(name);
    fs::read_to_string(path).expect("fixture should be readable")
}

#[test]
fn parses_collection_folders_and_http_requests() {
    let parsed = parse(&fixture("phase1-bundled.yml")).expect("fixture should parse");
    let collection = parsed.collection();

    assert_eq!(collection.metadata.name.as_deref(), Some("Pet Store"));
    assert_eq!(
        collection.metadata.summary.as_deref(),
        Some("Requests for the example pet service")
    );
    assert_eq!(collection.metadata.version.as_deref(), Some("2.1.0"));
    assert_eq!(collection.metadata.authors.len(), 1);
    assert_eq!(
        collection.metadata.authors[0].email.as_deref(),
        Some("probe@example.com")
    );
    assert_eq!(collection.items.len(), 2);

    let CollectionItem::Folder(folder) = &collection.items[0] else {
        panic!("first item should be a folder");
    };
    assert_eq!(folder.metadata.name.as_deref(), Some("Pets"));
    assert_eq!(folder.metadata.sequence, Some(1.0));
    assert_eq!(folder.items.len(), 1);

    let CollectionItem::Request(request) = &folder.items[0] else {
        panic!("folder child should be an HTTP request");
    };
    assert_eq!(request.metadata.name.as_deref(), Some("List pets"));
    assert_eq!(request.method.as_deref(), Some("GET"));
    assert_eq!(request.url.as_deref(), Some("https://api.example.com/pets"));
    assert_eq!(request.headers.len(), 2);
    assert_eq!(request.headers[0].name, "Accept");
    assert!(!request.headers[0].disabled);
    assert!(request.headers[1].disabled);
    assert_eq!(request.query_parameters.len(), 1);
    assert_eq!(request.query_parameters[0].name, "limit");
    assert_eq!(request.query_parameters[0].value, "25");
    assert_eq!(request.path_parameters.len(), 1);
    assert_eq!(request.path_parameters[0].name, "ownerId");
    assert_eq!(request.path_parameters[0].value, "42");
    assert_eq!(
        request.settings.timeout,
        Some(Duration::from_micros(2_500_500))
    );
    assert_eq!(request.settings.follow_redirects, Some(false));
    assert_eq!(request.settings.max_redirects, Some(3));
}

#[test]
fn fixtures_round_trip_without_data_loss() {
    for name in [
        "phase1-round-trip.yml",
        "phase1-bodies-auth-environments.yml",
        "graphql-http.yml",
        "websocket.yml",
    ] {
        let source = fixture(name);
        let parsed = parse(&source).unwrap_or_else(|_| panic!("{name} should parse"));
        let serialized = parsed
            .to_yaml()
            .unwrap_or_else(|_| panic!("{name} should serialize"));
        let reparsed =
            parse(&serialized).unwrap_or_else(|_| panic!("{name} serialized should parse"));

        assert_eq!(parsed.collection(), reparsed.collection(), "{name}");

        let before: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&source).expect("source should be YAML");
        let after: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&serialized).expect("serialized output should be YAML");
        assert_eq!(before, after, "{name}");
    }
}

#[test]
fn unsupported_projection_is_reported_and_retained() {
    let source = fixture("unsupported-projection.yml");
    let parsed = parse(&source).unwrap();
    assert_eq!(parsed.collection().items.len(), 2);
    let diagnostics = parsed
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.path.as_str(),
                diagnostic.kind,
                diagnostic.value.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 5);
    let names = parsed
        .diagnostics()
        .iter()
        .map(|diagnostic| (diagnostic.value.as_str(), diagnostic.item_name.as_deref()))
        .collect::<Vec<_>>();
    assert!(names.contains(&("matrix", Some("Supported request with future fields"))));
    assert!(names.contains(&("binary-stream", Some("Future body"))));
    assert!(names.contains(&("grpc", Some("Future item"))));
    for expected in [
        (
            "items/0/http/params/0/type",
            ProjectionDiagnosticKind::ParameterType,
            "matrix",
        ),
        (
            "items/0/http/auth/futureProperty",
            ProjectionDiagnosticKind::AuthenticationProperty,
            "futureProperty",
        ),
        (
            "items/0/http/auth",
            ProjectionDiagnosticKind::AuthenticationProperty,
            "Number(7)",
        ),
        (
            "items/1/http/body/type",
            ProjectionDiagnosticKind::BodyType,
            "binary-stream",
        ),
        (
            "items/2/info/type",
            ProjectionDiagnosticKind::ItemType,
            "grpc",
        ),
    ] {
        assert!(diagnostics.contains(&expected), "missing {expected:?}");
    }

    let serialized = parsed.to_yaml().unwrap();
    let before: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source).unwrap();
    let after: serde_yaml_ng::Value = serde_yaml_ng::from_str(&serialized).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        parse(&serialized).unwrap().diagnostics(),
        parsed.diagnostics()
    );
}

#[test]
fn diagnostics_identify_the_nearest_named_item_without_changing_source() {
    let source = fixture("diagnostic-context.yml");
    let parsed = parse(&source).unwrap();
    let contexts = parsed
        .diagnostics()
        .iter()
        .map(|diagnostic| (diagnostic.value.as_str(), diagnostic.item_name.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        contexts,
        [
            ("Contacts", Some("List contacts")),
            ("future-body", Some("Customers")),
            ("future-item", None),
            ("<missing>", None),
        ]
    );
    let serialized = parsed.to_yaml().unwrap();
    assert_eq!(
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&source).unwrap(),
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&serialized).unwrap()
    );
    assert_eq!(
        parse(&serialized).unwrap().diagnostics(),
        parsed.diagnostics()
    );
}

#[test]
fn parses_and_interpolates_native_graphql_requests() {
    let parsed = parse(&fixture("graphql-http.yml")).expect("GraphQL fixture should parse");
    let collection = parsed.collection();
    let CollectionItem::Request(request) = &collection.items[0] else {
        panic!("fixture should contain a native GraphQL request");
    };

    assert_eq!(request.method.as_deref(), Some("POST"));
    assert_eq!(request.url.as_deref(), Some("{{serverUrl}}/graphql"));
    let probe_core::GraphqlBody::Single(graphql) = request.graphql().unwrap() else {
        panic!("fixture should contain one GraphQL body");
    };
    assert_eq!(graphql.operation_name.as_deref(), Some("Viewer"));
    assert_eq!(graphql.variables.as_ref().unwrap()["login"], "{{login}}");
    assert_eq!(
        graphql.extensions.as_ref().unwrap()["trace"]["enabled"],
        true
    );

    let environment = resolve_environment(&collection.environments, "local")
        .expect("fixture environment should resolve");
    let resolved = resolve_request(request, &environment)
        .expect("GraphQL request variables should interpolate");
    assert_eq!(resolved.url.as_deref(), Some("__SERVER_URL__/graphql"));
    assert_eq!(
        resolved
            .selected_graphql()
            .unwrap()
            .unwrap()
            .variables
            .as_ref()
            .unwrap()["login"],
        "octocat"
    );
}

#[test]
fn parses_and_interpolates_native_websocket_requests() {
    let parsed = parse(&fixture("websocket.yml")).expect("WebSocket fixture should parse");
    let collection = parsed.collection();
    let requests = collection
        .items
        .iter()
        .map(|item| match item {
            CollectionItem::Request(request) => request,
            CollectionItem::Folder(_) => panic!("fixture should contain only requests"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.kind.as_str())
            .collect::<Vec<_>>(),
        ["websocket", "websocket", "http", "graphql"]
    );

    let subscribe = requests[0];
    assert_eq!(subscribe.metadata.name.as_deref(), Some("Subscribe"));
    assert_eq!(subscribe.metadata.sequence, Some(1.0));
    assert_eq!(
        subscribe.metadata.description,
        Some(Documentation::Text("Subscribe to order events".to_owned()))
    );
    assert_eq!(subscribe.docs.as_deref(), Some("Streams order events."));
    assert_eq!(subscribe.method, None);
    assert_eq!(subscribe.url.as_deref(), Some("wss://{{host}}/events"));
    assert!(subscribe.query_parameters.is_empty());
    assert!(subscribe.path_parameters.is_empty());
    assert_eq!(subscribe.headers.len(), 2);
    assert!(subscribe.headers[1].disabled);
    let authentication = subscribe.authentication.as_ref().unwrap();
    assert_eq!(authentication.kind, AuthenticationKind::Bearer);
    assert_eq!(
        subscribe.selected_websocket_message().unwrap(),
        Some(&WebSocketMessage {
            kind: WebSocketMessageKind::Json,
            data: r#"{"subscribe":"{{channel}}"}"#.to_owned(),
        })
    );
    assert_eq!(
        subscribe.settings.timeout,
        Some(Duration::from_millis(1500))
    );
    assert_eq!(
        subscribe.settings.keep_alive_interval,
        Some(Duration::from_secs(30))
    );

    let variants = requests[1];
    let Some(WebSocketMessageSet::Variants(messages)) = variants.websocket_message() else {
        panic!("fixture should contain WebSocket message variants");
    };
    assert_eq!(
        messages
            .iter()
            .map(|variant| (
                variant.title.as_str(),
                variant.selected,
                variant.message.kind
            ))
            .collect::<Vec<_>>(),
        [
            ("Greeting", false, WebSocketMessageKind::Text),
            ("Payload", true, WebSocketMessageKind::Xml),
        ]
    );
    assert_eq!(
        variants.selected_websocket_message().unwrap().unwrap().data,
        "<ping/>"
    );
    assert_eq!(variants.settings.timeout, None);
    assert_eq!(variants.settings.keep_alive_interval, None);
    assert_eq!(
        parsed
            .diagnostics()
            .iter()
            .map(|diagnostic| (
                diagnostic.path.as_str(),
                diagnostic.kind,
                diagnostic.value.as_str(),
                diagnostic.item_name.as_deref()
            ))
            .collect::<Vec<_>>(),
        [(
            "items/1/websocket/message/2/message/type",
            ProjectionDiagnosticKind::BodyType,
            "cbor",
            Some("Variants")
        )]
    );

    assert_eq!(requests[2].method.as_deref(), Some("GET"));
    assert_eq!(requests[2].settings.keep_alive_interval, None);
    assert_eq!(
        requests[3]
            .selected_graphql()
            .unwrap()
            .unwrap()
            .query
            .as_deref(),
        Some("{ viewer { login } }")
    );

    let environment = resolve_environment(&collection.environments, "local").unwrap();
    let resolved = resolve_request(subscribe, &environment).unwrap();
    assert_eq!(
        resolved.url.as_deref(),
        Some("wss://socket.example.com/events")
    );
    assert_eq!(resolved.headers[0].value, "probe-orders");
    assert_eq!(
        resolved.authentication.as_ref().unwrap().properties["token"],
        AuthenticationValue::String("local-token".to_owned())
    );
    assert_eq!(
        resolved.selected_websocket_message().unwrap().unwrap().data,
        r#"{"subscribe":"orders"}"#
    );
    let resolved_variants = resolve_request(variants, &environment).unwrap();
    let Some(WebSocketMessageSet::Variants(messages)) = resolved_variants.websocket_message()
    else {
        panic!("resolution should keep WebSocket message variants");
    };
    assert_eq!(messages[0].message.data, "hello orders");
}

#[test]
fn websocket_settings_reject_invalid_keep_alive_intervals_only_for_websocket() {
    let document = |item_type: &str, interval: &str| {
        format!(
            "opencollection: 1.0.0\ninfo:\n  name: Settings\nbundled: true\nitems:\n- info:\n    name: Item\n    type: {item_type}\n  {item_type}:\n    url: wss://example.com\n  settings:\n    keepAliveInterval: {interval}\n"
        )
    };
    for interval in ["-1", "soon", "1e400"] {
        let error = parse(&document("websocket", interval)).unwrap_err();
        assert!(
            error.to_string().contains("keep-alive interval"),
            "{interval}: {error}"
        );
    }
    let http = parse(&document("http", "soon")).expect("HTTP ignores keepAliveInterval");
    let CollectionItem::Request(request) = &http.collection().items[0] else {
        panic!("fixture should contain a request");
    };
    assert_eq!(request.settings.keep_alive_interval, None);
}

#[test]
fn parses_bodies_authentication_and_environments() {
    let parsed = parse(&fixture("phase1-bodies-auth-environments.yml"))
        .expect("complete fixture should parse");
    let collection = parsed.collection();

    assert_eq!(collection.environments.len(), 2);
    let development = &collection.environments[0];
    assert_eq!(development.name, "development");
    assert_eq!(development.color.as_deref(), Some("green"));
    assert_eq!(
        development.dot_env_file_path.as_deref(),
        Some(".env.development")
    );
    assert_eq!(development.variables.len(), 4);

    let EnvironmentVariable::Plain(retries) = &development.variables[1] else {
        panic!("retries should be a plain variable");
    };
    assert_eq!(
        retries.value,
        Some(VariableValueSet::Single(VariableValue::Typed {
            kind: VariableValueType::Number,
            data: "3".to_owned(),
        }))
    );

    let EnvironmentVariable::Plain(region) = &development.variables[2] else {
        panic!("region should be a plain variable");
    };
    let Some(VariableValueSet::Variants(region_values)) = &region.value else {
        panic!("region should have variants");
    };
    assert_eq!(region_values.len(), 2);
    assert!(region_values[0].selected);

    let EnvironmentVariable::Secret(secret) = &development.variables[3] else {
        panic!("apiToken should be secret");
    };
    assert_eq!(secret.name.as_deref(), Some("apiToken"));
    assert_eq!(secret.value_type, Some(VariableValueType::String));
    assert_eq!(
        collection.environments[1].extends.as_deref(),
        Some("development")
    );

    let requests: Vec<_> = collection
        .items
        .iter()
        .map(|item| match item {
            CollectionItem::Request(request) if !request.kind.is_graphql() => request,
            CollectionItem::Folder(_) | CollectionItem::Request(_) => {
                panic!("fixture should contain only HTTP requests")
            }
        })
        .collect();
    assert_eq!(requests.len(), 5);

    let Some(RequestBody::Single(Body::Raw(raw))) = requests[0].http_body() else {
        panic!("first request should have a raw body");
    };
    assert_eq!(raw.kind, RawBodyKind::Json);
    assert_eq!(raw.data, r#"{"name":"Milo"}"#);
    let bearer = requests[0]
        .authentication
        .as_ref()
        .expect("first request should have auth");
    assert_eq!(bearer.kind, AuthenticationKind::Bearer);
    assert_eq!(
        bearer.properties.get("token"),
        Some(&AuthenticationValue::String("{{apiToken}}".to_owned()))
    );

    let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) = requests[1].http_body() else {
        panic!("second request should have a form body");
    };
    assert_eq!(fields.len(), 2);
    assert!(fields[1].disabled);
    assert_eq!(
        requests[1].authentication.as_ref().map(|auth| &auth.kind),
        Some(&AuthenticationKind::Basic)
    );

    let Some(RequestBody::Single(Body::Multipart(parts))) = requests[2].http_body() else {
        panic!("third request should have a multipart body");
    };
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[1].value,
        MultipartValue::Multiple(vec![
            "./images/one.png".to_owned(),
            "./images/two.png".to_owned()
        ])
    );

    let Some(RequestBody::Single(Body::File(files))) = requests[3].http_body() else {
        panic!("fourth request should have a file body");
    };
    assert_eq!(files[0].file_path, "./archive.zip");
    assert!(files[0].selected);
    assert_eq!(
        requests[3].authentication.as_ref().map(|auth| &auth.kind),
        Some(&AuthenticationKind::Inherit)
    );

    let Some(RequestBody::Variants(variants)) = requests[4].http_body() else {
        panic!("fifth request should have body variants");
    };
    assert_eq!(variants.len(), 2);
    assert!(variants[0].selected);
    let oauth = requests[4]
        .authentication
        .as_ref()
        .expect("fifth request should have auth");
    assert_eq!(oauth.kind, AuthenticationKind::OAuth2);
    assert_eq!(
        oauth.properties.get("flow"),
        Some(&AuthenticationValue::String(
            "client_credentials".to_owned()
        ))
    );
}

#[test]
fn api_key_authentication_is_executable_and_other_kinds_stay_diagnostic() {
    let source = concat!(
        "opencollection: 1.0.0\n",
        "info: { name: Auth }\n",
        "bundled: true\n",
        "items:\n",
        "  - info: { name: Key, type: http }\n",
        "    http:\n",
        "      method: GET\n",
        "      url: https://example.com/items\n",
        "      auth:\n",
        "        type: apikey\n",
        "        key: X-API-Key\n",
        "        value: \"{{apiToken}}\"\n",
        "        placement: query\n",
        "        extra: kept\n",
        "  - info: { name: Numeric, type: http }\n",
        "    http:\n",
        "      method: GET\n",
        "      url: https://example.com/numeric\n",
        "      auth:\n",
        "        type: apikey\n",
        "        key: api_key\n",
        "        value: 123\n",
        "        placement: header\n",
        "  - info: { name: Cookie, type: http }\n",
        "    http:\n",
        "      method: GET\n",
        "      url: https://example.com/cookie\n",
        "      auth:\n",
        "        type: apikey\n",
        "        key: session\n",
        "        value: secret\n",
        "        placement: cookie\n",
        "  - info: { name: Token, type: http }\n",
        "    http:\n",
        "      method: GET\n",
        "      url: https://example.com/oauth\n",
        "      auth:\n",
        "        type: oauth2\n",
        "        flow: client_credentials\n",
    );
    let parsed = parse(source).unwrap();
    let diagnostics: Vec<_> = parsed
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.path.as_str(),
                diagnostic.kind,
                diagnostic.value.as_str(),
            )
        })
        .collect();
    assert!(diagnostics.contains(&(
        "items/0/http/auth/extra",
        ProjectionDiagnosticKind::AuthenticationProperty,
        "extra",
    )));
    assert!(diagnostics.contains(&(
        "items/1/http/auth/value",
        ProjectionDiagnosticKind::AuthenticationProperty,
        "value",
    )));
    assert!(diagnostics.contains(&(
        "items/2/http/auth/placement",
        ProjectionDiagnosticKind::AuthenticationProperty,
        "placement",
    )));
    assert!(
        !diagnostics
            .iter()
            .any(|(path, _, _)| *path == "items/0/http/auth/placement"
                || *path == "items/1/http/auth/placement")
    );
    assert!(diagnostics.contains(&(
        "items/3/http/auth/type",
        ProjectionDiagnosticKind::AuthenticationKind,
        "oauth2",
    )));
    assert!(!diagnostics.iter().any(|(_, kind, value)| *kind
        == ProjectionDiagnosticKind::AuthenticationKind
        && *value == "apikey"));

    let CollectionItem::Request(request) = &parsed.collection().items[0] else {
        panic!("first item should be a request");
    };
    let auth = request.authentication.as_ref().unwrap();
    assert_eq!(auth.kind, AuthenticationKind::ApiKey);
    assert_eq!(
        auth.properties.get("placement"),
        Some(&AuthenticationValue::String("query".to_owned()))
    );
    assert_eq!(
        auth.properties.get("extra"),
        Some(&AuthenticationValue::String("kept".to_owned()))
    );
    let CollectionItem::Request(numeric) = &parsed.collection().items[1] else {
        panic!("second item should be a request");
    };
    assert_eq!(
        numeric
            .authentication
            .as_ref()
            .unwrap()
            .properties
            .get("value"),
        Some(&AuthenticationValue::Number("123".to_owned()))
    );
    let CollectionItem::Request(cookie) = &parsed.collection().items[2] else {
        panic!("third item should be a request");
    };
    assert_eq!(
        cookie
            .authentication
            .as_ref()
            .unwrap()
            .properties
            .get("placement"),
        Some(&AuthenticationValue::String("cookie".to_owned()))
    );
    let serialized = parsed.to_yaml().unwrap();
    assert_eq!(
        parse(&serialized).unwrap().collection(),
        parsed.collection()
    );
}

#[test]
fn loads_and_indexes_more_than_one_thousand_requests() {
    let parsed = parse(&fixture("phase2-large-workspace.yml"))
        .expect("large workspace fixture should parse");
    let workspace = Workspace::from_collection(parsed.into_collection());

    assert_eq!(workspace.request_count(), 1_001);
    assert_eq!(workspace.root_items().len(), 1_001);
    let Some(WorkspaceItemRef::Request(last_request)) = workspace.root_items().last() else {
        panic!("last root item should be a request");
    };
    assert_eq!(
        workspace
            .request(*last_request)
            .and_then(|request| request.metadata.name.as_deref()),
        Some("Request 1000")
    );
}

#[test]
fn rejects_missing_or_unsupported_collection_headers() {
    for source in [
        "info: { name: Missing version }\nbundled: true\n",
        "opencollection: 1.0.0\nbundled: true\n",
        "opencollection: 1.0.0\ninfo: { name: Missing mode }\n",
        "opencollection: 999.0.0\ninfo: { name: Future }\nbundled: true\n",
    ] {
        assert!(parse(source).is_err(), "unexpectedly accepted:\n{source}");
    }
}

#[test]
fn rejects_invalid_environment_inheritance_during_parse() {
    let source = concat!(
        "opencollection: 1.0.0\n",
        "info: { name: Invalid environments }\n",
        "bundled: true\n",
        "config:\n",
        "  environments:\n",
        "    - { name: development, extends: missing }\n",
    );

    assert!(parse(source).is_err());
}

#[test]
fn loads_documentation_without_flattening_objects_or_request_docs() {
    let source = concat!(
        "opencollection: 1.0.0\n",
        "info:\n",
        "  name: Docs\n",
        "  summary: short\n",
        "  description: collection has no description field\n",
        "bundled: true\n",
        "docs:\n",
        "  content: Collection guide\n",
        "  type: text/markdown\n",
        "items:\n",
        "  - info:\n",
        "      name: Pets\n",
        "      type: folder\n",
        "      description:\n",
        "        content: Pet folder\n",
        "        type: text/plain\n",
        "    docs: null\n",
        "    items:\n",
        "      - info:\n",
        "          name: Create pet\n",
        "          type: http\n",
        "          description:\n",
        "            content: Creates a pet\n",
        "            type: text/markdown\n",
        "        docs: request docs stay a string\n",
        "        http:\n",
        "          method: POST\n",
        "          url: https://example.com/pets\n",
    );
    let parsed = parse(source).expect("documentation fixture should parse");
    let collection = parsed.collection();
    assert_eq!(collection.metadata.summary.as_deref(), Some("short"));
    assert_eq!(
        collection.metadata.docs,
        Some(Documentation::Content {
            content: "Collection guide".into(),
            media_type: "text/markdown".into(),
        })
    );
    let CollectionItem::Folder(folder) = &collection.items[0] else {
        panic!("first item should be a folder");
    };
    assert_eq!(
        folder.metadata.description,
        Some(Documentation::Content {
            content: "Pet folder".into(),
            media_type: "text/plain".into(),
        })
    );
    assert_eq!(folder.docs, Some(Documentation::Null));
    let CollectionItem::Request(request) = &folder.items[0] else {
        panic!("folder child should be a request");
    };
    assert_eq!(
        request.metadata.description,
        Some(Documentation::Content {
            content: "Creates a pet".into(),
            media_type: "text/markdown".into(),
        })
    );
    assert_eq!(request.docs.as_deref(), Some("request docs stay a string"));

    let retained: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&parsed.to_yaml().unwrap()).unwrap();
    assert_eq!(
        retained["info"]["description"].as_str(),
        Some("collection has no description field")
    );
}

#[test]
fn request_docs_are_strings_for_http_and_graphql() {
    for protocol in ["http", "graphql"] {
        let source = fixture("documentation.yml")
            .replace("type: http", &format!("type: {protocol}"))
            .replace("        http:", &format!("        {protocol}:"));
        for (docs, expected) in [
            ("null", Documentation::Null),
            (
                "{content: Guide, type: text/plain}",
                Documentation::Content {
                    content: "Guide".into(),
                    media_type: "text/plain".into(),
                },
            ),
        ] {
            let docs: serde_yaml_ng::Value = serde_yaml_ng::from_str(docs).unwrap();
            let mut document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source).unwrap();
            document["docs"] = docs.clone();
            document["items"][0]["docs"] = docs.clone();
            let valid_source = serde_yaml_ng::to_string(&document).unwrap();
            let parsed = parse(&valid_source)
                .expect("collection and folder docs should accept Documentation");
            let collection = parsed.collection();
            assert_eq!(collection.metadata.docs.as_ref(), Some(&expected));
            let CollectionItem::Folder(folder) = &collection.items[0] else {
                panic!("first item should be a folder");
            };
            assert_eq!(folder.docs.as_ref(), Some(&expected));
            let CollectionItem::Request(request) = &folder.items[0] else {
                panic!("folder child should be a request");
            };
            assert_eq!(request.docs.as_deref(), Some("request docs stay a string"));

            document["items"][0]["items"][0]["docs"] = docs;
            let invalid_source = serde_yaml_ng::to_string(&document).unwrap();
            let error = parse(&invalid_source)
                .expect_err("request docs must reject object and null values");
            assert!(
                error.to_string().contains("request docs must be a string"),
                "{error}\n{invalid_source}"
            );
        }
    }
}
