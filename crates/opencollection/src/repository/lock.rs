use std::{
    fs,
    fs::OpenOptions,
    io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::SaveError;

pub(crate) struct SaveLock {
    // Closing the file releases the OS lock. Keep the sidecar: unlinking it
    // could split a waiter holding this inode from a new writer's lock.
    _file: fs::File,
}

impl SaveLock {
    pub(crate) fn acquire(destination: &Path) -> Result<Self, SaveError> {
        Self::acquire_path(&lock_path(destination)?)
    }

    pub(super) fn acquire_existing(destination: &Path) -> Result<(Self, PathBuf), SaveError> {
        let destination = fs::canonicalize(destination).map_err(|source| SaveError::Io {
            path: destination.to_owned(),
            source,
        })?;
        let lock = Self::acquire_path(&resolved_lock_path(&destination)?)?;
        Ok((lock, destination))
    }

    pub(crate) fn acquire_pair(
        first: &Path,
        second: &Path,
    ) -> Result<(Self, Option<Self>), SaveError> {
        let mut paths = [lock_path(first)?, lock_path(second)?];
        paths.sort();
        let [first, second] = paths;
        let lock = Self::acquire_path(&first)?;
        let other = if first == second {
            None
        } else {
            Some(Self::acquire_path(&second)?)
        };
        Ok((lock, other))
    }

    fn acquire_path(path: &Path) -> Result<Self, SaveError> {
        let map_io = |source| SaveError::Io {
            path: path.to_owned(),
            source,
        };
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Reject final-component symlinks atomically; don't hang opening a FIFO.
            options.mode(0o600).custom_flags(
                rustix::fs::OFlags::NOFOLLOW
                    .union(rustix::fs::OFlags::NONBLOCK)
                    .bits() as i32,
            );
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Open the reparse point itself so validation cannot follow a symlink.
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options.open(path).map_err(map_io)?;
        validate_lock(&file.metadata().map_err(map_io)?).map_err(map_io)?;
        file.lock().map_err(map_io)?;
        Ok(Self { _file: file })
    }
}

// Use a stable sidecar in the document's canonical parent, independent of TMPDIR
// and user identity. No shared temporary directory or private directory is needed.
// Workspace ancestors must be trusted: writers able to unlink these files can
// defeat advisory locking, just as they can change the collection itself.
fn lock_path(destination: &Path) -> Result<PathBuf, SaveError> {
    // Existing aliases (including case aliases on case-insensitive filesystems)
    // must resolve to one sidecar, or a rename could lock the same file twice.
    let canonical = match fs::canonicalize(destination) {
        Ok(path) => Some(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(SaveError::Io {
                path: destination.to_owned(),
                source,
            });
        }
    };
    resolved_lock_path(canonical.as_deref().unwrap_or(destination))
}

// The caller has already resolved an existing document, or established that it
// does not exist. Do not resolve the document again after choosing its identity.
fn resolved_lock_path(destination: &Path) -> Result<PathBuf, SaveError> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|source| SaveError::Io {
        path: destination.to_owned(),
        source,
    })?;
    let mut name = std::ffi::OsString::from(".");
    let filename = destination.file_name().ok_or_else(|| SaveError::Io {
        path: destination.to_owned(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "lock destination has no filename",
        ),
    })?;
    name.push(filename);
    name.push(".probe-lock");
    let readable = parent.join(name);
    // Ask the actual filesystem, rather than assuming every volume has NAME_MAX
    // 255 or counting UTF-8 bytes on Windows. Metadata lookup never follows the
    // sidecar's final symlink and does not create or modify a lock file.
    #[cfg(windows)]
    let within_component_limit = windows_component_fits(&readable);
    #[cfg(not(windows))]
    let within_component_limit = true;
    if within_component_limit
        && sidecar_name_fits(fs::symlink_metadata(&readable)).map_err(|source| SaveError::Io {
            path: readable.clone(),
            source,
        })?
    {
        return Ok(readable);
    }
    let identity = parent.join(filename);
    let mut hash = Sha256::new();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for value in identity.as_os_str().encode_wide() {
            hash.update(value.to_le_bytes());
        }
    }
    #[cfg(not(windows))]
    hash.update(identity.as_os_str().as_encoded_bytes());
    use std::fmt::Write;
    let mut name = String::with_capacity(76);
    name.push('.');
    for byte in hash.finalize() {
        write!(&mut name, "{byte:02x}").expect("writing to a String cannot fail");
    }
    name.push_str(".probe-lock");
    Ok(parent.join(name))
}

