use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
#[cfg(any(unix, windows))]
use fs4::FileExt;
use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, CollectionItem, Environment,
    FileReference, FolderKey, FormField, Header, MultipartPart, MultipartPartKind, MultipartValue,
    QueryParameter, RawBodyKind, RequestBody, RequestKey, RequestUpdate, Workspace,
    WorkspaceItemRef, validate_environments,
};
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::{EnvironmentDocument, ParseError, parse, project_item};

/// A request and its repository-backed selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedRequest {
    selector: String,
    key: RequestKey,
    persistence: Option<RequestPersistence>,
}

impl LocatedRequest {
    /// Returns the selector accepted by CLI and repository operations.
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Returns the request's session-only workspace key.
    #[must_use]
    pub const fn key(&self) -> RequestKey {
        self.key
    }
}

/// A folder and its repository-backed selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedFolder {
    selector: String,
    key: FolderKey,
}

impl LocatedFolder {
    /// Returns the stable selector used to restore presentation state.
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Returns the folder's session-only workspace key.
    #[must_use]
    pub const fn key(&self) -> FolderKey {
        self.key
    }
}

/// A loaded OpenCollection workspace and its persistence-locator index.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedWorkspace {
    workspace: Workspace,
    requests: Vec<LocatedRequest>,
    folders: Vec<LocatedFolder>,
    request_keys_by_selector: BTreeMap<String, RequestKey>,
    folder_keys_by_selector: BTreeMap<String, FolderKey>,
    request_selectors_by_key: BTreeMap<RequestKey, String>,
    folder_selectors_by_key: BTreeMap<FolderKey, String>,
    pub(crate) documents: BTreeMap<PathBuf, SourceDocument>,
    pub(crate) source: WorkspaceSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceSource {
    Bundled(PathBuf),
    Unbundled(PathBuf),
    Memory,
}

impl LoadedWorkspace {
    /// Returns the in-memory domain workspace.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Mutably looks up a request in the loaded in-memory workspace.
    ///
    /// Desktop editors use this fast path to apply draft changes immediately. Saving
    /// remains an explicit, separate repository operation.
    pub fn request_mut(&mut self, key: RequestKey) -> Option<&mut probe_core::HttpRequest> {
        self.workspace.request_mut(key)
    }

    /// Returns requests in collection traversal order.
    #[must_use]
    pub fn requests(&self) -> &[LocatedRequest] {
        &self.requests
    }

    /// Returns folders in collection traversal order.
    #[must_use]
    pub fn folders(&self) -> &[LocatedFolder] {
        &self.folders
    }

    /// Resolves a repository-backed selector to a request key.
    #[must_use]
    pub fn request_key(&self, selector: &str) -> Option<RequestKey> {
        self.request_keys_by_selector.get(selector).copied()
    }

    /// Resolves a repository-backed selector to a folder key.
    #[must_use]
    pub fn folder_key(&self, selector: &str) -> Option<FolderKey> {
        self.folder_keys_by_selector.get(selector).copied()
    }

    /// Returns the stable selector for a request key.
    #[must_use]
    pub fn request_selector(&self, key: RequestKey) -> Option<&str> {
        self.request_selectors_by_key.get(&key).map(String::as_str)
    }

    /// Returns the stable selector for a folder key.
    #[must_use]
    pub fn folder_selector(&self, key: FolderKey) -> Option<&str> {
        self.folder_selectors_by_key.get(&key).map(String::as_str)
    }

