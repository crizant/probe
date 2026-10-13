#[allow(dead_code, unused_imports)]
mod common;

use common::*;
use unicode_width::UnicodeWidthStr;

#[test]
fn piped_request_details_match_goldens_and_keep_json_contract() {
    let collection = fixture("request-human.yml");
    let populated = concat!(
        "Name: Get user\nSelector: items/0\nType: http\nEnvironment: <none>\n",
        "Method: GET\nURL: https://example.com/users/:id\nDescription: <unset>\nDocs: <unset>\n",
        "Headers\nNAME     VALUE             STATE\nAccept   application/json\nX-Debug  true              disabled\n",
        "Path parameters\nNAME    VALUE        STATE\nid      123\nunused  {{literal}}  disabled\n",
        "Query parameters\nNAME    VALUE  STATE\nlimit   20\noffset  10     disabled\n",
        "Body: <unset>\nAuthentication: <unset>\n",
    );
    let empty = concat!(
        "Name: Empty\nSelector: items/1\nType: http\nEnvironment: <none>\n",
        "Method: GET\nURL: https://example.com\nDescription: <unset>\nDocs: <unset>\n",
        "Headers\n  (none)\nPath parameters\n  (none)\nQuery parameters\n  (none)\n",
        "Body: <unset>\nAuthentication: <unset>\n",
    );
    for (selector, expected) in [("items/0", populated), ("items/1", empty)] {
        for columns in ["1", "999", "invalid"] {
            let output = probe()
                .args(["request", "get"])
                .arg(&collection)
                .arg(selector)
                .env("COLUMNS", columns)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(output.stdout, expected.as_bytes());
        }
    }
    let expected = serde_json::json!({
        "schemaVersion": 1, "name": "Get user", "selector": "items/0", "type": "http",
        "environment": null, "method": "GET", "url": "https://example.com/users/:id",
        "description": null, "docs": null, "body": null, "authentication": null, "graphql": null,
        "headers": [
            { "name": "Accept", "value": "application/json", "disabled": false },
            { "name": "X-Debug", "value": "true", "disabled": true }
        ],
        "pathParameters": [
            { "name": "id", "value": "123", "disabled": false },
            { "name": "unused", "value": "{{literal}}", "disabled": true }
        ],
        "queryParameters": [
            { "name": "limit", "value": "20", "disabled": false },
            { "name": "offset", "value": "10", "disabled": true }
        ],
    });
    assert_eq!(
        run_json(&["request", "get", collection.to_str().unwrap(), "items/0"]),
        expected
    );
}

