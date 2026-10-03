#[allow(dead_code, unused_imports)]
mod common;

use common::*;

const COLLECTION: &str = concat!(
    "opencollection: 1.0.0\n",
    "info:\n",
    "  name: Docs\n",
    "  summary: short\n",
    "bundled: true\n",
    "docs:\n",
    "  content: Collection guide\n",
    "  type: text/markdown\n",
    "items:\n",
    "  - info:\n",
    "      name: Pets\n",
    "      type: folder\n",
    "      description: plain folder\n",
    "    docs: FOLDER-DOCS-SHOULD-NOT-APPEAR-IN-LIST\n",
    "    items:\n",
    "      - info:\n",
    "          name: Create pet\n",
    "          type: http\n",
    "          description:\n",
    "            content: Creates a pet\n",
    "            type: text/markdown\n",
    "        docs: REQUEST-DOCS-SHOULD-NOT-APPEAR-IN-LIST\n",
    "        http:\n",
    "          method: POST\n",
    "          url: https://example.com/pets\n",
);

#[test]
fn reads_and_writes_documentation_without_listing_docs_or_flattening_objects() {
    let path = temporary_path("documentation.yml");
    let path_arg = path.to_str().unwrap();
    fs::write(&path, COLLECTION).unwrap();

    let collection = run_json(&["collection", "get", path_arg]);
    assert_eq!(collection["collection"]["summary"], "short");
    assert_eq!(
        collection["collection"]["docs"]["content"],
        "Collection guide"
    );
    assert_eq!(collection["collection"]["docs"]["type"], "text/markdown");

    let requests = run_json(&["request", "list", path_arg]);
    assert!(requests["requests"][0].get("docs").is_none());
    assert!(requests["requests"][0].get("description").is_none());
    let request_list = probe()
        .args(["request", "list", path_arg])
        .output()
        .unwrap();
    let request_list = String::from_utf8_lossy(&request_list.stdout);
    assert!(!request_list.contains("REQUEST-DOCS-SHOULD-NOT-APPEAR-IN-LIST"));
    assert!(!request_list.contains("Creates a pet"));

    let folders = run_json(&["folder", "list", path_arg]);
    assert!(folders["folders"][0].get("docs").is_none());
    assert!(folders["folders"][0].get("description").is_none());
    let folder_list = probe().args(["folder", "list", path_arg]).output().unwrap();
    let folder_list = String::from_utf8_lossy(&folder_list.stdout);
    assert!(!folder_list.contains("FOLDER-DOCS-SHOULD-NOT-APPEAR-IN-LIST"));

    let human = probe()
        .args(["request", "get", path_arg, "items/0/items/0"])
        .output()
        .unwrap();
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("Description type: text/markdown\n"));
    assert!(human.contains("Description: Creates a pet\n"));
    assert!(human.contains("Docs: REQUEST-DOCS-SHOULD-NOT-APPEAR-IN-LIST\n"));

    let request = run_json(&["request", "get", path_arg, "items/0/items/0"]);
    assert_eq!(request["description"]["content"], "Creates a pet");
    assert_eq!(request["description"]["type"], "text/markdown");
    assert_eq!(request["docs"], "REQUEST-DOCS-SHOULD-NOT-APPEAR-IN-LIST");
    let folder = run_json(&["folder", "get", path_arg, "items/0"]);
    assert_eq!(folder["description"], "plain folder");
    assert_eq!(folder["docs"], "FOLDER-DOCS-SHOULD-NOT-APPEAR-IN-LIST");

    run_json(&[
        "request",
        "set",
        path_arg,
        "items/0/items/0",
        "--description-json",
        r#"{"content":"Updated pet","type":"text/plain"}"#,
        "--docs",
        "Updated request docs",
    ]);
    run_json(&[
        "folder",
        "set",
        path_arg,
        "items/0",
        "--description-json",
        r#"{"content":"Updated folder","type":"text/markdown"}"#,
        "--docs-json",
        r#""folder string""#,
    ]);
    let folder = run_json(&["folder", "get", path_arg, "items/0"]);
    assert_eq!(folder["docs"], "folder string");
    let multiline = probe()
        .args([
            "request",
            "set",
            path_arg,
            "items/0/items/0",
            "--docs",
            "one\ntwo",
        ])
        .output()
        .unwrap();
    assert!(multiline.status.success(), "{multiline:?}");
    let human = String::from_utf8(multiline.stdout).unwrap();
    assert!(human.contains("Docs:\none\ntwo\n"));
    assert!(!human.contains("Docs: one\ntwo"));
    run_json(&[
        "request",
        "set",
        path_arg,
        "items/0/items/0",
        "--docs",
        "Updated request docs",
    ]);
    run_json(&["folder", "set", path_arg, "items/0", "--docs-json", "null"]);
    run_json(&[
        "collection",
        "set",
        path_arg,
        "--summary",
        "shorter",
        "--docs",
        "Collection text",
    ]);

    let request = run_json(&["request", "get", path_arg, "items/0/items/0"]);
    assert_eq!(request["description"]["content"], "Updated pet");
    assert_eq!(request["description"]["type"], "text/plain");
    assert!(request["description"].as_str().is_none());
    assert_eq!(request["docs"], "Updated request docs");
    let folder = run_json(&["folder", "get", path_arg, "items/0"]);
    assert_eq!(folder["description"]["content"], "Updated folder");
    assert_eq!(folder["description"]["type"], "text/markdown");
    assert!(folder["docs"].is_null());
    let collection = run_json(&["collection", "get", path_arg]);
    assert_eq!(collection["collection"]["summary"], "shorter");
    assert_eq!(collection["collection"]["docs"], "Collection text");

    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("content: Updated pet"));
    assert!(saved.contains("type: text/plain"));
    assert!(saved.contains("docs: Updated request docs"));
    assert!(saved.contains("content: Updated folder"));
    assert!(saved.contains("type: text/markdown"));
    assert!(saved.contains("summary: shorter"));
    assert!(saved.contains("docs: Collection text"));
    assert!(!saved.contains("content: Updated request docs"));

    let rejected = probe()
        .args([
            "request",
            "set",
            path_arg,
            "items/0/items/0",
            "--docs-json",
            r#"{"content":"nope","type":"text/plain"}"#,
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("request docs must be a string"));

    let rejected = probe()
        .args([
            "folder",
            "set",
            path_arg,
            "items/0",
            "--description-json",
            "{",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .contains("--description-json must be a JSON string")
    );

    let rejected = probe()
        .args(["collection", "set", path_arg, "--description", "nope"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("not description"));

    fs::remove_file(path).unwrap();
}

#[test]
fn unset_omits_documentation_keys_and_json_null_keeps_them() {
    let path = temporary_path("documentation-unset.yml");
    let path_arg = path.to_str().unwrap();
    fs::write(
        &path,
        concat!(
            "opencollection: 1.0.0\n",
            "info:\n",
            "  name: Docs\n",
            "  summary: short\n",
            "bundled: true\n",
            "items:\n",
            "  - info:\n",
            "      name: Pets\n",
            "      type: folder\n",
            "      description: plain folder\n",
            "    docs: null\n",
            "    items:\n",
            "      - info:\n",
            "          name: Create pet\n",
            "          type: http\n",
            "          description: null\n",
            "        docs: request docs text\n",
            "        http:\n",
            "          method: GET\n",
            "          url: https://example.com\n",
        ),
    )
    .unwrap();

    let document = yaml(&path);
    assert!(document.get("docs").is_none());
    assert_eq!(document["info"]["summary"].as_str(), Some("short"));
    assert!(document["items"][0]["docs"].is_null());
    assert!(document["items"][0]["items"][0]["info"]["description"].is_null());
    assert_eq!(
        document["items"][0]["items"][0]["docs"].as_str(),
        Some("request docs text")
    );

    run_json(&["collection", "set", path_arg, "--docs-json", "null"]);
    let document = yaml(&path);
    assert!(document.get("docs").is_some_and(|docs| docs.is_null()));

    let unset = run_json(&["collection", "unset", path_arg, "--docs"]);
    assert_eq!(unset["operation"], "unset");
    assert_eq!(unset["fields"], serde_json::json!(["docs"]));
    let document = yaml(&path);
    assert!(document.get("docs").is_none());
    assert_eq!(document["info"]["summary"].as_str(), Some("short"));

    let unset = run_json(&["collection", "unset", path_arg, "--summary", "--docs"]);
    assert_eq!(unset["fields"], serde_json::json!(["summary", "docs"]));
    let document = yaml(&path);
    assert!(document["info"].get("summary").is_none());
    assert_eq!(document["info"]["name"].as_str(), Some("Docs"));
    assert!(document.get("docs").is_none());

    let unset = run_json(&["folder", "unset", path_arg, "items/0", "--docs"]);
    assert_eq!(unset["selector"], "items/0");
    assert_eq!(unset["fields"], serde_json::json!(["docs"]));
    let folder = &yaml(&path)["items"][0];
    assert!(folder.get("docs").is_none());
    assert_eq!(folder["info"]["description"].as_str(), Some("plain folder"));

    run_json(&[
        "folder",
        "set",
        path_arg,
        "items/0",
        "--description-json",
        "null",
        "--docs-json",
        "null",
    ]);
    let folder = &yaml(&path)["items"][0];
    assert!(folder["info"]["description"].is_null());
    assert!(folder["docs"].is_null());

    run_json(&[
        "folder",
        "unset",
        path_arg,
        "items/0",
        "--description",
        "--docs",
    ]);
    let folder = &yaml(&path)["items"][0];
    assert!(folder["info"].get("description").is_none());
    assert!(folder.get("docs").is_none());
    assert_eq!(folder["info"]["name"].as_str(), Some("Pets"));

    let unset = run_json(&[
        "request",
        "unset",
        path_arg,
        "items/0/items/0",
        "--description",
    ]);
    assert_eq!(unset["fields"], serde_json::json!(["description"]));
    let request = &yaml(&path)["items"][0]["items"][0];
    assert!(request["info"].get("description").is_none());
    assert_eq!(request["docs"].as_str(), Some("request docs text"));

    let unset = run_json(&["request", "unset", path_arg, "items/0/items/0", "--docs"]);
    assert_eq!(unset["selector"], "items/0/items/0");
    assert_eq!(unset["fields"], serde_json::json!(["docs"]));
    let request = &yaml(&path)["items"][0]["items"][0];
    assert!(request.get("docs").is_none());
    assert_eq!(request["http"]["method"].as_str(), Some("GET"));

    run_json(&["collection", "set", path_arg, "--docs-json", "null"]);
    let rejected = probe()
        .args(["collection", "unset", path_arg, "--docs-json", "null"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("write YAML null"));
    assert!(yaml(&path).get("docs").is_some_and(|docs| docs.is_null()));

    for args in [
        ["collection", "unset", path_arg, "", ""].as_slice(),
        ["folder", "unset", path_arg, "items/0", ""].as_slice(),
        ["request", "unset", path_arg, "items/0/items/0", ""].as_slice(),
    ] {
        let args: Vec<&str> = args.iter().copied().filter(|arg| !arg.is_empty()).collect();
        let rejected = probe().args(&args).output().unwrap();
        assert!(!rejected.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("invalid command"),
            "{args:?}: {}",
            String::from_utf8_lossy(&rejected.stderr)
        );
    }
    assert!(yaml(&path).get("docs").is_some_and(|docs| docs.is_null()));

    fs::remove_file(path).unwrap();
}

#[test]
fn collection_unset_rejects_description_json_like_description() {
    let description = probe()
        .args(["collection", "unset", "unused.yml", "--description"])
        .output()
        .unwrap();
    let description_json = probe()
        .args([
            "collection",
            "unset",
            "unused.yml",
            "--description-json",
            "null",
        ])
        .output()
        .unwrap();
    assert!(!description.status.success());
    assert_eq!(description.stderr, description_json.stderr);
    assert!(
        String::from_utf8_lossy(&description.stderr).contains("collections have summary and docs")
    );
    assert!(!String::from_utf8_lossy(&description.stderr).contains("set --description-json null"));
}

fn yaml(path: &std::path::Path) -> serde_yaml_ng::Value {
    serde_yaml_ng::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