    /// Applies an update in memory and atomically persists its OpenCollection document.
    ///
    /// The save is rejected if the source file no longer exactly matches the bytes
    /// loaded by this repository instance. On persistence failure, the in-memory
    /// request remains updated so callers can report or retry the dirty state.
    pub fn update_request(
        &mut self,
        selector: &str,
        update: &RequestUpdate,
    ) -> Result<(), SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }

        let located = self
            .requests
            .iter()
            .find(|request| request.selector == selector)
            .cloned()
            .ok_or_else(|| SaveError::RequestNotFound(selector.to_owned()))?;
        let request = self
            .workspace
            .request_mut(located.key)
            .expect("repository request key must resolve");
        update.apply(request);

        let persistence = located.persistence.ok_or(SaveError::ReadOnlySource)?;
        let source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem request must retain its source document");
        let _save_lock = SaveLock::acquire(&persistence.document_path)?;
        let current = fs::read(&persistence.document_path).map_err(|source| SaveError::Io {
            path: persistence.document_path.clone(),
            source,
        })?;
        if current != source.original_source {
            return Err(SaveError::ConcurrentModification(
                persistence.document_path.clone(),
            ));
        }

        let mut document: Value =
            serde_yaml_ng::from_slice(&source.original_source).map_err(|error| {
                SaveError::InvalidDocument(format!("retained source cannot be parsed: {error}"))
            })?;
        let request_document = request_document_mut(&mut document, &persistence.item_path)?;
        apply_request_update(request_document, update)?;
        let serialized = serde_yaml_ng::to_string(&document).map_err(SaveError::Serialize)?;
        atomic_write(
            &persistence.document_path,
            serialized.as_bytes(),
            &source.original_source,
        )?;

        self.documents.insert(
            persistence.document_path,
            SourceDocument {
                original_source: serialized.into_bytes(),
            },
        );
        Ok(())
    }

    /// Captures a request save that can be executed away from the UI thread.
    ///
    /// Preparing is in-memory only. [`PreparedRequestSave::execute`] performs the
    /// conflict check and atomic filesystem write.
    pub fn prepare_request_save(
        &self,
        selector: &str,
        update: RequestUpdate,
    ) -> Result<PreparedRequestSave, SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }
        let persistence = self
            .requests
            .iter()
            .find(|request| request.selector == selector)
            .ok_or_else(|| SaveError::RequestNotFound(selector.to_owned()))?
            .persistence
            .clone()
            .ok_or(SaveError::ReadOnlySource)?;
        let original_source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem request must retain its source document")
            .original_source
            .clone();
        Ok(PreparedRequestSave {
            persistence,
            original_source,
            update,
        })
    }

    /// Refreshes the retained conflict baseline after a prepared save succeeds.
    pub fn complete_request_save(&mut self, saved: CompletedRequestSave) {
        self.documents.insert(
            saved.document_path,
            SourceDocument {
                original_source: saved.serialized_source,
            },
        );
    }
}

/// A filesystem save captured from a loaded workspace for background execution.
#[derive(Debug)]
pub struct PreparedRequestSave {
    persistence: RequestPersistence,
    original_source: Vec<u8>,
    update: RequestUpdate,
}

impl PreparedRequestSave {
    /// Performs the exact-source check and atomic write.
    pub fn execute(self) -> Result<CompletedRequestSave, SaveError> {
        let _save_lock = SaveLock::acquire(&self.persistence.document_path)?;
        let current =
            fs::read(&self.persistence.document_path).map_err(|source| SaveError::Io {
                path: self.persistence.document_path.clone(),
                source,
            })?;
        if current != self.original_source {
            return Err(SaveError::ConcurrentModification(
                self.persistence.document_path,
            ));
        }
        let mut document: Value =
            serde_yaml_ng::from_slice(&self.original_source).map_err(|error| {
                SaveError::InvalidDocument(format!("retained source cannot be parsed: {error}"))
            })?;
        let request = request_document_mut(&mut document, &self.persistence.item_path)?;
        apply_request_update(request, &self.update)?;
        let serialized = serde_yaml_ng::to_string(&document)
            .map_err(SaveError::Serialize)?
            .into_bytes();
        atomic_write(
            &self.persistence.document_path,
            &serialized,
            &self.original_source,
        )?;
        Ok(CompletedRequestSave {
            document_path: self.persistence.document_path,
            serialized_source: serialized,
        })
    }
}

/// The refreshed repository baseline produced by a successful prepared save.
#[derive(Debug)]
pub struct CompletedRequestSave {
    document_path: PathBuf,
    serialized_source: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequestPersistence {
    document_path: PathBuf,
    item_path: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceDocument {
    pub(crate) original_source: Vec<u8>,
}

/// Loads a bundled OpenCollection file or an unbundled collection directory.
pub fn load_workspace(path: impl AsRef<Path>) -> Result<LoadedWorkspace, LoadError> {
    let path = path.as_ref();
    let canonical_path = fs::canonicalize(path).map_err(|source| LoadError::Io {
        path: path.to_owned(),
        source,
    })?;
    if canonical_path.is_dir() {
        load_unbundled(&canonical_path)
    } else {
        load_bundled(&canonical_path)
    }
}

/// Loads a bundled OpenCollection workspace from an in-memory YAML document.
///
/// Structural selectors are identical to selectors produced when the same bundled
/// document is loaded from a file.
pub fn load_workspace_from_str(source: &str) -> Result<LoadedWorkspace, LoadError> {
    load_bundled_source(source, None)
}

/// An error raised while loading an OpenCollection workspace.
#[derive(Debug)]
pub enum LoadError {
    /// A filesystem operation failed.
    Io { path: PathBuf, source: io::Error },
    /// A YAML document failed to parse.
    Parse { path: PathBuf, source: ParseError },
    /// An unbundled collection has no root configuration file.
    MissingRoot(PathBuf),
    /// A collection item has an unsupported shape for its filesystem location.
    InvalidItem { path: PathBuf, message: String },
    /// The document mode does not match how the workspace was opened.
    InvalidMode {
        path: PathBuf,
        expected_bundled: bool,
    },
    /// Cross-document workspace semantics are invalid.
    Validation { path: PathBuf, message: String },
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "failed to parse {}: {source}", path.display())
            }
            Self::MissingRoot(path) => write!(
                formatter,
                "{} does not contain opencollection.yml or opencollection.yaml",
                path.display()
            ),
            Self::InvalidItem { path, message } => {
                write!(formatter, "invalid item {}: {message}", path.display())
            }
            Self::InvalidMode {
                path,
                expected_bundled,
            } => write!(
                formatter,
                "{} declares bundled: {}, but this workspace requires bundled: {}",
                path.display(),
                !expected_bundled,
                expected_bundled
            ),
            Self::Validation { path, message } => {
                write!(formatter, "invalid workspace {}: {message}", path.display())
            }
        }
    }
}

