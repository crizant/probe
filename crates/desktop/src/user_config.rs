//! Desktop user configuration.
//!
//! The file is optional TOML. It is not an OpenCollection document and it does
//! not store collection variables. The CLI does not load it. Parsing and
//! filesystem access stay in this module so GPUI views never read the file.

use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Desktop user settings loaded from the platform config file.
///
/// There are no settings yet. Add fields with [`Default`] and `#[serde(default)]`
/// so an existing file that omits them keeps working. Unknown keys are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UserConfig {}

impl UserConfig {
    /// Loads the config file for this process.
    ///
    /// A missing file returns [`UserConfig::default`] and does not create a file.
    pub(crate) fn load() -> Result<Self, ConfigError> {
        Self::load_from(&config_path()?)
    }

    /// Loads `path`.
    ///
    /// A missing file returns [`UserConfig::default`] and does not create a file
    /// or its parent directory. Any other filesystem failure, and invalid TOML,
    /// include `path` in the error.
    pub(crate) fn load_from(path: &Path) -> Result<Self, ConfigError> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(ConfigError::read(path, error)),
            Ok(_) => {
                let text =
                    fs::read_to_string(path).map_err(|error| ConfigError::read(path, error))?;
                toml::from_str(&text).map_err(|source| ConfigError::invalid(path, source))
            }
        }
    }
}

/// Failure to resolve or read the desktop user configuration file.
#[derive(Debug)]
pub(crate) enum ConfigError {
    /// The platform path could not be resolved.
    Location(String),
    /// The file exists but could not be read.
    Read {
        /// Config file that could not be read.
        path: PathBuf,
        /// Filesystem failure.
        source: io::Error,
    },
    /// The file was read but is not valid TOML for [`UserConfig`].
    Invalid {
        /// Config file that could not be parsed.
        path: PathBuf,
        /// TOML or schema failure.
        ///
        /// Boxed so `ConfigError` stays small enough for `result_large_err`.
        source: Box<toml::de::Error>,
    },
}

impl ConfigError {
    fn read(path: &Path, source: io::Error) -> Self {
        Self::Read {
            path: path.to_path_buf(),
            source,
        }
    }

    fn invalid(path: &Path, source: toml::de::Error) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            source: Box::new(source),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Location(message) => write!(
                formatter,
                "cannot resolve Probe's user configuration path: {message}"
            ),
            Self::Read { path, source } => write!(
                formatter,
                "cannot read Probe's user configuration at {}: {source}",
                path.display()
            ),
            Self::Invalid { path, source } => write!(
                formatter,
                "Probe's user configuration at {} is invalid: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Location(_) => None,
            Self::Read { source, .. } => Some(source),
            Self::Invalid { source, .. } => Some(source.as_ref()),
        }
    }
}

#[cfg(unix)]
fn config_path() -> Result<PathBuf, ConfigError> {
    let xdg = env::var_os("XDG_CONFIG_HOME");
    let home = env::var_os("HOME");
    unix_config_path(
        xdg.as_deref().map(Path::new),
        home.as_deref().map(Path::new),
    )
}

#[cfg(windows)]
fn config_path() -> Result<PathBuf, ConfigError> {
    let appdata = env::var_os("APPDATA");
    windows_config_path(appdata.as_deref().map(Path::new))
}

#[cfg(unix)]
fn unix_config_path(xdg: Option<&Path>, home: Option<&Path>) -> Result<PathBuf, ConfigError> {
    if let Some(dir) = xdg.filter(|path| path.is_absolute()) {
        return Ok(dir.join("probe").join("config.toml"));
    }
    let Some(home) = absolute(home) else {
        return Err(ConfigError::Location(
            "HOME is not an absolute path; Probe looks for ~/.config/probe/config.toml".to_owned(),
        ));
    };
    Ok(home.join(".config").join("probe").join("config.toml"))
}

#[cfg(windows)]
fn windows_config_path(appdata: Option<&Path>) -> Result<PathBuf, ConfigError> {
    let Some(dir) = absolute(appdata) else {
        return Err(ConfigError::Location(
            "APPDATA is not an absolute path; Probe looks for %APPDATA%\\probe\\config.toml"
                .to_owned(),
        ));
    };
    Ok(dir.join("probe").join("config.toml"))
}

fn absolute(path: Option<&Path>) -> Option<&Path> {
    path.filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "probe-desktop-user-config-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_returns_defaults_without_creating_it() {
        let root = TempDir::new();
        let path = root.0.join("probe").join("config.toml");

        assert!(!path.exists());
        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());
        assert!(!path.exists());
        assert!(!root.0.join("probe").exists());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_a_read_error() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        std::os::unix::fs::symlink(root.0.join("missing.toml"), &path).unwrap();

        let error = UserConfig::load_from(&path).unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, ConfigError::Read { .. }), "{message}");
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn valid_toml_loads_defaults() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        fs::write(&path, "# Probe user configuration\n").unwrap();

        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());
    }

    #[test]
    fn malformed_toml_error_includes_the_config_path() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        fs::write(&path, "timeout = [\n").unwrap();

        let message = UserConfig::load_from(&path).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        fs::write(&path, "renderer = \"gpu\"\n\n[window]\nwidth = 1200\n").unwrap();

        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());
    }

    #[cfg(unix)]
    #[test]
    fn absolute_xdg_config_home_overrides_the_home_fallback() {
        let path = unix_config_path(
            Some(Path::new("/tmp/probe-xdg")),
            Some(Path::new("/home/ada")),
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("/tmp/probe-xdg/probe/config.toml"));
    }

    #[cfg(unix)]
    #[test]
    fn relative_or_empty_xdg_config_home_falls_back_to_home() {
        let home = Path::new("/home/ada");
        let expected = PathBuf::from("/home/ada/.config/probe/config.toml");
        assert_eq!(
            unix_config_path(Some(Path::new("config")), Some(home)).unwrap(),
            expected
        );
        assert_eq!(
            unix_config_path(Some(Path::new("")), Some(home)).unwrap(),
            expected
        );
        assert_eq!(unix_config_path(None, Some(home)).unwrap(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn relative_home_is_a_location_error() {
        let error = unix_config_path(None, Some(Path::new("ada"))).unwrap_err();
        assert!(matches!(error, ConfigError::Location(_)), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn appdata_config_path() {
        let path = windows_config_path(Some(Path::new(r"C:\Users\ada\AppData\Roaming"))).unwrap();
        assert_eq!(
            path,
            Path::new(r"C:\Users\ada\AppData\Roaming")
                .join("probe")
                .join("config.toml")
        );
    }

    #[cfg(windows)]
    #[test]
    fn relative_appdata_is_a_location_error() {
        let error = windows_config_path(Some(Path::new(r"AppData\Roaming"))).unwrap_err();
        assert!(matches!(error, ConfigError::Location(_)), "{error}");
    }
}
