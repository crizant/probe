use futures_util::{SinkExt, StreamExt};
use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Header, QueryParameter, Request,
    RequestKind,
};
use probe_websocket::{WebSocketData, WebSocketError, WebSocketEvent, WebSocketRequest};
use std::{collections::BTreeMap, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_tungstenite::{
    accept_async, accept_hdr_async,
    tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

fn request(url: String) -> Request {
    Request {
        url: Some(url),
        kind: RequestKind::WebSocket { message: None },
        ..Request::default()
    }
}
async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    (listener, url)
}
fn auth(kind: AuthenticationKind, values: &[(&str, &str)]) -> Authentication {
    Authentication {
        kind,
        properties: values
            .iter()
            .map(|(k, v)| (k.to_string(), AuthenticationValue::String(v.to_string())))
            .collect(),
    }
}

#[tokio::test]
#[allow(clippy::result_large_err)] // tungstenite fixes the handshake callback error type.
async fn handshake_headers_and_http_auth_semantics() {
    for authentication in [
        auth(
            AuthenticationKind::Basic,
            &[("username", "user"), ("password", "pass")],
        ),
        auth(AuthenticationKind::Bearer, &[("token", "token")]),
        auth(
            AuthenticationKind::ApiKey,
            &[
                ("key", "X-Key"),
                ("value", "a b&c"),
                ("placement", "header"),
            ],
        ),
        auth(
            AuthenticationKind::ApiKey,
            &[("key", "key"), ("value", "a b&c"), ("placement", "query")],
        ),
    ] {
        let (listener, url) = listener().await;
        let (tx, rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut tx = Some(tx);
            let mut socket = accept_hdr_async(
                stream,
                move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                      response| {
                    tx.take().unwrap().send(req.clone()).unwrap();
                    Ok(response)
                },
            )
            .await
            .unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
            let _ = socket.flush().await;
        });
        let mut request = request(url);
        request.headers = vec![
            Header {
                name: "X-Test".into(),
                value: "first".into(),
                disabled: false,
            },
            Header {
                name: "X-Test".into(),
                value: "second".into(),
                disabled: false,
            },
            Header {
                name: "bad header".into(),
                value: "\n".into(),
                disabled: true,
            },
            Header {
                name: "".into(),
                value: "\n".into(),
                disabled: false,
            },
            Header {
                name: "Authorization".into(),
                value: "original".into(),
                disabled: false,
            },
        ];
        request.query_parameters = vec![
            QueryParameter {
                name: "q".into(),
                value: "space here".into(),
                disabled: false,
            },
            QueryParameter {
                name: "ignored".into(),
                value: "secret".into(),
                disabled: true,
            },
        ];
        request.authentication = Some(authentication.clone());
        let mut connection = WebSocketRequest::new(&request)
            .unwrap()
            .connect()
            .await
            .unwrap();
        let upgrade = rx.await.unwrap();
        assert_eq!(upgrade.headers().get_all("x-test").iter().count(), 2);
        assert_eq!(upgrade.headers()["x-test"], "first");
        assert!(upgrade.uri().to_string().contains("q=space+here"));
        assert!(!upgrade.uri().to_string().contains("ignored"));
        match authentication.kind {
            AuthenticationKind::Basic => {
                assert_eq!(upgrade.headers()["authorization"], "Basic dXNlcjpwYXNz")
            }
            AuthenticationKind::Bearer => {
                assert_eq!(upgrade.headers()["authorization"], "Bearer token")
            }
            AuthenticationKind::ApiKey => {
                assert_eq!(upgrade.headers()["authorization"], "original");
                if authentication.properties["placement"]
                    == AuthenticationValue::String("header".into())
                {
                    assert_eq!(upgrade.headers()["x-key"], "a b&c");
                } else {
                    assert!(upgrade.uri().to_string().contains("key=a+b%26c"));
                }
            }
            _ => unreachable!(),
        }
        connection.close().await.unwrap();
        server.await.unwrap();
    }
}

