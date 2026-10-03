use super::*;
use probe_core::{CollectionUpdate, Documentation, FolderUpdate};

const DOCUMENTATION_COLLECTION: &str = concat!(
    "opencollection: 1.0.0\n",
    "info:\n",
    "  name: Docs\n",
    "  summary: short\n",
    "  description: collection has no description field\n",
    "bundled: true\n",
    "docs:\n",
    "  content: Collection guide\n",
    "  type: text/markdown\n",
    "items:\n",
    "  - info:\n",
    "      name: Pets\n",
    "      type: folder\n",
    "      description:\n",
    "        content: Pet folder\n",
    "        type: text/plain\n",
    "    docs: folder docs text\n",
    "    items:\n",
    "      - info:\n",
    "          name: Create pet\n",
    "          type: http\n",
    "          description:\n",
    "            content: Creates a pet\n",
    "            type: text/markdown\n",
    "        docs: request docs stay a string\n",
    "        http:\n",
    "          method: POST\n",
    "          url: https://example.com/pets\n",
);

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
