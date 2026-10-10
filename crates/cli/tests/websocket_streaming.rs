#[allow(dead_code, unused_imports)]
mod common;

use common::*;
use futures_util::{SinkExt, StreamExt};
use std::{future::Future, io::BufReader, sync::mpsc, time::Duration};
use tokio::net::TcpStream;
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

const GUARD: Duration = Duration::from_secs(30);

fn server<F, Fut>(handler: F) -> (String, JoinHandle<()>)
where
    F: FnOnce(WebSocketStream<TcpStream>) -> Fut + Send + 'static,
    Fut: Future<Output = ()>,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(GUARD, async {
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    let (stream, _) = listener.accept().await.unwrap();
                    stream.set_nodelay(true).unwrap();
                    handler(accept_async(stream).await.unwrap()).await;
                })
                .await
                .expect("server hang guard");
            });
    });
    (url, thread)
}

fn workspace(url: &str, message: Option<&str>) -> PathBuf {
    let path = temporary_path("websocket.yml");
    let message = message
        .map(|text| {
            format!(
                "    message:\n      type: text\n      data: {}\n",
                serde_json::to_string(text).unwrap()
            )
        })
        .unwrap_or_default();
    fs::write(&path, format!("opencollection: 1.0.0\ninfo:\n  name: Streaming\nbundled: true\nitems:\n- info:\n    name: Socket\n    type: websocket\n  websocket:\n    url: {url}\n{message}")).unwrap();
    path
}

async fn close(socket: &mut WebSocketStream<TcpStream>) {
    socket.close(None).await.unwrap();
    while let Some(result) = socket.next().await {
        if matches!(result, Ok(Message::Close(_))) {
            break;
        }
    }
}

fn records(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["schemaVersion"], 1);
            // Key order can differ between standalone CLI and workspace builds
            // when serde_json's preserve_order feature is unified by GPUI.
            let mut quoted = false;
            let mut escaped = false;
            for byte in line.bytes() {
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        quoted = false;
                    }
                } else if byte == b'"' {
                    quoted = true;
                } else {
                    assert!(
                        !byte.is_ascii_whitespace(),
                        "event JSON must be compact: {line}"
                    );
                }
            }
            value
        })
        .collect()
}

