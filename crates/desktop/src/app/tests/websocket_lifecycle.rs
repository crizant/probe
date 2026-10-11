use super::websocket::wait;
use super::*;
use crate::websocket::ConnectionState;

#[gpui::test]
fn websocket_disconnect_reconnect_and_workspace_replacement_cleanup(cx: &mut TestAppContext) {
    use futures_util::{SinkExt, StreamExt};
    let (address_tx, address_rx) = std::sync::mpsc::channel();
    let (closed_tx, closed_rx) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                address_tx.send(listener.local_addr().unwrap()).unwrap();
                for _ in 0..2 {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    let message = socket.next().await.unwrap().unwrap();
                    assert!(message.is_close());
                    socket.flush().await.unwrap();
                    closed_tx.send(()).unwrap();
                }
            })
            .await
            .unwrap();
        });
    });
    let address = address_rx.recv().unwrap();
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), ProbeApp::new);
    let fixture = writable_bundled_fixture("websocket-cleanup");
    let mut loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = loaded.requests()[0].key();
    loaded.request_mut(key).unwrap().kind = probe_core::RequestKind::WebSocket { message: None };
    loaded.request_mut(key).unwrap().url = Some(format!("ws://{address}"));
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), loaded);
            view.select_request(key, cx);
            view.websocket_control(key, cx);
        })
        .unwrap();
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| {
                view.websocket.sessions[&key].state == ConnectionState::Connected
            })
            .unwrap()
    });
    let first = window
        .update(cx, |view, _, cx| {
            let generation = view.websocket.sessions[&key].generation;
            view.websocket_control(key, cx);
            assert_eq!(
                view.websocket.sessions[&key].state,
                ConnectionState::Closing
            );
            generation
        })
        .unwrap();
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| {
                view.websocket.sessions[&key].state == ConnectionState::Closed
            })
            .unwrap()
    });
    closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    window
        .update(cx, |view, _, cx| {
            assert!(view.websocket.sessions[&key].close.is_some());
            view.websocket_control(key, cx);
            assert_ne!(view.websocket.sessions[&key].generation, first);
            assert!(view.websocket.sessions[&key].timeline.is_empty());
        })
        .unwrap();
    wait(cx, |cx| {
        window
            .update(cx, |view, _, _| {
                view.websocket.sessions[&key].state == ConnectionState::Connected
            })
            .unwrap()
    });
    let mut reconciled = probe_opencollection::load_workspace(&fixture).unwrap();
    let new_key = reconciled.requests()[0].key();
    // A disk edit must not reconnect a surviving logical request.
    reconciled.request_mut(new_key).unwrap().kind =
        probe_core::RequestKind::WebSocket { message: None };
    reconciled.request_mut(new_key).unwrap().url = Some("ws://127.0.0.1:1/changed-on-disk".into());
    window
        .update(cx, |view, _, _| {
            let generation = view.websocket.sessions[&key].generation;
            let selector = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .request_selector(key)
                .unwrap()
                .to_owned();
            view.install_reloaded_workspace(
                reconciled,
                vec![],
                &BTreeMap::from([(key, new_key)]),
                &BTreeMap::from([(selector.clone(), selector)]),
            );
            assert_eq!(view.websocket.sessions[&new_key].generation, generation);
            assert_eq!(
                view.websocket.sessions[&new_key].state,
                ConnectionState::Connected
            );
            assert!(!view.websocket.sessions.contains_key(&key));
        })
        .unwrap();
    assert!(
        closed_rx.try_recv().is_err(),
        "reconcile must leave the live socket open"
    );
    let replacement = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, _| {
            view.set_workspace(fixture.clone(), replacement);
            assert!(view.websocket.sessions.is_empty());
        })
        .unwrap();
    closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    peer.join().unwrap();
    fs::remove_file(fixture).unwrap();
}
