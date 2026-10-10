use probe_core::{
    Body, FieldPatch, GraphqlRequestError, GraphqlUpdate, Header, QueryParameter, RawBody,
    RawBodyKind, Request, RequestBody, RequestDiffError, RequestKind, RequestProtocol,
    RequestUpdate, VariableUsage, WebSocketMessage, WebSocketMessageKind, WebSocketMessageSet,
    WebSocketMessageVariant, discover_request_variables,
};

fn message(kind: WebSocketMessageKind, data: &str) -> WebSocketMessage {
    WebSocketMessage {
        kind,
        data: data.to_owned(),
    }
}

fn websocket(message: Option<WebSocketMessageSet>) -> Request {
    Request {
        url: Some("wss://{{host}}/socket".to_owned()),
        headers: vec![Header {
            name: "X-Token".to_owned(),
            value: "{{token}}".to_owned(),
            disabled: false,
        }],
        kind: RequestKind::WebSocket { message },
        ..Request::default()
    }
}

fn variants(selected: [bool; 2]) -> WebSocketMessageSet {
    WebSocketMessageSet::Variants(vec![
        WebSocketMessageVariant {
            title: "Hello".to_owned(),
            selected: selected[0],
            message: message(WebSocketMessageKind::Text, "hello"),
        },
        WebSocketMessageVariant {
            title: "Ping".to_owned(),
            selected: selected[1],
            message: message(WebSocketMessageKind::Json, r#"{"ping":1}"#),
        },
    ])
}

#[test]
fn diff_round_trips_websocket_message_and_common_fields() {
    let base = websocket(Some(WebSocketMessageSet::Single(message(
        WebSocketMessageKind::Text,
        "hello",
    ))));
    assert!(
        RequestUpdate::between(Some(&base), &base)
            .unwrap()
            .is_empty()
    );

    let mut current = base.clone();
    current.url = Some("wss://{{host}}/v2?replay=true".to_owned());
    current.kind = RequestKind::WebSocket {
        message: Some(WebSocketMessageSet::Single(message(
            WebSocketMessageKind::Binary,
            "AAEC",
        ))),
    };
    let update = RequestUpdate::between(Some(&base), &current).unwrap();
    assert_eq!(
        update.websocket_message,
        FieldPatch::Set(message(WebSocketMessageKind::Binary, "AAEC"))
    );
    assert!(update.method.is_unchanged());
    assert!(update.body.is_unchanged());
    assert!(update.graphql.is_none());
    assert!(update.query_parameters.is_none());
    let mut restored = base.clone();
    update.apply(&mut restored).unwrap();
    assert_eq!(restored, current);

    let mut cleared = base.clone();
    cleared.kind = RequestKind::WebSocket { message: None };
    let update = RequestUpdate::between(Some(&base), &cleared).unwrap();
    assert_eq!(update.websocket_message, FieldPatch::Clear);
    let mut restored = base;
    update.apply(&mut restored).unwrap();
    assert_eq!(restored, cleared);
}

#[test]
fn message_updates_target_only_the_selected_variant() {
    let base = websocket(Some(variants([false, true])));
    let mut current = base.clone();
    let RequestKind::WebSocket {
        message: Some(WebSocketMessageSet::Variants(messages)),
    } = &mut current.kind
    else {
        unreachable!();
    };
    messages[1].message.data = r#"{"ping":2}"#.to_owned();

    let update = RequestUpdate::between(Some(&base), &current).unwrap();
    let mut restored = base.clone();
    update.apply(&mut restored).unwrap();
    assert_eq!(restored, current);

    let mut retitled = base.clone();
    let RequestKind::WebSocket {
        message: Some(WebSocketMessageSet::Variants(messages)),
    } = &mut retitled.kind
    else {
        unreachable!();
    };
    messages[0].title = "Renamed".to_owned();
    assert_eq!(
        RequestUpdate::between(Some(&base), &retitled),
        Err(RequestDiffError::UnsupportedChange(
            "WebSocket message variants"
        ))
    );
    assert_eq!(
        RequestUpdate::between(None, &base),
        Err(RequestDiffError::UnsupportedChange(
            "WebSocket message variants"
        ))
    );

    for (selection, error) in [
        ([false, false], "no selected value"),
        ([true, true], "multiple selected values"),
    ] {
        let mut ambiguous = websocket(Some(variants(selection)));
        assert!(matches!(
            ambiguous.selected_websocket_message(),
            Err(GraphqlRequestError::InvalidBodySelection(message)) if message.contains(error)
        ));
        let original = ambiguous.clone();
        let update = RequestUpdate {
            name: Some("Renamed".to_owned()),
            websocket_message: FieldPatch::Set(message(WebSocketMessageKind::Text, "x")),
            ..RequestUpdate::default()
        };
        assert!(matches!(
            update.apply(&mut ambiguous),
            Err(GraphqlRequestError::InvalidBodySelection(message)) if message.contains(error)
        ));
        assert_eq!(ambiguous, original);
    }
}

#[test]
fn unchanged_variants_without_one_selected_message_do_not_block_other_edits() {
    for selection in [[false, false], [true, true]] {
        let base = websocket(Some(variants(selection)));
        let mut current = base.clone();
        current.url = Some("wss://{{host}}/v2".to_owned());

        let update = RequestUpdate::between(Some(&base), &current).unwrap();
        assert!(update.websocket_message.is_unchanged());
        let mut restored = base;
        update.apply(&mut restored).unwrap();
        assert_eq!(restored, current);
    }
}

#[test]
fn new_websocket_requests_diff_without_http_only_fields() {
    let current = websocket(Some(WebSocketMessageSet::Single(message(
        WebSocketMessageKind::Json,
        "{}",
    ))));
    let update = RequestUpdate::between(None, &current).unwrap();
    assert_eq!(update.query_parameters, Some(Vec::new()));
    assert!(update.method.is_unchanged());
    let mut created = websocket(None);
    created.url = None;
    created.headers.clear();
    update.apply(&mut created).unwrap();
    assert_eq!(created, current);
}

#[test]
fn updates_reject_fields_outside_the_request_protocol() {
    let parameter = QueryParameter {
        name: "replay".to_owned(),
        value: "true".to_owned(),
        disabled: false,
    };
    let body = Body::Raw(RawBody {
        kind: RawBodyKind::Text,
        data: "x".to_owned(),
    });
    for (update, field) in [
        (
            RequestUpdate {
                method: FieldPatch::Set("GET".to_owned()),
                ..RequestUpdate::default()
            },
            "an HTTP method",
        ),
        (
            RequestUpdate {
                query_parameters: Some(vec![parameter.clone()]),
                ..RequestUpdate::default()
            },
            "query parameters",
        ),
        (
            RequestUpdate {
                path_parameters: Some(vec![parameter.clone()]),
                ..RequestUpdate::default()
            },
            "path parameters",
        ),
        (
            RequestUpdate {
                body: FieldPatch::Set(RequestBody::Single(body.clone())),
                ..RequestUpdate::default()
            },
            "an HTTP body",
        ),
        (
            RequestUpdate {
                body_content: FieldPatch::Set(body.clone()),
                ..RequestUpdate::default()
            },
            "an HTTP body",
        ),
    ] {
        let mut request = websocket(None);
        let error = update.apply(&mut request).unwrap_err();
        assert_eq!(
            error,
            GraphqlRequestError::UnsupportedField {
                protocol: RequestProtocol::WebSocket,
                field,
            }
        );
        assert_eq!(
            error.to_string(),
            format!("native WebSocket requests do not support {field}")
        );
        assert_eq!(request, websocket(None));
    }

    let graphql = RequestUpdate {
        graphql: Some(GraphqlUpdate {
            query: FieldPatch::Set("{ viewer }".to_owned()),
            ..GraphqlUpdate::default()
        }),
        ..RequestUpdate::default()
    };
    assert_eq!(
        graphql.apply(&mut websocket(None)),
        Err(GraphqlRequestError::NotGraphql)
    );
    let message_update = RequestUpdate {
        websocket_message: FieldPatch::Set(message(WebSocketMessageKind::Text, "x")),
        ..RequestUpdate::default()
    };
    assert!(!message_update.is_empty());
    for kind in [
        RequestKind::Http { body: None },
        RequestKind::Graphql { body: None },
    ] {
        let mut request = Request {
            kind,
            ..Request::default()
        };
        assert_eq!(
            message_update.apply(&mut request),
            Err(GraphqlRequestError::NotWebSocket)
        );
    }

    let http = Request::default();
    assert_eq!(
        RequestUpdate::between(Some(&http), &websocket(None)),
        Err(RequestDiffError::UnsupportedChange("request protocol"))
    );
}

#[test]
fn reconciliation_treats_the_websocket_message_as_one_protocol_unit() {
    let baseline = websocket(Some(WebSocketMessageSet::Single(message(
        WebSocketMessageKind::Text,
        "hello",
    ))));
    let mut local = baseline.clone();
    local.kind = RequestKind::WebSocket {
        message: Some(WebSocketMessageSet::Single(message(
            WebSocketMessageKind::Text,
            "local",
        ))),
    };
    let mut incoming = baseline.clone();
    incoming.url = Some("wss://{{host}}/incoming".to_owned());

    let (merged, conflicts) = Request::reconcile(&baseline, &local, &incoming);
    assert!(conflicts.is_empty());
    assert_eq!(merged.url, incoming.url);
    assert_eq!(merged.kind, local.kind);

    incoming.kind = RequestKind::WebSocket { message: None };
    let (merged, conflicts) = Request::reconcile(&baseline, &local, &incoming);
    assert_eq!(conflicts, ["body"]);
    assert_eq!(merged.kind, baseline.kind);
    assert_eq!(merged.url, incoming.url);
}

#[test]
fn websocket_requests_are_not_prepared_for_http_execution() {
    let error = websocket(None).into_http().unwrap_err();
    assert_eq!(
        error,
        GraphqlRequestError::UnsupportedExecution(RequestProtocol::WebSocket)
    );
    assert_eq!(
        error.to_string(),
        "native WebSocket requests cannot be executed yet"
    );
}

#[test]
fn variable_discovery_reports_websocket_message_usages() {
    let request = websocket(Some(WebSocketMessageSet::Variants(vec![
        WebSocketMessageVariant {
            title: "First".to_owned(),
            selected: true,
            message: message(WebSocketMessageKind::Json, r#"{"channel":"{{channel}}"}"#),
        },
        WebSocketMessageVariant {
            title: "Second".to_owned(),
            selected: false,
            message: message(WebSocketMessageKind::Text, "{{other}}"),
        },
    ])));
    let variables = discover_request_variables(&request, &[], None).unwrap();
    let usages = |name: &str| {
        variables
            .iter()
            .find(|variable| variable.name == name)
            .map(|variable| variable.usages.clone())
            .unwrap()
    };
    assert_eq!(usages("channel"), [VariableUsage::WebSocketMessage]);
    assert_eq!(usages("other"), [VariableUsage::WebSocketMessage]);
    assert_eq!(usages("host"), [VariableUsage::Url]);
    assert_eq!(
        usages("token"),
        [VariableUsage::Header {
            name: "X-Token".to_owned()
        }]
    );
}
