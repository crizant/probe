#[allow(dead_code, unused_imports)]
mod common;

use common::*;

const TEST_SECRET: &str = "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR";

fn secret_runtime_fixture(server_url: &str) -> PathBuf {
    let path = runtime_variables_fixture(server_url);
    let source = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    assert!(source.contains("- name: token\n          value: persisted-token"));
    let source = source.replace(
        "- name: token\n          value: persisted-token",
        "- name: token\n          secret: true",
    );
    fs::write(&path, source).unwrap();
    path
}

#[test]
fn runtime_environment_secret_reaches_http_but_is_redacted_everywhere_presented() {
    let (server_url, server) = serve_once(TEST_SECRET.as_bytes().to_vec(), "text/plain");
    let workspace = secret_runtime_fixture(&server_url);
    let source = fs::read(&workspace).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--secret-provider",
            "env",
            "--json",
        ])
        .env("token", TEST_SECRET)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let captured = server.join().unwrap();
    assert!(
        captured
            .head
            .contains(&format!("authorization: Bearer {TEST_SECRET}"))
    );
    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(!rendered.contains(TEST_SECRET));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["response"]["body"]["content"], "[REDACTED]");
    assert_eq!(fs::read(&workspace).unwrap(), source);
    fs::remove_file(workspace).unwrap();
}

#[test]
fn secret_dry_run_override_and_missing_value_fail_closed() {
    let workspace = secret_runtime_fixture("http://127.0.0.1:1");
    let dry = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--secret-provider",
            "env",
            "--dry-run",
            "--json",
        ])
        .env("token", TEST_SECRET)
        .output()
        .unwrap();
    assert!(dry.status.success());
    assert!(!String::from_utf8_lossy(&dry.stdout).contains(TEST_SECRET));

    let override_output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--var",
            &format!("token={TEST_SECRET}"),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(override_output.status.success());
    assert!(!String::from_utf8_lossy(&override_output.stdout).contains(TEST_SECRET));

    let missing = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--secret-provider",
            "env",
            "--dry-run",
            "--json",
        ])
        .env_remove("token")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(5));
    let error: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(error["error"]["category"], "secret_variable_unavailable");
    assert!(!String::from_utf8_lossy(&missing.stdout).contains(TEST_SECRET));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn graphql_runtime_secret_is_sent_and_presentation_retains_reference() {
    let (server_url, server) = serve_once(b"{}".to_vec(), "application/json");
    let workspace = graphql_runtime_fixture(&server_url);
    let source = fs::read_to_string(&workspace)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(source.contains("- name: login\n      value: octocat"));
    let source = source
        .replace(
            "- name: login\n      value: octocat",
            "- name: login\n      secret: true",
        )
        .replace(
            "query Viewer($login: String!)",
            "query Viewer($login: String!) # {{login}}",
        )
        .replace("operationName: Viewer", "operationName: '{{login}}'")
        .replace("enabled\\\":true", "enabled\\\":\\\"{{login}}\\\"");
    fs::write(&workspace, source).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--secret-provider",
            "env",
            "--json",
        ])
        .env("login", TEST_SECRET)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let captured = server.join().unwrap();
    let body: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(body["variables"]["login"], TEST_SECRET);
    assert_eq!(body["operationName"], TEST_SECRET);
    assert!(body["query"].as_str().unwrap().contains(TEST_SECRET));
    let presented: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        presented["request"]["graphql"]["variables"]["login"],
        "{{login}}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TEST_SECRET));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn failed_http_execution_does_not_print_secret_url_or_diagnostic() {
    let workspace = secret_runtime_fixture("http://127.0.0.1:1");
    let source = fs::read_to_string(&workspace)
        .unwrap()
        .replace("/users/{{userId}}", "/users/{{token}}");
    fs::write(&workspace, source).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--secret-provider",
            "env",
            "--json",
        ])
        .env("token", TEST_SECRET)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TEST_SECRET));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TEST_SECRET));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn dry_run_without_resolution_options_preserves_literal_templates() {
    let workspace = runtime_variables_fixture("http://example.invalid");
    let source = fs::read_to_string(&workspace).unwrap().replace(
        "{{serverUrl}}/users/{{userId}}",
        "http://example.invalid/{{unfinished",
    );
    fs::write(&workspace, source).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("{{unfinished"));
    let provider_only = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--secret-provider", "env", "--dry-run"])
        .output()
        .unwrap();
    assert!(provider_only.status.success());
    assert!(String::from_utf8_lossy(&provider_only.stdout).contains("{{unfinished"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn secret_provider_accepts_only_one_supported_backend() {
    let workspace = runtime_variables_fixture("http://example.invalid");
    for arguments in [
        vec!["--secret-provider", "file"],
        vec!["--secret-provider", "env", "--secret-provider", "env"],
    ] {
        let output = probe()
            .args(["request", "run"])
            .arg(&workspace)
            .arg("items/0")
            .args(arguments)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["category"], "invalid_arguments");
    }
    fs::remove_file(workspace).unwrap();
}

#[test]
fn run_value_options_require_values_before_another_option() {
    let workspace = runtime_variables_fixture("http://example.invalid");
    for (arguments, message) in [
        (
            vec!["--environment", "--secret-provider", "env"],
            "--environment requires a non-empty value",
        ),
        (
            vec!["--secret-provider", "--dry-run"],
            "--secret-provider requires a non-empty value",
        ),
        (
            vec!["--secret-provider"],
            "--secret-provider requires a non-empty value",
        ),
        (
            vec!["--environment", "--show-headers"],
            "--environment requires a non-empty value",
        ),
        (
            vec!["--secret-provider", "--show-headers"],
            "--secret-provider requires a non-empty value",
        ),
        (
            vec!["--output", "--show-headers"],
            "--output requires a non-empty value",
        ),
    ] {
        let output = probe()
            .args(["request", "run"])
            .arg(&workspace)
            .arg("items/0")
            .args(arguments)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["category"], "invalid_arguments");
        assert_eq!(value["error"]["message"], message);
    }
    fs::remove_file(workspace).unwrap();
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
fn request_list_includes_type_field() {
    let output = probe()
        .args(["request", "list"])
        .arg(fixture("graphql-http.yml"))
        .arg("--json")
        .output()
        .expect("list command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["requests"][0]["type"], "graphql");
    assert_eq!(value["requests"][0]["selector"], "items/0");
}

#[test]
fn reads_a_bundled_workspace_from_stdin() {
    let source = fs::read(fixture("phase1-bundled.yml")).unwrap();
    let output = run_with_stdin(&["request", "list", "-", "--json"], &source);

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
fn gets_path_parameters_in_json_output() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("phase1-bundled.yml"))
        .arg("items/0/items/0")
        .arg("--json")
        .output()
        .expect("get command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["pathParameters"][0]["name"], "ownerId");
    assert_eq!(value["pathParameters"][0]["value"], "42");
}

#[test]
fn gets_first_class_graphql_fields_as_json() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("graphql-http.yml"))
        .args(["items/0", "--environment", "local", "--json"])
        .output()
        .expect("get command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "graphql");
    assert_eq!(value["graphql"]["operationName"], "Viewer");
    assert_eq!(value["graphql"]["variables"]["login"], "octocat");
    assert!(value["body"].is_null());
    assert_eq!(value["graphql"]["extensions"]["trace"]["enabled"], true);
}

#[test]
fn sets_and_persists_request_fields_as_json() {
    let workspace = temporary_path("phase7-workspace.yml");
    fs::copy(fixture("phase1-round-trip.yml"), &workspace).unwrap();
    let output = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--name",
            "Replace pet",
            "--method",
            "PUT",
            "--url",
            "https://api.example.com/pets/42",
            "--json",
        ])
        .output()
        .expect("set command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["selector"], "items/0");
    assert_eq!(value["name"], "Replace pet");
    assert_eq!(value["method"], "PUT");
    assert_eq!(value["url"], "https://api.example.com/pets/42");

    let inspect = probe()
        .args(["request", "get"])
        .arg(&workspace)
        .args(["items/0", "--json"])
        .output()
        .expect("saved request should be inspectable");
    assert!(inspect.status.success());
    let reloaded: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(reloaded["name"], "Replace pet");
    assert_eq!(reloaded["method"], "PUT");
    assert_eq!(reloaded["url"], "https://api.example.com/pets/42");
    assert_eq!(reloaded["authentication"]["type"], "bearer");
    assert_eq!(
        reloaded["authentication"]["properties"]["token"],
        "not-used-by-phase-1"
    );
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("vendor.example"));
    assert!(saved.contains("runtime:"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn sets_and_persists_graphql_fields_as_json() {
    let workspace = temporary_path("graphql-set-workspace.yml");
    fs::copy(fixture("graphql-http.yml"), &workspace).unwrap();
    let output = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--graphql-variables", r#"{"includeName":true}"#, "--json"])
        .output()
        .expect("GraphQL set command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["graphql"]["query"]
            .as_str()
            .unwrap()
            .starts_with("query Viewer")
    );
    assert_eq!(value["graphql"]["variables"]["includeName"], true);
    assert_eq!(value["graphql"]["operationName"], "Viewer");

    let operation = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--graphql-operation-name",
            r#""RenamedViewer""#,
            "--graphql-extensions",
            r#"{"persistedQuery":{"version":1}}"#,
            "--json",
        ])
        .output()
        .expect("partial GraphQL set command should run");
    assert!(operation.status.success());
    let value: Value = serde_json::from_slice(&operation.stdout).unwrap();
    assert_eq!(value["graphql"]["operationName"], "RenamedViewer");
    assert_eq!(value["graphql"]["variables"]["includeName"], true);
    assert_eq!(
        value["graphql"]["extensions"]["persistedQuery"]["version"],
        1
    );

    let cleared = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--graphql-variables",
            "null",
            "--graphql-extensions",
            "null",
            "--json",
        ])
        .output()
        .expect("optional GraphQL fields should clear");
    assert!(cleared.status.success());
    let value: Value = serde_json::from_slice(&cleared.stdout).unwrap();
    assert!(value["graphql"]["variables"].is_null());
    assert_eq!(value["graphql"]["operationName"], "RenamedViewer");
    assert!(value["graphql"]["extensions"].is_null());
    assert!(value["graphql"]["query"].is_string());

    let cleared_name = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--graphql-operation-name", "null", "--json"])
        .output()
        .expect("GraphQL operation name should clear");
    assert!(cleared_name.status.success());
    let value: Value = serde_json::from_slice(&cleared_name.stdout).unwrap();
    assert!(value["graphql"]["operationName"].is_null());

    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("type: graphql"));
    assert!(saved.contains("graphql:"));
    assert!(!saved.contains("type: http"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn graphql_operation_name_handles_literal_null_string_vs_json_null() {
    let workspace = temporary_path("graphql-null-test.yml");
    fs::copy(fixture("graphql-http.yml"), &workspace).unwrap();

    let set_literal_null = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--graphql-operation-name", r#""null""#, "--json"])
        .output()
        .expect("should set literal 'null' string as operation name");
    assert!(set_literal_null.status.success());
    let value: Value = serde_json::from_slice(&set_literal_null.stdout).unwrap();
    assert_eq!(value["graphql"]["operationName"], "null");

    let clear_with_json_null = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--graphql-operation-name", "null", "--json"])
        .output()
        .expect("should clear operation name with JSON null");
    assert!(clear_with_json_null.status.success());
    let value: Value = serde_json::from_slice(&clear_with_json_null.stdout).unwrap();
    assert!(value["graphql"]["operationName"].is_null());

    fs::remove_file(workspace).unwrap();
}

#[test]
fn graphql_operation_name_rejects_non_string() {
    let workspace = temporary_path("graphql-bad-op-name.yml");
    fs::copy(fixture("graphql-http.yml"), &workspace).unwrap();

    let output = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .args(["items/0", "--graphql-operation-name", "123", "--json"])
        .output()
        .expect("should reject non-string operation name");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("operation name")
    );

    fs::remove_file(workspace).unwrap();
}

#[test]
fn graphql_set_rejects_non_object_variables() {
    let output = probe()
        .args(["request", "set"])
        .arg(fixture("phase1-round-trip.yml"))
        .args([
            "items/0",
            "--graphql-query",
            "query Viewer { viewer { login } }",
            "--graphql-variables",
            "[]",
            "--json",
        ])
        .output()
        .expect("GraphQL set command should run");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
    assert_eq!(
        value["error"]["message"],
        "GraphQL variables must be a JSON object or null"
    );
}

#[test]
fn graphql_set_rejects_http_requests() {
    let output = probe()
        .args(["request", "set"])
        .arg(fixture("phase1-round-trip.yml"))
        .args([
            "items/0",
            "--graphql-query",
            "query Viewer { viewer { login } }",
            "--json",
        ])
        .output()
        .expect("GraphQL set command should run");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
    assert_eq!(
        value["error"]["message"],
        "request is not a native GraphQL request"
    );
}

#[test]
fn graphql_create_with_query_implies_graphql_type() {
    let workspace = temporary_path("graphql-create-implied.yml");
    fs::copy(fixture("phase1-round-trip.yml"), &workspace).unwrap();

    let output = probe()
        .args(["request", "create"])
        .arg(&workspace)
        .args([
            "--name",
            "Viewer",
            "--graphql-query",
            "query Viewer { viewer { login } }",
            "--json",
        ])
        .output()
        .expect("create with graphql-query should imply graphql type");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["operation"], "create");

    let get_output = probe()
        .args(["request", "get"])
        .arg(&workspace)
        .args([value["selector"].as_str().unwrap(), "--json"])
        .output()
        .expect("should read created request");
    assert!(get_output.status.success());
    let request: Value = serde_json::from_slice(&get_output.stdout).unwrap();
    assert_eq!(request["type"], "graphql");
    assert_eq!(
        request["graphql"]["query"],
        "query Viewer { viewer { login } }"
    );

    fs::remove_file(workspace).unwrap();
}

