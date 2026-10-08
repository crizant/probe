use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use directories::BaseDirs;
use serde_json::json;

use crate::{CONFIGURATION_EXIT_CODE, CliError, CommandOutput, PERSISTENCE_EXIT_CODE, version};

const SKILL: &str = include_str!("../../../.agents/skills/probe/SKILL.md");
const CLI_REFERENCE: &str = include_str!("../../../docs/CLI.md");

pub(crate) fn install(force: bool) -> Result<CommandOutput, CliError> {
    let directories = BaseDirs::new();
    install_at(directories.as_ref().map(BaseDirs::home_dir), force)
}

// Keep the home directory injectable without changing process-wide environment.
fn install_at(home: Option<&Path>, force: bool) -> Result<CommandOutput, CliError> {
    let home = home
        .filter(|path| path.is_absolute())
        .ok_or_else(|| CliError {
            category: "home_directory_unavailable",
            message: "cannot resolve the current user's home directory".to_owned(),
            exit_code: CONFIGURATION_EXIT_CODE,
            details: None,
        })?;
    let agents = home.join(".agents");
    let skills = agents.join("skills");
    let destination = skills.join("probe");
    for directory in [&agents, &skills] {
        ensure_directory(directory)?;
    }

    // An exclusive directory creation prevents concurrent normal installers from
    // both considering the destination new and overwriting one another.
    let existed = match fs::create_dir(&destination) {
        Ok(()) => false,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            check_directory(&destination)?;
            true
        }
        Err(error) => return Err(filesystem_error(&destination, error)),
    };
    let references = destination.join("references");
    if references
        .try_exists()
        .map_err(|error| filesystem_error(&references, error))?
    {
        check_directory(&references)?;
    }
    let files = [
        (destination.join("SKILL.md"), SKILL.as_bytes()),
        (references.join("cli.md"), CLI_REFERENCE.as_bytes()),
    ];
    let originals = files
        .iter()
        .map(|(path, _)| read_owned_file(path))
        .collect::<Result<Vec<_>, _>>()?;
    if originals
        .iter()
        .zip(&files)
        .all(|(original, (_, contents))| original.as_deref() == Some(*contents))
    {
        return Ok(output(destination, false));
    }
    if existed && !force {
        return Err(CliError {
            category: "agent_skill_exists",
            message: format!(
                "Probe agent skill at {} differs or is incomplete; use 'probe agent skill install --force' to replace its files",
                destination.display()
            ),
            exit_code: PERSISTENCE_EXIT_CODE,
            details: None,
        });
    }
    ensure_directory(&references)?;

    // Stage both complete files before publishing either. Each commit is atomic;
    // a failed commit can leave mixed versions, but never a truncated owned file.
    let staged = files
        .iter()
        .map(|(path, contents)| {
            let mut file =
                AtomicWriteFile::open(path).map_err(|error| filesystem_error(path, error))?;
            file.write_all(contents)
                .map_err(|error| filesystem_error(path, error))?;
            Ok(file)
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    for ((path, _), original) in files.iter().zip(&originals) {
        if read_owned_file(path)? != *original {
            return Err(CliError {
                category: "agent_skill_modified",
                message: format!(
                    "agent skill file changed during installation: {}",
                    path.display()
                ),
                exit_code: PERSISTENCE_EXIT_CODE,
                details: None,
            });
        }
    }
    for ((path, _), file) in files.iter().zip(staged) {
        file.commit()
            .map_err(|error| filesystem_error(path, error))?;
    }
    Ok(output(destination, true))
}

fn output(path: PathBuf, changed: bool) -> CommandOutput {
    let action = if changed {
        "Installed"
    } else {
        "Already installed"
    };
    CommandOutput {
        human: format!("{action} Probe agent skill\nPath: {}\n", path.display()),
        json: json!({ "installed": true, "changed": changed, "path": path, "version": version() }),
    }
}

fn ensure_directory(path: &Path) -> Result<(), CliError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => check_directory(path),
        Err(error) => Err(filesystem_error(path, error)),
    }
}

fn check_directory(path: &Path) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| filesystem_error(path, error))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(filesystem_error(
            path,
            io::Error::other("expected a directory; symlinks are refused"),
        ))
    }
}

fn read_owned_file(path: &Path) -> Result<Option<Vec<u8>>, CliError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => fs::read(path)
            .map(Some)
            .map_err(|error| filesystem_error(path, error)),
        Ok(_) => Err(filesystem_error(
            path,
            io::Error::other("expected a regular file; symlinks are refused"),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(filesystem_error(path, error)),
    }
}

