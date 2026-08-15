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
    CollectionItem, Environment, FolderKey, RequestKey, RequestUpdate, Workspace, WorkspaceItemRef,
    validate_environments,
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
    documents: BTreeMap<PathBuf, SourceDocument>,
}

impl LoadedWorkspace {
    /// Returns the in-memory domain workspace.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequestPersistence {
    document_path: PathBuf,
    item_path: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct SourceDocument {
    original_source: Vec<u8>,
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
            let mut folder = match read_item(&folder_config)?.item {
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
            if let Some(item) = read.item {
                match item {
                    CollectionItem::HttpRequest(request) => {
                        let selector = relative_selector(root, &path);
                        documents.insert(
                            path.clone(),
                            SourceDocument {
                                original_source: read.original_source,
                            },
                        );
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
    if update.method.is_some() || update.url.is_some() {
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
    }
    Ok(())
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

fn atomic_write(path: &Path, contents: &[u8], expected_source: &[u8]) -> Result<(), SaveError> {
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

struct SaveLock {
    file: fs::File,
}

impl SaveLock {
    fn acquire(destination: &Path) -> Result<Self, SaveError> {
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
