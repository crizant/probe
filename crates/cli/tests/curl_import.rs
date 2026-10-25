#[allow(dead_code, unused_imports)]
mod common;

use common::*;
use std::{collections::BTreeMap, path::Path, process::Output};

fn import(path: &str, options: &[&str], source: &str) -> Output {
    let mut child = probe()
        .args(["request", "import", "curl", path])
        .args(options)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(source.as_bytes()) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

fn workspace(unbundled: bool) -> PathBuf {
    let path = temporary_path(if unbundled {
        "curl-workspace"
    } else {
        "curl.yml"
    });
    if unbundled {
        copy_directory(&fixture("phase16-unbundled"), &path);
    } else {
        fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    }
    path
}

fn documents(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    if path.is_file() {
        return BTreeMap::from([(path.to_owned(), fs::read(path).unwrap())]);
    }
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir()
            || matches!(
                path.extension().and_then(|v| v.to_str()),
                Some("yml" | "yaml")
            )
        {
            result.extend(documents(&path));
        }
    }
    result
}

fn cleanup(path: PathBuf) {
    if path.is_dir() {
        fs::remove_dir_all(path).unwrap();
    } else {
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn stdin_and_file_import_preserve_the_complete_request_in_both_layouts() {
    for unbundled in [false, true] {
        for from_file in [false, true] {
            let path = workspace(unbundled);
            let path_str = path.to_str().unwrap();
            let parent = if unbundled { "group" } else { "items/1" };
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let authentication = if from_file {
                "-u 'user:password-secret'"
            } else {
                "-H 'Authorization: Bearer literal-secret'"
            };
            let source = format!(
                "curl 'http://{}/import?a=1&a=2' -X PATCH -H 'X-Test: first' -H 'X-Test: second' {authentication} -b 'session=cookie-secret' --data-raw 'body-secret' -L --max-redirs 3 --max-time 1.25",
                listener.local_addr().unwrap()
            );
            let file = temporary_path("command.txt");
            let mut options = vec![
                "--name", "Imported", "--parent", parent, "--index", "0", "--json",
            ];
            if from_file {
                fs::write(&file, &source).unwrap();
                options.extend(["--file", file.to_str().unwrap()]);
            }
            // File mode must ignore malformed stdin.
            let output = import(
                path_str,
                &options,
                if from_file { "not curl" } else { &source },
            );
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty());
            let created: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(created["schemaVersion"], 1);
            assert_eq!(created["operation"], "import");
            assert_eq!(created["itemType"], "request");
            assert_eq!(created["parent"], parent);
            assert_eq!(created["index"], 0);
            assert!(created["previousSelector"].is_null());
            let selector = created["selector"].as_str().unwrap();
            assert_eq!(
                selector,
                if unbundled {
                    "group/imported.yml"
                } else {
                    "items/1/items/0"
                }
            );
            let diagnostics = String::from_utf8(output.stdout).unwrap();
            for secret in [
                "literal-secret",
                "cookie-secret",
                "password-secret",
                "body-secret",
            ] {
                assert!(!diagnostics.contains(secret));
            }
            let loaded = probe_opencollection::load_workspace(&path).unwrap();
            let actual = loaded
                .workspace()
                .request(loaded.request_key(selector).unwrap())
                .unwrap();
            let expected = probe_application::parse_curl(&source).unwrap();
            assert_eq!(actual.metadata.name.as_deref(), Some("Imported"));
            assert_eq!(actual.method, expected.method);
            assert_eq!(actual.url, expected.url);
            assert_eq!(actual.headers, expected.headers);
            assert_eq!(actual.http_body(), expected.http_body());
            assert_eq!(actual.authentication, expected.authentication);
            assert_eq!(actual.settings, expected.settings);
            let inspected = run_json(&["request", "get", path_str, selector]);
            assert_eq!(inspected["name"], "Imported");
            assert_eq!(inspected["method"], "PATCH");
            run_json(&["request", "list", path_str]);
            run_json(&["collection", "validate", path_str]);
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
            if from_file {
                fs::remove_file(file).unwrap();
            }
            cleanup(path);
        }
    }
}

#[test]
fn root_import_appends_and_keeps_existing_requests() {
    for unbundled in [false, true] {
        let path = workspace(unbundled);
        let path_str = path.to_str().unwrap();
        let before = run_json(&["request", "list", path_str]);
        let output = import(
            path_str,
            &["--name", "Root imported"],
            "curl https://example.com",
        );
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .starts_with("import request: ")
        );
        let after = run_json(&["request", "list", path_str]);
        assert_eq!(
            after["requests"].as_array().unwrap().len(),
            before["requests"].as_array().unwrap().len() + 1
        );
        assert_eq!(after["requests"][0], before["requests"][0]);
        let imported = after["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|request| request["name"] == "Root imported")
            .unwrap();
        let loaded = probe_opencollection::load_workspace(&path).unwrap();
        let key = loaded
            .request_key(imported["selector"].as_str().unwrap())
            .unwrap();
        let settings = &loaded.workspace().request(key).unwrap().settings;
        assert_eq!(settings.follow_redirects, Some(false));
        assert_eq!(settings.max_redirects, Some(50));

        cleanup(path);
    }
}

#[test]
fn malformed_input_and_failed_destinations_leave_no_partial_request() {
    for unbundled in [false, true] {
        let path = workspace(unbundled);
        let baseline = documents(&path);
        for (options, source, category, exit) in [
            (
                vec!["--name", "Imported"],
                "curl https://example.com --data-raw 'body-secret",
                "invalid_import",
                3,
            ),
            (
                vec!["--name", "Imported"],
                "curl https://example.com -H 'Authorization: Bearer literal-secret' --unknown password-secret",
                "invalid_import",
                3,
            ),
            (vec!["--name", "Imported"], "", "invalid_import", 3),
            (
                vec!["--name", "Imported", "--parent", "missing"],
                "curl https://example.com",
                "destination_not_found",
                2,
            ),
            (
                vec!["--name", "Imported", "--index", "999"],
                "curl https://example.com",
                "invalid_index",
                2,
            ),
            (vec![], "curl https://example.com", "invalid_arguments", 2),
            (
                vec!["--name", " "],
                "curl https://example.com",
                "invalid_arguments",
                2,
            ),
        ] {
            let mut options = options;
            options.push("--json");
            let output = import(path.to_str().unwrap(), &options, source);
            assert_eq!(output.status.code(), Some(exit), "{output:?}");
            assert!(output.stderr.is_empty());
            let error: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(error["schemaVersion"], 1);
            assert_eq!(error["error"]["category"], category);
            assert_eq!(error["error"]["exitCode"], exit);
            assert!(!String::from_utf8(output.stdout).unwrap().contains("secret"));
            assert_eq!(documents(&path), baseline);
        }
        cleanup(path);
    }
}

#[test]
fn missing_workspace_file_and_read_only_stdin_use_existing_error_contracts() {
    let missing = temporary_path("missing-curl.yml");
    let path = missing.to_str().unwrap();
    for (destination, options, expected) in [
        (path, vec!["--name", "Imported"], "invalid_workspace"),
        ("-", vec!["--name", "Imported"], "invalid_arguments"),
        (
            path,
            vec!["--name", "Imported", "--file", path],
            "invalid_import",
        ),
    ] {
        let mut options = options;
        options.push("--json");
        let output = import(destination, &options, "curl https://example.com");
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["category"], expected);
        assert!(!missing.exists());
    }
    let path = workspace(true);
    let baseline = documents(&path);
    let output = import(
        path.to_str().unwrap(),
        &["--name", "Alpha", "--json"],
        "curl https://example.com",
    );
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["category"], "duplicate_destination");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(documents(&path), baseline);
    cleanup(path);
}

