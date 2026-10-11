use crate::{ImportDiagnosticSeverity, YaakImportError, YaakSourceFormat, inspect_yaak_source};
use probe_core::{
    AuthenticationKind, Body, CollectionItem, GraphqlBody, MultipartPartKind, MultipartValue,
    RequestBody, RequestSettings, WorkspaceItemRef,
};
use probe_opencollection::create_bundled_workspace_from_collection;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

fn temporary_path(name: &str) -> PathBuf {
    // Parallel tests can observe the same wall-clock timestamp.
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "probe-yaak-{}-{nanos}-{id}-{name}",
        std::process::id()
    ))
}

#[test]
fn converts_export_http_hierarchy_and_environment() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/yaak/export-v4.json");

    let preview = inspect_yaak_source(&path).unwrap();
    assert_eq!(preview.format(), YaakSourceFormat::ExportJson);
    let imported = preview.convert(None, false).unwrap();
    assert_eq!(imported.collection.metadata.name.as_deref(), Some("Pets"));
    assert_eq!(imported.collection.environments[0].name, "Global Variables");
    assert_eq!(
        imported.default_environment.as_deref(),
        Some("Global Variables")
    );
    let CollectionItem::Folder(folder) = &imported.collection.items[0] else {
        panic!("expected folder");
    };
    let CollectionItem::Request(request) = &folder.items[0] else {
        panic!("expected request");
    };
    assert_eq!(request.path_parameters[0].name, "id");
    assert_eq!(request.query_parameters[0].name, "page");
    assert_eq!(request.headers[0].value, "{{TOKEN}}");
    assert_eq!(
        request.settings,
        RequestSettings {
            timeout: Some(std::time::Duration::from_millis(2500)),
            follow_redirects: Some(true),
            ..RequestSettings::default()
        }
    );
    assert_eq!(
        request.authentication.as_ref().unwrap().kind,
        AuthenticationKind::Bearer
    );
    let Some(RequestBody::Single(Body::Raw(body))) = request.http_body() else {
        panic!("expected raw body");
    };
    assert!(body.data.contains("{{TOKEN}}"));
}

#[test]
fn imports_form_file_bodies_and_reports_unsupported_body_types() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/yaak/export-bodies-v4.json");
    let preview = inspect_yaak_source(path).unwrap();
    assert!(matches!(
        preview.convert(None, false),
        Err(YaakImportError::Unsupported(_))
    ));
    let imported = preview.convert(None, true).unwrap();
    assert!(imported.partial);
    assert!(imported.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "unsupported_body_type"
            && diagnostic.resource_id.as_deref() == Some("unsupported")
    }));
    let requests = imported
        .collection
        .items
        .iter()
        .map(|item| {
            let CollectionItem::Request(request) = item else {
                panic!("expected request")
            };
            request
        })
        .collect::<Vec<_>>();
    let named = |name| {
        requests
            .iter()
            .find(|request| request.metadata.name.as_deref() == Some(name))
            .unwrap()
    };
    let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) = named("Form").http_body() else {
        panic!("expected form body")
    };
    assert_eq!(fields[0].value, "{{TOKEN}}");
    assert!(!fields[0].disabled);
    assert!(fields[1].disabled);
    let Some(RequestBody::Single(Body::Multipart(parts))) = named("Multipart").http_body() else {
        panic!("expected multipart body")
    };
    assert_eq!(parts[0].kind, MultipartPartKind::Text);
    assert_eq!(parts[1].kind, MultipartPartKind::File);
    assert_eq!(
        parts[1].value,
        MultipartValue::Single("/tmp/report.txt".to_owned())
    );
    assert_eq!(parts[1].content_type.as_deref(), Some("text/plain"));
    assert!(parts[1].disabled);
    let Some(RequestBody::Single(Body::File(files))) = named("Binary").http_body() else {
        panic!("expected binary file body")
    };
    assert_eq!(files[0].file_path, "/tmp/payload.bin");
    assert!(files[0].selected);
    let Some(RequestBody::Single(Body::Raw(raw))) = named("Unsupported").http_body() else {
        panic!("expected fallback text body")
    };
    assert_eq!(raw.data, "opaque");
}

