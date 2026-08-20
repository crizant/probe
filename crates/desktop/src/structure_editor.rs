use probe_core::{FolderKey, Workspace, WorkspaceItemRef};
use probe_opencollection::{ItemKind, StructureOperation};

pub(crate) const ROOT_PARENT: &str = "";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StructureDialogMode {
    CreateRequest,
    CreateFolder,
    Rename { kind: ItemKind, selector: String },
    Move { kind: ItemKind, selector: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StructureDialog {
    pub(crate) mode: StructureDialogMode,
    pub(crate) name: String,
    pub(crate) parent: String,
    pub(crate) index: String,
}

impl StructureDialog {
    pub(crate) fn create_request(parent: Option<String>) -> Self {
        Self {
            mode: StructureDialogMode::CreateRequest,
            name: String::new(),
            parent: parent.unwrap_or_default(),
            index: String::new(),
        }
    }

    pub(crate) fn create_folder(parent: Option<String>) -> Self {
        Self {
            mode: StructureDialogMode::CreateFolder,
            name: String::new(),
            parent: parent.unwrap_or_default(),
            index: String::new(),
        }
    }

    pub(crate) fn rename(kind: ItemKind, selector: String, name: String) -> Self {
        Self {
            mode: StructureDialogMode::Rename { kind, selector },
            name,
            parent: String::new(),
            index: String::new(),
        }
    }

    pub(crate) fn move_item(kind: ItemKind, selector: String, parent: Option<String>) -> Self {
        Self {
            mode: StructureDialogMode::Move { kind, selector },
            name: String::new(),
            parent: parent.unwrap_or_default(),
            index: String::new(),
        }
    }

    pub(crate) const fn title(&self) -> &'static str {
        match self.mode {
            StructureDialogMode::CreateRequest => "New Request",
            StructureDialogMode::CreateFolder => "New Folder",
            StructureDialogMode::Rename { .. } => "Rename",
            StructureDialogMode::Move { .. } => "Move",
        }
    }

    pub(crate) const fn submit_label(&self) -> &'static str {
        match self.mode {
            StructureDialogMode::CreateRequest | StructureDialogMode::CreateFolder => "Create",
            StructureDialogMode::Rename { .. } => "Rename",
            StructureDialogMode::Move { .. } => "Move",
        }
    }

    pub(crate) const fn edits_name(&self) -> bool {
        !matches!(self.mode, StructureDialogMode::Move { .. })
    }

    pub(crate) const fn edits_destination(&self) -> bool {
        matches!(self.mode, StructureDialogMode::Move { .. })
    }

    pub(crate) fn operation(&self) -> Result<StructureOperation, String> {
        let name = self.name.trim();
        let parent = (!self.parent.is_empty()).then(|| self.parent.clone());
        let index = if self.index.trim().is_empty() {
            None
        } else {
            Some(
                self.index
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Position must be a non-negative whole number.".to_owned())?,
            )
        };

        match &self.mode {
            StructureDialogMode::CreateRequest => {
                if name.is_empty() {
                    return Err("Request name is required.".to_owned());
                }
                Ok(StructureOperation::CreateRequest {
                    parent,
                    index: None,
                    name: name.to_owned(),
                    method: Some("GET".to_owned()),
                    url: None,
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
                Ok(match kind {
                    ItemKind::Request => StructureOperation::RenameRequest {
                        selector: selector.clone(),
                        name: name.to_owned(),
                    },
                    ItemKind::Folder => StructureOperation::RenameFolder {
                        selector: selector.clone(),
                        name: name.to_owned(),
                    },
                })
            }
            StructureDialogMode::Move { kind, selector } => Ok(match kind {
                ItemKind::Request => StructureOperation::MoveRequest {
                    selector: selector.clone(),
                    parent,
                    index,
                },
                ItemKind::Folder => StructureOperation::MoveFolder {
                    selector: selector.clone(),
                    parent,
                    index,
                },
            }),
        }
    }
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
        return Some(match kind {
            ItemKind::Request => StructureOperation::ReorderRequest { selector, index },
            ItemKind::Folder => StructureOperation::ReorderFolder { selector, index },
        });
    }
    Some(match kind {
        ItemKind::Request => StructureOperation::MoveRequest {
            selector,
            parent: dest_parent_selector,
            index: Some(index),
        },
        ItemKind::Folder => StructureOperation::MoveFolder {
            selector,
            parent: dest_parent_selector,
            index: Some(index),
        },
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
        Collection, CollectionItem, Folder, HttpRequest, ItemMetadata, Workspace, WorkspaceItemRef,
    };
    use probe_opencollection::{ItemKind, StructureOperation};

    use super::{
        DropReject, DropZone, StructureDialog, descendant_requests, drop_intent, drop_zone,
        hovered_row_index, item_is_within_folder, item_position, structure_operation_for_drop,
        validate_tree_drop, would_duplicate_path,
    };

    #[test]
    fn dialog_builds_typed_operations_and_validates_positions() {
        let mut dialog = StructureDialog::move_item(
            ItemKind::Request,
            "old.yml".to_owned(),
            Some("folder".to_owned()),
        );
        assert_eq!(
            dialog.operation().unwrap(),
            StructureOperation::MoveRequest {
                selector: "old.yml".to_owned(),
                parent: Some("folder".to_owned()),
                index: None,
            },
            "moves should append by default because the source index may be invalid in the destination"
        );
        dialog.index = "2".to_owned();
        assert_eq!(
            dialog.operation().unwrap(),
            StructureOperation::MoveRequest {
                selector: "old.yml".to_owned(),
                parent: Some("folder".to_owned()),
                index: Some(2),
            }
        );
        dialog.index = "-1".to_owned();
        assert!(dialog.operation().is_err());
    }

    #[test]
    fn tree_helpers_find_positions_and_descendant_requests() {
        let workspace = Workspace::from_collection(Collection {
            items: vec![CollectionItem::Folder(Folder {
                metadata: ItemMetadata {
                    name: Some("Group".to_owned()),
                    ..ItemMetadata::default()
                },
                items: vec![CollectionItem::HttpRequest(HttpRequest::default())],
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
                CollectionItem::HttpRequest(HttpRequest {
                    metadata: ItemMetadata {
                        name: Some("Alpha".to_owned()),
                        ..ItemMetadata::default()
                    },
                    ..HttpRequest::default()
                }),
                CollectionItem::Folder(Folder {
                    metadata: ItemMetadata {
                        name: Some("Group".to_owned()),
                        ..ItemMetadata::default()
                    },
                    items: vec![CollectionItem::HttpRequest(HttpRequest {
                        metadata: ItemMetadata {
                            name: Some("Nested".to_owned()),
                            ..ItemMetadata::default()
                        },
                        ..HttpRequest::default()
                    })],
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
            Some(StructureOperation::MoveRequest {
                selector: "items/0".to_owned(),
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
            Some(StructureOperation::ReorderFolder {
                selector: "items/1".to_owned(),
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
}