fn filesystem_error(path: &Path, error: io::Error) -> CliError {
    CliError {
        category: "persistence_error",
        message: format!(
            "cannot install Probe agent skill at {}: {error}",
            path.display()
        ),
        exit_code: PERSISTENCE_EXIT_CODE,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JSON_SCHEMA_VERSION, RunOutput};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Home(PathBuf);

    impl Home {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "probe-agent-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn skill(&self) -> PathBuf {
            self.0.join(".agents").join("skills").join("probe")
        }

        fn install(&self, force: bool) -> Result<CommandOutput, CliError> {
            install_at(Some(&self.0), force)
        }

        fn assert_resources(&self) {
            assert_eq!(
                fs::read(self.skill().join("SKILL.md")).unwrap(),
                SKILL.as_bytes()
            );
            assert_eq!(
                fs::read(self.skill().join("references/cli.md")).unwrap(),
                CLI_REFERENCE.as_bytes()
            );
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn installs_canonical_resources_and_reinstalls_idempotently() {
        let home = Home::new();
        let output = home.install(false).unwrap();
        let json: serde_json::Value = serde_json::from_str(&output.render(true, false)).unwrap();
        assert_eq!(
            json,
            json!({
                "schemaVersion": JSON_SCHEMA_VERSION,
                "installed": true,
                "changed": true,
                "path": home.skill(),
                "version": version(),
            })
        );
        home.assert_resources();
        // Compare with the only checked-in source files, not another installed copy.
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(
            fs::read(home.skill().join("SKILL.md")).unwrap(),
            fs::read(repository.join(".agents/skills/probe/SKILL.md")).unwrap()
        );
        assert_eq!(
            fs::read(home.skill().join("references/cli.md")).unwrap(),
            fs::read(repository.join("docs/CLI.md")).unwrap()
        );
        for force in [false, true] {
            let output = home.install(force).unwrap();
            assert_eq!(
                output.json,
                json!({
                    "installed": true,
                    "changed": false,
                    "path": home.skill(),
                    "version": version(),
                })
            );
            assert!(output.human.contains(&home.skill().display().to_string()));
        }
    }

    #[test]
    fn protects_modified_and_incomplete_installations_and_preserves_unknown_files() {
        let home = Home::new();
        home.install(false).unwrap();
        let other = home.0.join(".agents/skills/other");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("SKILL.md"), "other skill").unwrap();
        fs::write(home.skill().join("notes.txt"), "user notes").unwrap();
        for relative in ["SKILL.md", "references/cli.md"] {
            let path = home.skill().join(relative);
            fs::write(&path, "modified").unwrap();
            let error = home.install(false).unwrap_err();
            assert_eq!(error.category, "agent_skill_exists");
            assert_eq!(error.exit_code, PERSISTENCE_EXIT_CODE);
            assert!(error.message.contains("probe agent skill install --force"));
            assert_eq!(fs::read_to_string(&path).unwrap(), "modified");
            let output = home.install(true).unwrap();
            assert_eq!(output.json["installed"], true);
            assert_eq!(output.json["changed"], true);
            home.assert_resources();
            fs::remove_file(&path).unwrap();
            assert_eq!(
                home.install(false).unwrap_err().category,
                "agent_skill_exists"
            );
            home.install(true).unwrap();
            home.assert_resources();
        }
        assert_eq!(
            fs::read_to_string(home.skill().join("notes.txt")).unwrap(),
            "user notes"
        );
        assert_eq!(
            fs::read_to_string(other.join("SKILL.md")).unwrap(),
            "other skill"
        );
    }

    #[test]
    fn home_resolution_and_filesystem_errors_use_normal_cli_errors() {
        for home in [None, Some(Path::new("relative"))] {
            let error = install_at(home, false).unwrap_err();
            let output = RunOutput::failure(error, true);
            assert_eq!(output.exit_code, CONFIGURATION_EXIT_CODE);
            let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(json["schemaVersion"], JSON_SCHEMA_VERSION);
            assert_eq!(json["error"]["category"], "home_directory_unavailable");
        }
        let home = Home::new();
        fs::write(home.0.join(".agents"), "blocked").unwrap();
        // Unix reports a non-directory parent as an I/O error; Windows reports
        // NotFound. The installer rejects the blocked parent on both platforms.
        #[cfg(unix)]
        assert_eq!(
            read_owned_file(&home.0.join(".agents/SKILL.md"))
                .unwrap_err()
                .category,
            "persistence_error"
        );
        let error = home.install(false).unwrap_err();
        assert_eq!(error.category, "persistence_error");
        assert_eq!(error.exit_code, PERSISTENCE_EXIT_CODE);
        fs::remove_file(home.0.join(".agents")).unwrap();
        home.install(false).unwrap();
        fs::write(home.skill().join("SKILL.md"), "keep me").unwrap();
        fs::remove_file(home.skill().join("references/cli.md")).unwrap();
        fs::create_dir(home.skill().join("references/cli.md")).unwrap();
        assert_eq!(
            home.install(true).unwrap_err().category,
            "persistence_error"
        );
        assert_eq!(
            fs::read_to_string(home.skill().join("SKILL.md")).unwrap(),
            "keep me"
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_directories_and_files() {
        use std::os::unix::fs::symlink;
        let home = Home::new();
        let outside = Home::new();
        symlink(&outside.0, home.0.join(".agents")).unwrap();
        assert_eq!(
            home.install(true).unwrap_err().category,
            "persistence_error"
        );
        assert!(!outside.0.join("skills").exists());
        fs::remove_file(home.0.join(".agents")).unwrap();
        home.install(false).unwrap();
        fs::write(outside.0.join("owned.md"), "untouched").unwrap();
        fs::remove_file(home.skill().join("SKILL.md")).unwrap();
        symlink(outside.0.join("owned.md"), home.skill().join("SKILL.md")).unwrap();
        assert_eq!(
            home.install(true).unwrap_err().category,
            "persistence_error"
        );
        assert_eq!(
            fs::read_to_string(outside.0.join("owned.md")).unwrap(),
            "untouched"
        );
    }
}
