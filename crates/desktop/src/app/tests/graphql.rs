use probe_core::{GraphqlOperation, GraphqlUpdate, HttpRequest, RequestProtocol};
use probe_opencollection::{CreatedRequestProtocol, StructureOperation};

use crate::app::tests::*;

#[gpui::test]
async fn graphql_request_creation_and_persistence(cx: &mut App) {
    let (temp, mut view) = test_app(cx, None, basic_collection()).await;
    view.update(cx, |view, cx| {
        view.loaded_workspace.as_mut().unwrap().apply(
            &StructureOperation::CreateRequest {
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
            cx,
        )
    })
    .unwrap()
    .await
    .unwrap();

    let key = view.update(cx, |view, _| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        loaded.requests()[0].key()
    });

    // Verify the request was created with GraphQL protocol
    view.update(cx, |view, _| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let request = loaded.workspace().request(key).unwrap();
        assert!(matches!(
            request.protocol,
            RequestProtocol::Graphql(_)
        ));
        let operation = request.selected_graphql().unwrap().unwrap();
        assert_eq!(operation.query.as_deref(), Some("query { viewer { login } }"));
    });

    // Edit the GraphQL query
    view.update(cx, |view, cx| {
        let request = view.loaded_workspace.as_mut().unwrap().request_mut(key).unwrap();
        request.apply_graphql_update(&GraphqlUpdate {
            query: Some("query { user(id: 1) { name } }".to_owned()),
            ..GraphqlUpdate::default()
        }).unwrap();
        view.persistence.edited(key);
        cx.notify();
    });

    // Verify the edit
    view.update(cx, |view, _| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let request = loaded.workspace().request(key).unwrap();
        let operation = request.selected_graphql().unwrap().unwrap();
        assert_eq!(operation.query.as_deref(), Some("query { user(id: 1) { name } }"));
    });

    // Save and reload
    view.update(cx, |view, cx| view.save_workspace(cx))
        .unwrap()
        .await
        .unwrap();

    let (reloaded_temp, reloaded_view) = test_app(cx, Some(temp.path()), None).await;
    reloaded_view.update(cx, |view, _| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let request = loaded.workspace().request(key).unwrap();
        assert!(matches!(
            request.protocol,
            RequestProtocol::Graphql(_)
        ));
        let operation = request.selected_graphql().unwrap().unwrap();
        assert_eq!(operation.query.as_deref(), Some("query { user(id: 1) { name } }"));
    });

    drop(reloaded_temp);
}

#[gpui::test]
async fn graphql_variables_and_operation_name(cx: &mut App) {
    let (temp, mut view) = test_app(cx, None, basic_collection()).await;
    view.update(cx, |view, cx| {
        view.loaded_workspace.as_mut().unwrap().apply(
            &StructureOperation::CreateRequest {
                parent: None,
                index: None,
                name: "GraphQL with Variables".to_owned(),
                method: Some("POST".to_owned()),
                url: Some("https://api.example.com/graphql".to_owned()),
                protocol: CreatedRequestProtocol::Graphql,
                graphql: Some(GraphqlUpdate {
                    query: Some("query GetUser($id: Int!) { user(id: $id) { name } }".to_owned()),
                    variables: Some(Some(serde_json::from_str(r#"{"id": 1}"#).unwrap())),
                    operation_name: Some(Some("GetUser".to_owned())),
                    extensions: None,
                }),
            },
            cx,
        )
    })
    .unwrap()
    .await
    .unwrap();

    let key = view.update(cx, |view, _| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        loaded.requests()[0].key()
    });

    // Verify variables and operation name
    view.update(cx, |view, _| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let request = loaded.workspace().request(key).unwrap();
        let operation = request.selected_graphql().unwrap().unwrap();
        assert_eq!(operation.operation_name.as_deref(), Some("GetUser"));
        assert!(operation.variables.is_some());
        let vars = operation.variables.as_ref().unwrap();
        assert_eq!(vars.get("id").and_then(|v| v.as_i64()), Some(1));
    });

    drop(temp);
}