#[test]
fn selects_the_default_environment_from_converted_scopes() {
    let global_later = temporary_path("default-global.json");
    fs::write(
        &global_later,
        r#"{
  "yaakSchema": 4,
  "resources": {
    "workspaces": [{"model": "workspace", "id": "wk_1", "name": "Scoped"}],
    "environments": [
      {"model": "environment", "id": "en_folder", "workspaceId": "wk_1", "name": "Folder Only", "parentModel": "folder"},
      {"model": "environment", "id": "en_staging", "workspaceId": "wk_1", "name": "Staging", "parentModel": "environment"},
      {"model": "environment", "id": "en_global", "workspaceId": "wk_1", "name": "Global Variables", "parentModel": "workspace"}
    ]
  }
}"#,
    )
    .unwrap();
    let imported = inspect_yaak_source(&global_later)
        .unwrap()
        .convert(None, true)
        .unwrap();
    assert_eq!(
        imported.default_environment.as_deref(),
        Some("Global Variables")
    );
    assert_eq!(
        imported
            .collection
            .environments
            .iter()
            .map(|environment| environment.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Staging", "Global Variables"]
    );
    fs::remove_file(&global_later).unwrap();

    let fallback = temporary_path("default-fallback.json");
    fs::write(
        &fallback,
        r#"{
  "yaakSchema": 4,
  "resources": {
    "workspaces": [{"model": "workspace", "id": "wk_1", "name": "Scoped"}],
    "environments": [
      {"model": "environment", "id": "en_folder", "workspaceId": "wk_1", "name": "Folder Only", "parentModel": "folder"},
      {"model": "environment", "id": "en_staging", "workspaceId": "wk_1", "name": "Staging", "parentModel": "environment"},
      {"model": "environment", "id": "en_prod", "workspaceId": "wk_1", "name": "Production", "parentModel": "environment"}
    ]
  }
}"#,
    )
    .unwrap();
    let imported = inspect_yaak_source(&fallback)
        .unwrap()
        .convert(None, true)
        .unwrap();
    assert_eq!(imported.default_environment.as_deref(), Some("Staging"));
    assert!(
        imported
            .collection
            .environments
            .iter()
            .all(|environment| environment.name != "Folder Only")
    );
    fs::remove_file(&fallback).unwrap();

    let empty = temporary_path("default-none.json");
    fs::write(
        &empty,
        r#"{"yaakSchema":4,"resources":{"workspaces":[{"model":"workspace","id":"wk_1","name":"Empty"}]}}"#,
    )
    .unwrap();
    let imported = inspect_yaak_source(&empty)
        .unwrap()
        .convert(None, false)
        .unwrap();
    assert_eq!(imported.default_environment, None);
    assert!(imported.collection.environments.is_empty());
    fs::remove_file(empty).unwrap();
}

#[test]
fn converts_directory_sync_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/yaak/sync");
    let preview = inspect_yaak_source(path).unwrap();
    assert_eq!(preview.format(), YaakSourceFormat::SyncDirectory);
    let imported = preview.convert(None, false).unwrap();
    assert_eq!(
        imported.collection.metadata.name.as_deref(),
        Some("Sync Pets")
    );
    assert_eq!(imported.collection.environments.len(), 1);
    assert_eq!(imported.collection.items.len(), 1);
}