#[test]
fn graphql_create_rejects_conflicting_http_type() {
    let workspace = temporary_path("graphql-create-conflict.yml");
    fs::copy(fixture("phase1-round-trip.yml"), &workspace).unwrap();

    let output = probe()
        .args(["request", "create"])
        .arg(&workspace)
        .args([
            "--name",
            "BadRequest",
            "--type",
            "http",
            "--graphql-query",
            "query Test { test }",
            "--json",
        ])
        .output()
        .expect("create should reject conflicting type");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");

    fs::remove_file(workspace).unwrap();
}

#[test]
fn graphql_get_rejects_ambiguous_body_variants() {
    let workspace = temporary_path("graphql-variants.yml");
    fs::write(
        &workspace,
        concat!(
            "opencollection: 1.0.0\ninfo: { name: Variants }\nbundled: true\nitems:\n",
            "  - info: { name: Viewer, type: graphql }\n",
            "    graphql:\n      method: POST\n      url: https://example.com/graphql\n",
            "      body:\n        - title: One\n          selected: true\n",
            "          body: { query: 'query One { one }' }\n",
            "        - title: Two\n          selected: true\n",
            "          body: { query: 'query Two { two }' }\n",
        ),
    )
    .unwrap();
    let output = probe()
        .args(["request", "get"])
        .arg(&workspace)
        .args(["items/0", "--json"])
        .output()
        .expect("GraphQL get command should run");

    assert_eq!(output.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "request_configuration");
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("multiple selected values")
    );
    fs::remove_file(workspace).unwrap();
}

