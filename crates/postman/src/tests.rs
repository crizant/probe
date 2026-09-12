use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use probe_core::{
    AuthenticationKind, Body, CollectionItem, EnvironmentVariable, GraphqlBody, MultipartPartKind,
    RequestBody, VariableValue, VariableValueSet, VariableValueType, WorkspaceItemRef,
};
use probe_opencollection::create_bundled_workspace_from_collection;

use super::{
    COLLECTION_VARIABLES_ENVIRONMENT, PostmanImportError, PostmanSourceFormat,
    inspect_postman_source,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/postman")
        .join(name)
}

fn temporary_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "probe-postman-{}-{unique}-{name}",
        std::process::id()
    ))
}

#[test]
fn imports_v21_collection_into_domain_model() {
    let preview = inspect_postman_source(fixture("collection-v2.1.json")).unwrap();
    assert_eq!(preview.format(), PostmanSourceFormat::CollectionV2_1);
    let imported = preview.convert(false).unwrap();
    assert_eq!(imported.source.id.as_deref(), Some("pm_v21"));
    assert_eq!(
        imported.collection.metadata.name.as_deref(),
        Some("Postman Pets")
    );
    assert_eq!(
        imported.collection.metadata.version.as_deref(),
        Some("1.2.3-beta")
    );
    assert_eq!(imported.collection.items.len(), 1);
    let CollectionItem::Folder(folder) = &imported.collection.items[0] else {
        panic!("first item should be a folder");
    };
    let CollectionItem::HttpRequest(request) = &folder.items[0] else {
        panic!("folder should contain a request");
    };
    assert_eq!(request.url.as_deref(), Some("{{baseUrl}}/pets/:petId"));
    assert_eq!(request.query_parameters[0].name, "expand");
    assert_eq!(request.path_parameters[0].name, "petId");
    assert_eq!(
        request.authentication.as_ref().map(|auth| &auth.kind),
        Some(&AuthenticationKind::Bearer)
    );
    assert!(matches!(
        request.body,
        Some(RequestBody::Single(Body::Raw(_)))
    ));
    assert_eq!(
        imported.collection.environments[0].name,
        COLLECTION_VARIABLES_ENVIRONMENT
    );
    let probe_core::EnvironmentVariable::Plain(variable) =
        &imported.collection.environments[0].variables[0]
    else {
        panic!("collection variable should be plain");
    };
    assert!(matches!(
        variable.value,
        Some(VariableValueSet::Single(VariableValue::String(_)))
    ));
    let variables = &imported.collection.environments[0].variables;
    assert!(matches!(
        variables[2],
        EnvironmentVariable::Plain(probe_core::Variable {
            value: Some(VariableValueSet::Single(VariableValue::Typed {
                kind: VariableValueType::Number,
                ref data,
            })),
            ..
        }) if data == "25"
    ));
    assert!(matches!(
        variables[3],
        EnvironmentVariable::Plain(probe_core::Variable {
            value: Some(VariableValueSet::Single(VariableValue::Typed {
                kind: VariableValueType::Boolean,
                ref data,
            })),
            ..
        }) if data == "true"
    ));
    assert!(matches!(
        variables[4],
        EnvironmentVariable::Plain(probe_core::Variable {
            value: Some(VariableValueSet::Single(VariableValue::Typed {
                kind: VariableValueType::Object,
                ..
            })),
            ..
        })
    ));
    assert!(matches!(
        variables[5],
        EnvironmentVariable::Plain(probe_core::Variable {
            value: Some(VariableValueSet::Single(VariableValue::Typed {
                kind: VariableValueType::Null,
                ..
            })),
            ..
        })
    ));
    assert!(!imported.partial);
}

