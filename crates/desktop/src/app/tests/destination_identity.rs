use std::path::{Path, PathBuf};

use super::*;
use probe_core::{ItemKind, RequestProtocol};
use probe_opencollection::{ItemLocator, StructureOperation};

const COLLECTION: &str = r#"opencollection: 1.0.0
info:
  name: Destinations
bundled: true
items:
  - info: {name: Before, type: folder}
    items: []
  - info: {name: Target, type: folder}
    items:
      - info: {name: Marker, type: http}
        http: {method: GET, url: https://example.test/marker}
  - info: {name: After, type: folder}
    items: []
"#;

fn temp_collection(suffix: &str, source: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "probe-desktop-destination-{suffix}-{}-{unique}.yml",
        std::process::id()
    ));
    fs::write(&path, source).unwrap();
    path
}

fn folder_name(view: &ProbeApp, selector: &str) -> Option<String> {
    let loaded = view.loaded_workspace.as_ref()?;
    let key = loaded.folder_key(selector)?;
    loaded.workspace().folder(key)?.metadata.name.clone()
}

fn reset_collection(
    window: gpui_kit::WindowHandle<ProbeApp>,
    cx: &mut TestAppContext,
    path: &Path,
    source: &str,
) {
    fs::write(path, source).unwrap();
    let loaded = probe_opencollection::load_workspace(path).unwrap();
    window
        .update(cx, |view, _, _| {
            view.session_store = None;
            view.set_workspace(path.to_path_buf(), loaded);
        })
        .unwrap();
}

