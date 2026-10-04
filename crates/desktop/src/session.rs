use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::credential_presence::CredentialPresenceState;
use crate::shell::{DEFAULT_RESPONSE_HEIGHT, DEFAULT_RESPONSE_WIDTH, DEFAULT_SIDEBAR_WIDTH};

const SCHEMA_VERSION: u32 = 2;
const RECENT_COLLECTION_LIMIT: usize = 10;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "selector", rename_all = "snake_case")]
pub(crate) enum TabLocator {
    Request(String),
    Collection,
    Folder(String),
}

impl TabLocator {
    pub(crate) fn capture(
        tab: crate::shell::OpenTab,
        loaded: &probe_opencollection::LoadedWorkspace,
    ) -> Option<Self> {
        match tab {
            crate::shell::OpenTab::Request(key) => loaded
                .request_selector(key)
                .map(|s| Self::Request(s.to_owned())),
            crate::shell::OpenTab::Overview(crate::shell::OverviewTab::Collection) => {
                Some(Self::Collection)
            }
            crate::shell::OpenTab::Overview(crate::shell::OverviewTab::Folder(key)) => loaded
                .folder_selector(key)
                .map(|s| Self::Folder(s.to_owned())),
        }
    }

    pub(crate) fn resolve(
        &self,
        loaded: &probe_opencollection::LoadedWorkspace,
    ) -> Option<crate::shell::OpenTab> {
        match self {
            Self::Request(selector) => loaded.request_key(selector).map(Into::into),
            Self::Collection => Some(crate::shell::OverviewTab::Collection.into()),
            Self::Folder(selector) => loaded
                .folder_key(selector)
                .map(|key| crate::shell::OverviewTab::Folder(key).into()),
        }
    }

    pub(crate) fn remap(&self, remaps: &BTreeMap<String, String>) -> Self {
        match self {
            Self::Request(selector) => {
                Self::Request(remaps.get(selector).unwrap_or(selector).clone())
            }
            Self::Folder(selector) => {
                Self::Folder(remaps.get(selector).unwrap_or(selector).clone())
            }
            Self::Collection => Self::Collection,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct WorkspaceSessionState {
    pub(crate) open_tabs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ordered_tabs: Option<Vec<TabLocator>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_open_tab: Option<TabLocator>,
    pub(crate) active_tab: Option<String>,
    pub(crate) collapsed_folders: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SessionState {
    pub(crate) schema_version: u32,
    pub(crate) active_collection: Option<PathBuf>,
    pub(crate) recent_collections: Vec<PathBuf>,
    pub(crate) workspaces: BTreeMap<PathBuf, WorkspaceSessionState>,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) response_height: f32,
    pub(crate) response_width: f32,
    pub(crate) horizontal_panes: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) selected_environments: BTreeMap<PathBuf, String>,
    #[serde(flatten)]
    pub(crate) presence: CredentialPresenceState,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active_collection: None,
            recent_collections: Vec::new(),
            workspaces: BTreeMap::new(),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_collapsed: false,
            response_height: DEFAULT_RESPONSE_HEIGHT,
            response_width: DEFAULT_RESPONSE_WIDTH,
            horizontal_panes: false,
            selected_environments: BTreeMap::new(),
            presence: CredentialPresenceState::default(),
        }
    }
}

impl SessionState {
    pub(crate) fn activate_collection(&mut self, path: PathBuf) {
        self.recent_collections.retain(|recent| recent != &path);
        self.recent_collections.insert(0, path.clone());
        self.recent_collections.truncate(RECENT_COLLECTION_LIMIT);
        self.active_collection = Some(path);
        self.prune_selected_environments();
        self.prune_workspaces();
    }

    pub(crate) fn clear_active_collection(&mut self) {
        self.active_collection = None;
    }

    pub(crate) fn remove_recent_collection(&mut self, path: &Path) {
        self.recent_collections.retain(|recent| recent != path);
        self.selected_environments.remove(path);
        self.workspaces.remove(path);
    }

    pub(crate) fn selected_environment_for(&self, path: &Path) -> Option<&str> {
        self.selected_environments.get(path).map(String::as_str)
    }

