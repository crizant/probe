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

/// How the desktop chooses its built-in appearance.
///
/// This is the user preference. The rendered appearance is [`crate::theme::Theme`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThemeMode {
    /// Follow the operating system appearance.
    #[default]
    System,
    /// Always use the light built-in theme.
    Light,
    /// Always use the dark built-in theme.
    Dark,
}

/// Desktop user settings loaded from the platform config file.
///
/// Add fields with [`Default`] and `#[serde(default)]` so an existing file that
/// omits them keeps working. Unknown keys are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UserConfig {
    /// `system` follows the OS appearance. `light` and `dark` stay fixed.
    #[serde(default)]
    pub(crate) theme: ThemeMode,
}

/// User config resolved before the desktop window is created.
///
/// A load failure keeps [`UserConfig::default`] and records the message shown
/// after the window exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LoadedUserConfig {
    /// Settings to apply. Defaults when loading failed.
    pub(crate) config: UserConfig,
    /// Config error to show once the window exists.
    pub(crate) error: Option<String>,
}

impl LoadedUserConfig {
    /// Reads the platform config file.
    pub(crate) fn load() -> Self {
        Self::from_load_result(UserConfig::load())
    }

    fn from_load_result(result: Result<UserConfig, ConfigError>) -> Self {
        match result {
            Ok(config) => Self {
                config,
                error: None,
            },
            Err(error) => Self {
                config: UserConfig::default(),
                error: Some(error.to_string()),
            },
        }
    }
}

impl UserConfig {
    /// Loads the config file for this process.
    ///
    /// A missing file returns [`UserConfig::default`] and does not create a file.
    pub(crate) fn load() -> Result<Self, ConfigError> {
        Self::load_from(&config_path()?)
    }

    /// Updates the theme off the UI thread, preserving other TOML keys.
    pub(crate) fn save_theme(expected: ThemeMode, theme: ThemeMode) -> Result<(), String> {
        let path = config_path().map_err(|error| error.to_string())?;
        Self::save_theme_to(&path, expected, theme)
            .map_err(|error| format!("Could not save settings at {}: {error}", path.display()))
    }

    fn save_theme_to(path: &Path, expected: ThemeMode, theme: ThemeMode) -> Result<(), String> {
        use std::io::Write;

        Self::load_from(path).map_err(|error| error.to_string())?;
        // Follow dotfiles symlinks rather than replacing the link itself.
        let target = if path.exists() {
            fs::canonicalize(path).map_err(|error| error.to_string())?
        } else {
            path.to_path_buf()
        };
        let source = read_optional_config(&target)?;
        let current = source
            .as_deref()
            .map(toml::from_str::<Self>)
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        if current.theme != expected {
            return Err(
                "Settings changed outside Probe. Reopen Probe before changing the theme.".into(),
            );
        }
        let mut document = source
            .as_deref()
            .unwrap_or_default()
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| error.to_string())?;
        let mut value = toml_edit::Value::from(match theme {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        });
        if let Some(existing) = document.get("theme").and_then(toml_edit::Item::as_value) {
            *value.decor_mut() = existing.decor().clone();
        }
        document["theme"] = toml_edit::Item::Value(value);
        let output = document.to_string();
        if output.len() as u64 > MAX_CONFIG_BYTES {
            return Err("Saved settings would exceed the 64 KiB size limit.".into());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut file =
            atomic_write_file::AtomicWriteFile::open(&target).map_err(|error| error.to_string())?;
        file.write_all(output.as_bytes())
            .map_err(|error| error.to_string())?;
        if read_optional_config(&target)? != source {
            return Err("Settings changed outside Probe while saving. Try again.".into());
        }
        file.commit().map_err(|error| error.to_string())
    }

