use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use probe_core::{CollectionItem, Environment, RequestKey, Workspace, WorkspaceItemRef};
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::{EnvironmentDocument, ParseError, parse, project_item};

/// A request and its repository-backed selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedRequest {
    selector: String,
    key: RequestKey,
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

/// A loaded OpenCollection workspace and its persistence-locator index.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedWorkspace {
    workspace: Workspace,
    requests: Vec<LocatedRequest>,
    request_keys_by_selector: BTreeMap<String, RequestKey>,
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

    /// Resolves a repository-backed selector to a request key.
    #[must_use]
    pub fn request_key(&self, selector: &str) -> Option<RequestKey> {
        self.request_keys_by_selector.get(selector).copied()
    }
}

/// Loads a bundled OpenCollection file or an unbundled collection directory.
pub fn load_workspace(path: impl AsRef<Path>) -> Result<LoadedWorkspace, LoadError> {
    let path = path.as_ref();
    if path.is_dir() {
        load_unbundled(path)
    } else {
        load_bundled(path)
    }
}

/// Loads a bundled OpenCollection workspace from an in-memory YAML document.
///
/// Structural selectors are identical to selectors produced when the same bundled
/// document is loaded from a file.
pub fn load_workspace_from_str(source: &str) -> Result<LoadedWorkspace, LoadError> {
    load_bundled_source(source, Path::new("<memory>"))
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
        }
    }
}

impl Error for LoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::MissingRoot(_) | Self::InvalidItem { .. } => None,
        }
    }
}

#[derive(Debug)]
enum LocatorNode {
    Folder(Vec<LocatorNode>),
    Request(String),
}

fn load_bundled(path: &Path) -> Result<LoadedWorkspace, LoadError> {
    let source = read_to_string(path)?;
    load_bundled_source(&source, path)
}

fn load_bundled_source(source: &str, source_name: &Path) -> Result<LoadedWorkspace, LoadError> {
    let parsed = parse(source).map_err(|source| LoadError::Parse {
        path: source_name.to_owned(),
        source,
    })?;
    let nodes = bundled_locator_nodes(parsed.document(), "items");
    let workspace = Workspace::from_collection(parsed.into_collection());
    Ok(index_locators(workspace, &nodes))
}

fn load_unbundled(root: &Path) -> Result<LoadedWorkspace, LoadError> {
    let root_config = config_file(root, "opencollection")
        .ok_or_else(|| LoadError::MissingRoot(root.to_owned()))?;
    let source = read_to_string(&root_config)?;
    let parsed = parse(&source).map_err(|source| LoadError::Parse {
        path: root_config,
        source,
    })?;
    let mut collection = parsed.into_collection();
    let loaded_items = read_items(root, root, "opencollection")?;
    let (items, nodes): (Vec<_>, Vec<_>) = loaded_items.into_iter().unzip();
    collection.items = items;
    collection.environments.extend(read_environments(root)?);

    let workspace = Workspace::from_collection(collection);
    Ok(index_locators(workspace, &nodes))
}

fn read_items(
    directory: &Path,
    root: &Path,
    reserved_stem: &str,
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
            let mut folder = match read_item(&folder_config)? {
                Some(CollectionItem::Folder(folder)) => folder,
                Some(CollectionItem::HttpRequest(_)) => {
                    return Err(LoadError::InvalidItem {
                        path: folder_config,
                        message: "folder.yml must describe a folder".to_owned(),
                    });
                }
                None => continue,
            };
            let children = read_items(&path, root, "folder")?;
            let (child_items, child_nodes): (Vec<_>, Vec<_>) = children.into_iter().unzip();
            folder.items = child_items;
            items.push((
                CollectionItem::Folder(folder),
                LocatorNode::Folder(child_nodes),
            ));
        } else if is_yaml_file(&path)
            && path.file_stem().and_then(|stem| stem.to_str()) != Some(reserved_stem)
        {
            if let Some(item) = read_item(&path)? {
                match item {
                    CollectionItem::HttpRequest(request) => {
                        let selector = relative_selector(root, &path);
                        items.push((
                            CollectionItem::HttpRequest(request),
                            LocatorNode::Request(selector),
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

fn read_item(path: &Path) -> Result<Option<CollectionItem>, LoadError> {
    let source = read_to_string(path)?;
    let value: Value = serde_yaml_ng::from_str(&source).map_err(|source| LoadError::Parse {
        path: path.to_owned(),
        source: ParseError::new(source),
    })?;
    project_item(value).map_err(|source| LoadError::Parse {
        path: path.to_owned(),
        source: ParseError::new(source),
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

fn bundled_locator_nodes(document: &Value, prefix: &str) -> Vec<LocatorNode> {
    let document: LocatorItemsDocument = serde_yaml_ng::from_value(document.clone())
        .expect("successfully parsed document must retain an object root");
    locator_nodes_from_items(document.items, prefix)
}

fn locator_nodes_from_items(items: Vec<Value>, prefix: &str) -> Vec<LocatorNode> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let item: LocatorItemDocument = serde_yaml_ng::from_value(value)
                .expect("successfully projected item must retain a valid object shape");
            match item.info.item_type.as_deref() {
                Some("http") => Some(LocatorNode::Request(format!("{prefix}/{index}"))),
                Some("folder") => Some(LocatorNode::Folder(locator_nodes_from_items(
                    item.items,
                    &format!("{prefix}/{index}/items"),
                ))),
                _ => None,
            }
        })
        .collect()
}

fn index_locators(workspace: Workspace, nodes: &[LocatorNode]) -> LoadedWorkspace {
    let mut requests = Vec::new();
    index_locator_nodes(&workspace, workspace.root_items(), nodes, &mut requests);
    let request_keys_by_selector = requests
        .iter()
        .map(|request: &LocatedRequest| (request.selector.clone(), request.key))
        .collect();
    LoadedWorkspace {
        workspace,
        requests,
        request_keys_by_selector,
    }
}

fn index_locator_nodes(
    workspace: &Workspace,
    items: &[WorkspaceItemRef],
    nodes: &[LocatorNode],
    requests: &mut Vec<LocatedRequest>,
) {
    assert_eq!(
        items.len(),
        nodes.len(),
        "locator tree must match workspace"
    );
    for (item, node) in items.iter().zip(nodes) {
        match (item, node) {
            (WorkspaceItemRef::Request(key), LocatorNode::Request(selector)) => {
                requests.push(LocatedRequest {
                    selector: selector.clone(),
                    key: *key,
                });
            }
            (WorkspaceItemRef::Folder(key), LocatorNode::Folder(children)) => {
                let folder = workspace
                    .folder(*key)
                    .expect("workspace folder reference must resolve");
                index_locator_nodes(workspace, &folder.children, children, requests);
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
