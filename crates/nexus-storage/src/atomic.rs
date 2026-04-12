//! Atomic file writes via temp-fsync-rename.
//!
//! This module provides [`atomic_write`], which writes content to a temporary
//! file in `temp_dir`, calls `fsync(2)` on the file descriptor, then renames
//! the temp file into place. On failure it retries up to three times with
//! exponential back-off (100 ms, 400 ms, 1 600 ms).

use std::fs;
use std::io::Write as _;
use std::os::unix::io::AsRawFd;
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
/// 5. On failure, deletes the temp file and retries up to **3** times with
///    exponential back-off (100 ms → 400 ms → 1 600 ms).
///
/// # Errors
///
/// Returns [`StorageError::WriteFailed`] if all attempts are exhausted.
pub fn atomic_write(
    target: &Path,
    content: &[u8],
    temp_dir: &Path,
) -> Result<(), StorageError> {
    // Ensure parent dirs exist.
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| StorageError::WriteFailed {
            path: target.display().to_string(),
            reason: e.to_string(),
        })?;
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

                if attempt + 1 < max_attempts {
                    thread::sleep(delay);
                    delay *= 4;
                } else {
                    return Err(e);
                }
            }
        }
    }

    Err(StorageError::WriteFailed {
        path: target.display().to_string(),
        reason: "exhausted retries".to_string(),
    })
}

/// Single attempt: write → fsync → rename.
fn try_write(
    target: &Path,
    content: &[u8],
    tmp_path: &Path,
) -> Result<(), StorageError> {
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(tmp_path)
        .map_err(|e| StorageError::WriteFailed {
            path: target.display().to_string(),
            reason: e.to_string(),
        })?;

    (&file).write_all(content).map_err(|e| StorageError::WriteFailed {
        path: target.display().to_string(),
        reason: e.to_string(),
    })?;

    // SAFETY: `fd` is valid for the lifetime of `file`. `fsync` is async-
    // signal-safe and the only side effect is flushing kernel buffers for
    // this file descriptor.
    let ret = unsafe { libc::fsync(file.as_raw_fd()) };
    if ret != 0 {
        return Err(StorageError::WriteFailed {
            path: target.display().to_string(),
            reason: std::io::Error::last_os_error().to_string(),
        });
    }

    fs::rename(tmp_path, target).map_err(|e| StorageError::WriteFailed {
        path: target.display().to_string(),
        reason: e.to_string(),
    })?;

    Ok(())
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
            .filter_map(|e| e.ok())
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
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
}
