use std::{
    error::Error,
    fmt, fs,
    io::{self, Write},
    path::PathBuf,
};

use atomic_write_file::AtomicWriteFile;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;
const RECENT_COLLECTION_LIMIT: usize = 10;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SessionState {
    pub(crate) schema_version: u32,
    pub(crate) active_collection: Option<PathBuf>,
    pub(crate) recent_collections: Vec<PathBuf>,
    pub(crate) open_tabs: Vec<String>,
    pub(crate) active_tab: Option<String>,
    pub(crate) collapsed_folders: Vec<String>,
    pub(crate) sidebar_width: f32,
    pub(crate) response_height: f32,
    pub(crate) response_width: f32,
    pub(crate) horizontal_panes: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active_collection: None,
            recent_collections: Vec::new(),
            open_tabs: Vec::new(),
            active_tab: None,
            collapsed_folders: Vec::new(),
            sidebar_width: 260.0,
            response_height: 220.0,
            response_width: 440.0,
            horizontal_panes: false,
        }
    }
}

impl SessionState {
    pub(crate) fn activate_collection(&mut self, path: PathBuf) {
        self.recent_collections.retain(|recent| recent != &path);
        self.recent_collections.insert(0, path.clone());
        self.recent_collections.truncate(RECENT_COLLECTION_LIMIT);
        self.active_collection = Some(path);
    }

    pub(crate) fn clear_active_collection(&mut self) {
        self.active_collection = None;
        self.open_tabs.clear();
        self.active_tab = None;
        self.collapsed_folders.clear();
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
    fn at(path: PathBuf) -> Self {
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
        let state: SessionState = serde_json::from_slice(&source).map_err(SessionError::Parse)?;
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
    fn path(&self) -> &std::path::Path {
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
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{SessionState, SessionStore};

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
        state.open_tabs = vec!["users/list.yml".to_owned()];
        state.active_tab = state.open_tabs.first().cloned();
        state.collapsed_folders = vec!["users".to_owned()];
        state.sidebar_width = 312.0;
        state.response_width = 480.0;
        state.horizontal_panes = true;

        store.save(&state).unwrap();
        assert_eq!(store.load().unwrap(), state);
        assert!(store.path().is_file());
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
}
