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

#[cfg(test)]
mod tests {
    use probe_core::{Collection, CollectionItem, Folder, HttpRequest, ItemMetadata, Workspace};
    use probe_opencollection::{ItemKind, StructureOperation};

    use super::{StructureDialog, descendant_requests, item_position};

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
}