#[test]
fn human_parse_errors_are_safe_and_shell_commands_are_not_executed() {
    let path = workspace(false);
    let marker = temporary_path("curl-must-not-execute");
    let source = format!(
        "curl https://example.com -d body-secret; touch {}",
        marker.display()
    );
    let before = fs::read(&path).unwrap();
    let output = import(path.to_str().unwrap(), &["--name", "Imported"], &source);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("error[invalid_import]:"));
    assert!(!stderr.contains("body-secret"));
    assert!(!marker.exists());
    assert_eq!(fs::read(&path).unwrap(), before);
    cleanup(path);
}

#[test]
fn input_read_failures_and_invalid_utf8_do_not_expose_input() {
    struct FailingReader;
    impl Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("credential-secret"))
        }
    }
    let path = workspace(false);
    let before = fs::read(&path).unwrap();
    let args = [
        "request",
        "import",
        "curl",
        path.to_str().unwrap(),
        "--name",
        "Imported",
        "--json",
    ];
    let output = probe_cli::run_with_stdin(args, &mut FailingReader);
    assert_eq!(output.exit_code, 3);
    let error: Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(error["error"]["category"], "stdin_error");
    assert!(!output.stdout.contains("credential-secret"));
    let output = probe_cli::run_with_stdin(args, &mut &b"curl credential-secret\xff"[..]);
    assert_eq!(output.exit_code, 3);
    assert!(!output.stdout.contains("credential-secret"));
    let file = temporary_path("invalid-curl.txt");
    fs::write(&file, b"credential-secret\xff").unwrap();
    let output = import(
        path.to_str().unwrap(),
        &[
            "--name",
            "Imported",
            "--file",
            file.to_str().unwrap(),
            "--json",
        ],
        "",
    );
    assert_eq!(output.status.code(), Some(3));
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["category"], "invalid_import");
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("credential-secret")
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(file).unwrap();
    cleanup(path);
}

