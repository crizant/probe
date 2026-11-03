use super::*;
use gpui_kit::base::input::EditorState;
use probe_core::{
    Body, GraphqlBody, GraphqlOperation, RawBody, RawBodyKind, RequestBody, RequestKind,
};

#[gpui_kit::test]
fn request_editors_keep_sessions_across_sections_and_requests(cx: &mut TestAppContext) {
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
    let object = serde_json::from_value(serde_json::json!({"rows": vec!["needle"; 100]})).unwrap();
    let graphql = workspace.add_detached_request(Request {
        kind: RequestKind::Graphql {
            body: Some(GraphqlBody::Single(GraphqlOperation {
                query: Some(text.clone()),
                variables: Some(object),
                extensions: Some(
                    serde_json::from_value(serde_json::json!({"rows": vec!["needle"; 100]}))
                        .unwrap(),
                ),
                ..Default::default()
            })),
        },
        ..Default::default()
    });
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
        window
            .update(cx, |view, _, cx| {
                view.select_request(key, cx);
                view.request_editor.set_section(key, section);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let editor = window
            .update(cx, |view, window, cx| {
                let editor = view.request_editor.editor(key, slot).editor(cx);
                editor.update(cx, |editor, cx| {
                    editor.set_selected_range(0..0, cx);
                    editor.focus(window, cx);
                });
                editor
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), find_shortcut());
        cx.simulate_input(window.into(), "needle");
        cx.run_until_parked();
        editor.update(cx, |editor: &mut EditorState, cx| {
            editor.next_search_match(cx);
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(0.0), px(-100.0)), cx)
        });
        cx.run_until_parked();
        let before = editor.read_with(cx, |editor, _| {
            (
                editor.scroll_offset(),
                editor.selected_range(),
                editor.search_session().matcher.current_match_index(),
            )
        });
        assert!(before.0.y < px(0.0), "{slot} should be scrolled");
        // Both section switches and request switches unmount the editor.
        for other in [false, true] {
            window
                .update(cx, |view, _, cx| {
                    if other {
                        view.select_request(if key == http { graphql } else { http }, cx);
                    } else {
                        view.request_editor.set_section(key, EditorSection::Headers);
                        cx.notify();
                    }
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |view, _, cx| {
                    view.select_request(key, cx);
                    view.request_editor.set_section(key, section);
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |view, _, cx| {
                    assert_eq!(view.request_editor.editor(key, slot).editor(cx), editor);
                    let state = editor.read(cx);
                    assert_eq!(state.scroll_offset(), before.0, "{slot}");
                    assert_eq!(state.selected_range(), before.1, "{slot}");
                    assert_eq!(state.search_session().query, "needle", "{slot}");
                    assert_eq!(
                        state.search_session().matcher.current_match_index(),
                        before.2,
                        "{slot}"
                    );
                })
                .unwrap();
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            assert!(
                visual.debug_bounds("editor-search-card").is_some(),
                "{slot}"
            );
        }
        // Close this search so another editor in the same request can be checked independently.
        window
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| editor.focus(window, cx))
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
    }
    // Incomplete JSON is a local draft; the last valid model value must not replace it.
    window
        .update(cx, |view, _, cx| {
            view.select_request(graphql, cx);
            view.request_editor
                .set_section(graphql, EditorSection::GraphqlVariables);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    let variables = window
        .update(cx, |view, window, cx| {
            let editor = view
                .request_editor
                .editor(graphql, "graphql-variables")
                .editor(cx);
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(0..editor.value().len(), cx);
                editor.focus(window, cx);
            });
            editor
        })
        .unwrap();
    cx.simulate_input(window.into(), "{");
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| view.select_request(http, cx))
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| view.select_request(graphql, cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(variables.read_with(cx, |editor, _| editor.value()), "{");
    let weak = variables.downgrade();
    drop(variables);
    window
        .update(cx, |view, _, cx| {
            view.remove_open_tab(crate::shell::OpenTab::Request(graphql));
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
fn overview_editor_keeps_undo_and_search_until_closed(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), ProbeApp::new);
    let fixture = bundled_fixture();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = workspace.requests()[0].key();
    let overview = crate::shell::OverviewTab::Collection;
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
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
    let original = editor.read_with(cx, |editor, _| editor.value());
    cx.simulate_input(window.into(), "additional documentation");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), find_shortcut());
    cx.run_until_parked();
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
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("editor-search-card").is_some());
    }
    window
        .update(cx, |_, window, cx| {
            editor.update(cx, |editor, cx| editor.focus(window, cx))
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "escape");
    cx.simulate_keystrokes(
        window.into(),
        if cfg!(target_os = "macos") {
            "cmd-z"
        } else {
            "ctrl-z"
        },
    );
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |editor, _| editor.value()), original);
    window
        .update(cx, |view, _, _| {
            view.remove_open_tab(crate::shell::OpenTab::Overview(overview));
            assert!(!view.overview_editors.borrow().contains_key(&target));
        })
        .unwrap();
}
