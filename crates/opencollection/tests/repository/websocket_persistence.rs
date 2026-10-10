use std::time::Duration;

use probe_core::{
    GraphqlRequestError, WebSocketMessage, WebSocketMessageKind, WebSocketMessageSet,
};
use probe_opencollection::ItemLocator;

use super::*;

fn yaml(path: &std::path::Path) -> serde_yaml_ng::Value {
    serde_yaml_ng::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn request<'a>(loaded: &'a probe_opencollection::LoadedWorkspace, selector: &str) -> &'a Request {
    loaded
        .workspace()
        .request(loaded.request_key(selector).unwrap())
        .unwrap()
}

fn message(kind: WebSocketMessageKind, data: &str) -> WebSocketMessage {
    WebSocketMessage {
        kind,
        data: data.to_owned(),
    }
}

#[test]
fn bundled_websocket_edits_save_reload_and_preserve_unmodeled_yaml() {
    let path = temporary_path("websocket-bundled.yml");
    fs::copy(fixture("websocket.yml"), &path).unwrap();
    let before = yaml(&path);
    let mut loaded = load_workspace(&path).unwrap();
    let base = request(&loaded, "items/0").clone();

    let mut edited = base.clone();
    edited.url = Some("wss://{{host}}/v2/events?replay=true".to_owned());
    edited.headers.push(Header {
        name: "X-Trace".to_owned(),
        value: "{{token}}".to_owned(),
        disabled: false,
    });
    edited.authentication = None;
    edited.docs = Some("Updated docs.".to_owned());
    edited.kind = probe_core::RequestKind::WebSocket {
        message: Some(WebSocketMessageSet::Single(message(
            WebSocketMessageKind::Text,
            "ping {{channel}}",
        ))),
    };
    let update = RequestUpdate::between(Some(&base), &edited).unwrap();
    assert!(update.method.is_unchanged());
    assert!(update.query_parameters.is_none());
    loaded.update_request("items/0", &update).unwrap();
    assert_eq!(request(&loaded, "items/0"), &edited);

    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(request(&reloaded, "items/0"), &edited);
    assert!(reloaded.diagnostics().iter().all(|d| d.value == "cbor"));

    let after = yaml(&path);
    let saved = &after["items"][0];
    assert_eq!(saved["info"]["type"], "websocket");
    assert_eq!(saved["info"]["tags"], before["items"][0]["info"]["tags"]);
    assert_eq!(saved["runtime"], before["items"][0]["runtime"]);
    assert_eq!(saved["settings"], before["items"][0]["settings"]);
    assert_eq!(saved["x-vendor"], "retained");
    assert_eq!(
        saved["websocket"]["url"],
        "wss://{{host}}/v2/events?replay=true"
    );
    assert!(saved["websocket"].get("method").is_none());
    assert!(saved["websocket"].get("params").is_none());
    assert!(saved["websocket"].get("auth").is_none());
    assert_eq!(saved["websocket"]["message"]["type"], "text");
    assert_eq!(saved["websocket"]["message"]["data"], "ping {{channel}}");
    assert!(saved.get("http").is_none());
    for index in 1..4 {
        assert_eq!(
            after["items"][index], before["items"][index],
            "item {index}"
        );
    }
}