    pub(crate) fn remember_selected_environment(
        &mut self,
        path: PathBuf,
        environment: Option<String>,
    ) {
        match environment.filter(|name| !name.is_empty()) {
            Some(name) => {
                self.selected_environments.insert(path, name);
            }
            None => {
                self.selected_environments.remove(&path);
            }
        }
    }

    fn prune_selected_environments(&mut self) {
        self.selected_environments
            .retain(|path, _| self.recent_collections.contains(path));
    }

    fn prune_workspaces(&mut self) {
        self.workspaces
            .retain(|path, _| self.recent_collections.contains(path));
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub(crate) fn for_application() -> Option<Self> {
        ProjectDirs::from("dev", "Probe", "Probe").map(|directories| Self {
            path: directories.data_local_dir().join("desktop-session.json"),
        })
    }

    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load(&self) -> Result<SessionState, SessionError> {
        let source = match fs::read(&self.path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(SessionState::default());
            }
            Err(source) => return Err(SessionError::Io(source)),
        };
        let mut source: serde_json::Value =
            serde_json::from_slice(&source).map_err(SessionError::Parse)?;
        let version = source
            .get("schema_version")
            .and_then(serde_json::Value::as_u64);
        let legacy_state = version == Some(1)
            || (source.get("schema_version").is_none()
                && ["open_tabs", "active_tab", "collapsed_folders"]
                    .iter()
                    .any(|field| source.get(field).is_some()));
        if legacy_state && let Some(object) = source.as_object_mut() {
            let active = object
                .get("active_collection")
                .and_then(|path| path.as_str());
            if let Some(path) = active.map(str::to_owned) {
                let mut workspace = serde_json::Map::new();
                for field in ["open_tabs", "active_tab", "collapsed_folders"] {
                    if let Some(value) = object.remove(field) {
                        workspace.insert(field.to_owned(), value);
                    }
                }
                let mut workspaces = serde_json::Map::new();
                workspaces.insert(path, workspace.into());
                object.insert("workspaces".to_owned(), workspaces.into());
            }
            object.insert("schema_version".to_owned(), SCHEMA_VERSION.into());
        }
        let state: SessionState = serde_json::from_value(source).map_err(SessionError::Parse)?;
        if state.schema_version != SCHEMA_VERSION {
            return Err(SessionError::UnsupportedVersion(state.schema_version));
        }
        Ok(state)
    }

    pub(crate) fn save(&self, state: &SessionState) -> Result<(), SessionError> {
        let parent = self
            .path
            .parent()
            .expect("desktop session path must have a parent directory");
        fs::create_dir_all(parent).map_err(SessionError::Io)?;
        let mut source = serde_json::to_vec_pretty(state).map_err(SessionError::Serialize)?;
        source.push(b'\n');
        let mut file = AtomicWriteFile::open(&self.path).map_err(SessionError::Io)?;
        file.write_all(&source).map_err(SessionError::Io)?;
        file.sync_all().map_err(SessionError::Io)?;
        file.commit().map_err(SessionError::Io)
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[derive(Debug)]
pub(crate) enum SessionError {
    Io(io::Error),
    Parse(serde_json::Error),
    Serialize(serde_json::Error),
    UnsupportedVersion(u32),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cannot access desktop session state: {error}"),
            Self::Parse(error) => write!(formatter, "invalid desktop session state: {error}"),
            Self::Serialize(error) => {
                write!(formatter, "cannot serialize desktop session state: {error}")
            }
            Self::UnsupportedVersion(version) => write!(
                formatter,
                "desktop session state uses unsupported schema version {version}"
            ),
        }
    }
}

