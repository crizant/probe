use crate::{
    Collection, CollectionItem, CollectionMetadata, Environment, ItemMetadata, Request,
    arena::{Arena, ArenaKey},
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{error::Error, fmt};

static NEXT_WORKSPACE_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_workspace_generation() -> u64 {
    NEXT_WORKSPACE_GENERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |generation| {
            generation.checked_add(1)
        })
        .expect("workspace generation exhausted")
}

/// Session-only generational key for an HTTP request in a loaded workspace.
///
/// Keys are rebuilt whenever a workspace is loaded and are never serialized. If a
/// deleted request's slot is reused, its replacement receives a different generation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestKey {
    workspace_generation: u64,
    slot: usize,
    generation: u64,
}

impl RequestKey {
    fn in_workspace(workspace_generation: u64, key: ArenaKey) -> Self {
        Self {
            workspace_generation,
            slot: key.slot,
            generation: key.generation,
        }
    }

    /// Returns the generation of the loaded workspace that issued this key.
    #[must_use]
    pub const fn workspace_generation(self) -> u64 {
        self.workspace_generation
    }

    /// Returns the workspace-local storage slot.
    #[must_use]
    pub const fn slot(self) -> usize {
        self.slot
    }

    /// Returns the slot generation used to reject stale keys.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// Session-only generational key for a folder in a loaded workspace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FolderKey {
    workspace_generation: u64,
    slot: usize,
    generation: u64,
}

impl FolderKey {
    fn in_workspace(workspace_generation: u64, key: ArenaKey) -> Self {
        Self {
            workspace_generation,
            slot: key.slot,
            generation: key.generation,
        }
    }

    /// Returns the generation of the loaded workspace that issued this key.
    #[must_use]
    pub const fn workspace_generation(self) -> u64 {
        self.workspace_generation
    }

    /// Returns the workspace-local storage slot.
    #[must_use]
    pub const fn slot(self) -> usize {
        self.slot
    }

    /// Returns the slot generation used to reject stale keys.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// A request or folder reference used to retain collection ordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceItemRef {
    /// A folder reference.
    Folder(FolderKey),
    /// An HTTP request reference.
    Request(RequestKey),
}

/// Indexed folder metadata and ordered children.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceFolder {
    /// Session-only folder key.
    pub key: FolderKey,
    /// Folder metadata.
    pub metadata: ItemMetadata,
    /// Ordered direct children.
    pub children: Vec<WorkspaceItemRef>,
}

/// An active, fully in-memory workspace.
///
/// Requests and folders are stored once in generational arenas. Selecting an item by
/// key requires no filesystem access, parsing, database query, or network operation.
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    workspace_generation: u64,
    metadata: CollectionMetadata,
    root_items: Vec<WorkspaceItemRef>,
    requests: Arena<Request>,
    folders: Arena<WorkspaceFolder>,
    request_ancestors: BTreeMap<RequestKey, Vec<FolderKey>>,
    environments: Vec<Environment>,
}

/// A parent location for a structural workspace edit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceParent {
    /// The collection root.
    Root,
    /// A folder in the loaded workspace.
    Folder(FolderKey),
}

/// Errors produced by in-memory hierarchy operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceEditError {
    /// A runtime key does not resolve in this workspace.
    ItemNotFound,
    /// The destination folder does not resolve.
    DestinationNotFound,
    /// A folder cannot be moved into itself or one of its descendants.
    InvalidDestination,
    /// The requested insertion index is greater than the child count.
    InvalidIndex,
}

impl fmt::Display for WorkspaceEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ItemNotFound => "workspace item not found",
            Self::DestinationNotFound => "destination folder not found",
            Self::InvalidDestination => "folder cannot be moved into itself or its descendant",
            Self::InvalidIndex => "insertion index is out of bounds",
        })
    }
}

impl Error for WorkspaceEditError {}

