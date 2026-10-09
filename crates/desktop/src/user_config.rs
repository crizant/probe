//! Desktop user configuration.
//!
//! The file is optional TOML shared by a future settings UI. It is not an
//! OpenCollection document and it does not store collection variables. The CLI
//! does not load it. Parsing and path resolution stay in this module so GPUI
//! views never read the file.

use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const DIRECTORY: &str = "probe";
const FILE_NAME: &str = "config.toml";

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
        let env = ProcessConfigEnv::from_process();
        Self::load_with(&env.as_input())
    }

    /// Loads the file selected by `input`.
    ///
    /// A missing file returns [`UserConfig::default`] and does not create a file.
    pub(crate) fn load_with(input: &ConfigPathInput<'_>) -> Result<Self, ConfigError> {
        Self::load_from(&resolve_config_path(input)?)
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
        source: toml::de::Error,
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
            source,
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
            Self::Invalid { source, .. } => Some(source),
        }
    }
}

/// Operating system rules for the config file location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigPlatform {
    /// `~/.config/probe/config.toml`, or `$XDG_CONFIG_HOME` when it is set.
    MacOs,
    /// `$XDG_CONFIG_HOME/probe/config.toml`, falling back to `~/.config`.
    Linux,
    /// `%APPDATA%\probe\config.toml`.
    Windows,
}

/// Injected environment used to resolve the config path.
///
/// `None` means the variable is unset. `Some` empty value means it is set but
/// empty. Callers can exercise every platform without changing the process
/// environment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ConfigPathInput<'a> {
    /// Platform path rules to apply.
    pub(crate) platform: ConfigPlatform,
    /// Home directory, used by the macOS and Linux fallbacks.
    pub(crate) home: Option<&'a Path>,
    /// `XDG_CONFIG_HOME` when the variable is present.
    pub(crate) xdg_config_home: Option<&'a std::ffi::OsStr>,
    /// `APPDATA` when the variable is present.
    pub(crate) appdata: Option<&'a std::ffi::OsStr>,
}

/// Resolves the config file path from injected platform and environment values.
///
/// This does not read or create the file.
pub(crate) fn resolve_config_path(input: &ConfigPathInput<'_>) -> Result<PathBuf, ConfigError> {
    match input.platform {
        ConfigPlatform::Windows => resolve_windows(input),
        ConfigPlatform::MacOs | ConfigPlatform::Linux => resolve_unix(input),
    }
}

fn resolve_unix(input: &ConfigPathInput<'_>) -> Result<PathBuf, ConfigError> {
    if let Some(xdg) = non_empty(input.xdg_config_home) {
        if !is_absolute(xdg, input.platform) {
            return Err(ConfigError::Location(format!(
                "XDG_CONFIG_HOME is '{}', which is not an absolute path",
                xdg.to_string_lossy()
            )));
        }
        return Ok(config_file(xdg, input.platform));
    }
    let home = absolute_home(input)?;
    let mut directory = without_trailing_separator(home.as_os_str(), input.platform);
    push_segment(&mut directory, ".config", input.platform);
    Ok(config_file(&directory, input.platform))
}

fn resolve_windows(input: &ConfigPathInput<'_>) -> Result<PathBuf, ConfigError> {
    let Some(appdata) = non_empty(input.appdata) else {
        return Err(ConfigError::Location(
            "APPDATA is not set or is empty; Probe looks for %APPDATA%\\probe\\config.toml"
                .to_owned(),
        ));
    };
    if !is_absolute(appdata, ConfigPlatform::Windows) {
        return Err(ConfigError::Location(format!(
            "APPDATA is '{}', which is not an absolute path",
            appdata.to_string_lossy()
        )));
    }
    Ok(config_file(appdata, ConfigPlatform::Windows))
}

