#[allow(dead_code, unused_imports)]
mod common;
use common::*;

const SECRET: &str = "PR3_SECRET_MARKER";
const ASSERTIONS: &str = r#"runtime:
  assertions:
    - expression: response.status
      operator: equals
      value: '{{expectedStatus}}'
    - expression: response.body.name
      operator: equals
      value: '"John Doe"'
    - expression: sensitive-disabled-expression
      operator: invalid
      value: '{{missingSecret}}'
      disabled: true
"#;

fn workspace(url: &str, protocol: &str, assertions: &str, bundled: bool) -> PathBuf {
    let config = r#"config:
  environments:
    - name: local
      variables:
        - name: expectedStatus
          value: '200'
        - name: PR3_SECRET_MARKER
          secret: true
        - name: alias
          value: '{{PR3_SECRET_MARKER}}'
"#;
    let graphql = if protocol == "graphql" {
        "  body:\n    query: '{ name }'\n"
    } else {
        ""
    };
    let request = format!(
        "info:\n  name: Users\n  type: {protocol}\n{protocol}:\n  method: POST\n  url: {url}\n{graphql}{assertions}"
    );
    let root = format!(
        "opencollection: 1.0.0\ninfo:\n  name: CLI assertions\nbundled: {bundled}\n{config}"
    );
    let path = temporary_path(if bundled {
        "assertions.yml"
    } else {
        "assertions"
    });
    if bundled {
        let indented = request
            .lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>();
        fs::write(
            &path,
            format!("{root}items:\n  - {}", indented.trim_start()),
        )
        .unwrap();
    } else {
        fs::create_dir(&path).unwrap();
        fs::write(path.join("opencollection.yml"), root).unwrap();
        fs::write(path.join("users.yml"), request).unwrap();
    }
    path
}

fn run(path: &std::path::Path, bundled: bool, flags: &[&str]) -> std::process::Output {
    probe()
        .args(["request", "run"])
        .arg(path)
        .arg(if bundled { "items/0" } else { "users.yml" })
        .args(["--environment", "local"])
        .args(flags)
        .output()
        .unwrap()
}