#[test]
fn request_set_confirms_selector_without_dumping_details_and_keeps_json() {
    let collection = temporary_path("set-human.yml");
    fs::copy(fixture("request-human.yml"), &collection).unwrap();
    let output = probe()
        .args(["request", "set"])
        .arg(&collection)
        .args(["items/0", "--name", "Updated user"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"Updated request: items/0\n");
    let updated = run_json(&["request", "get", collection.to_str().unwrap(), "items/0"]);
    assert_eq!(updated["name"], "Updated user");
    assert_eq!(
        run_json(&[
            "request",
            "set",
            collection.to_str().unwrap(),
            "items/0",
            "--name",
            "Updated user"
        ]),
        updated
    );
    fs::remove_file(collection).unwrap();
}

#[test]
fn request_detail_tables_bound_long_names_and_values_without_changing_json() {
    let collection = temporary_path("detail-tables.yml");
    fs::copy(fixture("request-human.yml"), &collection).unwrap();
    let name = "測試".repeat(30);
    let value = "value".repeat(80);
    let entries =
        serde_json::json!([{ "name": name, "value": value, "disabled": true }]).to_string();
    run_json(&[
        "request",
        "set",
        collection.to_str().unwrap(),
        "items/0",
        "--headers",
        &entries,
        "--path-parameters",
        &entries,
        "--query-parameters",
        &entries,
    ]);
    for columns in ["10", "999"] {
        let output = probe()
            .args(["request", "get"])
            .arg(&collection)
            .arg("items/0")
            .env("COLUMNS", columns)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let human = String::from_utf8(output.stdout).unwrap();
        for section in ["Headers", "Path parameters", "Query parameters"] {
            let mut lines = human.split(&format!("{section}\n")).nth(1).unwrap().lines();
            assert_eq!(
                lines.next().unwrap().split_whitespace().collect::<Vec<_>>(),
                ["NAME", "VALUE", "STATE"]
            );
            let row = lines.next().unwrap();
            assert_eq!(row.width(), 120);
            assert_eq!(row.matches('…').count(), 2, "{row}");
            assert!(row.ends_with("disabled"));
        }
    }
    let json = run_json(&["request", "get", collection.to_str().unwrap(), "items/0"]);
    for field in ["headers", "pathParameters", "queryParameters"] {
        assert_eq!(
            json[field],
            serde_json::from_str::<Value>(&entries).unwrap()
        );
    }
    fs::remove_file(collection).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn request_details_use_narrow_stdout_terminal_and_piped_fallback() {
    let collection = fixture("request-human.yml");
    for width in [20, 42] {
        let terminal = Command::new("script").args(["-q", "-e", "-c",
            "stty rows 24 cols \"$PROBE_TABLE_WIDTH\"; \"$PROBE_TABLE_BINARY\" request get \"$PROBE_TABLE_COLLECTION\" items/0", "/dev/null"])
            .env("PROBE_TABLE_WIDTH", width.to_string())
            .env("PROBE_TABLE_BINARY", env!("CARGO_BIN_EXE_probe"))
            .env("PROBE_TABLE_COLLECTION", &collection).output().unwrap();
        assert!(terminal.status.success(), "{terminal:?}");
        let human = String::from_utf8(terminal.stdout)
            .unwrap()
            .replace("\r\n", "\n");
        assert!(human.starts_with("Name: Get user\nSelector: items/0\n"));
        for title in ["Headers", "Path parameters", "Query parameters"] {
            let lines = human
                .split(&format!("{title}\n"))
                .nth(1)
                .unwrap()
                .lines()
                .take(3)
                .collect::<Vec<_>>();
            assert!(lines.iter().all(|line| line.width() <= width), "{human}");
            assert!(lines[2].ends_with("disabled"), "{human}");
        }
    }
    let destination = temporary_path("redirected-request-get.txt");
    let redirected = Command::new("script").args(["-q", "-e", "-c",
        "stty rows 24 cols 20; \"$PROBE_TABLE_BINARY\" request get \"$PROBE_TABLE_COLLECTION\" items/0 > \"$PROBE_TABLE_OUTPUT\"", "/dev/null"])
        .env("PROBE_TABLE_BINARY", env!("CARGO_BIN_EXE_probe"))
        .env("PROBE_TABLE_COLLECTION", &collection).env("PROBE_TABLE_OUTPUT", &destination).output().unwrap();
    assert!(redirected.status.success(), "{redirected:?}");
    let piped = probe()
        .args(["request", "get"])
        .arg(&collection)
        .arg("items/0")
        .output()
        .unwrap();
    assert!(piped.status.success());
    assert_eq!(fs::read(&destination).unwrap(), piped.stdout);
    fs::remove_file(destination).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn response_summary_bounds_long_reason_in_a_narrow_terminal() {
    let (url, server) = serve_once_with_status(
        b"hello".to_vec(),
        "text/plain",
        431,
        "Request Header Fields Too Large",
    );
    let collection = runtime_fixture(&url);
    let terminal = Command::new("script").args(["-q", "-e", "-c",
        "stty rows 24 cols 32; \"$PROBE_TABLE_BINARY\" request run \"$PROBE_TABLE_COLLECTION\" items/0 --environment local", "/dev/null"])
        .env("PROBE_TABLE_BINARY", env!("CARGO_BIN_EXE_probe"))
        .env("PROBE_TABLE_COLLECTION", &collection).output().unwrap();
    assert!(terminal.status.success(), "{terminal:?}");
    let human = String::from_utf8(terminal.stdout)
        .unwrap()
        .replace("\r\n", "\n");
    let lines = human.lines().skip(2).take(2).collect::<Vec<_>>();
    assert!(lines.iter().all(|line| line.width() <= 32), "{human}");
    assert_eq!(
        lines[0].split_whitespace().collect::<Vec<_>>(),
        ["STATUS", "TIME", "SIZE"]
    );
    assert!(lines[1].starts_with("431 Request"), "{human}");
    assert!(lines[1].contains('…'), "{human}");
    assert!(lines[1].ends_with("5 B"), "{human}");
    assert!(human.ends_with("\n\nhello\n"));
    server.join().unwrap();
    fs::remove_file(collection).unwrap();
}

#[test]
fn piped_request_list_matches_the_byte_exact_golden() {
    let expected = b"SELECTOR         TYPE  METHOD  NAME    URL\nitems/0          http  GET     Alpha   https://example.com/alpha\nitems/1/items/0  http  POST    Nested  https://example.com/nested\n";
    for columns in ["1", "999", "invalid"] {
        let output = probe()
            .args(["request", "list"])
            .arg(fixture("phase16-bundled.yml"))
            .env("COLUMNS", columns)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout, expected);
    }
}

#[test]
fn folder_parent_selectors_are_capped_without_changing_json() {
    let workspace = temporary_path("parent-column-cap.yml");
    let mut items = Vec::new();
    for depth in (0..8).rev() {
        items = vec![
            serde_json::json!({"info": {"name": format!("Level {depth}"), "type": "folder"}, "items": items}),
        ];
    }
    let source = serde_json::json!({"opencollection": "1.0.0", "info": {"name": "Parents"}, "bundled": true, "items": items});
    fs::write(&workspace, serde_json::to_vec(&source).unwrap()).unwrap();
    let output = probe()
        .args(["folder", "list"])
        .arg(&workspace)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let human = String::from_utf8(output.stdout).unwrap();
    for line in human.lines().skip(2) {
        let parent = line.split_whitespace().last().unwrap();
        assert!(parent.width() <= 28, "{parent}");
    }
    let deepest_parent = human
        .lines()
        .last()
        .unwrap()
        .split_whitespace()
        .last()
        .unwrap();
    assert!(deepest_parent.contains('…'));
    assert!(deepest_parent.ends_with("items/0"));
    let listed = run_json(&["folder", "list", workspace.to_str().unwrap()]);
    assert_eq!(listed["folders"][7]["parent"], ["items/0"; 7].join("/"));
    fs::remove_file(workspace).unwrap();
}

// util-linux script supplies a controlled terminal on Linux, independent of the
// developer's terminal. The portable formatter tests cover every width directly.
#[cfg(target_os = "linux")]
#[test]
fn terminal_size_is_used_only_when_stdout_is_a_terminal() {
    let collection = fixture("phase16-bundled.yml");
    let terminal = Command::new("script")
        .args([
            "-q",
            "-e",
            "-c",
            "stty rows 24 cols 42; \"$PROBE_TABLE_BINARY\" request list \"$PROBE_TABLE_COLLECTION\"",
            "/dev/null",
        ])
        .env("PROBE_TABLE_BINARY", env!("CARGO_BIN_EXE_probe"))
        .env("PROBE_TABLE_COLLECTION", &collection)
        .output()
        .unwrap();
    assert!(terminal.status.success(), "{terminal:?}");
    let human = String::from_utf8(terminal.stdout).unwrap();
    assert_eq!(human.lines().count(), 3);
    assert!(human.contains('…'));
    assert!(human.lines().all(|line| line.width() <= 42), "{human}");

    // stderr still has a terminal, but stdout is redirected. Its output must
    // match ordinary piped output, rather than inheriting stderr's narrow size.
    let destination = temporary_path("redirected-human-table.txt");
    let redirected = Command::new("script")
        .args([
            "-q",
            "-e",
            "-c",
            "stty rows 24 cols 42; \"$PROBE_TABLE_BINARY\" request list \"$PROBE_TABLE_COLLECTION\" > \"$PROBE_TABLE_OUTPUT\"",
            "/dev/null",
        ])
        .env("PROBE_TABLE_BINARY", env!("CARGO_BIN_EXE_probe"))
        .env("PROBE_TABLE_COLLECTION", &collection)
        .env("PROBE_TABLE_OUTPUT", &destination)
        .output()
        .unwrap();
    assert!(redirected.status.success(), "{redirected:?}");
    let piped = probe()
        .args(["request", "list"])
        .arg(&collection)
        .output()
        .unwrap();
    assert!(piped.status.success());
    assert_eq!(fs::read(&destination).unwrap(), piped.stdout);
    fs::remove_file(destination).unwrap();
}

#[test]
fn all_human_tables_bound_long_values_and_json_keeps_them() {
    let workspace = temporary_path("human-tables.yml");
    let long_name = "測試 API ".repeat(30);
    let long_url = format!("https://example.com/{}", "path/".repeat(60));
    let long_variable = "variable".repeat(10);
    let long_header = "X-Header-".repeat(30);
    let long_environment = "environment".repeat(10);
    let parent = "parent".repeat(40);
    fs::write(&workspace, format!(
        "opencollection: 1.0.0\ninfo:\n  name: Tables\nbundled: true\nitems:\n  - info:\n      name: {long_name:?}\n      type: folder\n    items:\n      - info:\n          name: {long_name:?}\n          type: http\n        http:\n          method: GET\n          url: {long_url:?}\n          headers:\n            - name: {long_header:?}\n              value: '{{{{{long_variable}}}}}'\nconfig:\n  environments:\n    - name: {parent:?}\n    - name: {long_environment:?}\n      extends: {parent:?}\n"
    )).unwrap();
    let path = workspace.to_str().unwrap();
    for arguments in [
        vec!["request", "list", path],
        vec!["folder", "list", path],
        vec!["environment", "list", path],
        vec!["request", "variables", path, "items/0/items/0"],
    ] {
        let output = probe().args(&arguments).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let human = String::from_utf8(output.stdout).unwrap();
        assert!(!human.contains('\t'), "{human}");
        assert!(human.contains('…'), "{human}");
        assert!(human.lines().all(|line| line.width() <= 120), "{human}");
        assert_eq!(
            human.lines().count(),
            if arguments[0] == "environment" { 3 } else { 2 }
        );
        let repeated = probe()
            .args(&arguments)
            .env("COLUMNS", "20")
            .output()
            .unwrap();
        assert_eq!(human.as_bytes(), repeated.stdout);
    }
    let requests = run_json(&["request", "list", path]);
    assert_eq!(requests["requests"][0]["name"], long_name);
    assert_eq!(requests["requests"][0]["url"], long_url);
    let folders = run_json(&["folder", "list", path]);
    assert_eq!(folders["folders"][0]["name"], long_name);
    let environments = run_json(&["environment", "list", path]);
    assert_eq!(environments["environments"][1]["name"], long_environment);
    assert_eq!(environments["environments"][1]["extends"], parent);
    let variables = run_json(&["request", "variables", path, "items/0/items/0"]);
    assert_eq!(variables["variables"][0]["name"], long_variable);
    assert_eq!(variables["variables"][0]["usages"][0]["name"], long_header);
    fs::remove_file(workspace).unwrap();
}
