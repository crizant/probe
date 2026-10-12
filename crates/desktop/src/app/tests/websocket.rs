use super::*;
use crate::app::{SendRequest, SendWebSocketMessage};
use crate::websocket::ConnectionState;
use probe_application::{SessionData, SessionEvent, WebSocketInputFormat};

#[gpui::test]
fn websocket_shortcut_routes_by_focus_context(cx: &mut TestAppContext) {
    cx.update(bind_platform_hotkeys);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let shortcut = gpui::Keystroke::parse(send_shortcut()).unwrap();
        for (contexts, composer) in [
            (vec!["Probe"], false),
            (vec!["Input"], false),
            (vec!["WebSocketComposer"], true),
            (vec!["WebSocketComposer", "Input"], true),
            (
                vec!["WebSocketComposer", "Input", "ProbeEditorSearch"],
                true,
            ),
        ] {
            let contexts: Vec<_> = contexts
                .into_iter()
                .map(|context| gpui::KeyContext::parse(context).unwrap())
                .collect();
            let (bindings, _) =
                keymap.bindings_for_input(std::slice::from_ref(&shortcut), &contexts);
            let action = bindings
                .first()
                .unwrap_or_else(|| panic!("send shortcut is bound: {contexts:?}; {shortcut:?}"))
                .action();
            if composer {
                assert!(action.as_any().is::<SendWebSocketMessage>());
                assert!(
                    bindings
                        .iter()
                        .all(|binding| { !binding.action().as_any().is::<SendRequest>() }),
                    "composer must never fall through to connect"
                );
            } else {
                assert!(action.as_any().is::<SendRequest>());
            }
            for text_key in ["enter", "shift-enter"] {
                let key = gpui::Keystroke::parse(text_key).unwrap();
                let (bindings, _) = keymap.bindings_for_input(&[key], &contexts);
                assert!(bindings.iter().all(|binding| {
                    !binding.action().as_any().is::<SendRequest>()
                        && !binding.action().as_any().is::<SendWebSocketMessage>()
                }));
            }
        }
    });
}