#[test]
fn reports_graphql_variable_locations() {
    let output = probe()
        .args(["request", "variables"])
        .arg(fixture("graphql-http.yml"))
        .args(["items/0", "--environment", "local", "--json"])
        .output()
        .expect("GraphQL variables command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let variables = value["variables"].as_array().unwrap();
    let login = variables
        .iter()
        .find(|variable| variable["name"] == "login")
        .unwrap();
    assert_eq!(
        login["usages"],
        serde_json::json!([{ "location": "graphql_variables" }])
    );
}

#[test]
fn gets_graphql_fields_in_human_output() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("graphql-http.yml"))
        .args(["items/0", "--environment", "local"])
        .output()
        .expect("GraphQL get command should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Body: graphql"));
    assert!(stdout.contains("GraphQL query: query Viewer"));
    assert!(stdout.contains("GraphQL operation name: Viewer"));
    assert!(stdout.contains("octocat"));
}

#[test]
fn set_requires_at_least_one_explicit_field() {
    let output = probe()
        .args(["request", "set"])
        .arg(fixture("phase1-round-trip.yml"))
        .args(["items/0", "--json"])
        .output()
        .expect("set command should run");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
}

#[test]
fn mutating_commands_reject_stdin_as_read_only() {
    let request_set = [
        "request",
        "set",
        "-",
        "items/0",
        "--url",
        "https://example.com/updated",
        "--json",
    ];
    let request_create = ["request", "create", "-", "--name", "Read only", "--json"];
    let environment_set = [
        "environment",
        "set",
        "-",
        "--environment",
        "development",
        "--name",
        "token",
        "--value",
        "rotated",
        "--json",
    ];
    let environment_create = ["environment", "create", "-", "--name", "staging", "--json"];
    let environment_delete = [
        "environment",
        "delete",
        "-",
        "--environment",
        "development",
        "--json",
    ];
    let environment_rename = [
        "environment",
        "rename",
        "-",
        "--environment",
        "development",
        "--name",
        "staging",
        "--json",
    ];
    let cases: &[(&str, &[&str])] = &[
        ("phase1-round-trip.yml", request_set.as_slice()),
        ("phase16-bundled.yml", request_create.as_slice()),
        ("phase4-environments.yml", environment_set.as_slice()),
        ("phase4-environments.yml", environment_create.as_slice()),
        ("phase4-environments.yml", environment_delete.as_slice()),
        ("phase4-environments.yml", environment_rename.as_slice()),
    ];

    for (fixture_name, arguments) in cases {
        let source = fs::read(fixture(fixture_name)).unwrap();
        let output = run_with_stdin(arguments, &source);
        assert_eq!(
            output.status.code(),
            Some(7),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(output.stderr.is_empty(), "{arguments:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["exitCode"], 7);
        assert_eq!(value["error"]["category"], "persistence_read_only");
    }
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
fn discovers_request_variables_as_versioned_deterministic_json() {
    let command = || {
        probe()
            .args(["request", "variables"])
            .arg(fixture("phase4-environments.yml"))
            .arg("items/0")
            .args(["--environment", "development", "--json"])
            .output()
            .expect("variables command should run")
    };
    let first = command();
    let second = command();

    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let value: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(
        value["variables"],
        serde_json::json!([
            {
                "name": "baseUrl",
                "defined": true,
                "secret": false,
                "usages": [{ "location": "url" }]
            },
            {
                "name": "tenant",
                "defined": true,
                "secret": false,
                "usages": [
                    { "location": "url" },
                    { "location": "query_parameter", "name": "tenant" },
                    { "location": "body" }
                ]
            },
            {
                "name": "token",
                "defined": true,
                "secret": false,
                "usages": [
                    { "location": "header", "name": "Authorization" },
                    { "location": "authentication", "name": "token" }
                ]
            }
        ])
    );
}

#[test]
fn discovers_missing_and_secret_variables_without_resolving_values() {
    for (selector, name, defined, secret) in [
        ("items/1", "missing", false, false),
        ("items/2", "secretToken", true, true),
    ] {
        let output = probe()
            .args(["request", "variables"])
            .arg(fixture("phase4-environments.yml"))
            .arg(selector)
            .args(["--environment", "development", "--json"])
            .output()
            .expect("variables command should not resolve values");
        assert!(output.status.success(), "{selector}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let variable = value["variables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variable| variable["name"] == name)
            .unwrap();
        assert_eq!(variable["defined"], defined);
        assert_eq!(variable["secret"], secret);
    }

    let missing_environment = probe()
        .args(["request", "variables"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/0")
        .args(["--environment", "missing", "--json"])
        .output()
        .expect("variables command should validate its selected environment");
    assert_eq!(missing_environment.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&missing_environment.stdout).unwrap();
    assert_eq!(value["error"]["category"], "environment_not_found");
}

#[test]
fn renders_request_variables_for_humans_and_rejects_runtime_values() {
    let output = probe()
        .args(["request", "variables"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/2")
        .args(["--environment", "development"])
        .output()
        .expect("variables command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "NAME\tDEFINED\tSECRET\tUSED IN\nbaseUrl\ttrue\tfalse\turl\nsecretToken\ttrue\ttrue\theader: Authorization\n"
    );

    let rejected = probe()
        .args(["request", "variables"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/2")
        .args(["--var", "secretToken=must-not-appear", "--json"])
        .output()
        .expect("variables command should reject runtime values");
    assert_eq!(rejected.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&rejected.stdout).contains("must-not-appear"));
}

#[test]
fn reports_environment_resolution_errors() {
    for (selector, environment, category) in [
        ("items/0", "production", "environment_not_found"),
        ("items/2", "development", "secret_variable_unavailable"),
    ] {
        let output = probe()
            .args(["request", "get"])
            .arg(fixture("phase4-environments.yml"))
            .arg(selector)
            .args(["--environment", environment, "--json"])
            .output()
            .expect("get command should run");
        assert_eq!(output.status.code(), Some(5), "{category}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["category"], category);
    }
}

#[test]
fn get_preserves_undefined_request_variables() {
    let output = probe()
        .args(["request", "get"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/1")
        .args(["--environment", "development", "--json"])
        .output()
        .expect("get command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["url"], "https://dev.example.com/{{missing}}");
}

#[test]
fn strict_variables_rejects_undefined_request_variables() {
    for action in ["get", "run"] {
        let output = probe()
            .args(["request", action])
            .arg(fixture("phase4-environments.yml"))
            .arg("items/1")
            .args([
                "--environment",
                "development",
                "--strict-variables",
                "--json",
            ])
            .output()
            .expect("strict request command should run");

        assert_eq!(output.status.code(), Some(5), "{action}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["category"], "missing_variable");
    }

    let output = probe()
        .args(["request", "get"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/1")
        .args(["--strict-variables", "--json"])
        .output()
        .expect("strict request command without an environment should run");
    assert_eq!(output.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "missing_variable");
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
    assert!(value["request"]["graphql"].is_null());
    assert_eq!(value["response"]["status"], 200);
    assert_eq!(value["response"]["sizeBytes"], 15);
    assert_eq!(value["response"]["body"]["encoding"], "utf8");
    assert_eq!(value["response"]["body"]["content"], "{\"result\":\"ok\"}");
    assert!(value.get("expectations").is_none());
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
fn relative_file_body_is_read_from_the_workspace_directory() {
    let (server_url, server) = serve_once(Vec::new(), "text/plain");
    let output = probe()
        .args(["request", "run"])
        .arg(fixture("relative-file-body.yml"))
        .args([
            "items/0",
            "--var",
            &format!("serverUrl={server_url}"),
            "--json",
        ])
        .output()
        .expect("file body request should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let captured = server.join().unwrap();
    assert!(captured.head.starts_with("POST /upload HTTP/1.1\r\n"));
    assert!(
        captured
            .head
            .split("\r\n")
            .any(|line| line.eq_ignore_ascii_case("content-type: text/plain"))
    );
    assert_eq!(
        captured.body,
        fs::read(fixture("relative-upload.txt")).unwrap()
    );
}

#[test]
fn executes_graphql_json_envelopes_and_preserves_application_errors() {
    let response =
        r#"{"data":{"viewer":{"login":"octocat"}},"errors":[{"message":"viewer is unavailable"}]}"#;
    let (server_url, server) = serve_once(response.as_bytes().to_vec(), "application/json");
    let workspace = graphql_runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--json"])
        .output()
        .expect("GraphQL request should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["request"]["method"], "POST");
    assert_eq!(value["request"]["graphql"]["operationName"], "Viewer");
    assert_eq!(value["response"]["status"], 200);
    assert_eq!(value["response"]["body"]["content"], response);

    let captured = server.join().unwrap();
    assert!(captured.head.starts_with("POST /graphql HTTP/1.1\r\n"));
    assert!(captured.head.contains("content-type: application/json\r\n"));
    let envelope: Value =
        serde_json::from_slice(&captured.body).expect("request should be a JSON envelope");
    assert_eq!(
        envelope["query"],
        "query Viewer($login: String!) { viewer(login: $login) { login } }"
    );
    assert_eq!(envelope["variables"]["login"], "octocat");
    assert_eq!(envelope["operationName"], "Viewer");
    fs::remove_file(workspace).unwrap();
}

#[test]
fn executes_native_graphql_get_with_query_parameters() {
    let (server_url, server) =
        serve_once(br#"{"data":{"viewer":null}}"#.to_vec(), "application/json");
    let source = fs::read_to_string(fixture("graphql-http.yml")).unwrap();
    let workspace = temporary_path("graphql-get-workspace.yml");
    fs::write(
        &workspace,
        source
            .replace("__SERVER_URL__", &server_url)
            .replace("method: POST", "method: GET"),
    )
    .unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .args(["items/0", "--environment", "local", "--json"])
        .output()
        .expect("GraphQL GET request should run");

    assert!(output.status.success());
    let captured = server.join().unwrap();
    assert!(captured.head.starts_with("GET /graphql?"));
    assert!(captured.head.contains("query="));
    assert!(captured.head.contains("variables="));
    assert!(captured.head.contains("operationName=Viewer"));
    assert!(captured.head.contains("extensions="));
    assert!(captured.body.is_empty());
    fs::remove_file(workspace).unwrap();
}

#[test]
fn native_graphql_runtime_variables_override_the_environment() {
    let (server_url, server) = serve_once(br#"{"data":{}}"#.to_vec(), "application/json");
    let workspace = graphql_runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .args([
            "items/0",
            "--environment",
            "local",
            "--var",
            "login=hubot",
            "--json",
        ])
        .output()
        .expect("GraphQL runtime override should run");

    assert!(output.status.success());
    let captured = server.join().unwrap();
    let envelope: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(envelope["variables"]["login"], "hubot");
    fs::remove_file(workspace).unwrap();
}

#[test]
fn run_uses_runtime_variables_without_an_environment_and_does_not_persist_them() {
    let (server_url, server) = serve_once(Vec::new(), "text/plain");
    let workspace = runtime_variables_fixture(&server_url);
    let before = fs::read(&workspace).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--var", &format!("serverUrl={server_url}")])
        .args(["--var", "userId=123"])
        .args(["--var", "token=abc=def==", "--json"])
        .output()
        .expect("run command should accept runtime-only variables");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    let captured = server.join().unwrap();
    assert!(captured.head.starts_with("GET /users/123 HTTP/1.1\r\n"));
    assert!(
        captured
            .head
            .contains("authorization: Bearer abc=def==\r\n")
    );
    assert_eq!(fs::read(&workspace).unwrap(), before);
    fs::remove_file(workspace).unwrap();
}

#[test]
fn run_runtime_variables_override_a_selected_environment_and_last_value_wins() {
    let (server_url, server) = serve_once(Vec::new(), "text/plain");
    let workspace = runtime_variables_fixture(&server_url);
    let before = fs::read(&workspace).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--var",
            "userId=123",
            "--var",
            "userId=456",
            "--var",
            "token=override",
            "--json",
        ])
        .output()
        .expect("run command should apply runtime overrides");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let captured = server.join().unwrap();
    assert!(captured.head.starts_with("GET /users/456 HTTP/1.1\r\n"));
    assert!(captured.head.contains("authorization: Bearer override\r\n"));
    assert_eq!(fs::read(&workspace).unwrap(), before);
    fs::remove_file(workspace).unwrap();
}

#[test]
fn run_rejects_malformed_runtime_variables_without_exposing_the_value() {
    for value in ["missing-separator", "=secret-value"] {
        let output = probe()
            .args(["request", "run"])
            .arg(fixture("phase-runtime-variables.yml"))
            .args(["items/0", "--var", value, "--json"])
            .output()
            .expect("run command should reject malformed runtime variables");

        assert_eq!(output.status.code(), Some(2));
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["error"]["category"], "invalid_arguments");
        assert!(!json["error"]["message"].as_str().unwrap().contains(value));
    }
}

#[test]
fn effective_request_urls_do_not_look_like_redirects() {
    for (path, parameters, expected_path) in [
        (
            "/users/:id",
            "      params:\n        - name: id\n          value: '7'\n          type: path\n",
            "/users/7",
        ),
        (
            "/search",
            "      params:\n        - name: q\n          value: 'hello world'\n          type: query\n",
            "/search?q=hello+world",
        ),
        ("/before/../normalized", "", "/normalized"),
    ] {
        let (base_url, server) = serve_once(b"hello".to_vec(), "text/plain");
        let url = format!("{}{path}", base_url.replacen("http:", "HTTP:", 1));
        let workspace = run_url_fixture(&url, parameters);
        let output = probe()
            .args(["request", "run"])
            .arg(&workspace)
            .arg("items/0")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let human = String::from_utf8(output.stdout).unwrap();
        assert!(
            human.starts_with(&format!("GET {base_url}{expected_path}\n")),
            "{human}"
        );
        assert!(!human.contains("Final URL:"), "{human}");
        assert!(human.ends_with("\n\nhello\n"));
        let captured = server.join().unwrap();
        assert!(
            captured
                .head
                .starts_with(&format!("GET {expected_path} HTTP/1.1\r\n")),
            "{captured:?}"
        );
        fs::remove_file(workspace).unwrap();
    }
}

#[test]
fn secret_bearing_urls_keep_references_in_human_and_json_output() {
    // Spaces and punctuation are encoded by the engine: replacing raw secret
    // substrings in the built URL would not be a safe disclosure policy.
    let secret = "private value&tail";
    for json in [false, true] {
        let (base_url, server) = serve_once(b"hello".to_vec(), "text/plain");
        let url = format!("{base_url}/users/{{{{token}}}}?token={{{{token}}}}");
        let workspace = run_url_fixture(&url, "");
        let mut source = fs::read_to_string(&workspace).unwrap();
        source.push_str("config:\n  environments:\n    - name: local\n      variables:\n        - name: token\n          secret: true\n");
        fs::write(&workspace, source).unwrap();
        let mut command = probe();
        command
            .args(["request", "run"])
            .arg(&workspace)
            .args([
                "items/0",
                "--environment",
                "local",
                "--secret-provider",
                "env",
            ])
            .env("token", secret);
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let rendered = String::from_utf8(output.stdout).unwrap();
        assert!(!rendered.contains(secret), "{rendered}");
        assert!(!rendered.contains("private%20value"), "{rendered}");
        assert!(!rendered.contains("Final URL:"), "{rendered}");
        if json {
            let value: Value = serde_json::from_str(&rendered).unwrap();
            assert_eq!(value["request"]["url"], url);
            assert_eq!(value["response"]["url"], url);
            assert!(value["response"].get("initial_url").is_none());
        } else {
            assert!(rendered.starts_with(&format!("GET {url}\n")), "{rendered}");
        }
        let captured = server.join().unwrap();
        assert!(
            captured.head.starts_with(
                "GET /users/private%20value&tail?token=private%20value&tail HTTP/1.1\r\n"
            ),
            "{captured:?}"
        );
        fs::remove_file(workspace).unwrap();
    }
}

#[test]
fn real_redirects_show_the_final_url_and_keep_json_unchanged() {
    for json in [false, true] {
        let (target_url, target) = serve_once(b"hello".to_vec(), "text/plain");
        let final_url = format!("{target_url}/final");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let initial_url = format!("http://{}/start", listener.local_addr().unwrap());
        let location = final_url.clone();
        let redirect = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(&mut stream);
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
            }
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let workspace = run_url_fixture(&initial_url, "");
        let mut command = probe();
        command
            .args(["request", "run"])
            .arg(&workspace)
            .arg("items/0");
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let rendered = String::from_utf8(output.stdout).unwrap();
        if json {
            let value: Value = serde_json::from_str(&rendered).unwrap();
            assert_eq!(value["response"]["url"], final_url);
            assert!(value["response"].get("urlChanged").is_none());
            assert!(value["response"].get("url_changed").is_none());
            assert!(value["response"].get("initialUrl").is_none());
            assert!(value["response"].get("initial_url").is_none());
            assert_eq!(value["request"]["url"], initial_url);
        } else {
            assert!(
                rendered.starts_with(&format!("GET {initial_url}\n")),
                "{rendered}"
            );
            assert!(
                rendered.contains(&format!("Final URL: {final_url}\n")),
                "{rendered}"
            );
        }
        redirect.join().unwrap();
        assert!(
            target
                .join()
                .unwrap()
                .head
                .starts_with("GET /final HTTP/1.1\r\n")
        );
        fs::remove_file(workspace).unwrap();
    }
}

fn run_url_fixture(url: &str, parameters: &str) -> PathBuf {
    let workspace = temporary_path("effective-url.yml");
    fs::write(&workspace, format!("opencollection: 1.0.0\ninfo:\n  name: Effective URL\nbundled: true\nitems:\n  - info:\n      name: Request\n      type: http\n    http:\n      method: GET\n      url: '{url}'\n{parameters}")).unwrap();
    workspace
}

#[test]
fn human_response_headers_are_opt_in() {
    for flags in [&[][..], &["--show-headers"][..]] {
        let output = run_response_with_flags("hello", "text/plain", flags);
        assert!(output.starts_with("POST http://127.0.0.1:"), "{output}");
        assert!(output.contains("\n200 OK\n"), "{output}");
        assert!(output.contains(" ms\n5 B\n"), "{output}");
        assert!(!output.contains("Final URL:"), "{output}");
        assert!(output.ends_with("\n\nhello\n"), "{output}");
        assert_eq!(output.contains("Headers:\n"), !flags.is_empty());
        assert_eq!(
            output.contains("  content-type: text/plain\n"),
            !flags.is_empty()
        );
        assert_eq!(output.contains("  content-length: 5\n"), !flags.is_empty());
    }
}

#[test]
fn json_response_includes_headers_with_or_without_show_headers() {
    for flags in [&["--json"][..], &["--json", "--show-headers"][..]] {
        let output = run_response_with_flags("hello", "text/plain", flags);
        let value: Value = serde_json::from_str(&output).unwrap();
        let headers = value["response"]["headers"].as_array().unwrap();
        assert!(
            headers
                .iter()
                .any(|header| header["name"] == "content-type" && header["value"] == "text/plain")
        );
        assert!(
            headers
                .iter()
                .any(|header| header["name"] == "content-length" && header["value"] == "5")
        );
        assert_eq!(value["response"]["body"]["content"], "hello");
    }
}

#[test]
fn show_headers_uses_standard_flag_validation_and_help() {
    let output = probe()
        .args(["request", "run", "--show-headers", "--show-headers"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--show-headers may only be specified once")
    );
    for args in [&["--help"][..], &["request", "run", "--help"][..]] {
        let output = probe().args(args).output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("--show-headers"));
    }
}

#[test]
fn human_response_pretty_prints_json_without_colors() {
    for content_type in [
        "application/json",
        "Application/JSON; charset=utf-8",
        "application/problem+json",
    ] {
        let output = run_response_output(r#"{"items":[1,true,null]}"#, content_type, false);
        assert!(output.ends_with("\n\n{\n  \"items\": [\n    1,\n    true,\n    null\n  ]\n}\n"));
        assert!(!output.contains('\u{1b}'));
    }
}

#[test]
fn human_response_preserves_json_tokens() {
    for (body, expected) in [
        (r#"{"z":1,"a":2}"#, "{\n  \"z\": 1,\n  \"a\": 2\n}"),
        (r#"{"a":1,"a":2}"#, "{\n  \"a\": 1,\n  \"a\": 2\n}"),
        (
            "[1234567890123456789012345678901234567890,1.2300,1E+999,-0]",
            "[\n  1234567890123456789012345678901234567890,\n  1.2300,\n  1E+999,\n  -0\n]",
        ),
    ] {
        let output = run_response_output(body, "application/json", false);
        assert!(output.ends_with(&format!("\n\n{expected}\n")), "{output}");
    }
}

#[test]
fn human_response_preserves_invalid_json() {
    let body = "  {\"broken\":\n";
    let output = run_response_output(body, "application/json", false);
    assert!(output.ends_with(&format!("\n\n{body}")));
}

#[test]
fn human_response_preserves_non_json() {
    for (body, content_type) in [
        ("plain text\n", "text/plain"),
        (r#"{"items":[1,true,null]}"#, "text/plain"),
        (r#"{"items":[1,true,null]}"#, "application/jsonp"),
    ] {
        let output = run_response_output(body, content_type, false);
        assert!(output.ends_with(&format!("\n\n{}\n", body.trim_end_matches('\n'))));
    }
}

#[test]
fn structured_response_preserves_original_json_body() {
    let body = r#"{"z":1,"a":2,"a":3,"items":[123456789012345678901234567890,1E+999,-0,"\u0061"]}"#;
    let output = run_response_output(body, "application/json", true);
    let value: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value["response"]["body"]["content"], body);
    assert_eq!(value["response"]["body"]["encoding"], "utf8");
    assert_eq!(value["response"]["body"]["omitted"], false);
}

fn run_response_output(body: &str, content_type: &'static str, json: bool) -> String {
    run_response_with_flags(body, content_type, if json { &["--json"] } else { &[] })
}

fn run_response_with_flags(body: &str, content_type: &'static str, flags: &[&str]) -> String {
    let (server_url, server) = serve_once(body.as_bytes().to_vec(), content_type);
    let workspace = runtime_fixture(&server_url);
    let mut command = probe();
    command
        .args(["request", "run"])
        .arg(&workspace)
        .args(["items/0", "--environment", "local"]);
    command.args(flags);
    let output = command.output().expect("request should run");
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
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
    let response_body = vec![b'x'; MAX_IN_MEMORY_RESPONSE_BYTES + 1];
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
fn run_sends_undefined_request_variables() {
    let (server_url, server) = serve_once(Vec::new(), "text/plain");
    let workspace = temporary_path("undefined-variable.yml");
    let source = fs::read_to_string(fixture("phase4-environments.yml")).unwrap();
    fs::write(&workspace, source.replace("https://{{host}}", &server_url)).unwrap();
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/1")
        .args(["--environment", "development", "--json"])
        .output()
        .expect("run command should run");

    assert!(output.status.success());
    assert!(
        server
            .join()
            .unwrap()
            .head
            .starts_with("GET /%7B%7Bmissing%7D%7D HTTP/1.1\r\n")
    );
    fs::remove_file(workspace).unwrap();
}

#[test]
fn dry_run_resolves_without_opening_a_network_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should be non-blocking");
    let server_url = format!("http://{}", listener.local_addr().unwrap());
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--dry-run", "--json"])
        .output()
        .expect("dry-run command should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["dryRun"], true);
    assert_eq!(value["request"]["method"], "POST");
    assert_eq!(value["request"]["url"], format!("{server_url}/echo"));
    assert!(value["request"]["graphql"].is_null());
    assert!(value.get("response").is_none());
    assert!(
        matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "dry-run must not connect to the resolved URL"
    );
    fs::remove_file(workspace).unwrap();
}

#[test]
fn dry_run_applies_environment_and_runtime_variables() {
    let output = probe()
        .args(["request", "run"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/0")
        .args([
            "--environment",
            "development",
            "--var",
            "tenant=us",
            "--dry-run",
        ])
        .output()
        .expect("dry-run command should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "GET https://dev.example.com/us/users\n"
    );

    let json = probe()
        .args(["request", "run"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/0")
        .args([
            "--environment",
            "development",
            "--var",
            "tenant=us",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("dry-run JSON command should run");
    let value: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["dryRun"], true);
    assert_eq!(value["request"]["method"], "GET");
    assert_eq!(value["request"]["url"], "https://dev.example.com/us/users");
    assert!(value.get("response").is_none());
}

#[test]
fn dry_run_fails_closed_when_a_secret_variable_is_unavailable() {
    let output = probe()
        .args(["request", "run"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/2")
        .args(["--environment", "development", "--dry-run", "--json"])
        .output()
        .expect("dry-run command should fail closed for secrets");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["error"]["category"], "secret_variable_unavailable");
    assert_eq!(value["error"]["exitCode"], 5);
    assert!(value.get("request").is_none());
    assert!(value.get("dryRun").is_none());
    let rendered = value.to_string();
    assert!(!rendered.to_lowercase().contains("bearer"));
}

#[test]
fn dry_run_rejects_an_output_file() {
    let output = probe()
        .args(["request", "run"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/0")
        .args(["--dry-run", "--output", "body.bin", "--json"])
        .output()
        .expect("dry-run should reject --output");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
}

#[test]
fn expect_passes_when_status_matches() {
    let (server_url, server) = serve_once(b"{\"ok\":true}".to_vec(), "application/json");
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--expect", "status=200", "--json"])
        .output()
        .expect("expect command should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["response"]["status"], 200);
    assert_eq!(value["expectations"][0]["expr"], "status=200");
    assert_eq!(value["expectations"][0]["ok"], true);
    assert_eq!(value["expectations"][0]["actual"], 200);
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
}

#[test]
fn expect_fails_when_status_does_not_match() {
    let (server_url, server) =
        serve_once_with_status(b"missing".to_vec(), "text/plain", 404, "Not Found");
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--expect", "status=200", "--json"])
        .output()
        .expect("expect command should fail");

    assert_eq!(output.status.code(), Some(9));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["error"]["category"], "expectation_failed");
    assert_eq!(value["error"]["exitCode"], 9);
    assert_eq!(
        value["error"]["details"]["expectations"][0]["expr"],
        "status=200"
    );
    assert_eq!(value["error"]["details"]["expectations"][0]["ok"], false);
    assert_eq!(value["error"]["details"]["expectations"][0]["actual"], 404);
    assert!(value.get("response").is_none());
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
}

#[test]
fn expect_accepts_alternate_status_codes() {
    let (server_url, server) =
        serve_once_with_status(b"created".to_vec(), "text/plain", 201, "Created");
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--expect",
            "status=200|201",
            "--json",
        ])
        .output()
        .expect("alternate expect command should run");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["expectations"][0]["expr"], "status=200|201");
    assert_eq!(value["expectations"][0]["ok"], true);
    assert_eq!(value["expectations"][0]["actual"], 201);
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
}

#[test]
fn expect_rejects_an_invalid_expression_without_sending() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should be non-blocking");
    let server_url = format!("http://{}", listener.local_addr().unwrap());
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--expect",
            "header:content-type~json",
            "--json",
        ])
        .output()
        .expect("invalid expect should be rejected");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
    assert_eq!(
        value["error"]["message"],
        "invalid --expect expression: header:content-type~json; expected status=<code> or status=<code|code>"
    );
    assert!(
        matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "invalid --expect must not send the request"
    );
    fs::remove_file(workspace).unwrap();
}

#[test]
fn expect_reports_every_outcome_when_one_expression_fails() {
    let (server_url, server) =
        serve_once_with_status(b"created".to_vec(), "text/plain", 201, "Created");
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--expect",
            "status=200",
            "--expect",
            "status=201",
            "--json",
        ])
        .output()
        .expect("expect command should fail");

    assert_eq!(output.status.code(), Some(9));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["error"]["category"], "expectation_failed");
    assert_eq!(
        value["error"]["message"],
        "expectation failed: status=200 (actual 201)"
    );
    assert!(value.get("response").is_none());
    let expectations = value["error"]["details"]["expectations"]
        .as_array()
        .expect("expectations should be an array");
    assert_eq!(expectations.len(), 2);
    assert_eq!(expectations[0]["expr"], "status=200");
    assert_eq!(expectations[0]["ok"], false);
    assert_eq!(expectations[0]["actual"], 201);
    assert_eq!(expectations[1]["expr"], "status=201");
    assert_eq!(expectations[1]["ok"], true);
    assert_eq!(expectations[1]["actual"], 201);
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();

    let (server_url, server) =
        serve_once_with_status(b"created".to_vec(), "text/plain", 201, "Created");
    let workspace = runtime_fixture(&server_url);
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args([
            "--environment",
            "local",
            "--expect",
            "status=200",
            "--expect",
            "status=201",
        ])
        .output()
        .expect("expect command should fail");

    assert_eq!(output.status.code(), Some(9));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"error[expectation_failed]: expectation failed: status=200 (actual 201)\n"
    );
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
}

#[test]
fn expect_keeps_the_output_file_when_the_status_fails() {
    let response_body = b"missing".to_vec();
    let (server_url, server) =
        serve_once_with_status(response_body.clone(), "text/plain", 404, "Not Found");
    let workspace = runtime_fixture(&server_url);
    let body_output = temporary_path("response.bin");
    let output = probe()
        .args(["request", "run"])
        .arg(&workspace)
        .arg("items/0")
        .args(["--environment", "local", "--output"])
        .arg(&body_output)
        .args(["--expect", "status=200", "--json"])
        .output()
        .expect("expect command should fail after writing the body");

    assert_eq!(output.status.code(), Some(9));
    assert_eq!(fs::read(&body_output).unwrap(), response_body);
    server.join().unwrap();
    fs::remove_file(workspace).unwrap();
    fs::remove_file(body_output).unwrap();
}

#[test]
fn expect_rejects_a_missing_value_without_sending() {
    let cases: &[&[&str]] = &[
        &["--environment", "local", "--json", "--expect"],
        &["--environment", "local", "--expect", "--dry-run", "--json"],
    ];
    for arguments in cases {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        listener
            .set_nonblocking(true)
            .expect("listener should be non-blocking");
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        let workspace = runtime_fixture(&server_url);
        let output = probe()
            .args(["request", "run"])
            .arg(&workspace)
            .arg("items/0")
            .args(*arguments)
            .output()
            .expect("missing --expect value should be rejected");

        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stderr.is_empty(), "{arguments:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["category"], "invalid_arguments");
        assert_eq!(
            value["error"]["message"],
            "--expect requires status=<code> or status=<code|code>"
        );
        assert!(
            matches!(
                listener.accept(),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
            ),
            "missing --expect must not send the request ({arguments:?})"
        );
        fs::remove_file(workspace).unwrap();
    }
}

#[test]
fn expect_rejects_combination_with_dry_run() {
    let output = probe()
        .args(["request", "run"])
        .arg(fixture("phase4-environments.yml"))
        .arg("items/0")
        .args(["--dry-run", "--expect", "status=200", "--json"])
        .output()
        .expect("expect plus dry-run should be rejected");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
    assert_eq!(value["error"]["exitCode"], 2);
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

fn request_json(workspace: &std::path::Path, selector: &str) -> Value {
    let output = probe()
        .args(["request", "get"])
        .arg(workspace)
        .args([selector, "--json"])
        .output()
        .expect("request get should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn set_fields(workspace: &std::path::Path, selector: &str, args: &[&str]) -> Value {
    let output = probe()
        .args(["request", "set"])
        .arg(workspace)
        .arg(selector)
        .args(args)
        .arg("--json")
        .output()
        .expect("request set should run");
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn sets_replaces_and_clears_headers_parameters_body_and_auth() {
    let workspace = temporary_path("request-fields.yml");
    fs::copy(fixture("phase1-round-trip.yml"), &workspace).unwrap();

    let updated = set_fields(
        &workspace,
        "items/0",
        &[
            "--headers",
            r#"[{"name":"X-Probe","value":"1","disabled":true}]"#,
            "--body",
            r#"{"type":"text","data":"hello"}"#,
            "--auth",
            r#"{"type":"basic","username":"demo","password":"secret"}"#,
        ],
    );
    assert_eq!(updated["headers"][0]["name"], "X-Probe");
    assert_eq!(updated["headers"][0]["disabled"], true);
    assert_eq!(updated["body"]["value"]["type"], "text");
    assert_eq!(updated["body"]["value"]["data"], "hello");
    assert_eq!(updated["authentication"]["type"], "basic");
    assert_eq!(updated["authentication"]["properties"]["username"], "demo");

    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("description: Payload media type"));
    assert!(saved.contains("vendor.example"));
    assert!(saved.contains("runtime:"));
    assert!(!saved.contains("not-used-by-phase-1"));

    let cleared = set_fields(
        &workspace,
        "items/0",
        &["--headers", "null", "--body", "null", "--auth", "null"],
    );
    assert_eq!(cleared["headers"].as_array().unwrap().len(), 0);
    assert!(cleared["body"].is_null());
    assert!(cleared["authentication"].is_null());
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(!saved.contains("auth:"));
    assert!(saved.contains("vendor.example"));
    assert_eq!(request_json(&workspace, "items/0")["body"], Value::Null);

    let nested = set_fields(
        &workspace,
        "items/0",
        &[
            "--auth",
            r#"{"type":"apikey","key":"X-API-Key","value":"{{apiToken}}","placement":"header"}"#,
        ],
    );
    assert_eq!(nested["authentication"]["type"], "apikey");
    assert_eq!(nested["authentication"]["properties"]["key"], "X-API-Key");
    assert_eq!(
        nested["authentication"]["properties"]["value"],
        "{{apiToken}}"
    );
    assert_eq!(
        nested["authentication"]["properties"]["placement"],
        "header"
    );
    let reloaded = request_json(&workspace, "items/0");
    assert_eq!(
        reloaded["authentication"]["properties"]["placement"],
        "header"
    );

    set_fields(&workspace, "items/0", &["--auth", r#""inherit""#]);
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("auth: inherit"));
    assert_eq!(
        request_json(&workspace, "items/0")["authentication"]["type"],
        "inherit"
    );
    fs::remove_file(workspace).unwrap();

    let params = temporary_path("request-params.yml");
    fs::copy(fixture("phase1-bundled.yml"), &params).unwrap();
    let query = set_fields(
        &params,
        "items/0/items/0",
        &[
            "--query-parameters",
            r#"[{"name":"preview","value":"true","disabled":true}]"#,
        ],
    );
    assert_eq!(query["queryParameters"][0]["name"], "preview");
    assert_eq!(query["queryParameters"][0]["disabled"], true);
    assert_eq!(query["pathParameters"][0]["name"], "ownerId");
    assert_eq!(query["pathParameters"][0]["value"], "42");

    let path = set_fields(
        &params,
        "items/0/items/0",
        &["--path-parameters", r#"[{"name":"petId","value":"7"}]"#],
    );
    assert_eq!(path["pathParameters"][0]["name"], "petId");
    assert_eq!(path["pathParameters"][0]["value"], "7");
    assert_eq!(path["pathParameters"][0]["disabled"], false);
    assert_eq!(path["queryParameters"][0]["name"], "preview");

    let cleared = set_fields(
        &params,
        "items/0/items/0",
        &["--query-parameters", "[]", "--headers", "null"],
    );
    assert_eq!(cleared["queryParameters"].as_array().unwrap().len(), 0);
    assert_eq!(cleared["headers"].as_array().unwrap().len(), 0);
    assert_eq!(cleared["pathParameters"][0]["name"], "petId");
    let saved = fs::read_to_string(&params).unwrap();
    assert!(saved.contains("name: ownerId") || saved.contains("name: petId"));
    assert!(saved.contains("type: path"));
    assert!(!saved.contains("name: preview"));
    assert!(saved.contains("name: Probe Team") || saved.contains("summary:"));
    fs::remove_file(params).unwrap();
}

#[test]
fn writes_every_http_body_kind() {
    let workspace = temporary_path("request-body-kinds.yml");
    fs::copy(fixture("phase1-round-trip.yml"), &workspace).unwrap();
    let bodies = [
        (r#"{"type":"json","data":"{}"}"#, "json", "data"),
        (r#"{"type":"xml","data":"<pet/>"}"#, "xml", "data"),
        (
            r#"{"type":"sparql","data":"SELECT * WHERE { ?s ?p ?o }"}"#,
            "sparql",
            "data",
        ),
        (
            r#"{"type":"form-urlencoded","data":[{"name":"title","value":"hello"},{"name":"draft","value":"true","disabled":true}]}"#,
            "form-urlencoded",
            "data",
        ),
        (
            r#"{"type":"multipart-form","data":[{"name":"caption","type":"text","value":"Summer"},{"name":"files","type":"file","value":["./one.png","./two.png"],"contentType":"image/png"}]}"#,
            "multipart-form",
            "data",
        ),
        (
            r#"{"type":"file","data":[{"filePath":"./archive.zip","contentType":"application/zip","selected":true},{"filePath":"./other.zip","contentType":"application/zip","selected":false}]}"#,
            "file",
            "data",
        ),
    ];
    for (body, kind, field) in bodies {
        set_fields(&workspace, "items/0", &["--body", body]);
        let request = request_json(&workspace, "items/0");
        assert_eq!(request["body"]["mode"], "single", "{kind}");
        assert_eq!(request["body"]["value"]["type"], kind, "{kind}");
        assert!(request["body"]["value"].get(field).is_some(), "{kind}");
    }
    let file = request_json(&workspace, "items/0");
    assert_eq!(file["body"]["value"]["data"][0]["selected"], true);
    assert_eq!(file["body"]["value"]["data"][1]["selected"], false);
    assert_eq!(
        file["body"]["value"]["data"][0]["filePath"],
        "./archive.zip"
    );

    set_fields(&workspace, "items/0", &["--body", bodies[3].0]);
    let form = request_json(&workspace, "items/0");
    assert_eq!(form["body"]["value"]["data"][1]["disabled"], true);
    assert_eq!(form["body"]["value"]["data"][0]["disabled"], false);

    set_fields(&workspace, "items/0", &["--body", bodies[4].0]);
    let multipart = request_json(&workspace, "items/0");
    assert_eq!(multipart["body"]["value"]["data"][0]["type"], "text");
    assert!(multipart["body"]["value"]["data"][0]["contentType"].is_null());
    assert_eq!(
        multipart["body"]["value"]["data"][1]["value"],
        serde_json::json!(["./one.png", "./two.png"])
    );
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("type: multipart-form"));
    assert!(saved.contains("contentType: image/png"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn body_write_keeps_an_existing_variant_list() {
    let workspace = temporary_path("request-body-variants.yml");
    fs::copy(fixture("phase1-bodies-auth-environments.yml"), &workspace).unwrap();
    let updated = set_fields(
        &workspace,
        "items/4",
        &["--body", r#"{"type":"text","data":"replaced"}"#],
    );
    assert_eq!(updated["body"]["mode"], "variants");
    assert_eq!(updated["body"]["variants"][0]["title"], "JSON");
    assert_eq!(updated["body"]["variants"][0]["selected"], true);
    assert_eq!(updated["body"]["variants"][0]["body"]["type"], "text");
    assert_eq!(updated["body"]["variants"][0]["body"]["data"], "replaced");
    assert_eq!(updated["body"]["variants"][1]["title"], "Text");
    assert_eq!(updated["body"]["variants"][1]["selected"], false);
    assert_eq!(updated["body"]["variants"][1]["body"]["data"], "enabled");
    assert_eq!(updated["authentication"]["type"], "oauth2");
    assert_eq!(
        updated["authentication"]["properties"]["flow"],
        "client_credentials"
    );
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("title: JSON"));
    assert!(saved.contains("title: Text"));
    assert!(saved.contains("data: enabled"));

    set_fields(&workspace, "items/4", &["--body", "null"]);
    let cleared = request_json(&workspace, "items/4");
    assert!(cleared["body"].is_null());
    assert_eq!(cleared["authentication"]["type"], "oauth2");
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(!saved.contains("title: JSON"));
    assert!(!saved.contains("title: Text"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn body_write_rejects_ambiguous_variant_selection_without_writing() {
    let workspace = temporary_path("request-body-ambiguous.yml");
    let source = concat!(
        "opencollection: 1.0.0\ninfo: { name: Variants }\nbundled: true\nitems:\n",
        "  - info: { name: Variant, type: http }\n    http:\n      method: POST\n      body:\n",
        "        - { title: One, selected: true, body: { type: text, data: a } }\n",
        "        - { title: Two, selected: true, body: { type: text, data: b } }\n",
    );
    fs::write(&workspace, source).unwrap();
    let output = probe()
        .args(["request", "set"])
        .arg(&workspace)
        .args([
            "items/0",
            "--body",
            r#"{"type":"text","data":"nope"}"#,
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "request_configuration");
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("multiple selected values")
    );
    assert_eq!(fs::read_to_string(&workspace).unwrap(), source);
    fs::remove_file(workspace).unwrap();
}

#[test]
fn create_persists_headers_parameters_body_and_auth() {
    let workspace = temporary_path("request-create-fields.yml");
    fs::copy(fixture("phase1-round-trip.yml"), &workspace).unwrap();
    let output = probe()
        .args(["request", "create"])
        .arg(&workspace)
        .args([
            "--name",
            "Upload",
            "--method",
            "POST",
            "--url",
            "https://example.com/upload",
            "--headers",
            r#"[{"name":"Accept","value":"application/json"}]"#,
            "--query-parameters",
            r#"[{"name":"limit","value":"10"}]"#,
            "--path-parameters",
            r#"[{"name":"id","value":"1"}]"#,
            "--body",
            r#"{"type":"xml","data":"<a/>"}"#,
            "--auth",
            r#"{"type":"bearer","token":"{{token}}"}"#,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let created: Value = serde_json::from_slice(&output.stdout).unwrap();
    let selector = created["selector"].as_str().unwrap();
    let request = request_json(&workspace, selector);
    assert_eq!(request["name"], "Upload");
    assert_eq!(request["method"], "POST");
    assert_eq!(request["headers"][0]["name"], "Accept");
    assert_eq!(request["headers"][0]["disabled"], false);
    assert_eq!(request["queryParameters"][0]["value"], "10");
    assert_eq!(request["pathParameters"][0]["name"], "id");
    assert_eq!(request["body"]["value"]["type"], "xml");
    assert_eq!(
        request["authentication"]["properties"]["token"],
        "{{token}}"
    );
    let saved = fs::read_to_string(&workspace).unwrap();
    assert!(saved.contains("type: xml"));
    assert!(saved.contains("type: bearer"));
    assert!(saved.contains("type: path"));
    assert!(saved.contains("type: query"));
    fs::remove_file(workspace).unwrap();
}

#[test]
fn http_field_writes_reject_invalid_values_and_graphql_bodies() {
    let graphql = temporary_path("request-graphql-fields.yml");
    fs::copy(fixture("graphql-http.yml"), &graphql).unwrap();
    let original = fs::read(&graphql).unwrap();
    let output = probe()
        .args(["request", "set"])
        .arg(&graphql)
        .args([
            "items/0",
            "--body",
            r#"{"type":"text","data":"nope"}"#,
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["category"], "invalid_arguments");
    assert_eq!(
        value["error"]["message"],
        "HTTP body updates cannot be applied to a native GraphQL request"
    );
    assert_eq!(fs::read(&graphql).unwrap(), original);

    let headers = set_fields(
        &graphql,
        "items/0",
        &[
            "--headers",
            r#"[{"name":"Accept","value":"application/json"}]"#,
            "--auth",
            r#"{"type":"bearer","token":"{{login}}"}"#,
        ],
    );
    assert_eq!(headers["type"], "graphql");
    assert!(
        headers["graphql"]["query"]
            .as_str()
            .unwrap()
            .starts_with("query Viewer")
    );
    assert_eq!(headers["headers"][0]["name"], "Accept");
    assert_eq!(headers["authentication"]["type"], "bearer");
    let saved = fs::read_to_string(&graphql).unwrap();
    assert!(saved.contains("type: graphql"));
    assert!(!saved.contains("type: http"));
    fs::remove_file(graphql).unwrap();

    let workspace = temporary_path("request-invalid-fields.yml");
    fs::copy(fixture("phase1-round-trip.yml"), &workspace).unwrap();
    let cases = [
        (
            &["--headers", r#"{"name":"A","value":"B"}"#][..],
            "headers must be a JSON array or null",
        ),
        (
            &["--query-parameters", r#"[{"name":"a"}]"#][..],
            "query parameter must be a JSON object with string name and value",
        ),
        (
            &["--path-parameters", r#"[{"name":"a","value":1}]"#][..],
            "path parameter must be a JSON object with string name and value",
        ),
        (
            &["--headers", r#"[{"name":"A","value":"B","extra":true}]"#][..],
            "header contains unsupported field 'extra'",
        ),
        (
            &["--body", r#"{"type":"yaml","data":"nope"}"#][..],
            "HTTP body type must be json, text, xml, sparql, form-urlencoded, multipart-form, or file",
        ),
        (
            &[
                "--body",
                r#"{"type":"file","data":[{"filePath":"./a.zip","contentType":"application/zip"}]}"#,
            ][..],
            "file body entry must be a JSON object with string filePath, string contentType, and boolean selected",
        ),
        (
            &["--auth", "inherit"][..],
            "authentication must be a JSON object, the string \"inherit\", or null",
        ),
        (
            &[
                "--auth",
                r#"{"type":"basic","username":"a","token":"nope"}"#,
            ][..],
            "basic authentication contains unsupported field 'token'",
        ),
        (
            &["--auth", "{}"][..],
            "authentication type must be a non-empty string",
        ),
        (
            &["--auth", r#"{"type":"whatever"}"#][..],
            "authentication type must be inherit, basic, bearer, or apikey",
        ),
        (
            &["--body", "null", "--body", "null"][..],
            "--body may only be specified once",
        ),
    ];
    for (args, message) in cases {
        let output = probe()
            .args(["request", "set"])
            .arg(&workspace)
            .arg("items/0")
            .args(args.iter().copied())
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["category"], "invalid_arguments", "{args:?}");
        assert_eq!(value["error"]["message"], message, "{args:?}");
    }

    let created = probe()
        .args(["request", "create"])
        .arg(&workspace)
        .args([
            "--name",
            "Bad",
            "--type",
            "graphql",
            "--body",
            r#"{"type":"text","data":"nope"}"#,
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(created.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(
        value["error"]["message"],
        "HTTP body updates cannot be applied to a native GraphQL request"
    );
    fs::remove_file(workspace).unwrap();
}
