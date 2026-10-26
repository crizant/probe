use std::path::{Path, PathBuf};

use super::*;
use crate::app::OverviewTarget;
use probe_core::ItemKind;
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

fn temp_collection(suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "probe-desktop-reload-{suffix}-{}-{unique}.yml",
        std::process::id()
    ));
    fs::write(&path, COLLECTION).unwrap();
    path
}

fn reset_collection(
    window: gpui_kit::WindowHandle<ProbeApp>,
    cx: &mut TestAppContext,
    path: &Path,
) {
    fs::write(path, COLLECTION).unwrap();
    let loaded = probe_opencollection::load_workspace(path).unwrap();
    window
        .update(cx, |view, _, _| {
            view.session_store = None;
            view.set_workspace(path.to_path_buf(), loaded);
        })
        .unwrap();
}

fn move_target(path: &Path) -> probe_opencollection::LoadedWorkspace {
    let mut external = probe_opencollection::load_workspace(path).unwrap();
    external
        .apply_structure(StructureOperation::Move {
            target: ItemLocator::new(ItemKind::Folder, "items/1"),
            parent: None,
            index: Some(0),
        })
        .unwrap();
    external
}

fn reconcile(
    window: gpui_kit::WindowHandle<ProbeApp>,
    cx: &mut TestAppContext,
    fresh: probe_opencollection::LoadedWorkspace,
) {
    window
        .update(cx, |view, window, cx| {
            assert!(view.reconcile_filesystem_workspace(fresh, BTreeMap::new(), window, cx));
        })
        .unwrap();
}

fn folder_names(path: &Path) -> Vec<String> {
    let loaded = probe_opencollection::load_workspace(path).unwrap();
    loaded
        .folders()
        .iter()
        .filter_map(|folder| {
            loaded
                .workspace()
                .folder(folder.key())
                .and_then(|folder| folder.metadata.name.clone())
        })
        .collect()
}

fn open_app(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<ProbeApp> {
    cx.update(Theme::init);
    cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    })
}