#[test]
fn configured_send_and_stdin_order_are_literal_and_ndjson_preserves_data() {
    let (url, server) = server(|mut socket| async move {
        for expected in [
            "initial",
            "{{literal}}",
            "--json",
            "--help",
            "--quiet",
            "",
            "one",
            "",
            "two",
        ] {
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Text(expected.into())
            );
        }
        socket
            .send(Message::Text("hello\nworld".into()))
            .await
            .unwrap();
        socket
            .send(Message::Binary(vec![0, 1, 2].into()))
            .await
            .unwrap();
        close(&mut socket).await;
    });
    let path = workspace(&url, Some("initial"));
    let output = probe_cli::run_with_stdin(
        [
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--send",
            "{{literal}}",
            "--send",
            "--json",
            "--send",
            "--help",
            "--send",
            "--quiet",
            "--send",
            "",
            "--json",
        ],
        &mut &b"one\r\n\ntwo"[..],
    );
    assert_eq!(output.exit_code, 0, "{output:?}");
    assert!(output.stderr.is_empty());
    let values = records(&output.stdout);
    assert_eq!(values[0]["event"], "opened");
    assert_eq!(values[0]["url"], url);
    assert_eq!(
        values
            .iter()
            .filter(|v| v["event"] == "sent")
            .map(|v| v["data"]["value"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "initial",
            "{{literal}}",
            "--json",
            "--help",
            "--quiet",
            "",
            "one",
            "",
            "two"
        ]
    );
    let incoming: Vec<_> = values.iter().filter(|v| v["event"] == "received").collect();
    assert_eq!(incoming[0]["data"]["value"], "hello\nworld");
    assert_eq!(
        incoming[1]["data"],
        serde_json::json!({"type":"binary","encoding":"base64","value":"AAEC"})
    );
    assert_eq!(values.last().unwrap()["event"], "closed");
    assert_eq!(values.last().unwrap()["origin"], "remote");
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn real_binary_flushes_inbound_while_stdin_waits_and_eof_keeps_session_open() {
    let (url, server) = server(|mut socket| async move {
        socket.send(Message::Text("ready".into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("answer".into())
        );
        socket
            .send(Message::Binary(vec![0, 1, 2].into()))
            .await
            .unwrap();
        close(&mut socket).await;
    });
    let path = workspace(&url, None);
    let mut child = probe()
        .args(["request", "run"])
        .arg(&path)
        .arg("items/0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            tx.send(line.unwrap()).unwrap();
        }
    });
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), format!("Connected: {url}"));
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), "< ready");
    // The server waits for this line, which is supplied only after inbound output
    // arrives. Buffering output or reading stdin in the event loop deadlocks.
    stdin.write_all(b"answer\r\n").unwrap();
    drop(stdin);
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), "> answer");
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), "< [binary/base64] AAEC");
    assert!(
        rx.recv_timeout(GUARD)
            .unwrap()
            .starts_with("Closed: remote")
    );
    assert!(child.wait().unwrap().success());
    reader.join().unwrap();
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn max_messages_counts_only_inbound_and_quiet_suppresses_success() {
    for mode in [None, Some("--json"), Some("--quiet")] {
        let (url, server) = server(|mut socket| async move {
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Text("initial".into())
            );
            socket.send(Message::Text("reply".into())).await.unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
            socket.flush().await.unwrap();
        });
        let path = workspace(&url, Some("initial"));
        let mut args = vec![
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--max-messages",
            "1",
        ];
        if let Some(mode) = mode {
            args.push(mode);
        }
        let output = probe_cli::run(args);
        assert_eq!(output.exit_code, 0, "{output:?}");
        match mode {
            Some("--json") => {
                let values = records(&output.stdout);
                assert_eq!(
                    values
                        .iter()
                        .map(|v| v["event"].as_str().unwrap())
                        .collect::<Vec<_>>(),
                    ["opened", "sent", "received", "closed"]
                );
                assert_eq!(values[3]["origin"], "local");
            }
            Some("--quiet") => assert!(output.stdout.is_empty()),
            _ => assert!(output.stdout.contains("> initial\n< reply\nClosed: local")),
        }
        server.join().unwrap();
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn protocol_error_is_one_ndjson_error_then_close_and_fails() {
    let (url, server) = server(|socket| async move {
        // A raw unmasked reserved opcode is an invalid server frame.
        let mut stream = socket.into_inner();
        use tokio::io::AsyncWriteExt;
        stream.write_all(&[0x83, 0]).await.unwrap();
    });
    let path = workspace(&url, None);
    let output = probe_cli::run([
        "request",
        "run",
        path.to_str().unwrap(),
        "items/0",
        "--json",
    ]);
    assert_eq!(output.exit_code, 6, "{output:?}");
    let values = records(&output.stdout);
    assert_eq!(
        values
            .iter()
            .map(|v| v["event"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["opened", "error", "closed"]
    );
    assert_eq!(values[1]["error"]["category"], "network_execution");
    assert!(output.stderr.is_empty());
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn options_are_rejected_without_network_io_and_dry_run_is_a_document() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    // Reject accidental connections immediately so validation regressions fail
    // assertions instead of waiting indefinitely for a WebSocket handshake.
    let monitor = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let mut stopped = stopped;
                let mut connections = 0;
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            let (stream, _) = accepted.unwrap();
                            connections += 1;
                            drop(stream);
                        }
                        _ = &mut stopped => return connections,
                    }
                }
            })
    });
    let path = workspace(&format!("ws://{}", address), Some("hi"));
    for extra in [
        vec!["--output", "response.txt"],
        vec!["--show-headers"],
        vec!["--expect", "status=200"],
    ] {
        let mut args = vec![
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--json",
        ];
        args.extend(extra);
        let output = probe_cli::run(args);
        assert_eq!(output.exit_code, 2, "{output:?}");
        assert_eq!(
            serde_json::from_str::<Value>(&output.stdout).unwrap()["error"]["category"],
            "invalid_arguments"
        );
    }
    for protocol in ["http", "graphql"] {
        let http = temporary_path("protocol.yml");
        fs::write(&http, format!("opencollection: 1.0.0\ninfo:\n  name: Protocol\nbundled: true\nitems:\n- info:\n    name: Request\n    type: {protocol}\n  {protocol}:\n    method: GET\n    url: http://{}\n", address)).unwrap();
        for extra in [
            vec!["--send", "hello"],
            vec!["--timeout", "1"],
            vec!["--max-messages", "1"],
        ] {
            let mut args = vec!["request", "run", http.to_str().unwrap(), "items/0"];
            args.extend(extra);
            assert_eq!(probe_cli::run(args).exit_code, 2);
        }
        let get = probe_cli::run(["request", "get", http.to_str().unwrap(), "items/0"]);
        assert!(get.stdout.contains("Method: GET\n"));
        fs::remove_file(http).unwrap();
    }
    for extra in [
        vec!["--send", "hello"],
        vec!["--timeout", "1"],
        vec!["--max-messages", "1"],
        vec!["--output", "response.txt"],
        vec!["--expect", "status=200"],
    ] {
        let mut args = vec![
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--dry-run",
        ];
        args.extend(extra);
        assert_eq!(probe_cli::run(args).exit_code, 2);
    }
    for option in ["--max-messages", "--timeout"] {
        for value in ["0", "-1", "abc", ""] {
            assert_eq!(
                probe_cli::run([
                    "request",
                    "run",
                    path.to_str().unwrap(),
                    "items/0",
                    option,
                    value
                ])
                .exit_code,
                2
            );
        }
        assert_eq!(
            probe_cli::run([
                "request",
                "run",
                path.to_str().unwrap(),
                "items/0",
                option,
                "1",
                option,
                "2"
            ])
            .exit_code,
            2
        );
    }
    assert_eq!(
        probe_cli::run([
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--dry-run"
        ])
        .stdout,
        format!("WebSocket ws://{}\n", address)
    );
    let output = probe_cli::run([
        "request",
        "run",
        path.to_str().unwrap(),
        "items/0",
        "--dry-run",
        "--json",
    ]);
    assert!(
        serde_json::from_str::<Value>(&output.stdout).unwrap()["dryRun"]
            .as_bool()
            .unwrap()
    );
    let get = probe_cli::run(["request", "get", path.to_str().unwrap(), "items/0"]);
    assert!(get.stdout.contains("WebSocket message (text): hi"));
    assert!(!get.stdout.contains("Body:"));
    assert!(!get.stdout.contains("Method:"));
    let get = probe_cli::run([
        "request",
        "get",
        path.to_str().unwrap(),
        "items/0",
        "--json",
    ]);
    assert_eq!(
        serde_json::from_str::<Value>(&get.stdout).unwrap()["websocketMessage"],
        serde_json::json!({"type":"text","data":"hi"})
    );
    stop.send(()).unwrap();
    assert_eq!(monitor.join().unwrap(), 0, "validation opened a connection");
    fs::remove_file(path).unwrap();
}