impl Error for LoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::MissingRoot(_)
            | Self::InvalidItem { .. }
            | Self::InvalidMode { .. }
            | Self::Validation { .. } => None,
        }
    }
}

/// An error raised while persisting an OpenCollection request update.
#[derive(Debug)]
pub enum SaveError {
    /// No request matched the repository selector.
    RequestNotFound(String),
    /// The requested update did not contain any changed fields.
    EmptyUpdate,
    /// The workspace came from an in-memory source such as stdin.
    ReadOnlySource,
    /// The source changed after it was loaded and was not overwritten.
    ConcurrentModification(PathBuf),
    /// A retained source document no longer has the expected OpenCollection shape.
    InvalidDocument(String),
    /// YAML serialization failed.
    Serialize(serde_yaml_ng::Error),
    /// An atomic filesystem operation failed.
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestNotFound(selector) => {
                write!(formatter, "request selector not found: {selector}")
            }
            Self::EmptyUpdate => formatter.write_str("request update has no changed fields"),
            Self::ReadOnlySource => {
                formatter.write_str("a workspace loaded from stdin cannot be persisted")
            }
            Self::ConcurrentModification(path) => write!(
                formatter,
                "refusing to overwrite externally modified file: {}",
                path.display()
            ),
            Self::InvalidDocument(message) => {
                write!(
                    formatter,
                    "cannot update retained OpenCollection document: {message}"
                )
            }
            Self::Serialize(source) => {
                write!(formatter, "cannot serialize OpenCollection YAML: {source}")
            }
            Self::Io { path, source } => {
                write!(formatter, "cannot persist {}: {source}", path.display())
            }
        }
    }
}

impl Error for SaveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialize(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::RequestNotFound(_)
            | Self::EmptyUpdate
            | Self::ReadOnlySource
            | Self::ConcurrentModification(_)
            | Self::InvalidDocument(_) => None,
        }
    }
}

#[derive(Debug)]
enum LocatorNode {
    Folder {
        selector: String,
        children: Vec<LocatorNode>,
    },
    Request {
        selector: String,
        persistence: Option<RequestPersistence>,
    },
}

fn load_bundled(path: &Path) -> Result<LoadedWorkspace, LoadError> {
    let source = read_to_string(path)?;
    load_bundled_source(&source, Some(path))
}

fn load_bundled_source(
    source: &str,
    document_path: Option<&Path>,
) -> Result<LoadedWorkspace, LoadError> {
    let source_name = document_path.unwrap_or_else(|| Path::new("<memory>"));
    let parsed = parse(source).map_err(|source| LoadError::Parse {
        path: source_name.to_owned(),
        source,
    })?;
    if !parsed.is_bundled() {
        return Err(LoadError::InvalidMode {
            path: source_name.to_owned(),
            expected_bundled: true,
        });
    }
    let nodes = bundled_locator_nodes(parsed.document(), "items", document_path);
    let workspace = Workspace::from_collection(parsed.into_collection());
    let mut loaded = index_locators(workspace, &nodes);
    if let Some(path) = document_path {
        loaded.documents.insert(
            path.to_owned(),
            SourceDocument {
                original_source: source.as_bytes().to_vec(),
            },
        );
        loaded.source = WorkspaceSource::Bundled(path.to_owned());
    }
    Ok(loaded)
}

