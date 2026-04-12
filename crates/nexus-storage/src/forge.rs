//! Forge directory layout manager and exclusive lock.
//!
//! A "forge" is the root directory of a Nexus knowledge base. This module
//! handles creating the expected subdirectory structure, managing a temp
//! directory, and acquiring an exclusive advisory lock via `flock(2)`.

use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::StorageError;

/// Manages the directory layout of a Nexus forge root.
///
/// The layout created by [`Forge::init`] is:
///
/// ```text
/// <root>/
///   notes/
///   attachments/
///   .forge/
///     temp/
///     search/
///     lock      (created on first lock acquisition)
/// ```
pub struct Forge {
    root: PathBuf,
}

impl Forge {
    /// Create a new `Forge` handle for the given root directory.
    ///
    /// This does **not** create any directories; call [`Forge::init`] for that.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Return the forge root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the `notes/` subdirectory path.
    #[must_use]
    pub fn notes_dir(&self) -> PathBuf {
        self.root.join("notes")
    }

    /// Return the `attachments/` subdirectory path.
    #[must_use]
    pub fn attachments_dir(&self) -> PathBuf {
        self.root.join("attachments")
    }

    /// Return the `.forge/` hidden subdirectory path.
    #[must_use]
    pub fn forge_dir(&self) -> PathBuf {
        self.root.join(".forge")
    }

    /// Return the `.forge/temp/` subdirectory path.
    #[must_use]
    pub fn temp_dir(&self) -> PathBuf {
        self.forge_dir().join("temp")
    }

    /// Return the path to the `SQLite` index database.
    #[must_use]
    pub fn index_db_path(&self) -> PathBuf {
        self.forge_dir().join("index.db")
    }

    /// Return the `.forge/search/` subdirectory path (Tantivy index).
    #[must_use]
    pub fn search_dir(&self) -> PathBuf {
        self.forge_dir().join("search")
    }

    /// Return the path to the exclusive advisory lock file.
    #[must_use]
    pub fn lock_path(&self) -> PathBuf {
        self.forge_dir().join("lock")
    }

    /// Initialize the forge directory structure.
    ///
    /// Creates the following directories (idempotent — safe to call multiple
    /// times):
    /// - `notes/`
    /// - `attachments/`
    /// - `.forge/`
    /// - `.forge/temp/`
    /// - `.forge/search/`
    ///
    /// Does **not** create `canvases/` or `databases/`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Io`] if any directory cannot be created.
    pub fn init(&self) -> Result<(), StorageError> {
        let dirs = [
            self.notes_dir(),
            self.attachments_dir(),
            self.forge_dir(),
            self.temp_dir(),
            self.search_dir(),
        ];
        for dir in &dirs {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Remove files in `.forge/temp/` whose modification time is older than
    /// one hour.
    ///
    /// Subdirectories are left in place. Errors on individual files are
    /// silently ignored so a partial cleanup doesn't abort the whole operation.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Io`] if the temp directory cannot be read.
    pub fn clean_temp(&self) -> Result<(), StorageError> {
        let cutoff = SystemTime::now()
            .checked_sub(Duration::from_secs(3600))
            .unwrap_or(SystemTime::UNIX_EPOCH);

        for entry in fs::read_dir(self.temp_dir())? {
            let Ok(entry) = entry else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            if let Ok(mtime) = meta.modified()
                && mtime < cutoff
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    /// Acquire an exclusive non-blocking advisory lock on `.forge/lock`.
    ///
    /// The lock is automatically released when the returned [`ForgeLock`] is
    /// dropped (because the underlying [`std::fs::File`] is closed).
    ///
    /// # Errors
    ///
    /// - [`StorageError::LockHeld`] — another process already holds the lock.
    /// - [`StorageError::Io`] — the lock file could not be opened or
    ///   `flock(2)` returned an unexpected error.
    pub fn acquire_lock(&self) -> Result<ForgeLock, StorageError> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.lock_path())?;

        let fd = file.as_raw_fd();
        // SAFETY: `fd` is valid for the lifetime of `file`, and `flock` is
        // safe to call with a valid file descriptor and well-known flags.
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            let errno = std::io::Error::last_os_error();
            if errno.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Err(StorageError::LockHeld(
                    "another process holds the forge lock".to_string(),
                ));
            }
            return Err(StorageError::Io(errno));
        }

        Ok(ForgeLock { _file: file })
    }
}