#[test]
fn accepts_every_supported_export_schema() {
    for schema in 1..=4 {
        let path = temporary_path(&format!("schema-{schema}.json"));
        fs::write(
                &path,
                format!(
                    r#"{{"yaakSchema":{schema},"resources":{{"workspaces":[{{"model":"workspace","id":"wk_{schema}","name":"Schema {schema}"}}]}}}}"#
                ),
            )
            .unwrap();
        let imported = inspect_yaak_source(&path)
            .unwrap()
            .convert(None, false)
            .unwrap();
        assert_eq!(
            imported.collection.metadata.name.as_deref(),
            Some(format!("Schema {schema}").as_str())
        );
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn strict_mode_rejects_lossy_resources_and_partial_reports_them() {
    let path = temporary_path("lossy.json");
    fs::write(
        &path,
        r#"{
  "yaakSchema":4,
  "resources":{
    "workspaces":[{"model":"workspace","id":"wk_1","name":"Mixed"}],
    "grpcRequests":[{"model":"grpc_request","id":"gr_1","workspaceId":"wk_1"}]
  }
}"#,
    )
    .unwrap();
    let preview = inspect_yaak_source(&path).unwrap();
    assert!(matches!(
        preview.convert(None, false),
        Err(YaakImportError::Unsupported(_))
    ));
    let imported = preview.convert(None, true).unwrap();
    assert!(imported.partial);
    assert!(imported.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "unsupported_resource"
            && diagnostic.severity == ImportDiagnosticSeverity::Lossy
    }));
    fs::remove_file(path).unwrap();
}

#[test]
fn sync_directory_requires_valid_relationships() {
    let root = temporary_path("sync");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("yaak.wk_1.yaml"),
        "model: workspace\nid: wk_1\nname: Sync\n",
    )
    .unwrap();
    fs::write(
        root.join("yaak.rq_1.yaml"),
        "model: http_request\nid: rq_1\nworkspaceId: wk_1\nfolderId: missing\nname: Broken\n",
    )
    .unwrap();
    let preview = inspect_yaak_source(&root).unwrap();
    assert!(matches!(
        preview.convert(None, false),
        Err(YaakImportError::Invalid(_))
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sync_directory_allows_partial_import_with_unsupported_resources() {
    let root = temporary_path("sync-partial");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("yaak.wk_1.yaml"),
        "model: workspace\nid: wk_1\nname: Sync\n",
    )
    .unwrap();
    fs::write(
        root.join("yaak.sse_1.yaml"),
        "model: sse_request\nid: sse_1\nworkspaceId: wk_1\nname: Events\n",
    )
    .unwrap();

    let preview = inspect_yaak_source(&root).unwrap();
    assert!(matches!(
        preview.convert(None, false),
        Err(YaakImportError::Unsupported(_))
    ));
    let imported = preview.convert(None, true).unwrap();
    assert!(imported.partial);
    assert!(imported.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "unsupported_resource"
            && diagnostic.resource_type == "sse_request"
            && diagnostic.resource_id.as_deref() == Some("sse_1")
    }));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn imports_native_graphql_requests_and_round_trips() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/yaak/export-graphql-v4.json");
    let imported = inspect_yaak_source(&path)
        .unwrap()
        .convert(None, false)
        .unwrap();
    assert!(!imported.partial);
    assert_eq!(imported.collection.items.len(), 2);

    let CollectionItem::Request(viewer) = &imported.collection.items[0] else {
        panic!("first item should be a GraphQL request");
    };
    assert_eq!(viewer.metadata.name.as_deref(), Some("Viewer"));
    assert_eq!(viewer.headers[0].value, "{{TRACE}}");
    let Some(GraphqlBody::Single(operation)) = viewer.graphql() else {
        panic!("Viewer should have a single GraphQL operation");
    };
    assert_eq!(
        operation.query.as_deref(),
        Some("query Viewer($login: String!) { viewer(login: $login) { login } }")
    );
    assert_eq!(operation.operation_name.as_deref(), Some("Viewer"));
    assert_eq!(
        operation
            .variables
            .as_ref()
            .and_then(|variables| variables.get("login"))
            .and_then(serde_json::Value::as_str),
        Some("{{LOGIN}}")
    );
    assert_eq!(
        operation
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.get("trace"))
            .and_then(serde_json::Value::as_object)
            .and_then(|trace| trace.get("enabled")),
        Some(&serde_json::Value::Bool(true))
    );

    let CollectionItem::Request(legacy) = &imported.collection.items[1] else {
        panic!("second item should be a GraphQL request");
    };
    let Some(GraphqlBody::Single(operation)) = legacy.graphql() else {
        panic!("legacy envelope should have a single GraphQL operation");
    };
    assert_eq!(operation.query.as_deref(), Some("query Pet { pet { id } }"));
    assert_eq!(operation.operation_name.as_deref(), Some("Pet"));
    assert_eq!(
        operation
            .variables
            .as_ref()
            .and_then(|variables| variables.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("{{PET_ID}}")
    );

    let destination = temporary_path("graphql-roundtrip.yml");
    let loaded =
        create_bundled_workspace_from_collection(&destination, &imported.collection).unwrap();
    let workspace = loaded.workspace();
    assert_eq!(workspace.request_count(), 2);
    let WorkspaceItemRef::Request(request_key) = workspace.root_items()[0] else {
        panic!("round-tripped root item should be a request");
    };
    let request = workspace.request(request_key).unwrap();
    assert_eq!(request.kind.as_str(), "graphql");
    assert_eq!(
        request
            .selected_graphql()
            .unwrap()
            .unwrap()
            .operation_name
            .as_deref(),
        Some("Viewer")
    );
    assert!(
        fs::read_to_string(&destination)
            .unwrap()
            .contains("type: graphql")
    );
    fs::remove_file(destination).unwrap();
    let mut document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    document["resources"]["httpRequests"][0]["body"]["futureOption"] = serde_json::json!(true);
    let preview = inspect_websocket_export(document);
    assert!(matches!(
        preview.convert(None, false),
        Err(YaakImportError::Unsupported(_))
    ));
    assert!(
        preview
            .convert(None, true)
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "unknown_field"
                && d.resource_type == "http_request"
                && d.field.as_deref() == Some("futureOption"))
    );
}

