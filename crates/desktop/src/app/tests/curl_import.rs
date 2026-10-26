use super::*;
use crate::app::{DesktopMenuItem, ImportCurl};

const COMMAND: &str = "curl -X POST 'https://example.test/import?q=1' -H 'Authorization: Bearer example' -b 'session=example' --data-raw 'payload'";

fn import_window(
    cx: &mut TestAppContext,
    suffix: &str,
) -> (gpui_kit::WindowHandle<ProbeApp>, PathBuf) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let path = writable_structure_fixture(suffix);
    let loaded = probe_opencollection::load_workspace(&path).unwrap();
    let window = cx.open_window(size(px(900.0), px(800.0)), ProbeApp::new);
    window
        .update(cx, |view, _, _| {
            view.session_store = None;
            view.set_workspace(path.clone(), loaded);
        })
        .unwrap();
    cx.run_until_parked();
    (window, path)
}

fn fill_import(window: gpui_kit::WindowHandle<ProbeApp>, cx: &mut TestAppContext, command: &str) {
    window
        .update(cx, |view, window, cx| {
            view.open_curl_import_dialog(window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Imported".into();
            view.edit_curl_command(command.into(), cx);
            assert!(view.curl_import_disabled(), "wait for background preview");
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn curl_import_validation_and_cancel_preserve_collection(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-cancel");
    let before = fs::read(&path).unwrap();
    fill_import(
        window,
        cx,
        "curl https://example.test; touch /tmp/should-never-run",
    );
    window
        .update(cx, |view, window, cx| {
            assert!(view.curl_import_disabled());
            assert!(matches!(
                view.structure_dialog
                    .as_ref()
                    .unwrap()
                    .curl
                    .as_ref()
                    .unwrap()
                    .parsed,
                Some(Err(_))
            ));
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            assert!(view.structure_dialog.is_some());
            view.edit_curl_command(COMMAND.into(), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(!view.curl_import_disabled());
            view.structure_dialog.as_mut().unwrap().name = " ".into();
            assert!(view.curl_import_disabled());
            view.structure_dialog_focus.focus(window, cx);
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.is_none());
            assert!(view.structure_task.is_none());
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_selects_folder_persists_and_preserves_existing_drafts(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-success");
    window
        .update(cx, |view, window, cx| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let key = loaded.requests()[0].key();
            let folder = loaded.folder_key("items/1").unwrap();
            view.select_request(key, cx);
            view.request_editor.set_section(key, EditorSection::Headers);
            view.loaded_workspace
                .as_mut()
                .unwrap()
                .request_mut(key)
                .unwrap()
                .url = Some("https://draft.test/unsaved".into());
            view.persistence.edited(key);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let detached = view.shell.active_tab().unwrap();
            view.loaded_workspace
                .as_mut()
                .unwrap()
                .request_mut(detached)
                .unwrap()
                .url = Some("https://detached.test".into());
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
        })
        .unwrap();
    fill_import(window, cx, COMMAND);
    cx.simulate_keystrokes(window.into(), super::save_shortcut());
    cx.simulate_keystrokes(
        window.into(),
        if cfg!(target_os = "macos") {
            "cmd-w"
        } else {
            "ctrl-w"
        },
    );
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "items/1");
            view.structure_dialog_focus.focus(window, cx);
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let command = visual.debug_bounds("curl-command-input").unwrap();
    visual.simulate_click(command.center(), Modifiers::default());
    drop(visual);
    cx.simulate_keystrokes(
        window.into(),
        if cfg!(target_os = "macos") {
            "cmd-enter"
        } else {
            "ctrl-enter"
        },
    );
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let active = view.shell.active_tab().unwrap();
            let request = loaded.workspace().request(active).unwrap();
            assert_eq!(request.metadata.name.as_deref(), Some("Imported"));
            assert_eq!(request.method.as_deref(), Some("POST"));
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Request(active))
            );
            assert!(
                loaded
                    .request_selector(active)
                    .unwrap()
                    .starts_with("items/1/items/")
            );
            let original = loaded.request_key("items/0").unwrap();
            assert_eq!(
                loaded.workspace().request(original).unwrap().url.as_deref(),
                Some("https://draft.test/unsaved")
            );
            assert!(view.request_is_dirty(original));
            assert_eq!(
                view.request_editor.section(original),
                EditorSection::Headers
            );
            assert!(view.detached_requests.iter().any(|key| {
                loaded
                    .workspace()
                    .request(*key)
                    .is_some_and(|request| request.url.as_deref() == Some("https://detached.test"))
            }));
        })
        .unwrap();
    let disk = probe_opencollection::load_workspace(&path).unwrap();
    let imported = disk
        .requests()
        .iter()
        .find_map(|located| {
            let request = disk.workspace().request(located.key())?;
            (request.metadata.name.as_deref() == Some("Imported")).then_some(request)
        })
        .unwrap();
    let parsed = probe_application::parse_curl(COMMAND).unwrap();
    assert_eq!(imported.headers, parsed.headers);
    assert_eq!(imported.http_body(), parsed.http_body());
    assert_eq!(imported.settings, parsed.settings);
    assert_ne!(
        disk.workspace()
            .request(disk.request_key("items/0").unwrap())
            .unwrap()
            .url
            .as_deref(),
        Some("https://draft.test/unsaved")
    );
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_write_failure_retains_input_and_retries_after_reload(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-conflict");
    fill_import(window, cx, COMMAND);
    let mut external = probe_opencollection::load_workspace(&path).unwrap();
    external
        .apply_structure(probe_opencollection::StructureOperation::CreateFolder {
            parent: None,
            index: None,
            name: "External".into(),
        })
        .unwrap();
    window
        .update(cx, |view, window, cx| {
            view.submit_structure_dialog(window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.name, "Imported");
            let curl = dialog.curl.as_ref().unwrap();
            assert_eq!(curl.command, COMMAND);
            assert!(
                curl.error
                    .as_deref()
                    .unwrap()
                    .contains("externally modified")
            );
            assert!(!curl.committed);
            assert!(view.reconcile_filesystem_workspace(
                probe_opencollection::load_workspace(&path).unwrap(),
                BTreeMap::new(),
                window,
                cx
            ));
            assert_eq!(
                view.structure_dialog
                    .as_ref()
                    .unwrap()
                    .curl
                    .as_ref()
                    .unwrap()
                    .command,
                COMMAND
            );
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.is_none(), "{:?}", toast_debug(view))
        })
        .unwrap();
    let disk = probe_opencollection::load_workspace(&path).unwrap();
    assert!(disk.folders().iter().any(|folder| {
        disk.workspace()
            .folder(folder.key())
            .unwrap()
            .metadata
            .name
            .as_deref()
            == Some("External")
    }));
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_blocks_active_save_and_reload_and_ignores_obsolete_parse(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-races");
    fill_import(window, cx, COMMAND);
    let before = fs::read(&path).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.request_save_task =
                Some(cx.spawn(async |_, _| std::future::pending::<()>().await));
            assert!(view.curl_import_disabled());
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            assert_eq!(view.structure_dialog.as_ref().unwrap().name, "Imported");
            view.request_save_task = None;
            view.loading = true;
            assert!(view.curl_import_disabled());
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            view.loading = false;
            view.edit_curl_command("curl https://stale.test".into(), cx);
            view.open_curl_import_dialog(window, cx);
            view.edit_curl_command("not curl".into(), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.curl_import_disabled());
            let curl = view
                .structure_dialog
                .as_ref()
                .unwrap()
                .curl
                .as_ref()
                .unwrap();
            assert_eq!(curl.command, "not curl");
            assert!(matches!(curl.parsed, Some(Err(_))));
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_file_menu_opens_dialog_for_an_open_collection(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-file-menu");
    window
        .update(cx, |view, window, cx| {
            let file_menu = view.desktop_menu_definition(DesktopMenu::File);
            let action = file_menu
                .items
                .into_iter()
                .find_map(|item| match item {
                    DesktopMenuItem::Submenu("Import From…", _, submenu) => {
                        submenu.items.into_iter().find_map(|item| match item {
                            DesktopMenuItem::Action("cURL…", action, _) => Some(action),
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .expect("File > Import From includes cURL");
            window.dispatch_action(action, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.as_ref().unwrap().curl.is_some());
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(view.structure_dialog.is_none());
            view.loaded_workspace = None;
            window.dispatch_action(Box::new(ImportCurl), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| assert!(view.structure_dialog.is_none()))
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_add_menu_and_multiline_keyboard(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-menu");
    window
        .update(cx, |view, _, cx| {
            view.selected_tree_item = None;
            view.transient.structure_add_menu_open = true;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let item = visual
        .debug_bounds("tree-import-curl")
        .expect("Add menu includes cURL import");
    visual.simulate_click(item.center(), Modifiers::default());
    drop(visual);
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(view.structure_dialog.as_ref().unwrap().parent, "");
            assert!(!view.transient.structure_add_menu_open);
            assert!(view.curl_import_disabled());
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let command = visual
        .debug_bounds("curl-command-input")
        .expect("multiline input renders");
    visual.simulate_click(command.center(), Modifiers::default());
    visual.simulate_input("curl \\");
    visual.simulate_keystrokes("enter");
    visual.simulate_input("https://example.test");
    drop(visual);
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert!(dialog.curl.as_ref().unwrap().command.contains('\n'));
            assert!(matches!(dialog.curl.as_ref().unwrap().parsed, Some(Ok(_))));
            assert!(
                view.structure_task.is_none(),
                "Enter edits instead of importing"
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| assert!(view.structure_dialog.is_none()))
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_protects_documentation_and_committed_refresh_failure(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-protection");
    fill_import(window, cx, COMMAND);
    let before = fs::read(&path).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.edit_overview(
                crate::app::documentation::OverviewTarget::Collection,
                true,
                "Unsaved documentation".into(),
                cx,
            );
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            assert!(view.has_dirty_overviews());
            assert!(
                view.structure_dialog
                    .as_ref()
                    .unwrap()
                    .curl
                    .as_ref()
                    .unwrap()
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("documentation")
            );
            let curl = view
                .structure_dialog
                .as_mut()
                .unwrap()
                .curl
                .as_mut()
                .unwrap();
            curl.committed = true;
            assert!(view.curl_import_disabled());
            view.submit_structure_dialog(window, cx);
            assert!(
                view.structure_task.is_none(),
                "a committed import cannot be duplicated"
            );
            view.edit_curl_command("curl https://different.test".into(), cx);
            assert_eq!(
                view.structure_dialog
                    .as_ref()
                    .unwrap()
                    .curl
                    .as_ref()
                    .unwrap()
                    .command,
                COMMAND
            );
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_keeps_its_lock_when_about_opens_and_defers_reload_and_close(
    cx: &mut TestAppContext,
) {
    let (window, path) = import_window(cx, "curl-reload-race");
    fill_import(window, cx, COMMAND);
    window
        .update(cx, |view, window, cx| {
            let fresh = probe_opencollection::load_workspace(&path).unwrap();
            let baseline = view.loaded_workspace.as_ref().unwrap().baseline();
            view.submit_structure_dialog(window, cx);
            assert!(view.curl_import_in_progress());
            // AboutProbe's global handler calls this shared entry point.
            view.show_application_dialog(ApplicationDialog::About, window, cx);
            assert!(view.application_dialog.is_none());
            assert!(
                view.curl_import_in_progress(),
                "About must not remove the import lock"
            );
            assert_eq!(
                view.structure_dialog
                    .as_ref()
                    .unwrap()
                    .curl
                    .as_ref()
                    .unwrap()
                    .command,
                COMMAND
            );
            assert!(!view.apply_watched_workspace(
                baseline,
                Ok(fresh),
                BTreeMap::new(),
                window,
                cx
            ));
            assert!(!view.request_close_window(window, cx));
            view.request_load_workspace(bundled_fixture(), None, window, cx);
            assert_eq!(view.workspace_path.as_ref(), Some(&path));
            assert!(view.structure_dialog.is_some());
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(view.workspace_path.as_ref(), Some(&path));
            assert!(view.structure_dialog.is_none(), "{:?}", toast_debug(view));
            assert_eq!(
                view.loaded_workspace
                    .as_ref()
                    .unwrap()
                    .workspace()
                    .request(view.shell.active_tab().unwrap())
                    .unwrap()
                    .metadata
                    .name
                    .as_deref(),
                Some("Imported")
            );
        })
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_resumes_saves_queued_during_atomic_creation(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-queued-save");
    fill_import(window, cx, COMMAND);
    window
        .update(cx, |view, window, cx| {
            let key = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .request_key("items/0")
                .unwrap();
            view.loaded_workspace
                .as_mut()
                .unwrap()
                .request_mut(key)
                .unwrap()
                .url = Some("https://queued-save.test".into());
            view.persistence.edited(key);
            view.submit_structure_dialog(window, cx);
            view.persistence.enqueue([key]);
            view.pump_workspace_writes(window, cx);
            assert!(
                view.request_save_task.is_none(),
                "import owns the write slot"
            );
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.is_none(), "{:?}", toast_debug(view));
            assert!(!view.persistence.has_outstanding_saves());
            assert!(!view.has_active_workspace_write());
            let key = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .request_key("items/0")
                .unwrap();
            assert!(!view.request_is_dirty(key));
        })
        .unwrap();
    let disk = probe_opencollection::load_workspace(&path).unwrap();
    assert_eq!(
        disk.workspace()
            .request(disk.request_key("items/0").unwrap())
            .unwrap()
            .url
            .as_deref(),
        Some("https://queued-save.test")
    );
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_disambiguates_same_named_destination_folders(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-folder-labels");
    fs::write(
        &path,
        r#"opencollection: 1.0.0
info:
  name: Destinations
bundled: true
items:
  - info: {name: Auth, type: folder}
    items:
      - info: {name: Examples, type: folder}
        items: []
  - info: {name: Payments, type: folder}
    items:
      - info: {name: Examples, type: folder}
        items: []
"#,
    )
    .unwrap();
    window
        .update(cx, |view, window, cx| {
            view.set_workspace(
                path.clone(),
                probe_opencollection::load_workspace(&path).unwrap(),
            );
            view.open_curl_import_dialog(window, cx);
            let choices = view.curl_import_destinations();
            let auth = choices
                .iter()
                .find(|(selector, _)| selector == "items/0/items/0")
                .unwrap();
            let payments = choices
                .iter()
                .find(|(selector, _)| selector == "items/1/items/0")
                .unwrap();
            assert!(auth.1.contains("Examples"));
            assert!(payments.1.contains("Examples"));
            assert_ne!(
                auth.1, payments.1,
                "same-named folders must be distinguishable"
            );
            assert!(auth.1.contains(&auth.0));
            assert!(payments.1.contains(&payments.0));
        })
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn curl_import_removed_destination_requires_explicit_reselection(cx: &mut TestAppContext) {
    let (window, path) = import_window(cx, "curl-removed-destination");
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
    fill_import(window, cx, COMMAND);
    let mut external = probe_opencollection::load_workspace(&path).unwrap();
    external
        .apply_structure(probe_opencollection::StructureOperation::Delete {
            target: probe_opencollection::ItemLocator {
                kind: probe_core::ItemKind::Folder,
                selector: "items/1".into(),
            },
        })
        .unwrap();
    let before = fs::read(&path).unwrap();
    window
        .update(cx, |view, window, cx| {
            assert!(view.reconcile_filesystem_workspace(external, BTreeMap::new(), window, cx));
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "items/1", "do not silently select the root");
            assert!(dialog.curl.as_ref().unwrap().destination_required);
            assert!(view.curl_import_disabled());
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
            // Correcting command/name validation must not acknowledge a missing folder.
            view.structure_dialog.as_mut().unwrap().name = "Reselected".into();
            view.edit_curl_command("curl https://reselected.test".into(), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(view.curl_import_disabled());
            view.submit_structure_dialog(window, cx);
            assert!(view.structure_task.is_none());
        })
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);

    // Explicitly choosing Collection root is permitted, even though its selector
    // is empty. Exercise the actual dropdown callback rather than clearing a flag.
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let destination = visual.debug_bounds("curl-parent-trigger").unwrap();
    visual.simulate_click(destination.center(), Modifiers::default());
    drop(visual);
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.parent, "");
            assert!(!dialog.curl.as_ref().unwrap().destination_required);
            assert!(!view.curl_import_disabled());
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.is_none(), "{:?}", toast_debug(view))
        })
        .unwrap();
    let disk = probe_opencollection::load_workspace(&path).unwrap();
    let imported = disk
        .workspace()
        .request(disk.request_key("items/1").unwrap())
        .unwrap();
    assert_eq!(imported.metadata.name.as_deref(), Some("Reselected"));
    assert_eq!(imported.url.as_deref(), Some("https://reselected.test"));
    fs::remove_file(path).unwrap();
}