fn open_save_on_target(window: gpui_kit::WindowHandle<ProbeApp>, cx: &mut TestAppContext) {
    window
        .update(cx, |view, window, cx| {
            view.new_detached_request(RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_save_detached_request_dialog(key, window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Placed".into();
            view.toggle_destination_folder("items/1".into(), cx);
            assert_eq!(view.structure_dialog.as_ref().unwrap().parent, "items/1");
        })
        .unwrap();
}

fn external_edit(
    path: &Path,
    operation: StructureOperation,
) -> probe_opencollection::LoadedWorkspace {
    let mut external = probe_opencollection::load_workspace(path).unwrap();
    external.apply_structure(operation).unwrap();
    external
}

fn reconcile(
    window: gpui_kit::WindowHandle<ProbeApp>,
    cx: &mut TestAppContext,
    fresh: probe_opencollection::LoadedWorkspace,
) {
    window
        .update(cx, |view, window, cx| {
            assert!(
                view.reconcile_filesystem_workspace(fresh, BTreeMap::new(), window, cx),
                "{:?}",
                toast_debug(view)
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn open_dialog_follows_a_folder_through_reorder_insert_remove_and_rename(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let path = temp_collection("follow", COLLECTION);

    reset_collection(window, cx, &path, COLLECTION);
    open_save_on_target(window, cx);
    window
        .update(cx, |view, _, _| {
            let before = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/0")
                .unwrap();
            view.shell.collapse_folder(before);
        })
        .unwrap();
    let fresh = external_edit(
        &path,
        StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "items/1"),
            parent: None,
            index: Some(0),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let dialog = view
                .structure_dialog
                .as_ref()
                .expect("save dialog stays open");
            assert_eq!(dialog.parent, "items/0");
            assert_eq!(folder_name(view, &dialog.parent).as_deref(), Some("Target"));
            assert!(dialog.expanded_folders.contains("items/0"));
            assert!(
                !dialog.expanded_folders.contains("items/1"),
                "expansion must not stay on the folder that slid into the old position"
            );
            assert!(!dialog.destination_required);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let before = loaded.folder_key("items/1").unwrap();
            let target = loaded.folder_key("items/0").unwrap();
            assert_eq!(folder_name(view, "items/1").as_deref(), Some("Before"));
            assert!(!view.shell.folder_is_expanded(before));
            assert!(view.shell.folder_is_expanded(target));
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let saved = probe_opencollection::load_workspace(&path).unwrap();
    let placed = saved
        .requests()
        .iter()
        .find(|located| {
            saved
                .workspace()
                .request(located.key())
                .unwrap()
                .metadata
                .name
                .as_deref()
                == Some("Placed")
        })
        .expect("save writes through the structure operation");
    assert_eq!(placed.selector(), "items/0/items/1");

    reset_collection(window, cx, &path, COLLECTION);
    open_save_on_target(window, cx);
    let fresh = external_edit(
        &path,
        StructureOperation::CreateFolder {
            parent: None,
            index: Some(0),
            name: "Inserted".into(),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "items/2");
            assert_eq!(folder_name(view, &dialog.parent).as_deref(), Some("Target"));
            assert!(dialog.expanded_folders.contains("items/2"));
            assert!(!dialog.expanded_folders.contains("items/1"));
            assert!(!dialog.destination_required);
        })
        .unwrap();

    reset_collection(window, cx, &path, COLLECTION);
    open_save_on_target(window, cx);
    let fresh = external_edit(
        &path,
        StructureOperation::Delete {
            target: ItemLocator::new(ItemKind::Folder, "items/0"),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "items/0");
            assert_eq!(folder_name(view, &dialog.parent).as_deref(), Some("Target"));
            assert!(dialog.expanded_folders.contains("items/0"));
            assert!(!dialog.destination_required);
        })
        .unwrap();

    reset_collection(window, cx, &path, COLLECTION);
    open_save_on_target(window, cx);
    let fresh = external_edit(
        &path,
        StructureOperation::Rename {
            target: ItemLocator::new(ItemKind::Folder, "items/0"),
            name: "Renamed".into(),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "items/1");
            assert_eq!(folder_name(view, &dialog.parent).as_deref(), Some("Target"));
            assert_eq!(folder_name(view, "items/0").as_deref(), Some("Renamed"));
            assert!(dialog.expanded_folders.contains("items/1"));
            assert!(!dialog.destination_required);
        })
        .unwrap();

    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn same_named_siblings_are_not_silently_retargeted(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let distinguished = r#"opencollection: 1.0.0
info:
  name: Twins
bundled: true
items:
  - info: {name: Twin, type: folder}
    items:
      - info: {name: Left, type: http}
        http: {method: GET, url: https://example.test/left}
  - info: {name: Twin, type: folder}
    items:
      - info: {name: Right, type: http}
        http: {method: GET, url: https://example.test/right}
"#;
    let path = temp_collection("twins", distinguished);
    reset_collection(window, cx, &path, distinguished);
    window
        .update(cx, |view, window, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_create_request_dialog(RequestProtocol::Http, window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Created".into();
            assert_eq!(view.structure_dialog.as_ref().unwrap().parent, "items/1");
        })
        .unwrap();
    let fresh = external_edit(
        &path,
        StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "items/1"),
            parent: None,
            index: Some(0),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "items/0");
            assert!(!dialog.destination_required);
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let saved = probe_opencollection::load_workspace(&path).unwrap();
    let created = saved.request_key("items/0/items/1").unwrap();
    assert_eq!(
        saved
            .workspace()
            .request(created)
            .unwrap()
            .metadata
            .name
            .as_deref(),
        Some("Created")
    );
    assert!(saved.request_key("items/0/items/0").is_some());
    assert_eq!(
        saved
            .workspace()
            .request(saved.request_key("items/0/items/0").unwrap())
            .unwrap()
            .metadata
            .name
            .as_deref(),
        Some("Right")
    );

    let identical = r#"opencollection: 1.0.0
info:
  name: Twins
bundled: true
items:
  - info: {name: Twin, type: folder}
    items: []
  - info: {name: Twin, type: folder}
    items: []
"#;
    reset_collection(window, cx, &path, identical);
    window
        .update(cx, |view, window, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_create_folder_dialog(window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Nested".into();
        })
        .unwrap();
    let fresh = external_edit(
        &path,
        StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "items/1"),
            parent: None,
            index: Some(0),
        },
    );
    let after_move = fs::read(&path).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().expect("dialog stays open");
            assert!(dialog.destination_required);
            assert!(
                dialog.parent.is_empty(),
                "an identical sibling must not stay selected by its old position"
            );
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            assert!(view.structure_dialog.is_some());
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), after_move);

    let fresh = external_edit(
        &path,
        StructureOperation::CreateFolder {
            parent: None,
            index: Some(0),
            name: "Notes".into(),
        },
    );
    let after_unrelated = fs::read(&path).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().expect("dialog stays open");
            assert!(
                dialog.destination_required,
                "a later reload must not treat the cleared destination as collection root"
            );
            assert!(dialog.parent.is_empty());
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            assert!(view.structure_dialog.is_some());
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), after_unrelated);
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn creation_dialogs_share_destination_identity(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let path = temp_collection("modes", COLLECTION);
    for protocol in [RequestProtocol::Http, RequestProtocol::Graphql] {
        reset_collection(window, cx, &path, COLLECTION);
        window
            .update(cx, |view, window, cx| {
                let folder = view
                    .loaded_workspace
                    .as_ref()
                    .unwrap()
                    .folder_key("items/1")
                    .unwrap();
                view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
                view.open_create_request_dialog(protocol, window, cx);
            })
            .unwrap();
        let fresh = external_edit(
            &path,
            StructureOperation::Move {
                target: ItemLocator::new(ItemKind::Folder, "items/1"),
                parent: None,
                index: Some(0),
            },
        );
        reconcile(window, cx, fresh);
        window
            .update(cx, |view, _, _| {
                let dialog = view.structure_dialog.as_ref().unwrap();
                assert_eq!(dialog.parent, "items/0");
                assert_eq!(folder_name(view, &dialog.parent).as_deref(), Some("Target"));
                assert!(!dialog.destination_required);
            })
            .unwrap();
    }

    reset_collection(window, cx, &path, COLLECTION);
    window
        .update(cx, |view, window, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_create_folder_dialog(window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Inbox".into();
        })
        .unwrap();
    let fresh = external_edit(
        &path,
        StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "items/1"),
            parent: None,
            index: Some(0),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            assert_eq!(view.structure_dialog.as_ref().unwrap().parent, "items/0");
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let saved = probe_opencollection::load_workspace(&path).unwrap();
    assert!(
        saved.folder_key("items/0/items/1").is_some(),
        "new folder is created inside Target"
    );
    assert_eq!(
        saved
            .workspace()
            .folder(saved.folder_key("items/0/items/1").unwrap())
            .unwrap()
            .metadata
            .name
            .as_deref(),
        Some("Inbox")
    );

    reset_collection(window, cx, &path, COLLECTION);
    window
        .update(cx, |view, window, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_curl_import_dialog(window, cx);
            let dialog = view.structure_dialog.as_mut().unwrap();
            dialog.name = "Imported".into();
            dialog.curl.as_mut().unwrap().command = "curl https://example.test/import".into();
        })
        .unwrap();
    let fresh = external_edit(
        &path,
        StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "items/1"),
            parent: None,
            index: Some(0),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "items/0");
            assert!(!dialog.destination_required);
            assert_eq!(dialog.name, "Imported");
            assert_eq!(
                dialog.curl.as_ref().unwrap().command,
                "curl https://example.test/import"
            );
        })
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn directory_collection_destination_survives_an_external_reload(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let fixture = std::env::temp_dir().join(format!(
        "probe-desktop-destination-dir-{}-{unique}",
        std::process::id()
    ));
    write_directory_collection(&fixture);
    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), loaded);
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("users")
                .unwrap();
            view.shell.collapse_folder(folder);
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_create_request_dialog(RequestProtocol::Http, window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Created".into();
            assert_eq!(view.structure_dialog.as_ref().unwrap().parent, "users");
        })
        .unwrap();
    let folder_file = fixture.join("users/folder.yml");
    let mut source = fs::read_to_string(&folder_file).unwrap();
    source.push_str("  seq: 4\n");
    fs::write(&folder_file, source).unwrap();
    let fresh = probe_opencollection::load_workspace(&fixture).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "users");
            assert!(!dialog.destination_required);
            let users = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("users")
                .unwrap();
            assert!(!view.shell.folder_is_expanded(users));
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let saved = probe_opencollection::load_workspace(&fixture).unwrap();
    let created = saved
        .requests()
        .iter()
        .find(|located| {
            saved
                .workspace()
                .request(located.key())
                .and_then(|request| request.metadata.name.as_deref())
                == Some("Created")
        })
        .expect("directory collections still accept the folder selector");
    assert!(created.selector().starts_with("users/"));
    let _ = fs::remove_dir_all(fixture);
}