#[test]
fn rejects_non_string_graphql_query_and_text() {
    for (body, field) in [
        (r#"{"query":{"nested":true}}"#, "query"),
        (r#"{"text":{"query":"{ pet { id } }"}}"#, "text"),
    ] {
        let path = temporary_path(&format!("invalid-graphql-{field}.json"));
        fs::write(
            &path,
            format!(
                r#"{{
  "yaakSchema": 4,
  "resources": {{
    "workspaces": [{{"model":"workspace","id":"wk_1","name":"Broken"}}],
    "httpRequests": [{{
      "model":"http_request",
      "id":"rq_1",
      "workspaceId":"wk_1",
      "name":"Broken",
      "method":"POST",
      "url":"https://api.example.com/graphql",
      "bodyType":"graphql",
      "body":{body}
    }}]
  }}
}}"#
            ),
        )
        .unwrap();
        let preview = inspect_yaak_source(&path).unwrap();
        assert!(
            matches!(
                preview.convert(None, false),
                Err(YaakImportError::Invalid(message)) if message.contains(field)
            ),
            "non-string GraphQL {field} should be rejected"
        );
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn ignores_whitespace_only_inactive_graphql_text() {
    let path = temporary_path("graphql-whitespace-text.json");
    fs::write(
        &path,
        r#"{
  "yaakSchema": 4,
  "resources": {
    "workspaces": [{"model":"workspace","id":"wk_1","name":"Whitespace"}],
    "httpRequests": [{
      "model":"http_request",
      "id":"rq_1",
      "workspaceId":"wk_1",
      "name":"Viewer",
      "method":"POST",
      "url":"https://api.example.com/graphql",
      "bodyType":"graphql",
      "body":{"query":"{ pet { id } }","text":"   "}
    }]
  }
}"#,
    )
    .unwrap();
    let imported = inspect_yaak_source(&path)
        .unwrap()
        .convert(None, false)
        .unwrap();
    assert!(!imported.partial);
    assert!(
        imported
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code != "inactive_body_data" })
    );
    let CollectionItem::Request(request) = &imported.collection.items[0] else {
        panic!("item should be a GraphQL request");
    };
    let Some(GraphqlBody::Single(operation)) = request.graphql() else {
        panic!("request should have a single GraphQL operation");
    };
    assert_eq!(operation.query.as_deref(), Some("{ pet { id } }"));
    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_malformed_graphql_variables() {
    let path = temporary_path("invalid-graphql.json");
    fs::write(
        &path,
        r#"{
  "yaakSchema": 4,
  "resources": {
    "workspaces": [{"model":"workspace","id":"wk_1","name":"Broken"}],
    "httpRequests": [{
      "model":"http_request",
      "id":"rq_1",
      "workspaceId":"wk_1",
      "name":"Broken",
      "method":"POST",
      "url":"https://api.example.com/graphql",
      "bodyType":"graphql",
      "body":{"query":"{ pet { id } }","variables":"[]"}
    }]
  }
}"#,
    )
    .unwrap();
    let preview = inspect_yaak_source(&path).unwrap();
    assert!(matches!(
        preview.convert(None, false),
        Err(YaakImportError::Invalid(message)) if message.contains("variables")
    ));
    fs::remove_file(path).unwrap();
}