fn load_unbundled(root: &Path) -> Result<LoadedWorkspace, LoadError> {
    let root_config = config_file(root, "opencollection")
        .ok_or_else(|| LoadError::MissingRoot(root.to_owned()))?;
    let source = read_to_string(&root_config)?;
    let parsed = parse(&source).map_err(|source| LoadError::Parse {
        path: root_config.clone(),
        source,
    })?;
    if parsed.is_bundled() {
        return Err(LoadError::InvalidMode {
            path: root_config,
            expected_bundled: false,
        });
    }
    let mut collection = parsed.into_collection();
    let mut documents = BTreeMap::new();
    documents.insert(
        root_config.clone(),
        SourceDocument {
            original_source: source.as_bytes().to_vec(),
        },
    );
    let loaded_items = read_items(root, root, "opencollection", &mut documents)?;
    let (items, nodes): (Vec<_>, Vec<_>) = loaded_items.into_iter().unzip();
    collection.items = items;
    collection.environments.extend(read_environments(root)?);
    validate_environments(&collection.environments).map_err(|error| LoadError::Validation {
        path: root.to_owned(),
        message: error.to_string(),
    })?;

    let workspace = Workspace::from_collection(collection);
    let mut loaded = index_locators(workspace, &nodes);
    loaded.documents = documents;
    loaded.source = WorkspaceSource::Unbundled(root.to_owned());
    Ok(loaded)
}

fn read_items(
    directory: &Path,
    root: &Path,
    reserved_stem: &str,
    documents: &mut BTreeMap<PathBuf, SourceDocument>,
) -> Result<Vec<(CollectionItem, LocatorNode)>, LoadError> {
    let mut entries = read_directory(directory)?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut items = Vec::new();

    for entry in entries {
        let file_type = entry.file_type().map_err(|source| LoadError::Io {
            path: entry.path(),
            source,
        })?;
        if file_type.is_symlink() {
            continue;
        }

        let path = entry.path();
        if file_type.is_dir() {
            let Some(folder_config) = config_file(&path, "folder") else {
                continue;
            };
            let read = read_item(&folder_config)?;
            documents.insert(
                folder_config.clone(),
                SourceDocument {
                    original_source: read.original_source.clone(),
                },
            );
            let mut folder = match read.item {
                Some(CollectionItem::Folder(folder)) => folder,
                Some(CollectionItem::HttpRequest(_)) => {
                    return Err(LoadError::InvalidItem {
                        path: folder_config,
                        message: "folder.yml must describe a folder".to_owned(),
                    });
                }
                None => continue,
            };
            let children = read_items(&path, root, "folder", documents)?;
            let (child_items, child_nodes): (Vec<_>, Vec<_>) = children.into_iter().unzip();
            folder.items = child_items;
            items.push((
                CollectionItem::Folder(folder),
                LocatorNode::Folder {
                    selector: relative_selector(root, &path),
                    children: child_nodes,
                },
            ));
        } else if is_yaml_file(&path)
            && path.file_stem().and_then(|stem| stem.to_str()) != Some(reserved_stem)
        {
            let read = read_item(&path)?;
            documents.insert(
                path.clone(),
                SourceDocument {
                    original_source: read.original_source.clone(),
                },
            );
            if let Some(item) = read.item {
                match item {
                    CollectionItem::HttpRequest(request) => {
                        let selector = relative_selector(root, &path);
                        items.push((
                            CollectionItem::HttpRequest(request),
                            LocatorNode::Request {
                                selector,
                                persistence: Some(RequestPersistence {
                                    document_path: path,
                                    item_path: Vec::new(),
                                }),
                            },
                        ));
                    }
                    CollectionItem::Folder(_) => {
                        return Err(LoadError::InvalidItem {
                            path,
                            message: "folders must be represented by directories with folder.yml"
                                .to_owned(),
                        });
                    }
                }
            }
        }
    }

    items.sort_by(|(left, left_node), (right, right_node)| {
        item_sequence(left)
            .total_cmp(&item_sequence(right))
            .then_with(|| locator_selector(left_node).cmp(locator_selector(right_node)))
    });
    Ok(items)
}

struct ReadItem {
    item: Option<CollectionItem>,
    original_source: Vec<u8>,
}

fn read_item(path: &Path) -> Result<ReadItem, LoadError> {
    let source = read_to_string(path)?;
    let value: Value = serde_yaml_ng::from_str(&source).map_err(|source| LoadError::Parse {
        path: path.to_owned(),
        source: ParseError::new(source),
    })?;
    let item = project_item(value).map_err(|source| LoadError::Parse {
        path: path.to_owned(),
        source: ParseError::new(source),
    })?;
    Ok(ReadItem {
        item,
        original_source: source.into_bytes(),
    })
}