#[test]
fn websocket_message_variant_edits_keep_titles_selection_and_unknown_variants() {
    let path = temporary_path("websocket-variants.yml");
    fs::copy(fixture("websocket.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let base = request(&loaded, "items/1").clone();
    let mut edited = base.clone();
    let probe_core::RequestKind::WebSocket {
        message: Some(WebSocketMessageSet::Variants(variants)),
    } = &mut edited.kind
    else {
        panic!("fixture should contain WebSocket message variants");
    };
    variants[1].message = message(WebSocketMessageKind::Json, r#"{"ping":true}"#);

    let update = RequestUpdate::between(Some(&base), &edited).unwrap();
    assert_eq!(
        update.websocket_message,
        FieldPatch::Set(message(WebSocketMessageKind::Json, r#"{"ping":true}"#))
    );
    loaded.update_request("items/1", &update).unwrap();

    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(request(&reloaded, "items/1"), &edited);
    let saved = yaml(&path);
    let messages = saved["items"][1]["websocket"]["message"]
        .as_sequence()
        .unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["title"], "Greeting");
    assert_eq!(messages[1]["title"], "Payload");
    assert_eq!(messages[1]["selected"], true);
    assert_eq!(messages[1]["message"]["type"], "json");
    assert_eq!(messages[2]["message"]["type"], "cbor");

    let mut cleared = request(&reloaded, "items/1").clone();
    cleared.kind = probe_core::RequestKind::WebSocket { message: None };
    let mut reloaded = reloaded;
    reloaded
        .update_request(
            "items/1",
            &RequestUpdate::between(Some(request(&reloaded, "items/1")), &cleared).unwrap(),
        )
        .unwrap();
    assert!(
        yaml(&path)["items"][1]["websocket"]
            .get("message")
            .is_none()
    );
    assert_eq!(
        request(&load_workspace(&path).unwrap(), "items/1"),
        &cleared
    );
}

#[test]
fn url_edits_save_when_the_selected_message_variant_is_unknown_or_missing() {
    for (name, selected) in [("unknown", [false, true]), ("missing", [false, false])] {
        let path = temporary_path(&format!("websocket-selection-{name}.yml"));
        fs::write(
            &path,
            format!(
                r"opencollection: 1.0.0
info:
  name: Sockets
bundled: true
items:
  - info:
      name: Variants
      type: websocket
    websocket:
      url: wss://example.test/socket
      message:
        - title: Greeting
          selected: {}
          message:
            type: text
            data: hello
        - title: Future
          selected: {}
          message:
            type: cbor
            data: oQ==
",
                selected[0], selected[1]
            ),
        )
        .unwrap();
        let mut loaded = load_workspace(&path).unwrap();
        let base = request(&loaded, "items/0").clone();
        let mut edited = base.clone();
        edited.url = Some("wss://example.test/v2".to_owned());

        let update = RequestUpdate::between(Some(&base), &edited).unwrap();
        assert!(update.websocket_message.is_unchanged(), "{name}");
        loaded.update_request("items/0", &update).unwrap();

        let reloaded = load_workspace(&path).unwrap();
        assert_eq!(request(&reloaded, "items/0"), &edited, "{name}");
        let saved = yaml(&path);
        let messages = saved["items"][0]["websocket"]["message"]
            .as_sequence()
            .unwrap();
        assert_eq!(messages[1]["selected"], selected[1], "{name}");
        assert_eq!(messages[1]["message"]["type"], "cbor", "{name}");
        assert_eq!(messages[1]["message"]["data"], "oQ==", "{name}");
    }
}

#[test]
fn websocket_rejects_fields_outside_its_opencollection_shape_without_writing() {
    let path = temporary_path("websocket-rejects.yml");
    fs::copy(fixture("websocket.yml"), &path).unwrap();
    let original = fs::read(&path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let parameter = QueryParameter {
        name: "replay".to_owned(),
        value: "true".to_owned(),
        disabled: false,
    };
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
                body: FieldPatch::Clear,
                ..RequestUpdate::default()
            },
            "an HTTP body",
        ),
    ] {
        let error = loaded.update_request("items/0", &update).unwrap_err();
        assert!(
            matches!(
                error,
                SaveError::Graphql(GraphqlRequestError::UnsupportedField {
                    protocol: RequestProtocol::WebSocket,
                    field: rejected,
                }) if rejected == field
            ),
            "{field}: {error:?}"
        );
    }
    let error = loaded
        .update_request(
            "items/2",
            &RequestUpdate {
                websocket_message: FieldPatch::Set(message(WebSocketMessageKind::Text, "x")),
                ..RequestUpdate::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        SaveError::Graphql(GraphqlRequestError::NotWebSocket)
    ));
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn unbundled_websocket_edits_save_reload_and_preserve_unknown_fields() {
    let root = temporary_path("websocket-unbundled");
    copy_directory(&fixture("websocket-unbundled"), &root);
    let health = fs::read(root.join("health.yml")).unwrap();
    let mut loaded = load_workspace(&root).unwrap();
    assert!(loaded.diagnostics().is_empty());
    let base = request(&loaded, "socket.yml").clone();
    assert_eq!(base.kind.as_str(), "websocket");
    assert_eq!(
        base.selected_websocket_message().unwrap(),
        Some(&message(WebSocketMessageKind::Binary, "AAEC"))
    );
    assert_eq!(
        base.settings.keep_alive_interval,
        Some(Duration::from_secs(10))
    );

    let mut edited = base.clone();
    edited.headers[0].value = "Bearer {{otherToken}}".to_owned();
    edited.kind = probe_core::RequestKind::WebSocket {
        message: Some(WebSocketMessageSet::Single(message(
            WebSocketMessageKind::Binary,
            "AAECAw==",
        ))),
    };
    loaded
        .update_request(
            "socket.yml",
            &RequestUpdate::between(Some(&base), &edited).unwrap(),
        )
        .unwrap();

    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(request(&reloaded, "socket.yml"), &edited);
    let saved = yaml(&root.join("socket.yml"));
    assert_eq!(saved["x-socket"], "retained");
    assert_eq!(saved["settings"]["keepAliveInterval"], 10000);
    assert_eq!(saved["websocket"]["message"]["data"], "AAECAw==");
    assert_eq!(fs::read(root.join("health.yml")).unwrap(), health);
}

#[test]
fn websocket_structure_operations_create_rename_and_reorder() {
    let path = temporary_path("websocket-create.yml");
    fs::copy(fixture("websocket.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let error = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: None,
            index: None,
            name: "Invalid".to_owned(),
            method: Some("GET".to_owned()),
            url: None,
            protocol: RequestProtocol::WebSocket,
            graphql: None,
            update: None,
        })
        .unwrap_err();
    assert!(matches!(error, StructureError::InvalidDocument(_)));
    let error = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: None,
            index: None,
            name: "Invalid".to_owned(),
            method: None,
            url: None,
            protocol: RequestProtocol::WebSocket,
            graphql: None,
            update: Some(RequestUpdate {
                query_parameters: Some(vec![QueryParameter {
                    name: "replay".to_owned(),
                    value: "true".to_owned(),
                    disabled: false,
                }]),
                ..RequestUpdate::default()
            }),
        })
        .unwrap_err();
    assert!(
        matches!(&error, StructureError::InvalidDocument(message) if message.contains("query parameters")),
        "{error:?}"
    );
    let error = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: None,
            index: None,
            name: "Invalid HTTP".to_owned(),
            method: Some("GET".to_owned()),
            url: None,
            protocol: RequestProtocol::Http,
            graphql: None,
            update: Some(RequestUpdate {
                websocket_message: FieldPatch::Set(message(WebSocketMessageKind::Text, "x")),
                ..RequestUpdate::default()
            }),
        })
        .unwrap_err();
    assert!(
        matches!(&error, StructureError::InvalidDocument(message) if message.contains("not a native WebSocket request")),
        "{error:?}"
    );
    assert_eq!(
        load_workspace(&path).unwrap().workspace().request_count(),
        4
    );
    let created = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: None,
            index: None,
            name: "Created socket".to_owned(),
            method: None,
            url: Some("wss://{{host}}/created".to_owned()),
            protocol: RequestProtocol::WebSocket,
            graphql: None,
            update: Some(RequestUpdate {
                query_parameters: Some(Vec::new()),
                websocket_message: FieldPatch::Set(message(WebSocketMessageKind::Json, "{}")),
                ..RequestUpdate::default()
            }),
        })
        .unwrap();
    let selector = created.selector.unwrap();
    let reloaded = load_workspace(&path).unwrap();
    let created = request(&reloaded, &selector);
    assert_eq!(created.kind.as_str(), "websocket");
    assert_eq!(created.method, None);
    assert_eq!(
        created.selected_websocket_message().unwrap(),
        Some(&message(WebSocketMessageKind::Json, "{}"))
    );
    let saved = yaml(&path);
    let item = saved["items"].as_sequence().unwrap().last().unwrap();
    assert_eq!(item["info"]["type"], "websocket");
    assert!(item["websocket"].get("method").is_none());
    assert!(item["websocket"].get("params").is_none());

    let root = temporary_path("websocket-unbundled-structure");
    copy_directory(&fixture("websocket-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();
    let renamed = loaded
        .apply_structure(StructureOperation::Rename {
            target: ItemLocator::new(probe_core::ItemKind::Request, "socket.yml"),
            name: "Renamed socket".to_owned(),
        })
        .unwrap();
    assert_eq!(renamed.selector.as_deref(), Some("renamed-socket.yml"));
    loaded
        .apply_structure(StructureOperation::Reorder {
            target: ItemLocator::new(probe_core::ItemKind::Request, "renamed-socket.yml"),
            index: 1,
        })
        .unwrap();
    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded
            .requests()
            .iter()
            .map(|request| request.selector())
            .collect::<Vec<_>>(),
        ["health.yml", "renamed-socket.yml"]
    );
    let saved = yaml(&root.join("renamed-socket.yml"));
    assert_eq!(saved["info"]["name"], "Renamed socket");
    assert_eq!(saved["info"]["seq"], 2);
    assert_eq!(saved["x-socket"], "retained");
}

