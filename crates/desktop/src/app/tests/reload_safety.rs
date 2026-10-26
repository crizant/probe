use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::app::OverviewTarget;
use probe_core::{ItemKind, RequestKey};
use probe_opencollection::{ItemLocator, StructureOperation};

const COLLECTION: &str = "opencollection: 1.0.0\ninfo: {name: Destinations}\nbundled: true\nitems:\n- {info: {name: Before, type: folder}, items: []}\n- {info: {name: Target, type: folder}, items: [{info: {name: Marker, type: http}, http: {method: GET, url: https://example.test/marker}}]}\n- {info: {name: After, type: folder}, items: []}\n";
const REQUESTS: &str = "opencollection: 1.0.0\ninfo: {name: Requests}\nbundled: true\nitems:\n- {info: {name: Alpha, type: http}, http: {method: GET, url: https://example.test/alpha}}\n- {info: {name: Beta, type: http}, http: {method: GET, url: https://example.test/beta}}\n";
const TWINS: &str = "opencollection: 1.0.0\ninfo: {name: Twins}\nbundled: true\nitems:\n- {info: {name: Same, type: folder}, items: []}\n- {info: {name: Same, type: folder}, items: [{info: {name: Marker, type: http}, http: {method: GET, url: https://example.test/marker}}]}\n";
const DUPLICATES: &str = "opencollection: 1.0.0\ninfo: {name: Duplicates}\nbundled: true\nitems:\n- {info: {name: Dup, type: http}, http: {method: GET, url: https://example.test/one}}\n- {info: {name: Dup, type: http}, http: {method: GET, url: https://example.test/two}}\n";
const TWIN_FOLDERS: &str = "opencollection: 1.0.0\ninfo: {name: Twin folders}\nbundled: true\nitems:\n- {info: {name: Same, type: folder}, items: [{info: {name: Child, type: http}, http: {method: GET, url: https://example.test/one}}]}\n- {info: {name: Same, type: folder}, items: [{info: {name: Child, type: http}, http: {method: GET, url: https://example.test/two}}]}\n";
const NESTED_DUPS: &str = "opencollection: 1.0.0\ninfo: {name: Nested}\nbundled: true\nitems:\n- {info: {name: Group, type: folder}, items: [{info: {name: Dup, type: http}, http: {method: GET, url: https://example.test/one}}, {info: {name: Dup, type: http}, http: {method: GET, url: https://example.test/two}}]}\n";
const NESTED_SWAPPED: &str = "opencollection: 1.0.0\ninfo: {name: Nested}\nbundled: true\nitems:\n- {info: {name: Group, type: folder}, items: [{info: {name: Dup, type: http}, http: {method: GET, url: https://example.test/two}}, {info: {name: Dup, type: http}, http: {method: GET, url: https://example.test/oneX}}]}\n";
const NESTED_PARENTS: &str = "opencollection: 1.0.0\ninfo: {name: Parents}\nbundled: true\nitems:\n- {info: {name: Same, type: folder}, items: [{info: {name: Child, type: folder}, items: [{info: {name: One, type: http}, http: {method: GET, url: https://example.test/one}}]}]}\n- {info: {name: Same, type: folder}, items: [{info: {name: Child, type: folder}, items: [{info: {name: One, type: http}, http: {method: GET, url: https://example.test/two}}]}]}\n";

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

struct Workspace {
    window: gpui_kit::WindowHandle<ProbeApp>,
    path: PathBuf,
}

impl Workspace {
    fn open(cx: &mut TestAppContext, suffix: &str, source: &str) -> Self {
        cx.update(Theme::init);
        let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let path = std::env::temp_dir().join(format!(
            "probe-desktop-reload-{suffix}-{}-{}.yml",
            std::process::id(),
            next_id()
        ));
        let workspace = Self { window, path };
        workspace.reset(cx, source);
        workspace
    }

