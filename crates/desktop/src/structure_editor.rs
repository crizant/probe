use std::collections::{BTreeMap, BTreeSet};

use probe_core::{FolderKey, ItemKind, RequestKey, RequestProtocol, Workspace, WorkspaceItemRef};
use probe_opencollection::{ItemLocator, LoadedWorkspace, StructureOperation};

pub(crate) const ROOT_PARENT: &str = "";

/// Parsed data stays in memory until the existing structure operation commits it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CurlImport {
    pub(crate) command: String,
    pub(crate) parsed: Option<Result<probe_core::Request, probe_application::CurlParseError>>,
    pub(crate) error: Option<String>,
    pub(crate) committed: bool,
}

/// Shown when a reload cannot prove the chosen folder is still the same one.
pub(crate) const DESTINATION_RESELECTION_MESSAGE: &str =
    "The destination folder could not be identified. Select a destination before continuing.";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StructureDialogMode {
    CreateRequest(RequestProtocol),
    ImportCurl,
    CreateFolder,
    SaveDetachedRequest { key: RequestKey },
    Rename { kind: ItemKind, selector: String },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StructureDialog {
    pub(crate) mode: StructureDialogMode,
    pub(crate) curl: Option<CurlImport>,
    pub(crate) name: String,
    pub(crate) parent: String,
    /// The chosen folder could not be identified after a reload. Submit stays
    /// disabled until the user selects a destination again.
    pub(crate) destination_required: bool,
    /// Folder selectors expanded in the dialog destination tree.
    ///
    /// This is independent of the sidebar so collapsing a destination does not
    /// collapse the collection tree. Selectors are re-resolved to the same
    /// folders after an external reload.
    pub(crate) expanded_folders: BTreeSet<String>,
    /// `Some` while the save dialog's New Folder sheet is open.
    pub(crate) new_folder_name: Option<String>,
}

impl StructureDialog {
    pub(crate) fn save_detached_request(
        key: RequestKey,
        name: String,
        parent: Option<String>,
    ) -> Self {
        Self {
            mode: StructureDialogMode::SaveDetachedRequest { key },
            name,
            parent: parent.unwrap_or_default(),
            destination_required: false,
            expanded_folders: BTreeSet::new(),
            new_folder_name: None,
            curl: None,
        }
    }
    pub(crate) fn create_request(protocol: RequestProtocol, parent: Option<String>) -> Self {
        Self {
            mode: StructureDialogMode::CreateRequest(protocol),
            name: String::new(),
            parent: parent.unwrap_or_default(),
            destination_required: false,
            expanded_folders: BTreeSet::new(),
            new_folder_name: None,
            curl: None,
        }
    }

    pub(crate) fn create_folder(parent: Option<String>) -> Self {
        Self {
            mode: StructureDialogMode::CreateFolder,
            name: String::new(),
            parent: parent.unwrap_or_default(),
            destination_required: false,
            expanded_folders: BTreeSet::new(),
            new_folder_name: None,
            curl: None,
        }
    }

    pub(crate) fn rename(kind: ItemKind, selector: String, name: String) -> Self {
        Self {
            mode: StructureDialogMode::Rename { kind, selector },
            name,
            parent: String::new(),
            destination_required: false,
            expanded_folders: BTreeSet::new(),
            new_folder_name: None,
            curl: None,
        }
    }

    pub(crate) fn import_curl(parent: Option<String>) -> Self {
        let mut dialog = Self::create_request(RequestProtocol::Http, parent);
        dialog.mode = StructureDialogMode::ImportCurl;
        dialog.curl = Some(CurlImport::default());
        dialog
    }

    pub(crate) const fn title(&self) -> &'static str {
        match self.mode {
            StructureDialogMode::ImportCurl => "Import from cURL",
            StructureDialogMode::CreateRequest(RequestProtocol::Http) => "New HTTP Request",
            StructureDialogMode::CreateRequest(RequestProtocol::Graphql) => "New GraphQL Request",
            StructureDialogMode::CreateRequest(RequestProtocol::WebSocket) => {
                "New WebSocket Request"
            }
            StructureDialogMode::CreateFolder => "New Folder",
            StructureDialogMode::SaveDetachedRequest { .. } => "Save Request",
            StructureDialogMode::Rename { .. } => "Rename",
        }
    }

    pub(crate) const fn submit_label(&self) -> &'static str {
        match self.mode {
            StructureDialogMode::ImportCurl => "Import Request",
            StructureDialogMode::CreateRequest(_) | StructureDialogMode::CreateFolder => "Create",
            StructureDialogMode::SaveDetachedRequest { .. } => "Save",
            StructureDialogMode::Rename { .. } => "Rename",
        }
    }

    pub(crate) const fn edits_destination(&self) -> bool {
        matches!(
            self.mode,
            StructureDialogMode::CreateRequest(_)
                | StructureDialogMode::ImportCurl
                | StructureDialogMode::CreateFolder
                | StructureDialogMode::SaveDetachedRequest { .. }
        )
    }

    pub(crate) const fn key_context(&self) -> &'static str {
        if matches!(self.mode, StructureDialogMode::ImportCurl) {
            "CurlImportDialog"
        } else if self.edits_destination() {
            "TreeStructureDialog"
        } else {
            "StructureDialog"
        }
    }

    pub(crate) fn operation(&self) -> Result<StructureOperation, String> {
        let name = self.name.trim();
        let parent = (!self.parent.is_empty()).then(|| self.parent.clone());
        match &self.mode {
            StructureDialogMode::ImportCurl => {
                if name.is_empty() {
                    return Err("Request name is required.".into());
                }
                let request = self
                    .curl
                    .as_ref()
                    .and_then(|curl| curl.parsed.as_ref())
                    .ok_or("Paste a supported cURL command and wait for its preview.")?
                    .as_ref()
                    .map_err(|error| error.to_string())?;
                let update = probe_core::RequestUpdate {
                    headers: Some(request.headers.clone()),
                    query_parameters: Some(request.query_parameters.clone()),
                    path_parameters: Some(request.path_parameters.clone()),
                    body: probe_core::FieldPatch::from_optional(request.http_body().cloned()),
                    authentication: probe_core::FieldPatch::from_optional(
                        request.authentication.clone(),
                    ),
                    ..Default::default()
                };
                Ok(StructureOperation::CreateRequest {
                    parent,
                    index: None,
                    name: name.to_owned(),
                    method: request.method.clone(),
                    url: request.url.clone(),
                    settings: Some(request.settings.clone()),
                    protocol: RequestProtocol::Http,
                    graphql: None,
                    update: Some(update),
                })
            }
            StructureDialogMode::SaveDetachedRequest { .. } => {
                Err("Request save is handled by the desktop editor.".to_owned())
            }
            StructureDialogMode::CreateRequest(protocol) => {
                if name.is_empty() {
                    return Err("Request name is required.".to_owned());
                }
                Ok(StructureOperation::CreateRequest {
                    parent,
                    index: None,
                    name: name.to_owned(),
                    method: protocol.default_method().map(str::to_owned),
                    url: None,
                    settings: None,
                    protocol: *protocol,
                    graphql: None,
                    update: None,
                })
            }
            StructureDialogMode::CreateFolder => {
                if name.is_empty() {
                    return Err("Folder name is required.".to_owned());
                }
                Ok(StructureOperation::CreateFolder {
                    parent,
                    index: None,
                    name: name.to_owned(),
                })
            }
            StructureDialogMode::Rename { kind, selector } => {
                if name.is_empty() {
                    return Err("Name is required.".to_owned());
                }
                Ok(StructureOperation::Rename {
                    target: ItemLocator {
                        kind: *kind,
                        selector: selector.clone(),
                    },
                    name: name.to_owned(),
                })
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SaveDestinationRow {
    pub(crate) selector: String,
    pub(crate) name: String,
    pub(crate) depth: usize,
    pub(crate) expandable: bool,
    pub(crate) expanded: bool,
}

pub(crate) fn save_destination_rows(
    loaded: &LoadedWorkspace,
    expanded: &BTreeSet<String>,
) -> Vec<SaveDestinationRow> {
    let mut rows = vec![SaveDestinationRow {
        selector: ROOT_PARENT.to_owned(),
        name: "Collection root".to_owned(),
        depth: 0,
        expandable: false,
        expanded: true,
    }];
    let root = loaded.workspace().root_items().to_vec();
    // The synthetic collection-root choice does not add a level to sidebar folders.
    append_save_destination_folders(loaded, &root, 0, expanded, &mut rows);
    rows
}

/// Selectors from the collection root down to `target`, including `target`.
pub(crate) fn folder_ancestor_selectors(loaded: &LoadedWorkspace, target: &str) -> Vec<String> {
    let root = loaded.workspace().root_items().to_vec();
    let mut path = Vec::new();
    if walk_folder_ancestors(loaded, &root, target, &mut path) {
        path
    } else {
        Vec::new()
    }
}

fn append_save_destination_folders(
    loaded: &LoadedWorkspace,
    items: &[WorkspaceItemRef],
    depth: usize,
    expanded: &BTreeSet<String>,
    rows: &mut Vec<SaveDestinationRow>,
) {
    let folders = items
        .iter()
        .filter_map(|item| match *item {
            WorkspaceItemRef::Folder(key) => Some(key),
            WorkspaceItemRef::Request(_) => None,
        })
        .collect::<Vec<_>>();
    for key in folders {
        let Some(selector) = loaded.folder_selector(key).map(str::to_owned) else {
            continue;
        };
        let children = folder_children(loaded, key);
        let expandable = children
            .iter()
            .any(|child| matches!(child, WorkspaceItemRef::Folder(_)));
        let is_expanded = expanded.contains(&selector);
        rows.push(SaveDestinationRow {
            selector: selector.clone(),
            name: folder_label(loaded, key),
            depth,
            expandable,
            expanded: is_expanded,
        });
        if is_expanded {
            append_save_destination_folders(loaded, &children, depth + 1, expanded, rows);
        }
    }
}

fn walk_folder_ancestors(
    loaded: &LoadedWorkspace,
    items: &[WorkspaceItemRef],
    target: &str,
    path: &mut Vec<String>,
) -> bool {
    for item in items {
        let WorkspaceItemRef::Folder(key) = *item else {
            continue;
        };
        let Some(selector) = loaded.folder_selector(key).map(str::to_owned) else {
            continue;
        };
        path.push(selector.clone());
        if selector == target {
            return true;
        }
        let children = folder_children(loaded, key);
        if walk_folder_ancestors(loaded, &children, target, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn folder_children(loaded: &LoadedWorkspace, key: FolderKey) -> Vec<WorkspaceItemRef> {
    loaded
        .workspace()
        .folder(key)
        .map(|folder| folder.children.clone())
        .unwrap_or_default()
}

fn folder_label(loaded: &LoadedWorkspace, key: FolderKey) -> String {
    loaded
        .workspace()
        .folder(key)
        .and_then(|folder| folder.metadata.name.clone())
        .unwrap_or_else(|| "Untitled folder".to_owned())
}

/// Identity of one folder, captured before a workspace reload.
///
/// Bundled selectors such as `items/1` are positions. A name path finds the
/// folder again after a reorder, and the child structure separates same-named
/// siblings. The captured selector is only a key for filesystem rename hints;
/// it is not used to choose among identical siblings, because a swap would
/// make that position address the other folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FolderAnchor {
    names: Vec<Option<String>>,
    children: Vec<ChildIdentity>,
    selector: String,
    /// Whether any other folder shared [`Self::names`] when this anchor was captured.
    ///
    /// A later reload may leave only one folder with that name path. That survivor
    /// is the captured folder only when the path was already unique; otherwise the
    /// child structure has to match.
    name_path_unique: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChildIdentity {
    Request(Option<String>),
    Folder {
        name: Option<String>,
        children: Vec<ChildIdentity>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResolvedFolder {
    Found(String),
    Unresolved,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DialogFolderAnchors {
    pub(crate) parent: Option<FolderAnchor>,
    pub(crate) expanded: Vec<FolderAnchor>,
}

impl DialogFolderAnchors {
    pub(crate) fn capture(dialog: Option<&StructureDialog>, index: &FolderIndex) -> Self {
        let Some(dialog) = dialog.filter(|dialog| dialog.edits_destination()) else {
            return Self::default();
        };
        Self {
            parent: (!dialog.parent.is_empty())
                .then(|| index.anchor(&dialog.parent))
                .flatten(),
            expanded: dialog
                .expanded_folders
                .iter()
                .filter_map(|selector| index.anchor(selector))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ParentUpdate {
    Root,
    Folder(String),
    Unresolved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DialogDestination {
    pub(crate) expanded: BTreeSet<String>,
    pub(crate) parent: ParentUpdate,
    /// Unresolved only. True when the stored parent selector still addresses a
    /// folder, so keeping it would create the item in a folder the user did not
    /// choose. False when that selector addresses no folder: it is kept so the
    /// collection root is not treated as the chosen destination.
    pub(crate) clear_live_parent: bool,
}

pub(crate) fn resolve_dialog_destination(
    dialog: &StructureDialog,
    anchors: &DialogFolderAnchors,
    remaps: &BTreeMap<String, String>,
    loaded: &LoadedWorkspace,
    index: &FolderIndex,
) -> DialogDestination {
    let expanded = anchors
        .expanded
        .iter()
        .filter_map(|anchor| relocate_folder(anchor, remaps, loaded, index))
        .collect();
    if dialog.parent.is_empty() {
        // An empty selector is the collection root only when the user chose it.
        // A previous reload may have cleared the parent because the folder could
        // not be identified; that still requires an explicit selection.
        if dialog.destination_required {
            return DialogDestination {
                expanded,
                parent: ParentUpdate::Unresolved,
                clear_live_parent: false,
            };
        }
        return DialogDestination {
            expanded,
            parent: ParentUpdate::Root,
            clear_live_parent: false,
        };
    }
    let parent = anchors
        .parent
        .as_ref()
        .and_then(|anchor| relocate_folder(anchor, remaps, loaded, index));
    match parent {
        Some(selector) => DialogDestination {
            expanded,
            parent: ParentUpdate::Folder(selector),
            clear_live_parent: false,
        },
        None => DialogDestination {
            expanded,
            parent: ParentUpdate::Unresolved,
            clear_live_parent: loaded.folder_key(&dialog.parent).is_some(),
        },
    }
}

/// Follows a directory rename hint when it still addresses a folder, then the
/// captured folder identity.
///
/// Bundled collections never produce those hints. Their reload maps are request
/// selectors, so a bundled folder falls through to [`FolderIndex::resolve`].
pub(crate) fn relocate_folder(
    anchor: &FolderAnchor,
    remaps: &BTreeMap<String, String>,
    loaded: &LoadedWorkspace,
    index: &FolderIndex,
) -> Option<String> {
    if let Some(mapped) = remaps.get(&anchor.selector)
        && loaded.folder_key(mapped).is_some()
    {
        return Some(mapped.clone());
    }
    match index.resolve(anchor) {
        ResolvedFolder::Found(selector) => Some(selector),
        ResolvedFolder::Unresolved => None,
    }
}

pub(crate) struct FolderIndex {
    folders: Vec<IndexedFolder>,
}

struct IndexedFolder {
    selector: String,
    names: Vec<Option<String>>,
    children: Vec<ChildIdentity>,
}

impl FolderIndex {
    pub(crate) fn build(loaded: &LoadedWorkspace) -> Self {
        let mut folders = Vec::new();
        let mut names = Vec::new();
        index_folders(
            loaded,
            loaded.workspace().root_items(),
            &mut names,
            &mut folders,
        );
        Self { folders }
    }

    pub(crate) fn anchor(&self, selector: &str) -> Option<FolderAnchor> {
        let folder = self
            .folders
            .iter()
            .find(|folder| folder.selector == selector)?;
        let name_path_unique = self
            .folders
            .iter()
            .filter(|other| other.names == folder.names)
            .count()
            == 1;
        Some(FolderAnchor {
            names: folder.names.clone(),
            children: folder.children.clone(),
            selector: folder.selector.clone(),
            name_path_unique,
        })
    }

    pub(crate) fn resolve(&self, anchor: &FolderAnchor) -> ResolvedFolder {
        let name_matches: Vec<_> = self
            .folders
            .iter()
            .filter(|folder| folder.names == anchor.names)
            .collect();
        if anchor.name_path_unique && name_matches.len() == 1 {
            return ResolvedFolder::Found(name_matches[0].selector.clone());
        }
        let distinguished: Vec<_> = name_matches
            .into_iter()
            .filter(|folder| folder.children == anchor.children)
            .collect();
        if distinguished.len() == 1 {
            return ResolvedFolder::Found(distinguished[0].selector.clone());
        }
        // The name path was shared, or it no longer matches a single folder whose
        // children are the captured ones. A lone survivor with different children
        // is a different folder.
        ResolvedFolder::Unresolved
    }
}

fn index_folders(
    loaded: &LoadedWorkspace,
    items: &[WorkspaceItemRef],
    names: &mut Vec<Option<String>>,
    folders: &mut Vec<IndexedFolder>,
) -> Vec<ChildIdentity> {
    let mut children = Vec::new();
    for item in items {
        match *item {
            WorkspaceItemRef::Request(key) => {
                children.push(ChildIdentity::Request(item_name(
                    loaded.workspace().request(key),
                )));
            }
            WorkspaceItemRef::Folder(key) => {
                let Some(selector) = loaded.folder_selector(key).map(str::to_owned) else {
                    continue;
                };
                let name = loaded
                    .workspace()
                    .folder(key)
                    .and_then(|folder| folder.metadata.name.clone());
                names.push(name.clone());
                let nested = folder_children(loaded, key);
                let nested_children = index_folders(loaded, &nested, names, folders);
                folders.push(IndexedFolder {
                    selector,
                    names: names.clone(),
                    children: nested_children.clone(),
                });
                names.pop();
                children.push(ChildIdentity::Folder {
                    name,
                    children: nested_children,
                });
            }
        }
    }
    children
}

fn item_name(request: Option<&probe_core::Request>) -> Option<String> {
    request.and_then(|request| request.metadata.name.clone())
}

pub(crate) fn item_position(
    workspace: &Workspace,
    target: WorkspaceItemRef,
) -> Option<(Option<FolderKey>, usize)> {
    locate_item(workspace, workspace.root_items(), None, target)
}

fn locate_item(
    workspace: &Workspace,
    items: &[WorkspaceItemRef],
    parent: Option<FolderKey>,
    target: WorkspaceItemRef,
) -> Option<(Option<FolderKey>, usize)> {
    for (index, item) in items.iter().copied().enumerate() {
        if item == target {
            return Some((parent, index));
        }
        if let WorkspaceItemRef::Folder(folder_key) = item
            && let Some(folder) = workspace.folder(folder_key)
            && let Some(position) =
                locate_item(workspace, &folder.children, Some(folder_key), target)
        {
            return Some(position);
        }
    }
    None
}

pub(crate) fn descendant_requests(
    workspace: &Workspace,
    folder: FolderKey,
    requests: &mut Vec<probe_core::RequestKey>,
) {
    let Some(folder) = workspace.folder(folder) else {
        return;
    };
    for item in &folder.children {
        match *item {
            WorkspaceItemRef::Request(key) => requests.push(key),
            WorkspaceItemRef::Folder(key) => descendant_requests(workspace, key, requests),
        }
    }
}

/// Vertical fraction of a tree row used as a folder-edge drop zone.
pub(crate) const FOLDER_DROP_EDGE: f32 = 0.25;

/// Where a pointer sits on a hovered tree row during drag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DropZone {
    Before,
    Into,
    After,
}

/// Visual insertion hint for a valid or rejected tree drop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DropIndicator {
    Before(WorkspaceItemRef),
    After(WorkspaceItemRef),
    IntoFolder(FolderKey),
    RootEnd,
}

/// Destination implied by hovering a visible tree row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TreeDropIntent {
    pub parent: Option<FolderKey>,
    pub index: usize,
    pub indicator: DropIndicator,
}

/// Why a computed drop destination cannot be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DropReject {
    NoOp,
    InvalidDestination,
    DuplicatePath,
}

/// Maps a pointer's vertical fraction of a row onto a drop zone.
#[must_use]
pub(crate) fn drop_zone(is_folder: bool, relative_y: f32) -> DropZone {
    if is_folder {
        if relative_y < FOLDER_DROP_EDGE {
            DropZone::Before
        } else if relative_y > 1.0 - FOLDER_DROP_EDGE {
            DropZone::After
        } else {
            DropZone::Into
        }
    } else if relative_y < 0.5 {
        DropZone::Before
    } else {
        DropZone::After
    }
}

/// Resolves which visible row the pointer is over, including list padding and empty space.
#[must_use]
pub(crate) fn hovered_row_index(
    pointer_y: f32,
    list_top: f32,
    padding_top: f32,
    scroll_y: f32,
    row_height: f32,
    row_count: usize,
) -> Option<(usize, f32)> {
    if row_count == 0 || row_height <= 0.0 {
        return None;
    }
    let y = pointer_y - list_top - padding_top - scroll_y;
    if y <= 0.0 {
        return Some((0, 0.0));
    }
    let index = (y / row_height).floor() as usize;
    if index >= row_count {
        return Some((row_count - 1, 1.0));
    }
    Some((index, (y / row_height).fract()))
}

/// Converts a hovered row and zone into a destination parent and insertion index.
#[must_use]
pub(crate) fn drop_intent(
    workspace: &Workspace,
    hovered: WorkspaceItemRef,
    zone: DropZone,
    folder_expanded: bool,
) -> Option<TreeDropIntent> {
    let (parent, index) = item_position(workspace, hovered)?;
    match (hovered, zone) {
        (WorkspaceItemRef::Folder(folder), DropZone::Into) => Some(TreeDropIntent {
            parent: Some(folder),
            index: workspace.folder(folder)?.children.len(),
            indicator: DropIndicator::IntoFolder(folder),
        }),
        (WorkspaceItemRef::Folder(folder), DropZone::After) if folder_expanded => {
            Some(TreeDropIntent {
                parent: Some(folder),
                index: 0,
                indicator: DropIndicator::After(hovered),
            })
        }
        (_, DropZone::Before) => Some(TreeDropIntent {
            parent,
            index,
            indicator: DropIndicator::Before(hovered),
        }),
        (_, DropZone::After | DropZone::Into) => Some(TreeDropIntent {
            parent,
            index: index + 1,
            indicator: DropIndicator::After(hovered),
        }),
    }
}

/// Returns whether `item` is `folder` or a descendant of `folder`.
#[must_use]
pub(crate) fn item_is_within_folder(
    workspace: &Workspace,
    folder: FolderKey,
    item: WorkspaceItemRef,
) -> bool {
    if WorkspaceItemRef::Folder(folder) == item {
        return true;
    }
    let Some(folder) = workspace.folder(folder) else {
        return false;
    };
    folder.children.iter().any(|child| match *child {
        WorkspaceItemRef::Folder(child_folder) => {
            item == *child || item_is_within_folder(workspace, child_folder, item)
        }
        WorkspaceItemRef::Request(_) => item == *child,
    })
}

/// Returns true when an unbundled move would collide with an existing path.
#[must_use]
pub(crate) fn would_duplicate_path(
    uses_path_locators: bool,
    source_selector: &str,
    dest_parent_selector: Option<&str>,
    occupied: impl Fn(&str) -> bool,
) -> bool {
    if !uses_path_locators {
        return false;
    }
    let name = source_selector
        .rsplit('/')
        .next()
        .unwrap_or(source_selector);
    let proposed = match dest_parent_selector.filter(|parent| !parent.is_empty()) {
        Some(parent) => format!("{parent}/{name}"),
        None => name.to_owned(),
    };
    proposed != source_selector && occupied(&proposed)
}

/// Rejects no-op, self/descendant, and duplicate-path destinations.
pub(crate) fn validate_tree_drop(
    workspace: &Workspace,
    source: WorkspaceItemRef,
    source_parent: Option<FolderKey>,
    source_index: usize,
    intent: TreeDropIntent,
    duplicate_path: bool,
) -> Result<TreeDropIntent, DropReject> {
    if let WorkspaceItemRef::Folder(folder) = source
        && intent.parent.is_some_and(|parent| {
            item_is_within_folder(workspace, folder, WorkspaceItemRef::Folder(parent))
        })
    {
        return Err(DropReject::InvalidDestination);
    }
    if duplicate_path {
        return Err(DropReject::DuplicatePath);
    }
    let adjusted = adjusted_drop_index(source_parent, source_index, intent.parent, intent.index);
    if source_parent == intent.parent && adjusted == source_index {
        return Err(DropReject::NoOp);
    }
    Ok(intent)
}

/// Builds the same move/reorder operation used by the CLI and keyboard workflows.
#[must_use]
pub(crate) fn structure_operation_for_drop(
    kind: ItemKind,
    selector: String,
    source_parent: Option<FolderKey>,
    source_index: usize,
    dest_parent: Option<FolderKey>,
    dest_parent_selector: Option<String>,
    dest_index: usize,
) -> Option<StructureOperation> {
    let index = adjusted_drop_index(source_parent, source_index, dest_parent, dest_index);
    if source_parent == dest_parent {
        if index == source_index {
            return None;
        }
        return Some(StructureOperation::Reorder {
            target: ItemLocator { kind, selector },
            index,
        });
    }
    Some(StructureOperation::Move {
        target: ItemLocator { kind, selector },
        parent: dest_parent_selector,
        index: Some(index),
    })
}

fn adjusted_drop_index(
    source_parent: Option<FolderKey>,
    source_index: usize,
    dest_parent: Option<FolderKey>,
    dest_index: usize,
) -> usize {
    if source_parent == dest_parent && source_index < dest_index {
        dest_index - 1
    } else {
        dest_index
    }
}

#[cfg(test)]
mod tests {
    use probe_core::{
        Collection, CollectionItem, Folder, ItemKind, ItemMetadata, Request, RequestProtocol,
        Workspace, WorkspaceItemRef,
    };
    use probe_opencollection::{ItemLocator, StructureOperation};

    use super::{
        DropReject, DropZone, StructureDialog, descendant_requests, drop_intent, drop_zone,
        hovered_row_index, item_is_within_folder, item_position, structure_operation_for_drop,
        validate_tree_drop, would_duplicate_path,
    };

    #[test]
    fn dialog_builds_typed_create_operations() {
        for (protocol, method, title) in [
            (RequestProtocol::Http, "GET", "New HTTP Request"),
            (RequestProtocol::Graphql, "POST", "New GraphQL Request"),
        ] {
            let mut dialog = StructureDialog::create_request(protocol, Some("folder".to_owned()));
            assert!(dialog.operation().is_err(), "requests require a name");
            dialog.name = "  New request  ".to_owned();
            assert_eq!(dialog.title(), title);
            assert_eq!(dialog.submit_label(), "Create");
            assert_eq!(
                dialog.operation().unwrap(),
                StructureOperation::CreateRequest {
                    parent: Some("folder".to_owned()),
                    index: None,
                    name: "New request".to_owned(),
                    method: Some(method.to_owned()),
                    url: None,
                    settings: None,
                    protocol,
                    graphql: None,
                    update: None,
                }
            );
        }
    }

    #[test]
    fn tree_helpers_find_positions_and_descendant_requests() {
        let workspace = Workspace::from_collection(Collection {
            items: vec![CollectionItem::Folder(Folder {
                metadata: ItemMetadata {
                    name: Some("Group".to_owned()),
                    ..ItemMetadata::default()
                },
                items: vec![CollectionItem::Request(Request::default())],
                ..Folder::default()
            })],
            ..Collection::default()
        });
        let probe_core::WorkspaceItemRef::Folder(folder) = workspace.root_items()[0] else {
            panic!("fixture should contain a folder");
        };
        let request = workspace.folder(folder).unwrap().children[0];
        assert_eq!(item_position(&workspace, request), Some((Some(folder), 0)));
        let mut requests = Vec::new();
        descendant_requests(&workspace, folder, &mut requests);
        assert_eq!(requests.len(), 1);
    }

    fn nested_workspace() -> (
        Workspace,
        WorkspaceItemRef,
        WorkspaceItemRef,
        WorkspaceItemRef,
    ) {
        let workspace = Workspace::from_collection(Collection {
            items: vec![
                CollectionItem::Request(Request {
                    metadata: ItemMetadata {
                        name: Some("Alpha".to_owned()),
                        ..ItemMetadata::default()
                    },
                    ..Request::default()
                }),
                CollectionItem::Folder(Folder {
                    metadata: ItemMetadata {
                        name: Some("Group".to_owned()),
                        ..ItemMetadata::default()
                    },
                    items: vec![CollectionItem::Request(Request {
                        metadata: ItemMetadata {
                            name: Some("Nested".to_owned()),
                            ..ItemMetadata::default()
                        },
                        ..Request::default()
                    })],
                    ..Folder::default()
                }),
            ],
            ..Collection::default()
        });
        let alpha = workspace.root_items()[0];
        let folder = workspace.root_items()[1];
        let WorkspaceItemRef::Folder(folder_key) = folder else {
            panic!("fixture should contain a folder");
        };
        let nested = workspace.folder(folder_key).unwrap().children[0];
        (workspace, alpha, folder, nested)
    }

    #[test]
    fn drop_zones_split_folder_edges_from_nesting() {
        assert_eq!(drop_zone(true, 0.1), DropZone::Before);
        assert_eq!(drop_zone(true, 0.5), DropZone::Into);
        assert_eq!(drop_zone(true, 0.9), DropZone::After);
        assert_eq!(drop_zone(false, 0.4), DropZone::Before);
        assert_eq!(drop_zone(false, 0.6), DropZone::After);
    }

    #[test]
    fn hovered_row_index_maps_padding_scroll_and_empty_space() {
        assert_eq!(
            hovered_row_index(10.0, 10.0, 2.0, 0.0, 28.0, 3),
            Some((0, 0.0))
        );
        assert_eq!(
            hovered_row_index(10.0 + 2.0 + 42.0, 10.0, 2.0, 0.0, 28.0, 3),
            Some((1, 0.5))
        );
        assert_eq!(
            hovered_row_index(10.0 + 2.0, 10.0, 2.0, -28.0, 28.0, 3),
            Some((1, 0.0))
        );
        assert_eq!(
            hovered_row_index(10.0 + 2.0 + 200.0, 10.0, 2.0, 0.0, 28.0, 3),
            Some((2, 1.0))
        );
        assert_eq!(
            hovered_row_index(10.0 + 2.0 + 3.5 * 28.0, 10.0, 2.0, 0.0, 28.0, 4),
            Some((3, 0.5))
        );
    }

    #[test]
    fn drop_intent_nests_into_folders_and_inserts_before_siblings() {
        let (workspace, alpha, folder, nested) = nested_workspace();
        let WorkspaceItemRef::Folder(folder_key) = folder else {
            panic!("folder");
        };
        let into = drop_intent(&workspace, folder, DropZone::Into, false).unwrap();
        assert_eq!(into.parent, Some(folder_key));
        assert_eq!(into.index, 1);
        let before = drop_intent(&workspace, alpha, DropZone::Before, false).unwrap();
        assert_eq!(before.parent, None);
        assert_eq!(before.index, 0);
        let after_expanded = drop_intent(&workspace, folder, DropZone::After, true).unwrap();
        assert_eq!(after_expanded.parent, Some(folder_key));
        assert_eq!(after_expanded.index, 0);
        let after_nested = drop_intent(&workspace, nested, DropZone::After, false).unwrap();
        assert_eq!(after_nested.parent, Some(folder_key));
        assert_eq!(after_nested.index, 1);
    }

    #[test]
    fn validate_tree_drop_rejects_self_descendant_duplicate_and_noop() {
        let (workspace, alpha, folder, nested) = nested_workspace();
        let WorkspaceItemRef::Folder(folder_key) = folder else {
            panic!("folder");
        };
        let into_self = drop_intent(&workspace, folder, DropZone::Into, false).unwrap();
        assert_eq!(
            validate_tree_drop(&workspace, folder, None, 1, into_self, false),
            Err(DropReject::InvalidDestination)
        );
        let into_descendant = drop_intent(&workspace, nested, DropZone::After, false).unwrap();
        assert_eq!(
            validate_tree_drop(&workspace, folder, None, 1, into_descendant, false),
            Err(DropReject::InvalidDestination)
        );
        assert!(item_is_within_folder(&workspace, folder_key, nested));
        let noop = drop_intent(&workspace, alpha, DropZone::Before, false).unwrap();
        assert_eq!(
            validate_tree_drop(&workspace, alpha, None, 0, noop, false),
            Err(DropReject::NoOp)
        );
        let into_folder = drop_intent(&workspace, folder, DropZone::Into, false).unwrap();
        assert_eq!(
            validate_tree_drop(&workspace, alpha, None, 0, into_folder, true),
            Err(DropReject::DuplicatePath)
        );
        assert!(validate_tree_drop(&workspace, alpha, None, 0, into_folder, false).is_ok());
    }

    #[test]
    fn drop_operations_match_cli_move_and_reorder() {
        let (workspace, _, folder, _) = nested_workspace();
        let into = drop_intent(&workspace, folder, DropZone::Into, false).unwrap();
        assert_eq!(
            structure_operation_for_drop(
                ItemKind::Request,
                "items/0".to_owned(),
                None,
                0,
                into.parent,
                Some("items/1".to_owned()),
                into.index,
            ),
            Some(StructureOperation::Move {
                target: ItemLocator::new(ItemKind::Request, "items/0"),
                parent: Some("items/1".to_owned()),
                index: Some(1),
            })
        );
        assert_eq!(
            structure_operation_for_drop(
                ItemKind::Folder,
                "items/1".to_owned(),
                None,
                1,
                None,
                None,
                0,
            ),
            Some(StructureOperation::Reorder {
                target: ItemLocator::new(ItemKind::Folder, "items/1"),
                index: 0,
            })
        );
        assert_eq!(
            structure_operation_for_drop(
                ItemKind::Request,
                "alpha.yml".to_owned(),
                None,
                0,
                None,
                None,
                0,
            ),
            None
        );
        assert!(would_duplicate_path(
            true,
            "alpha.yml",
            Some("group"),
            |selector| selector == "group/alpha.yml"
        ));
        assert!(!would_duplicate_path(
            false,
            "items/0",
            Some("items/1"),
            |_| true
        ));
    }

    use std::collections::BTreeMap;

    use probe_opencollection::LoadedWorkspace;

    use super::{DialogFolderAnchors, ParentUpdate, ResolvedFolder, resolve_dialog_destination};

    fn load(source: &str) -> LoadedWorkspace {
        probe_opencollection::load_workspace_from_str(source).unwrap()
    }

    fn bundled(items: &str) -> String {
        format!(
            "opencollection: 1.0.0\ninfo:\n  name: Destinations\nbundled: true\nitems:\n{items}"
        )
    }

    const BEFORE: &str = "  - info: {name: Before, type: folder}\n    items: []\n";
    const TARGET: &str = "  - info: {name: Target, type: folder}\n    items:\n      - info: {name: Marker, type: http}\n        http: {method: GET, url: https://example.test/marker}\n";
    const AFTER: &str = "  - info: {name: After, type: folder}\n    items: []\n";

    fn anchor_parent(source: &str, selector: &str) -> (LoadedWorkspace, super::FolderAnchor) {
        let loaded = load(source);
        let anchor = super::FolderIndex::build(&loaded)
            .anchor(selector)
            .expect("folder");
        (loaded, anchor)
    }

    fn resolved(source: &str, anchor: &super::FolderAnchor) -> ResolvedFolder {
        super::FolderIndex::build(&load(source)).resolve(anchor)
    }

    #[test]
    fn folder_identity_follows_a_reordered_inserted_removed_or_renamed_sibling() {
        let original = bundled(&format!("{BEFORE}{TARGET}{AFTER}"));
        let (_, anchor) = anchor_parent(&original, "items/1");
        assert_eq!(
            resolved(&bundled(&format!("{TARGET}{BEFORE}{AFTER}")), &anchor),
            ResolvedFolder::Found("items/0".into()),
            "reordering must not keep the old position"
        );
        assert_eq!(
            resolved(
                &bundled(&format!(
                    "  - info: {{name: Inserted, type: folder}}\n    items: []\n{BEFORE}{TARGET}{AFTER}"
                )),
                &anchor
            ),
            ResolvedFolder::Found("items/2".into())
        );
        assert_eq!(
            resolved(&bundled(&format!("{TARGET}{AFTER}")), &anchor),
            ResolvedFolder::Found("items/0".into())
        );
        let renamed_sibling = bundled(&format!(
            "  - info: {{name: Renamed, type: folder}}\n    items: []\n{TARGET}{AFTER}"
        ));
        assert_eq!(
            resolved(&renamed_sibling, &anchor),
            ResolvedFolder::Found("items/1".into())
        );
    }

    #[test]
    fn renaming_the_chosen_folder_is_not_treated_as_the_same_position() {
        let original = bundled(&format!("{BEFORE}{TARGET}"));
        let (_, anchor) = anchor_parent(&original, "items/1");
        let renamed = bundled(
            "  - info: {name: Before, type: folder}\n    items: []\n  - info: {name: Renamed, type: folder}\n    items:\n      - info: {name: Marker, type: http}\n        http: {method: GET, url: https://example.test/marker}\n",
        );
        assert_eq!(resolved(&renamed, &anchor), ResolvedFolder::Unresolved);
    }

    #[test]
    fn same_named_siblings_follow_child_identity_and_identical_ones_stay_unresolved() {
        let distinguished = bundled(
            "  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Left, type: http}\n        http: {method: GET, url: https://example.test/left}\n  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Right, type: http}\n        http: {method: GET, url: https://example.test/right}\n",
        );
        let (_, right) = anchor_parent(&distinguished, "items/1");
        let swapped = bundled(
            "  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Right, type: http}\n        http: {method: GET, url: https://example.test/right}\n  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Left, type: http}\n        http: {method: GET, url: https://example.test/left}\n",
        );
        assert_eq!(
            resolved(&swapped, &right),
            ResolvedFolder::Found("items/0".into())
        );

        let identical = bundled(
            "  - info: {name: Twin, type: folder}\n    items: []\n  - info: {name: Twin, type: folder}\n    items: []\n",
        );
        let (_, second) = anchor_parent(&identical, "items/1");
        assert_eq!(
            resolved(&identical, &second),
            ResolvedFolder::Unresolved,
            "identical siblings cannot be told apart by position"
        );
    }

    #[test]
    fn nested_folder_identity_follows_its_parent() {
        let original = bundled(
            "  - info: {name: Outer, type: folder}\n    items:\n      - info: {name: Inner, type: folder}\n        items: []\n  - info: {name: Other, type: folder}\n    items: []\n",
        );
        let (_, inner) = anchor_parent(&original, "items/0/items/0");
        let moved = bundled(
            "  - info: {name: Other, type: folder}\n    items: []\n  - info: {name: Outer, type: folder}\n    items:\n      - info: {name: Inner, type: folder}\n        items: []\n",
        );
        assert_eq!(
            resolved(&moved, &inner),
            ResolvedFolder::Found("items/1/items/0".into())
        );
    }

    #[test]
    fn dialog_destination_clears_a_positional_selector_that_now_points_elsewhere() {
        let original = load(&bundled(&format!("{BEFORE}{TARGET}{AFTER}")));
        let mut dialog = StructureDialog::create_folder(Some("items/1".into()));
        dialog.expanded_folders.insert("items/0".into());
        dialog.expanded_folders.insert("items/1".into());
        let anchors =
            DialogFolderAnchors::capture(Some(&dialog), &super::FolderIndex::build(&original));
        let fresh = load(&bundled(&format!("{TARGET}{BEFORE}{AFTER}")));
        let resolved = resolve_dialog_destination(
            &dialog,
            &anchors,
            &BTreeMap::new(),
            &fresh,
            &super::FolderIndex::build(&fresh),
        );
        assert_eq!(resolved.parent, ParentUpdate::Folder("items/0".into()));
        assert_eq!(
            resolved.expanded,
            ["items/0".to_owned(), "items/1".to_owned()]
                .into_iter()
                .collect()
        );
        assert!(!resolved.clear_live_parent);

        let identical = load(&bundled(
            "  - info: {name: Twin, type: folder}\n    items: []\n  - info: {name: Twin, type: folder}\n    items: []\n",
        ));
        dialog.parent = "items/1".into();
        dialog.expanded_folders.clear();
        let anchors =
            DialogFolderAnchors::capture(Some(&dialog), &super::FolderIndex::build(&identical));
        let swapped = load(&bundled(
            "  - info: {name: Twin, type: folder}\n    items: []\n  - info: {name: Twin, type: folder}\n    items: []\n",
        ));
        let resolved = resolve_dialog_destination(
            &dialog,
            &anchors,
            &BTreeMap::new(),
            &swapped,
            &super::FolderIndex::build(&swapped),
        );
        assert_eq!(resolved.parent, ParentUpdate::Unresolved);
        assert!(resolved.clear_live_parent);
    }

    const LEFT_TWIN: &str = "  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Left, type: http}\n        http: {method: GET, url: https://example.test/left}\n";
    const RIGHT_TWIN: &str = "  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Right, type: http}\n        http: {method: GET, url: https://example.test/right}\n";

    #[test]
    fn deleting_or_renaming_one_of_two_same_named_folders_is_unresolved() {
        let original = bundled(&format!("{LEFT_TWIN}{RIGHT_TWIN}"));
        let (_, right) = anchor_parent(&original, "items/1");
        assert_eq!(
            resolved(&bundled(LEFT_TWIN), &right),
            ResolvedFolder::Unresolved,
            "the remaining twin is not the deleted folder"
        );
        assert_eq!(
            resolved(
                &bundled(&format!(
                    "{LEFT_TWIN}  - info: {{name: Renamed, type: folder}}\n    items:\n      - info: {{name: Right, type: http}}\n        http: {{method: GET, url: https://example.test/right}}\n"
                )),
                &right
            ),
            ResolvedFolder::Unresolved
        );
    }

    #[test]
    fn a_shared_name_path_does_not_follow_a_twin_whose_children_changed() {
        let original = bundled(&format!("{LEFT_TWIN}{RIGHT_TWIN}"));
        let (_, right) = anchor_parent(&original, "items/1");
        let changed = bundled(
            "  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Left, type: http}\n        http: {method: GET, url: https://example.test/left}\n  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Renamed, type: http}\n        http: {method: GET, url: https://example.test/right}\n",
        );
        assert_eq!(resolved(&changed, &right), ResolvedFolder::Unresolved);

        let (_, unique) = anchor_parent(&bundled(TARGET), "items/0");
        let with_child = bundled(
            "  - info: {name: Target, type: folder}\n    items:\n      - info: {name: Marker, type: http}\n        http: {method: GET, url: https://example.test/marker}\n      - info: {name: Extra, type: http}\n        http: {method: GET, url: https://example.test/extra}\n",
        );
        assert_eq!(
            resolved(&with_child, &unique),
            ResolvedFolder::Found("items/0".into()),
            "a unique name path still matches after its children change"
        );
    }

    #[test]
    fn nested_duplicate_folders_are_not_confused_when_one_is_deleted() {
        let original = bundled(
            "  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Inner, type: folder}\n        items:\n          - info: {name: Left, type: http}\n            http: {method: GET, url: https://example.test/left}\n  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Inner, type: folder}\n        items:\n          - info: {name: Right, type: http}\n            http: {method: GET, url: https://example.test/right}\n",
        );
        let (_, right_inner) = anchor_parent(&original, "items/1/items/0");
        let survivor = bundled(
            "  - info: {name: Twin, type: folder}\n    items:\n      - info: {name: Inner, type: folder}\n        items:\n          - info: {name: Left, type: http}\n            http: {method: GET, url: https://example.test/left}\n",
        );
        assert_eq!(
            resolved(&survivor, &right_inner),
            ResolvedFolder::Unresolved
        );
    }

    #[test]
    fn moving_a_folder_to_another_parent_is_not_the_old_position() {
        let original = bundled(
            "  - info: {name: Before, type: folder}\n    items:\n      - info: {name: Target, type: folder}\n        items:\n          - info: {name: Marker, type: http}\n            http: {method: GET, url: https://example.test/marker}\n  - info: {name: After, type: folder}\n    items: []\n",
        );
        let (_, target) = anchor_parent(&original, "items/0/items/0");
        let moved = bundled(
            "  - info: {name: Before, type: folder}\n    items: []\n  - info: {name: After, type: folder}\n    items:\n      - info: {name: Target, type: folder}\n        items:\n          - info: {name: Marker, type: http}\n            http: {method: GET, url: https://example.test/marker}\n",
        );
        assert_eq!(resolved(&moved, &target), ResolvedFolder::Unresolved);
        assert_ne!(
            resolved(&moved, &target),
            ResolvedFolder::Found("items/0/items/0".into())
        );
    }

    #[test]
    fn a_cleared_destination_stays_unresolved_on_the_next_reload() {
        let fresh = load(&bundled(&format!("{BEFORE}{TARGET}")));
        let mut dialog = StructureDialog::create_folder(None);
        dialog.destination_required = true;
        let index = super::FolderIndex::build(&fresh);
        let anchors = DialogFolderAnchors::capture(Some(&dialog), &index);
        let resolved =
            resolve_dialog_destination(&dialog, &anchors, &BTreeMap::new(), &fresh, &index);
        assert_eq!(resolved.parent, ParentUpdate::Unresolved);
        assert!(!resolved.clear_live_parent);
    }

    #[test]
    fn filesystem_rename_hint_relocates_a_folder_whose_name_changed() {
        let original = load(&bundled(
            "  - info: {name: Users, type: folder}\n    items: []\n",
        ));
        let anchor = super::FolderIndex::build(&original)
            .anchor("items/0")
            .unwrap();
        let renamed = load(&bundled(
            "  - info: {name: People, type: folder}\n    items: []\n",
        ));
        let remaps = BTreeMap::from([("items/0".to_owned(), "items/0".to_owned())]);
        assert_eq!(
            super::relocate_folder(
                &anchor,
                &remaps,
                &renamed,
                &super::FolderIndex::build(&renamed)
            )
            .as_deref(),
            Some("items/0")
        );
        assert_eq!(
            super::FolderIndex::build(&renamed).resolve(&anchor),
            ResolvedFolder::Unresolved
        );
    }

    #[test]
    fn a_remap_onto_a_request_falls_through_to_folder_identity() {
        let original = load(&bundled(&format!("{BEFORE}{TARGET}")));
        let anchor = super::FolderIndex::build(&original)
            .anchor("items/1")
            .unwrap();
        let fresh = load(&bundled(
            "  - info: {name: Kept, type: http}\n    http: {method: GET, url: https://example.test}\n  - info: {name: Target, type: folder}\n    items:\n      - info: {name: Marker, type: http}\n        http: {method: GET, url: https://example.test/marker}\n",
        ));
        let remaps = BTreeMap::from([("items/1".to_owned(), "items/0".to_owned())]);
        assert!(fresh.folder_key("items/0").is_none());
        assert_eq!(
            super::relocate_folder(&anchor, &remaps, &fresh, &super::FolderIndex::build(&fresh))
                .as_deref(),
            Some("items/1")
        );
    }
}