#[tokio::test]
async fn text_binary_ping_and_remote_close() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("hello".into())
        );
        socket
            .send(Message::Ping(b"probe".to_vec().into()))
            .await
            .unwrap();
        socket.send(Message::Text("reply".into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Pong(b"probe".to_vec().into())
        );
        socket
            .send(Message::Binary(vec![0, 255].into()))
            .await
            .unwrap();
        socket
            .close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "finished".into(),
            }))
            .await
            .unwrap();
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
    });
    let mut connection = WebSocketRequest::new(&request(url))
        .unwrap()
        .connect()
        .await
        .unwrap();
    assert!(!format!("{connection:?}").contains("socket"));
    connection
        .send(WebSocketData::Text("hello".into()))
        .await
        .unwrap();
    assert!(
        matches!(connection.receive().await.unwrap(), WebSocketEvent::Data(WebSocketData::Text(text)) if text == "reply")
    );
    assert!(
        matches!(connection.receive().await.unwrap(), WebSocketEvent::Data(WebSocketData::Binary(bytes)) if bytes == [0, 255])
    );
    assert!(
        matches!(connection.receive().await.unwrap(), WebSocketEvent::Closed { code: Some(1000), reason } if reason == "finished")
    );
    assert_eq!(
        connection
            .send(WebSocketData::Text("too late".into()))
            .await,
        Err(WebSocketError::Closed)
    );
    connection.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn connection_deadline_includes_handshake() {
    let (listener, url) = listener().await;
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        accepted_tx.send(()).unwrap();
        std::future::pending::<()>().await;
        drop(stream);
    });
    let mut request = request(url);
    request.settings.timeout = Some(Duration::from_secs(30));
    let connect = tokio::spawn(WebSocketRequest::new(&request).unwrap().connect());
    accepted_rx.await.unwrap();
    tokio::time::advance(Duration::from_secs(30)).await;
    assert_eq!(connect.await.unwrap().unwrap_err(), WebSocketError::Timeout);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn keep_alive_uses_ping_and_shutdown_is_bounded() {
    let (listener, url) = listener().await;
    let (ping_tx, ping_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Ping(Vec::new().into())
        );
        ping_tx.send(()).unwrap();
        release_rx.await.unwrap();
        socket.send(Message::Text("done".into())).await.unwrap();
        // Deliberately do not read or acknowledge the close frame.
        std::future::pending::<()>().await;
    });
    let mut request = request(url);
    request.settings.keep_alive_interval = Some(Duration::from_secs(10));
    let mut connection = WebSocketRequest::new(&request)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let receive = tokio::spawn(async move {
        assert!(
            matches!(connection.receive().await.unwrap(), WebSocketEvent::Data(WebSocketData::Text(text)) if text == "done")
        );
        connection
    });
    tokio::time::advance(Duration::from_secs(10)).await;
    ping_rx.await.unwrap();
    release_tx.send(()).unwrap();
    let mut connection = receive.await.unwrap();
    let close = tokio::spawn(async move { connection.close().await });
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(
        close.await.unwrap().unwrap_err(),
        WebSocketError::CloseTimeout
    );
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[test]
fn configuration_errors_are_typed_and_do_not_disclose_inputs() {
    let mut req = request("wss://example.com/private-secret".into());
    req.settings.timeout = Some(Duration::ZERO);
    req.settings.keep_alive_interval = Some(Duration::ZERO);
    let prepared = WebSocketRequest::new(&req).unwrap();
    assert!(!format!("{prepared:?}").contains("private-secret"));
    for url in [
        "http://example.com",
        "ws://example.com/#fragment",
        "invalid-secret",
    ] {
        req.url = Some(url.into());
        assert_eq!(
            WebSocketRequest::new(&req).unwrap_err(),
            WebSocketError::InvalidUrl
        );
    }
    req.url = None;
    assert_eq!(
        WebSocketRequest::new(&req).unwrap_err(),
        WebSocketError::MissingUrl
    );
    req.url = Some("ws://localhost".into());
    req.authentication = Some(auth(AuthenticationKind::Digest, &[]));
    assert_eq!(
        WebSocketRequest::new(&req).unwrap_err(),
        WebSocketError::UnsupportedAuthentication
    );
    for (values, expected) in [
        (
            vec![],
            WebSocketError::MissingAuthenticationProperty {
                scheme: "apikey",
                property: "key",
            },
        ),
        (
            vec![("key", "")],
            WebSocketError::MissingAuthenticationProperty {
                scheme: "apikey",
                property: "key",
            },
        ),
        (
            vec![("key", "key")],
            WebSocketError::MissingAuthenticationProperty {
                scheme: "apikey",
                property: "value",
            },
        ),
        (
            vec![("key", "key"), ("value", "secret")],
            WebSocketError::MissingAuthenticationProperty {
                scheme: "apikey",
                property: "placement",
            },
        ),
        (
            vec![("key", "key"), ("value", "secret"), ("placement", "secret")],
            WebSocketError::InvalidApiKeyPlacement,
        ),
        (
            vec![
                ("key", "bad key"),
                ("value", "secret"),
                ("placement", "header"),
            ],
            WebSocketError::InvalidHeader,
        ),
    ] {
        req.authentication = Some(auth(AuthenticationKind::ApiKey, &values));
        assert_eq!(WebSocketRequest::new(&req).unwrap_err(), expected);
        assert!(!format!("{expected:?} {expected}").contains("secret"));
    }
    req.authentication = Some(Authentication {
        kind: AuthenticationKind::Bearer,
        properties: BTreeMap::from([("token".into(), AuthenticationValue::Boolean(true))]),
    });
    assert_eq!(
        WebSocketRequest::new(&req).unwrap_err(),
        WebSocketError::MissingAuthenticationProperty {
            scheme: "bearer",
            property: "token"
        }
    );
    req.authentication = None;
    req.kind = RequestKind::default();
    assert_eq!(
        WebSocketRequest::new(&req).unwrap_err(),
        WebSocketError::NotWebSocket
    );
}