fn absolute_home<'a>(input: &'a ConfigPathInput<'_>) -> Result<&'a Path, ConfigError> {
    let Some(home) = input.home.filter(|path| !path.as_os_str().is_empty()) else {
        return Err(ConfigError::Location(
            "the home directory is not available; set HOME to an absolute path".to_owned(),
        ));
    };
    if !is_absolute(home.as_os_str(), input.platform) {
        return Err(ConfigError::Location(format!(
            "the home directory '{}' must be an absolute path",
            home.display()
        )));
    }
    Ok(home)
}

fn non_empty(value: Option<&std::ffi::OsStr>) -> Option<&std::ffi::OsStr> {
    value.filter(|value| !value.is_empty())
}

fn config_file(directory: &std::ffi::OsStr, platform: ConfigPlatform) -> PathBuf {
    let mut path = without_trailing_separator(directory, platform);
    push_segment(&mut path, DIRECTORY, platform);
    push_segment(&mut path, FILE_NAME, platform);
    PathBuf::from(path)
}

fn without_trailing_separator(
    path: &std::ffi::OsStr,
    platform: ConfigPlatform,
) -> std::ffi::OsString {
    let text = path.to_string_lossy();
    let trimmed = match platform {
        ConfigPlatform::Windows => text.trim_end_matches(['\\', '/']),
        ConfigPlatform::MacOs | ConfigPlatform::Linux => text.trim_end_matches('/'),
    };
    if trimmed.is_empty() {
        return std::ffi::OsString::from(separator(platform));
    }
    if trimmed.len() == text.len() {
        path.to_os_string()
    } else {
        trimmed.into()
    }
}

fn push_segment(path: &mut std::ffi::OsString, segment: &str, platform: ConfigPlatform) {
    if !path.is_empty() && !ends_with_separator(path, platform) {
        path.push(separator(platform));
    }
    path.push(segment);
}

fn separator(platform: ConfigPlatform) -> &'static str {
    match platform {
        ConfigPlatform::Windows => "\\",
        ConfigPlatform::MacOs | ConfigPlatform::Linux => "/",
    }
}

fn ends_with_separator(path: &std::ffi::OsStr, platform: ConfigPlatform) -> bool {
    let bytes = path.as_encoded_bytes();
    match platform {
        ConfigPlatform::Windows => bytes.ends_with(br"\") || bytes.ends_with(b"/"),
        ConfigPlatform::MacOs | ConfigPlatform::Linux => bytes.ends_with(b"/"),
    }
}

fn is_absolute(path: &std::ffi::OsStr, platform: ConfigPlatform) -> bool {
    match platform {
        ConfigPlatform::Windows => is_windows_absolute(path),
        ConfigPlatform::MacOs | ConfigPlatform::Linux => {
            path.as_encoded_bytes().first() == Some(&b'/')
        }
    }
}

fn is_windows_absolute(path: &std::ffi::OsStr) -> bool {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        return true;
    }
    (bytes.starts_with(br"\\") || bytes.starts_with(b"//")) && bytes.len() > 2
}

struct ProcessConfigEnv {
    platform: ConfigPlatform,
    home: Option<PathBuf>,
    xdg_config_home: Option<std::ffi::OsString>,
    appdata: Option<std::ffi::OsString>,
}

impl ProcessConfigEnv {
    fn from_process() -> Self {
        Self {
            platform: current_platform(),
            home: home_dir(),
            xdg_config_home: env::var_os("XDG_CONFIG_HOME"),
            appdata: env::var_os("APPDATA"),
        }
    }

    fn as_input(&self) -> ConfigPathInput<'_> {
        ConfigPathInput {
            platform: self.platform,
            home: self.home.as_deref(),
            xdg_config_home: self.xdg_config_home.as_deref(),
            appdata: self.appdata.as_deref(),
        }
    }
}