fn read_environments(root: &Path) -> Result<Vec<Environment>, LoadError> {
    let directory = root.join("environments");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = read_directory(&directory)?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut environments = Vec::new();
    for entry in entries {
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|source| LoadError::Io {
                path: path.clone(),
                source,
            })?
            .is_file()
            || !is_yaml_file(&path)
        {
            continue;
        }
        let source = read_to_string(&path)?;
        let environment: EnvironmentDocument =
            serde_yaml_ng::from_str(&source).map_err(|source| LoadError::Parse {
                path: path.clone(),
                source: ParseError::new(source),
            })?;
        environments.push(environment.into_domain());
    }
    Ok(environments)
}

#[derive(Deserialize)]
struct LocatorItemsDocument {
    #[serde(default)]
    items: Vec<Value>,
}

#[derive(Default, Deserialize)]
struct LocatorItemDocument {
    #[serde(default)]
    info: LocatorInfoDocument,
    #[serde(default)]
    items: Vec<Value>,
}

#[derive(Default, Deserialize)]
struct LocatorInfoDocument {
    #[serde(rename = "type")]
    item_type: Option<String>,
}

fn bundled_locator_nodes(
    document: &Value,
    prefix: &str,
    document_path: Option<&Path>,
) -> Vec<LocatorNode> {
    let document: LocatorItemsDocument = serde_yaml_ng::from_value(document.clone())
        .expect("successfully parsed document must retain an object root");
    locator_nodes_from_items(document.items, prefix, document_path, &[])
}

fn locator_nodes_from_items(
    items: Vec<Value>,
    prefix: &str,
    document_path: Option<&Path>,
    parent_path: &[usize],
) -> Vec<LocatorNode> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let mut item_path = parent_path.to_vec();
            item_path.push(index);
            let item: LocatorItemDocument = serde_yaml_ng::from_value(value)
                .expect("successfully projected item must retain a valid object shape");
            match item.info.item_type.as_deref() {
                Some("http") => Some(LocatorNode::Request {
                    selector: format!("{prefix}/{index}"),
                    persistence: document_path.map(|path| RequestPersistence {
                        document_path: path.to_owned(),
                        item_path,
                    }),
                }),
                Some("folder") => Some(LocatorNode::Folder {
                    selector: format!("{prefix}/{index}"),
                    children: locator_nodes_from_items(
                        item.items,
                        &format!("{prefix}/{index}/items"),
                        document_path,
                        &item_path,
                    ),
                }),
                _ => None,
            }
        })
        .collect()
}

fn index_locators(workspace: Workspace, nodes: &[LocatorNode]) -> LoadedWorkspace {
    let mut requests = Vec::new();
    let mut folders = Vec::new();
    index_locator_nodes(
        &workspace,
        workspace.root_items(),
        nodes,
        &mut requests,
        &mut folders,
    );
    let request_keys_by_selector = requests
        .iter()
        .map(|request: &LocatedRequest| (request.selector.clone(), request.key))
        .collect();
    let folder_keys_by_selector = folders
        .iter()
        .map(|folder: &LocatedFolder| (folder.selector.clone(), folder.key))
        .collect();
    let request_selectors_by_key = requests
        .iter()
        .map(|request: &LocatedRequest| (request.key, request.selector.clone()))
        .collect();
    let folder_selectors_by_key = folders
        .iter()
        .map(|folder: &LocatedFolder| (folder.key, folder.selector.clone()))
        .collect();
    LoadedWorkspace {
        workspace,
        requests,
        folders,
        request_keys_by_selector,
        folder_keys_by_selector,
        request_selectors_by_key,
        folder_selectors_by_key,
        documents: BTreeMap::new(),
        source: WorkspaceSource::Memory,
    }
}

fn item_sequence(item: &CollectionItem) -> f64 {
    match item {
        CollectionItem::Folder(folder) => folder.metadata.sequence,
        CollectionItem::HttpRequest(request) => request.metadata.sequence,
    }
    .unwrap_or(f64::INFINITY)
}

fn locator_selector(node: &LocatorNode) -> &str {
    match node {
        LocatorNode::Folder { selector, .. } | LocatorNode::Request { selector, .. } => selector,
    }
}

