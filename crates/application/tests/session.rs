use futures_util::{SinkExt, StreamExt};
use probe_application::{
    CloseOrigin, NoSecrets, RequestResolution, SessionData, SessionError, SessionEvent,
    prepare_request,
};
use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Environment, EnvironmentVariable,
    Header, Request, RequestKind, SecretVariable, Variable, VariableValue, VariableValueSet,
    WebSocketMessage, WebSocketMessageKind, WebSocketMessageSet, WebSocketMessageVariant,
};
use std::{collections::BTreeMap, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_tungstenite::{
    accept_async, accept_hdr_async,
    tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

const SECRET: &str = "private-secret-value";

// A wall-clock watchdog also bounds tests with paused Tokio time held still.
// Cancellation wakes its thread immediately; no sleeps or virtual-time assumptions.
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    let (done, waiting) = std::sync::mpsc::channel();
    let (expired, deadline) = oneshot::channel();
    let watchdog = std::thread::spawn(move || {
        if waiting.recv_timeout(Duration::from_secs(2)).is_err() {
            let _ = expired.send(());
        }
    });
    let result = tokio::select! {
        result = future => result,
        _ = deadline => panic!("WebSocket test exceeded its wall-clock deadline"),
    };
    let _ = done.send(());
    watchdog.join().unwrap();
    result
}

fn request(url: &str, message: Option<WebSocketMessageSet>) -> Request {
    Request {
        url: Some(url.into()),
        kind: RequestKind::WebSocket { message },
        ..Request::default()
    }
}
fn message(kind: WebSocketMessageKind, data: &str) -> WebSocketMessageSet {
    WebSocketMessageSet::Single(WebSocketMessage {
        kind,
        data: data.into(),
    })
}
async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    (listener, url)
}
fn prepared(req: &Request) -> probe_application::PreparedRequest {
    prepare_request(req, &RequestResolution::default(), &NoSecrets).unwrap()
}

#[test]
fn protocol_and_message_configuration_fail_before_network_io() {
    for kind in [RequestKind::default(), RequestKind::Graphql { body: None }] {
        let mut req = request("ws://127.0.0.1:1", None);
        req.kind = kind;
        assert_eq!(
            prepared(&req).into_websocket().unwrap_err(),
            SessionError::NotWebSocket
        );
    }
    let req = request("ws://127.0.0.1:1", None);
    assert!(
        prepared(&req)
            .into_http()
            .unwrap_err()
            .to_string()
            .contains("through HTTP")
    );
    for selected in [[false, false], [true, true]] {
        let req = request(
            "ws://127.0.0.1:1",
            Some(WebSocketMessageSet::Variants(
                selected
                    .into_iter()
                    .map(|selected| WebSocketMessageVariant {
                        title: "variant".into(),
                        selected,
                        message: WebSocketMessage {
                            kind: WebSocketMessageKind::Text,
                            data: "hello".into(),
                        },
                    })
                    .collect(),
            )),
        );
        assert_eq!(
            prepared(&req).into_websocket().unwrap_err(),
            SessionError::InvalidMessageSelection
        );
    }
    let req = request(
        "invalid url",
        Some(message(WebSocketMessageKind::Binary, "AAEC")),
    );
    assert_eq!(
        prepared(&req).into_websocket().unwrap_err(),
        SessionError::UnsupportedBinaryMessage
    );
    let req = request("invalid-secret", None);
    let error = prepared(&req).into_websocket().unwrap_err();
    assert_eq!(error, SessionError::Configuration);
    assert!(!format!("{error:?} {error}").contains("invalid-secret"));
}