fn cleanup(path: PathBuf) {
    if path.is_dir() {
        fs::remove_dir_all(path).unwrap();
    } else {
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn collection_and_cli_assertions_cover_both_layouts_and_protocols_without_writes() {
    for bundled in [true, false] {
        for protocol in ["http", "graphql"] {
            let (url, server) = serve_once(
                br#"{"name":"John Doe","items":[1],"deleted":null}"#.to_vec(),
                "application/json",
            );
            let path = workspace(&url, protocol, ASSERTIONS, bundled);
            let source_path = if bundled {
                path.clone()
            } else {
                path.join("users.yml")
            };
            let before = fs::read(&source_path).unwrap();
            let output = run(
                &path,
                bundled,
                &[
                    "--json",
                    "--var",
                    "expectedStatus=200",
                    "--assert",
                    "response.body.items.length greaterThan 0",
                    "--assert",
                    "response.body.deleted isNull",
                    "--assert",
                    "response.body.name equals \"John Doe\"",
                    "--assert",
                    "response.status equals {{expectedStatus}}",
                ],
            );
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            let results = value["assertions"].as_array().unwrap();
            assert_eq!(results.len(), 7);
            assert_eq!(
                results[0],
                serde_json::json!({"source":"collection","index":0,"outcome":"passed","actual":"200","actualTruncated":false})
            );
            assert_eq!(results[2]["outcome"], "disabled");
            assert_eq!(
                results[3],
                serde_json::json!({"source":"cli","index":0,"outcome":"passed","actual":"1","actualTruncated":false})
            );
            assert_eq!(results[6]["index"], 3);
            assert!(output.stderr.is_empty());
            assert_eq!(fs::read(source_path).unwrap(), before);
            server.join().unwrap();
            cleanup(path);
        }
    }
}

#[test]
fn mixed_failures_report_all_safe_outcomes_and_skip_only_collection_when_requested() {
    let assertions = format!(
        "{ASSERTIONS}    - expression: response.body.name\n      operator: equals\n      value: SECRET_EXPECTED\n    - expression: response.body.name\n      operator: equals\n      value: '{{{{alias}}}}'\n"
    );
    for bundled in [true, false] {
        for no_assertions in [false, true] {
            let (url, server) = serve_once(br#"{"name":"John Doe"}"#.to_vec(), "application/json");
            let path = workspace(&url, "http", &assertions, bundled);
            let mut flags = vec![
                "--json",
                "--assert",
                "response.status equals 201",
                "--assert",
                "response.status equals 200",
            ];
            if no_assertions {
                flags.push("--no-assertions");
            }
            let output = run(&path, bundled, &flags);
            assert_eq!(output.status.code(), Some(9));
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["error"]["category"], "assertion_failed");
            assert!(value.get("response").is_none());
            let results = value["error"]["details"]["assertions"].as_array().unwrap();
            assert_eq!(results.len(), if no_assertions { 2 } else { 7 });
            if !no_assertions {
                assert_eq!(results[3]["outcome"], "failed");
                assert_eq!(results[4]["error"]["category"], "variable_resolution");
            }
            assert_eq!(
                value["error"]["message"],
                if no_assertions {
                    "Assertions: 1 passed, 1 failed, 0 errors, 0 disabled"
                } else {
                    "Assertions: 3 passed, 2 failed, 1 errors, 1 disabled"
                }
            );
            let failed_status = &results[if no_assertions { 0 } else { 5 }];
            assert_eq!(failed_status["outcome"], "failed");
            assert_eq!(failed_status["actual"], "200");
            assert_eq!(failed_status["actualTruncated"], false);
            assert_eq!(results.last().unwrap()["outcome"], "passed");
            let text = String::from_utf8_lossy(&output.stdout);
            for sensitive in [
                "SECRET_EXPECTED",
                "response.body.name",
                SECRET,
                "sensitive-disabled-expression",
            ] {
                assert!(!text.contains(sensitive), "{text}");
            }
            server.join().unwrap();
            cleanup(path);
        }
    }
}

#[test]
fn human_and_quiet_summaries_keep_failures_and_exit_codes() {
    for quiet in [false, true] {
        for fail in [false, true] {
            let (url, server) = serve_once(b"null".to_vec(), "application/json");
            let path = workspace(&url, "http", "", true);
            let mut flags = vec![
                "--assert",
                if fail {
                    "response.status equals 201"
                } else {
                    "response.status equals 200"
                },
            ];
            if quiet {
                flags.push("--quiet");
            }
            let output = run(&path, true, &flags);
            assert_eq!(output.status.code(), Some(if fail { 9 } else { 0 }));
            if fail {
                assert!(output.stdout.is_empty());
                assert_eq!(
                    String::from_utf8(output.stderr).unwrap(),
                    if quiet {
                        "error[assertion_failed]: Assertions: 0 passed, 1 failed, 0 errors, 0 disabled\n"
                    } else {
                        "error[assertion_failed]: Assertions: 0 passed, 1 failed, 0 errors, 0 disabled\n\n  ✗ CLI assertion #1 — Failed\n    Got: 200\n"
                    }
                );
            } else if quiet {
                assert!(output.stdout.is_empty());
            } else {
                assert!(
                    String::from_utf8_lossy(&output.stdout)
                        .contains("Assertions: 1 passed, 0 failed, 0 errors, 0 disabled")
                );
            }
            server.join().unwrap();
            cleanup(path);
        }
    }
}

#[test]
fn human_and_quiet_mixed_summaries_count_disabled_assertions_and_errors() {
    let assertions = format!(
        "{ASSERTIONS}    - expression: response.status\n      operator: equals\n      value: '{{{{alias}}}}'\n    - expression: another-disabled-expression\n      operator: invalid\n      disabled: true\n    - expression: response.body.name\n      operator: equals\n      value: SECRET_EXPECTED\n      description: SECRET_DESCRIPTION\n"
    );
    for quiet in [false, true] {
        let (url, server) = serve_once(br#"{"name":"John Doe"}"#.to_vec(), "application/json");
        let path = workspace(&url, "http", &assertions, true);
        let mut flags = vec![
            "--assert",
            "response.status equals 201",
            "--assert",
            "response.body.missing isNull",
        ];
        if quiet {
            flags.push("--quiet");
        }
        let output = run(&path, true, &flags);
        assert_eq!(output.status.code(), Some(9));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            if quiet {
                "error[assertion_failed]: Assertions: 2 passed, 2 failed, 2 errors, 2 disabled\n"
            } else {
                concat!(
                    "error[assertion_failed]: Assertions: 2 passed, 2 failed, 2 errors, 2 disabled\n",
                    "\n  ! Collection assertion #4 — Error\n",
                    "    Reason: assertion variable resolution failed or a value is unavailable\n",
                    "\n  ✗ Collection assertion #6 — Failed\n    Got: John Doe\n",
                    "\n  ✗ CLI assertion #1 — Failed\n    Got: 200\n",
                    "\n  ! CLI assertion #2 — Error\n",
                    "    Reason: response property or array index does not exist\n",
                )
            }
        );
        server.join().unwrap();
        cleanup(path);
    }
}

#[test]
fn legacy_expect_migration_also_applies_to_non_run_commands() {
    for command in [
        vec!["request", "get", "missing.yml", "items/0"],
        vec!["collection", "validate", "missing.yml"],
    ] {
        for legacy in [
            vec!["--expect", "sensitive-value"],
            vec!["--expect=sensitive-value"],
        ] {
            let output = probe()
                .args(&command)
                .args(legacy)
                .arg("--json")
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(!text.contains("sensitive-value"));
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["error"]["category"], "invalid_arguments");
            assert_eq!(
                value["error"]["message"],
                "--expect was removed after Probe v1.1.1; migrate to --assert 'response.status equals 200' (repeat --assert for AND checks)"
            );
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn malformed_flags_and_legacy_migration_never_send_or_echo_values() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let path = workspace(
        &format!("http://{}", listener.local_addr().unwrap()),
        "http",
        ASSERTIONS,
        true,
    );
    for flags in [
        vec!["--assert"],
        vec!["--assert", "sensitive-value"],
        vec!["--assert", "response.status equals"],
        vec!["--assert", "response.status unknown sensitive-value"],
        vec!["--assert", " equals sensitive-value"],
        vec!["--assert", "response.status equals 200", "--dry-run"],
        vec!["--expect", "sensitive-value"],
        vec!["--expect=sensitive-value"],
    ] {
        let mut flags = flags;
        flags.push("--json");
        let output = run(&path, true, &flags);
        assert_eq!(output.status.code(), Some(2));
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("sensitive-value"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["error"]["category"], "invalid_arguments");
        if flags.iter().any(|flag| flag.starts_with("--expect")) {
            assert!(text.contains("v1.1.1") && text.contains("--assert"));
        }
    }
    let output = run(&path, true, &["--dry-run", "--json"]);
    assert!(output.status.success());
    assert!(
        serde_json::from_slice::<Value>(&output.stdout)
            .unwrap()
            .get("assertions")
            .is_none()
    );
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    cleanup(path);
}

#[test]
fn assertions_use_original_secret_response_but_all_display_modes_stay_redacted() {
    let assertions = "runtime:\n  assertions:\n    - expression: response.body.token\n      operator: equals\n      value: '{{alias}}'\n";
    for assertion_only_secret in [false, true] {
        for mode in ["--json", "--show-headers", "--quiet"] {
            let body = format!("{{\"token\":\"{SECRET}\"}}");
            let (url, server) = serve_once(body.into_bytes(), "application/json");
            let path = workspace(&url, "http", assertions, true);
            if !assertion_only_secret {
                let source = fs::read_to_string(&path).unwrap();
                // Add request usage so preparation and assertions share one secret.
                let source = source.replace("    runtime:", "      headers:\n        - name: X-Token\n          value: '{{alias}}'\n    runtime:");
                fs::write(&path, source).unwrap();
            }
            let output = probe()
                .args(["request", "run"])
                .arg(&path)
                .arg("items/0")
                .args([
                    "--environment",
                    "local",
                    "--secret-provider",
                    "env",
                    "--assert",
                    "response.body.token equals {{alias}}",
                    mode,
                ])
                .env(SECRET, SECRET)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            for bytes in [&output.stdout, &output.stderr] {
                assert!(!String::from_utf8_lossy(bytes).contains(SECRET));
            }
            if mode == "--json" {
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["assertions"][0]["outcome"], "passed");
                assert_eq!(value["assertions"][1]["outcome"], "passed");
                assert!(String::from_utf8_lossy(&output.stdout).contains("REDACTED"));
            }
            server.join().unwrap();
            cleanup(path);
        }
    }
}

#[test]
fn output_file_is_preserved_and_incomplete_body_cannot_false_pass() {
    let mut body = br#"{"ok":true}"#.to_vec();
    body.resize(MAX_IN_MEMORY_RESPONSE_BYTES + 10, b' ');
    for to_file in [false, true] {
        let (url, server) = serve_once(body.clone(), "application/json");
        let path = workspace(&url, "http", "", true);
        let output_path = temporary_path("assert-response.json");
        let mut command = probe();
        command
            .args(["request", "run"])
            .arg(&path)
            .arg("items/0")
            .args([
                "--assert",
                "response.body.ok equals true",
                "--assert",
                "response.status equals 200",
                "--json",
            ]);
        if to_file {
            command.arg("--output").arg(&output_path);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(9));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let results = &value["error"]["details"]["assertions"];
        assert_eq!(results[0]["error"]["category"], "body_unavailable");
        assert_eq!(results[1]["outcome"], "passed");
        if to_file {
            assert_eq!(fs::read(&output_path).unwrap(), body);
            fs::remove_file(output_path).unwrap();
        }
        server.join().unwrap();
        cleanup(path);
    }
}

#[test]
fn missing_assertion_secrets_can_be_skipped_without_changing_no_assertion_output() {
    for skip in [false, true] {
        let (url, server) = serve_once(b"null".to_vec(), "application/json");
        let path = workspace(
            &url,
            "http",
            "runtime:\n  assertions:\n    - expression: response.body\n      operator: equals\n      value: '{{alias}}'\n",
            true,
        );
        let flags = if skip {
            vec!["--no-assertions", "--json"]
        } else {
            vec!["--json"]
        };
        let output = run(&path, true, &flags);
        assert_eq!(output.status.code(), Some(if skip { 0 } else { 9 }));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        if skip {
            assert!(value.get("assertions").is_none());
            assert_eq!(value["response"]["status"], 200);
        } else {
            assert_eq!(
                value["error"]["details"]["assertions"][0]["error"]["category"],
                "variable_resolution"
            );
        }
        server.join().unwrap();
        cleanup(path);
    }
}

#[test]
fn cli_operators_preserve_typed_values_and_literal_whitespace() {
    let (url, server) = serve_once(
        br#"{"text":"  spaced  ","n":3,"empty":"","spaces":"  ","flag":true,"nil":null}"#.to_vec(),
        "application/json",
    );
    let path = workspace(&url, "http", "", true);
    let output = run(
        &path,
        true,
        &[
            "--json",
            "--no-assertions",
            "--assert",
            "response.body.text equals   spaced  ",
            "--assert",
            "response.body.text notEquals \"spaced\"",
            "--assert",
            "response.body.text contains spaced",
            "--assert",
            "response.body.text notContains missing",
            "--assert",
            "response.body.n greaterThan 2",
            "--assert",
            "response.body.n lessThan 4",
            "--assert",
            "response.body.nil isNull",
            "--assert",
            "response.body.n isNotNull",
            "--assert",
            "response.body.empty equals \"\"",
            "--assert",
            "response.body.empty equals ",
            "--assert",
            "response.body.spaces equals   ",
            "--assert",
            "response.body.flag equals true",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["assertions"].as_array().unwrap().len(), 12);
    server.join().unwrap();
    cleanup(path);
}

#[test]
fn assertion_secret_overrides_use_the_request_resolution_and_remain_private() {
    let (url, server) = serve_once(
        format!("{{\"token\":\"{SECRET}\"}}").into_bytes(),
        "application/json",
    );
    let path = workspace(&url, "http", "", true);
    let binding = format!("{SECRET}={SECRET}");
    let output = run(
        &path,
        true,
        &[
            "--json",
            "--var",
            &binding,
            "--assert",
            "response.body.token equals {{alias}}",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
    server.join().unwrap();
    cleanup(path);
}

#[test]
fn configuration_and_transport_errors_keep_precedence_over_assertions() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let path = workspace(&url, "http", ASSERTIONS, true);
    let output = run(
        &path,
        true,
        &["--json", "--assert", "response.status equals {{alias}}"],
    );
    assert_eq!(output.status.code(), Some(6));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "network_execution");
    assert!(value["error"]["details"].get("assertions").is_none());
    let output = probe()
        .args(["request", "run"])
        .arg(&path)
        .arg("items/0")
        .args([
            "--environment",
            "absent",
            "--assert",
            "response.status equals 200",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "environment_not_found");
    cleanup(path);
}

#[test]
fn disabled_only_assertions_never_resolve_values_or_parse_the_body() {
    let (url, server) = serve_once(b"not JSON".to_vec(), "text/plain");
    let path = workspace(
        &url,
        "http",
        "runtime:\n  assertions:\n    - expression: sensitive-expression\n      operator: invalid\n      value: '{{alias}}'\n      disabled: true\n",
        true,
    );
    let output = run(&path, true, &["--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["assertions"],
        serde_json::json!([{ "source": "collection", "index": 0, "outcome": "disabled" }])
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sensitive-expression"));
    server.join().unwrap();
    cleanup(path);
}

#[test]
fn no_assertions_alone_omits_the_human_assertion_summary() {
    let (url, server) = serve_once(br#"{"name":"John Doe"}"#.to_vec(), "application/json");
    let path = workspace(&url, "http", ASSERTIONS, true);
    let output = run(&path, true, &["--no-assertions"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let human = String::from_utf8(output.stdout).unwrap();
    assert!(human.contains("200"));
    assert!(human.contains("John Doe"));
    assert!(!human.contains("Assertions:"));
    server.join().unwrap();
    cleanup(path);
}

#[test]
fn overlapping_assertion_and_request_secrets_never_leave_echoed_fragments_in_output() {
    // Exercise both declaration orders, and request-plus-assertion disclosure.
    for request_uses_short in [false, true] {
        for short_first in [false, true] {
            for mode in ["--json", "human", "--show-headers", "failure"] {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let url = format!("http://{}", listener.local_addr().unwrap());
                let server = thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        stream.read_exact(&mut byte).unwrap();
                        head.push(byte[0]);
                    }
                    let body = br#"{"echo":"abcdef abc"}"#;
                    write!(stream, "HTTP/1.1 200 abcdef\r\nX-Echo: abcdef abc\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                    stream.write_all(body).unwrap();
                });
                let path = workspace(&url, "http", "", true);
                let mut source = fs::read_to_string(&path).unwrap();
                source = source.replace("        - name: expectedStatus", "        - name: short\n          secret: true\n        - name: long\n          secret: true\n        - name: expectedStatus");
                if request_uses_short {
                    source.push_str(
                        "      headers:\n        - name: X-Token\n          value: '{{short}}'\n",
                    );
                }
                let names = if short_first {
                    ["short", "long"]
                } else {
                    ["long", "short"]
                };
                source.push_str("    runtime:\n      assertions:\n");
                for name in names {
                    source.push_str(&format!("        - expression: response.body.echo\n          operator: contains\n          value: '{{{{{name}}}}}'\n"));
                }
                fs::write(&path, source).unwrap();
                let mut command = probe();
                command
                    .args(["request", "run"])
                    .arg(&path)
                    .arg("items/0")
                    .args(["--environment", "local", "--secret-provider", "env"])
                    .env("short", "abc")
                    .env("long", "abcdef");
                match mode {
                    "human" => {}
                    "failure" => {
                        command.args(["--assert", "response.status equals 201"]);
                    }
                    flag => {
                        command.arg(flag);
                    }
                }
                let output = command.output().unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(if mode == "failure" { 9 } else { 0 }),
                    "{output:?}"
                );
                for bytes in [&output.stdout, &output.stderr] {
                    let text = String::from_utf8_lossy(bytes);
                    for fragment in ["abcdef", "abc", "def"] {
                        assert!(
                            !text.contains(fragment),
                            "leaked {fragment} in {mode}: {text}"
                        );
                    }
                }
                if mode == "failure" {
                    assert!(String::from_utf8_lossy(&output.stderr).contains("assertion_failed"));
                } else {
                    assert!(output.stderr.is_empty());
                    let text = String::from_utf8_lossy(&output.stdout);
                    assert!(text.contains("[REDACTED] [REDACTED]"), "{text}");
                    if mode == "--json" {
                        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                        assert!(
                            value["assertions"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .all(|result| result["outcome"] == "passed")
                        );
                    }
                    if mode == "--show-headers" {
                        assert!(text.contains("x-echo"));
                    }
                }
                server.join().unwrap();
                cleanup(path);
            }
        }
    }
}

#[test]
fn failure_actual_is_bounded_and_marks_truncation_in_human_and_json() {
    for mode in ["--json", "human"] {
        let (url, server) = serve_once(
            serde_json::to_vec(&"é".repeat(600)).unwrap(),
            "application/json",
        );
        let path = workspace(&url, "http", "", true);
        let mut flags = vec!["--assert", "response.body equals short"];
        if mode == "--json" {
            flags.push(mode);
        }
        let output = run(&path, true, &flags);
        assert_eq!(output.status.code(), Some(9));
        let actual = format!("{}… [truncated]", "é".repeat(512));
        if mode == "--json" {
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                value["error"]["message"],
                "Assertions: 0 passed, 1 failed, 0 errors, 0 disabled"
            );
            let result = &value["error"]["details"]["assertions"][0];
            assert_eq!(result["outcome"], "failed");
            assert_eq!(result["actual"], actual);
            assert_eq!(result["actualTruncated"], true);
        } else {
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(&format!("Got: {actual}\n")), "{stderr}");
        }
        for bytes in [&output.stdout, &output.stderr] {
            assert!(!String::from_utf8_lossy(bytes).contains(&"é".repeat(513)));
        }
        server.join().unwrap();
        cleanup(path);
    }
}

#[test]
fn human_failure_actual_escapes_terminal_controls_and_preserves_unicode() {
    let actual = "Jane\nforged line\roverwrite\u{1b}[31mé界🙂\u{1b}[0m";
    for json in [false, true] {
        let (url, server) = serve_once(serde_json::to_vec(actual).unwrap(), "application/json");
        let path = workspace(&url, "http", "", true);
        let mut flags = vec!["--assert", "response.body equals short"];
        if json {
            flags.push("--json");
        }
        let output = run(&path, true, &flags);
        assert_eq!(output.status.code(), Some(9));
        if json {
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                value["error"]["details"]["assertions"][0],
                serde_json::json!({
                    "source": "cli", "index": 0, "outcome": "failed",
                    "actual": actual, "actualTruncated": false,
                })
            );
        } else {
            assert!(output.stdout.is_empty());
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                concat!(
                    "error[assertion_failed]: Assertions: 0 passed, 1 failed, 0 errors, 0 disabled\n",
                    "\n  ✗ CLI assertion #1 — Failed\n",
                    "    Got: Jane\\nforged line\\roverwrite\\u{1b}[31mé界🙂\\u{1b}[0m\n",
                )
            );
        }
        server.join().unwrap();
        cleanup(path);
    }
}

#[test]
fn human_summary_has_one_blank_line_after_response_body() {
    for body in ["body", "body\n", "body\r\n", "body\n\n", "body  \n"] {
        let (url, server) = serve_once(body.as_bytes().to_vec(), "text/plain");
        let path = workspace(&url, "http", "", true);
        let output = run(&path, true, &["--assert", "response.status equals 200"]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let human = String::from_utf8(output.stdout).unwrap();
        assert!(
            human.ends_with(&format!(
                "{}\n\nAssertions: 1 passed, 0 failed, 0 errors, 0 disabled\n",
                body.trim_end_matches(['\r', '\n'])
            )),
            "{human}"
        );
        server.join().unwrap();
        cleanup(path);
    }
}

#[test]
fn failed_human_actuals_use_secrets_disclosed_by_later_assertions() {
    let assertions = "runtime:\n  assertions:\n    - expression: response.body.token\n      operator: equals\n      value: SECRET_EXPECTED\n      description: SECRET_DESCRIPTION\n    - expression: response.body.token\n      operator: equals\n      value: '{{alias}}'\n";
    let (url, server) = serve_once(
        format!("{{\"token\":\"{SECRET}\"}}").into_bytes(),
        "application/json",
    );
    let path = workspace(&url, "http", assertions, true);
    let output = probe()
        .args(["request", "run"])
        .arg(&path)
        .arg("items/0")
        .args(["--environment", "local", "--secret-provider", "env"])
        .env(SECRET, SECRET)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(9));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        concat!(
            "error[assertion_failed]: Assertions: 1 passed, 1 failed, 0 errors, 0 disabled\n",
            "\n  ✗ Collection assertion #1 — Failed\n    Got: [REDACTED]\n",
        )
    );
    server.join().unwrap();
    cleanup(path);
}
