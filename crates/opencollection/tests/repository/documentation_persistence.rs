use super::*;
use probe_core::{CollectionUpdate, Documentation, FolderUpdate};

const DOCUMENTATION_COLLECTION: &str =
    include_str!("../../../../tests/fixtures/opencollection/documentation.yml");

#[test]
fn documentation_edits_round_trip_without_flattening_objects() {
    let path = temporary_path("documentation.yml");
    fs::write(&path, DOCUMENTATION_COLLECTION).unwrap();
    let mut loaded = load_workspace(&path).unwrap();

    loaded
        .update_request(
            "items/0/items/0",
            &RequestUpdate {
                method: FieldPatch::Set("PUT".into()),
                ..RequestUpdate::default()
            },
        )
        .unwrap();
    let mut reloaded = load_workspace(&path).unwrap();
    let request = request_at(&reloaded, "items/0/items/0");
    assert_eq!(request.method.as_deref(), Some("PUT"));
    assert_eq!(
        request.metadata.description,
        Some(markdown("Creates a pet"))
    );
    assert_eq!(request.docs.as_deref(), Some("request docs stay a string"));

    reloaded
        .update_request(
            "items/0/items/0",
            &RequestUpdate {
                description: FieldPatch::Set(markdown("Updated pet")),
                docs: FieldPatch::Set("Updated request docs".into()),
                ..RequestUpdate::default()
            },
        )
        .unwrap();
    reloaded
        .update_folder(
            "items/0",
            &FolderUpdate {
                description: FieldPatch::Set(Documentation::Null),
                docs: FieldPatch::Set(markdown("Folder guide")),
            },
        )
        .unwrap();
    reloaded
        .update_collection(&CollectionUpdate {
            summary: FieldPatch::Set("shorter".into()),
            docs: FieldPatch::Set(Documentation::Text("Collection text".into())),
        })
        .unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&saved).unwrap();
    assert_eq!(
        document["info"]["description"].as_str(),
        Some("collection has no description field")
    );
    assert_eq!(document["info"]["summary"].as_str(), Some("shorter"));
    assert_eq!(document["docs"].as_str(), Some("Collection text"));
    assert!(document["docs"].as_mapping().is_none());
    assert_eq!(
        document["items"][0]["info"]["description"],
        serde_yaml_ng::Value::Null
    );
    assert_eq!(
        document["items"][0]["docs"]["content"].as_str(),
        Some("Folder guide")
    );
    assert_eq!(
        document["items"][0]["docs"]["type"].as_str(),
        Some("text/markdown")
    );
    assert_eq!(
        document["items"][0]["items"][0]["info"]["description"]["content"].as_str(),
        Some("Updated pet")
    );
    assert_eq!(
        document["items"][0]["items"][0]["info"]["description"]["type"].as_str(),
        Some("text/markdown")
    );
    assert!(
        document["items"][0]["items"][0]["info"]["description"]
            .as_str()
            .is_none()
    );
    assert_eq!(
        document["items"][0]["items"][0]["docs"].as_str(),
        Some("Updated request docs")
    );

    let loaded = load_workspace(&path).unwrap();
    assert_eq!(
        loaded.workspace().metadata().summary.as_deref(),
        Some("shorter")
    );
    assert_eq!(
        loaded.workspace().metadata().docs,
        Some(Documentation::Text("Collection text".into()))
    );
    let folder = folder_at(&loaded, "items/0");
    assert_eq!(folder.metadata.description, Some(Documentation::Null));
    assert_eq!(folder.docs, Some(markdown("Folder guide")));
    let request = request_at(&loaded, "items/0/items/0");
    assert_eq!(request.metadata.description, Some(markdown("Updated pet")));
    assert_eq!(request.docs.as_deref(), Some("Updated request docs"));

    let mut loaded = load_workspace(&path).unwrap();
    loaded
        .update_folder(
            "items/0",
            &FolderUpdate {
                description: FieldPatch::Set(Documentation::Text("Plain folder".into())),
                ..FolderUpdate::default()
            },
        )
        .unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    assert_eq!(
        folder_at(&loaded, "items/0").metadata.description,
        Some(Documentation::Text("Plain folder".into()))
    );
    loaded
        .update_folder(
            "items/0",
            &FolderUpdate {
                description: FieldPatch::Set(Documentation::Text("Edited plain folder".into())),
                ..FolderUpdate::default()
            },
        )
        .unwrap();
    let saved: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        saved["items"][0]["info"]["description"].as_str(),
        Some("Edited plain folder")
    );
    let mut loaded = load_workspace(&path).unwrap();
    assert_eq!(
        folder_at(&loaded, "items/0").metadata.description,
        Some(Documentation::Text("Edited plain folder".into()))
    );
    loaded
        .update_request(
            "items/0/items/0",
            &RequestUpdate {
                description: FieldPatch::Clear,
                docs: FieldPatch::Clear,
                ..RequestUpdate::default()
            },
        )
        .unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&saved).unwrap();
    let request = &document["items"][0]["items"][0];
    assert!(request.get("docs").is_none());
    assert!(request["info"].get("description").is_none());
    assert_eq!(request["http"]["method"].as_str(), Some("PUT"));
}

