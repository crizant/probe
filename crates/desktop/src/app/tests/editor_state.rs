use super::*;
use gpui_kit::{Entity, WindowHandle, base::input::EditorState};
use probe_core::{
    Body, GraphqlBody, GraphqlBodyVariant, GraphqlOperation, RawBody, RawBodyKind, RequestBody,
    RequestKind,
};

fn select_editor(
    window: WindowHandle<ProbeApp>,
    key: probe_core::RequestKey,
    section: EditorSection,
    slot: &'static str,
    cx: &mut TestAppContext,
) -> Entity<EditorState> {
    window
        .update(cx, |view, _, cx| {
            view.select_request(key, cx);
            view.request_editor.set_section(key, section);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            let editor = view.request_editor.editor(key, slot).editor(cx);
            editor.update(cx, |editor, cx| editor.focus(window, cx));
            editor
        })
        .unwrap()
}

fn editor_workspace(
    cx: &mut TestAppContext,
) -> (WindowHandle<ProbeApp>, [probe_core::RequestKey; 3]) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), ProbeApp::new);
    let fixture = bundled_fixture();
    let mut workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let http = workspace.requests()[0].key();
    let text = "needle row\n".repeat(100);
    let request = workspace.request_mut(http).unwrap();
    request.kind = RequestKind::Http {
        body: Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Text,
            data: text.clone(),
        }))),
    };
    request.docs = Some(text.clone());
    let object = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
        serde_json::json!({"rows": vec!["needle"; 100]}),
    )
    .unwrap();
    let operation = GraphqlOperation {
        query: Some(text.clone()),
        variables: Some(object.clone()),
        extensions: Some(object),
        ..Default::default()
    };
    let graphql = workspace.requests()[1].key();
    workspace.request_mut(graphql).unwrap().kind = RequestKind::Graphql {
        body: Some(GraphqlBody::Variants(
            [true, false]
                .map(|selected| GraphqlBodyVariant {
                    title: "Same title".into(),
                    selected,
                    body: operation.clone(),
                })
                .into(),
        )),
    };
    let websocket = workspace.add_detached_request(Request {
        kind: RequestKind::WebSocket {
            message: Some(probe_core::WebSocketMessageSet::Single(
                probe_core::WebSocketMessage {
                    kind: probe_core::WebSocketMessageKind::Text,
                    data: text.clone(),
                },
            )),
        },
        ..Default::default()
    });
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            let (cancel, _) = oneshot::channel();
            view.websocket.begin(websocket, cancel);
            let session = view.websocket.sessions.get_mut(&websocket).unwrap();
            session.state = crate::websocket::ConnectionState::Connected;
            session.draft = text;
            view.select_request(http, cx);
        })
        .unwrap();
    cx.run_until_parked();

    (window, [http, graphql, websocket])
}