impl Workspace {
    /// Builds an indexed workspace from a domain collection.
    #[must_use]
    pub fn from_collection(collection: Collection) -> Self {
        let workspace_generation = next_workspace_generation();
        let mut requests = Arena::default();
        let mut folders = Arena::default();
        let mut request_ancestors = BTreeMap::new();
        let root_items = index_items(
            collection.items,
            workspace_generation,
            &mut requests,
            &mut folders,
            &mut request_ancestors,
            &[],
        );

        Self {
            workspace_generation,
            metadata: collection.metadata,
            root_items,
            requests,
            folders,
            request_ancestors,
            environments: collection.environments,
        }
    }

    /// Returns collection metadata.
    #[must_use]
    pub const fn metadata(&self) -> &CollectionMetadata {
        &self.metadata
    }

    /// Returns the ordered items at the workspace root.
    #[must_use]
    pub fn root_items(&self) -> &[WorkspaceItemRef] {
        &self.root_items
    }

    /// Looks up a request in constant time, rejecting stale generations.
    #[must_use]
    pub fn request(&self, key: RequestKey) -> Option<&Request> {
        if key.workspace_generation != self.workspace_generation {
            return None;
        }
        self.requests.get(key.into())
    }

    /// Mutably looks up a request in constant time, rejecting stale generations.
    pub fn request_mut(&mut self, key: RequestKey) -> Option<&mut Request> {
        if key.workspace_generation != self.workspace_generation {
            return None;
        }
        self.requests.get_mut(key.into())
    }

    /// Returns the number of live requests.
    #[must_use]
    pub const fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Adds a request at the workspace root and returns its new runtime key.
    pub fn add_root_request(&mut self, request: Request) -> RequestKey {
        let key =
            RequestKey::in_workspace(self.workspace_generation, self.requests.insert(request));
        self.request_ancestors.insert(key, Vec::new());
        self.root_items.push(WorkspaceItemRef::Request(key));
        key
    }

    /// Retains an editor draft without adding it to the collection hierarchy.
    /// The returned key is valid for the lifetime of this workspace only.
    pub fn add_detached_request(&mut self, request: Request) -> RequestKey {
        RequestKey::in_workspace(self.workspace_generation, self.requests.insert(request))
    }

    /// Inserts a request at an exact position under a parent.
    pub fn insert_request(
        &mut self,
        parent: WorkspaceParent,
        index: usize,
        request: Request,
    ) -> Result<RequestKey, WorkspaceEditError> {
        self.validate_insertion(parent, index)?;
        let key =
            RequestKey::in_workspace(self.workspace_generation, self.requests.insert(request));
        self.insert_reference(parent, index, WorkspaceItemRef::Request(key))?;
        self.rebuild_request_ancestors();
        Ok(key)
    }

    /// Inserts an empty folder at an exact position under a parent.
    pub fn insert_folder(
        &mut self,
        parent: WorkspaceParent,
        index: usize,
        metadata: ItemMetadata,
    ) -> Result<FolderKey, WorkspaceEditError> {
        self.validate_insertion(parent, index)?;
        let workspace_generation = self.workspace_generation;
        let arena_key = self.folders.insert_with_key(|arena_key| WorkspaceFolder {
            key: FolderKey::in_workspace(workspace_generation, arena_key),
            metadata,
            children: Vec::new(),
        });
        let key = FolderKey::in_workspace(workspace_generation, arena_key);
        self.insert_reference(parent, index, WorkspaceItemRef::Folder(key))?;
        Ok(key)
    }

    /// Renames a request in memory.
    pub fn rename_request(
        &mut self,
        key: RequestKey,
        name: String,
    ) -> Result<(), WorkspaceEditError> {
        self.request_mut(key)
            .ok_or(WorkspaceEditError::ItemNotFound)?
            .metadata
            .name = Some(name);
        Ok(())
    }

    /// Renames a folder in memory.
    pub fn rename_folder(
        &mut self,
        key: FolderKey,
        name: String,
    ) -> Result<(), WorkspaceEditError> {
        if self.folder(key).is_none() {
            return Err(WorkspaceEditError::ItemNotFound);
        }
        self.folders
            .get_mut(key.into())
            .ok_or(WorkspaceEditError::ItemNotFound)?
            .metadata
            .name = Some(name);
        Ok(())
    }