#[test]
fn unbundled_folder_and_collection_documentation_round_trip() {
    let root = temporary_path("documentation-unbundled");
    fs::create_dir_all(root.join("pets")).unwrap();
    fs::write(root.join("notes.yml"), "note: unrelated\n").unwrap();
    fs::write(
        root.join("opencollection.yml"),
        concat!(
            "opencollection: 1.0.0\n",
            "info:\n",
            "  name: Docs\n",
            "  summary: short\n",
            "bundled: false\n",
            "docs: null\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("pets/folder.yml"),
        concat!(
            "info:\n",
            "  name: Pets\n",
            "  type: folder\n",
            "  description: plain folder\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("pets/create.yml"),
        concat!(
            "info:\n",
            "  name: Create pet\n",
            "  type: http\n",
            "http:\n",
            "  method: POST\n",
            "  url: https://example.com/pets\n",
            "docs: request docs stay a string\n",
        ),
    )
    .unwrap();

    let mut loaded = load_workspace(&root).unwrap();
    loaded
        .update_collection(&CollectionUpdate {
            summary: FieldPatch::Unchanged,
            docs: FieldPatch::Set(markdown("Collection guide")),
        })
        .unwrap();
    loaded
        .update_folder(
            "pets",
            &FolderUpdate {
                description: FieldPatch::Set(markdown("Pet folder")),
                docs: FieldPatch::Set(Documentation::Text("folder docs".into())),
            },
        )
        .unwrap();

    let collection: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join("opencollection.yml")).unwrap())
            .unwrap();
    assert_eq!(
        collection["docs"]["content"].as_str(),
        Some("Collection guide")
    );
    assert_eq!(collection["docs"]["type"].as_str(), Some("text/markdown"));
    assert_eq!(collection["info"]["summary"].as_str(), Some("short"));
    assert_eq!(
        fs::read_to_string(root.join("notes.yml")).unwrap(),
        "note: unrelated\n"
    );
    let folder: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join("pets/folder.yml")).unwrap())
            .unwrap();
    assert_eq!(
        folder["info"]["description"]["content"].as_str(),
        Some("Pet folder")
    );
    assert_eq!(folder["docs"].as_str(), Some("folder docs"));
    let request: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join("pets/create.yml")).unwrap())
            .unwrap();
    assert_eq!(request["docs"].as_str(), Some("request docs stay a string"));

    let loaded = load_workspace(&root).unwrap();
    assert_eq!(
        loaded.workspace().metadata().docs,
        Some(markdown("Collection guide"))
    );
    let folder = folder_at(&loaded, "pets");
    assert_eq!(folder.metadata.description, Some(markdown("Pet folder")));
    assert_eq!(folder.docs, Some(Documentation::Text("folder docs".into())));
}

#[test]
fn prepared_unbundled_documentation_writes_only_its_source_document() {
    let root = temporary_path("prepared-unbundled-documentation");
    let source = fixture("unbundled");
    copy_directory(&source, &root);
    let mut loaded = load_workspace(&root).unwrap();
    let folder_selector = loaded.folders()[0].selector().to_owned();
    let before_requests: Vec<_> = loaded
        .requests()
        .iter()
        .map(|request| {
            let path = root.join(request.selector());
            (path.clone(), fs::read(path).unwrap())
        })
        .collect();
    let prepared = loaded
        .prepare_collection_save(CollectionUpdate {
            summary: FieldPatch::Set("Prepared collection".into()),
            docs: FieldPatch::Set(markdown("Prepared collection docs")),
        })
        .unwrap();
    loaded
        .complete_documentation_save(prepared.execute().unwrap())
        .unwrap();
    let prepared = loaded
        .prepare_folder_save(
            &folder_selector,
            FolderUpdate {
                description: FieldPatch::Set(markdown("Prepared folder")),
                docs: FieldPatch::Set(Documentation::Text("Prepared folder docs".into())),
            },
        )
        .unwrap();
    loaded
        .complete_documentation_save(prepared.execute().unwrap())
        .unwrap();
    let reloaded = load_workspace(&root).unwrap();
    assert_eq!(
        reloaded.workspace().metadata().summary.as_deref(),
        Some("Prepared collection")
    );
    assert_eq!(
        reloaded.workspace().metadata().docs,
        Some(markdown("Prepared collection docs"))
    );
    let folder = folder_at(&reloaded, &folder_selector);
    assert_eq!(
        folder.metadata.description,
        Some(markdown("Prepared folder"))
    );
    assert_eq!(
        folder.docs,
        Some(Documentation::Text("Prepared folder docs".into()))
    );
    for (path, before) in before_requests {
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

fn markdown(content: &str) -> Documentation {
    Documentation::Content {
        content: content.to_owned(),
        media_type: "text/markdown".into(),
    }
}

fn request_at<'a>(
    loaded: &'a probe_opencollection::LoadedWorkspace,
    selector: &str,
) -> &'a Request {
    let key = loaded.request_key(selector).expect("request selector");
    loaded.workspace().request(key).expect("request")
}