#[gpui_kit::test]
fn request_editors_keep_sessions_across_sections_and_requests(cx: &mut TestAppContext) {
    let (window, [http, graphql, websocket]) = editor_workspace(cx);
    for (key, section, slot) in [
        (http, EditorSection::Body, "body-text"),
        (http, EditorSection::Docs, "request-docs"),
        (graphql, EditorSection::GraphqlQuery, "graphql-query"),
        (
            graphql,
            EditorSection::GraphqlVariables,
            "graphql-variables",
        ),
        (
            graphql,
            EditorSection::GraphqlExtensions,
            "graphql-extensions",
        ),
        (
            websocket,
            EditorSection::WebSocketMessage,
            "websocket-message",
        ),
        (
            websocket,
            EditorSection::WebSocketMessage,
            "websocket-composer",
        ),
    ] {
        let editor = select_editor(window, key, section, slot, cx);
        editor.update(cx, |editor, cx| editor.set_selected_range(0..6, cx));
        // Unmount the section, then leave the request. Each editor must retain
        // its session through both transitions; restore it only once.
        window
            .update(cx, |view, _, cx| {
                view.request_editor.set_section(key, EditorSection::Headers);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                view.select_request(if key == http { graphql } else { http }, cx);
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(select_editor(window, key, section, slot, cx), editor);
        assert_eq!(
            editor.read_with(cx, |state, _| state.selected_range()),
            0..6,
            "{slot}"
        );
    }
    let editor = select_editor(window, http, EditorSection::Body, "body-text", cx);
    let weak = editor.downgrade();
    drop(editor);
    window
        .update(cx, |view, _, cx| {
            view.remove_open_tab(crate::shell::OpenTab::Request(http));
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        weak.upgrade().is_none(),
        "closing the tab releases its native editor"
    );
}

#[gpui_kit::test]
fn overview_editor_keeps_session_until_closed(cx: &mut TestAppContext) {
    let (window, [request, ..]) = editor_workspace(cx);
    let overview = crate::shell::OverviewTab::Collection;
    window
        .update(cx, |view, _, cx| {
            view.select_open_tab(crate::shell::OpenTab::Overview(overview), cx);
        })
        .unwrap();
    cx.run_until_parked();
    let target = super::super::documentation::OverviewTarget::Collection;
    let editor = window
        .update(cx, |view, window, cx| {
            let editor = view.overview_editors.borrow()[&target][1].editor(cx);
            editor.update(cx, |editor, cx| editor.focus(window, cx));
            editor
        })
        .unwrap();
    cx.simulate_input(window.into(), "additional documentation");
    cx.run_until_parked();
    let selection = editor.read_with(cx, |editor, _| editor.selected_range());
    window
        .update(cx, |view, _, cx| view.select_request(request, cx))
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            view.select_open_tab(crate::shell::OpenTab::Overview(overview), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .read_with(cx, |view, cx| {
            let restored = view.overview_editors.borrow()[&target][1].editor(cx);
            assert_eq!(restored, editor);
            assert_eq!(restored.read(cx).selected_range(), selection);
        })
        .unwrap();
    window
        .update(cx, |view, _, _| {
            view.remove_open_tab(crate::shell::OpenTab::Overview(overview));
            assert!(!view.overview_editors.borrow().contains_key(&target));
        })
        .unwrap();
}

#[gpui_kit::test]
fn graphql_reload_keeps_unchanged_sessions_but_resets_changed_operations(cx: &mut TestAppContext) {
    let (window, [_, key, _]) = editor_workspace(cx);
    let fixture = bundled_fixture();
    let mut editors = Vec::new();
    for (section, slot) in [
        (EditorSection::GraphqlQuery, "graphql-query"),
        (EditorSection::GraphqlVariables, "graphql-variables"),
        (EditorSection::GraphqlExtensions, "graphql-extensions"),
    ] {
        let editor = select_editor(window, key, section, slot, cx);
        if section != EditorSection::GraphqlQuery {
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(0..editor.value().len(), cx)
            });
        }
        cx.simulate_input(
            window.into(),
            if section == EditorSection::GraphqlQuery {
                "edited"
            } else {
                "invalid"
            },
        );
        cx.run_until_parked();
        editors.push((section, slot, editor));
    }
    let expected_query = editors[0].2.read_with(cx, |editor, _| editor.value());
    for change in 0..3 {
        let changed = change != 0;
        let key = window
            .update(cx, |view, _, cx| {
                let mut fresh = probe_opencollection::load_workspace(&fixture).unwrap();
                let current = view.loaded_workspace.as_ref().unwrap();
                let mut kind = current
                    .workspace()
                    .request(current.requests()[1].key())
                    .unwrap()
                    .kind
                    .clone();
                if changed
                    && let RequestKind::Graphql {
                        body: Some(GraphqlBody::Variants(variants)),
                    } = &mut kind
                {
                    // The new operation has identical text and a duplicate title.
                    if change == 1 {
                        variants[1].body = variants[0].body.clone();
                        variants[0].selected = false;
                        variants[1].selected = true;
                    } else {
                        variants[1].body.operation_name = Some("Changed".into());
                    }
                }
                let key = fresh.requests()[1].key();
                fresh.request_mut(key).unwrap().kind = kind;
                view.apply_reconciled_workspace(reconciled_workspace(fresh), cx);
                key
            })
            .unwrap();
        for (section, slot, previous) in &mut editors {
            let restored = select_editor(window, key, *section, slot, cx);
            assert_eq!(restored == *previous, !changed, "{slot}");
            *previous = restored.clone();
            if *section == EditorSection::GraphqlQuery {
                window
                    .update(cx, |_, window, cx| {
                        window.dispatch_action(Box::new(gpui_kit::base::input::Undo), cx);
                    })
                    .unwrap();
                cx.run_until_parked();
                if !changed {
                    assert_ne!(
                        restored.read_with(cx, |editor, _| editor.value()),
                        expected_query
                    );
                    window
                        .update(cx, |_, window, cx| {
                            window.dispatch_action(Box::new(gpui_kit::base::input::Redo), cx);
                        })
                        .unwrap();
                    cx.run_until_parked();
                }
                assert_eq!(
                    restored.read_with(cx, |editor, _| editor.value()),
                    expected_query
                );
            } else {
                assert_eq!(
                    restored.read_with(cx, |editor, _| editor.value()) == "invalid",
                    !changed,
                    "{slot}"
                );
            }
        }
    }
}