    fn reset(&self, cx: &mut TestAppContext, source: &str) {
        fs::write(&self.path, source).unwrap();
        let loaded = probe_opencollection::load_workspace(&self.path).unwrap();
        self.update(cx, |view, _, _| {
            view.session_store = None;
            view.set_workspace(self.path.clone(), loaded);
        });
    }

    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        update: impl FnOnce(&mut ProbeApp, &mut gpui_kit::Window, &mut gpui_kit::Context<ProbeApp>) -> R,
    ) -> R {
        self.window.update(cx, update).unwrap()
    }

    fn select(&self, cx: &mut TestAppContext, kind: ItemKind, selector: &str) {
        self.update(cx, |view, _, cx| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let item = match kind {
                ItemKind::Folder => WorkspaceItemRef::Folder(loaded.folder_key(selector).unwrap()),
                ItemKind::Request => {
                    WorkspaceItemRef::Request(loaded.request_key(selector).unwrap())
                }
            };
            view.select_tree_item(item, cx);
        });
    }

    fn delete(&self, cx: &mut TestAppContext) {
        self.update(cx, |view, window, cx| {
            view.request_delete_selected(window, cx)
        });
    }

    fn rename(&self, cx: &mut TestAppContext, name: Option<&str>) {
        self.update(cx, |view, window, cx| {
            view.open_rename_dialog(window, cx);
            if let Some(name) = name {
                view.structure_dialog.as_mut().unwrap().name = name.into();
            }
        });
    }

    fn save_into(&self, cx: &mut TestAppContext, folder: &str, name: Option<&str>) {
        self.update(cx, |view, window, cx| {
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key(folder)
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_save_detached_request_dialog(key, window, cx);
            if let Some(name) = name {
                view.structure_dialog.as_mut().unwrap().name = name.into();
            }
        });
    }

    fn create_named(
        &self,
        cx: &mut TestAppContext,
        protocol: Option<probe_core::RequestProtocol>,
        name: &str,
    ) {
        self.update(cx, |view, window, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            match protocol {
                Some(protocol) => view.open_create_request_dialog(protocol, window, cx),
                None => view.open_create_folder_dialog(window, cx),
            }
            view.structure_dialog.as_mut().unwrap().name = name.into();
        });
    }

    fn edit_docs(&self, cx: &mut TestAppContext, selector: &str, text: &str) {
        self.update(cx, |view, _, cx| {
            view.edit_overview(
                OverviewTarget::Folder(selector.into()),
                true,
                text.into(),
                cx,
            );
        });
    }

    fn reconcile(&self, cx: &mut TestAppContext, fresh: probe_opencollection::LoadedWorkspace) {
        self.update(cx, |view, window, cx| {
            assert!(view.reconcile_filesystem_workspace(fresh, BTreeMap::new(), window, cx));
        });
    }

    /// External move, then reload. Returned bytes are the disk image before confirm.
    fn reconcile_move(
        &self,
        cx: &mut TestAppContext,
        kind: ItemKind,
        selector: &str,
        index: usize,
    ) -> Vec<u8> {
        let mut external = probe_opencollection::load_workspace(&self.path).unwrap();
        external
            .apply_structure(StructureOperation::Move {
                target: ItemLocator::new(kind, selector),
                parent: None,
                index: Some(index),
            })
            .unwrap();
        let disk = fs::read(&self.path).unwrap();
        self.reconcile(cx, external);
        disk
    }

    fn reconcile_text(&self, cx: &mut TestAppContext, source: &str) -> Vec<u8> {
        fs::write(&self.path, source).unwrap();
        let fresh = probe_opencollection::load_workspace(&self.path).unwrap();
        let disk = fs::read(&self.path).unwrap();
        self.reconcile(cx, fresh);
        disk
    }

    fn reconcile_same(&self, cx: &mut TestAppContext) {
        self.reconcile(
            cx,
            probe_opencollection::load_workspace(&self.path).unwrap(),
        );
    }

    fn confirm_delete(&self, cx: &mut TestAppContext) {
        self.update(cx, |view, window, cx| {
            view.handle_application_dialog_action(ApplicationDialogAction::Delete, window, cx);
        });
    }

    fn submit(&self, cx: &mut TestAppContext) {
        self.update(cx, |view, window, cx| {
            view.submit_structure_dialog(window, cx)
        });
    }

    fn bytes(&self) -> Vec<u8> {
        fs::read(&self.path).unwrap()
    }

    fn toast(&self, cx: &mut TestAppContext, intent: ToastIntent, text: &str) -> bool {
        self.update(cx, |view, _, _| has_active_toast(view, intent, text))
    }

    fn urls(&self) -> Vec<String> {
        let mut urls = self
            .name_url()
            .into_iter()
            .map(|(_, url)| url)
            .collect::<Vec<_>>();
        urls.sort();
        urls
    }

    fn names(&self, folders: bool) -> Vec<String> {
        let loaded = probe_opencollection::load_workspace(&self.path).unwrap();
        let workspace = loaded.workspace();
        if folders {
            loaded
                .folders()
                .iter()
                .filter_map(|item| {
                    workspace
                        .folder(item.key())
                        .and_then(|folder| folder.metadata.name.clone())
                })
                .collect()
        } else {
            loaded
                .requests()
                .iter()
                .filter_map(|item| {
                    workspace
                        .request(item.key())
                        .and_then(|request| request.metadata.name.clone())
                })
                .collect()
        }
    }

    fn name_url(&self) -> Vec<(String, String)> {
        let loaded = probe_opencollection::load_workspace(&self.path).unwrap();
        loaded
            .requests()
            .iter()
            .filter_map(|item| {
                let request = loaded.workspace().request(item.key())?;
                Some((
                    request.metadata.name.clone().unwrap_or_default(),
                    request.url.clone().unwrap_or_default(),
                ))
            })
            .collect()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

enum Pending {
    Delete,
    Rename(Option<&'static str>),
}

enum Change {
    MoveTo(usize),
    Replace(&'static str),
}

struct RequestCase {
    label: &'static str,
    source: &'static str,
    selector: &'static str,
    pending: Pending,
    change: Change,
    cancelled: bool,
    urls: Option<&'static [&'static str]>,
    names: Option<&'static [&'static str]>,
    pairs: &'static [(&'static str, &'static str)],
}

fn req(
    label: &'static str,
    source: &'static str,
    selector: &'static str,
    pending: Pending,
    change: Change,
) -> RequestCase {
    RequestCase {
        label,
        source,
        selector,
        pending,
        change,
        cancelled: false,
        urls: None,
        names: None,
        pairs: &[],
    }
}

impl RequestCase {
    fn cancelled(mut self, names: &'static [&'static str]) -> Self {
        self.cancelled = true;
        self.names = Some(names);
        self
    }

    fn urls(mut self, urls: &'static [&'static str]) -> Self {
        self.urls = Some(urls);
        self
    }

    fn pairs(mut self, pairs: &'static [(&'static str, &'static str)]) -> Self {
        self.pairs = pairs;
        self
    }
}

