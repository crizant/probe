use probe_core::Workspace;
use probe_opencollection::parse;

#[path = "../benches/support/fixtures.rs"]
mod fixtures;

#[test]
fn generated_performance_fixtures_are_valid_and_exactly_sized() {
    for request_count in fixtures::WORKSPACE_SIZES {
        let source = fixtures::bundled_workspace(request_count);
        let parsed = parse(&source).expect("generated fixture should be valid OpenCollection YAML");
        let workspace = Workspace::from_collection(parsed.into_collection());

        assert_eq!(workspace.request_count(), request_count);
        assert_eq!(workspace.folder_count(), request_count.div_ceil(100));
    }
}
