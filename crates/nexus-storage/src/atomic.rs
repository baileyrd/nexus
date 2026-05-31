//! Atomic file writes via temp-fsync-rename.
//!
//! This module provides [`atomic_write`], which writes content to a temporary
//! file in `temp_dir`, calls `fsync(2)` on the file descriptor, renames the
//! temp file into place, then `fsync(2)`s the parent directory so the rename
//! is durable across a crash. On *transient* failure it retries up to three
//! times with exponential back-off (100 ms, 400 ms, 1 600 ms). Permanent
//! failures (`PermissionDenied`, `NotFound`, ENOSPC, etc.) bail immediately
//! since retries can't succeed.

use std::fs;
use std::io::{self, Write as _};
use std::path::Path;
use std::thread;
use std::time::Duration;

use uuid::Uuid;

use crate::StorageError;

/// Write `content` atomically to `target` using a temp-fsync-rename pattern.
///
/// 1. Creates `target`'s parent directories if they do not exist.
/// 2. Writes `content` to `temp_dir/<uuid>.tmp`.
/// 3. Calls `fsync(2)` on the temp file's file descriptor.
/// 4. Renames the temp file to `target`.
/// 5. On Unix, `fsync(2)`s the parent directory of `target` so the rename is
///    durable across a crash. On Windows this is a no-op (NTFS journals
///    rename metadata as part of the same transaction the file write
///    participates in).
/// 6. On *transient* failure (`Interrupted`, `WouldBlock`, `TimedOut`,
///    `ConnectionReset`, `UnexpectedEof`), deletes the temp file and
///    retries up to **3** times with exponential back-off (100 ms → 400 ms
///    → 1 600 ms). On *permanent* failure (`PermissionDenied`,
///    `NotFound`, `AlreadyExists`, ENOSPC, EXDEV, etc.) — bails immediately
///    rather than wasting wall-clock on guaranteed-to-fail retries.
///
/// # Errors
///
/// Returns [`StorageError::WriteFailed`] on permanent failure or after
/// exhausting retries on a transient one.
pub fn atomic_write(target: &Path, content: &[u8], temp_dir: &Path) -> Result<(), StorageError> {
    // Ensure parent dirs exist.
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| StorageError::WriteFailed {
                path: target.display().to_string(),
                reason: e.to_string(),
            })?;
        }
    }

    let mut delay = Duration::from_millis(100);
    let max_attempts = 4u32; // 1 initial + 3 retries

    for attempt in 0..max_attempts {
        let tmp_path = temp_dir.join(format!("{}.tmp", Uuid::new_v4()));

        let result = try_write(target, content, &tmp_path);
        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                // Best-effort cleanup of the temp file.
                let _ = fs::remove_file(&tmp_path);

                // Issue #84. Pre-fix, every error class went through the
                // retry loop and ate up to ~2 seconds of wall-clock waiting
                // for `EACCES` / `ENOSPC` / `EXDEV` to spontaneously become
                // success. Now permanent kinds bail immediately and only
                // genuinely-transient kinds get the back-off.
                if !is_transient_io_error(&e) || attempt + 1 >= max_attempts {
                    return Err(StorageError::WriteFailed {
                        path: target.display().to_string(),
                        reason: e.to_string(),
                    });
                }
                thread::sleep(delay);
                delay *= 4;
            }
        }
    }

    Err(StorageError::WriteFailed {
        path: target.display().to_string(),
        reason: "exhausted retries".to_string(),
    })
}

/// Single attempt: write → fsync → rename → parent-dir fsync.
///
/// Returns `io::Error` directly (rather than the wrapped `StorageError`)
/// so the retry loop can match on `e.kind()` and skip retries on
/// permanent failures. The caller is responsible for translating the
/// final failure into `StorageError::WriteFailed`.
fn try_write(target: &Path, content: &[u8], tmp_path: &Path) -> io::Result<()> {
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(tmp_path)?;

    (&file).write_all(content)?;

    // Cross-platform durable flush — `fsync(2)` on Unix, `FlushFileBuffers`
    // on Windows. Replaces the prior unsafe `libc::fsync` call which wasn't
    // portable to MSVC targets.
    file.sync_all()?;

    fs::rename(tmp_path, target)?;

    // Issue #84. The rename above is *atomic* on POSIX (the inode
    // either points at the new content or the old) but not *durable*
    // until the directory's metadata is also synced — a crash between
    // rename and the parent-dir fsync can revert the rename. For
    // file-as-truth this matters: the on-disk markdown is the source
    // of record, and a "successful save" claim has to survive an
    // ungraceful shutdown. Sync the parent dir.
    //
    // Windows has no parent-dir-fsync equivalent — NTFS journals
    // metadata changes as part of the same transaction the file
    // write participates in, so rename atomicity implies durability.
    fsync_parent(target)?;

    Ok(())
}