fn websocket_export() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/yaak/export-websocket-v4.json"
    ))
    .unwrap()
}

fn inspect_websocket_export(document: serde_json::Value) -> crate::YaakImportPreview {
    let path = temporary_path("websocket.json");
    fs::write(&path, document.to_string()).unwrap();
    let result = inspect_yaak_source(&path);
    fs::remove_file(path).unwrap();
    result.unwrap()
}

#[test]
fn imports_mixed_websocket_hierarchy_and_round_trips() {
    use probe_core::{AuthenticationValue, WebSocketMessageKind};
    let imported = inspect_websocket_export(websocket_export())
        .convert(None, false)
        .unwrap();
    assert!(!imported.partial);
    assert!(imported.diagnostics.is_empty());
    let CollectionItem::Request(root) = &imported.collection.items[0] else {
        panic!("expected root request")
    };
    assert_eq!(root.metadata.name.as_deref(), Some("Connect"));
    assert_eq!(root.metadata.sequence, Some(1.0));
    assert_eq!(root.url.as_deref(), Some("ws://localhost:8080"));
    assert!(root.kind.is_websocket());
    assert!(root.websocket_message().is_none());
    assert_eq!(
        root.authentication.as_ref().unwrap().kind,
        AuthenticationKind::Bearer
    );
    assert_eq!(
        root.authentication.as_ref().unwrap().properties["token"],
        AuthenticationValue::String("{{TOKEN}}".into())
    );
    let CollectionItem::Folder(outer) = &imported.collection.items[1] else {
        panic!("expected outer folder")
    };
    let CollectionItem::Folder(inner) = &outer.items[0] else {
        panic!("expected nested folder")
    };
    assert_eq!(outer.metadata.sequence, Some(2.0));
    assert_eq!(inner.metadata.sequence, Some(3.0));
    let CollectionItem::Request(socket) = &inner.items[0] else {
        panic!("expected socket")
    };
    let CollectionItem::Request(http) = &inner.items[1] else {
        panic!("expected HTTP")
    };
    assert_eq!(socket.metadata.name.as_deref(), Some("Subscribe"));
    assert_eq!(socket.metadata.sequence, Some(1.5));
    assert_eq!(http.method.as_deref(), Some("GET"));
    assert_eq!(http.metadata.sequence, Some(4.0));
    assert_eq!(socket.url.as_deref(), Some("wss://{{HOST}}/events"));
    assert_eq!(socket.docs.as_deref(), Some("Subscribe to events"));
    assert!(socket.method.is_none());
    assert!(socket.query_parameters.is_empty() && socket.path_parameters.is_empty());
    assert_eq!(socket.headers.len(), 2);
    assert_eq!(socket.headers[0].value, "{{TRACE}}");
    assert!(!socket.headers[0].disabled);
    assert_eq!(socket.headers[1].name, "{{HEADER}}");
    assert_eq!(socket.headers[1].value, "{{VALUE}}");
    assert!(socket.headers[1].disabled);
    let auth = socket.authentication.as_ref().unwrap();
    assert_eq!(auth.kind, AuthenticationKind::Basic);
    assert_eq!(
        auth.properties["username"],
        AuthenticationValue::String("{{USER}}".into())
    );
    assert_eq!(
        auth.properties["password"],
        AuthenticationValue::String("{{PASSWORD}}".into())
    );
    assert_eq!(socket.settings.timeout, None);
    assert_eq!(
        http.settings.timeout,
        Some(std::time::Duration::from_millis(2500))
    );
    assert_eq!(socket.settings.follow_redirects, None);
    let message = socket.selected_websocket_message().unwrap().unwrap();
    assert_eq!(message.kind, WebSocketMessageKind::Text);
    assert_eq!(message.data, r#"{"topic":"{{TOPIC}}"}"#);

    let destination = temporary_path("websocket-roundtrip.yml");
    let loaded =
        create_bundled_workspace_from_collection(&destination, &imported.collection).unwrap();
    assert_eq!(loaded.workspace().request_count(), 3);
    // Reimport the canonical document and compare protocol data, headers, auth, and settings.
    let reloaded = probe_opencollection::parse(&fs::read_to_string(&destination).unwrap()).unwrap();
    assert_eq!(reloaded.collection(), &imported.collection);
    fs::remove_file(destination).unwrap();
}

#[test]
fn websocket_sync_validates_folder_references() {
    let root = temporary_path("websocket-sync");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("workspace.yaml"),
        "model: workspace\nid: wk_1\nname: Sync\nsettingFollowRedirects: false\n",
    )
    .unwrap();
    let request_path = root.join("socket.yaml");
    fs::write(
        &request_path,
        "model: websocket_request\nid: ws_1\nworkspaceId: wk_1\nfolderId: missing\n",
    )
    .unwrap();
    assert!(
        matches!(inspect_yaak_source(&root).unwrap().convert(None, true), Err(YaakImportError::Invalid(message)) if message.contains("missing folder"))
    );
    fs::write(&request_path, "model: websocket_request\nid: ws_1\nworkspaceId: wk_1\nname: Sync socket\nurl: ws://localhost\nmessage: '${[DATA]}'\n").unwrap();
    let imported = inspect_yaak_source(&root)
        .unwrap()
        .convert(None, false)
        .unwrap();
    let CollectionItem::Request(socket) = &imported.collection.items[0] else {
        panic!("expected socket")
    };
    assert_eq!(
        socket.selected_websocket_message().unwrap().unwrap().data,
        "{{DATA}}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn websocket_loss_is_reported_with_field_and_resource_identity() {
    let cases = [
        (
            "urlParameters",
            serde_json::json!([{"name":"page","value":"1"}]),
            "unsupported_field",
        ),
        (
            "settingRequestMessageSize",
            serde_json::json!({"enabled":true,"value":1024}),
            "unsupported_field",
        ),
        (
            "settingValidateCertificates",
            serde_json::json!({"enabled":true,"value":false}),
            "unsupported_setting",
        ),
        (
            "authenticationType",
            serde_json::json!("custom"),
            "unsupported_authentication",
        ),
        (
            "authentication",
            serde_json::json!({"token":"${[TOKEN]}","prefix":"Token"}),
            "unsupported_authentication",
        ),
        (
            "message",
            serde_json::json!("${[response('id')]}"),
            "unsupported_template",
        ),
        (
            "messageVariants",
            serde_json::json!([{"message":"hello"}]),
            "unknown_field",
        ),
    ];
    for (field, value, code) in cases {
        let mut document = websocket_export();
        document["resources"]["websocketRequests"][0][field] = value;
        let diagnostic_field = if field == "authentication" {
            document["resources"]["websocketRequests"][0]["authenticationType"] =
                serde_json::json!("bearer");
            "authentication.prefix"
        } else {
            field
        };
        let preview = inspect_websocket_export(document);
        assert!(
            matches!(
                preview.convert(None, false),
                Err(YaakImportError::Unsupported(_))
            ),
            "{field}"
        );
        let imported = preview.convert(None, true).unwrap();
        assert!(imported.partial);
        assert!(
            imported.diagnostics.iter().any(|d| d.code == code
                && d.resource_type == "websocket_request"
                && d.resource_id.as_deref() == Some("ws_nested")
                && d.field.as_deref() == Some(diagnostic_field)),
            "{field}: {:?}",
            imported.diagnostics
        );
        assert!(
            imported
                .diagnostics
                .iter()
                .all(|d| d.code != "unsupported_resource")
        );
    }
}

#[test]
fn mixed_authentication_overrides_and_order_ties() {
    use probe_core::AuthenticationValue;
    for (scheme, expected, warning, location) in [
        ("none", None, false, Some("query")),
        (
            "basic",
            Some(AuthenticationKind::Basic),
            false,
            Some("query"),
        ),
        (
            "apikey",
            Some(AuthenticationKind::ApiKey),
            false,
            Some("query"),
        ),
        ("apikey", Some(AuthenticationKind::ApiKey), false, None),
        (
            "digest",
            Some(AuthenticationKind::Digest),
            true,
            Some("query"),
        ),
    ] {
        let mut document = websocket_export();
        document["resources"]["httpRequests"][0]["id"] = serde_json::json!("z_http");
        let socket = &mut document["resources"]["websocketRequests"][0];
        socket["id"] = serde_json::json!("a_socket");
        socket["sortPriority"] = serde_json::json!(4);
        socket["authenticationType"] = serde_json::json!(scheme);
        socket["authentication"] = serde_json::json!({"key":"${[KEY]}","value":"${[VALUE]}","location":"query", "extra":[null,true,42,{"nested":"${[env.NESTED]}"}]});
        if location.is_none() {
            socket["authentication"]
                .as_object_mut()
                .unwrap()
                .remove("location");
        }
        let auth_type = socket["authenticationType"].clone();
        let auth = socket["authentication"].clone();
        document["resources"]["httpRequests"][0]["authenticationType"] = auth_type;
        document["resources"]["httpRequests"][0]["authentication"] = auth;
        let imported = inspect_websocket_export(document)
            .convert(None, false)
            .unwrap();
        let CollectionItem::Folder(outer) = &imported.collection.items[1] else {
            panic!("expected folder")
        };
        let CollectionItem::Folder(inner) = &outer.items[0] else {
            panic!("expected folder")
        };
        let CollectionItem::Request(http) = &inner.items[1] else {
            panic!("expected HTTP")
        };
        let CollectionItem::Request(socket) = &inner.items[0] else {
            panic!("expected socket")
        };
        assert_eq!(http.metadata.name.as_deref(), Some("Health"));
        assert_eq!(http.authentication, socket.authentication);
        assert_eq!(
            imported
                .diagnostics
                .iter()
                .any(|d| d.code == "execution_unsupported" && d.resource_type == "http_request"),
            warning
        );
        assert_eq!(
            socket.authentication.as_ref().map(|auth| auth.kind.clone()),
            expected
        );
        assert_eq!(
            imported.diagnostics.iter().any(
                |d| d.code == "execution_unsupported" && d.resource_type == "websocket_request"
            ),
            warning
        );
        if let Some(auth) = &socket.authentication {
            if scheme == "apikey" {
                assert_eq!(
                    auth.properties["placement"],
                    AuthenticationValue::String(location.unwrap_or("header").into())
                );
                assert!(!auth.properties.contains_key("location"));
            }
            assert_eq!(
                auth.properties["key"],
                AuthenticationValue::String("{{KEY}}".into())
            );
            assert_eq!(
                auth.properties["extra"],
                AuthenticationValue::Sequence(vec![
                    AuthenticationValue::Null,
                    AuthenticationValue::Boolean(true),
                    AuthenticationValue::Number("42".into()),
                    AuthenticationValue::Object(std::collections::BTreeMap::from([(
                        "nested".into(),
                        AuthenticationValue::String("{{NESTED}}".into())
                    )]))
                ])
            );
        }
    }
}

#[test]
fn websocket_message_size_inherits_and_can_restore_default() {
    let mut document = websocket_export();
    document["resources"]["workspaces"][0]["settingRequestMessageSize"] = serde_json::json!(1024);
    let preview = inspect_websocket_export(document.clone());
    let imported = preview.convert(None, true).unwrap();
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.field.as_deref() == Some("settingRequestMessageSize")
                && d.resource_id.as_deref() == Some("ws_nested"))
    );
    document["resources"]["folders"][1]["settingRequestMessageSize"] =
        serde_json::json!({"enabled":true,"value":64*1024*1024});
    let imported = inspect_websocket_export(document)
        .convert(None, true)
        .unwrap();
    assert!(
        imported
            .diagnostics
            .iter()
            .all(|d| d.resource_id.as_deref() != Some("ws_nested"))
    );
}

