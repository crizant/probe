use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use probe_application::parse_curl;
use probe_core::{Collection, CollectionItem, CollectionMetadata};
use probe_opencollection::{create_bundled_workspace_from_collection, load_workspace};

#[test]
fn parsed_browser_request_survives_opencollection_persistence() {
    let mut request = parse_curl(include_str!(
        "../../../tests/fixtures/curl/chrome-posix.txt"
    ))
    .unwrap();
    request.metadata.name = Some("Imported browser request".into());
    let collection = Collection {
        metadata: CollectionMetadata {
            name: Some("cURL import".into()),
            ..Default::default()
        },
        items: vec![CollectionItem::Request(request.clone())],
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!(
        "probe-curl-{}-{}.yml",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    create_bundled_workspace_from_collection(&path, &collection).unwrap();
    let loaded = load_workspace(&path).unwrap();
    let key = loaded.request_key("items/0").unwrap();
    let persisted = loaded.workspace().request(key).unwrap();
    assert_eq!(persisted.method, request.method);
    assert_eq!(persisted.url, request.url);
    assert_eq!(persisted.headers, request.headers);
    assert_eq!(persisted.authentication, request.authentication);
    assert_eq!(persisted.http_body(), request.http_body());
    assert_eq!(persisted.settings, request.settings);
    fs::remove_file(path).unwrap();
}