#[gpui_kit::test]
fn sidebar_folder_selection_follows_an_external_reorder(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let path = temp_collection("sidebar-selection", COLLECTION);
    reset_collection(window, cx, &path, COLLECTION);
    window
        .update(cx, |view, _, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
        })
        .unwrap();
    let fresh = external_edit(
        &path,
        StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "items/1"),
            parent: None,
            index: Some(0),
        },
    );
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let target = loaded.folder_key("items/0").unwrap();
            assert_eq!(folder_name(view, "items/0").as_deref(), Some("Target"));
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Folder(target))
            );
            view.open_create_folder_dialog(window, cx);
            let dialog = view.structure_dialog.as_mut().unwrap();
            assert_eq!(dialog.parent, "items/0");
            dialog.name = "Inbox".into();
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let saved = probe_opencollection::load_workspace(&path).unwrap();
    let inbox = saved.folder_key("items/0/items/1").unwrap();
    assert_eq!(
        saved
            .workspace()
            .folder(inbox)
            .unwrap()
            .metadata
            .name
            .as_deref(),
        Some("Inbox")
    );

    let identical = r#"opencollection: 1.0.0
info:
  name: Twins
bundled: true
items:
  - info: {name: Twin, type: folder}
    items: []
  - info: {name: Twin, type: folder}
    items: []
