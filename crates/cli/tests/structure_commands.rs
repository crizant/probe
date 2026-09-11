#[allow(dead_code, unused_imports)]
mod common;

use common::*;

#[test]
fn structural_cli_commands_share_repository_operations() {
    let workspace = temporary_path("phase16-cli.yml");
    fs::copy(fixture("phase16-bundled.yml"), &workspace).unwrap();
    let path = workspace.to_str().unwrap();

    let folder = run_json(&["folder", "create", path, "--name", "Destination"]);
    assert_eq!(folder["operation"], "create");
    assert_eq!(folder["itemType"], "folder");
    assert_eq!(folder["selector"], "items/2");
    let folders = run_json(&["folder", "list", path]);
    assert_eq!(folders["folders"][0]["selector"], "items/1");
    assert_eq!(folders["folders"][1]["selector"], "items/2");
    assert!(folders["folders"][1]["parent"].is_null());

    let request = run_json(&[
        "request",
        "create",
        path,
        "--parent",
        "items/2",
        "--index",
        "0",
        "--name",
        "CLI Request",
        "--method",
        "PATCH",
        "--url",
        "https://example.com/cli",
    ]);
    assert_eq!(request["selector"], "items/2/items/0");
    let renamed = run_json(&[
        "request",
        "rename",
        path,
        "items/2/items/0",
        "--name",
        "Renamed CLI Request",
    ]);
    assert_eq!(renamed["previousSelector"], "items/2/items/0");

    let moved = run_json(&["request", "move", path, "items/2/items/0", "--index", "0"]);
    assert_eq!(moved["selector"], "items/0");
    let reordered = run_json(&["request", "reorder", path, "items/0", "--index", "2"]);
    assert_eq!(reordered["operation"], "reorder");
    assert_eq!(reordered["selector"], "items/2");
    run_json(&["request", "delete", path, "items/2"]);

    let reordered_folder = run_json(&["folder", "reorder", path, "items/2", "--index", "0"]);
    assert_eq!(reordered_folder["selector"], "items/0");
    let moved_folder = run_json(&[
        "folder", "move", path, "items/0", "--parent", "items/2", "--index", "0",
    ]);
    assert_eq!(moved_folder["selector"], "items/1/items/0");
    let renamed_folder = run_json(&[
        "folder",
        "rename",
        path,
        "items/1/items/0",
        "--name",
        "Renamed Destination",
    ]);
    assert_eq!(renamed_folder["selector"], "items/1/items/0");
    run_json(&["folder", "delete", path, "items/1/items/0"]);

    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("vendor.example"));
    assert!(!saved.contains("Renamed CLI Request"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn creates_native_graphql_requests() {
    let workspace = temporary_path("graphql-create.yml");
    fs::copy(fixture("phase16-bundled.yml"), &workspace).unwrap();
    let path = workspace.to_str().unwrap();
    let created = run_json(&[
        "request",
        "create",
        path,
        "--name",
        "Viewer",
        "--type",
        "graphql",
        "--method",
        "POST",
        "--url",
        "https://example.com/graphql",
        "--graphql-query",
        "query Viewer { viewer { login } }",
        "--graphql-operation-name",
        "Viewer",
    ]);
    assert_eq!(created["operation"], "create");
    let selector = created["selector"].as_str().unwrap();
    let inspect = run_json(&["request", "get", path, selector]);
    assert_eq!(inspect["type"], "graphql");
    assert_eq!(inspect["graphql"]["operationName"], "Viewer");
    assert!(
        inspect["graphql"]["query"]
            .as_str()
            .unwrap()
            .contains("query Viewer")
    );
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("type: graphql"));
    assert!(saved.contains("graphql:"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn create_rejects_graphql_fields_on_http_type() {
    let workspace = temporary_path("graphql-create-http.yml");
    fs::copy(fixture("phase16-bundled.yml"), &workspace).unwrap();
    let output = probe()
        .args(["request", "create"])
        .arg(&workspace)
        .args([
            "--name",
            "Viewer",
            "--type",
            "http",
            "--graphql-query",
            "query Viewer { viewer { login } }",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
    assert_eq!(
        value["error"]["message"],
        "GraphQL fields cannot be applied to an HTTP request"
    );
    fs::remove_file(workspace).unwrap();
}

#[test]
fn structural_cli_errors_have_stable_categories() {
    let workspace = temporary_path("phase16-cli-errors");
    copy_directory(&fixture("phase16-unbundled"), &workspace);
    let output = probe()
        .args(["request", "create"])
        .arg(&workspace)
        .args(["--name", "Alpha", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "duplicate_destination");

    let missing = probe()
        .args(["folder", "move"])
        .arg(&workspace)
        .args(["missing", "--json"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(4));
    let value: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(value["error"]["category"], "folder_not_found");

    let missing_parent = probe()
        .args(["request", "create"])
        .arg(&workspace)
        .args(["--name", "Child", "--parent", "missing", "--json"])
        .output()
        .unwrap();
    assert_eq!(missing_parent.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&missing_parent.stdout).unwrap();
    assert_eq!(value["error"]["category"], "destination_not_found");

    let invalid_destination = probe()
        .args(["folder", "move"])
        .arg(&workspace)
        .args(["group", "--parent", "group", "--json"])
        .output()
        .unwrap();
    assert_eq!(invalid_destination.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&invalid_destination.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_destination");
    fs::remove_dir_all(workspace).unwrap();
}