    /// Moves or reorders a request under a parent.
    pub fn move_request(
        &mut self,
        key: RequestKey,
        parent: WorkspaceParent,
        index: usize,
    ) -> Result<(), WorkspaceEditError> {
        if self.request(key).is_none() {
            return Err(WorkspaceEditError::ItemNotFound);
        }
        self.move_reference(WorkspaceItemRef::Request(key), parent, index)?;
        self.rebuild_request_ancestors();
        Ok(())
    }

    /// Moves or reorders a folder under a parent.
    pub fn move_folder(
        &mut self,
        key: FolderKey,
        parent: WorkspaceParent,
        index: usize,
    ) -> Result<(), WorkspaceEditError> {
        if self.folder(key).is_none() {
            return Err(WorkspaceEditError::ItemNotFound);
        }
        if parent == WorkspaceParent::Folder(key)
            || matches!(parent, WorkspaceParent::Folder(destination) if self.folder_contains(key, destination))
        {
            return Err(WorkspaceEditError::InvalidDestination);
        }
        self.move_reference(WorkspaceItemRef::Folder(key), parent, index)?;
        self.rebuild_request_ancestors();
        Ok(())
    }

    /// Removes a folder and all descendant folders and requests.
    pub fn remove_folder(&mut self, key: FolderKey) -> Result<WorkspaceFolder, WorkspaceEditError> {
        if self.folder(key).is_none() {
            return Err(WorkspaceEditError::ItemNotFound);
        }
        self.remove_reference(WorkspaceItemRef::Folder(key))
            .ok_or(WorkspaceEditError::ItemNotFound)?;
        let removed = self
            .folders
            .remove(key.into())
            .expect("validated folder key must remain live");
        self.remove_descendants(&removed.children);
        self.rebuild_request_ancestors();
        Ok(removed)
    }

    /// Removes a request and every hierarchy reference to it.
    ///
    /// A later request may reuse the storage slot, but receives a new generation so
    /// the removed key can never resolve to the replacement.
    pub fn remove_request(&mut self, key: RequestKey) -> Option<Request> {
        if key.workspace_generation != self.workspace_generation {
            return None;
        }
        let request = self.requests.remove(key.into())?;
        self.request_ancestors.remove(&key);
        self.root_items
            .retain(|item| *item != WorkspaceItemRef::Request(key));
        for folder in self.folders.values_mut() {
            folder
                .children
                .retain(|item| *item != WorkspaceItemRef::Request(key));
        }
        Some(request)
    }

    /// Looks up a folder in constant time, rejecting stale generations.
    #[must_use]
    pub fn folder(&self, key: FolderKey) -> Option<&WorkspaceFolder> {
        if key.workspace_generation != self.workspace_generation {
            return None;
        }
        self.folders.get(key.into())
    }

    /// Returns the number of live folders.
    #[must_use]
    pub const fn folder_count(&self) -> usize {
        self.folders.len()
    }

    /// Returns a request's ancestor folder keys from the collection root inward.
    ///
    /// The path is indexed when the workspace is built, so request selection and
    /// presentation do not need to scan the collection tree.
    #[must_use]
    pub fn request_ancestor_folders(&self, key: RequestKey) -> Option<&[FolderKey]> {
        self.request_ancestors.get(&key).map(Vec::as_slice)
    }

    /// Returns collection environments in source order.
    #[must_use]
    pub fn environments(&self) -> &[Environment] {
        &self.environments
    }

    /// Creates a new environment with an optional parent.
    pub fn create_environment(
        &mut self,
        name: String,
        extends: Option<String>,
    ) -> Result<(), crate::EnvironmentResolutionError> {
        crate::create_environment(&mut self.environments, name, extends)
    }

    /// Removes a newly created environment that has not been persisted.
    pub fn revert_created_environment(&mut self, name: &str) {
        crate::revert_created_environment(&mut self.environments, name);
    }

    /// Replaces one environment and revalidates the inheritance graph.
    pub fn replace_environment(
        &mut self,
        original_name: &str,
        replacement: crate::Environment,
    ) -> Result<(), crate::EnvironmentResolutionError> {
        crate::replace_environment(&mut self.environments, original_name, replacement)
    }