#[test]
fn created_request_settings_contain_only_keys_valid_for_the_protocol() {
    let path = temporary_path("websocket-settings.yml");
    let settings = probe_core::RequestSettings {
        timeout: Some(Duration::from_millis(1500)),
        follow_redirects: Some(false),
        max_redirects: Some(3),
        keep_alive_interval: Some(Duration::from_secs(30)),
    };
    let item = |kind| {
        CollectionItem::Request(Request {
            settings: settings.clone(),
            kind,
            ..Request::default()
        })
    };
    let collection = Collection {
        items: vec![
            item(probe_core::RequestKind::Http { body: None }),
            item(probe_core::RequestKind::Graphql { body: None }),
            item(probe_core::RequestKind::WebSocket { message: None }),
        ],
        ..Collection::default()
    };

    create_bundled_workspace_from_collection(&path, &collection).unwrap();
    let saved = yaml(&path);
    let keys = |index: usize| {
        saved["items"][index]["settings"]
            .as_mapping()
            .unwrap()
            .keys()
            .map(|key| key.as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    for index in [0, 1] {
        assert_eq!(
            keys(index),
            ["timeout", "followRedirects", "maxRedirects"],
            "item {index}"
        );
    }
    assert_eq!(keys(2), ["timeout", "keepAliveInterval"]);
    assert_eq!(saved["items"][2]["settings"]["timeout"], 1500.0);
    assert_eq!(saved["items"][2]["settings"]["keepAliveInterval"], 30000.0);
}

#[test]
fn bundled_creation_from_a_domain_collection_round_trips_websocket_requests() {
    let path = temporary_path("websocket-domain.yml");
    let socket = Request {
        metadata: ItemMetadata {
            name: Some("Socket".to_owned()),
            ..ItemMetadata::default()
        },
        url: Some("wss://{{host}}/socket".to_owned()),
        headers: vec![Header {
            name: "X-Token".to_owned(),
            value: "{{token}}".to_owned(),
            disabled: false,
        }],
        authentication: Some(Authentication {
            kind: AuthenticationKind::Bearer,
            properties: [(
                "token".to_owned(),
                AuthenticationValue::String("{{token}}".to_owned()),
            )]
            .into(),
        }),
        settings: probe_core::RequestSettings {
            timeout: Some(Duration::from_millis(2500)),
            keep_alive_interval: Some(Duration::from_secs(15)),
            ..probe_core::RequestSettings::default()
        },
        kind: probe_core::RequestKind::WebSocket {
            message: Some(WebSocketMessageSet::Variants(vec![
                probe_core::WebSocketMessageVariant {
                    title: "Hello".to_owned(),
                    selected: true,
                    message: message(WebSocketMessageKind::Text, "hello"),
                },
                probe_core::WebSocketMessageVariant {
                    title: "Bytes".to_owned(),
                    selected: false,
                    message: message(WebSocketMessageKind::Binary, "AA=="),
                },
            ])),
        },
        ..Request::default()
    };
    let collection = Collection {
        metadata: CollectionMetadata {
            name: Some("Sockets".to_owned()),
            ..CollectionMetadata::default()
        },
        items: vec![CollectionItem::Request(socket.clone())],
        ..Collection::default()
    };

    create_bundled_workspace_from_collection(&path, &collection).unwrap();
    let reloaded = load_workspace(&path).unwrap();
    assert!(reloaded.diagnostics().is_empty());
    assert_eq!(request(&reloaded, "items/0"), &socket);
    let saved = yaml(&path);
    assert_eq!(saved["items"][0]["settings"]["keepAliveInterval"], 15000.0);
    assert!(saved["items"][0]["websocket"].get("method").is_none());
}