#[tokio::test]
async fn initial_text_json_xml_selected_variant_and_event_order() {
    bounded(async {
        for kind in [
            WebSocketMessageKind::Text,
            WebSocketMessageKind::Json,
            WebSocketMessageKind::Xml,
        ] {
            let (listener, url) = listener().await;
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_async(stream).await.unwrap();
                assert_eq!(
                    socket.next().await.unwrap().unwrap(),
                    Message::Text("resolved message".into())
                );
                socket
                    .send(Message::Text("first reply".into()))
                    .await
                    .unwrap();
                assert_eq!(
                    socket.next().await.unwrap().unwrap(),
                    Message::Text("next".into())
                );
                socket
                    .send(Message::Text("second reply".into()))
                    .await
                    .unwrap();
                assert!(matches!(
                    socket.next().await.unwrap().unwrap(),
                    Message::Close(_)
                ));
                let _ = socket.flush().await;
            });
            let env = Environment {
                name: "local".into(),
                color: None,
                description: None,
                extends: None,
                dot_env_file_path: None,
                variables: vec![EnvironmentVariable::Plain(Variable {
                    name: Some("data".into()),
                    value: Some(VariableValueSet::Single(VariableValue::String(
                        "resolved message".into(),
                    ))),
                    disabled: false,
                })],
            };
            let req = request(
                &url,
                Some(WebSocketMessageSet::Variants(vec![
                    WebSocketMessageVariant {
                        title: "unused".into(),
                        selected: false,
                        message: WebSocketMessage {
                            kind: WebSocketMessageKind::Binary,
                            data: "do not send".into(),
                        },
                    },
                    WebSocketMessageVariant {
                        title: "selected".into(),
                        selected: true,
                        message: WebSocketMessage {
                            kind,
                            data: "{{data}}".into(),
                        },
                    },
                ])),
            );
            let execution = prepare_request(
                &req,
                &RequestResolution {
                    environments: &[env],
                    environment: Some("local"),
                    strict_variables: true,
                    ..RequestResolution::default()
                },
                &NoSecrets,
            )
            .unwrap()
            .into_websocket()
            .unwrap();
            assert!(format!("{execution:?}").starts_with("WebSocketExecution {"));
            let mut session = execution.connect().await.unwrap();
            assert_eq!(
                session.next_event().await,
                Some(SessionEvent::Opened { url })
            );
            assert_eq!(
                session.next_event().await,
                Some(SessionEvent::Sent(SessionData::Text(
                    "resolved message".into()
                )))
            );
            assert_eq!(
                session.next_event().await,
                Some(SessionEvent::Received(SessionData::Text(
                    "first reply".into()
                )))
            );
            session
                .send(SessionData::Text("next".into()))
                .await
                .unwrap();
            assert_eq!(
                session.next_event().await,
                Some(SessionEvent::Sent(SessionData::Text("next".into())))
            );
            assert_eq!(
                session.next_event().await,
                Some(SessionEvent::Received(SessionData::Text(
                    "second reply".into()
                )))
            );
            session.close();
            session.close();
            session.wait_closed().await;
            assert_eq!(
                session.send(SessionData::Text("too late".into())).await,
                Err(SessionError::Closed)
            );
            assert_eq!(
                session.next_event().await,
                Some(SessionEvent::Closed {
                    origin: CloseOrigin::Local,
                    code: None,
                    reason: String::new()
                })
            );
            assert_eq!(session.next_event().await, None);
            server.await.unwrap();
        }
    })
    .await;
}

#[tokio::test]
async fn no_initial_message_stays_open_and_drop_closes() {
    bounded(async {
        let (listener, url) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            // A drop must send Close as the first frame, with no unsolicited text.
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
            let _ = socket.flush().await;
        });
        let mut session = prepared(&request(&url, None))
            .into_websocket()
            .unwrap()
            .connect()
            .await
            .unwrap();
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Opened { url })
        );
        assert!(format!("{session:?}").starts_with("Session {"));
        assert!(!format!("{session:?}").contains("ws://"));
        drop(session);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    })
    .await;
}

