#[allow(dead_code, unused_imports)]
mod common;

use common::*;
use unicode_width::UnicodeWidthStr;

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