"#;
    reset_collection(window, cx, &path, identical);
    window
        .update(cx, |view, _, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
        })
        .unwrap();
    let fresh = probe_opencollection::load_workspace(&path).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            assert_eq!(view.selected_tree_item, None);
            view.open_create_folder_dialog(window, cx);
            assert!(view.structure_dialog.as_ref().unwrap().parent.is_empty());
        })
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn save_folder_sheet_explains_a_missing_destination(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let path = temp_collection("save-folder-sheet", COLLECTION);
    reset_collection(window, cx, &path, COLLECTION);
    let before = fs::read(&path).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.new_detached_request(RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.open_save_detached_request_dialog(key, window, cx);
            view.structure_dialog.as_mut().unwrap().destination_required = true;
            view.open_save_folder_dialog(window, cx);
            view.structure_dialog.as_mut().unwrap().new_folder_name = Some("Inbox".into());
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual
            .debug_bounds("save-folder-destination-required")
            .expect("the sheet explains that a destination must be selected");
        let create = visual
            .debug_bounds("save-folder-create")
            .expect("create button stays visible");
        visual.simulate_click(create.center(), Modifiers::default());
        visual.run_until_parked();
    }
    window
        .update(cx, |view, window, cx| {
            window.dispatch_action(Box::new(crate::app::SubmitSaveFolderDialog), cx);
            assert!(view.structure_task.is_none());
            let dialog = view.structure_dialog.as_ref().expect("sheet stays open");
            assert_eq!(dialog.new_folder_name.as_deref(), Some("Inbox"));
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(path).unwrap();
}

fn write_directory_collection(root: &Path) {
    fs::create_dir_all(root.join("users")).unwrap();
    fs::write(
        root.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: Directory\nbundled: false\n",
    )
    .unwrap();
    fs::write(
        root.join("users/folder.yml"),
        "info:\n  name: Users\n  type: folder\n",
    )
    .unwrap();
    fs::write(
        root.join("users/list.yml"),
        "info:\n  name: List\n  type: http\nhttp:\n  method: GET\n  url: https://example.test/users\n",
    )
    .unwrap();
}