#[tokio::test]
#[allow(clippy::result_large_err)] // tungstenite fixes the handshake callback error type.
async fn secrets_are_used_on_wire_and_redacted_at_every_event_boundary() {
    bounded(async {
    for auth_kind in [AuthenticationKind::Bearer, AuthenticationKind::ApiKey] {
        let (listener, url) = listener().await;
        let auth_on_wire = auth_kind.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_hdr_async(
                stream,
                move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                      response| {
                    assert!(req.uri().to_string().contains(SECRET));
                    assert_eq!(req.headers()["x-secret"], SECRET);
                    if auth_on_wire == AuthenticationKind::Bearer {
                        assert_eq!(req.headers()["authorization"], format!("Bearer {SECRET}"));
                    } else {
                        assert!(req.uri().to_string().contains(&format!("key={SECRET}")));
                    }
                    Ok(response)
                },
            )
            .await
            .unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Text(format!("initial {SECRET}").into())
            );
            socket
                .send(Message::Text(format!("echo {SECRET}").into()))
                .await
                .unwrap();
            socket
                .send(Message::Binary(
                    format!("binary {SECRET}").into_bytes().into(),
                ))
                .await
                .unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Binary(SECRET.as_bytes().to_vec().into())
            );
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Text(SECRET.into())
            );
            socket
                .close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    reason: format!("close {SECRET}").into(),
                }))
                .await
                .unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
        });
        let mut req = request(
            &format!("{url}/{{{{token}}}}"),
            Some(message(WebSocketMessageKind::Text, "initial {{token}}")),
        );
        req.headers.push(Header {
            name: "X-Secret".into(),
            value: "{{token}}".into(),
            disabled: false,
        });
        let properties = if auth_kind == AuthenticationKind::Bearer {
            BTreeMap::from([(
                "token".into(),
                AuthenticationValue::String("{{token}}".into()),
            )])
        } else {
            BTreeMap::from([
                ("key".into(), AuthenticationValue::String("key".into())),
                (
                    "value".into(),
                    AuthenticationValue::String("{{token}}".into()),
                ),
                (
                    "placement".into(),
                    AuthenticationValue::String("query".into()),
                ),
            ])
        };
        req.authentication = Some(Authentication {
            kind: auth_kind,
            properties,
        });
        let env = Environment {
            name: "local".into(),
            color: None,
            description: None,
            extends: None,
            dot_env_file_path: None,
            variables: vec![EnvironmentVariable::Secret(SecretVariable {
                name: Some("token".into()),
                value_type: None,
                disabled: false,
            })],
        };
        let request = prepare_request(
            &req,
            &RequestResolution {
                environments: &[env],
                environment: Some("local"),
                overrides: &[("token".into(), SECRET.into())],
                ..RequestResolution::default()
            },
            &NoSecrets,
        )
        .unwrap();
        assert!(!format!("{request:?}").contains(SECRET));
        let execution = request.into_websocket().unwrap();
        assert!(!format!("{execution:?}").contains(SECRET));
        assert!(format!("{execution:?}").starts_with("WebSocketExecution {"));
        let mut session = execution.connect().await.unwrap();
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Opened {
                url: req.url.clone().unwrap()
            })
        );
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Sent(SessionData::Text(
                "initial {{token}}".into()
            )))
        );
        let received = session.next_event().await.unwrap();
        assert!(
            matches!(&received, SessionEvent::Received(SessionData::Text(text)) if text.starts_with("echo ") && !text.contains(SECRET))
        );
        let received = session.next_event().await.unwrap();
        assert!(
            matches!(&received, SessionEvent::Received(SessionData::Binary(bytes)) if bytes.starts_with(b"binary ") && !String::from_utf8_lossy(bytes).contains(SECRET))
        );
        session
            .send(SessionData::Binary(SECRET.as_bytes().to_vec()))
            .await
            .unwrap();
        let sent = session.next_event().await.unwrap();
        assert!(
            matches!(&sent, SessionEvent::Sent(SessionData::Binary(bytes)) if !String::from_utf8_lossy(bytes).contains(SECRET))
        );
        session
            .send(SessionData::Text(SECRET.into()))
            .await
            .unwrap();
        let text_sent = session.next_event().await.unwrap();
        assert!(
            matches!(&text_sent, SessionEvent::Sent(SessionData::Text(text)) if !text.contains(SECRET))
        );
        let closed = session.next_event().await.unwrap();
        assert!(
            matches!(&closed, SessionEvent::Closed { origin: CloseOrigin::Remote, code: Some(1000), reason } if reason.starts_with("close ") && !reason.contains(SECRET))
        );
        assert!(!format!("{received:?} {sent:?} {text_sent:?} {closed:?}").contains(SECRET));
        assert_eq!(session.next_event().await, None);
        session.wait_closed().await;
        server.await.unwrap();
    }
    }).await;
}

