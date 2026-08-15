use std::path::PathBuf;

use probe_opencollection::{load_workspace, load_workspace_from_str};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/opencollection")
        .join(path)
}

#[test]
fn loads_bundled_file_with_structural_selectors() {
    let loaded = load_workspace(fixture("phase1-bundled.yml"))
        .expect("bundled workspace fixture should load");
    let selectors: Vec<_> = loaded
        .requests()
        .iter()
        .map(|request| request.selector())
        .collect();

    assert_eq!(selectors, ["items/0/items/0", "items/1"]);
    let key = loaded
        .request_key("items/0/items/0")
        .expect("selector should resolve");
    assert_eq!(
        loaded
            .workspace()
            .request(key)
            .and_then(|request| request.metadata.name.as_deref()),
        Some("List pets")
    );
}

#[test]
fn loads_unbundled_directory_with_relative_path_selectors() {
    let loaded =
        load_workspace(fixture("unbundled")).expect("unbundled workspace fixture should load");
    let selectors: Vec<_> = loaded
        .requests()
        .iter()
        .map(|request| request.selector())
        .collect();

    assert_eq!(selectors, ["health.yml", "users/list-users.yml"]);
    assert_eq!(loaded.workspace().request_count(), 2);
    assert_eq!(loaded.workspace().folder_count(), 1);
    assert_eq!(loaded.workspace().environments().len(), 1);
    assert_eq!(loaded.workspace().environments()[0].name, "development");
}

#[test]
fn bundled_selector_uses_source_position_when_unsupported_items_are_skipped() {
    let loaded = load_workspace(fixture("phase3-bundled-selectors.yml"))
        .expect("bundled selector fixture should load");

    assert_eq!(loaded.requests().len(), 1);
    assert_eq!(loaded.requests()[0].selector(), "items/1");
}

#[test]
fn loads_bundled_workspace_from_yaml_source() {
    let source = std::fs::read_to_string(fixture("phase1-bundled.yml")).unwrap();
    let loaded = load_workspace_from_str(&source).expect("source workspace should load");

    let selectors: Vec<_> = loaded
        .requests()
        .iter()
        .map(|request| request.selector())
        .collect();
    assert_eq!(selectors, ["items/0/items/0", "items/1"]);
}