// Only a component-length error permits changing lock identity. Preserve other
// I/O failures rather than silently switching to a different sidecar.
fn sidecar_name_fits(metadata: io::Result<fs::Metadata>) -> io::Result<bool> {
    match metadata {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) if filename_too_long(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

fn filename_too_long(error: &io::Error) -> bool {
    #[cfg(unix)]
    return error.raw_os_error() == Some(rustix::io::Errno::NAMETOOLONG.raw_os_error());
    #[cfg(windows)]
    // ERROR_INVALID_NAME or ERROR_FILENAME_EXCED_RANGE, depending on the volume.
    return windows_filename_too_long(error);
    #[cfg(not(any(unix, windows)))]
    {
        let _ = error;
        false
    }
}

fn validate_lock(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "persistence lock is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "persistence lock must be owned by the current user, private (0600), and have one link; cross-user saves are unsupported",
            ));
        }
    }
    #[cfg(windows)]
    windows_validate_lock(metadata)?;
    Ok(())
}

// The mutation gate runs on macOS. These Windows-only functions are covered by
// native Windows tests, and narrowly excluded from that host's mutation run.
#[cfg(windows)]
fn windows_component_fits(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    path.file_name()
        .expect("sidecar has a filename")
        .encode_wide()
        .count()
        <= 255
}

#[cfg(windows)]
fn windows_filename_too_long(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(123 | 206))
}