/// Open a request Delete or Rename, apply one external change, confirm when the
/// dialog should survive, and check the collection on disk.
fn confirm_request(cx: &mut TestAppContext, case: &RequestCase) {
    let workspace = Workspace::open(cx, case.label, case.source);
    workspace.select(cx, ItemKind::Request, case.selector);
    let rename = match case.pending {
        Pending::Delete => {
            workspace.delete(cx);
            false
        }
        Pending::Rename(name) => {
            workspace.rename(cx, name);
            true
        }
    };
    let disk = match case.change {
        Change::MoveTo(index) => {
            workspace.reconcile_move(cx, ItemKind::Request, case.selector, index)
        }
        Change::Replace(source) => workspace.reconcile_text(cx, source),
    };
    let open = workspace.update(cx, |view, _, _| match rename {
        true => matches!(
            view.structure_dialog.as_ref().map(|dialog| &dialog.mode),
            Some(StructureDialogMode::Rename { .. })
        ),
        false => matches!(
            view.application_dialog,
            Some(ApplicationDialog::Delete { .. })
        ),
    });
    assert_eq!(open, !case.cancelled, "{}", case.label);
    if case.cancelled {
        assert_eq!(workspace.bytes(), disk, "{}", case.label);
    } else if rename {
        workspace.submit(cx);
        cx.run_until_parked();
    } else {
        workspace.confirm_delete(cx);
        cx.run_until_parked();
    }
    if let Some(urls) = case.urls {
        assert_eq!(workspace.urls(), urls, "{}", case.label);
    }
    if let Some(names) = case.names {
        assert_eq!(workspace.names(false), names, "{}", case.label);
    }
    let rows = workspace.name_url();
    for pair in case.pairs {
        assert!(
            rows.contains(&(pair.0.into(), pair.1.into())),
            "{} missing {pair:?} in {rows:?}",
            case.label
        );
    }
}