#[test]
fn imports_v2_object_authentication() {
    let preview = inspect_postman_source(fixture("collection-v2.json")).unwrap();
    assert_eq!(preview.format(), PostmanSourceFormat::CollectionV2);
    let imported = preview.convert(false).unwrap();
    let CollectionItem::HttpRequest(request) = &imported.collection.items[0] else {
        panic!("item should be a request");
    };
    let authentication = request.authentication.as_ref().unwrap();
    assert_eq!(authentication.kind, AuthenticationKind::Basic);
    assert_eq!(
        authentication.properties.get("username"),
        Some(&probe_core::AuthenticationValue::String("demo".to_owned()))
    );
}

#[test]
fn imports_all_supported_bodies_and_expands_authentication_inheritance() {
    let preview = inspect_postman_source(fixture("collection-bodies-v2.1.json")).unwrap();
    let imported = preview.convert(false).unwrap();
    assert!(!imported.partial);
    let CollectionItem::Folder(folder) = &imported.collection.items[0] else {
        panic!("first item should be a folder");
    };
    assert_eq!(folder.items.len(), 5);
    let requests = folder.items[..4]
        .iter()
        .map(|item| match item {
            CollectionItem::HttpRequest(request) => request,
            CollectionItem::Folder(_) | CollectionItem::GraphqlRequest(_) => {
                panic!("first four payload items should be HTTP requests")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        requests[0].url.as_deref(),
        Some("https://api.example.com/pets/:petId")
    );
    assert_eq!(requests[0].query_parameters[0].value, "right");
    assert!(requests[0].query_parameters[0].disabled);
    assert!(requests[0].path_parameters[0].disabled);
    assert_eq!(
        requests[0].authentication.as_ref().map(|auth| &auth.kind),
        Some(&AuthenticationKind::Bearer)
    );
    assert!(requests[1].authentication.is_none());
    assert!(matches!(
        requests[0].body,
        Some(RequestBody::Single(Body::Raw(_)))
    ));
    assert!(matches!(
        requests[1].body,
        Some(RequestBody::Single(Body::FormUrlEncoded(_)))
    ));
    let Some(RequestBody::Single(Body::Multipart(parts))) = &requests[2].body else {
        panic!("third request should be multipart");
    };
    assert_eq!(parts[1].kind, MultipartPartKind::File);
    assert!(matches!(
        requests[3].body,
        Some(RequestBody::Single(Body::File(_)))
    ));
    let CollectionItem::GraphqlRequest(graphql) = &folder.items[4] else {
        panic!("fifth payload item should be a GraphQL request");
    };
    assert_eq!(
        graphql.url.as_deref(),
        Some("https://api.example.com/graphql")
    );
    let Some(GraphqlBody::Single(operation)) = &graphql.body else {
        panic!("GraphQL request should have a single operation");
    };
    assert_eq!(
        operation.query.as_deref(),
        Some("query Pet($id: ID) { pet(id: $id) { id } }")
    );
    assert_eq!(operation.operation_name.as_deref(), Some("Pet"));
    assert_eq!(
        operation
            .variables
            .as_ref()
            .and_then(|variables| variables.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("{{petId}}")
    );
    let CollectionItem::HttpRequest(string_request) = &imported.collection.items[1] else {
        panic!("second root item should be a request");
    };
    assert_eq!(
        string_request
            .authentication
            .as_ref()
            .map(|auth| &auth.kind),
        Some(&AuthenticationKind::Basic)
    );
}

#[test]
fn strict_mode_rejects_lossy_data_and_partial_is_explicit() {
    let preview = inspect_postman_source(fixture("collection-lossy.json")).unwrap();
    let diagnostics = match preview.convert(false) {
        Err(PostmanImportError::Unsupported(diagnostics)) => diagnostics,
        other => panic!("expected strict compatibility failure, got {other:?}"),
    };
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code == "unsupported_scripts")
    );
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code == "unsupported_examples")
    );
    assert!(diagnostics.iter().any(|item| item.code == "unknown_field"));
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code == "unsupported_variable_scope")
    );
    let imported = preview.convert(true).unwrap();
    assert!(imported.partial);
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|item| item.code == "dynamic_variable_unsupported")
    );
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|item| item.code == "execution_unsupported")
    );
}

