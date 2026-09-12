use crate::{ImportDiagnosticSeverity, YaakImportError, YaakSourceFormat, inspect_yaak_source};
use probe_core::{
    AuthenticationKind, Body, CollectionItem, GraphqlBody, RequestBody, WorkspaceItemRef,
};
use probe_opencollection::create_bundled_workspace_from_collection;
use std::{fs, path::PathBuf, time::SystemTime};

fn temporary_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("probe-yaak-{}-{nanos}-{name}", std::process::id()))
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
    let CollectionItem::Folder(folder) = &imported.collection.items[0] else {
        panic!("expected folder");
    };
    let CollectionItem::HttpRequest(request) = &folder.items[0] else {
        panic!("expected request");
    };
    assert_eq!(request.path_parameters[0].name, "id");
    assert_eq!(request.query_parameters[0].name, "page");
    assert_eq!(request.headers[0].value, "{{TOKEN}}");
    assert_eq!(
        request.authentication.as_ref().unwrap().kind,
        AuthenticationKind::Bearer
    );
    let Some(RequestBody::Single(Body::Raw(body))) = &request.body else {
        panic!("expected raw body");
    };
    assert!(body.data.contains("{{TOKEN}}"));
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

    let CollectionItem::GraphqlRequest(viewer) = &imported.collection.items[0] else {
        panic!("first item should be a GraphQL request");
    };
    assert_eq!(viewer.metadata.name.as_deref(), Some("Viewer"));
    assert_eq!(viewer.headers[0].value, "{{TRACE}}");
    let Some(GraphqlBody::Single(operation)) = &viewer.body else {
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

    let CollectionItem::GraphqlRequest(legacy) = &imported.collection.items[1] else {
        panic!("second item should be a GraphQL request");
    };
    let Some(GraphqlBody::Single(operation)) = &legacy.body else {
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
    assert_eq!(request.protocol.as_str(), "graphql");
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
    let CollectionItem::GraphqlRequest(request) = &imported.collection.items[0] else {
        panic!("item should be a GraphQL request");
    };
    let Some(GraphqlBody::Single(operation)) = &request.body else {
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