#[cfg(windows)]
fn windows_validate_lock(metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "persistence lock is a reparse point",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::{Child, Command},
        time::{Duration, Instant},
    };

    #[allow(dead_code)]
    mod support {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/mod.rs"));
    }

    struct Worker(Child, PathBuf);

    impl Worker {
        fn spawn(directory: &Path, action: &str) -> Self {
            Self::spawn_for(
                directory,
                action,
                &directory.join("a.yml"),
                &directory.join("b.yml"),
            )
        }

        fn spawn_for(directory: &Path, action: &str, a: &Path, b: &Path) -> Self {
            Self(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "repository::lock::tests::process_worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("PROBE_LOCK_TEST_DIRECTORY", directory)
                    .env("PROBE_LOCK_TEST_ACTION", action)
                    .env("PROBE_LOCK_TEST_A", a)
                    .env("PROBE_LOCK_TEST_B", b)
                    // Different temp settings must not create independent lock domains.
                    .env("TMPDIR", directory.join(action))
                    .env("TMP", directory.join(action))
                    .env("TEMP", directory.join(action))
                    .spawn()
                    .unwrap(),
                directory.join(format!("{action}.ready")),
            )
        }

        fn finish(&mut self) {
            wait_for(&self.1);
            // A child has started once ready. Bound deadlock failures below the
            // mutation gate's timeout, even with several serial regressions.
            let deadline = Instant::now() + Duration::from_secs(4);
            loop {
                if let Some(status) = self.0.try_wait().unwrap() {
                    assert!(status.success());
                    return;
                }
                assert!(Instant::now() < deadline, "lock worker timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "worker did not create {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "subprocess entry point"]
    fn process_worker() {
        let directory = PathBuf::from(std::env::var_os("PROBE_LOCK_TEST_DIRECTORY").unwrap());
        let action = std::env::var("PROBE_LOCK_TEST_ACTION").unwrap();
        let a = PathBuf::from(std::env::var_os("PROBE_LOCK_TEST_A").unwrap());
        let b = PathBuf::from(std::env::var_os("PROBE_LOCK_TEST_B").unwrap());
        fs::write(directory.join(format!("{action}.ready")), []).unwrap();
        match action.as_str() {
            "save" => {
                let result =
                    super::super::mutate_existing_document(&a, b"name: original\n", |document| {
                        document["name"] = "child".into();
                        Ok(())
                    });
                assert!(matches!(result, Err(SaveError::ConcurrentModification(_))));
            }
            "same" | "alias" => {
                let second = if action == "same" { &a } else { &b };
                let (_first, second) = SaveLock::acquire_pair(&a, second).unwrap();
                assert!(second.is_none());
            }
            "reverse" | "forward" => {
                let (old, new) = if action == "reverse" {
                    (&b, &a)
                } else {
                    (&a, &b)
                };
                let replacement = probe_core::Environment {
                    name: "renamed".to_owned(),
                    color: None,
                    description: None,
                    extends: None,
                    dot_env_file_path: None,
                    variables: Vec::new(),
                    client_certificates: Vec::new(),
                };
                let result = super::super::environment::persist_unbundled_environment_rename(
                    old,
                    new,
                    b"name: original\n",
                    &replacement,
                    &probe_core::FieldPatch::Unchanged,
                );
                assert!(
                    matches!(result, Err(SaveError::ConcurrentModification(path)) if path == *new)
                );
            }
            _ => panic!("unknown worker action"),
        }
        fs::write(directory.join(format!("{action}.done")), []).unwrap();
    }

    #[test]
    fn processes_serialize_source_checks_and_keep_the_lock_file() {
        let directory = support::temporary_path("lock-processes");
        fs::create_dir(&directory).unwrap();
        let document = directory.join("a.yml");
        fs::write(&document, b"name: original\n").unwrap();
        let lock = SaveLock::acquire(&document).unwrap();
        let mut child = Worker::spawn(&directory, "save");
        wait_for(&directory.join("save.ready"));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!directory.join("save.done").exists());
        fs::write(&document, b"name: parent\n").unwrap();
        drop(lock);
        child.finish();
        assert_eq!(fs::read(&document).unwrap(), b"name: parent\n");
        assert!(lock_path(&document).unwrap().is_file());
        let _lock = SaveLock::acquire(&document).unwrap();
    }

    #[test]
    fn opposite_pairs_wait_for_the_same_first_lock() {
        let directory = support::temporary_path("lock-order");
        fs::create_dir(&directory).unwrap();
        let a = directory.join("a.yml");
        let b = directory.join("b.yml");
        assert_eq!(
            lock_path(&a).unwrap().file_name().unwrap(),
            ".a.yml.probe-lock"
        );
        check_opposite_pairs(&directory, &a, &b);
    }

    fn check_opposite_pairs(directory: &Path, a: &Path, b: &Path) {
        let (a, b) = if lock_path(a).unwrap() <= lock_path(b).unwrap() {
            (a, b)
        } else {
            (b, a)
        };
        fs::write(a, b"name: original\n").unwrap();
        fs::write(b, b"name: original\n").unwrap();
        let first = SaveLock::acquire(a).unwrap();
        let mut reverse = Worker::spawn_for(directory, "reverse", a, b);
        wait_for(&directory.join("reverse.ready"));
        std::thread::sleep(Duration::from_millis(100));
        // Reverse order must wait on A without holding B. This catches B -> A.
        let b_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path(b).unwrap())
            .unwrap();
        b_file.try_lock().expect("reverse pair acquired B before A");
        b_file.unlock().unwrap();
        // Use the production permissions for subsequent opens of the test file.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(lock_path(b).unwrap(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut forward = Worker::spawn_for(directory, "forward", a, b);
        wait_for(&directory.join("forward.ready"));
        drop(first);
        reverse.finish();
        forward.finish();
        Worker::spawn_for(directory, "same", a, b).finish();
        assert_eq!(fs::read(a).unwrap(), b"name: original\n");
        assert_eq!(fs::read(b).unwrap(), b"name: original\n");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_hard_links_and_preexisting_directories() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = support::temporary_path("lock-links");
        fs::create_dir(&directory).unwrap();
        let document = directory.join("a.yml");
        let path = lock_path(&document).unwrap();
        let loop_path = directory.join("loop.yml");
        symlink(&loop_path, &loop_path).unwrap();
        assert!(SaveLock::acquire(&loop_path).is_err());
        let victim = directory.join("victim");
        fs::write(&victim, b"untouched").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&victim, &path).unwrap();
        assert!(SaveLock::acquire(&document).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&victim, &path).unwrap();
        assert!(SaveLock::acquire(&document).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(SaveLock::acquire(&document).is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
    }

    #[cfg(unix)]
    #[test]
    fn private_lock_permissions_and_permission_failures() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = support::temporary_path("lock-permissions");
        fs::create_dir(&directory).unwrap();
        let document = directory.join("a.yml");
        let path = lock_path(&document).unwrap();
        drop(SaveLock::acquire(&document).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(
            matches!(SaveLock::acquire(&document), Err(SaveError::Io { source, .. }) if source.kind() == io::ErrorKind::PermissionDenied)
        );
        // Do not silently chmod or replace an unsafe pre-existing lock.
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o666);
        fs::remove_file(&path).unwrap();
        if !rustix::process::geteuid().is_root() {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();
            let result = SaveLock::acquire(&document);
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(
                matches!(result, Err(SaveError::Io { source, .. }) if source.kind() == io::ErrorKind::PermissionDenied)
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn foreign_owned_private_lock_is_rejected() {
        if !rustix::process::geteuid().is_root() {
            eprintln!("foreign-owner regression requires root to chown a private test file");
            return;
        }
        let directory = support::temporary_path("lock-owner");
        fs::create_dir(&directory).unwrap();
        let document = directory.join("a.yml");
        drop(SaveLock::acquire(&document).unwrap());
        rustix::fs::chown(
            lock_path(&document).unwrap(),
            Some(rustix::process::Uid::from_raw(65534)),
            None,
        )
        .unwrap();
        assert!(
            matches!(SaveLock::acquire(&document), Err(SaveError::Io { source, .. }) if source.kind() == io::ErrorKind::PermissionDenied)
        );
    }

    #[cfg(unix)]
    #[test]
    fn parent_directory_aliases_share_the_lock() {
        use std::os::unix::fs::symlink;
        let directory = support::temporary_path("lock-parent-alias");
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join("real")).unwrap();
        symlink(directory.join("real"), directory.join("alias")).unwrap();
        let document = directory.join("real/a.yml");
        let alias = directory.join("alias/a.yml");
        let _lock = SaveLock::acquire(&document).unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path(&alias).unwrap())
            .unwrap();
        assert!(matches!(
            file.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn existing_document_aliases_are_deduplicated() {
        use std::os::unix::fs::symlink;
        let directory = support::temporary_path("lock-file-alias");
        fs::create_dir(&directory).unwrap();
        let document = directory.join("a.yml");
        let alias = directory.join("b.yml");
        fs::write(&document, b"name: original\n").unwrap();
        symlink(&document, &alias).unwrap();
        Worker::spawn(&directory, "alias").finish();
    }

    #[test]
    fn long_filenames_use_stable_distinct_hashes_and_order_final_locks() {
        let directory = support::temporary_path("long-lock-names");
        fs::create_dir(&directory).unwrap();
        let a = directory.join(format!("{}.yml", "a".repeat(251)));
        let b = directory.join(format!("{}.yml", "b".repeat(251)));
        fs::write(&a, b"name: original\n").unwrap();
        fs::write(&b, b"name: original\n").unwrap();
        let first = lock_path(&a).unwrap();
        let second = lock_path(&b).unwrap();
        assert_ne!(first, second);
        let name = first.file_name().unwrap().to_str().unwrap();
        assert_eq!(name.len(), 76);
        assert!(
            name.strip_prefix('.')
                .unwrap()
                .strip_suffix(".probe-lock")
                .unwrap()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        drop(SaveLock::acquire(&a).unwrap());
        assert_eq!(lock_path(&a).unwrap(), first);
        assert!(first.is_file());
        check_opposite_pairs(&directory, &a, &b);
        // Filesystems count Unicode components differently. Lookup must use the
        // volume's rules; Windows counts UTF-16 units, not UTF-8 bytes.
        let unicode = directory.join(format!("{}.yml", "é".repeat(121)));
        fs::write(&unicode, b"name: original\n").unwrap();
        drop(SaveLock::acquire(&unicode).unwrap());
        assert!(lock_path(&unicode).unwrap().is_file());
        #[cfg(windows)]
        assert_eq!(
            lock_path(&unicode).unwrap().file_name().unwrap(),
            std::ffi::OsStr::new(&format!(".{}.yml.probe-lock", "é".repeat(121)))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let alias = directory.join("alias.yml");
            symlink(&a, &alias).unwrap();
            assert_eq!(lock_path(&alias).unwrap(), first);
            Worker::spawn_for(&directory, "alias", &a, &alias).finish();
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_and_reparse_validation() {
        assert!(windows_component_fits(Path::new(&"a".repeat(255))));
        assert!(!windows_component_fits(Path::new(&"a".repeat(256))));
        assert!(windows_component_fits(Path::new(&"é".repeat(200))));
        assert!(windows_filename_too_long(&io::Error::from_raw_os_error(
            123
        )));
        assert!(windows_filename_too_long(&io::Error::from_raw_os_error(
            206
        )));
        assert!(!windows_filename_too_long(&io::Error::from_raw_os_error(5)));
        let directory = support::temporary_path("windows-lock-reparse");
        fs::create_dir(&directory).unwrap();
        let document = directory.join("a.yml");
        drop(SaveLock::acquire(&document).unwrap());
        let path = lock_path(&document).unwrap();
        assert!(windows_validate_lock(&fs::metadata(&path).unwrap()).is_ok());
        let alias = directory.join("alias");
        match std::os::windows::fs::symlink_file(&path, &alias) {
            Ok(()) => {
                assert!(windows_validate_lock(&fs::symlink_metadata(&alias).unwrap()).is_err())
            }
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("symlink regression requires Windows symlink permission");
            }
            Err(error) => panic!("could not create test symlink: {error}"),
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hashed_fallback_preserves_unrelated_io_failures() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
            io::ErrorKind::Interrupted,
        ] {
            let error =
                sidecar_name_fits(Err(io::Error::new(kind, "metadata failure"))).unwrap_err();
            assert_eq!(error.kind(), kind);
        }
        assert!(sidecar_name_fits(Err(io::Error::from(io::ErrorKind::NotFound))).unwrap());
        #[cfg(unix)]
        let too_long = io::Error::from_raw_os_error(rustix::io::Errno::NAMETOOLONG.raw_os_error());
        #[cfg(windows)]
        let too_long = io::Error::from_raw_os_error(206);
        assert!(!sidecar_name_fits(Err(too_long)).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_its_resolved_target_when_the_source_symlink_is_retargeted() {
        use std::os::unix::fs::symlink;
        let directory = support::temporary_path("save-retargeted-symlink");
        fs::create_dir(&directory).unwrap();
        let original = directory.join("original.yml");
        let other = directory.join("other.yml");
        let alias = directory.join("alias.yml");
        let source = b"name: original\n";
        fs::write(&original, source).unwrap();
        fs::write(&other, source).unwrap();
        symlink(&original, &alias).unwrap();
        super::super::mutate_existing_document(&alias, source, |document| {
            fs::remove_file(&alias).unwrap();
            symlink(&other, &alias).unwrap();
            document["name"] = "saved".into();
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read_link(&alias).unwrap(), other);
        assert_eq!(fs::read(&other).unwrap(), source);
        let saved: serde_yaml_ng::Value =
            serde_yaml_ng::from_slice(&fs::read(&original).unwrap()).unwrap();
        assert_eq!(saved["name"].as_str(), Some("saved"));
    }
}