    /// Returns effective plain variables for `selected`, including inherited values.
    #[must_use]
    pub fn effective_environment_variables(
        &self,
        selected: &crate::Environment,
    ) -> Vec<crate::EffectiveEnvironmentVariable> {
        crate::effective_environment_variables(&self.environments, selected)
    }

    /// Deletes an environment that has no children.
    pub fn delete_environment(
        &mut self,
        name: &str,
    ) -> Result<crate::Environment, crate::EnvironmentResolutionError> {
        crate::delete_environment(&mut self.environments, name)
    }

    /// Updates a plain variable on the named environment, or adds an override.
    pub fn set_environment_variable(
        &mut self,
        environment_name: &str,
        variable_name: &str,
        value: String,
    ) -> Result<(), crate::EnvironmentResolutionError> {
        crate::set_environment_variable(
            &mut self.environments,
            environment_name,
            variable_name,
            value,
        )
    }

    /// Removes a plain variable from the named environment only.
    pub fn unset_environment_variable(
        &mut self,
        environment_name: &str,
        variable_name: &str,
    ) -> Result<(), crate::EnvironmentResolutionError> {
        crate::unset_environment_variable(&mut self.environments, environment_name, variable_name)
    }

    fn children(&self, parent: WorkspaceParent) -> Result<&[WorkspaceItemRef], WorkspaceEditError> {
        match parent {
            WorkspaceParent::Root => Ok(&self.root_items),
            WorkspaceParent::Folder(key) => self
                .folder(key)
                .map(|folder| folder.children.as_slice())
                .ok_or(WorkspaceEditError::DestinationNotFound),
        }
    }

    fn children_mut(
        &mut self,
        parent: WorkspaceParent,
    ) -> Result<&mut Vec<WorkspaceItemRef>, WorkspaceEditError> {
        match parent {
            WorkspaceParent::Root => Ok(&mut self.root_items),
            WorkspaceParent::Folder(key) => {
                if key.workspace_generation != self.workspace_generation {
                    return Err(WorkspaceEditError::DestinationNotFound);
                }
                self.folders
                    .get_mut(key.into())
                    .map(|folder| &mut folder.children)
                    .ok_or(WorkspaceEditError::DestinationNotFound)
            }
        }
    }

    fn validate_insertion(
        &self,
        parent: WorkspaceParent,
        index: usize,
    ) -> Result<(), WorkspaceEditError> {
        if index > self.children(parent)?.len() {
            return Err(WorkspaceEditError::InvalidIndex);
        }
        Ok(())
    }

    fn insert_reference(
        &mut self,
        parent: WorkspaceParent,
        index: usize,
        item: WorkspaceItemRef,
    ) -> Result<(), WorkspaceEditError> {
        let children = self.children_mut(parent)?;
        if index > children.len() {
            return Err(WorkspaceEditError::InvalidIndex);
        }
        children.insert(index, item);
        Ok(())
    }

    fn remove_reference(&mut self, item: WorkspaceItemRef) -> Option<(WorkspaceParent, usize)> {
        if let Some(index) = self
            .root_items
            .iter()
            .position(|candidate| *candidate == item)
        {
            self.root_items.remove(index);
            return Some((WorkspaceParent::Root, index));
        }
        for folder in self.folders.values_mut() {
            if let Some(index) = folder
                .children
                .iter()
                .position(|candidate| *candidate == item)
            {
                folder.children.remove(index);
                return Some((WorkspaceParent::Folder(folder.key), index));
            }
        }
        None
    }

    fn move_reference(
        &mut self,
        item: WorkspaceItemRef,
        parent: WorkspaceParent,
        index: usize,
    ) -> Result<(), WorkspaceEditError> {
        let destination_len = self.children(parent)?.len();
        let (old_parent, old_index) = self
            .reference_location(item)
            .ok_or(WorkspaceEditError::ItemNotFound)?;
        let destination_len = destination_len - usize::from(old_parent == parent);
        if index > destination_len {
            return Err(WorkspaceEditError::InvalidIndex);
        }
        self.children_mut(old_parent)?.remove(old_index);
        self.children_mut(parent)?.insert(index, item);
        Ok(())
    }