#[test]
fn curl_import_help_and_argument_validation_follow_cli_conventions() {
    let help = probe()
        .args(["request", "import", "curl", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for text in [
        "import curl",
        "--name",
        "--file",
        "--parent",
        "--index",
        "plaintext",
    ] {
        assert!(help.contains(text), "{help}");
    }
    for args in [
        vec!["--name", "Imported", "--file"],
        vec!["--name", "Imported", "--file", "--parent", "folder"],
        vec!["--name", "Imported", "--file", "a", "--file", "b"],
        vec!["--name", "Imported", "--name", "Other"],
        vec!["--name", "Imported", "--parent", "a", "--parent", "b"],
        vec!["--name", "Imported", "--index", "-1"],
        vec!["--name", "Imported", "--index", "0", "--index", "1"],
        vec!["--name", "Imported", "--url", "https://example.com"],
    ] {
        let mut args = args;
        args.push("--json");
        let output = import("missing.yml", &args, "");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["category"], "invalid_arguments");
    }
}

fn with_terminal_status(
    args: &[&str],
    stdin: &mut impl Read,
    terminal: bool,
) -> probe_cli::RunOutput {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = probe_cli::run_with_io_and_terminal(
        args,
        stdin,
        terminal,
        &mut stdout,
        &mut stderr,
        |_| panic!("cURL import must not start WebSocket input"),
    )
    .unwrap();
    probe_cli::RunOutput {
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
        exit_code,
    }
}

#[test]
fn terminal_stdin_fails_without_reading_but_file_and_redirected_input_work() {
    struct MustNotRead;
    impl Read for MustNotRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("interactive stdin must not be read");
        }
    }
    for unbundled in [false, true] {
        let path = workspace(unbundled);
        let before = documents(&path);
        for json in [false, true] {
            let mut args = vec![
                "request",
                "import",
                "curl",
                path.to_str().unwrap(),
                "--name",
                "Imported",
            ];
            if json {
                args.push("--json");
            }
            let output = with_terminal_status(&args, &mut MustNotRead, true);
            assert_eq!(output.exit_code, 2);
            let message = if json {
                assert!(output.stderr.is_empty());
                let error: Value = serde_json::from_str(&output.stdout).unwrap();
                assert_eq!(error["schemaVersion"], 1);
                assert_eq!(error["error"]["category"], "invalid_arguments");
                assert_eq!(error["error"]["exitCode"], 2);
                error["error"]["message"].as_str().unwrap().to_owned()
            } else {
                assert!(output.stdout.is_empty());
                assert!(output.stderr.starts_with("error[invalid_arguments]:"));
                output.stderr
            };
            assert!(message.contains("pipe a cURL command"));
            assert!(message.contains("--file <path>"));
            assert_eq!(documents(&path), before);
        }
        let file = temporary_path("terminal-curl.txt");
        fs::write(&file, "curl https://example.com").unwrap();
        let output = with_terminal_status(
            &[
                "request",
                "import",
                "curl",
                path.to_str().unwrap(),
                "--name",
                "File imported",
                "--file",
                file.to_str().unwrap(),
            ],
            &mut MustNotRead,
            true,
        );
        assert_eq!(output.exit_code, 0, "{output:?}");
        let output = with_terminal_status(
            &[
                "request",
                "import",
                "curl",
                path.to_str().unwrap(),
                "--name",
                "Redirected imported",
            ],
            &mut &b"curl https://example.com"[..],
            false,
        );
        assert_eq!(output.exit_code, 0, "{output:?}");
        fs::remove_file(file).unwrap();
        cleanup(path);
    }
}

#[test]
fn file_read_and_utf8_errors_are_distinct_safe_and_leave_workspace_unchanged() {
    let root = temporary_path("curl-read-errors");
    fs::create_dir(&root).unwrap();
    let missing = root.join("missing");
    let invalid_utf8 = root.join("invalid.txt");
    fs::write(&invalid_utf8, b"credential-secret\xff").unwrap();
    for unbundled in [false, true] {
        let path = workspace(unbundled);
        let before = documents(&path);
        for (file, message) in [
            (&missing, "cannot read cURL command file"),
            (&root, "cannot read cURL command file"),
            (&invalid_utf8, "cURL command file is not valid UTF-8"),
        ] {
            for json in [false, true] {
                let mut options = vec!["--name", "Imported", "--file", file.to_str().unwrap()];
                if json {
                    options.push("--json");
                }
                let output = import(path.to_str().unwrap(), &options, "");
                assert_eq!(output.status.code(), Some(3));
                if json {
                    assert!(output.stderr.is_empty());
                    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
                    assert_eq!(error["schemaVersion"], 1);
                    assert_eq!(error["error"]["category"], "invalid_import");
                    assert_eq!(error["error"]["exitCode"], 3);
                    assert_eq!(error["error"]["message"], message);
                } else {
                    assert!(output.stdout.is_empty());
                    assert_eq!(
                        String::from_utf8(output.stderr).unwrap(),
                        format!("error[invalid_import]: {message}\n")
                    );
                }
                assert_eq!(documents(&path), before);
            }
        }
        cleanup(path);
    }
    fs::remove_dir_all(root).unwrap();
}
