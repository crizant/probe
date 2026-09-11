use probe_core::{GraphqlUpdate, RequestProtocol};
use probe_opencollection::{CreatedRequestProtocol, StructureOperation};

use super::*;

#[gpui::test]
fn graphql_request_creation_and_persistence(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("graphql-create-persist");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.apply_structure(
                StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "GraphQL Query".to_owned(),
                    method: Some("POST".to_owned()),
                    url: Some("https://api.example.com/graphql".to_owned()),
                    protocol: CreatedRequestProtocol::Graphql,
                    graphql: Some(GraphqlUpdate {
                        query: Some("query { viewer { login } }".to_owned()),
                        variables: None,
                        operation_name: None,
                        extensions: None,
                    }),
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let key = window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            loaded.requests().last().unwrap().key()
        })
        .unwrap();

    // Verify the request was created with GraphQL protocol
    window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let request = loaded.workspace().request(key).unwrap();
            assert!(matches!(request.protocol, RequestProtocol::Graphql(_)));
            let operation = request.selected_graphql().unwrap().unwrap();
            assert_eq!(
                operation.query.as_deref(),
                Some("query { viewer { login } }")
            );
        })
        .unwrap();

    // Edit the GraphQL query
    window
        .update(cx, |view, _, cx| {
            let request = view
                .loaded_workspace
                .as_mut()
                .unwrap()
                .request_mut(key)
                .unwrap();
            request
                .apply_graphql_update(&GraphqlUpdate {
                    query: Some("query { user(id: 1) { name } }".to_owned()),
                    ..GraphqlUpdate::default()
                })
                .unwrap();
            view.persistence.edited(key);
            cx.notify();
        })
        .unwrap();

    // Verify the edit
    window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let request = loaded.workspace().request(key).unwrap();
            let operation = request.selected_graphql().unwrap().unwrap();
            assert_eq!(
                operation.query.as_deref(),
                Some("query { user(id: 1) { name } }")
            );
        })
        .unwrap();

    // Save
    window
        .update(cx, |view, window, cx| {
            view.save_active_request(window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    // Verify saved
    let reloaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = reloaded
        .workspace()
        .request(reloaded.requests().last().unwrap().key())
        .unwrap();
    assert!(matches!(request.protocol, RequestProtocol::Graphql(_)));
    let operation = request.selected_graphql().unwrap().unwrap();
    assert_eq!(
        operation.query.as_deref(),
        Some("query { user(id: 1) { name } }")
    );

    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn graphql_variables_and_operation_name(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("graphql-variables");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.apply_structure(
                StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "GraphQL with Variables".to_owned(),
                    method: Some("POST".to_owned()),
                    url: Some("https://api.example.com/graphql".to_owned()),
                    protocol: CreatedRequestProtocol::Graphql,
                    graphql: Some(GraphqlUpdate {
                        query: Some(
                            "query GetUser($id: Int!) { user(id: $id) { name } }".to_owned(),
                        ),
                        variables: Some(Some(serde_json::from_str(r#"{"id": 1}"#).unwrap())),
                        operation_name: Some(Some("GetUser".to_owned())),
                        extensions: None,
                    }),
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let key = window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            loaded.requests().last().unwrap().key()
        })
        .unwrap();

    // Verify variables and operation name
    window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let request = loaded.workspace().request(key).unwrap();
            let operation = request.selected_graphql().unwrap().unwrap();
            assert_eq!(operation.operation_name.as_deref(), Some("GetUser"));
            assert!(operation.variables.is_some());
            let vars = operation.variables.as_ref().unwrap();
            assert_eq!(vars.get("id").and_then(|v| v.as_i64()), Some(1));
        })
        .unwrap();

    fs::remove_file(fixture).unwrap();
}