fn index_locator_nodes(
    workspace: &Workspace,
    items: &[WorkspaceItemRef],
    nodes: &[LocatorNode],
    requests: &mut Vec<LocatedRequest>,
    folders: &mut Vec<LocatedFolder>,
) {
    assert_eq!(
        items.len(),
        nodes.len(),
        "locator tree must match workspace"
    );
    for (item, node) in items.iter().zip(nodes) {
        match (item, node) {
            (
                WorkspaceItemRef::Request(key),
                LocatorNode::Request {
                    selector,
                    persistence,
                },
            ) => {
                requests.push(LocatedRequest {
                    selector: selector.clone(),
                    key: *key,
                    persistence: persistence.clone(),
                });
            }
            (WorkspaceItemRef::Folder(key), LocatorNode::Folder { selector, children }) => {
                folders.push(LocatedFolder {
                    selector: selector.clone(),
                    key: *key,
                });
                let folder = workspace
                    .folder(*key)
                    .expect("workspace folder reference must resolve");
                index_locator_nodes(workspace, &folder.children, children, requests, folders);
            }
            _ => unreachable!("locator node type must match workspace item type"),
        }
    }
}

fn config_file(directory: &Path, stem: &str) -> Option<PathBuf> {
    ["yml", "yaml"]
        .into_iter()
        .map(|extension| directory.join(format!("{stem}.{extension}")))
        .find(|path| path.is_file())
}

fn read_directory(path: &Path) -> Result<Vec<fs::DirEntry>, LoadError> {
    fs::read_dir(path)
        .map_err(|source| LoadError::Io {
            path: path.to_owned(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| LoadError::Io {
            path: path.to_owned(),
            source,
        })
}

fn read_to_string(path: &Path) -> Result<String, LoadError> {
    fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_owned(),
        source,
    })
}

fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("yml" | "yaml")
    )
}

fn relative_selector(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn request_document_mut<'a>(
    document: &'a mut Value,
    item_path: &[usize],
) -> Result<&'a mut Value, SaveError> {
    let mut current = document;
    for index in item_path {
        let mapping = current.as_mapping_mut().ok_or_else(|| {
            SaveError::InvalidDocument("an item parent is not a mapping".to_owned())
        })?;
        let items = mapping
            .get_mut(Value::String("items".to_owned()))
            .and_then(Value::as_sequence_mut)
            .ok_or_else(|| {
                SaveError::InvalidDocument("an item parent has no items sequence".to_owned())
            })?;
        current = items.get_mut(*index).ok_or_else(|| {
            SaveError::InvalidDocument(format!("item index {index} is out of bounds"))
        })?;
    }
    Ok(current)
}

fn apply_request_update(document: &mut Value, update: &RequestUpdate) -> Result<(), SaveError> {
    let request = document.as_mapping_mut().ok_or_else(|| {
        SaveError::InvalidDocument("the request item is not a mapping".to_owned())
    })?;

    if let Some(name) = &update.name {
        let info = mapping_child(request, "info")?;
        info.insert(
            Value::String("name".to_owned()),
            Value::String(name.clone()),
        );
    }
    if update.method.is_some()
        || update.url.is_some()
        || update.headers.is_some()
        || update.query_parameters.is_some()
        || update.body.is_some()
        || update.authentication.is_some()
    {
        let http = mapping_child(request, "http")?;
        if let Some(method) = &update.method {
            http.insert(
                Value::String("method".to_owned()),
                Value::String(method.clone()),
            );
        }
        if let Some(url) = &update.url {
            http.insert(Value::String("url".to_owned()), Value::String(url.clone()));
        }
        if let Some(headers) = &update.headers {
            merge_sequence(http, "headers", headers.iter().map(header_value).collect());
        }
        if let Some(parameters) = &update.query_parameters {
            merge_sequence_preserving(
                http,
                "params",
                parameters.iter().map(query_parameter_value).collect(),
                &["type"],
            );
        }
        if let Some(body) = &update.body {
            set_optional_merged(http, "body", body.as_ref().map(request_body_value));
        }
        if let Some(authentication) = &update.authentication {
            set_optional(
                http,
                "auth",
                authentication.as_ref().map(authentication_value),
            );
        }
    }
    Ok(())
}

fn string_key(name: &str) -> Value {
    Value::String(name.to_owned())
}

fn set_optional(mapping: &mut serde_yaml_ng::Mapping, name: &str, value: Option<Value>) {
    let key = string_key(name);
    if let Some(value) = value {
        mapping.insert(key, value);
    } else {
        mapping.remove(&key);
    }
}

fn set_optional_merged(mapping: &mut serde_yaml_ng::Mapping, name: &str, value: Option<Value>) {
    let key = string_key(name);
    if let Some(mut value) = value {
        if let Some(existing) = mapping.remove(&key) {
            merge_yaml(&mut value, existing);
        }
        mapping.insert(key, value);
    } else {
        mapping.remove(&key);
    }
}