#[gpui_kit::test]
fn external_reorder_clears_the_destination_and_does_not_write(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let path = temp_collection("destination");
    reset_collection(window, cx, &path);
    window
        .update(cx, |view, window, cx| {
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
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
        })
        .unwrap();
    let fresh = move_target(&path);
    let after_move = fs::read(&path).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().expect("form stays open");
            assert_eq!(dialog.name, "Placed");
            assert!(dialog.parent.is_empty());
            assert!(dialog.destination_required);
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), after_move);
    assert_eq!(folder_names(&path), ["Target", "Before", "After"]);

    reset_collection(window, cx, &path);
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
    let fresh = move_target(&path);
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().expect("form stays open");
            assert_eq!(dialog.name, "Inbox");
            assert!(dialog.destination_required);
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            view.structure_dialog = None;
            view.open_create_request_dialog(probe_core::RequestProtocol::Graphql, window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Query".into();
        })
        .unwrap();
    let fresh = move_target(&path);
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            let dialog = view.structure_dialog.as_ref().expect("form stays open");
            assert_eq!(dialog.name, "Query");
            assert!(dialog.destination_required);
            assert!(matches!(
                dialog.mode,
                StructureDialogMode::CreateRequest(probe_core::RequestProtocol::Graphql)
            ));
        })
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn external_reorder_cancels_delete_and_rename(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let path = temp_collection("confirm");
    reset_collection(window, cx, &path);
    window
        .update(cx, |view, window, cx| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let target = loaded.folder_key("items/1").unwrap();
            let before = loaded.folder_key("items/0").unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(target), cx);
            view.request_delete_selected(window, cx);
            view.select_tree_item(WorkspaceItemRef::Folder(before), cx);
            view.request_delete_selected(window, cx);
            view.shell.collapse_folder(target);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Delete { .. })
            ));
            assert_eq!(view.pending_application_dialogs.len(), 1);
        })
        .unwrap();
    let fresh = move_target(&path);
    let after_move = fs::read(&path).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            assert!(view.application_dialog.is_none());
            assert!(view.pending_application_dialogs.is_empty());
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "Delete “Target”"
            ));
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "Delete “Before”"
            ));
            assert_eq!(view.shell.collapsed_folders().count(), 0);
            assert!(view.selected_tree_item.is_none());
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), after_move);

    reset_collection(window, cx, &path);
    window
        .update(cx, |view, window, cx| {
            let target = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(target), cx);
            view.open_rename_dialog(window, cx);
            assert!(matches!(
                view.structure_dialog.as_ref().map(|dialog| &dialog.mode),
                Some(StructureDialogMode::Rename { .. })
            ));
        })
        .unwrap();
    let fresh = move_target(&path);
    let after_move = fs::read(&path).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.is_none());
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "Rename “Target”"
            ));
        })
        .unwrap();
    assert_eq!(folder_names(&path), ["Target", "Before", "After"]);
    assert_eq!(fs::read(&path).unwrap(), after_move);
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn external_reorder_cancels_close_other_tabs_and_keeps_unsaved_folder_text(
    cx: &mut TestAppContext,
) {
    let window = open_app(cx);
    let path = temp_collection("drafts");
    reset_collection(window, cx, &path);
    window
        .update(cx, |view, window, cx| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let before = loaded.folder_key("items/0").unwrap();
            let target = loaded.folder_key("items/1").unwrap();
            view.shell
                .open_overview(crate::shell::OverviewTab::Folder(before));
            view.shell
                .open_overview(crate::shell::OverviewTab::Folder(target));
            view.edit_overview(
                OverviewTarget::Folder("items/0".into()),
                true,
                "Before notes".into(),
                cx,
            );
            view.edit_overview(
                OverviewTarget::Folder("items/1".into()),
                true,
                "Target notes".into(),
                cx,
            );
            view.request_close_other_tabs(crate::shell::OverviewTab::Folder(before), window, cx);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Unsaved {
                    pending: PendingClose::OtherTabs { .. },
                    ..
                })
            ));
        })
        .unwrap();
    let fresh = move_target(&path);
    let after_move = fs::read(&path).unwrap();
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, window, cx| {
            assert!(view.application_dialog.is_none());
            assert!(view.pending_close.is_none());
            let notes: Vec<_> = view
                .overview_drafts
                .iter()
                .map(|(target, draft)| {
                    assert!(matches!(target, OverviewTarget::Detached(_)));
                    draft.current.docs.clone()
                })
                .collect();
            assert!(notes.contains(&Some(probe_core::Documentation::Text(
                "Before notes".into()
            ))));
            assert!(notes.contains(&Some(probe_core::Documentation::Text(
                "Target notes".into()
            ))));
            let target = view
                .overview_drafts
                .keys()
                .next()
                .expect("draft text stays")
                .clone();
            view.enqueue_documentation_save(target);
            view.pump_workspace_writes(window, cx);
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "Close Other Tabs"
            ));
            assert!(has_active_toast(
                view,
                ToastIntent::Error,
                "folders changed"
            ));
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), after_move);
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn unchanged_or_content_reload_keeps_folder_state(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let path = temp_collection("unchanged");
    reset_collection(window, cx, &path);
    window
        .update(cx, |view, window, cx| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let target = loaded.folder_key("items/1").unwrap();
            let before = loaded.folder_key("items/0").unwrap();
            view.shell.collapse_folder(before);
            view.select_tree_item(WorkspaceItemRef::Folder(target), cx);
            view.edit_overview(
                OverviewTarget::Folder("items/0".into()),
                true,
                "Before notes".into(),
                cx,
            );
            view.request_delete_selected(window, cx);
        })
        .unwrap();
    let identical = probe_opencollection::load_workspace(&path).unwrap();
    reconcile(window, cx, identical);
    window
        .update(cx, |view, _, _| {
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Delete { .. })
            ));
            assert!(
                view.overview_drafts
                    .contains_key(&OverviewTarget::Folder("items/0".into()))
            );
            assert_eq!(view.shell.collapsed_folders().count(), 1);
            assert!(matches!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Folder(_))
            ));
            assert!(!has_active_toast(view, ToastIntent::Warning, "cancelled"));
        })
        .unwrap();

    let mut source = fs::read_to_string(&path).unwrap();
    source = source.replace("https://example.test/marker", "https://example.test/edited");
    fs::write(&path, source).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.application_dialog = None;
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_save_detached_request_dialog(key, window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Kept".into();
        })
        .unwrap();
    let edited = probe_opencollection::load_workspace(&path).unwrap();
    reconcile(window, cx, edited);
    window
        .update(cx, |view, _, _| {
            let dialog = view.structure_dialog.as_ref().expect("destination stays");
            assert_eq!(dialog.parent, "items/1");
            assert!(!dialog.destination_required);
            assert!(
                view.overview_drafts
                    .contains_key(&OverviewTarget::Folder("items/0".into()))
            );
            assert_eq!(view.shell.collapsed_folders().count(), 1);
        })
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn documentation_save_matches_its_own_detached_draft(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let path = temp_collection("save-token");
    reset_collection(window, cx, &path);
    window
        .update(cx, |view, _, cx| {
            view.edit_overview(
                OverviewTarget::Folder("items/0".into()),
                true,
                "Before notes".into(),
                cx,
            );
            view.edit_overview(
                OverviewTarget::Folder("items/1".into()),
                true,
                "Target notes".into(),
                cx,
            );
            view.overview_drafts
                .get_mut(&OverviewTarget::Folder("items/1".into()))
                .unwrap()
                .save_token = Some(7);
            view.overview_drafts
                .get_mut(&OverviewTarget::Folder("items/0".into()))
                .unwrap()
                .save_token = Some(8);
        })
        .unwrap();
    let fresh = move_target(&path);
    reconcile(window, cx, fresh);
    window
        .update(cx, |view, _, _| {
            for (token, marker) in [(7, "saved-baseline"), (8, "other-baseline")] {
                view.overview_drafts
                    .values_mut()
                    .find(|draft| draft.save_token == Some(token))
                    .unwrap()
                    .original
                    .docs = Some(probe_core::Documentation::Text(marker.into()));
            }
            let submitted = view
                .overview_drafts
                .values()
                .find(|draft| draft.save_token == Some(7))
                .unwrap()
                .current
                .clone();
            view.complete_overview_draft_save(
                &OverviewTarget::Folder("items/1".into()),
                &submitted,
                7,
            );
            let other = view
                .overview_drafts
                .values()
                .find(|draft| draft.save_token == Some(8))
                .unwrap();
            assert_eq!(
                other.original.docs,
                Some(probe_core::Documentation::Text("other-baseline".into()))
            );
            let saved = view
                .overview_drafts
                .values()
                .find(|draft| draft.save_token == Some(7))
                .unwrap();
            assert_ne!(
                saved.original.docs,
                Some(probe_core::Documentation::Text("saved-baseline".into()))
            );
        })
        .unwrap();
    fs::remove_file(path).unwrap();
}