/// On Unix, open the parent directory of `target` and call `sync_all`
/// on it so the rename above is durable across a crash. On Windows
/// this is a no-op — see [`try_write`] for the rationale.
#[cfg(unix)]
fn fsync_parent(target: &Path) -> io::Result<()> {
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let dir = fs::File::open(parent)?;
    dir.sync_all()
}

#[cfg(not(unix))]
fn fsync_parent(_target: &Path) -> io::Result<()> {
    Ok(())
}

/// Return `true` if `e` is the kind of `io::Error` that has a
/// reasonable chance of succeeding on a retry. Permanent failure
/// classes (`PermissionDenied`, `NotFound`, `AlreadyExists`, ENOSPC
/// surfaced as `StorageFull`, EXDEV surfaced as a generic `Other`,
/// etc.) shouldn't burn the retry budget.
fn is_transient_io_error(e: &io::Error) -> bool {
    use io::ErrorKind;
    matches!(
        e.kind(),
        ErrorKind::Interrupted
            | ErrorKind::WouldBlock
            | ErrorKind::TimedOut
            | ErrorKind::ConnectionReset
            | ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn atomic_write_creates_file() {
        let dir = tmp();
        let target = dir.path().join("output.txt");
        atomic_write(&target, b"hello", dir.path()).expect("write");
        assert_eq!(fs::read(&target).unwrap(), b"hello");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tmp();
        let target = dir.path().join("output.txt");
        atomic_write(&target, b"first", dir.path()).expect("first write");
        atomic_write(&target, b"second", dir.path()).expect("second write");
        assert_eq!(fs::read(&target).unwrap(), b"second");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let dir = tmp();
        let temp_dir = dir.path().join("temp");
        fs::create_dir_all(&temp_dir).unwrap();
        let target = dir.path().join("output.txt");
        atomic_write(&target, b"data", &temp_dir).expect("write");

        let leftovers: Vec<_> = fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn atomic_write_handles_empty_content() {
        let dir = tmp();
        let target = dir.path().join("empty.txt");
        atomic_write(&target, b"", dir.path()).expect("write empty");
        assert_eq!(fs::read(&target).unwrap(), b"");
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let dir = tmp();
        let target = dir.path().join("deep").join("nested").join("file.txt");
        atomic_write(&target, b"nested", dir.path()).expect("write nested");
        assert_eq!(fs::read(&target).unwrap(), b"nested");
    }

    #[test]
    fn atomic_write_handles_large_content() {
        let dir = tmp();
        let target = dir.path().join("large.bin");
        let data = vec![0xABu8; 1024 * 1024]; // 1 MiB
        atomic_write(&target, &data, dir.path()).expect("write large");
        assert_eq!(fs::read(&target).unwrap().len(), 1024 * 1024);
    }

    /// Issue #84. Permanent error classes (here: write to a path
    /// whose parent doesn't exist) should bail immediately rather
    /// than waste ~2s of wall-clock on three retries that have no
    /// chance of succeeding. The temp_dir argument is a non-existent
    /// path so the temp-file open fails with `NotFound` — which is a
    /// permanent failure for retry purposes.
    #[test]
    fn atomic_write_does_not_retry_on_permanent_error() {
        let dir = tmp();
        let target = dir.path().join("output.txt");
        let nonexistent_temp = dir.path().join("does-not-exist");
        let start = std::time::Instant::now();
        let err = atomic_write(&target, b"data", &nonexistent_temp).expect_err("must fail");
        let elapsed = start.elapsed();
        // Pre-fix this would have slept 100ms + 400ms + 1600ms = 2.1s.
        // With the fix, the failure is returned on the first attempt
        // — well under 100ms.
        assert!(
            elapsed < Duration::from_millis(100),
            "atomic_write retried on a permanent error (took {:?}, expected <100ms)",
            elapsed
        );
        match err {
            StorageError::WriteFailed { .. } => {}
            other => panic!("expected WriteFailed, got: {other:?}"),
        }
    }

    /// Helper-level coverage of the transient-error classifier so a
    /// future refactor can't silently widen the retry policy without
    /// the test failing.
    #[test]
    fn is_transient_io_error_matrix() {
        use io::ErrorKind;

        // Transient — must retry.
        for kind in [
            ErrorKind::Interrupted,
            ErrorKind::WouldBlock,
            ErrorKind::TimedOut,
            ErrorKind::ConnectionReset,
            ErrorKind::UnexpectedEof,
        ] {
            assert!(
                is_transient_io_error(&io::Error::from(kind)),
                "{kind:?} should be transient"
            );
        }

        // Permanent — must bail.
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::NotFound,
            ErrorKind::AlreadyExists,
            ErrorKind::InvalidInput,
            ErrorKind::Other,
        ] {
            assert!(
                !is_transient_io_error(&io::Error::from(kind)),
                "{kind:?} should be permanent"
            );
        }
    }
}