fn move_folder(workspace: &Workspace, cx: &mut TestAppContext) -> Vec<u8> {
    workspace.reconcile_move(cx, ItemKind::Folder, "items/1", 0)
}

fn note(text: &str) -> probe_core::Documentation {
    probe_core::Documentation::Text(text.into())
}

#[gpui_kit::test]
fn external_reorder_clears_the_destination_and_does_not_write(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "destination", COLLECTION);
    workspace.save_into(cx, "items/1", Some("Placed"));
    let disk = move_folder(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        let dialog = view.structure_dialog.as_ref().expect("form stays open");
        assert_eq!(dialog.name, "Placed");
        assert!(dialog.parent.is_empty() && dialog.destination_required);
        view.submit_structure_dialog(window, cx);
        assert!(view.structure_task.is_none());
    });
    assert_eq!(workspace.bytes(), disk);
    assert_eq!(workspace.names(true), ["Target", "Before", "After"]);

    workspace.reset(cx, COLLECTION);
    workspace.create_named(cx, None, "Inbox");
    move_folder(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        let dialog = view.structure_dialog.as_ref().expect("form stays open");
        assert_eq!(dialog.name, "Inbox");
        assert!(dialog.destination_required);
        view.submit_structure_dialog(window, cx);
        assert!(view.structure_task.is_none());
        view.structure_dialog = None;
    });
    workspace.create_named(cx, Some(probe_core::RequestProtocol::Graphql), "Query");
    move_folder(&workspace, cx);
    workspace.update(cx, |view, _, _| {
        let dialog = view.structure_dialog.as_ref().expect("form stays open");
        assert_eq!(dialog.name, "Query");
        assert!(dialog.destination_required);
        assert!(matches!(
            dialog.mode,
            StructureDialogMode::CreateRequest(probe_core::RequestProtocol::Graphql)
        ));
    });
}

#[gpui_kit::test]
fn external_reorder_cancels_delete_and_rename(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "confirm", COLLECTION);
    workspace.update(cx, |view, window, cx| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let target = loaded.folder_key("items/1").unwrap();
        let before = loaded.folder_key("items/0").unwrap();
        view.select_tree_item(WorkspaceItemRef::Folder(target), cx);
        view.request_delete_selected(window, cx);
        view.select_tree_item(WorkspaceItemRef::Folder(before), cx);
        view.request_delete_selected(window, cx);
        view.shell.collapse_folder(target);
    });
    let disk = move_folder(&workspace, cx);
    workspace.update(cx, |view, _, _| {
        assert!(view.application_dialog.is_none());
        assert!(view.pending_application_dialogs.is_empty());
        assert_eq!(view.shell.collapsed_folders().count(), 0);
        assert!(view.selected_tree_item.is_none());
    });
    assert!(workspace.toast(cx, ToastIntent::Warning, "Delete “Target”"));
    assert!(workspace.toast(cx, ToastIntent::Warning, "Delete “Before”"));
    assert_eq!(workspace.bytes(), disk);

    workspace.reset(cx, COLLECTION);
    workspace.select(cx, ItemKind::Folder, "items/1");
    workspace.rename(cx, None);
    let disk = move_folder(&workspace, cx);
    assert!(workspace.update(cx, |view, _, _| view.structure_dialog.is_none()));
    assert!(workspace.toast(cx, ToastIntent::Warning, "Rename “Target”"));
    assert_eq!(workspace.names(true), ["Target", "Before", "After"]);
    assert_eq!(workspace.bytes(), disk);
}

#[gpui_kit::test]
fn external_reorder_cancels_close_other_tabs_and_keeps_unsaved_folder_text(
    cx: &mut TestAppContext,
) {
    let workspace = Workspace::open(cx, "drafts", COLLECTION);
    workspace.update(cx, |view, window, cx| {
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
    });
    let disk = move_folder(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        assert!(view.application_dialog.is_none() && view.pending_close.is_none());
        let notes: Vec<_> = view
            .overview_drafts
            .iter()
            .map(|(target, draft)| {
                assert!(matches!(target, OverviewTarget::Detached(_)));
                draft.current.docs.clone()
            })
            .collect();
        assert!(notes.contains(&Some(note("Before notes"))));
        assert!(notes.contains(&Some(note("Target notes"))));
        view.enqueue_documentation_save(view.overview_drafts.keys().next().unwrap().clone());
        view.pump_workspace_writes(window, cx);
    });
    assert!(workspace.toast(cx, ToastIntent::Warning, "Close Other Tabs"));
    assert!(workspace.toast(cx, ToastIntent::Error, "folders changed"));
    assert_eq!(workspace.bytes(), disk);
}