#[tokio::test]
async fn runtime_failure_is_terminal_and_diagnostic_free() {
    bounded(async {
        let (listener, url) = listener().await;
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_async(stream).await.unwrap();
            release_rx.await.unwrap();
            drop(socket);
        });
        let mut session = prepared(&request(&url, None))
            .into_websocket()
            .unwrap()
            .connect()
            .await
            .unwrap();
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Opened { url })
        );
        release_tx.send(()).unwrap();
        let error = session.next_event().await.unwrap();
        assert!(matches!(
            error,
            SessionEvent::Error(SessionError::Protocol | SessionError::Connection)
        ));
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Closed {
                origin: CloseOrigin::Error,
                code: None,
                reason: String::new()
            })
        );
        assert_eq!(session.next_event().await, None);
        session.wait_closed().await;
        server.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn close_releases_runtime_without_draining_events() {
    bounded(async {
        let (listener, url) = listener().await;
        let (flood_tx, flood_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            for _ in 0..32 {
                socket.send(Message::Text("inbound".into())).await.unwrap();
            }
            flood_tx.send(()).unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
            let _ = socket.flush().await;
        });
        let mut session = prepared(&request(&url, None))
            .into_websocket()
            .unwrap()
            .connect()
            .await
            .unwrap();
        flood_rx.await.unwrap();
        session.close();
        tokio::time::timeout(Duration::from_secs(2), session.wait_closed())
            .await
            .unwrap();
        server.await.unwrap();
        let mut count = 0;
        while let Some(event) = session.next_event().await {
            if matches!(event, SessionEvent::Closed { .. }) {
                count += 1;
            }
        }
        assert_eq!(count, 1);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn prepared_session_connection_errors_are_safe_and_typed() {
    bounded(async {
        let (listener, url) = listener().await;
        drop(listener);
        assert_eq!(
            prepared(&request(&url, None))
                .into_websocket()
                .unwrap()
                .connect()
                .await
                .unwrap_err(),
            SessionError::Connection
        );
        let (listener, url) = self::listener().await;
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            accepted_tx.send(()).unwrap();
            std::future::pending::<()>().await;
            drop(stream);
        });
        let mut req = request(&url, None);
        req.settings.timeout = Some(Duration::from_secs(30));
        let connect = tokio::spawn(prepared(&req).into_websocket().unwrap().connect());
        accepted_rx.await.unwrap();
        tokio::time::advance(Duration::from_secs(30)).await;
        assert_eq!(connect.await.unwrap().unwrap_err(), SessionError::Timeout);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    })
    .await;
}

#[test]
fn secret_bearing_configuration_diagnostics_withhold_all_request_values() {
    let env = Environment {
        name: "local".into(),
        color: None,
        description: None,
        extends: None,
        dot_env_file_path: None,
        variables: vec![EnvironmentVariable::Secret(SecretVariable {
            name: Some("token".into()),
            value_type: None,
            disabled: false,
        })],
    };
    let reqs = [
        request("{{token}}", None),
        Request {
            headers: vec![Header {
                name: "{{token}} invalid".into(),
                value: "{{token}}".into(),
                disabled: false,
            }],
            ..request("ws://localhost", None)
        },
        Request {
            authentication: Some(Authentication {
                kind: AuthenticationKind::ApiKey,
                properties: BTreeMap::from([
                    ("key".into(), AuthenticationValue::String("key".into())),
                    (
                        "value".into(),
                        AuthenticationValue::String("{{token}}".into()),
                    ),
                    (
                        "placement".into(),
                        AuthenticationValue::String("{{token}}".into()),
                    ),
                ]),
            }),
            ..request("ws://localhost", None)
        },
        request(
            "ws://localhost",
            Some(message(WebSocketMessageKind::Binary, "{{token}}")),
        ),
    ];
    for req in reqs {
        let prepared = prepare_request(
            &req,
            &RequestResolution {
                environments: std::slice::from_ref(&env),
                environment: Some("local"),
                overrides: &[("token".into(), SECRET.into())],
                ..RequestResolution::default()
            },
            &NoSecrets,
        )
        .unwrap();
        assert!(!format!("{prepared:?}").contains(SECRET));
        let error = prepared.into_websocket().unwrap_err();
        assert!(matches!(
            error,
            SessionError::Configuration | SessionError::UnsupportedBinaryMessage
        ));
        assert!(!format!("{error:?} {error}").contains(SECRET));
    }
}

#[tokio::test]
async fn concurrent_sender_is_backpressured_and_does_not_keep_dropped_session_alive() {
    bounded(async {
        let (listener, url) = listener().await;
        let (first_tx, first_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Text("data".into())
            );
            first_tx.send(()).unwrap();
            loop {
                match socket.next().await.unwrap().unwrap() {
                    Message::Text(text) => assert_eq!(text, "data"),
                    Message::Close(_) => {
                        let _ = socket.flush().await;
                        break;
                    }
                    message => panic!("unexpected frame {message:?}"),
                }
            }
        });
        let session = prepared(&request(&url, None))
            .into_websocket()
            .unwrap()
            .connect()
            .await
            .unwrap();
        let sender = session.sender();
        assert!(format!("{sender:?}").starts_with("SessionSender {"));
        assert!(!format!("{sender:?}").contains("ws://"));
        let send = tokio::spawn(async move {
            for _ in 0..128 {
                sender.send(SessionData::Text("data".into())).await?;
            }
            Ok::<_, SessionError>(())
        });
        first_rx.await.unwrap();
        // With events undrained, 128 commands cannot fit in the bounded pipeline.
        assert!(!send.is_finished());
        drop(session);
        assert_eq!(send.await.unwrap(), Err(SessionError::Closed));
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    })
    .await;
}