#[test]
fn websocket_http_settings_are_noops_in_strict_import() {
    for workspace in [None, Some(true), Some(false)] {
        for override_value in [None, Some(true), Some(false)] {
            let mut document = websocket_export();
            let resources = &mut document["resources"];
            resources["workspaces"][0]
                .as_object_mut()
                .unwrap()
                .remove("settingFollowRedirects");
            if let Some(value) = workspace {
                resources["workspaces"][0]["settingFollowRedirects"] = serde_json::json!(value);
            }
            if let Some(value) = override_value {
                resources["folders"][0]["settingFollowRedirects"] =
                    serde_json::json!({"enabled":true,"value":value});
                resources["websocketRequests"][0]["settingFollowRedirects"] =
                    serde_json::json!({"enabled":true,"value":value});
                resources["folders"][1]["settingRequestTimeout"] =
                    serde_json::json!({"enabled":true,"value":1000});
                resources["websocketRequests"][0]["settingRequestTimeout"] =
                    serde_json::json!({"enabled":true,"value":500});
            }
            let imported = inspect_websocket_export(document)
                .convert(None, false)
                .unwrap();
            assert!(!imported.partial);
            assert!(imported.diagnostics.is_empty());
            let CollectionItem::Folder(outer) = &imported.collection.items[1] else {
                panic!("expected outer folder")
            };
            let CollectionItem::Folder(inner) = &outer.items[0] else {
                panic!("expected inner folder")
            };
            let CollectionItem::Request(socket) = &inner.items[0] else {
                panic!("expected WebSocket")
            };
            assert_eq!(socket.settings.timeout, None);
            assert_eq!(socket.settings.follow_redirects, None);
        }
    }
}

