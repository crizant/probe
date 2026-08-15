use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/opencollection")
        .join(path)
}

fn probe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_probe"))
}

#[derive(Debug)]
struct CapturedRequest {
    head: String,
    body: Vec<u8>,
}

fn serve_once(body: Vec<u8>, content_type: &str) -> (String, JoinHandle<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mock server should bind");
    let address = listener.local_addr().unwrap();
    let content_type = content_type.to_owned();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 8 * 1024];
        let header_end = loop {
            if let Some(position) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break position + 4;
            }
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
        };
        let head = String::from_utf8_lossy(&request[..header_end]).into_owned();
        let content_length = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim().parse::<usize>().unwrap())
            .unwrap_or(0);
        while request.len() - header_end < content_length {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
        }
        let captured = CapturedRequest {
            head,
            body: request[header_end..header_end + content_length].to_vec(),
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
        captured
    });
    (format!("http://{address}"), handle)
}

fn runtime_fixture(server_url: &str) -> PathBuf {
    let source = fs::read_to_string(fixture("phase5-http.yml")).unwrap();
    let path = temporary_path("workspace.yml");
    fs::write(&path, source.replace("__SERVER_URL__", server_url)).unwrap();
    path
}

fn temporary_path(suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "probe-cli-{}-{unique}-{suffix}",
        std::process::id()
    ))
}

#[test]
fn help_starts_successfully() {
    let output = probe()
        .arg("--help")
        .output()
        .expect("probe binary should start");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("collection validate"));
    assert!(output.stderr.is_empty());
}

#[test]
fn validates_unbundled_workspace_as_json() {
    let output = probe()
        .args(["collection", "validate"])
        .arg(fixture("unbundled"))
        .arg("--json")
        .output()
        .expect("validate command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["valid"], true);
    assert_eq!(value["collection"]["name"], "Unbundled fixture");
    assert_eq!(value["counts"]["requests"], 2);
    assert_eq!(value["counts"]["folders"], 1);
    assert_eq!(value["counts"]["environments"], 1);
}

#[test]
fn lists_requests_deterministically_as_json() {
    let path = fixture("unbundled");
    let first = probe()
        .args(["request", "list"])
        .arg(&path)
        .arg("--json")
        .output()
        .expect("first list command should run");
    let second = probe()
        .args(["request", "list"])
        .arg(&path)
        .arg("--json")
        .output()
        .expect("second list command should run");

    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let value: Value = serde_json::from_slice(&first.stdout).expect("stdout should be JSON");
    assert_eq!(value["requests"][0]["selector"], "health.yml");
    assert_eq!(value["requests"][1]["selector"], "users/list-users.yml");
}

#[test]
fn reads_a_bundled_workspace_from_stdin() {
    let source = fs::read(fixture("phase1-bundled.yml")).unwrap();
    let mut child = probe()
        .args(["request", "list", "-", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("list command should start");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&source)
        .expect("fixture should be written to stdin");
    let output = child
        .wait_with_output()
        .expect("list command should finish");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["requests"][0]["selector"], "items/0/items/0");
    assert_eq!(value["requests"][1]["selector"], "items/1");
}

#[test]
fn quiet_mode_suppresses_success_output() {
    let output = probe()
        .args(["collection", "validate"])
        .arg(fixture("unbundled"))
        .arg("--quiet")
        .output()
        .expect("validate command should run");

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn gets_request_by_repository_selector() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("unbundled"))
        .arg("users/list-users.yml")
        .arg("--json")
        .output()
        .expect("get command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["name"], "List users");
    assert_eq!(value["method"], "GET");
    assert_eq!(value["headers"][0]["name"], "Accept");
    assert_eq!(value["queryParameters"][0]["name"], "limit");
    assert!(value["environment"].is_null());
}

#[test]
fn gets_request_resolved_with_selected_environment() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/0")
        .args(["--environment", "development", "--json"])
        .output()
        .expect("resolved get command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["environment"], "development");
    assert_eq!(value["url"], "https://dev.example.com/au/users");
    assert_eq!(value["headers"][0]["value"], "Bearer development-token");
    assert_eq!(value["queryParameters"][0]["value"], "au");
    assert_eq!(value["body"]["value"]["data"], "{\"tenant\":\"au\"}");
    assert_eq!(
        value["authentication"]["properties"]["token"],
        "development-token"
    );
}

#[test]
fn reports_environment_and_missing_variable_errors() {
    let missing_environment = probe()
        .args(["request", "get"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/0")
        .args(["--environment", "production", "--json"])
        .output()
        .expect("get command should run");
    assert_eq!(missing_environment.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&missing_environment.stdout).unwrap();
    assert_eq!(value["error"]["category"], "environment_not_found");

    let missing_variable = probe()
        .args(["request", "get"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/1")
        .args(["--environment", "development", "--json"])
        .output()
        .expect("get command should run");
    assert_eq!(missing_variable.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&missing_variable.stdout).unwrap();
    assert_eq!(value["error"]["category"], "missing_variable");
}

#[test]
fn reports_unavailable_collection_secret() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/2")
        .args(["--environment", "development", "--json"])
        .output()
        .expect("get command should run");

    assert_eq!(output.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "secret_variable_unavailable");
}

#[test]
fn reports_request_not_found_as_structured_error() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("unbundled"))
        .arg("missing.yml")
        .arg("--json")
        .output()
        .expect("get command should run");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["error"]["exitCode"], 4);
    assert_eq!(value["error"]["category"], "request_not_found");
}

#[test]
fn quiet_mode_preserves_failure_diagnostics_and_status() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("unbundled"))
        .arg("missing.yml")
        .arg("--quiet")
        .output()
        .expect("get command should run");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("request_not_found"));
}