fn current_platform() -> ConfigPlatform {
    if cfg!(target_os = "windows") {
        ConfigPlatform::Windows
    } else if cfg!(target_os = "macos") {
        ConfigPlatform::MacOs
    } else {
        ConfigPlatform::Linux
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                let drive = env::var_os("HOMEDRIVE")?;
                let path = env::var_os("HOMEPATH")?;
                if drive.is_empty() || path.is_empty() {
                    return None;
                }
                let mut combined = drive;
                combined.push(path);
                Some(PathBuf::from(combined))
            })
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "probe-desktop-user-config-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn expect_path(input: ConfigPathInput<'_>, expected: &str) {
        let path = resolve_config_path(&input).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(path.as_os_str(), OsStr::new(expected), "{path:?}");
    }

    fn expect_error(input: ConfigPathInput<'_>, fragment: &str) {
        let message = resolve_config_path(&input)
            .expect_err("path resolution should fail")
            .to_string();
        assert!(
            message.contains(fragment),
            "error `{message}` should contain `{fragment}`"
        );
    }

    #[test]
    fn missing_file_returns_defaults_without_creating_it() {
        let root = TempDir::new();
        let path = root.path().join("probe").join("config.toml");

        assert!(!path.exists());
        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());
        assert!(!path.exists());
        assert!(!root.path().join("probe").exists());
    }

    #[test]
    fn valid_toml_loads_defaults() {
        let root = TempDir::new();
        let path = root.path().join("config.toml");
        fs::write(&path, "# Probe user configuration\n").unwrap();

        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());

        let serialized = toml::to_string(&UserConfig::default()).unwrap();
        fs::write(&path, serialized).unwrap();
        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());

        fs::write(&path, "").unwrap();
        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());
    }

    #[test]
    fn partial_toml_omitting_keys_uses_defaults() {
        let root = TempDir::new();
        let path = root.path().join("config.toml");
        // A partial file lists none of the settings. Omitted keys stay at Default.
        fs::write(
            &path,
            "# Only some settings are listed in a partial file.\n# This file sets none of them.\n",
        )
        .unwrap();

        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());
    }

    #[test]
    fn malformed_toml_error_includes_the_config_path() {
        let root = TempDir::new();
        let path = root.path().join("config.toml");
        fs::write(&path, "timeout = [\n").unwrap();

        let message = UserConfig::load_from(&path).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains("invalid"), "{message}");

        fs::write(&path, "\"not a table\"\n").unwrap();
        let message = UserConfig::load_from(&path).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let root = TempDir::new();
        let path = root.path().join("config.toml");
        fs::write(&path, "renderer = \"gpu\"\n\n[window]\nwidth = 1200\n").unwrap();

        assert_eq!(UserConfig::load_from(&path).unwrap(), UserConfig::default());
    }

    #[test]
    fn directory_read_error_includes_the_config_path() {
        let root = TempDir::new();
        let path = root.path().join("config.toml");
        fs::create_dir(&path).unwrap();

        let message = UserConfig::load_from(&path).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlink_reports_the_config_path() {
        let root = TempDir::new();
        let path = root.path().join("config.toml");
        std::os::unix::fs::symlink(root.path().join("missing.toml"), &path).unwrap();

        let message = UserConfig::load_from(&path).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn linux_and_macos_paths_follow_xdg_and_home_fallback() {
        let home = Path::new("/Users/ada");
        let xdg = OsStr::new("/custom/config");

        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::Linux,
                home: Some(home),
                xdg_config_home: Some(xdg),
                appdata: None,
            },
            "/custom/config/probe/config.toml",
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::Linux,
                home: Some(home),
                xdg_config_home: None,
                appdata: None,
            },
            "/Users/ada/.config/probe/config.toml",
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::Linux,
                home: Some(home),
                xdg_config_home: Some(OsStr::new("")),
                appdata: None,
            },
            "/Users/ada/.config/probe/config.toml",
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::MacOs,
                home: Some(home),
                xdg_config_home: None,
                appdata: Some(OsStr::new("/ignored")),
            },
            "/Users/ada/.config/probe/config.toml",
        );
        let macos = resolve_config_path(&ConfigPathInput {
            platform: ConfigPlatform::MacOs,
            home: Some(home),
            xdg_config_home: None,
            appdata: None,
        })
        .unwrap();
        assert!(
            !macos
                .as_os_str()
                .to_string_lossy()
                .contains("Application Support"),
            "{macos:?}"
        );
        assert!(
            !macos.as_os_str().to_string_lossy().contains("Library"),
            "{macos:?}"
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::MacOs,
                home: Some(home),
                xdg_config_home: Some(xdg),
                appdata: None,
            },
            "/custom/config/probe/config.toml",
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::MacOs,
                home: None,
                xdg_config_home: Some(OsStr::new("/custom/config/")),
                appdata: None,
            },
            "/custom/config/probe/config.toml",
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::Linux,
                home: None,
                xdg_config_home: Some(OsStr::new("/")),
                appdata: None,
            },
            "/probe/config.toml",
        );
    }

    #[test]
    fn relative_xdg_config_home_is_rejected() {
        expect_error(
            ConfigPathInput {
                platform: ConfigPlatform::Linux,
                home: Some(Path::new("/Users/ada")),
                xdg_config_home: Some(OsStr::new("relative/config")),
                appdata: None,
            },
            "relative/config",
        );
        expect_error(
            ConfigPathInput {
                platform: ConfigPlatform::MacOs,
                home: Some(Path::new("/Users/ada")),
                xdg_config_home: Some(OsStr::new("relative/config")),
                appdata: None,
            },
            "XDG_CONFIG_HOME",
        );
    }

    #[test]
    fn missing_or_relative_home_is_an_error_when_the_fallback_needs_it() {
        expect_error(
            ConfigPathInput {
                platform: ConfigPlatform::Linux,
                home: None,
                xdg_config_home: None,
                appdata: None,
            },
            "home directory",
        );
        expect_error(
            ConfigPathInput {
                platform: ConfigPlatform::MacOs,
                home: Some(Path::new("ada")),
                xdg_config_home: None,
                appdata: None,
            },
            "ada",
        );
    }

    #[test]
    fn windows_uses_appdata_and_ignores_xdg_and_home() {
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::Windows,
                home: Some(Path::new("/Users/ada")),
                xdg_config_home: Some(OsStr::new("/custom/config")),
                appdata: Some(OsStr::new(r"C:\Users\ada\AppData\Roaming")),
            },
            r"C:\Users\ada\AppData\Roaming\probe\config.toml",
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::Windows,
                home: None,
                xdg_config_home: None,
                appdata: Some(OsStr::new(r"D:/ProbeData/")),
            },
            r"D:/ProbeData\probe\config.toml",
        );
        expect_path(
            ConfigPathInput {
                platform: ConfigPlatform::Windows,
                home: None,
                xdg_config_home: None,
                appdata: Some(OsStr::new(r"C:\")),
            },
            r"C:\probe\config.toml",
        );
        expect_error(
            ConfigPathInput {
                platform: ConfigPlatform::Windows,
                home: Some(Path::new(r"C:\Users\ada")),
                xdg_config_home: Some(OsStr::new(r"C:\xdg")),
                appdata: None,
            },
            "APPDATA",
        );
        expect_error(
            ConfigPathInput {
                platform: ConfigPlatform::Windows,
                home: None,
                xdg_config_home: None,
                appdata: Some(OsStr::new(r"Roaming")),
            },
            "Roaming",
        );
    }

    #[test]
    fn load_with_reads_the_resolved_file() {
        let root = TempDir::new();
        let path = root.path().join("probe").join("config.toml");
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, "future = true\n").unwrap();

        let loaded = UserConfig::load_with(&ConfigPathInput {
            platform: ConfigPlatform::Linux,
            home: None,
            xdg_config_home: Some(root.path().as_os_str()),
            appdata: None,
        })
        .unwrap();
        assert_eq!(loaded, UserConfig::default());

        fs::write(&path, "=\n").unwrap();
        let message = UserConfig::load_with(&ConfigPathInput {
            platform: ConfigPlatform::Linux,
            home: None,
            xdg_config_home: Some(root.path().as_os_str()),
            appdata: None,
        })
        .unwrap_err()
        .to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }
}