// Paused Tokio time otherwise auto-advances during idle socket I/O. Keep a task
// runnable so heartbeat deadlines move only through explicit test advances.
fn hold_paused_clock() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    })
}

#[tokio::test(start_paused = true)]
async fn runtime_keep_alive_is_independent_of_event_consumption() {
    bounded(async {
        let clock = hold_paused_clock();
        let (listener, url) = listener().await;
        let (ping_tx, ping_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Ping(Vec::new().into())
            );
            ping_tx.send(()).unwrap();
            // Deliver the automatic Pong before testing the client's Ping response.
            socket.flush().await.unwrap();
            socket
                .send(Message::Ping(b"challenge".to_vec().into()))
                .await
                .unwrap();
            // Wait for the client's response before initiating the remote close.
            loop {
                match socket.next().await.unwrap().unwrap() {
                    Message::Pong(bytes) if bytes.as_ref() == b"challenge" => break,
                    Message::Pong(_) | Message::Ping(_) => {
                        socket.flush().await.unwrap();
                    }
                    frame => panic!("unexpected user frame {frame:?}"),
                }
            }
            socket.close(None).await.unwrap();
            loop {
                match socket.next().await.unwrap().unwrap() {
                    Message::Close(_) => break,
                    Message::Ping(_) | Message::Pong(_) => {
                        let _ = socket.flush().await;
                    }
                    frame => panic!("unexpected user frame {frame:?}"),
                }
            }
        });
        let mut req = request(&url, None);
        req.settings.keep_alive_interval = Some(Duration::from_secs(10));
        let mut session = prepared(&req)
            .into_websocket()
            .unwrap()
            .connect()
            .await
            .unwrap();
        tokio::time::advance(Duration::from_secs(10)).await;
        // No next_event polling is necessary to drive transport control frames.
        ping_rx.await.unwrap();
        session.wait_closed().await;
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Opened { url })
        );
        assert!(matches!(
            session.next_event().await,
            Some(SessionEvent::Closed {
                origin: CloseOrigin::Remote,
                ..
            })
        ));
        assert_eq!(session.next_event().await, None);
        server.await.unwrap();
        clock.abort();
        assert!(clock.await.unwrap_err().is_cancelled());
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn nonresponsive_peer_cannot_retain_closed_runtime() {
    bounded(async {
        let (listener, url) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_async(stream).await.unwrap();
            std::future::pending::<()>().await;
            drop(socket);
        });
        let mut session = prepared(&request(&url, None))
            .into_websocket()
            .unwrap()
            .connect()
            .await
            .unwrap();
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Opened { url })
        );
        session.close();
        // Paused time automatically advances to the shutdown deadline when idle.
        session.wait_closed().await;
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Error(SessionError::CloseTimeout))
        );
        assert!(matches!(
            session.next_event().await,
            Some(SessionEvent::Closed {
                origin: CloseOrigin::Local,
                ..
            })
        ));
        assert_eq!(session.next_event().await, None);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn missing_pong_is_one_terminal_error_followed_by_close() {
    bounded(async {
        use tokio::io::AsyncReadExt;

        let clock = hold_paused_clock();
        let (listener, url) = listener().await;
        let (close_tx, close_rx) = oneshot::channel();
        let (ping_tx, ping_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Ping(Vec::new().into())
            );
            ping_tx.send(()).unwrap();
            // Read the raw close bytes without flushing the automatic Pong or
            // acknowledging Close. This synchronizes the bounded shutdown deadline.
            let mut bytes = [0; 128];
            assert!(socket.get_mut().read(&mut bytes).await.unwrap() > 0);
            assert_eq!(bytes[0] & 0x0f, 8);
            close_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let mut req = request(&url, None);
        req.settings.keep_alive_interval = Some(Duration::from_secs(10));
        let mut session = prepared(&req)
            .into_websocket()
            .unwrap()
            .connect()
            .await
            .unwrap();
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Opened { url })
        );
        tokio::time::advance(Duration::from_secs(10)).await;
        ping_rx.await.unwrap();
        tokio::time::advance(Duration::from_secs(10)).await;
        close_rx.await.unwrap();
        tokio::time::advance(Duration::from_secs(5)).await;
        session.wait_closed().await;
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Error(SessionError::KeepAliveTimeout))
        );
        assert_eq!(
            session.next_event().await,
            Some(SessionEvent::Closed {
                origin: CloseOrigin::Error,
                code: None,
                reason: String::new(),
            })
        );
        assert_eq!(session.next_event().await, None);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        clock.abort();
        assert!(clock.await.unwrap_err().is_cancelled());
    })
    .await;
}