#[test]
fn executes_request_as_deterministic_json() {
    let (server_url, server) = serve_once(b"{\"result\":\"ok\"}".to_vec(), "application/json");
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--json"])
        .output()
        .expect("run command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["request"]["method"], "POST");
    assert_eq!(value["response"]["status"], 200);
    assert_eq!(value["response"]["sizeBytes"], 15);
    assert_eq!(value["response"]["body"]["encoding"], "utf8");
    assert_eq!(value["response"]["body"]["content"], "{\"result\":\"ok\"}");
    let captured = server.join().unwrap();
    assert!(
        captured
            .head
            .starts_with("POST /echo?mode=cli HTTP/1.1\r\n")
    );
    assert!(
        captured
            .head
            .contains("authorization: Bearer cli-token\r\n")
    );
    assert!(captured.head.contains("x-probe: phase-five\r\n"));
    assert_eq!(captured.body, b"{\"source\":\"cli\"}");
    fs::remove_file(workspace).unwrap();
}

#[test]
fn writes_response_body_to_an_explicit_file() {
    let response_body = vec![0, 159, 146, 150];
    let (server_url, server) = serve_once(response_body.clone(), "application/octet-stream");
    let workspace = runtime_fixture(&server_url);
    let body_output = temporary_path("response.bin");
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--output"])
        .arg(&body_output)
        .arg("--json")
        .output()
        .expect("run command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["response"]["body"]["content"], Value::Null);
    assert_eq!(value["response"]["body"]["omitted"], false);
    assert_eq!(
        value["response"]["body"]["outputPath"],
        body_output.to_string_lossy().as_ref()
    );
    assert_eq!(fs::read(&body_output).unwrap(), response_body);
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
    fs::remove_file(body_output).unwrap();
}

#[test]
fn omits_large_response_body_from_stdout() {
    let response_body = vec![b'x'; 1024 * 1024 + 1];
    let (server_url, server) = serve_once(response_body, "text/plain");
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--json"])
        .output()
        .expect("run command should run");

    assert!(output.status.success());
    assert!(output.stdout.len() < 10_000);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["response"]["body"]["content"], Value::Null);
    assert_eq!(value["response"]["body"]["omitted"], true);
    assert_eq!(value["response"]["body"]["omissionReason"], "too_large");
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
}

#[test]
fn run_preflights_environment_resolution_before_http() {
    let output = probe()
        .args(["request", "run"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/1")
        .args(["--environment", "development", "--json"])
        .output()
        .expect("run command should run");

    assert_eq!(output.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "missing_variable");
}

#[test]
fn distinguishes_invalid_workspace() {
    let output = probe()
        .args(["collection", "validate"])
        .arg(fixture("does-not-exist.yml"))
        .arg("--json")
        .output()
        .expect("validate command should run");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["error"]["category"], "invalid_workspace");
}

#[test]
fn distinguishes_invalid_arguments() {
    let output = probe()
        .args(["unknown", "command", "--json"])
        .output()
        .expect("invalid command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["error"]["exitCode"], 2);
    assert_eq!(value["error"]["category"], "invalid_arguments");
}

#[test]
fn rejects_json_and_quiet_together_as_structured_error() {
    let output = probe()
        .args(["collection", "validate"])
        .arg(fixture("unbundled"))
        .args(["--json", "--quiet"])
        .output()
        .expect("validate command should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["error"]["category"], "invalid_arguments");
}
