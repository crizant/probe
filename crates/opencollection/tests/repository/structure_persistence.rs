use super::*;
use probe_core::ItemKind;
use probe_opencollection::ItemLocator;

#[test]
fn creation_preserves_omitted_and_explicit_methods() {
    let path = temporary_path("protocol-defaults.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    for (name, protocol, method) in [
        ("HTTP default", RequestProtocol::Http, None),
        ("GraphQL default", RequestProtocol::Graphql, None),
        ("HTTP override", RequestProtocol::Http, Some("PATCH")),
        ("GraphQL override", RequestProtocol::Graphql, Some("GET")),
    ] {
        let created = loaded
            .apply_structure(StructureOperation::CreateRequest {
                parent: None,
                index: None,
                name: name.to_owned(),
                method: method.map(str::to_owned),
                url: None,
                protocol,
                graphql: None,
                update: None,
            })
            .unwrap();
        let reloaded = load_workspace(&path).unwrap();
        let key = reloaded.request_key(&created.selector.unwrap()).unwrap();
        let request = reloaded.workspace().request(key).unwrap();
        assert_eq!(request.kind.protocol(), protocol);
        assert_eq!(request.method.as_deref(), method);
        assert!(request.http_body().is_none());
        assert!(request.graphql().is_none());
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn bundled_create_writes_complete_request_in_one_operation() {
    let path = temporary_path("complete-create.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let result = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: None,
            index: None,
            name: "Complete".to_owned(),
            method: Some("POST".to_owned()),
            url: Some("https://example.test".to_owned()),
            protocol: RequestProtocol::Http,
            graphql: None,
            update: Some(probe_core::RequestUpdate {
                headers: Some(vec![probe_core::Header {
                    name: "X-Test".to_owned(),
                    value: "yes".to_owned(),
                    disabled: false,
                }]),
                ..probe_core::RequestUpdate::default()
            }),
        })
        .unwrap();
    let reloaded = load_workspace(&path).unwrap();
    let key = reloaded
        .request_key(result.selector.as_deref().unwrap())
        .unwrap();
    assert_eq!(
        reloaded.workspace().request(key).unwrap().headers[0].name,
        "X-Test"
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn invalid_create_update_leaves_bundled_source_untouched() {
    let path = temporary_path("invalid-complete-create.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let original = fs::read(&path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let result = loaded.apply_structure(StructureOperation::CreateRequest {
        parent: None,
        index: None,
        name: "Invalid".to_owned(),
        method: Some("POST".to_owned()),
        url: None,
        protocol: RequestProtocol::Graphql,
        graphql: None,
        update: Some(probe_core::RequestUpdate {
            body: FieldPatch::Clear,
            ..probe_core::RequestUpdate::default()
        }),
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_file(path).unwrap();
}

#[test]
fn unbundled_create_writes_complete_request_before_publishing_file() {
    let root = temporary_path("complete-create-unbundled");
    copy_directory(&fixture("phase16-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();
    let result = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: Some("group".to_owned()),
            index: None,
            name: "Complete".to_owned(),
            method: Some("POST".to_owned()),
            url: Some("https://example.test".to_owned()),
            protocol: RequestProtocol::Http,
            graphql: None,
            update: Some(probe_core::RequestUpdate {
                headers: Some(vec![probe_core::Header {
                    name: "X-Test".to_owned(),
                    value: "yes".to_owned(),
                    disabled: false,
                }]),
                ..probe_core::RequestUpdate::default()
            }),
        })
        .unwrap();
    let reloaded = load_workspace(&root).unwrap();
    let key = reloaded
        .request_key(result.selector.as_deref().unwrap())
        .unwrap();
    assert_eq!(
        reloaded.workspace().request(key).unwrap().headers[0].value,
        "yes"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundled_structure_edits_save_reload_and_preserve_unknown_fields() {
    let path = temporary_path("phase16-bundled.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();

    let created = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: Some("items/1".to_owned()),
            index: Some(0),
            name: "Created".to_owned(),
            method: Some("PUT".to_owned()),
            url: Some("https://example.com/created".to_owned()),
            protocol: RequestProtocol::Http,
            graphql: None,
            update: None,
        })
        .unwrap();
    assert_eq!(created.selector.as_deref(), Some("items/1/items/0"));
    let renamed = loaded
        .apply_structure(StructureOperation::Rename {
            target: ItemLocator::new(ItemKind::Request, "items/1/items/0"),
            name: "Renamed".to_owned(),
        })
        .unwrap();
    assert_eq!(
        renamed
            .selector_remaps
            .get("items/1/items/0")
            .map(String::as_str),
        Some("items/1/items/0")
    );
    let renamed_in_memory = loaded
        .requests()
        .iter()
        .filter(|located| {
            loaded
                .workspace()
                .request(located.key())
                .unwrap()
                .metadata
                .name
                .as_deref()
                == Some("Renamed")
        })
        .map(|located| located.selector())
        .collect::<Vec<_>>();
    assert_eq!(renamed_in_memory, ["items/1/items/0"]);
    let moved = loaded
        .apply_structure(StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Request, "items/1/items/0"),
            parent: None,
            index: Some(0),
        })
        .unwrap();
    assert_eq!(moved.selector.as_deref(), Some("items/0"));
    loaded
        .apply_structure(StructureOperation::CreateFolder {
            parent: None,
            index: None,
            name: "Empty".to_owned(),
        })
        .unwrap();
    loaded
        .apply_structure(StructureOperation::Delete {
            target: ItemLocator::new(ItemKind::Request, "items/1"),
        })
        .unwrap();

    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(reloaded.requests().len(), 2);
    let renamed = reloaded
        .workspace()
        .request(reloaded.request_key("items/0").unwrap())
        .unwrap();
    assert_eq!(renamed.metadata.name.as_deref(), Some("Renamed"));
    assert_eq!(renamed.method.as_deref(), Some("PUT"));
    assert_eq!(reloaded.folders().len(), 2);
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("vendor.example"));
    assert!(saved.contains("x-folder: retained"));
    fs::remove_file(path).unwrap();
}

#[test]
fn bundled_move_handles_reordering_and_destination_index_shifts() {
    let path = temporary_path("phase16-bundled-moves.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();

    let moved = loaded
        .apply_structure(StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Request, "items/0"),
            parent: Some("items/1".to_owned()),
            index: Some(1),
        })
        .unwrap();
    assert_eq!(moved.selector.as_deref(), Some("items/0/items/1"));
    assert_eq!(
        moved.selector_remaps.get("items/0").map(String::as_str),
        Some("items/0/items/1")
    );
    assert_eq!(
        moved.selector_remaps.get("items/1").map(String::as_str),
        Some("items/0")
    );
    assert_eq!(
        moved
            .selector_remaps
            .get("items/1/items/0")
            .map(String::as_str),
        Some("items/0/items/0")
    );
    let moved_key = loaded.request_key("items/0/items/1").unwrap();
    loaded.request_mut(moved_key).unwrap().url = Some("https://example.com/unsaved".to_owned());
    let reordered = loaded
        .apply_structure(StructureOperation::Reorder {
            target: ItemLocator::new(ItemKind::Request, "items/0/items/1"),
            index: 0,
        })
        .unwrap();
    assert_eq!(reordered.selector.as_deref(), Some("items/0/items/0"));
    assert_eq!(
        reordered
            .selector_remaps
            .get("items/0/items/1")
            .map(String::as_str),
        Some("items/0/items/0")
    );
    assert_eq!(
        reordered
            .selector_remaps
            .get("items/0/items/0")
            .map(String::as_str),
        Some("items/0/items/1")
    );
    assert_eq!(
        loaded
            .workspace()
            .request(loaded.request_key("items/0/items/0").unwrap())
            .unwrap()
            .url
            .as_deref(),
        Some("https://example.com/unsaved")
    );

    let reloaded = load_workspace(&path).unwrap();
    let first = reloaded
        .workspace()
        .request(reloaded.request_key("items/0/items/0").unwrap())
        .unwrap();
    assert_eq!(first.metadata.name.as_deref(), Some("Alpha"));
    fs::remove_file(path).unwrap();
}

#[test]
fn bundled_duplicate_request_copies_request_after_original() {
    let path = temporary_path("phase16-bundled-duplicate.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();

    let duplicated = loaded
        .apply_structure(StructureOperation::DuplicateRequest {
            selector: "items/0".to_owned(),
        })
        .unwrap();

    assert_eq!(duplicated.selector.as_deref(), Some("items/1"));
    assert_eq!(
        duplicated
            .selector_remaps
            .get("items/0")
            .map(String::as_str),
        Some("items/0")
    );
    assert_eq!(
        duplicated
            .selector_remaps
            .get("items/1")
            .map(String::as_str),
        Some("items/2")
    );
    assert_eq!(
        duplicated
            .selector_remaps
            .get("items/1/items/0")
            .map(String::as_str),
        Some("items/2/items/0")
    );

    let reloaded = load_workspace(&path).unwrap();
    let copy = reloaded
        .workspace()
        .request(reloaded.request_key("items/1").unwrap())
        .unwrap();
    assert_eq!(copy.metadata.name.as_deref(), Some("Alpha Copied"));
    assert_eq!(copy.method.as_deref(), Some("GET"));
    assert_eq!(copy.url.as_deref(), Some("https://example.com/alpha"));
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("x-request: retained"));
    fs::remove_file(path).unwrap();
}

#[test]
fn unbundled_websocket_rename_and_reorder_preserve_request_and_unknown_fields() {
    let root = temporary_path("websocket-rename-reorder-unbundled");
    copy_directory(&fixture("websocket-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();
    let mut expected = loaded
        .workspace()
        .request(loaded.request_key("socket.yml").unwrap())
        .unwrap()
        .clone();
    expected.metadata.name = Some("Renamed Socket".to_owned());
    let sibling = fs::read(root.join("health.yml")).unwrap();
    let mut expected_yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(&fs::read(root.join("socket.yml")).unwrap()).unwrap();
    expected_yaml["info"]["name"] = "Renamed Socket".into();

    let renamed = loaded
        .apply_structure(StructureOperation::Rename {
            target: ItemLocator::new(ItemKind::Request, "socket.yml"),
            name: "Renamed Socket".to_owned(),
        })
        .unwrap();
    let selector = "renamed-socket.yml";
    assert_eq!(renamed.selector.as_deref(), Some(selector));
    assert_eq!(renamed.index, Some(0));
    assert_eq!(
        renamed
            .selector_remaps
            .get("socket.yml")
            .map(String::as_str),
        Some(selector)
    );
    assert!(loaded.request_key("socket.yml").is_none());
    assert!(!root.join("socket.yml").exists());
    assert_eq!(fs::read(root.join("health.yml")).unwrap(), sibling);
    let mut reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded
            .workspace()
            .request(reloaded.request_key(selector).unwrap()),
        Some(&expected)
    );

    let reordered = reloaded
        .apply_structure(StructureOperation::Reorder {
            target: ItemLocator::new(ItemKind::Request, selector),
            index: 1,
        })
        .unwrap();
    assert_eq!(reordered.index, Some(1));
    expected.metadata.sequence = Some(2.0);
    expected_yaml["info"]["seq"] = 2.into();
    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded
            .requests()
            .iter()
            .map(|item| item.selector())
            .collect::<Vec<_>>(),
        ["health.yml", selector]
    );
    assert_eq!(
        reloaded
            .workspace()
            .request(reloaded.request_key(selector).unwrap()),
        Some(&expected)
    );
    assert_eq!(
        serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(&fs::read(root.join(selector)).unwrap())
            .unwrap(),
        expected_yaml
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbundled_graphql_rename_preserves_request_order_and_unknown_fields() {
    let root = temporary_path("graphql-rename-unbundled");
    copy_directory(&fixture("phase16-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();
    let selector = "group/graphql.yml";
    let key = loaded.request_key(selector).unwrap();
    let mut expected = loaded.workspace().request(key).unwrap().clone();
    expected.metadata.name = Some("Renamed GraphQL".to_owned());
    let sibling = fs::read(root.join("group/nested.yml")).unwrap();

    let result = loaded
        .apply_structure(StructureOperation::Rename {
            target: ItemLocator::new(ItemKind::Request, selector),
            name: "Renamed GraphQL".to_owned(),
        })
        .unwrap();
    let renamed_selector = "group/renamed-graphql.yml";
    assert_eq!(result.previous_selector.as_deref(), Some(selector));
    assert_eq!(result.selector.as_deref(), Some(renamed_selector));
    assert_eq!(result.parent.as_deref(), Some("group"));
    assert_eq!(result.index, Some(1));
    assert_eq!(
        result.selector_remaps.get(selector).map(String::as_str),
        Some(renamed_selector)
    );
    assert!(loaded.request_key(selector).is_none());
    assert_eq!(
        loaded
            .workspace()
            .request(loaded.request_key(renamed_selector).unwrap()),
        Some(&expected)
    );
    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded
            .workspace()
            .request(reloaded.request_key(renamed_selector).unwrap()),
        Some(&expected)
    );
    assert!(!root.join(selector).exists());
    assert_eq!(fs::read(root.join("group/nested.yml")).unwrap(), sibling);
    assert!(
        fs::read_to_string(root.join(renamed_selector))
            .unwrap()
            .contains("x-unsupported: retained")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbundled_graphql_reorder_and_move_preserve_request_and_unknown_fields() {
    let root = temporary_path("graphql-reorder-move-unbundled");
    copy_directory(&fixture("phase16-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();
    let selector = "group/graphql.yml";
    let mut expected = loaded
        .workspace()
        .request(loaded.request_key(selector).unwrap())
        .unwrap()
        .clone();
    expected.metadata.sequence = Some(1.0);
    let mut expected_yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(&fs::read(root.join(selector)).unwrap()).unwrap();
    expected_yaml["info"]["seq"] = 1.into();

    let reordered = loaded
        .apply_structure(StructureOperation::Reorder {
            target: ItemLocator::new(ItemKind::Request, selector),
            index: 0,
        })
        .unwrap();
    assert_eq!(reordered.selector.as_deref(), Some(selector));
    assert_eq!(reordered.parent.as_deref(), Some("group"));
    assert_eq!(reordered.index, Some(0));
    let mut reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded
            .requests()
            .iter()
            .map(|item| item.selector())
            .collect::<Vec<_>>(),
        ["alpha.yml", "group/graphql.yml", "group/nested.yml"]
    );
    assert_eq!(
        reloaded
            .workspace()
            .request(reloaded.request_key(selector).unwrap()),
        Some(&expected)
    );

    let moved = reloaded
        .apply_structure(StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Request, selector),
            parent: None,
            index: Some(0),
        })
        .unwrap();
    assert_eq!(moved.previous_selector.as_deref(), Some(selector));
    assert_eq!(moved.selector.as_deref(), Some("graphql.yml"));
    assert_eq!(moved.parent, None);
    assert_eq!(moved.index, Some(0));
    assert_eq!(
        moved.selector_remaps.get(selector).map(String::as_str),
        Some("graphql.yml")
    );
    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded
            .requests()
            .iter()
            .map(|item| item.selector())
            .collect::<Vec<_>>(),
        ["graphql.yml", "alpha.yml", "group/nested.yml"]
    );
    assert_eq!(
        reloaded
            .workspace()
            .request(reloaded.request_key("graphql.yml").unwrap()),
        Some(&expected)
    );
    assert_eq!(
        reloaded
            .workspace()
            .request(reloaded.request_key("group/nested.yml").unwrap())
            .unwrap()
            .metadata
            .sequence,
        Some(1.0)
    );
    assert!(reloaded.request_key(selector).is_none());
    assert!(!root.join(selector).exists());
    assert_eq!(
        serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(
            &fs::read(root.join("graphql.yml")).unwrap()
        )
        .unwrap(),
        expected_yaml
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbundled_structure_edits_persist_paths_order_and_unknown_fields() {
    let root = temporary_path("phase16-unbundled");
    copy_directory(&fixture("phase16-unbundled"), &root);
    let mut graphql: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(&fs::read(root.join("group/graphql.yml")).unwrap()).unwrap();
    graphql["info"]["seq"] = 2.into();
    let mut loaded = load_workspace(&root).unwrap();

    let created = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: Some("group".to_owned()),
            index: Some(0),
            name: "Created Request".to_owned(),
            method: Some("PATCH".to_owned()),
            url: Some("https://example.com/created".to_owned()),
            protocol: RequestProtocol::Http,
            graphql: None,
            update: None,
        })
        .unwrap();
    assert_eq!(
        created.selector.as_deref(),
        Some("group/created-request.yml")
    );
    let renamed = loaded
        .apply_structure(StructureOperation::Rename {
            target: ItemLocator::new(ItemKind::Request, "group/created-request.yml"),
            name: "Renamed Request".to_owned(),
        })
        .unwrap();
    assert_eq!(
        renamed.selector.as_deref(),
        Some("group/renamed-request.yml")
    );
    let folder = loaded
        .apply_structure(StructureOperation::CreateFolder {
            parent: None,
            index: Some(0),
            name: "Destination".to_owned(),
        })
        .unwrap();
    assert_eq!(folder.selector.as_deref(), Some("destination"));
    let moved = loaded
        .apply_structure(StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Request, "group/renamed-request.yml"),
            parent: Some("destination".to_owned()),
            index: Some(0),
        })
        .unwrap();
    assert_eq!(
        moved.selector.as_deref(),
        Some("destination/renamed-request.yml")
    );
    loaded
        .apply_structure(StructureOperation::Delete {
            target: ItemLocator::new(ItemKind::Request, "alpha.yml"),
        })
        .unwrap();

    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded.requests()[0].selector(),
        "destination/renamed-request.yml"
    );
    assert_eq!(reloaded.folders()[0].selector(), "destination");
    assert!(
        fs::read_to_string(root.join("opencollection.yml"))
            .unwrap()
            .contains("vendor.example")
    );
    assert!(
        fs::read_to_string(root.join("group/folder.yml"))
            .unwrap()
            .contains("x-folder: retained")
    );
    assert_eq!(
        serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(
            &fs::read(root.join("group/graphql.yml")).unwrap()
        )
        .unwrap(),
        graphql
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbundled_folder_edits_and_explicit_reordering_survive_reload() {
    let root = temporary_path("phase16-unbundled-folders");
    copy_directory(&fixture("phase16-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();

    loaded
        .apply_structure(StructureOperation::CreateFolder {
            parent: None,
            index: None,
            name: "Destination".to_owned(),
        })
        .unwrap();
    let renamed = loaded
        .apply_structure(StructureOperation::Rename {
            target: ItemLocator::new(ItemKind::Folder, "group"),
            name: "Renamed Group".to_owned(),
        })
        .unwrap();
    assert_eq!(renamed.selector.as_deref(), Some("renamed-group"));
    let moved = loaded
        .apply_structure(StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "renamed-group"),
            parent: Some("destination".to_owned()),
            index: Some(0),
        })
        .unwrap();
    assert_eq!(moved.selector.as_deref(), Some("destination/renamed-group"));
    assert!(
        loaded
            .request_key("destination/renamed-group/nested.yml")
            .is_some()
    );
    let reordered = loaded
        .apply_structure(StructureOperation::Reorder {
            target: ItemLocator::new(ItemKind::Folder, "destination"),
            index: 0,
        })
        .unwrap();
    assert_eq!(reordered.index, Some(0));
    loaded
        .apply_structure(StructureOperation::Delete {
            target: ItemLocator::new(ItemKind::Folder, "destination/renamed-group"),
        })
        .unwrap();

    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(reloaded.folders()[0].selector(), "destination");
    assert_eq!(reloaded.workspace().folder_count(), 1);
    assert_eq!(reloaded.workspace().request_count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbundled_duplicate_request_copies_request_after_original() {
    let root = temporary_path("phase16-unbundled-duplicate");
    copy_directory(&fixture("phase16-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();

    let duplicated = loaded
        .apply_structure(StructureOperation::DuplicateRequest {
            selector: "alpha.yml".to_owned(),
        })
        .unwrap();

    assert_eq!(duplicated.selector.as_deref(), Some("alpha-copied.yml"));
    assert_eq!(duplicated.index, Some(1));
    assert_eq!(
        duplicated
            .selector_remaps
            .get("alpha.yml")
            .map(String::as_str),
        Some("alpha.yml")
    );

    let reloaded = load_workspace(&root).unwrap();
    let copy = reloaded
        .workspace()
        .request(reloaded.request_key("alpha-copied.yml").unwrap())
        .unwrap();
    assert_eq!(copy.metadata.name.as_deref(), Some("Alpha Copied"));
    assert_eq!(copy.method.as_deref(), Some("GET"));
    assert_eq!(copy.url.as_deref(), Some("https://example.com/alpha"));
    let saved = fs::read_to_string(root.join("alpha-copied.yml")).unwrap();
    assert!(saved.contains("x-request: retained"));
    assert!(saved.contains("seq: 2"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn prepared_structure_edit_returns_disk_state_and_checks_the_prepared_baseline() {
    let path = temporary_path("phase16-bundled-prepared.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let key = loaded.request_key("items/0").unwrap();
    let item = probe_core::WorkspaceItemRef::Request(key);
    assert_eq!(loaded.item_selector(item), Some("items/0"));
    assert_eq!(loaded.item_key(item.kind(), "items/0"), Some(item));
    assert!(loaded.item_key(ItemKind::Folder, "items/0").is_none());
    let persisted_url = loaded.workspace().request(key).unwrap().url.clone();
    loaded.request_mut(key).unwrap().url = Some("https://example.com/unsaved".to_owned());

    let (result, disk) = loaded
        .prepare_structure(StructureOperation::Rename {
            target: ItemLocator::new(ItemKind::Request, "items/0"),
            name: "Renamed".to_owned(),
        })
        .unwrap()
        .execute()
        .unwrap();

    assert!(disk.item_selector(item).is_none());
    let renamed = disk
        .workspace()
        .request(
            disk.request_key(result.selector.as_deref().unwrap())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(renamed.metadata.name.as_deref(), Some("Renamed"));
    assert_eq!(renamed.url, persisted_url);
    let source = loaded.workspace().request(key).unwrap();
    assert_eq!(source.metadata.name.as_deref(), Some("Alpha"));
    assert_eq!(source.url.as_deref(), Some("https://example.com/unsaved"));

    let prepared = disk
        .prepare_structure(StructureOperation::Delete {
            target: ItemLocator::new(ItemKind::Request, "items/0"),
        })
        .unwrap();
    let external = fs::read_to_string(&path).unwrap() + "\nx-external: true\n";
    fs::write(&path, &external).unwrap();
    let conflict = prepared.execute().unwrap_err();
    assert!(matches!(
        conflict,
        StructureError::ConcurrentModification(_)
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
    fs::remove_file(path).unwrap();
}

#[test]
fn structural_targets_reject_the_wrong_kind_for_both_storage_formats() {
    for (fixture_name, request, folder) in [
        ("phase16-bundled.yml", "items/0", "items/1"),
        ("phase16-unbundled", "alpha.yml", "group"),
    ] {
        let loaded = load_workspace(fixture(fixture_name)).unwrap();
        let item = loaded.item_key(ItemKind::Folder, folder).unwrap();
        assert_eq!(loaded.item_selector(item), Some(folder));
        for (kind, selector) in [(ItemKind::Request, folder), (ItemKind::Folder, request)] {
            let target = ItemLocator::new(kind, selector);
            for operation in [
                StructureOperation::Rename {
                    target: target.clone(),
                    name: "Wrong".into(),
                },
                StructureOperation::Delete {
                    target: target.clone(),
                },
                StructureOperation::Move {
                    target: target.clone(),
                    parent: None,
                    index: None,
                },
                StructureOperation::Reorder {
                    target: target.clone(),
                    index: 0,
                },
            ] {
                assert!(matches!(loaded.prepare_structure(operation),
                    Err(StructureError::ItemNotFound { kind: actual, selector: actual_selector })
                        if actual == kind && actual_selector == selector));
            }
        }
    }
}

#[test]
fn structure_edits_reject_duplicates_invalid_destinations_and_conflicts() {
    let root = temporary_path("phase16-errors");
    copy_directory(&fixture("phase16-unbundled"), &root);
    let mut loaded = load_workspace(&root).unwrap();

    let reserved = "opencollection.yml";
    let rejected = loaded
        .apply_structure(StructureOperation::Delete {
            target: ItemLocator::new(ItemKind::Request, reserved.to_owned()),
        })
        .unwrap_err();
    assert!(matches!(
        rejected,
        StructureError::ItemNotFound {
            kind: ItemKind::Request,
            ..
        }
    ));
    assert!(root.join(reserved).exists());
    fs::create_dir(root.join("rogue")).unwrap();
    fs::write(
        root.join("rogue/folder.yml"),
        "info: { name: Rogue, type: folder }\n",
    )
    .unwrap();
    let unsupported_parent = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: Some("rogue".to_owned()),
            index: None,
            name: "Unsafe".to_owned(),
            method: None,
            url: None,
            protocol: RequestProtocol::Http,
            graphql: None,
            update: None,
        })
        .unwrap_err();
    assert!(matches!(
        unsupported_parent,
        StructureError::DestinationNotFound(_)
    ));
    assert!(!root.join("rogue/unsafe.yml").exists());
    fs::remove_dir_all(root.join("rogue")).unwrap();

    let duplicate = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: None,
            index: None,
            name: "Alpha".to_owned(),
            method: None,
            url: None,
            protocol: RequestProtocol::Http,
            graphql: None,
            update: None,
        })
        .unwrap_err();
    assert!(matches!(duplicate, StructureError::DuplicateDestination(_)));
    let descendant = loaded
        .apply_structure(StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "group"),
            parent: Some("group".to_owned()),
            index: None,
        })
        .unwrap_err();
    assert!(matches!(descendant, StructureError::InvalidDestination(_)));

    fs::write(
        root.join("external.yml"),
        "info: { name: External, type: http }\nhttp: { method: GET }\n",
    )
    .unwrap();
    let external_creation = loaded
        .apply_structure(StructureOperation::CreateFolder {
            parent: None,
            index: None,
            name: "Should Conflict".to_owned(),
        })
        .unwrap_err();
    assert!(matches!(
        external_creation,
        StructureError::ConcurrentModification(_)
    ));
    fs::remove_file(root.join("external.yml")).unwrap();

    fs::write(
        root.join("alpha.yml"),
        fs::read_to_string(root.join("alpha.yml")).unwrap() + "\nexternal: true\n",
    )
    .unwrap();
    let conflict = loaded
        .apply_structure(StructureOperation::Delete {
            target: ItemLocator::new(ItemKind::Request, "alpha.yml"),
        })
        .unwrap_err();
    assert!(matches!(
        conflict,
        StructureError::ConcurrentModification(_)
    ));
    assert!(root.join("alpha.yml").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundled_create_can_write_native_graphql_requests() {
    let path = temporary_path("graphql-create.yml");
    fs::copy(fixture("phase16-bundled.yml"), &path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let created = loaded
        .apply_structure(StructureOperation::CreateRequest {
            parent: None,
            index: Some(0),
            name: "Viewer".to_owned(),
            method: Some("POST".to_owned()),
            url: Some("https://example.com/graphql".to_owned()),
            protocol: RequestProtocol::Graphql,
            graphql: Some(probe_core::GraphqlUpdate {
                query: probe_core::FieldPatch::Set("query Viewer { viewer { login } }".to_owned()),
                operation_name: FieldPatch::Set("Viewer".to_owned()),
                ..probe_core::GraphqlUpdate::default()
            }),
            update: None,
        })
        .unwrap();
    let selector = created.selector.unwrap();
    let reloaded = load_workspace(&path).unwrap();
    let request = reloaded
        .workspace()
        .request(reloaded.request_key(&selector).unwrap())
        .unwrap();
    assert_eq!(request.kind.as_str(), "graphql");
    assert_eq!(
        request
            .selected_graphql()
            .unwrap()
            .unwrap()
            .operation_name
            .as_deref(),
        Some("Viewer")
    );
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("type: graphql"));
    fs::remove_file(path).unwrap();
}