#[gpui_kit::test]
fn unchanged_or_content_reload_keeps_folder_state(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "unchanged", COLLECTION);
    workspace.update(cx, |view, window, cx| {
        let loaded = view.loaded_workspace.as_ref().unwrap();
        let target = loaded.folder_key("items/1").unwrap();
        view.shell
            .collapse_folder(loaded.folder_key("items/0").unwrap());
        view.select_tree_item(WorkspaceItemRef::Folder(target), cx);
        view.edit_overview(
            OverviewTarget::Folder("items/0".into()),
            true,
            "Before notes".into(),
            cx,
        );
        view.request_delete_selected(window, cx);
    });
    workspace.reconcile_same(cx);
    workspace.update(cx, |view, _, _| {
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
    });

    workspace.update(cx, |view, _, _| view.application_dialog = None);
    let source = fs::read_to_string(&workspace.path)
        .unwrap()
        .replace("https://example.test/marker", "https://example.test/edited");
    workspace.save_into(cx, "items/1", Some("Kept"));
    workspace.reconcile_text(cx, &source);
    workspace.update(cx, |view, _, _| {
        let dialog = view.structure_dialog.as_ref().expect("form stays open");
        assert!(dialog.parent.is_empty() && dialog.destination_required);
        assert!(
            view.overview_drafts
                .keys()
                .any(|target| matches!(target, OverviewTarget::Detached(_)))
        );
        assert!(
            !view
                .overview_drafts
                .contains_key(&OverviewTarget::Folder("items/0".into()))
        );
        assert!(view.selected_tree_item.is_none());
        assert_eq!(view.shell.collapsed_folders().count(), 1);
    });
}

#[gpui_kit::test]
fn documentation_save_matches_its_own_detached_draft(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "save-token", COLLECTION);
    workspace.edit_docs(cx, "items/0", "Before notes");
    workspace.edit_docs(cx, "items/1", "Target notes");
    workspace.update(cx, |view, _, _| {
        view.overview_drafts
            .get_mut(&OverviewTarget::Folder("items/1".into()))
            .unwrap()
            .save_token = Some(7);
        view.overview_drafts
            .get_mut(&OverviewTarget::Folder("items/0".into()))
            .unwrap()
            .save_token = Some(8);
    });
    move_folder(&workspace, cx);
    workspace.update(cx, |view, _, _| {
        for (token, marker) in [(7, "saved-baseline"), (8, "other-baseline")] {
            view.overview_drafts
                .values_mut()
                .find(|draft| draft.save_token == Some(token))
                .unwrap()
                .original
                .docs = Some(note(marker));
        }
        let submitted = view
            .overview_drafts
            .values()
            .find(|draft| draft.save_token == Some(7))
            .unwrap()
            .current
            .clone();
        view.complete_overview_draft_save(&OverviewTarget::Folder("items/1".into()), &submitted, 7);
        assert_eq!(
            view.overview_drafts
                .values()
                .find(|draft| draft.save_token == Some(8))
                .unwrap()
                .original
                .docs,
            Some(note("other-baseline"))
        );
        assert!(
            view.overview_drafts
                .values()
                .all(|draft| draft.save_token != Some(7))
        );
    });
}

#[gpui_kit::test]
fn request_reorder_does_not_delete_or_rename_the_other_request(cx: &mut TestAppContext) {
    let names = &["Beta", "Alpha"][..];
    for case in [
        req(
            "reorder-delete",
            REQUESTS,
            "items/0",
            Pending::Delete,
            Change::MoveTo(1),
        )
        .cancelled(names),
        req(
            "reorder-rename",
            REQUESTS,
            "items/0",
            Pending::Rename(None),
            Change::MoveTo(1),
        )
        .cancelled(names),
    ] {
        confirm_request(cx, &case);
    }
}