pub(super) fn wait(cx: &mut TestAppContext, mut ready: impl FnMut(&mut TestAppContext) -> bool) {
    cx.executor().allow_parking();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "WebSocket desktop operation timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn websocket_message_editor_controls_warn_without_deleting_variants(cx: &mut TestAppContext) {
    use probe_core::{WebSocketMessageKind as Kind, WebSocketMessageSet, WebSocketMessageVariant};
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), ProbeApp::new);
    let fixture = writable_bundled_fixture("websocket-message-editor");
    let mut loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = loaded.requests()[0].key();
    loaded.request_mut(key).unwrap().kind = probe_core::RequestKind::WebSocket { message: None };
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), loaded);
            view.select_request(key, cx);
        })
        .unwrap();
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual
                .debug_bounds("websocket-no-initial-message")
                .is_some()
        );
        let binary = visual.debug_bounds("websocket-kind-Binary").unwrap();
        visual.simulate_click(binary.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("websocket-binary-warning").is_some());
        assert!(
            visual
                .debug_bounds("websocket-kind-Binary-selected")
                .is_some(),
            "the configured message kind is the selected editor mode"
        );
    }
    window
        .update(cx, |view, _, cx| {
            let request = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(key)
                .unwrap();
            assert_eq!(
                request.selected_websocket_message().unwrap().unwrap().kind,
                Kind::Binary
            );
            assert!(view.persistence.is_dirty(key, request));
            view.edit_request(
                key,
                |request| {
                    request.kind = probe_core::RequestKind::WebSocket {
                        message: Some(WebSocketMessageSet::Variants(vec![
                            WebSocketMessageVariant {
                                title: "Read-only context".into(),
                                selected: true,
                                message: probe_core::WebSocketMessage {
                                    kind: Kind::Text,
                                    data: "retained".into(),
                                },
                            },
                        ])),
                    };
                },
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("websocket-kind-None").is_none(),
            "None must not delete a variant set"
        );
        let json = visual.debug_bounds("websocket-kind-JSON").unwrap();
        visual.simulate_click(json.center(), Modifiers::default());
        visual.run_until_parked();
    }
    window
        .update(cx, |view, _, cx| {
            let request = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(key)
                .unwrap();
            let Some(WebSocketMessageSet::Variants(variants)) = request.websocket_message() else {
                panic!();
            };
            assert_eq!(variants[0].title, "Read-only context");
            assert!(variants[0].selected);
            assert_eq!(variants[0].message.kind, Kind::Json);
            assert_eq!(variants[0].message.data, "retained");
            view.edit_request(
                key,
                |request| {
                    let probe_core::RequestKind::WebSocket {
                        message: Some(WebSocketMessageSet::Variants(variants)),
                    } = &mut request.kind
                    else {
                        panic!();
                    };
                    variants[0].selected = false;
                },
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("websocket-invalid-selection").is_some());
        assert!(visual.debug_bounds("websocket-kind-Text").is_none());
    }
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn websocket_tabs_keep_live_sessions_composer_is_literal_and_close_releases_history(
    cx: &mut TestAppContext,
) {
    use futures_util::{SinkExt, StreamExt};
    let (address_tx, address_rx) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                address_tx.send(listener.local_addr().unwrap()).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                while let Some(message) = socket.next().await {
                    let message = message.unwrap();
                    if message.is_close() {
                        socket.flush().await.unwrap();
                        break;
                    }
                    socket.send(message).await.unwrap();
                }
            })
            .await
            .unwrap();
        });
    });
    let address = address_rx.recv().unwrap();
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), ProbeApp::new);
    let fixture = writable_bundled_fixture("websocket-session");
    let mut loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = loaded.requests()[0].key();
    loaded.request_mut(key).unwrap().kind = probe_core::RequestKind::WebSocket { message: None };
    loaded.request_mut(key).unwrap().url = Some(format!("ws://{address}"));
    let other = loaded.add_detached_request(Request::default());
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), loaded);
            view.select_request(key, cx);
            assert_eq!(
                view.request_editor.section(key),
                EditorSection::WebSocketMessage
            );
        })
        .unwrap();
    cx.run_until_parked();
    // Outside the composer the shared request action connects, not sends.
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_keystrokes(send_shortcut());
    }
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| {
                view.websocket.sessions[&key].state == ConnectionState::Connected
            })
            .unwrap()
    });
    window
        .update(cx, |view, _, cx| {
            let generation = view.websocket.sessions[&key].generation;
            view.send_request(key, cx);
            assert_eq!(
                view.websocket.sessions[&key].generation, generation,
                "Send shortcut cannot disconnect"
            );
            view.select_request(other, cx);
            assert_eq!(
                view.websocket.sessions[&key].state,
                ConnectionState::Connected
            );
            assert_eq!(
                view.websocket.sessions[&key].input_format,
                WebSocketInputFormat::Text
            );
            view.websocket.sessions.get_mut(&key).unwrap().draft =
                "{{token}}\n{\"ok\":true}".into();
            view.send_websocket_message(key, cx);
            assert_eq!(
                view.websocket.sessions[&key].sent, 0,
                "only Sent events add outbound rows"
            );
        })
        .unwrap();
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| view.websocket.sessions[&key].received == 1)
            .unwrap()
    });
    window.update(cx, |view, _, cx| {
        view.select_request(key, cx);
        let session = &view.websocket.sessions[&key];
        assert!(session.draft.is_empty());
        assert_eq!(session.sent, 1);
        assert!(matches!(&session.timeline[1].event, SessionEvent::Sent(SessionData::Text(text)) if text == "{{token}}\n{\"ok\":true}"));
        assert!(matches!(&session.timeline[2].event, SessionEvent::Received(_)));
        for layout in [PaneLayout::Vertical, PaneLayout::Horizontal] { view.shell.pane_layout = layout; let _ = view.render_websocket_panel(Theme::light(), 780.0, cx); }
    }).unwrap();
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let composer = visual.debug_bounds("websocket-composer-input").unwrap();
        visual.simulate_click(composer.center(), Modifiers::default());
        visual.simulate_input("{{literal}}");
        visual.simulate_keystrokes("enter");
        visual.simulate_input("second line");
        visual.simulate_keystrokes("shift-enter");
        visual.simulate_input("third line");
        visual.run_until_parked();
    }
    window
        .update(cx, |view, _, _| {
            let session = &view.websocket.sessions[&key];
            assert_eq!(session.draft, "{{literal}}\nsecond line\nthird line");
            assert_eq!(session.sent, 1, "ordinary Enter must not send");
        })
        .unwrap();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        #[cfg(target_os = "macos")]
        visual.simulate_keystrokes("cmd-enter");
        #[cfg(not(target_os = "macos"))]
        visual.simulate_keystrokes("ctrl-enter");
    }
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| view.websocket.sessions[&key].received == 2)
            .unwrap()
    });
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for (selector, payload) in [
            ("websocket-copy-1", "{{token}}\n{\"ok\":true}"),
            ("websocket-copy-3", "{{literal}}\nsecond line\nthird line"),
        ] {
            let copy = visual
                .debug_bounds(selector)
                .expect("each payload row has its own Copy button");
            visual.simulate_click(copy.center(), Modifiers::default());
            visual.run_until_parked();
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
                Some(payload.to_owned())
            );
        }
    }
    // Changing the runtime message type must preserve the literal draft.
    let mode_draft = "Keep this draft {{literal}}\nAAEC";
    window
        .update(cx, |view, _, cx| {
            view.websocket.sessions.get_mut(&key).unwrap().draft = mode_draft.into();
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    for (shortcut, format) in [
        ("down enter", WebSocketInputFormat::Binary),
        ("up enter", WebSocketInputFormat::Text),
        ("down enter", WebSocketInputFormat::Binary),
    ] {
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let selector = visual
                .debug_bounds("websocket-message-type-trigger")
                .unwrap();
            visual.simulate_click(selector.center(), Modifiers::default());
            visual.run_until_parked();
            visual.simulate_keystrokes(shortcut);
            visual.run_until_parked();
        }
        window
            .update(cx, |view, _, _| {
                let state = &view.websocket.sessions[&key];
                assert_eq!(state.input_format, format);
                assert_eq!(state.draft, mode_draft);
                assert_eq!(state.sent, 2, "changing type must not send");
                assert!(!state.sending);
            })
            .unwrap();
    }
    window
        .update(cx, |view, _, cx| {
            let state = view.websocket.sessions.get_mut(&key).unwrap();
            assert_eq!(state.input_format, WebSocketInputFormat::Binary);
            state.draft = "invalid!".into();
            view.send_websocket_message(key, cx);
            let state = &view.websocket.sessions[&key];
            assert_eq!(state.draft, "invalid!");
            assert!(!state.sending);
            assert_eq!(state.sent, 2);
            assert!(has_active_toast(
                view,
                ToastIntent::Error,
                "standard Base64"
            ));
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            let state = view.websocket.sessions.get_mut(&key).unwrap();
            assert_eq!(
                state.draft, "invalid!",
                "validation must preserve the draft"
            );
            assert_eq!(state.sent, 2, "invalid Base64 must not send");
            assert_eq!(state.received, 2);
            state.draft = "AAEC".into();
            // Dismiss the validation toast before further pointer interactions.
            view.clear_toasts();
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let composer = visual.debug_bounds("websocket-composer-input").unwrap();
        visual.simulate_click(composer.center(), Modifiers::default());
        visual.simulate_keystrokes(send_shortcut());
    }
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| {
                let state = &view.websocket.sessions[&key];
                state.received == 3 && !state.sending
            })
            .unwrap()
    });
    window
        .update(cx, |view, _, _| {
            let state = &view.websocket.sessions[&key];
            assert!(
                state.draft.is_empty(),
                "successful binary send clears the draft"
            );
            assert_eq!(state.sent, 3);
            assert_eq!(
                state.timeline[5].event,
                SessionEvent::Sent(SessionData::Binary(vec![0, 1, 2]))
            );
            assert_eq!(
                state.timeline[6].event,
                SessionEvent::Received(SessionData::Binary(vec![0, 1, 2]))
            );
            for entry in [&state.timeline[5], &state.timeline[6]] {
                assert_eq!(entry.original.as_deref(), Some("AAEC"));
                assert_eq!(entry.display, "Binary · 3 B\nAAEC");
            }
        })
        .unwrap();
    window
        .update(cx, |view, _, cx| {
            view.websocket.sessions.get_mut(&key).unwrap().draft = "/w==".into();
            view.send_websocket_message(key, cx);
            view.websocket.sessions.get_mut(&key).unwrap().draft = "Edited during send".into();
        })
        .unwrap();
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| {
                let state = &view.websocket.sessions[&key];
                state.received == 4 && !state.sending
            })
            .unwrap()
    });
    window
        .update(cx, |view, _, cx| {
            let state = &view.websocket.sessions[&key];
            assert_eq!(
                state.draft, "Edited during send",
                "a send must preserve newer edits"
            );
            assert_eq!(
                state.timeline[7].event,
                SessionEvent::Sent(SessionData::Binary(vec![255]))
            );
            view.websocket.sessions.get_mut(&key).unwrap().draft = "Keep this draft".into();
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("websocket-connection-status").is_some());
        let clear = visual.debug_bounds("websocket-clear").unwrap();
        visual.simulate_click(clear.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("websocket-copy-1").is_none());
    }
    window
        .update(cx, |view, _, cx| {
            let session = &view.websocket.sessions[&key];
            assert!(session.timeline.is_empty());
            assert_eq!(session.draft, "Keep this draft");
            assert_eq!(session.state, ConnectionState::Connected);
            assert_eq!((session.sent, session.received), (4, 4));
            view.websocket_control(key, cx);
        })
        .unwrap();
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| {
                view.websocket.sessions[&key].state == ConnectionState::Closed
            })
            .unwrap()
    });
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let composer = visual.debug_bounds("websocket-composer-input").unwrap();
        visual.simulate_click(composer.center(), Modifiers::default());
        visual.simulate_input("blocked edit");
        visual.simulate_keystrokes(send_shortcut());
        visual.run_until_parked();
    }
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.websocket.sessions[&key].draft, "Keep this draft",
                "the disconnected composer preserves the draft and rejects typing"
            );
            assert_eq!(view.websocket.sessions[&key].state, ConnectionState::Closed);
            assert_eq!(view.websocket.sessions[&key].sent, 4);
            assert!(
                view.toasts
                    .iter()
                    .any(|(_, toast, _)| { toast.message.contains("Connect before sending") })
            );
            view.close_tab_now(key, cx);
            assert!(!view.websocket.sessions.contains_key(&key));
        })
        .unwrap();
    peer.join().unwrap();
    fs::remove_file(fixture).unwrap();
}
