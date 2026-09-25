use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{SystemTime, UNIX_EPOCH},
};

static BODY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A process-safe, quota-bounded cache for complete large response bodies.
#[derive(Clone, Debug)]
pub struct ResponseCache {
    inner: Arc<ResponseCacheInner>,
}

#[derive(Debug)]
struct ResponseCacheInner {
    directory: PathBuf,
    quota_bytes: u64,
    session: Mutex<Option<ResponseCacheSession>>,
}

#[derive(Debug)]
struct ResponseCacheSession {
    directory: PathBuf,
    _lease: std::fs::File,
    cleanup: Sender<PathBuf>,
}

impl ResponseCache {
    /// Creates a lazily initialized cache rooted at `directory`.
    #[must_use]
    pub fn new(directory: PathBuf, quota_bytes: u64) -> Self {
        Self {
            inner: Arc::new(ResponseCacheInner {
                directory,
                quota_bytes,
                session: Mutex::new(None),
            }),
        }
    }

    /// Returns the maximum combined size of retained response bodies.
    #[must_use]
    pub fn quota_bytes(&self) -> u64 {
        self.inner.quota_bytes
    }

    /// Initializes the process session and removes cache sessions left by crashes.
    ///
    /// This performs filesystem I/O and should run off a UI thread.
    pub fn initialize(&self) -> io::Result<()> {
        self.ensure_session().map(drop)
    }