#[tokio::test]
async fn connection_failure_and_drop_release_socket() {
    let (listener, url) = listener().await;
    drop(listener);
    assert_eq!(
        WebSocketRequest::new(&request(url))
            .unwrap()
            .connect()
            .await
            .unwrap_err(),
        WebSocketError::Connection
    );
    let (listener, url) = self::listener().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        assert!(socket.next().await.unwrap().is_err());
    });
    let connection = WebSocketRequest::new(&request(url))
        .unwrap()
        .connect()
        .await
        .unwrap();
    drop(connection);
    server.await.unwrap();
}

#[tokio::test]
#[allow(clippy::result_large_err)] // tungstenite fixes the handshake callback error type.
async fn failed_upgrade_with_echoed_request_returns_only_category() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let result = accept_hdr_async(
            stream,
            |_: &tokio_tungstenite::tungstenite::handshake::server::Request,
             _: tokio_tungstenite::tungstenite::handshake::server::Response| {
                Err(tokio_tungstenite::tungstenite::http::Response::builder()
                    .status(403)
                    .body(Some("private-secret-url-and-auth".into()))
                    .unwrap())
            },
        )
        .await;
        assert!(result.is_err());
    });
    let error = WebSocketRequest::new(&request(url))
        .unwrap()
        .connect()
        .await
        .unwrap_err();
    assert_eq!(error, WebSocketError::Handshake);
    assert!(!format!("{error:?} {error}").contains("private-secret"));
    server.await.unwrap();
}

#[tokio::test]
async fn wss_initiates_tls_without_requiring_a_global_crypto_provider() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut prefix = [0; 5];
        stream.read_exact(&mut prefix).await.unwrap();
        assert_eq!(prefix[0], 0x16); // TLS handshake record, never an HTTP request.
        stream.write_all(b"not a TLS record").await.unwrap();
    });
    let req = request(url.replacen("ws://", "wss://", 1));
    let error = WebSocketRequest::new(&req)
        .unwrap()
        .connect()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        WebSocketError::Tls | WebSocketError::Connection
    ));
    server.await.unwrap();
}