fn merge_yaml(replacement: &mut Value, existing: Value) {
    match (replacement, existing) {
        (Value::Mapping(replacement), Value::Mapping(existing)) => {
            for (key, old_value) in existing {
                if let Some(new_value) = replacement.get_mut(&key) {
                    merge_yaml(new_value, old_value);
                } else {
                    replacement.insert(key, old_value);
                }
            }
        }
        (Value::Sequence(replacement), Value::Sequence(existing)) => {
            for (new_value, old_value) in replacement.iter_mut().zip(existing) {
                merge_yaml(new_value, old_value);
            }
        }
        _ => {}
    }
}

fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Mapping(
        entries
            .into_iter()
            .map(|(key, value)| (string_key(key), value))
            .collect(),
    )
}

fn merge_sequence(parent: &mut serde_yaml_ng::Mapping, name: &str, replacements: Vec<Value>) {
    merge_sequence_preserving(parent, name, replacements, &[]);
}

fn merge_sequence_preserving(
    parent: &mut serde_yaml_ng::Mapping,
    name: &str,
    replacements: Vec<Value>,
    preserved_keys: &[&str],
) {
    let key = string_key(name);
    let existing = parent
        .get(&key)
        .and_then(Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    let merged = replacements
        .into_iter()
        .enumerate()
        .map(|(index, replacement)| {
            let Some(Value::Mapping(mut old)) = existing.get(index).cloned() else {
                return replacement;
            };
            let Value::Mapping(new) = replacement else {
                return replacement;
            };
            for (key, value) in new {
                if preserved_keys
                    .iter()
                    .any(|preserved| key == string_key(preserved))
                    && old.contains_key(&key)
                {
                    continue;
                }
                old.insert(key, value);
            }
            Value::Mapping(old)
        })
        .collect();
    parent.insert(key, Value::Sequence(merged));
}

fn header_value(header: &Header) -> Value {
    map([
        ("name", Value::String(header.name.clone())),
        ("value", Value::String(header.value.clone())),
        ("disabled", Value::Bool(header.disabled)),
    ])
}

fn query_parameter_value(parameter: &QueryParameter) -> Value {
    map([
        ("name", Value::String(parameter.name.clone())),
        ("value", Value::String(parameter.value.clone())),
        ("type", Value::String("query".to_owned())),
        ("disabled", Value::Bool(parameter.disabled)),
    ])
}

fn request_body_value(body: &RequestBody) -> Value {
    match body {
        RequestBody::Single(body) => body_value(body),
        RequestBody::Variants(variants) => Value::Sequence(
            variants
                .iter()
                .map(|variant| {
                    map([
                        ("title", Value::String(variant.title.clone())),
                        ("selected", Value::Bool(variant.selected)),
                        ("body", body_value(&variant.body)),
                    ])
                })
                .collect(),
        ),
    }
}

fn body_value(body: &Body) -> Value {
    match body {
        Body::Raw(body) => map([
            (
                "type",
                Value::String(
                    match body.kind {
                        RawBodyKind::Json => "json",
                        RawBodyKind::Text => "text",
                        RawBodyKind::Xml => "xml",
                        RawBodyKind::Sparql => "sparql",
                    }
                    .to_owned(),
                ),
            ),
            ("data", Value::String(body.data.clone())),
        ]),
        Body::FormUrlEncoded(fields) => map([
            ("type", Value::String("form-urlencoded".to_owned())),
            (
                "data",
                Value::Sequence(fields.iter().map(form_field_value).collect()),
            ),
        ]),
        Body::Multipart(parts) => map([
            ("type", Value::String("multipart-form".to_owned())),
            (
                "data",
                Value::Sequence(parts.iter().map(multipart_part_value).collect()),
            ),
        ]),
        Body::File(files) => map([
            ("type", Value::String("file".to_owned())),
            (
                "data",
                Value::Sequence(files.iter().map(file_reference_value).collect()),
            ),
        ]),
    }
}

fn form_field_value(field: &FormField) -> Value {
    map([
        ("name", Value::String(field.name.clone())),
        ("value", Value::String(field.value.clone())),
        ("disabled", Value::Bool(field.disabled)),
    ])
}

fn multipart_part_value(part: &MultipartPart) -> Value {
    let mut value = match map([
        ("name", Value::String(part.name.clone())),
        (
            "type",
            Value::String(
                match part.kind {
                    MultipartPartKind::Text => "text",
                    MultipartPartKind::File => "file",
                }
                .to_owned(),
            ),
        ),
        (
            "value",
            match &part.value {
                MultipartValue::Single(value) => Value::String(value.clone()),
                MultipartValue::Multiple(values) => {
                    Value::Sequence(values.iter().cloned().map(Value::String).collect())
                }
            },
        ),
        ("disabled", Value::Bool(part.disabled)),
    ]) {
        Value::Mapping(value) => value,
        _ => unreachable!(),
    };
    value.insert(
        string_key("contentType"),
        part.content_type
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    Value::Mapping(value)
}

fn file_reference_value(file: &FileReference) -> Value {
    map([
        ("filePath", Value::String(file.file_path.clone())),
        ("contentType", Value::String(file.content_type.clone())),
        ("selected", Value::Bool(file.selected)),
    ])
}

fn authentication_value(authentication: &Authentication) -> Value {
    if authentication.kind == AuthenticationKind::Inherit {
        return Value::String("inherit".to_owned());
    }
    let mut value = serde_yaml_ng::Mapping::new();
    value.insert(
        string_key("type"),
        Value::String(
            match &authentication.kind {
                AuthenticationKind::Inherit => "inherit",
                AuthenticationKind::AwsV4 => "awsv4",
                AuthenticationKind::Basic => "basic",
                AuthenticationKind::Wsse => "wsse",
                AuthenticationKind::Bearer => "bearer",
                AuthenticationKind::Digest => "digest",
                AuthenticationKind::Ntlm => "ntlm",
                AuthenticationKind::ApiKey => "apikey",
                AuthenticationKind::OAuth1 => "oauth1",
                AuthenticationKind::OAuth2 => "oauth2",
                AuthenticationKind::Other(value) => value,
            }
            .to_owned(),
        ),
    );
    value.extend(
        authentication
            .properties
            .iter()
            .map(|(name, value)| (Value::String(name.clone()), auth_property_value(value))),
    );
    Value::Mapping(value)
}

fn auth_property_value(value: &AuthenticationValue) -> Value {
    match value {
        AuthenticationValue::String(value) => Value::String(value.clone()),
        AuthenticationValue::Number(value) => {
            serde_yaml_ng::from_str(value).unwrap_or_else(|_| Value::String(value.clone()))
        }
        AuthenticationValue::Boolean(value) => Value::Bool(*value),
        AuthenticationValue::Null => Value::Null,
        AuthenticationValue::Sequence(values) => {
            Value::Sequence(values.iter().map(auth_property_value).collect())
        }
        AuthenticationValue::Object(values) => Value::Mapping(
            values
                .iter()
                .map(|(name, value)| (Value::String(name.clone()), auth_property_value(value)))
                .collect(),
        ),
    }
}

fn mapping_child<'a>(
    parent: &'a mut serde_yaml_ng::Mapping,
    name: &str,
) -> Result<&'a mut serde_yaml_ng::Mapping, SaveError> {
    let key = Value::String(name.to_owned());
    if !parent.contains_key(&key) {
        parent.insert(key.clone(), Value::Mapping(serde_yaml_ng::Mapping::new()));
    }
    parent
        .get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| SaveError::InvalidDocument(format!("'{name}' is not a mapping")))
}