    fn reference_location(&self, item: WorkspaceItemRef) -> Option<(WorkspaceParent, usize)> {
        if let Some(index) = self
            .root_items
            .iter()
            .position(|candidate| *candidate == item)
        {
            return Some((WorkspaceParent::Root, index));
        }
        self.folders.values().find_map(|folder| {
            folder
                .children
                .iter()
                .position(|candidate| *candidate == item)
                .map(|index| (WorkspaceParent::Folder(folder.key), index))
        })
    }

    fn folder_contains(&self, ancestor: FolderKey, candidate: FolderKey) -> bool {
        self.folder(ancestor).is_some_and(|folder| {
            folder.children.iter().any(|item| match item {
                WorkspaceItemRef::Folder(child) => {
                    *child == candidate || self.folder_contains(*child, candidate)
                }
                WorkspaceItemRef::Request(_) => false,
            })
        })
    }

    fn remove_descendants(&mut self, items: &[WorkspaceItemRef]) {
        for item in items {
            match *item {
                WorkspaceItemRef::Request(key) => {
                    let _ = self.requests.remove(key.into());
                    self.request_ancestors.remove(&key);
                }
                WorkspaceItemRef::Folder(key) => {
                    if let Some(folder) = self.folders.remove(key.into()) {
                        self.remove_descendants(&folder.children);
                    }
                }
            }
        }
    }

    fn rebuild_request_ancestors(&mut self) {
        let mut ancestors = BTreeMap::new();
        collect_request_ancestors(
            &self.folders,
            &self.root_items,
            &mut ancestors,
            &mut Vec::new(),
        );
        self.request_ancestors = ancestors;
    }
}

fn collect_request_ancestors(
    folders: &Arena<WorkspaceFolder>,
    items: &[WorkspaceItemRef],
    output: &mut BTreeMap<RequestKey, Vec<FolderKey>>,
    path: &mut Vec<FolderKey>,
) {
    for item in items {
        match *item {
            WorkspaceItemRef::Request(key) => {
                output.insert(key, path.clone());
            }
            WorkspaceItemRef::Folder(key) => {
                if let Some(folder) = folders.get(key.into()) {
                    path.push(key);
                    collect_request_ancestors(folders, &folder.children, output, path);
                    path.pop();
                }
            }
        }
    }
}

impl From<RequestKey> for ArenaKey {
    fn from(key: RequestKey) -> Self {
        Self {
            slot: key.slot,
            generation: key.generation,
        }
    }
}

impl From<FolderKey> for ArenaKey {
    fn from(key: FolderKey) -> Self {
        Self {
            slot: key.slot,
            generation: key.generation,
        }
    }
}

fn index_items(
    items: Vec<CollectionItem>,
    workspace_generation: u64,
    requests: &mut Arena<Request>,
    folders: &mut Arena<WorkspaceFolder>,
    request_ancestors: &mut BTreeMap<RequestKey, Vec<FolderKey>>,
    ancestors: &[FolderKey],
) -> Vec<WorkspaceItemRef> {
    items
        .into_iter()
        .map(|item| {
            index_item(
                item,
                workspace_generation,
                requests,
                folders,
                request_ancestors,
                ancestors,
            )
        })
        .collect()
}