    /// Loads `path`.
    ///
    /// A missing file returns [`UserConfig::default`] and does not create a file
    /// or its parent directory. A symlink to a regular file is followed.
    /// A dangling symlink, a path that is not a regular file, a file larger
    /// than [`MAX_CONFIG_BYTES`], and invalid TOML include `path` in the error.
    pub(crate) fn load_from(path: &Path) -> Result<Self, ConfigError> {
        // `metadata` follows symlinks, so a dotfiles link to a normal TOML file
        // is accepted. A dangling link looks missing here and is distinguished below.
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return missing_or_dangling(path, error);
            }
            Err(error) => return Err(ConfigError::read(path, error)),
        };
        if !metadata.is_file() {
            return Err(ConfigError::not_a_file(path));
        }
        let len = metadata.len();
        if len > MAX_CONFIG_BYTES {
            return Err(ConfigError::too_large(path, len));
        }
        let text = fs::read_to_string(path).map_err(|error| ConfigError::read(path, error))?;
        toml::from_str(&text).map_err(|source| ConfigError::invalid(path, source))
    }
}

fn read_optional_config(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Largest config file read before the window is created.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

fn missing_or_dangling(path: &Path, metadata_error: io::Error) -> Result<UserConfig, ConfigError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(UserConfig::default()),
        Err(error) => Err(ConfigError::read(path, error)),
        Ok(_) => Err(ConfigError::read(path, metadata_error)),
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
    /// The path exists and is not a regular file.
    NotAFile {
        /// Config path that is a directory or other non-file.
        path: PathBuf,
    },
    /// The regular file is larger than [`MAX_CONFIG_BYTES`].
    TooLarge {
        /// Config file that was not read.
        path: PathBuf,
        /// Size reported by metadata, in bytes.
        len: u64,
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

    fn not_a_file(path: &Path) -> Self {
        Self::NotAFile {
            path: path.to_path_buf(),
        }
    }

    fn too_large(path: &Path, len: u64) -> Self {
        Self::TooLarge {
            path: path.to_path_buf(),
            len,
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
            Self::NotAFile { path } => write!(
                formatter,
                "Probe's user configuration at {} is not a regular file",
                path.display()
            ),
            Self::TooLarge { path, len } => write!(
                formatter,
                "Probe's user configuration at {} is too large ({len} bytes; limit is {MAX_CONFIG_BYTES} bytes)",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Location(_) | Self::NotAFile { .. } | Self::TooLarge { .. } => None,
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
    #[test]
    fn settings_save_round_trips_all_theme_modes_and_preserves_unknown_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested/config.toml");
        let mut previous = ThemeMode::System;
        for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System] {
            UserConfig::save_theme_to(&path, previous, mode).unwrap();
            assert_eq!(UserConfig::load_from(&path).unwrap().theme, mode);
            previous = mode;
        }
        fs::write(
            &path,
            "theme = \"system\"\nfuture = \"keep\"\n[extra]\nvalue = 42\n",
        )
        .unwrap();
        UserConfig::save_theme_to(&path, ThemeMode::System, ThemeMode::Dark).unwrap();
        let document: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(document["future"].as_str(), Some("keep"));
        assert_eq!(document["extra"]["value"].as_integer(), Some(42));
    }

    #[test]
    fn settings_save_preserves_comments_order_and_spacing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let source = concat!(
            "# User preferences\n",
            "zebra = 'leave this alone'\n\n",
            "# Follow the desktop by default\n",
            "  theme  =   \"system\"  # appearance\n\n",
            "alpha = { x=1, y = 2 } # keep spacing\n\n",
            "[extra] # future settings\n",
            "values = [\n  3, # keep this comment\n  2,\n]\n",
        );
        fs::write(&path, source).unwrap();
        UserConfig::save_theme_to(&path, ThemeMode::System, ThemeMode::Dark).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            source.replace("\"system\"", "\"dark\"")
        );
        assert_eq!(UserConfig::load_from(&path).unwrap().theme, ThemeMode::Dark);
    }

    #[test]
    fn settings_save_adds_missing_theme_without_reformatting_existing_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let source = "# Preferences\nfuture  =  [1,2]  # keep\n\n[extra]\nvalue='unchanged'\n";
        fs::write(&path, source).unwrap();
        UserConfig::save_theme_to(&path, ThemeMode::System, ThemeMode::Light).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert_eq!(saved.replace("theme = \"light\"\n", ""), source);
        assert_eq!(
            UserConfig::load_from(&path).unwrap().theme,
            ThemeMode::Light
        );
    }

    #[test]
    fn settings_save_refuses_external_theme_changes_and_invalid_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        for source in [
            "theme = \"light\"\n",
            "theme = \"invalid\"\n",
            "invalid = [",
        ] {
            fs::write(&path, source).unwrap();
            assert!(UserConfig::save_theme_to(&path, ThemeMode::System, ThemeMode::Dark).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }

    #[cfg(unix)]
    #[test]
    fn settings_save_follows_dotfiles_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("dotfiles.toml");
        let link = temp.path().join("config.toml");
        fs::write(&target, "theme = \"system\"\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        UserConfig::save_theme_to(&link, ThemeMode::System, ThemeMode::Dark).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            UserConfig::load_from(&target).unwrap().theme,
            ThemeMode::Dark
        );
    }

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

    #[test]
    fn directory_config_path_is_not_a_regular_file() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        fs::create_dir(&path).unwrap();

        let error = UserConfig::load_from(&path).unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, ConfigError::NotAFile { .. }), "{message}");
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn oversized_config_file_is_rejected() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        fs::write(&path, vec![b' '; MAX_CONFIG_BYTES as usize]).unwrap();
        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());

        fs::write(&path, vec![b'a'; MAX_CONFIG_BYTES as usize + 1]).unwrap();
        let error = UserConfig::load_from(&path).unwrap_err();
        let message = error.to_string();
        assert!(
            matches!(error, ConfigError::TooLarge { len, .. } if len == MAX_CONFIG_BYTES + 1),
            "{message}"
        );
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains(&MAX_CONFIG_BYTES.to_string()), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_a_regular_file_loads() {
        let root = TempDir::new();
        let target = root.0.join("real.toml");
        fs::write(&target, "theme = \"light\"\n").unwrap();
        let path = root.0.join("config.toml");
        std::os::unix::fs::symlink(&target, &path).unwrap();

        assert_eq!(
            UserConfig::load_from(&path).unwrap().theme,
            ThemeMode::Light
        );
    }

    #[test]
    fn theme_loading_uses_system_by_default_and_accepts_each_mode() {
        assert_eq!(UserConfig::default().theme, ThemeMode::System);
        for (contents, expected) in [
            ("# Probe user configuration\n", ThemeMode::System),
            ("theme = \"system\"\n", ThemeMode::System),
            ("theme = \"light\"\n", ThemeMode::Light),
            ("theme = \"dark\"\n", ThemeMode::Dark),
        ] {
            let root = TempDir::new();
            let path = root.0.join("config.toml");
            fs::write(&path, contents).unwrap();
            assert_eq!(
                UserConfig::load_from(&path).unwrap().theme,
                expected,
                "{contents:?}"
            );
        }
    }

    #[test]
    fn invalid_theme_is_a_config_parse_error() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        fs::write(&path, "theme = \"blue\"\n").unwrap();

        let error = UserConfig::load_from(&path).unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, ConfigError::Invalid { .. }), "{message}");
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn startup_keeps_a_parsed_theme_and_defaults_after_a_parse_error() {
        let root = TempDir::new();
        let path = root.0.join("config.toml");
        fs::write(&path, "theme = \"dark\"\n").unwrap();

        let loaded = LoadedUserConfig::from_load_result(UserConfig::load_from(&path));
        assert_eq!(loaded.config.theme, ThemeMode::Dark);
        assert_eq!(loaded.error, None);

        fs::write(&path, "theme = \"blue\"\n").unwrap();
        let loaded = LoadedUserConfig::from_load_result(UserConfig::load_from(&path));
        assert_eq!(loaded.config, UserConfig::default());
        let message = loaded.error.expect("parse error is recorded for startup");
        assert!(message.contains(&path.display().to_string()), "{message}");
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
