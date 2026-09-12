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

    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("request-protocol-label").is_some());
        assert!(visual.debug_bounds("request-tree-protocol-label").is_some());
        assert!(visual.debug_bounds("request-breadcrumb-protocol").is_none());
    }

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
            assert!(
                view.request_editor.section.available_for(true),
                "created GraphQL request should leave a GraphQL-available section selected"
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
fn graphql_variables_extensions_and_operation_name_persist(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("graphql-variables-extensions");
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
                        extensions: Some(Some(
                            serde_json::from_str(r#"{"persistedQuery":{"version":1}}"#).unwrap(),
                        )),
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

    window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let request = loaded.workspace().request(key).unwrap();
            let operation = request.selected_graphql().unwrap().unwrap();
            assert_eq!(operation.operation_name.as_deref(), Some("GetUser"));
            assert_eq!(
                operation
                    .variables
                    .as_ref()
                    .and_then(|vars| vars.get("id"))
                    .and_then(|v| v.as_i64()),
                Some(1)
            );
            assert_eq!(
                operation
                    .extensions
                    .as_ref()
                    .and_then(|ext| ext.get("persistedQuery"))
                    .and_then(|v| v.get("version"))
                    .and_then(|v| v.as_i64()),
                Some(1)
            );
        })
        .unwrap();

    window
        .update(cx, |view, window, cx| {
            view.save_active_request(window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    let reloaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = reloaded
        .workspace()
        .request(reloaded.requests().last().unwrap().key())
        .unwrap();
    let operation = request.selected_graphql().unwrap().unwrap();
    assert_eq!(operation.operation_name.as_deref(), Some("GetUser"));
    assert_eq!(
        operation
            .variables
            .as_ref()
            .and_then(|vars| vars.get("id"))
            .and_then(|v| v.as_i64()),
        Some(1)
    );
    assert_eq!(
        operation
            .extensions
            .as_ref()
            .and_then(|ext| ext.get("persistedQuery"))
            .and_then(|v| v.get("version"))
            .and_then(|v| v.as_i64()),
        Some(1)
    );

    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn selecting_request_resets_unavailable_editor_section(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("graphql-section-remap");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.apply_structure(
                StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "HTTP".to_owned(),
                    method: Some("GET".to_owned()),
                    url: Some("https://api.example.com".to_owned()),
                    protocol: CreatedRequestProtocol::Http,
                    graphql: None,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let http_key = window
        .update(cx, |view, _, _| {
            view.loaded_workspace
                .as_ref()
                .unwrap()
                .requests()
                .last()
                .unwrap()
                .key()
        })
        .unwrap();

    window
        .update(cx, |view, window, cx| {
            view.apply_structure(
                StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "GraphQL".to_owned(),
                    method: Some("POST".to_owned()),
                    url: Some("https://api.example.com/graphql".to_owned()),
                    protocol: CreatedRequestProtocol::Graphql,
                    graphql: None,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let graphql_key = window
        .update(cx, |view, _, _| {
            view.loaded_workspace
                .as_ref()
                .unwrap()
                .requests()
                .last()
                .unwrap()
                .key()
        })
        .unwrap();

    window
        .update(cx, |view, _, cx| {
            view.request_editor.section = EditorSection::Body;
            view.select_request(graphql_key, cx);
            assert_eq!(view.request_editor.section, EditorSection::GraphqlQuery);

            view.request_editor.section = EditorSection::GraphqlExtensions;
            view.select_request(http_key, cx);
            assert_eq!(view.request_editor.section, EditorSection::Body);
        })
        .unwrap();

    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn closing_graphql_tab_resets_unavailable_editor_section(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("graphql-section-remap-close-tab");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.apply_structure(
                StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "HTTP".to_owned(),
                    method: Some("GET".to_owned()),
                    url: Some("https://api.example.com".to_owned()),
                    protocol: CreatedRequestProtocol::Http,
                    graphql: None,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let http_key = window
        .update(cx, |view, _, _| {
            view.loaded_workspace
                .as_ref()
                .unwrap()
                .requests()
                .last()
                .unwrap()
                .key()
        })
        .unwrap();

    window
        .update(cx, |view, window, cx| {
            view.apply_structure(
                StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "GraphQL".to_owned(),
                    method: Some("POST".to_owned()),
                    url: Some("https://api.example.com/graphql".to_owned()),
                    protocol: CreatedRequestProtocol::Graphql,
                    graphql: None,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let graphql_key = window
        .update(cx, |view, _, _| {
            view.loaded_workspace
                .as_ref()
                .unwrap()
                .requests()
                .last()
                .unwrap()
                .key()
        })
        .unwrap();

    window
        .update(cx, |view, _, cx| {
            view.select_request(http_key, cx);
            view.select_request(graphql_key, cx);
            view.request_editor.section = EditorSection::GraphqlVariables;
            assert_eq!(view.shell.active_tab(), Some(graphql_key));

            view.close_tab_now(graphql_key, cx);
            assert_eq!(view.shell.active_tab(), Some(http_key));
            assert_eq!(view.request_editor.section, EditorSection::Body);

            view.select_request(graphql_key, cx);
            view.request_editor.section = EditorSection::GraphqlExtensions;
            view.close_other_tabs_now(http_key, cx);
            assert_eq!(view.shell.active_tab(), Some(http_key));
            assert_eq!(view.request_editor.section, EditorSection::Body);
        })
        .unwrap();

    fs::remove_file(fixture).unwrap();
}