/// RAII guard that holds the exclusive forge lock.
///
/// The lock is released automatically when this value is dropped, because
/// dropping it closes the underlying [`std::fs::File`], which causes the
/// kernel to release the `flock` advisory lock.
#[derive(Debug)]
pub struct ForgeLock {
    _file: fs::File,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use filetime::{set_file_mtime, FileTime};
    use tempfile::TempDir;

    use super::*;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ── Layout helpers ────────────────────────────────────────────────────────

    #[test]
    fn notes_dir_returns_correct_path() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        assert_eq!(forge.notes_dir(), dir.path().join("notes"));
    }

    #[test]
    fn forge_dir_returns_dot_forge_path() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        assert_eq!(forge.forge_dir(), dir.path().join(".forge"));
    }

    #[test]
    fn temp_dir_returns_correct_path() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        assert_eq!(forge.temp_dir(), dir.path().join(".forge").join("temp"));
    }

    #[test]
    fn index_db_path_returns_correct_path() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        assert_eq!(
            forge.index_db_path(),
            dir.path().join(".forge").join("index.db")
        );
    }

    #[test]
    fn search_dir_returns_correct_path() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        assert_eq!(
            forge.search_dir(),
            dir.path().join(".forge").join("search")
        );
    }

    // ── init() ────────────────────────────────────────────────────────────────

    #[test]
    fn init_creates_directory_structure() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("init");

        assert!(forge.notes_dir().is_dir(), "notes/ missing");
        assert!(forge.attachments_dir().is_dir(), "attachments/ missing");
        assert!(forge.forge_dir().is_dir(), ".forge/ missing");
        assert!(forge.temp_dir().is_dir(), ".forge/temp/ missing");
        assert!(forge.search_dir().is_dir(), ".forge/search/ missing");
    }

    #[test]
    fn init_is_idempotent() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("first init");

        // Write a marker file into notes/
        let marker = forge.notes_dir().join("marker.md");
        fs::write(&marker, b"hello").expect("write marker");

        // Second init must not remove the marker
        forge.init().expect("second init");
        assert!(marker.exists(), "marker file was deleted by second init");
    }

    #[test]
    fn init_does_not_create_canvases_or_databases() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("init");

        assert!(
            !dir.path().join("canvases").exists(),
            "canvases/ should not be created"
        );
        assert!(
            !dir.path().join("databases").exists(),
            "databases/ should not be created"
        );
    }

    // ── clean_temp() ─────────────────────────────────────────────────────────

    #[test]
    fn clean_temp_removes_stale_files() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("init");

        let stale = forge.temp_dir().join("stale.tmp");
        fs::write(&stale, b"old").expect("write stale");

        // Set mtime 2 hours in the past
        let two_hours_ago = FileTime::from_system_time(
            SystemTime::now() - Duration::from_secs(7200),
        );
        set_file_mtime(&stale, two_hours_ago).expect("set mtime");

        forge.clean_temp().expect("clean_temp");
        assert!(!stale.exists(), "stale file should have been removed");
    }

    #[test]
    fn clean_temp_preserves_recent_files() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("init");

        let recent = forge.temp_dir().join("recent.tmp");
        fs::write(&recent, b"new").expect("write recent");
        // mtime is just now — no manipulation needed

        forge.clean_temp().expect("clean_temp");
        assert!(recent.exists(), "recent file should be preserved");
    }

    // ── acquire_lock() ───────────────────────────────────────────────────────

    #[test]
    fn acquire_lock_succeeds() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("init");

        let _lock = forge.acquire_lock().expect("acquire lock");
        assert!(forge.lock_path().exists());
    }

    #[test]
    fn acquire_lock_twice_fails() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("init");

        let _lock = forge.acquire_lock().expect("first lock");
        let result = forge.acquire_lock();
        assert!(
            matches!(result, Err(StorageError::LockHeld(_))),
            "expected LockHeld, got: {result:?}",
        );
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = tmp();
        let forge = Forge::new(dir.path());
        forge.init().expect("init");

        {
            let _lock = forge.acquire_lock().expect("first lock");
        } // dropped here → flock released

        // Should be acquirable again
        forge.acquire_lock().expect("lock after drop");
    }
}