fn index_item(
    item: CollectionItem,
    workspace_generation: u64,
    requests: &mut Arena<Request>,
    folders: &mut Arena<WorkspaceFolder>,
    request_ancestors: &mut BTreeMap<RequestKey, Vec<FolderKey>>,
    ancestors: &[FolderKey],
) -> WorkspaceItemRef {
    match item {
        CollectionItem::Request(request) => {
            let key = RequestKey::in_workspace(workspace_generation, requests.insert(request));
            request_ancestors.insert(key, ancestors.to_vec());
            WorkspaceItemRef::Request(key)
        }
        CollectionItem::Folder(folder) => {
            let arena_key = folders.insert_with_key(|arena_key| WorkspaceFolder {
                key: FolderKey::in_workspace(workspace_generation, arena_key),
                metadata: folder.metadata,
                children: Vec::new(),
            });
            let key = FolderKey::in_workspace(workspace_generation, arena_key);
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(key);
            let children = index_items(
                folder.items,
                workspace_generation,
                requests,
                folders,
                request_ancestors,
                &child_ancestors,
            );
            let indexed_folder = folders
                .get_mut(arena_key)
                .expect("newly inserted folder key must remain valid");
            indexed_folder.key = key;
            indexed_folder.children = children;
            WorkspaceItemRef::Folder(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Collection, CollectionItem, CollectionMetadata, Folder, ItemMetadata, Request};

    use super::{Workspace, WorkspaceEditError, WorkspaceItemRef, WorkspaceParent};

    fn request(name: &str) -> CollectionItem {
        CollectionItem::Request(http_request(name))
    }

    fn http_request(name: &str) -> Request {
        Request {
            metadata: ItemMetadata {
                name: Some(name.to_owned()),
                sequence: None,
            },
            ..Request::default()
        }
    }

    #[test]
    fn folder_keys_do_not_cross_workspace_generations() {
        let collection = || Collection {
            items: vec![CollectionItem::Folder(Folder {
                metadata: ItemMetadata::default(),
                items: Vec::new(),
            })],
            ..Collection::default()
        };
        let first = Workspace::from_collection(collection());
        let mut second = Workspace::from_collection(collection());
        let WorkspaceItemRef::Folder(key) = first.root_items()[0] else {
            unreachable!()
        };
        assert!(second.folder(key).is_none());
        assert_eq!(
            second.rename_folder(key, "wrong".into()),
            Err(WorkspaceEditError::ItemNotFound)
        );
        assert_eq!(
            second.remove_folder(key),
            Err(WorkspaceEditError::ItemNotFound)
        );
        assert_eq!(
            second.insert_request(WorkspaceParent::Folder(key), 0, Request::default()),
            Err(WorkspaceEditError::DestinationNotFound)
        );
    }

    #[test]
    fn indexes_nested_items_and_preserves_order() {
        let collection = Collection {
            metadata: CollectionMetadata {
                name: Some("Example".to_owned()),
                ..CollectionMetadata::default()
            },
            items: vec![
                CollectionItem::Folder(Folder {
                    metadata: ItemMetadata {
                        name: Some("Users".to_owned()),
                        sequence: Some(1.0),
                    },
                    items: vec![request("List users")],
                }),
                request("Health"),
            ],
            environments: Vec::new(),
        };

        let workspace = Workspace::from_collection(collection);
        let WorkspaceItemRef::Folder(folder_key) = workspace.root_items()[0] else {
            panic!("first root item should be a folder");
        };
        let WorkspaceItemRef::Request(health_key) = workspace.root_items()[1] else {
            panic!("second root item should be a request");
        };
        let folder = workspace
            .folder(folder_key)
            .expect("folder key should resolve");
        let WorkspaceItemRef::Request(list_users_key) = folder.children[0] else {
            panic!("folder child should be a request");
        };

        assert_eq!(workspace.request_count(), 2);
        assert_eq!(workspace.folder_count(), 1);
        assert_eq!(
            workspace
                .request(list_users_key)
                .and_then(|request| request.metadata.name.as_deref()),
            Some("List users")
        );
        assert_eq!(
            workspace
                .request(health_key)
                .and_then(|request| request.metadata.name.as_deref()),
            Some("Health")
        );
    }

    #[test]
    fn stale_request_key_cannot_resolve_reused_slot() {
        let mut workspace = Workspace::from_collection(Collection {
            items: vec![request("Request X")],
            ..Collection::default()
        });
        let WorkspaceItemRef::Request(request_x_key) = workspace.root_items()[0] else {
            panic!("root item should be a request");
        };

        let removed = workspace
            .remove_request(request_x_key)
            .expect("request X should be removed");
        assert_eq!(removed.metadata.name.as_deref(), Some("Request X"));
        let request_y_key = workspace.add_root_request(http_request("Request Y"));

        assert_eq!(request_x_key.slot(), request_y_key.slot());
        assert_ne!(request_x_key.generation(), request_y_key.generation());
        assert!(workspace.request(request_x_key).is_none());
        assert_eq!(
            workspace
                .request(request_y_key)
                .and_then(|request| request.metadata.name.as_deref()),
            Some("Request Y")
        );
        assert_eq!(
            workspace.root_items(),
            [WorkspaceItemRef::Request(request_y_key)]
        );
    }

    #[test]
    fn request_keys_are_scoped_to_their_loaded_workspace() {
        let first = Workspace::from_collection(Collection {
            items: vec![request("First")],
            ..Collection::default()
        });
        let mut second = Workspace::from_collection(Collection {
            items: vec![request("Second")],
            ..Collection::default()
        });
        let WorkspaceItemRef::Request(first_key) = first.root_items()[0] else {
            unreachable!()
        };
        let WorkspaceItemRef::Request(second_key) = second.root_items()[0] else {
            unreachable!()
        };

        assert_eq!(
            (first_key.slot(), first_key.generation()),
            (second_key.slot(), second_key.generation())
        );
        assert_ne!(
            first_key.workspace_generation(),
            second_key.workspace_generation()
        );
        assert!(second.request(first_key).is_none());
        assert!(second.request_mut(first_key).is_none());
        assert!(second.remove_request(first_key).is_none());
        assert_eq!(
            second
                .request(second_key)
                .and_then(|request| request.metadata.name.as_deref()),
            Some("Second")
        );
    }

    #[test]
    fn structural_operations_preserve_hierarchy_and_reject_folder_cycles() {
        let mut workspace = Workspace::from_collection(Collection {
            items: vec![request("Root")],
            ..Collection::default()
        });
        let request_key = match workspace.root_items()[0] {
            WorkspaceItemRef::Request(key) => key,
            WorkspaceItemRef::Folder(_) => unreachable!(),
        };
        let folder_key = workspace
            .insert_folder(
                WorkspaceParent::Root,
                0,
                ItemMetadata {
                    name: Some("Folder".to_owned()),
                    sequence: None,
                },
            )
            .unwrap();
        let child_key = workspace
            .insert_folder(
                WorkspaceParent::Folder(folder_key),
                0,
                ItemMetadata {
                    name: Some("Child".to_owned()),
                    sequence: None,
                },
            )
            .unwrap();
        workspace
            .move_request(request_key, WorkspaceParent::Folder(child_key), 0)
            .unwrap();
        assert_eq!(
            workspace.request_ancestor_folders(request_key),
            Some([folder_key, child_key].as_slice())
        );
        workspace
            .move_folder(child_key, WorkspaceParent::Root, 0)
            .unwrap();
        assert_eq!(
            workspace.request_ancestor_folders(request_key),
            Some([child_key].as_slice())
        );
        workspace
            .move_folder(child_key, WorkspaceParent::Folder(folder_key), 0)
            .unwrap();
        assert_eq!(
            workspace.move_folder(folder_key, WorkspaceParent::Folder(child_key), 0),
            Err(WorkspaceEditError::InvalidDestination)
        );
        workspace
            .rename_folder(child_key, "Renamed".to_owned())
            .unwrap();
        workspace.remove_folder(folder_key).unwrap();
        assert!(workspace.request(request_key).is_none());
        assert_eq!(workspace.folder_count(), 0);
        assert_eq!(workspace.request_count(), 0);
    }

    #[test]
    fn invalid_move_destination_does_not_detach_the_item() {
        let mut workspace = Workspace::from_collection(Collection {
            items: vec![request("Root"), CollectionItem::Folder(Folder::default())],
            ..Collection::default()
        });
        let WorkspaceItemRef::Request(request_key) = workspace.root_items()[0] else {
            panic!("first item should be a request");
        };
        let WorkspaceItemRef::Folder(folder_key) = workspace.root_items()[1] else {
            panic!("second item should be a folder");
        };
        workspace.remove_folder(folder_key).unwrap();

        assert_eq!(
            workspace.move_request(request_key, WorkspaceParent::Folder(folder_key), 0),
            Err(WorkspaceEditError::DestinationNotFound)
        );
        assert_eq!(
            workspace.root_items(),
            [WorkspaceItemRef::Request(request_key)]
        );
    }
}