#[gpui_kit::test]
fn same_name_folder_swap_does_not_touch_the_other_folder(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "twins", TWINS);
    workspace.select(cx, ItemKind::Folder, "items/0");
    workspace.delete(cx);
    let disk = move_folder(&workspace, cx);
    assert!(workspace.update(cx, |view, _, _| view.application_dialog.is_none()));
    assert!(workspace.toast(cx, ToastIntent::Warning, "Delete “Same”"));
    assert_eq!(workspace.names(false), ["Marker"]);
    assert_eq!(workspace.names(true), ["Same", "Same"]);
    assert_eq!(workspace.bytes(), disk);

    workspace.reset(cx, TWINS);
    workspace.save_into(cx, "items/0", None);
    workspace.edit_docs(cx, "items/0", "Empty notes");
    move_folder(&workspace, cx);
    workspace.update(cx, |view, _, _| {
        let dialog = view.structure_dialog.as_ref().expect("form stays open");
        assert!(dialog.parent.is_empty() && dialog.destination_required);
        assert!(view.overview_drafts.iter().any(|(target, draft)| {
            matches!(target, OverviewTarget::Detached(_))
                && draft.current.docs == Some(note("Empty notes"))
        }));
        assert!(
            !view
                .overview_drafts
                .contains_key(&OverviewTarget::Folder("items/0".into()))
        );
    });
}

#[gpui_kit::test]
fn swapped_same_name_requests_follow_the_chosen_request(cx: &mut TestAppContext) {
    confirm_request(
        cx,
        &req(
            "dup-delete",
            DUPLICATES,
            "items/0",
            Pending::Delete,
            Change::MoveTo(1),
        )
        .urls(&["https://example.test/two"]),
    );
    confirm_request(
        cx,
        &req(
            "dup-rename",
            DUPLICATES,
            "items/0",
            Pending::Rename(Some("Renamed")),
            Change::MoveTo(1),
        )
        .pairs(&[
            ("Dup", "https://example.test/two"),
            ("Renamed", "https://example.test/one"),
        ]),
    );
}

#[gpui_kit::test]
fn swapped_same_name_folders_do_not_delete_the_other_folder(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "twin-folders", TWIN_FOLDERS);
    workspace.select(cx, ItemKind::Folder, "items/0");
    workspace.delete(cx);
    workspace.edit_docs(cx, "items/0", "Chosen notes");
    move_folder(&workspace, cx);
    assert!(workspace.update(cx, |view, _, _| {
        view.application_dialog.is_none()
            && view
                .overview_drafts
                .keys()
                .any(|target| matches!(target, OverviewTarget::Detached(_)))
    }));
    workspace.confirm_delete(cx);
    cx.run_until_parked();
    assert_eq!(
        workspace.urls(),
        ["https://example.test/one", "https://example.test/two"]
    );

    workspace.reset(cx, TWIN_FOLDERS);
    workspace.save_into(cx, "items/0", None);
    move_folder(&workspace, cx);
    workspace.update(cx, |view, _, _| {
        let dialog = view.structure_dialog.as_ref().expect("form stays open");
        assert!(dialog.destination_required && dialog.parent.is_empty());
    });
}

#[gpui_kit::test]
fn edited_and_swapped_duplicate_delete_keeps_the_other_request(cx: &mut TestAppContext) {
    confirm_request(
        cx,
        &req(
            "n1b-delete",
            NESTED_DUPS,
            "items/0/items/1",
            Pending::Delete,
            Change::Replace(NESTED_SWAPPED),
        )
        .urls(&["https://example.test/oneX"]),
    );
}

#[gpui_kit::test]
fn swapped_same_named_parents_do_not_keep_the_child_folder(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "parents", NESTED_PARENTS);
    workspace.save_into(cx, "items/0/items/0", Some("Placed"));
    workspace.edit_docs(cx, "items/0/items/0", "Child notes");
    let disk = move_folder(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        let dialog = view.structure_dialog.as_ref().expect("form stays open");
        assert!(dialog.parent.is_empty() && dialog.destination_required);
        assert!(view.selected_tree_item.is_none());
        assert!(view.overview_drafts.iter().any(|(target, draft)| {
            matches!(target, OverviewTarget::Detached(_))
                && draft.current.docs == Some(note("Child notes"))
        }));
        assert!(
            !view
                .overview_drafts
                .contains_key(&OverviewTarget::Folder("items/0/items/0".into()))
        );
        view.submit_structure_dialog(window, cx);
        assert!(view.structure_task.is_none());
    });
    assert_eq!(workspace.bytes(), disk);
    assert_eq!(
        workspace.urls(),
        ["https://example.test/one", "https://example.test/two"]
    );
}