#[test]
fn rejects_malformed_json_and_unknown_schema() {
    let malformed = inspect_postman_source(fixture("malformed.json")).unwrap_err();
    assert!(matches!(malformed, PostmanImportError::Invalid(_)));
    let error = inspect_postman_source(fixture("collection-v3.json")).unwrap_err();
    assert!(matches!(error, PostmanImportError::Invalid(_)));
}

#[test]
fn bundled_save_reload_preserves_imported_semantics_and_environment() {
    let imported = inspect_postman_source(fixture("collection-v2.1.json"))
        .unwrap()
        .convert(false)
        .unwrap();
    let destination = temporary_path("roundtrip.yml");
    let loaded =
        create_bundled_workspace_from_collection(&destination, &imported.collection).unwrap();
    let workspace = loaded.workspace();
    assert_eq!(workspace.metadata(), &imported.collection.metadata);
    assert_eq!(workspace.request_count(), 1);
    assert_eq!(workspace.folder_count(), 1);
    assert_eq!(workspace.environments(), imported.collection.environments);
    let WorkspaceItemRef::Folder(folder_key) = workspace.root_items()[0] else {
        panic!("round-tripped root item should be a folder");
    };
    let folder = workspace.folder(folder_key).unwrap();
    let WorkspaceItemRef::Request(request_key) = folder.children[0] else {
        panic!("round-tripped folder item should be a request");
    };
    let request = workspace.request(request_key).unwrap();
    let CollectionItem::Folder(source_folder) = &imported.collection.items[0] else {
        panic!("source item should be a folder");
    };
    let CollectionItem::HttpRequest(source_request) = &source_folder.items[0] else {
        panic!("source folder item should be a request");
    };
    assert_eq!(request, source_request);
    fs::remove_file(destination).unwrap();
}

#[test]
fn imports_native_graphql_requests_and_round_trips() {
    let imported = inspect_postman_source(fixture("collection-graphql-v2.1.json"))
        .unwrap()
        .convert(false)
        .unwrap();
    assert!(!imported.partial);
    assert_eq!(imported.collection.items.len(), 2);

    let CollectionItem::GraphqlRequest(viewer) = &imported.collection.items[0] else {
        panic!("first item should be a GraphQL request");
    };
    assert_eq!(viewer.metadata.name.as_deref(), Some("Viewer"));
    assert_eq!(viewer.method.as_deref(), Some("POST"));
    assert_eq!(viewer.headers[0].name, "X-Trace");
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
        Some("{{login}}")
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

    let CollectionItem::GraphqlRequest(object_vars) = &imported.collection.items[1] else {
        panic!("second item should be a GraphQL request");
    };
    assert_eq!(object_vars.method.as_deref(), Some("GET"));
    let Some(GraphqlBody::Single(operation)) = &object_vars.body else {
        panic!("object-variable request should have a single GraphQL operation");
    };
    assert_eq!(
        operation
            .variables
            .as_ref()
            .and_then(|variables| variables.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("{{petId}}")
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
    let operation = request.selected_graphql().unwrap().unwrap();
    assert_eq!(operation.operation_name.as_deref(), Some("Viewer"));
    assert!(
        fs::read_to_string(&destination)
            .unwrap()
            .contains("type: graphql")
    );
    fs::remove_file(destination).unwrap();
}

#[test]
fn rejects_malformed_graphql_variables() {
    let path = temporary_path("invalid-graphql.json");
    fs::write(
        &path,
        r#"{
  "info": {
    "name": "Broken GraphQL",
    "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
  },
  "item": [
    {
      "name": "Broken",
      "request": {
        "method": "POST",
        "url": "https://api.example.com/graphql",
        "body": { "mode": "graphql", "graphql": { "query": "{ pet { id } }", "variables": "[]" } }
      }
    }
  ]
}"#,
    )
    .unwrap();
    let preview = inspect_postman_source(&path).unwrap();
    assert!(matches!(
        preview.convert(false),
        Err(PostmanImportError::Invalid(message)) if message.contains("variables")
    ));
    fs::remove_file(path).unwrap();
}