pub(crate) fn atomic_write(
    path: &Path,
    contents: &[u8],
    expected_source: &[u8],
) -> Result<(), SaveError> {
    let map_io = |source| SaveError::Io {
        path: path.to_owned(),
        source,
    };
    let mut file = AtomicWriteFile::open(path).map_err(map_io)?;
    file.write_all(contents).map_err(map_io)?;
    file.sync_all().map_err(map_io)?;
    let current = fs::read(path).map_err(map_io)?;
    if current != expected_source {
        return Err(SaveError::ConcurrentModification(path.to_owned()));
    }
    file.commit().map_err(map_io)?;
    Ok(())
}

pub(crate) struct SaveLock {
    file: fs::File,
}

impl SaveLock {
    pub(crate) fn acquire(destination: &Path) -> Result<Self, SaveError> {
        let directory = std::env::temp_dir().join("probe-persistence-locks");
        fs::create_dir_all(&directory).map_err(|source| SaveError::Io {
            path: destination.to_owned(),
            source,
        })?;
        let path = directory.join(format!("{:016x}.lock", stable_path_hash(destination)));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|source| SaveError::Io {
                path: destination.to_owned(),
                source,
            })?;
        #[cfg(any(unix, windows))]
        FileExt::lock(&file).map_err(|source| SaveError::Io {
            path: destination.to_owned(),
            source,
        })?;
        Ok(Self { file })
    }
}

fn stable_path_hash(path: &Path) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        for byte in path.as_os_str().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for value in path.as_os_str().encode_wide() {
            for byte in value.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(PRIME);
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

impl Drop for SaveLock {
    fn drop(&mut self) {
        #[cfg(any(unix, windows))]
        let _ = FileExt::unlock(&self.file);
    }
}
