use crate::{
    Collection, CollectionItem, CollectionMetadata, Environment, HttpRequest, ItemMetadata,
};

/// Session-only generational key for an HTTP request in a loaded workspace.
///
/// Keys are rebuilt whenever a workspace is loaded and are never serialized. If a
/// deleted request's slot is reused, its replacement receives a different generation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestKey {
    slot: usize,
    generation: u64,
}

impl RequestKey {
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
    slot: usize,
    generation: u64,
}

impl FolderKey {
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
    metadata: CollectionMetadata,
    root_items: Vec<WorkspaceItemRef>,
    requests: Arena<HttpRequest>,
    folders: Arena<WorkspaceFolder>,
    environments: Vec<Environment>,
}

impl Workspace {
    /// Builds an indexed workspace from a domain collection.
    #[must_use]
    pub fn from_collection(collection: Collection) -> Self {
        let mut requests = Arena::default();
        let mut folders = Arena::default();
        let root_items = index_items(collection.items, &mut requests, &mut folders);

        Self {
            metadata: collection.metadata,
            root_items,
            requests,
            folders,
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
    pub fn request(&self, key: RequestKey) -> Option<&HttpRequest> {
        self.requests.get(key.into())
    }

    /// Mutably looks up a request in constant time, rejecting stale generations.
    pub fn request_mut(&mut self, key: RequestKey) -> Option<&mut HttpRequest> {
        self.requests.get_mut(key.into())
    }

    /// Returns the number of live requests.
    #[must_use]
    pub const fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Adds a request at the workspace root and returns its new runtime key.
    pub fn add_root_request(&mut self, request: HttpRequest) -> RequestKey {
        let key = RequestKey::from(self.requests.insert(request));
        self.root_items.push(WorkspaceItemRef::Request(key));
        key
    }

    /// Removes a request and every hierarchy reference to it.
    ///
    /// A later request may reuse the storage slot, but receives a new generation so
    /// the removed key can never resolve to the replacement.
    pub fn remove_request(&mut self, key: RequestKey) -> Option<HttpRequest> {
        let request = self.requests.remove(key.into())?;
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
        self.folders.get(key.into())
    }

    /// Returns the number of live folders.
    #[must_use]
    pub const fn folder_count(&self) -> usize {
        self.folders.len()
    }

    /// Returns collection environments in source order.
    #[must_use]
    pub fn environments(&self) -> &[Environment] {
        &self.environments
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ArenaKey {
    slot: usize,
    generation: u64,
}

impl From<ArenaKey> for RequestKey {
    fn from(key: ArenaKey) -> Self {
        Self {
            slot: key.slot,
            generation: key.generation,
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

impl From<ArenaKey> for FolderKey {
    fn from(key: ArenaKey) -> Self {
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

#[derive(Clone, Debug, PartialEq)]
struct Arena<T> {
    slots: Vec<ArenaSlot<T>>,
    free_head: Option<usize>,
    len: usize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            len: 0,
        }
    }
}

impl<T> Arena<T> {
    const fn len(&self) -> usize {
        self.len
    }

    fn insert(&mut self, value: T) -> ArenaKey {
        self.len += 1;

        if let Some(slot_index) = self.free_head {
            let slot = &mut self.slots[slot_index];
            self.free_head = slot.next_free.take();
            slot.value = Some(value);
            ArenaKey {
                slot: slot_index,
                generation: slot.generation,
            }
        } else {
            let key = ArenaKey {
                slot: self.slots.len(),
                generation: 0,
            };
            self.slots.push(ArenaSlot {
                generation: key.generation,
                value: Some(value),
                next_free: None,
            });
            key
        }
    }

    fn get(&self, key: ArenaKey) -> Option<&T> {
        let slot = self.slots.get(key.slot)?;
        if slot.generation != key.generation {
            return None;
        }
        slot.value.as_ref()
    }

    fn get_mut(&mut self, key: ArenaKey) -> Option<&mut T> {
        let slot = self.slots.get_mut(key.slot)?;
        if slot.generation != key.generation {
            return None;
        }
        slot.value.as_mut()
    }

    fn remove(&mut self, key: ArenaKey) -> Option<T> {
        let slot = self.slots.get_mut(key.slot)?;
        if slot.generation != key.generation {
            return None;
        }

        let value = slot.value.take()?;
        if let Some(next_generation) = slot.generation.checked_add(1) {
            slot.generation = next_generation;
            slot.next_free = self.free_head;
            self.free_head = Some(key.slot);
        } else {
            // Retire an exhausted slot rather than allowing a generation to repeat.
            slot.next_free = None;
        }
        self.len -= 1;
        Some(value)
    }

    fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|slot| slot.value.as_mut())
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArenaSlot<T> {
    generation: u64,
    value: Option<T>,
    next_free: Option<usize>,
}

fn index_items(
    items: Vec<CollectionItem>,
    requests: &mut Arena<HttpRequest>,
    folders: &mut Arena<WorkspaceFolder>,
) -> Vec<WorkspaceItemRef> {
    items
        .into_iter()
        .map(|item| index_item(item, requests, folders))
        .collect()
}

fn index_item(
    item: CollectionItem,
    requests: &mut Arena<HttpRequest>,
    folders: &mut Arena<WorkspaceFolder>,
) -> WorkspaceItemRef {
    match item {
        CollectionItem::HttpRequest(request) => {
            WorkspaceItemRef::Request(requests.insert(request).into())
        }
        CollectionItem::Folder(folder) => {
            let placeholder = WorkspaceFolder {
                key: FolderKey {
                    slot: 0,
                    generation: 0,
                },
                metadata: folder.metadata,
                children: Vec::new(),
            };
            let arena_key = folders.insert(placeholder);
            let key = FolderKey::from(arena_key);
            let children = index_items(folder.items, requests, folders);
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
    use crate::{
        Collection, CollectionItem, CollectionMetadata, Folder, HttpRequest, ItemMetadata,
    };

    use super::{Workspace, WorkspaceItemRef};

    fn request(name: &str) -> CollectionItem {
        CollectionItem::HttpRequest(HttpRequest {
            metadata: ItemMetadata {
                name: Some(name.to_owned()),
                sequence: None,
            },
            ..HttpRequest::default()
        })
    }

    fn http_request(name: &str) -> HttpRequest {
        let CollectionItem::HttpRequest(request) = request(name) else {
            unreachable!();
        };
        request
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
}