fn open_dirty_request_pair(
    workspace: &Workspace,
    cx: &mut TestAppContext,
) -> (RequestKey, RequestKey) {
    workspace.update(cx, |view, _, cx| {
        let keys: Vec<_> = view
            .loaded_workspace
            .as_ref()
            .unwrap()
            .requests()
            .iter()
            .map(|request| request.key())
            .collect();
        view.select_request(keys[0], cx);
        view.select_request(keys[1], cx);
        view.edit_request(
            keys[0],
            |request| request.url = Some("https://unsaved.example".to_owned()),
            cx,
        );
        (keys[0], keys[1])
    })
}

fn structural_reload_keeps_both_request_tabs(workspace: &Workspace, cx: &mut TestAppContext) {
    let disk = workspace.reconcile_move(cx, ItemKind::Request, "items/0", 1);
    workspace.update(cx, |view, window, cx| {
        assert!(view.application_dialog.is_none());
        view.handle_application_dialog_action(ApplicationDialogAction::Discard, window, cx);
        assert_eq!(view.shell.open_tabs().len(), 2);
    });
    assert_eq!(workspace.names(false), ["Beta", "Alpha"]);
    assert_eq!(workspace.bytes(), disk);
    assert!(!workspace.urls().iter().any(|url| url.contains("unsaved")));
}

#[gpui_kit::test]
fn structural_reload_cancels_pending_request_close(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "close-tab", REQUESTS);
    let (dirty, _) = open_dirty_request_pair(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        view.request_close_tab(dirty, window, cx)
    });
    structural_reload_keeps_both_request_tabs(&workspace, cx);
    assert!(workspace.toast(cx, ToastIntent::Warning, "Closing the tab was cancelled."));
}

#[gpui_kit::test]
fn structural_reload_cancels_pending_close_other_tabs(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "close-others", REQUESTS);
    let (_, keep) = open_dirty_request_pair(&workspace, cx);
    workspace.update(cx, |view, window, cx| {
        view.request_close_other_tabs(keep, window, cx);
    });
    structural_reload_keeps_both_request_tabs(&workspace, cx);
    assert!(workspace.toast(cx, ToastIntent::Warning, "Close Other Tabs was cancelled."));
}

#[gpui_kit::test]
fn completing_a_detached_save_keeps_newer_edits(cx: &mut TestAppContext) {
    let workspace = Workspace::open(cx, "newer-doc", COLLECTION);
    workspace.edit_docs(cx, "items/0", "Original notes");
    let submitted = workspace.update(cx, |view, _, _| {
        let draft = view
            .overview_drafts
            .get_mut(&OverviewTarget::Folder("items/0".into()))
            .unwrap();
        draft.save_token = Some(3);
        draft.current.clone()
    });
    workspace.edit_docs(cx, "items/0", "Newer notes");
    move_folder(&workspace, cx);
    workspace.update(cx, |view, _, _| {
        view.complete_overview_draft_save(&OverviewTarget::Folder("items/0".into()), &submitted, 3);
        let draft = view
            .overview_drafts
            .values()
            .find(|draft| draft.current.docs == Some(note("Newer notes")))
            .expect("newer edits stay");
        assert!(draft.save_token.is_none());
    });
}

#[gpui_kit::test]
fn edited_and_swapped_duplicate_rename_leaves_the_other_request(cx: &mut TestAppContext) {
    confirm_request(
        cx,
        &req(
            "n1b-rename",
            NESTED_DUPS,
            "items/0/items/1",
            Pending::Rename(Some("Renamed")),
            Change::Replace(NESTED_SWAPPED),
        )
        .pairs(&[
            ("Dup", "https://example.test/oneX"),
            ("Renamed", "https://example.test/two"),
        ]),
    );
}