#[tokio::test]
#[allow(clippy::result_large_err)] // tungstenite fixes the handshake callback error type.
async fn upgrade_validation_rejects_invalid_and_unsolicited_headers() {
    for (name, value) in [
        ("Sec-WebSocket-Accept", "incorrect"),
        ("Upgrade", "http"),
        ("Connection", "keep-alive"),
        ("Sec-WebSocket-Extensions", "permessage-deflate"),
        ("Sec-WebSocket-Protocol", "unsolicited"),
    ] {
        let (listener, url) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = accept_hdr_async(stream, move |_: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                response.headers_mut().insert(tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(name.as_bytes()).unwrap(), value.parse().unwrap());
                Ok(response)
            }).await;
        });
        let error = WebSocketRequest::new(&request(url))
            .unwrap()
            .connect()
            .await
            .unwrap_err();
        assert_eq!(error, WebSocketError::Handshake, "{name}");
        server.await.unwrap();
    }
}

#[tokio::test]
async fn upgrade_preserves_first_frame_buffered_with_response_and_bounds_headers() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for oversized in [false, true] {
        let (listener, url) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut key = String::new();
            loop {
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                if line.to_ascii_lowercase().starts_with("sec-websocket-key:") {
                    key = line.split_once(':').unwrap().1.trim().to_owned();
                }
                if line == "\r\n" {
                    break;
                }
            }
            if oversized {
                let response = format!(
                    "HTTP/1.1 101 Switching Protocols\r\nX-Large: {}",
                    "x".repeat(70 * 1024)
                );
                let _ = stream.write_all(response.as_bytes()).await;
            } else {
                let accept = STANDARD.encode(
                    sha1_smol::Sha1::from(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11"))
                        .digest()
                        .bytes(),
                );
                let mut response = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").into_bytes();
                response.extend_from_slice(b"\x81\x05hello");
                stream.write_all(&response).await.unwrap();
            }
        });
        let result = WebSocketRequest::new(&request(url))
            .unwrap()
            .connect()
            .await;
        if oversized {
            assert_eq!(result.unwrap_err(), WebSocketError::Handshake);
        } else {
            let mut connection = result.unwrap();
            assert!(
                matches!(connection.receive().await.unwrap(), WebSocketEvent::Data(WebSocketData::Text(text)) if text == "hello")
            );
        }
        server.await.unwrap();
    }
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
async fn missing_pong_times_out_even_after_receive_is_cancelled() {
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
        // Reading Ping queues a Pong, but withholding flush simulates a half-open peer.
        std::future::pending::<()>().await;
    });
    let mut request = request(url);
    request.settings.keep_alive_interval = Some(Duration::from_secs(10));
    let mut connection = WebSocketRequest::new(&request)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let receive = tokio::spawn(async move {
        tokio::select! {
            _ = cancel_rx => {},
            result = connection.receive() => panic!("unexpected receive result: {}", result.is_ok()),
        }
        connection
    });
    tokio::time::advance(Duration::from_secs(10)).await;
    ping_rx.await.unwrap();
    cancel_tx.send(()).unwrap();
    let mut connection = receive.await.unwrap();
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(matches!(
        connection.receive().await,
        Err(WebSocketError::KeepAliveTimeout)
    ));
    drop(connection);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    clock.abort();
    assert!(clock.await.unwrap_err().is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn matching_pong_allows_the_next_keep_alive_interval() {
    let clock = hold_paused_clock();
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        for text in ["first", "second"] {
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Ping(Vec::new().into())
            );
            // Flush Pong before the text barrier: receiving text then proves
            // the client consumed Pong before the next clock advance.
            socket.flush().await.unwrap();
            socket.send(Message::Text(text.into())).await.unwrap();
        }
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let mut request = request(url);
    request.settings.keep_alive_interval = Some(Duration::from_secs(10));
    let mut connection = WebSocketRequest::new(&request)
        .unwrap()
        .connect()
        .await
        .unwrap();
    for expected in ["first", "second"] {
        tokio::time::advance(Duration::from_secs(10)).await;
        assert!(
            matches!(connection.receive().await.unwrap(), WebSocketEvent::Data(WebSocketData::Text(text)) if text == expected)
        );
    }
    connection.close().await.unwrap();
    server.await.unwrap();
    clock.abort();
    assert!(clock.await.unwrap_err().is_cancelled());
}