#[test]
fn timeout_requests_close_and_returns_timeout_after_closed() {
    let (url, server) = server(|mut socket| async move {
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let path = workspace(&url, None);
    let output = probe_cli::run([
        "request",
        "run",
        path.to_str().unwrap(),
        "items/0",
        "--timeout",
        "1",
        "--json",
    ]);
    assert_eq!(output.exit_code, 6, "{output:?}");
    let values = records(&output.stdout);
    assert_eq!(values[0]["event"], "opened");
    assert_eq!(values[1]["event"], "closed");
    assert_eq!(values[1]["origin"], "local");
    assert_eq!(values[2]["error"]["category"], "request_timeout");
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn ctrl_c_cleanly_closes_with_terminal_input_still_open() {
    let (url, server) = server(|mut socket| async move {
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let path = workspace(&url, None);
    let mut child = probe()
        .args(["request", "run"])
        .arg(&path)
        .arg("items/0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = mpsc::channel();
    let reading = thread::spawn(move || {
        let mut opened = String::new();
        reader.read_line(&mut opened).unwrap();
        tx.send(opened).unwrap();
        let mut rest = String::new();
        reader.read_to_string(&mut rest).unwrap();
        rest
    });
    assert!(rx.recv_timeout(GUARD).unwrap().starts_with("Connected:"));
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(child.wait().unwrap().success());
    assert!(reading.join().unwrap().contains("Closed: local"));
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn secret_url_header_initial_echo_and_close_are_safe_in_human_and_json() {
    const SECRET: &str = "SUPER_SECRET_WS_VALUE";
    for json in [false, true] {
        let (url, server) =
            server(|mut socket| async move {
                assert_eq!(
                    socket.next().await.unwrap().unwrap(),
                    Message::Text(SECRET.into())
                );
                socket.send(Message::Text(SECRET.into())).await.unwrap();
                socket
                    .send(Message::Binary(SECRET.as_bytes().to_vec().into()))
                    .await
                    .unwrap();
                socket.close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                reason: SECRET.into(),
            })).await.unwrap();
                while let Some(message) = socket.next().await {
                    if matches!(message, Ok(Message::Close(_))) {
                        break;
                    }
                }
            });
        let path = workspace(&format!("{url}/{{{{token}}}}"), Some("{{token}}"));
        let mut source = fs::read_to_string(&path).unwrap();
        source.push_str("    headers:\n    - name: X-Secret\n      value: '{{token}}'\nconfig:\n  environments:\n  - name: local\n    variables:\n    - name: token\n      secret: true\n");
        fs::write(&path, source).unwrap();
        let mut command = probe();
        command
            .args(["request", "run"])
            .arg(&path)
            .args([
                "items/0",
                "--environment",
                "local",
                "--secret-provider",
                "env",
            ])
            .env("token", SECRET);
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stdout.contains(SECRET));
        assert!(!stderr.contains(SECRET));
        assert!(stdout.contains("{{token}}"));
        if json {
            let values = records(&stdout);
            assert!(values[0]["url"].as_str().unwrap().ends_with("/{{token}}"));
            assert_eq!(values[1]["data"]["value"], "{{token}}");
            assert_ne!(values[3]["data"]["value"], "U1VQRVJfU0VDUkVUX1dTX1ZBTFVF");
        } else {
            assert!(stdout.contains("> {{token}}"));
        }
        let variables = probe_cli::run([
            "request",
            "variables",
            path.to_str().unwrap(),
            "items/0",
            "--environment",
            "local",
            "--json",
        ]);
        let value: Value = serde_json::from_str(&variables.stdout).unwrap();
        assert!(
            value["variables"][0]["usages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|usage| usage["location"] == "websocket_message")
        );
        assert!(
            probe_cli::run([
                "request",
                "variables",
                path.to_str().unwrap(),
                "items/0",
                "--environment",
                "local"
            ])
            .stdout
            .contains("WebSocket message")
        );
        server.join().unwrap();
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn unsupported_configured_binary_fails_before_connect_and_errors_are_safe_json() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let path = workspace(
        &format!("ws://{}", listener.local_addr().unwrap()),
        Some("private configured data"),
    );
    let source = fs::read_to_string(&path)
        .unwrap()
        .replace("type: text", "type: binary");
    fs::write(&path, source).unwrap();
    let output = probe_cli::run([
        "request",
        "run",
        path.to_str().unwrap(),
        "items/0",
        "--json",
    ]);
    assert_eq!(output.exit_code, 5);
    assert_eq!(
        serde_json::from_str::<Value>(&output.stdout).unwrap()["error"]["category"],
        "request_configuration"
    );
    assert!(!output.stdout.contains("private configured data"));
    assert!(listener.accept().is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn workspace_from_stdin_can_run_send_and_max_messages_after_yaml_eof() {
    let (url, server) = server(|mut socket| async move {
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("literal".into())
        );
        socket.send(Message::Text("reply".into())).await.unwrap();
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let path = workspace(&url, None);
    let yaml = fs::read_to_string(&path).unwrap();
    let output = probe_cli::run_with_stdin(
        [
            "request",
            "run",
            "-",
            "items/0",
            "--send",
            "literal",
            "--max-messages",
            "1",
        ],
        &mut yaml.as_bytes(),
    );
    assert_eq!(output.exit_code, 0, "{output:?}");
    assert!(output.stdout.contains("> literal\n< reply\nClosed: local"));
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn rejected_handshake_uses_safe_status_detail_and_never_peer_diagnostics() {
    const PRIVATE: &str = "PRIVATE_HANDSHAKE_DIAGNOSTIC";
    let (url, server) =
        serve_once_with_status(PRIVATE.as_bytes().to_vec(), "text/plain", 403, "Forbidden");
    let path = workspace(&url.replacen("http://", "ws://", 1), None);
    let output = probe_cli::run([
        "request",
        "run",
        path.to_str().unwrap(),
        "items/0",
        "--json",
    ]);
    assert_eq!(output.exit_code, 6);
    assert!(!output.stdout.contains(PRIVATE));
    let value: Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["error"]["category"], "network_execution");
    assert_eq!(value["error"]["details"]["status"], 403);
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn human_and_quiet_runtime_errors_use_stderr_and_nonzero_exit() {
    for quiet in [false, true] {
        let (url, server) = server(|socket| async move {
            drop(socket);
        });
        let path = workspace(&url, None);
        let mut args = vec!["request", "run", path.to_str().unwrap(), "items/0"];
        if quiet {
            args.push("--quiet");
        }
        let output = probe_cli::run(args);
        assert_eq!(output.exit_code, 6, "{output:?}");
        assert!(output.stderr.starts_with("error[network_execution]:"));
        assert_eq!(output.stderr.lines().count(), 1);
        if quiet {
            assert!(output.stdout.is_empty());
        } else {
            assert!(output.stdout.contains("Closed: error"));
        }
        server.join().unwrap();
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn input_channel_errors_close_and_report_without_repeating_transport_events() {
    let (url, server) = server(|mut socket| async move {
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let path = workspace(&url, None);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = probe_cli::run_with_io(
        [
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--json",
        ],
        &mut std::io::empty(),
        &mut stdout,
        &mut stderr,
        |_| {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(Err(std::io::Error::other("private input diagnostic")))
                .unwrap();
            Ok(rx)
        },
    )
    .unwrap();
    assert_eq!(code, 6);
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(!stdout.contains("private input diagnostic"));
    let values = records(&stdout);
    assert_eq!(values[0]["event"], "opened");
    assert_eq!(values[1]["event"], "closed");
    assert_eq!(values[2]["error"]["category"], "stdin_error");
    assert!(stderr.is_empty());
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn timeout_before_session_is_a_normal_json_failure_and_input_is_not_started() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (release, wait) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(GUARD)).unwrap();
        let mut reader = BufReader::new(&mut stream);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        wait.recv_timeout(GUARD).unwrap();
    });
    let path = workspace(&url, None);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = probe_cli::run_with_io(
        [
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--timeout",
            "1",
            "--json",
        ],
        &mut std::io::empty(),
        &mut stdout,
        &mut stderr,
        |_| panic!("input requires a connected session"),
    )
    .unwrap();
    assert_eq!(code, 6);
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["error"]["category"], "request_timeout");
    assert!(stderr.is_empty());
    release.send(()).unwrap();
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn input_factory_failure_is_safe_and_cleans_up_connected_session() {
    let (url, server) = server(|mut socket| async move {
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let path = workspace(&url, None);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = probe_cli::run_with_io(
        [
            "request",
            "run",
            path.to_str().unwrap(),
            "items/0",
            "--json",
        ],
        &mut std::io::empty(),
        &mut stdout,
        &mut stderr,
        |_| Err(std::io::Error::other("private factory diagnostic")),
    )
    .unwrap();
    assert_eq!(code, 6);
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["error"]["category"], "stdin_error");
    assert!(stderr.is_empty());
    assert!(
        !String::from_utf8(stdout)
            .unwrap()
            .contains("private factory diagnostic")
    );
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn http_ctrl_c_keeps_existing_cancellation_category() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(GUARD)).unwrap();
        let mut reader = BufReader::new(&mut stream);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(GUARD).unwrap();
    });
    let path = workspace(&url, None);
    let source = fs::read_to_string(&path)
        .unwrap()
        .replace("type: websocket", "type: http")
        .replace("  websocket:", "  http:\n    method: GET");
    fs::write(&path, source).unwrap();
    let mut child = probe()
        .args(["request", "run"])
        .arg(&path)
        .args(["items/0", "--json"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    ready_rx.recv_timeout(GUARD).unwrap();
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(6));
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["error"]["category"],
        "request_cancelled"
    );
    release_tx.send(()).unwrap();
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn ambiguous_message_selection_is_configuration_failure_before_network_io() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let path = workspace(&format!("ws://{}", listener.local_addr().unwrap()), None);
    let mut source = fs::read_to_string(&path).unwrap();
    source.push_str("    message:\n    - title: First\n      message:\n        type: text\n        data: one\n    - title: Second\n      message:\n        type: text\n        data: two\n");
    fs::write(&path, source).unwrap();
    let output = probe_cli::run([
        "request",
        "run",
        path.to_str().unwrap(),
        "items/0",
        "--json",
    ]);
    assert_eq!(output.exit_code, 5, "{output:?}");
    assert_eq!(
        serde_json::from_str::<Value>(&output.stdout).unwrap()["error"]["category"],
        "request_configuration"
    );
    assert!(listener.accept().is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn failed_event_writer_closes_the_session_and_reports_output_error() {
    struct BrokenOutput;
    impl Write for BrokenOutput {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("output unavailable"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (url, server) = server(|mut socket| async move {
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let path = workspace(&url, None);
    let mut stderr = Vec::new();
    let code = probe_cli::run_with_io(
        ["request", "run", path.to_str().unwrap(), "items/0"],
        &mut std::io::empty(),
        &mut BrokenOutput,
        &mut stderr,
        |_| {
            let (_, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        },
    )
    .unwrap();
    assert_eq!(code, 6);
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "error[output_error]: output unavailable\n"
    );
    server.join().unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn unsupported_wall_clock_duration_is_rejected_before_network_io() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let path = workspace(&format!("ws://{}", listener.local_addr().unwrap()), None);
    let output = probe_cli::run([
        "request",
        "run",
        path.to_str().unwrap(),
        "items/0",
        "--timeout",
        "18446744073709551615",
        "--json",
    ]);
    assert_eq!(output.exit_code, 2, "{output:?}");
    assert_eq!(
        serde_json::from_str::<Value>(&output.stdout).unwrap()["error"]["category"],
        "invalid_arguments"
    );
    assert!(listener.accept().is_err());
    fs::remove_file(path).unwrap();
}