    pub(crate) fn reserve(
        &self,
        expected_bytes: Option<u64>,
        minimum_bytes: u64,
    ) -> Result<ResponseCacheReservation, ResponseCacheReservationError> {
        let (session_directory, cleanup) = self
            .ensure_session()
            .map_err(ResponseCacheReservationError::Io)?;
        let _quota_lock = locked_file(&self.inner.directory.join("quota.lock"))
            .map_err(ResponseCacheReservationError::Io)?;
        recover_orphaned_sessions(&self.inner.directory, Some(&session_directory))
            .map_err(ResponseCacheReservationError::Io)?;

        let used = retained_body_bytes(&self.inner.directory)
            .map_err(ResponseCacheReservationError::Io)?;
        let available = self.inner.quota_bytes.saturating_sub(used);
        let reserved_bytes = expected_bytes
            .map(|expected| expected.max(minimum_bytes))
            .unwrap_or(available);
        if reserved_bytes > available {
            return Err(ResponseCacheReservationError::QuotaExceeded);
        }

        loop {
            let sequence = BODY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = session_directory.join(format!("response-{sequence}.body"));
            match create_private_file(&path) {
                Ok(file) => {
                    if let Err(error) = file.set_len(reserved_bytes) {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return Err(ResponseCacheReservationError::Io(error));
                    }
                    return Ok(ResponseCacheReservation {
                        file,
                        body_file: ResponseBodyFile {
                            inner: Arc::new(ResponseBodyFileInner {
                                path,
                                _cache: self.clone(),
                                cleanup,
                            }),
                        },
                        max_bytes: reserved_bytes,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(ResponseCacheReservationError::Io(error)),
            }
        }
    }

    fn ensure_session(&self) -> io::Result<(PathBuf, Sender<PathBuf>)> {
        let mut session = self
            .inner
            .session
            .lock()
            .map_err(|_| io::Error::other("response cache state is unavailable"))?;
        if let Some(session) = session.as_ref() {
            return Ok((session.directory.clone(), session.cleanup.clone()));
        }

        std::fs::create_dir_all(&self.inner.directory)?;
        let _quota_lock = locked_file(&self.inner.directory.join("quota.lock"))?;
        recover_orphaned_sessions(&self.inner.directory, None)?;

        let directory = loop {
            let sequence = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = self.inner.directory.join(format!(
                "session-{}-{timestamp}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let lease = locked_file(&directory.join("session.lock"))?;
        let (cleanup, pending) = mpsc::channel();
        let base = self.inner.directory.clone();
        std::thread::Builder::new()
            .name("probe-response-cache-cleanup".to_owned())
            .spawn(move || cleanup_released_bodies(&base, pending))?;
        *session = Some(ResponseCacheSession {
            directory: directory.clone(),
            _lease: lease,
            cleanup: cleanup.clone(),
        });
        Ok((directory, cleanup))
    }
}

impl PartialEq for ResponseCache {
    fn eq(&self, other: &Self) -> bool {
        self.inner.directory == other.inner.directory
            && self.inner.quota_bytes == other.inner.quota_bytes
    }
}

impl Eq for ResponseCache {}

/// An automatically managed complete response body stored on disk.
///
/// Clones share ownership. The file is queued for cleanup when the final owner is dropped.
#[derive(Clone, Debug)]
pub struct ResponseBodyFile {
    inner: Arc<ResponseBodyFileInner>,
}

#[derive(Debug)]
struct ResponseBodyFileInner {
    path: PathBuf,
    _cache: ResponseCache,
    cleanup: Sender<PathBuf>,
}

impl ResponseBodyFile {
    /// Returns the complete body's path for bounded or streaming reads.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }
}

impl PartialEq for ResponseBodyFile {
    fn eq(&self, other: &Self) -> bool {
        self.inner.path == other.inner.path
    }
}

impl Eq for ResponseBodyFile {}

impl Drop for ResponseBodyFileInner {
    fn drop(&mut self) {
        let _ = self.cleanup.send(std::mem::take(&mut self.path));
    }
}

fn cleanup_released_bodies(base: &Path, pending: Receiver<PathBuf>) {
    for path in pending {
        if let Ok(_quota_lock) = locked_file(&base.join("quota.lock")) {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub(crate) struct ResponseCacheReservation {
    pub(crate) file: std::fs::File,
    pub(crate) body_file: ResponseBodyFile,
    pub(crate) max_bytes: u64,
}

#[derive(Debug)]
pub(crate) enum ResponseCacheReservationError {
    QuotaExceeded,
    Io(io::Error),
}

fn create_private_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    if let Err(error) = set_private_permissions(&file) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(file)
}

fn open_lock_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    set_private_permissions(&file)?;
    Ok(file)
}

fn locked_file(path: &Path) -> io::Result<std::fs::File> {
    let file = open_lock_file(path)?;
    file.lock()?;
    Ok(file)
}

fn set_private_permissions(file: &std::fs::File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

fn recover_orphaned_sessions(base: &Path, current: Option<&Path>) -> io::Result<()> {
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir()
            || !entry.file_name().to_string_lossy().starts_with("session-")
            || current == Some(path.as_path())
        {
            continue;
        }
        let Ok(lease) = open_lock_file(&path.join("session.lock")) else {
            continue;
        };
        if lease.try_lock().is_ok() {
            drop(lease);
            let _ = std::fs::remove_dir_all(path);
        }
    }
    Ok(())
}

fn retained_body_bytes(base: &Path) -> io::Result<u64> {
    let mut total = 0_u64;
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(entry.path())? {
            let file = file?;
            if file.file_type()?.is_file() && file.file_name().to_string_lossy().ends_with(".body")
            {
                total = total.saturating_add(file.metadata()?.len());
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        time::{Duration, Instant},
    };

    use super::*;

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for cache state"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn child_holds_body() {
        let Ok(base) = std::env::var("PROBE_CACHE_CHILD_DIRECTORY") else {
            return;
        };
        let base = PathBuf::from(base);
        let cache = ResponseCache::new(base.clone(), 2048);
        let reservation = cache.reserve(Some(1024), 0).unwrap();
        let body_path = reservation.body_file.path().to_owned();
        std::fs::write(
            base.join("child-ready"),
            body_path.to_string_lossy().as_bytes(),
        )
        .unwrap();
        wait_until(|| base.join("child-release").exists());
        drop(reservation);
        wait_until(|| !body_path.exists());
    }

    #[test]
    fn another_process_does_not_recover_a_live_session() {
        let base = std::env::temp_dir().join(format!(
            "probe-cache-process-{}-{}",
            std::process::id(),
            SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cache::tests::child_holds_body"])
            .env("PROBE_CACHE_CHILD_DIRECTORY", &base)
            .spawn()
            .unwrap();
        wait_until(|| base.join("child-ready").exists());
        let body_path = PathBuf::from(std::fs::read_to_string(base.join("child-ready")).unwrap());
        let other_cache = ResponseCache::new(base.clone(), 2048);
        other_cache.initialize().unwrap();
        assert!(
            body_path.exists(),
            "live body in another process must remain"
        );
        assert!(matches!(
            other_cache.reserve(Some(1025), 0),
            Err(ResponseCacheReservationError::QuotaExceeded)
        ));
        std::fs::write(base.join("child-release"), []).unwrap();
        assert!(child.wait().unwrap().success());
        let reservation = other_cache.reserve(Some(1025), 0).unwrap();
        let released_path = reservation.body_file.path().to_owned();
        drop(reservation);
        wait_until(|| !released_path.exists());
        drop(other_cache);
        std::fs::remove_dir_all(base).unwrap();
    }
}