impl Error for SessionError {}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{SessionState, SessionStore, WorkspaceSessionState};

    fn store() -> SessionStore {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        SessionStore::at(std::env::temp_dir().join(format!(
            "probe-desktop-session-{}-{unique}/session.json",
            std::process::id()
        )))
    }

    #[test]
    fn missing_state_uses_safe_defaults() {
        assert_eq!(store().load().unwrap(), SessionState::default());
    }

    #[test]
    fn state_round_trips_through_an_atomic_file() {
        let store = store();
        let mut state = SessionState::default();
        state.activate_collection("/tmp/example".into());
        state.workspaces.insert(
            "/tmp/example".into(),
            WorkspaceSessionState {
                ordered_tabs: Some(vec![
                    super::TabLocator::Request("users/list.yml".into()),
                    super::TabLocator::Collection,
                    super::TabLocator::Folder("users".into()),
                ]),
                active_open_tab: Some(super::TabLocator::Collection),
                open_tabs: vec!["users/list.yml".to_owned()],
                active_tab: Some("users/list.yml".to_owned()),
                collapsed_folders: vec!["users".to_owned()],
            },
        );
        state.sidebar_width = 312.0;
        state.sidebar_collapsed = true;
        state.response_width = 480.0;
        state.horizontal_panes = true;
        state.remember_selected_environment("/tmp/example".into(), Some("development".to_owned()));

        store.save(&state).unwrap();
        assert_eq!(store.load().unwrap(), state);
        assert!(store.path().is_file());
    }

    #[test]
    fn two_workspace_states_survive_restart_and_keep_global_layout() {
        let store = store();
        let mut state = SessionState::default();
        let a = PathBuf::from("/tmp/a");
        let b = PathBuf::from("/tmp/b");
        state.activate_collection(a.clone());
        state.workspaces.insert(
            a.clone(),
            WorkspaceSessionState {
                ordered_tabs: None,
                active_open_tab: None,
                open_tabs: vec!["first".into(), "second".into()],
                active_tab: Some("first".into()),
                collapsed_folders: vec!["folder-a".into()],
            },
        );
        state.activate_collection(b.clone());
        state.workspaces.insert(
            b.clone(),
            WorkspaceSessionState {
                ordered_tabs: None,
                active_open_tab: None,
                open_tabs: vec!["other".into()],
                active_tab: Some("other".into()),
                collapsed_folders: vec!["folder-b".into()],
            },
        );
        state.sidebar_width = 330.0;
        store.save(&state).unwrap();

        let restored = store.load().unwrap();
        assert_eq!(restored.active_collection, Some(b.clone()));
        assert_eq!(restored.workspaces[&a].open_tabs, ["first", "second"]);
        assert_eq!(restored.workspaces[&a].active_tab.as_deref(), Some("first"));
        assert_eq!(restored.workspaces[&b].collapsed_folders, ["folder-b"]);
        assert_eq!(restored.sidebar_width, 330.0);
    }

    #[test]
    fn version_one_active_tabs_migrate_to_their_workspace() {
        let store = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(
            store.path(),
            r#"{
            "schema_version": 1,
            "active_collection": "/tmp/old",
            "open_tabs": ["first", "second"],
            "active_tab": "first",
            "collapsed_folders": ["folder"]
        }"#,
        )
        .unwrap();

        let restored = store.load().unwrap();
        assert_eq!(restored.schema_version, 2);
        let workspace = &restored.workspaces[Path::new("/tmp/old")];
        assert_eq!(workspace.open_tabs, ["first", "second"]);
        assert_eq!(workspace.active_tab.as_deref(), Some("first"));
        assert_eq!(workspace.collapsed_folders, ["folder"]);
        store.save(&restored).unwrap();
        assert_eq!(store.load().unwrap(), restored);
    }

    #[test]
    fn unversioned_legacy_tabs_migrate_to_their_workspace() {
        let store = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(
            store.path(),
            r#"{
            "active_collection": "/tmp/old",
            "open_tabs": ["first", "second"],
            "active_tab": "first",
            "collapsed_folders": ["folder"]
        }"#,
        )
        .unwrap();

        let restored = store.load().unwrap();
        assert_eq!(restored.schema_version, 2);
        let workspace = &restored.workspaces[Path::new("/tmp/old")];
        assert_eq!(workspace.open_tabs, ["first", "second"]);
        assert_eq!(workspace.active_tab.as_deref(), Some("first"));
        assert_eq!(workspace.collapsed_folders, ["folder"]);
        store.save(&restored).unwrap();
        assert_eq!(store.load().unwrap(), restored);
    }

    #[test]
    fn recent_collections_are_deduplicated_and_bounded() {
        let mut state = SessionState::default();
        for index in 0..12 {
            state.activate_collection(format!("/tmp/collection-{index}").into());
        }
        state.activate_collection("/tmp/collection-5".into());

        assert_eq!(state.recent_collections.len(), 10);
        assert_eq!(state.recent_collections[0], Path::new("/tmp/collection-5"));
    }

    #[test]
    fn removing_a_recent_collection_also_forgets_its_environment_selection() {
        let mut state = SessionState::default();
        let path = PathBuf::from("/tmp/collection");
        state.activate_collection(path.clone());
        state.remember_selected_environment(path.clone(), Some("development".to_owned()));
        state
            .workspaces
            .insert(path.clone(), WorkspaceSessionState::default());

        state.remove_recent_collection(&path);

        assert!(state.recent_collections.is_empty());
        assert_eq!(state.selected_environment_for(&path), None);
        assert!(!state.workspaces.contains_key(&path));
    }

    #[test]
    fn selected_environments_are_remembered_per_collection() {
        let mut state = SessionState::default();
        state.remember_selected_environment("/tmp/a".into(), Some("development".to_owned()));
        state.remember_selected_environment("/tmp/b".into(), Some("staging".to_owned()));
        state.remember_selected_environment("/tmp/a".into(), None);

        assert_eq!(state.selected_environment_for(Path::new("/tmp/a")), None);
        assert_eq!(
            state.selected_environment_for(Path::new("/tmp/b")),
            Some("staging")
        );
    }

    #[test]
    fn selected_environments_are_pruned_with_recent_collections() {
        let mut state = SessionState::default();
        for index in 0..12 {
            let path = PathBuf::from(format!("/tmp/collection-{index}"));
            state.activate_collection(path.clone());
            state.remember_selected_environment(path, Some("development".to_owned()));
        }

        assert_eq!(state.selected_environments.len(), 10);
        assert_eq!(
            state.selected_environment_for(Path::new("/tmp/collection-0")),
            None
        );
        assert_eq!(
            state.selected_environment_for(Path::new("/tmp/collection-1")),
            None
        );
        assert_eq!(
            state.selected_environment_for(Path::new("/tmp/collection-2")),
            Some("development")
        );
    }

    #[test]
    fn missing_selected_environments_default_to_empty() {
        let store = store();
        let parent = store
            .path()
            .parent()
            .expect("desktop session path must have a parent directory");
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(
            store.path(),
            r#"{
  "schema_version": 1,
  "active_collection": null,
  "recent_collections": [],
  "open_tabs": [],
  "active_tab": null,
  "collapsed_folders": [],
  "sidebar_width": 260.0,
  "response_height": 220.0,
  "response_width": 440.0,
  "horizontal_panes": false
}
"#,
        )
        .unwrap();

        let state = store.load().unwrap();
        assert_eq!(state.schema_version, 2);
        assert!(state.workspaces.is_empty());
        assert!(state.selected_environments.is_empty());
        assert!(state.presence.stored_credentials.is_empty());
        assert!(state.presence.missing_credentials.is_empty());
    }

    #[test]
    fn credential_presence_is_an_opaque_set_and_survives_collection_pruning() {
        let store = store();
        let mut state = SessionState::default();
        state
            .presence
            .stored_credentials
            .insert("v1-abc123".to_owned());
        state
            .presence
            .missing_credentials
            .insert("v1-def456".to_owned());
        state.activate_collection("/tmp/collection".into());
        state.remove_recent_collection(Path::new("/tmp/collection"));
        state.clear_active_collection();

        store.save(&state).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.presence.stored_credentials,
            state.presence.stored_credentials
        );
        assert_eq!(
            loaded.presence.missing_credentials,
            state.presence.missing_credentials
        );
        let source = std::fs::read_to_string(store.path()).unwrap();
        assert!(source.contains("\"stored_credentials\""));
        assert!(source.contains("\"missing_credentials\""));
        assert!(!source.contains("\"presence\""));
        assert!(!source.contains("\"revision\""));
        assert!(source.contains("v1-abc123"));
        assert!(source.contains("v1-def456"));
        assert!(!source.contains("secretToken"));
        assert!(!source.contains("/tmp/collection"));
    }

    #[test]
    fn corrupt_session_state_fails_closed_without_panicking() {
        let store = store();
        let parent = store
            .path()
            .parent()
            .expect("desktop session path must have a parent directory");
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(store.path(), b"{not-json").unwrap();
        assert!(store.load().is_err());
    }
}