#[test]
fn bearer_prefix_loss_is_reported_for_both_protocols() {
    for prefix in ["Bearer", "Token", ""] {
        let mut document = websocket_export();
        document["resources"]["folders"][1]["authenticationType"] = serde_json::json!("bearer");
        document["resources"]["folders"][1]["authentication"] =
            serde_json::json!({"token":"${[TOKEN]}","prefix":prefix});
        let preview = inspect_websocket_export(document);
        assert_eq!(preview.convert(None, false).is_ok(), prefix == "Bearer");
        let imported = preview.convert(None, true).unwrap();
        for (resource_type, id) in [("http_request", "http"), ("websocket_request", "ws_nested")] {
            assert_eq!(
                imported
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "unsupported_authentication"
                        && d.resource_type == resource_type
                        && d.resource_id.as_deref() == Some(id)
                        && d.field.as_deref() == Some("authentication.prefix")),
                prefix != "Bearer"
            );
        }
    }
}

#[test]
fn rejects_websocket_resource_identity_collisions() {
    for (id, expected) in [
        ("http", "duplicate"),
        ("ws_root", "duplicate"),
        ("", "empty id"),
    ] {
        let mut document = websocket_export();
        document["resources"]["websocketRequests"][0]["id"] = serde_json::json!(id);
        let path = temporary_path("invalid-websocket-id.json");
        fs::write(&path, document.to_string()).unwrap();
        assert!(
            matches!(inspect_yaak_source(&path), Err(YaakImportError::Invalid(message)) if message.contains(expected))
        );
        fs::remove_file(path).unwrap();
    }
}