fn folder_at<'a>(
    loaded: &'a probe_opencollection::LoadedWorkspace,
    selector: &str,
) -> &'a probe_core::WorkspaceFolder {
    let key = loaded.folder_key(selector).expect("folder selector");
    loaded.workspace().folder(key).expect("folder")
}

#[test]
fn prepared_documentation_saves_are_atomic_and_preserve_drafts_and_unknown_yaml() {
    let path = temporary_path("prepared-documentation.yml");
    fs::write(&path, DOCUMENTATION_COLLECTION).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let request = loaded.request_key("items/0/items/0").unwrap();
    loaded.request_mut(request).unwrap().url = Some("https://local.test/draft".into());
    let prepared = loaded
        .prepare_collection_save(CollectionUpdate {
            summary: FieldPatch::Set("Edited summary".into()),
            docs: FieldPatch::Set(markdown("Edited guide")),
        })
        .unwrap();
    assert_eq!(
        loaded.workspace().metadata().summary.as_deref(),
        Some("short")
    );
    loaded
        .complete_documentation_save(prepared.execute().unwrap())
        .unwrap();
    assert_eq!(
        loaded.workspace().request(request).unwrap().url.as_deref(),
        Some("https://local.test/draft")
    );
    let prepared = loaded
        .prepare_folder_save(
            "items/0",
            FolderUpdate {
                description: FieldPatch::Set(Documentation::Text("Edited description".into())),
                docs: FieldPatch::Set(markdown("Edited folder guide")),
            },
        )
        .unwrap();
    loaded
        .complete_documentation_save(prepared.execute().unwrap())
        .unwrap();
    let reloaded = load_workspace(&path).unwrap();
    assert_eq!(
        reloaded.workspace().metadata().summary.as_deref(),
        Some("Edited summary")
    );
    assert_eq!(
        reloaded.workspace().metadata().docs,
        Some(markdown("Edited guide"))
    );
    assert_eq!(
        folder_at(&reloaded, "items/0").metadata.description,
        Some(Documentation::Text("Edited description".into()))
    );
    assert_eq!(
        folder_at(&reloaded, "items/0").docs,
        Some(markdown("Edited folder guide"))
    );
    assert_eq!(
        request_at(&reloaded, "items/0/items/0").url.as_deref(),
        Some("https://example.com/pets")
    );
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        document["info"]["description"].as_str(),
        Some("collection has no description field")
    );
}

#[test]
fn prepared_documentation_rejects_empty_missing_and_externally_modified_sources() {
    let path = temporary_path("prepared-documentation-conflict.yml");
    fs::write(&path, DOCUMENTATION_COLLECTION).unwrap();
    let loaded = load_workspace(&path).unwrap();
    assert!(matches!(
        loaded.prepare_collection_save(CollectionUpdate::default()),
        Err(SaveError::EmptyUpdate)
    ));
    assert!(matches!(
        loaded.prepare_folder_save("items/0", FolderUpdate::default()),
        Err(SaveError::EmptyUpdate)
    ));
    let update = FolderUpdate {
        docs: FieldPatch::Set(markdown("Local guide")),
        ..FolderUpdate::default()
    };
    assert!(matches!(
        loaded.prepare_folder_save("missing", update.clone()),
        Err(SaveError::FolderNotFound(_))
    ));
    for prepared in [
        loaded.prepare_folder_save("items/0", update).unwrap(),
        loaded
            .prepare_collection_save(CollectionUpdate {
                summary: FieldPatch::Set("Local summary".into()),
                ..CollectionUpdate::default()
            })
            .unwrap(),
    ] {
        let external = format!("{DOCUMENTATION_COLLECTION}external: preserved\n");
        fs::write(&path, &external).unwrap();
        assert!(matches!(
            prepared.execute(),
            Err(SaveError::ConcurrentModification(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), external);
    }
}

#[test]
fn prepared_documentation_checks_live_baselines_before_and_after_writing() {
    let path = temporary_path("prepared-documentation-baseline.yml");
    fs::write(&path, DOCUMENTATION_COLLECTION).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    let update = CollectionUpdate {
        summary: FieldPatch::Set("Changed".into()),
        ..CollectionUpdate::default()
    };
    let stale = loaded.prepare_collection_save(update.clone()).unwrap();
    let saved = loaded
        .prepare_collection_save(update)
        .unwrap()
        .execute()
        .unwrap();
    loaded.complete_documentation_save(saved).unwrap();
    assert!(matches!(stale.execute(), Err(SaveError::StaleCompletion)));
    let saved = loaded
        .prepare_folder_save(
            "items/0",
            FolderUpdate {
                docs: FieldPatch::Set(markdown("Changed")),
                ..FolderUpdate::default()
            },
        )
        .unwrap()
        .execute()
        .unwrap();
    // A reloaded workspace has a different baseline from the completed write.
    let newer = load_workspace(&path).unwrap();
    loaded = newer;
    assert!(matches!(
        loaded.complete_documentation_save(saved),
        Err(SaveError::CommittedButNotIntegrated)
    ));
}
