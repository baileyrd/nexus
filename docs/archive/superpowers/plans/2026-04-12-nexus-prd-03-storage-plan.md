# Nexus PRD 03 — Storage Engine (M1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `nexus-storage` crate to interface-complete state — forge directory management, atomic writes, SQLite indexing (5 tables + FTS5), GFM markdown parsing, file watching with rename detection, reconciliation, and Tantivy full-text search, all compiling and tested.

**Architecture:** New `nexus-storage` workspace member with 9 internal modules behind a `StorageEngine` facade struct. Synchronous API (`std::sync::mpsc` for watcher events, `rayon` for parallelism). SQLite connection pool via `r2d2`, write-side mutex. No async runtime.

**Tech Stack:** Rust (edition 2024), `rusqlite` 0.31+ (bundled), `r2d2` 0.8, `r2d2_sqlite` 0.24, `tantivy` 0.22+, `comrak` 0.28+, `notify` 7.0+, `notify-debouncer-mini` 0.5+, `rayon` 1.10, `sha2` 0.10, `serde_yaml` 0.9+, `thiserror` 2.0, `serde` 1.0, `uuid` 1.0, `tempfile` 3 (dev-dep).

**Parent docs:**
- [`2026-04-12-nexus-prd-03-storage-design.md`](../specs/2026-04-12-nexus-prd-03-storage-design.md) — **the contract this plan implements**
- [`2026-04-11-nexus-m1-foundation-spec.md`](../specs/2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §7

---

## Prerequisites

1. PRD 02 (security crate) is complete and tests pass.
2. Verify: `cargo nextest run --workspace` passes with no failures (125 tests).
3. The `NexusEvent` enum in `nexus-kernel` already has `FileCreated`, `FileModified`, `FileDeleted`, `FileRenamed`, `IndexingStarted`, `IndexingProgress`, `IndexingCompleted` variants.

---

## File Structure

```
crates/nexus-storage/
├── Cargo.toml
└── src/
    ├── lib.rs              # crate-level docs, public re-exports, StorageEngine facade
    ├── error.rs            # StorageError enum
    ├── forge.rs            # forge directory init, layout, temp cleanup
    ├── atomic.rs           # atomic write (temp → fsync → rename)
    ├── schema.rs           # SQLite table DDL, migration runner
    ├── index.rs            # insert/query for files, blocks, links, tags, properties
    ├── parser.rs           # comrak markdown parsing, block/link/tag extraction
    ├── search.rs           # Tantivy schema, build, query
    ├── watcher.rs          # notify + debouncer, rename detection, git batch mode
    └── reconcile.rs        # full directory scan, hash delta, index sync
```

Modifications to existing files:
- `Cargo.toml` (workspace root): add `nexus-storage` to members, add 10 new workspace deps
- No modifications to `nexus-kernel` or `nexus-security` source files

---

## Task Overview

30 tasks across 10 phases:

1. Phase 1: Crate skeleton + workspace wiring (Tasks 1–2)
2. Phase 2: StorageError enum (Tasks 3–4)
3. Phase 3: Forge directory + forge lock + atomic writes (Tasks 5–8)
4. Phase 4: SQLite schema + migration runner (Tasks 9–11)
5. Phase 5: Markdown parser pipeline (Tasks 12–14)
6. Phase 6: Index operations (Tasks 15–17)
7. Phase 7: Tantivy search (Tasks 18–20)
8. Phase 8: File watcher with rename detection + git batch mode (Tasks 21–23)
9. Phase 9: Reconciliation (Tasks 24–25)
10. Phase 10: StorageEngine facade + smoke test (Tasks 26–30)

---

## Phase 1: Crate Skeleton

### Task 1: Add nexus-storage to workspace and create crate skeleton

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/nexus-storage/Cargo.toml`
- Create: `crates/nexus-storage/src/lib.rs`
- Create: `crates/nexus-storage/src/error.rs`

- [ ] **Step 1: Add workspace member and deps to root `Cargo.toml`**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`:

In the `[workspace]` members array, add `"crates/nexus-storage"`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/nexus-types",
    "crates/nexus-kernel",
    "crates/nexus-security",
    "crates/nexus-storage",
]
```

In `[workspace.dependencies]`, add:

```toml
# SQLite
rusqlite = { version = "0.31", features = ["bundled", "backup"] }
r2d2 = "0.8"
r2d2_sqlite = "0.24"

# Full-text search
tantivy = "0.22"

# Markdown parsing
comrak = "0.28"

# File watching
notify = "7"
notify-debouncer-mini = "0.5"

# Parallelism
rayon = "1.10"

# Hashing
sha2 = "0.10"

# YAML frontmatter
serde_yaml = "0.9"
```

- [ ] **Step 2: Create `crates/nexus-storage/Cargo.toml`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/Cargo.toml`:

```toml
[package]
name = "nexus-storage"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Nexus storage engine: forge layout, atomic writes, SQLite index, markdown parsing, file watching, Tantivy search"

[dependencies]
nexus-kernel = { path = "../nexus-kernel" }
nexus-types = { path = "../nexus-types" }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
rusqlite = { workspace = true }
r2d2 = { workspace = true }
r2d2_sqlite = { workspace = true }
tantivy = { workspace = true }
comrak = { workspace = true }
notify = { workspace = true }
notify-debouncer-mini = { workspace = true }
rayon = { workspace = true }
sha2 = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Create `crates/nexus-storage/src/lib.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`:

```rust
//! Nexus storage engine: forge layout, atomic writes, SQLite index,
//! markdown parsing, file watching, and Tantivy full-text search.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-03-storage-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;

pub use error::StorageError;
```

- [ ] **Step 4: Create placeholder `error.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/error.rs`:

```rust
//! Storage error types.

/// Errors from the storage subsystem.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Placeholder — replaced in Task 3.
    #[error("not yet implemented")]
    NotImplemented,
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p nexus-storage`
Expected: compiles successfully.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nexus-storage/
git commit -m "feat(storage): scaffold nexus-storage crate with workspace wiring"
```

---

### Task 2: Verify workspace builds clean

**Files:** (none — verification only)

- [ ] **Step 1: Full workspace check**

Run: `cargo check --workspace`
Expected: compiles. No errors from any crate.

- [ ] **Step 2: Run existing tests**

Run: `cargo nextest run --workspace`
Expected: all 125 PRD 01+02 tests pass. No regressions.

---

## Phase 2: StorageError Enum

### Task 3: Write StorageError tests

**Files:**
- Modify: `crates/nexus-storage/src/error.rs`

- [ ] **Step 1: Write tests for error Display messages**

Replace the contents of `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/error.rs` with:

```rust
//! Storage error types.

/// Errors from the storage subsystem.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Placeholder — replaced next task.
    #[error("not yet implemented")]
    NotImplemented,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_not_found_display() {
        let err = StorageError::FileNotFound("notes/missing.md".to_string());
        assert_eq!(err.to_string(), "file not found: notes/missing.md");
    }

    #[test]
    fn permission_denied_display() {
        let err = StorageError::PermissionDenied("/root/secret".to_string());
        assert_eq!(err.to_string(), "permission denied: /root/secret");
    }

    #[test]
    fn io_error_wraps() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = StorageError::Io(io_err);
        assert!(err.to_string().contains("gone"));
    }

    #[test]
    fn corrupt_file_display() {
        let err = StorageError::CorruptFile {
            path: "bad.md".to_string(),
            reason: "invalid UTF-8".to_string(),
        };
        assert_eq!(err.to_string(), "corrupt file bad.md: invalid UTF-8");
    }

    #[test]
    fn index_inconsistency_display() {
        let err = StorageError::IndexInconsistency {
            details: "orphan block".to_string(),
        };
        assert_eq!(err.to_string(), "index inconsistency: orphan block");
    }

    #[test]
    fn write_failed_display() {
        let err = StorageError::WriteFailed {
            path: "notes/x.md".to_string(),
            reason: "disk full".to_string(),
        };
        assert_eq!(err.to_string(), "write failed for notes/x.md: disk full");
    }

    #[test]
    fn parse_error_display() {
        let err = StorageError::ParseError {
            file: "notes/y.md".to_string(),
            error: "unterminated frontmatter".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "parse error in notes/y.md: unterminated frontmatter"
        );
    }

    #[test]
    fn lock_held_display() {
        let err = StorageError::LockHeld("pid 1234".to_string());
        assert_eq!(
            err.to_string(),
            "forge locked by another process: pid 1234"
        );
    }

    #[test]
    fn config_invalid_display() {
        let err = StorageError::ConfigInvalid("bad pool_size".to_string());
        assert_eq!(err.to_string(), "invalid configuration: bad pool_size");
    }

    #[test]
    fn io_error_converts_via_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let err: StorageError = io_err.into();
        assert!(matches!(err, StorageError::Io(_)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- error::tests`
Expected: FAIL — variants don't exist yet.

### Task 4: Implement StorageError enum

**Files:**
- Modify: `crates/nexus-storage/src/error.rs`

- [ ] **Step 1: Replace the enum with the full definition**

Replace the `StorageError` enum and everything before the `#[cfg(test)]` block in `crates/nexus-storage/src/error.rs`:

```rust
//! Storage error types.

/// Errors from the storage subsystem.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// File not found at the given path.
    #[error("file not found: {0}")]
    FileNotFound(String),

    /// Permission denied accessing path.
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// File failed to parse (corrupt or unsupported format).
    #[error("corrupt file {path}: {reason}")]
    CorruptFile {
        /// Path to the corrupt file.
        path: String,
        /// What went wrong.
        reason: String,
    },

    /// Index state doesn't match filesystem state.
    #[error("index inconsistency: {details}")]
    IndexInconsistency {
        /// Description of the inconsistency.
        details: String,
    },

    /// Atomic write failed after retries.
    #[error("write failed for {path}: {reason}")]
    WriteFailed {
        /// Target path that could not be written.
        path: String,
        /// Reason for failure.
        reason: String,
    },

    /// Markdown/MDX parse error.
    #[error("parse error in {file}: {error}")]
    ParseError {
        /// File that failed to parse.
        file: String,
        /// Parser error details.
        error: String,
    },

    /// Another process holds the forge lock.
    #[error("forge locked by another process: {0}")]
    LockHeld(String),

    /// Configuration is invalid.
    #[error("invalid configuration: {0}")]
    ConfigInvalid(String),

    /// SQLite error.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Tantivy search error.
    #[error("search error: {0}")]
    Search(#[from] tantivy::TantivyError),

    /// File watcher error.
    #[error("watcher error: {0}")]
    Watcher(#[from] notify::Error),
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- error::tests`
Expected: all 10 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/src/error.rs
git commit -m "feat(storage): define StorageError enum with all M1 variants"
```

---

## Phase 3: Forge Directory & Atomic Writes

### Task 5: Write forge directory tests

**Files:**
- Create: `crates/nexus-storage/src/forge.rs`

- [ ] **Step 1: Create `forge.rs` with tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/forge.rs`:

```rust
//! Forge directory initialization and layout.

use std::path::{Path, PathBuf};

use crate::StorageError;

/// Manages the forge directory structure.
pub struct Forge {
    root: PathBuf,
}

impl Forge {
    /// Create a new `Forge` handle for the given root. Does not create directories.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// The forge root path.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_directory_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();

        assert!(tmp.path().join("notes").is_dir());
        assert!(tmp.path().join("attachments").is_dir());
        assert!(tmp.path().join(".forge").is_dir());
        assert!(tmp.path().join(".forge/temp").is_dir());
    }

    #[test]
    fn init_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();
        // Write a marker file to verify init doesn't wipe existing content
        std::fs::write(tmp.path().join("notes/existing.md"), "hello").unwrap();
        forge.init().unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("notes/existing.md")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn init_does_not_create_canvases_or_databases() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();

        assert!(!tmp.path().join("canvases").exists());
        assert!(!tmp.path().join("databases").exists());
    }

    #[test]
    fn notes_dir_returns_correct_path() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        assert_eq!(forge.notes_dir(), tmp.path().join("notes"));
    }

    #[test]
    fn forge_dir_returns_dot_forge_path() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        assert_eq!(forge.forge_dir(), tmp.path().join(".forge"));
    }

    #[test]
    fn temp_dir_returns_correct_path() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        assert_eq!(forge.temp_dir(), tmp.path().join(".forge/temp"));
    }

    #[test]
    fn index_db_path_returns_correct_path() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        assert_eq!(forge.index_db_path(), tmp.path().join(".forge/index.db"));
    }

    #[test]
    fn search_dir_returns_correct_path() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        assert_eq!(forge.search_dir(), tmp.path().join(".forge/search"));
    }

    #[test]
    fn clean_temp_removes_stale_files() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();

        // Create a fake stale temp file
        let temp_file = tmp.path().join(".forge/temp/stale.tmp");
        std::fs::write(&temp_file, "stale data").unwrap();

        // Set mtime to 2 hours ago
        let two_hours_ago = std::time::SystemTime::now()
            - std::time::Duration::from_secs(7200);
        filetime::set_file_mtime(
            &temp_file,
            filetime::FileTime::from_system_time(two_hours_ago),
        )
        .unwrap();

        forge.clean_temp().unwrap();
        assert!(!temp_file.exists());
    }

    #[test]
    fn clean_temp_preserves_recent_files() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();

        let temp_file = tmp.path().join(".forge/temp/recent.tmp");
        std::fs::write(&temp_file, "recent data").unwrap();
        // mtime is now — should not be cleaned

        forge.clean_temp().unwrap();
        assert!(temp_file.exists());
    }

    #[test]
    fn acquire_lock_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();

        let lock = forge.acquire_lock();
        assert!(lock.is_ok());
    }

    #[test]
    fn acquire_lock_twice_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();

        let _lock1 = forge.acquire_lock().unwrap();
        let lock2 = forge.acquire_lock();
        assert!(matches!(lock2, Err(StorageError::LockHeld(_))));
    }

    #[test]
    fn lock_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let forge = Forge::new(tmp.path());
        forge.init().unwrap();

        {
            let _lock = forge.acquire_lock().unwrap();
        } // lock dropped here

        // Should be able to acquire again
        let lock = forge.acquire_lock();
        assert!(lock.is_ok());
    }
}
```

- [ ] **Step 2: Add `filetime` to workspace deps**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`, add to `[workspace.dependencies]`:

```toml
filetime = "0.2"
```

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/Cargo.toml`, add to `[dev-dependencies]`:

```toml
filetime = { workspace = true }
```

- [ ] **Step 3: Add `mod forge;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod error;`:

```rust
mod forge;

pub use forge::{Forge, ForgeLock};
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- forge::tests`
Expected: FAIL — methods don't exist yet.

### Task 6: Implement Forge

**Files:**
- Modify: `crates/nexus-storage/src/forge.rs`

- [ ] **Step 1: Implement all methods**

Replace everything above the `#[cfg(test)]` block in `crates/nexus-storage/src/forge.rs` with:

```rust
//! Forge directory initialization and layout.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::StorageError;

/// Stale temp file threshold: files older than this are cleaned up.
const TEMP_MAX_AGE: Duration = Duration::from_secs(3600); // 1 hour

/// RAII guard for the forge exclusive lock. Releases the lock on drop.
pub struct ForgeLock {
    _file: std::fs::File,
}

impl ForgeLock {
    /// Acquire an exclusive lock on the forge lock file.
    /// Returns `StorageError::LockHeld` if another process holds it.
    fn acquire(lock_path: &Path) -> Result<Self, StorageError> {
        use std::os::unix::io::AsRawFd;

        // Create/open the lock file
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path)?;

        // Try non-blocking flock
        let fd = file.as_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Err(StorageError::LockHeld(
                    lock_path.display().to_string(),
                ));
            }
            return Err(StorageError::Io(err));
        }

        Ok(Self { _file: file })
    }
}

// flock is released automatically when the File is dropped.

/// Manages the forge directory structure.
pub struct Forge {
    root: PathBuf,
}

impl Forge {
    /// Create a new `Forge` handle for the given root. Does not create directories.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// The forge root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path to the `notes/` directory.
    pub fn notes_dir(&self) -> PathBuf {
        self.root.join("notes")
    }

    /// Path to the `attachments/` directory.
    pub fn attachments_dir(&self) -> PathBuf {
        self.root.join("attachments")
    }

    /// Path to the `.forge/` directory.
    pub fn forge_dir(&self) -> PathBuf {
        self.root.join(".forge")
    }

    /// Path to the `.forge/temp/` directory.
    pub fn temp_dir(&self) -> PathBuf {
        self.root.join(".forge/temp")
    }

    /// Path to the `.forge/index.db` file.
    pub fn index_db_path(&self) -> PathBuf {
        self.root.join(".forge/index.db")
    }

    /// Path to the `.forge/search/` directory.
    pub fn search_dir(&self) -> PathBuf {
        self.root.join(".forge/search")
    }

    /// Path to the `.forge/lock` file.
    pub fn lock_path(&self) -> PathBuf {
        self.root.join(".forge/lock")
    }

    /// Acquire an exclusive lock on this forge.
    /// The lock is released when the returned `ForgeLock` is dropped.
    pub fn acquire_lock(&self) -> Result<ForgeLock, StorageError> {
        ForgeLock::acquire(&self.lock_path())
    }

    /// Initialize the forge directory structure. Idempotent — safe to call
    /// on an already-initialized forge.
    pub fn init(&self) -> Result<(), StorageError> {
        std::fs::create_dir_all(self.notes_dir())?;
        std::fs::create_dir_all(self.attachments_dir())?;
        std::fs::create_dir_all(self.forge_dir())?;
        std::fs::create_dir_all(self.temp_dir())?;
        std::fs::create_dir_all(self.search_dir())?;
        Ok(())
    }

    /// Remove stale temp files older than 1 hour.
    pub fn clean_temp(&self) -> Result<(), StorageError> {
        let temp_dir = self.temp_dir();
        if !temp_dir.exists() {
            return Ok(());
        }
        let now = SystemTime::now();
        for entry in std::fs::read_dir(&temp_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Ok(metadata) = path.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(age) = now.duration_since(modified) {
                            if age > TEMP_MAX_AGE {
                                std::fs::remove_file(&path)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- forge::tests`
Expected: all 10 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nexus-storage/
git commit -m "feat(storage): add Forge directory init, layout helpers, and temp cleanup"
```

---

### Task 7: Write atomic write tests

**Files:**
- Create: `crates/nexus-storage/src/atomic.rs`

- [ ] **Step 1: Create `atomic.rs` with tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/atomic.rs`:

```rust
//! Atomic file writes via temp-fsync-rename.

use std::path::Path;

use crate::StorageError;

/// Write `content` to `target` atomically using a temp file in `temp_dir`.
///
/// Algorithm: write to temp → fsync → rename to target.
/// On failure, retries up to 3 times with exponential backoff.
pub fn atomic_write(target: &Path, content: &[u8], temp_dir: &Path) -> Result<(), StorageError> {
    let _ = (target, content, temp_dir);
    Err(StorageError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_dir = tmp.path().join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let target = tmp.path().join("output.md");

        atomic_write(&target, b"hello world", &temp_dir).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_dir = tmp.path().join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let target = tmp.path().join("output.md");

        std::fs::write(&target, "old content").unwrap();
        atomic_write(&target, b"new content", &temp_dir).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_dir = tmp.path().join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let target = tmp.path().join("output.md");

        atomic_write(&target, b"content", &temp_dir).unwrap();

        let temp_files: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .collect();
        assert!(temp_files.is_empty(), "temp dir should be empty after write");
    }

    #[test]
    fn atomic_write_handles_empty_content() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_dir = tmp.path().join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let target = tmp.path().join("empty.md");

        atomic_write(&target, b"", &temp_dir).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "");
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_dir = tmp.path().join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let target = tmp.path().join("subdir/nested/file.md");

        atomic_write(&target, b"nested", &temp_dir).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "nested");
    }

    #[test]
    fn atomic_write_handles_large_content() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_dir = tmp.path().join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let target = tmp.path().join("large.md");

        let content = vec![b'x'; 1_000_000]; // 1MB
        atomic_write(&target, &content, &temp_dir).unwrap();

        assert_eq!(std::fs::read(&target).unwrap().len(), 1_000_000);
    }
}
```

- [ ] **Step 2: Add `mod atomic;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod forge;`:

```rust
mod atomic;
```

- [ ] **Step 3: Add `NotImplemented` back to StorageError temporarily**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/error.rs`. In the `StorageError` enum, add at the end (before the closing `}`):

```rust
    /// Placeholder for unimplemented features.
    #[error("not yet implemented")]
    NotImplemented,
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- atomic::tests`
Expected: FAIL — `atomic_write` returns `NotImplemented`.

### Task 8: Implement atomic_write

**Files:**
- Modify: `crates/nexus-storage/src/atomic.rs`

- [ ] **Step 1: Replace the function body**

Replace everything above the `#[cfg(test)]` block in `crates/nexus-storage/src/atomic.rs` with:

```rust
//! Atomic file writes via temp-fsync-rename.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::StorageError;

/// Retry delays for atomic write (exponential backoff).
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(400),
    Duration::from_millis(1600),
];

/// Write `content` to `target` atomically using a temp file in `temp_dir`.
///
/// Algorithm: write to temp → fsync → rename to target.
/// On failure, retries up to 3 times with exponential backoff.
pub fn atomic_write(target: &Path, content: &[u8], temp_dir: &Path) -> Result<(), StorageError> {
    // Ensure parent directories exist
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = temp_dir.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut last_err = None;

    for (attempt, delay) in std::iter::once(&Duration::ZERO)
        .chain(RETRY_DELAYS.iter())
        .enumerate()
    {
        if attempt > 0 {
            thread::sleep(*delay);
        }

        // Write to temp file
        match write_and_sync(&temp_path, content) {
            Ok(()) => {}
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                last_err = Some(e);
                continue;
            }
        }

        // Atomic rename
        match fs::rename(&temp_path, target) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                last_err = Some(StorageError::Io(e));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| StorageError::WriteFailed {
        path: target.display().to_string(),
        reason: "all retries exhausted".to_string(),
    }))
}

/// Write content to a file and fsync it.
fn write_and_sync(path: &Path, content: &[u8]) -> Result<(), StorageError> {
    let mut file = File::create(path)?;
    file.write_all(content)?;

    // fsync via libc
    let fd = file.as_raw_fd();
    let ret = unsafe { libc::fsync(fd) };
    if ret != 0 {
        return Err(StorageError::Io(std::io::Error::last_os_error()));
    }

    Ok(())
}
```

- [ ] **Step 2: Add `libc` to workspace deps**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`, add to `[workspace.dependencies]`:

```toml
libc = "0.2"
```

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/Cargo.toml`, add to `[dependencies]`:

```toml
libc = { workspace = true }
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- atomic::tests`
Expected: all 6 tests PASS.

- [ ] **Step 4: Remove `NotImplemented` variant from StorageError**

Edit `crates/nexus-storage/src/error.rs` — remove the `NotImplemented` variant and its doc comment.

- [ ] **Step 5: Verify everything still compiles**

Run: `cargo check -p nexus-storage`
Expected: compiles successfully.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nexus-storage/
git commit -m "feat(storage): add atomic file writes with temp-fsync-rename and retry"
```

---

## Phase 4: SQLite Schema & Migration Runner

### Task 9: Write schema tests

**Files:**
- Create: `crates/nexus-storage/src/schema.rs`

- [ ] **Step 1: Create `schema.rs` with tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/schema.rs`:

```rust
//! SQLite schema definitions and migration runner.

use rusqlite::Connection;

use crate::StorageError;

/// Apply all pending migrations to the database.
pub fn migrate(conn: &Connection) -> Result<u32, StorageError> {
    let _ = conn;
    todo!()
}

/// Configure SQLite pragmas (WAL, synchronous, cache, foreign keys).
pub fn configure_pragmas(conn: &Connection) -> Result<(), StorageError> {
    let _ = conn;
    todo!()
}

/// The current schema version.
pub const CURRENT_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn configure_pragmas_sets_wal_mode() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        // In-memory databases may report "memory" instead of "wal"
        assert!(mode == "wal" || mode == "memory");
    }

    #[test]
    fn configure_pragmas_enables_foreign_keys() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        let fk: i32 = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn migrate_creates_schema_version_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        let version: u32 = conn
            .query_row(
                "SELECT version FROM _schema_version ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    #[test]
    fn migrate_creates_files_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('test.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrate_creates_blocks_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        // Insert a file first (FK constraint)
        conn.execute(
            "INSERT INTO files (id, path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (1, 'test.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO blocks (file_id, block_type, content, start_line, end_line)
             VALUES (1, 'heading', 'Title', 1, 1)",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrate_creates_links_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (id, path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (1, 'a.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO links (source_file_id, link_text, link_type)
             VALUES (1, '[[other]]', 'wikilink')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM links", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrate_creates_tags_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (id, path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (1, 'a.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO tags (name, file_id, source) VALUES ('rust', 1, 'inline')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrate_creates_properties_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (id, path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (1, 'a.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO properties (file_id, key, value) VALUES (1, 'title', '\"My Note\"')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM properties", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrate_enforces_unique_file_path() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('dup.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('dup.md', 'note', 'def', 200, 0, 0)",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn migrate_enforces_unique_property_per_file() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (id, path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (1, 'a.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO properties (file_id, key, value) VALUES (1, 'title', '\"A\"')",
            [],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO properties (file_id, key, value) VALUES (1, 'title', '\"B\"')",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        let v1 = migrate(&conn).unwrap();
        let v2 = migrate(&conn).unwrap();
        assert_eq!(v1, v2);
        assert_eq!(v1, CURRENT_VERSION);
    }

    #[test]
    fn cascade_delete_removes_blocks() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (id, path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (1, 'a.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO blocks (file_id, block_type, content, start_line, end_line)
             VALUES (1, 'paragraph', 'text', 1, 2)",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM files WHERE id = 1", []).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
```

- [ ] **Step 2: Add `mod schema;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod forge;`:

```rust
mod schema;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- schema::tests`
Expected: FAIL — functions are `todo!()`.

### Task 10: Implement schema and migration runner

**Files:**
- Modify: `crates/nexus-storage/src/schema.rs`

- [ ] **Step 1: Replace function bodies**

Replace everything above the `#[cfg(test)]` block in `crates/nexus-storage/src/schema.rs` with:

```rust
//! SQLite schema definitions and migration runner.

use rusqlite::Connection;

use crate::StorageError;

/// The current schema version.
pub const CURRENT_VERSION: u32 = 1;

/// Migration 001: initial schema.
const MIGRATION_001: &str = "
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    file_type TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    is_deleted BOOLEAN DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_files_path_type ON files(path, file_type);
CREATE INDEX IF NOT EXISTS idx_files_hash ON files(content_hash);

CREATE TABLE IF NOT EXISTS blocks (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL,
    block_type TEXT NOT NULL,
    level INTEGER,
    content TEXT NOT NULL,
    raw_markdown TEXT,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    parent_block_id INTEGER,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
    FOREIGN KEY(parent_block_id) REFERENCES blocks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_blocks_file_id ON blocks(file_id);
CREATE INDEX IF NOT EXISTS idx_blocks_type ON blocks(block_type);

CREATE TABLE IF NOT EXISTS links (
    id INTEGER PRIMARY KEY,
    source_file_id INTEGER NOT NULL,
    source_block_id INTEGER,
    target_path TEXT,
    target_file_id INTEGER,
    link_text TEXT NOT NULL,
    link_type TEXT NOT NULL,
    is_resolved BOOLEAN DEFAULT 0,
    FOREIGN KEY(source_file_id) REFERENCES files(id) ON DELETE CASCADE,
    FOREIGN KEY(target_file_id) REFERENCES files(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_links_source ON links(source_file_id);
CREATE INDEX IF NOT EXISTS idx_links_target ON links(target_file_id);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    file_id INTEGER NOT NULL,
    block_id INTEGER,
    source TEXT NOT NULL,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
    FOREIGN KEY(block_id) REFERENCES blocks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_tags_name ON tags(name);
CREATE INDEX IF NOT EXISTS idx_tags_file ON tags(file_id);

CREATE TABLE IF NOT EXISTS properties (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    property_type TEXT,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
    UNIQUE(file_id, key)
);
";

/// Configure SQLite pragmas (WAL, synchronous, cache, foreign keys).
pub fn configure_pragmas(conn: &Connection) -> Result<(), StorageError> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(StorageError::Database)?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(StorageError::Database)?;
    conn.pragma_update(None, "cache_size", -16000_i32)
        .map_err(StorageError::Database)?;
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(StorageError::Database)?;
    Ok(())
}

/// Apply all pending migrations to the database. Returns the current version.
pub fn migrate(conn: &Connection) -> Result<u32, StorageError> {
    // Create version tracking table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _schema_version (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );"
    )
    .map_err(StorageError::Database)?;

    // Check current version
    let current: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
            [],
            |row| row.get(0),
        )
        .map_err(StorageError::Database)?;

    if current >= CURRENT_VERSION {
        return Ok(current);
    }

    // Apply migrations inside a transaction
    if current < 1 {
        let tx = conn.unchecked_transaction().map_err(StorageError::Database)?;

        tx.execute_batch(MIGRATION_001)
            .map_err(StorageError::Database)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![1u32, now as i64],
        )
        .map_err(StorageError::Database)?;

        tx.commit().map_err(StorageError::Database)?;
    }

    Ok(CURRENT_VERSION)
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- schema::tests`
Expected: all 12 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/
git commit -m "feat(storage): add SQLite schema (5 tables + FTS5) and migration runner"
```

---

### Task 11: Add FTS5 virtual table to schema

**Files:**
- Modify: `crates/nexus-storage/src/schema.rs`

- [ ] **Step 1: Add FTS5 test**

Add to the `#[cfg(test)] mod tests` block in `crates/nexus-storage/src/schema.rs`:

```rust
    #[test]
    fn migrate_creates_fts5_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        // Insert a file and block to populate FTS
        conn.execute(
            "INSERT INTO files (id, path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (1, 'a.md', 'note', 'abc', 100, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO blocks (id, file_id, block_type, content, start_line, end_line)
             VALUES (1, 1, 'paragraph', 'hello world of rust programming', 1, 1)",
            [],
        )
        .unwrap();

        // Manually sync FTS content
        conn.execute(
            "INSERT INTO fts_blocks (rowid, file_path, block_content, block_type)
             VALUES (1, 'a.md', 'hello world of rust programming', 'paragraph')",
            [],
        )
        .unwrap();

        // Query FTS
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_blocks WHERE fts_blocks MATCH 'rust'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
```

- [ ] **Step 2: Add FTS5 DDL to MIGRATION_001**

In `crates/nexus-storage/src/schema.rs`, append to the end of the `MIGRATION_001` string (before the closing `";`):

```sql

CREATE VIRTUAL TABLE IF NOT EXISTS fts_blocks USING fts5(
    file_path UNINDEXED,
    block_content,
    block_type UNINDEXED
);
```

Note: we use a standalone FTS5 table (not content-synced to `blocks`) because content-sync tables require careful trigger management. Manual sync is simpler and more explicit for M1.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- schema::tests`
Expected: all 13 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/schema.rs
git commit -m "feat(storage): add FTS5 virtual table to schema migration"
```

---

## Phase 5: Markdown Parser Pipeline

### Task 12: Write parser tests

**Files:**
- Create: `crates/nexus-storage/src/parser.rs`

- [ ] **Step 1: Create parser data types and tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/parser.rs`:

```rust
//! Markdown/MDX parsing: frontmatter, blocks, links, tags.

use crate::StorageError;

/// Result of parsing a single markdown/MDX file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// SHA-256 hex digest of the raw file content.
    pub content_hash: String,
    /// Key-value properties from YAML frontmatter.
    pub frontmatter: Vec<Property>,
    /// Flattened content blocks from the AST.
    pub blocks: Vec<ParsedBlock>,
    /// Links found in the content.
    pub links: Vec<ParsedLink>,
    /// Tags found in the content and frontmatter.
    pub tags: Vec<ParsedTag>,
}

/// A frontmatter property.
#[derive(Debug, Clone)]
pub struct Property {
    /// Property key.
    pub key: String,
    /// JSON-serialized value.
    pub value: String,
    /// Inferred type: "string", "number", "date", "list", "object".
    pub property_type: Option<String>,
}

/// A parsed content block.
#[derive(Debug, Clone)]
pub struct ParsedBlock {
    /// Block type: "heading", "paragraph", "codeblock", "list", "table".
    pub block_type: String,
    /// Heading level (1-6), None for non-headings.
    pub level: Option<i32>,
    /// Plaintext content.
    pub content: String,
    /// Original markdown source.
    pub raw_markdown: Option<String>,
    /// 1-based start line in the source file.
    pub start_line: u32,
    /// 1-based end line in the source file.
    pub end_line: u32,
}

/// A parsed link.
#[derive(Debug, Clone)]
pub struct ParsedLink {
    /// The link text as written (e.g., "[[other note]]" or "[text](url)").
    pub link_text: String,
    /// Resolved target path, if any.
    pub target_path: Option<String>,
    /// "wikilink", "markdown", or "embed".
    pub link_type: String,
}

/// A parsed tag.
#[derive(Debug, Clone)]
pub struct ParsedTag {
    /// Tag name (without the # prefix).
    pub name: String,
    /// "frontmatter" or "inline".
    pub source: String,
}

/// Parse a markdown/MDX file.
pub fn parse_markdown(content: &str) -> Result<ParsedFile, StorageError> {
    let _ = content;
    todo!()
}

/// Compute SHA-256 hex digest of content.
pub fn content_hash(content: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_produces_hex_string() {
        let hash = content_hash(b"hello");
        assert_eq!(hash.len(), 64); // SHA-256 = 64 hex chars
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn content_hash_is_deterministic() {
        let h1 = content_hash(b"hello");
        let h2 = content_hash(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        let h1 = content_hash(b"hello");
        let h2 = content_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn parse_empty_file() {
        let result = parse_markdown("").unwrap();
        assert!(result.frontmatter.is_empty());
        assert!(result.blocks.is_empty());
        assert!(result.links.is_empty());
        assert!(result.tags.is_empty());
    }

    #[test]
    fn parse_simple_heading() {
        let result = parse_markdown("# Hello World").unwrap();
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0].block_type, "heading");
        assert_eq!(result.blocks[0].level, Some(1));
        assert!(result.blocks[0].content.contains("Hello World"));
    }

    #[test]
    fn parse_multiple_headings() {
        let md = "# Title\n\n## Subtitle\n\n### Deep";
        let result = parse_markdown(md).unwrap();
        let headings: Vec<_> = result
            .blocks
            .iter()
            .filter(|b| b.block_type == "heading")
            .collect();
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].level, Some(1));
        assert_eq!(headings[1].level, Some(2));
        assert_eq!(headings[2].level, Some(3));
    }

    #[test]
    fn parse_paragraph() {
        let result = parse_markdown("Just a paragraph.").unwrap();
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0].block_type, "paragraph");
        assert!(result.blocks[0].content.contains("Just a paragraph"));
    }

    #[test]
    fn parse_code_block() {
        let md = "```rust\nfn main() {}\n```";
        let result = parse_markdown(md).unwrap();
        let code_blocks: Vec<_> = result
            .blocks
            .iter()
            .filter(|b| b.block_type == "codeblock")
            .collect();
        assert_eq!(code_blocks.len(), 1);
        assert!(code_blocks[0].content.contains("fn main()"));
    }

    #[test]
    fn parse_frontmatter() {
        let md = "---\ntitle: My Note\nauthor: Test\n---\n\n# Content";
        let result = parse_markdown(md).unwrap();
        assert!(!result.frontmatter.is_empty());
        let title = result.frontmatter.iter().find(|p| p.key == "title");
        assert!(title.is_some());
        assert!(title.unwrap().value.contains("My Note"));
    }

    #[test]
    fn parse_frontmatter_with_tags() {
        let md = "---\ntags:\n  - rust\n  - programming\n---\n\n# Content";
        let result = parse_markdown(md).unwrap();
        let tags: Vec<_> = result
            .tags
            .iter()
            .filter(|t| t.source == "frontmatter")
            .collect();
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|t| t.name == "rust"));
        assert!(tags.iter().any(|t| t.name == "programming"));
    }

    #[test]
    fn parse_inline_tags() {
        let md = "This is about #rust and #programming.";
        let result = parse_markdown(md).unwrap();
        let tags: Vec<_> = result
            .tags
            .iter()
            .filter(|t| t.source == "inline")
            .collect();
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|t| t.name == "rust"));
        assert!(tags.iter().any(|t| t.name == "programming"));
    }

    #[test]
    fn parse_wikilink() {
        let md = "See [[other note]] for details.";
        let result = parse_markdown(md).unwrap();
        let wikilinks: Vec<_> = result
            .links
            .iter()
            .filter(|l| l.link_type == "wikilink")
            .collect();
        assert_eq!(wikilinks.len(), 1);
        assert!(wikilinks[0].link_text.contains("other note"));
    }

    #[test]
    fn parse_wikilink_with_display_text() {
        let md = "See [[path/to/note|display text]] here.";
        let result = parse_markdown(md).unwrap();
        let wikilinks: Vec<_> = result
            .links
            .iter()
            .filter(|l| l.link_type == "wikilink")
            .collect();
        assert_eq!(wikilinks.len(), 1);
        assert_eq!(wikilinks[0].target_path, Some("path/to/note".to_string()));
        assert!(wikilinks[0].link_text.contains("display text"));
    }

    #[test]
    fn parse_markdown_link() {
        let md = "Click [here](https://example.com) for more.";
        let result = parse_markdown(md).unwrap();
        let md_links: Vec<_> = result
            .links
            .iter()
            .filter(|l| l.link_type == "markdown")
            .collect();
        assert_eq!(md_links.len(), 1);
        assert_eq!(
            md_links[0].target_path,
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn parse_embed() {
        let md = "![[embedded-note]]";
        let result = parse_markdown(md).unwrap();
        let embeds: Vec<_> = result
            .links
            .iter()
            .filter(|l| l.link_type == "embed")
            .collect();
        assert_eq!(embeds.len(), 1);
        assert!(embeds[0].link_text.contains("embedded-note"));
    }

    #[test]
    fn parse_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let result = parse_markdown(md).unwrap();
        let tables: Vec<_> = result
            .blocks
            .iter()
            .filter(|b| b.block_type == "table")
            .collect();
        assert_eq!(tables.len(), 1);
    }

    #[test]
    fn parse_no_frontmatter() {
        let md = "# Just a heading\n\nSome text.";
        let result = parse_markdown(md).unwrap();
        assert!(result.frontmatter.is_empty());
    }
}
```

- [ ] **Step 2: Add `mod parser;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod atomic;`:

```rust
mod parser;

pub use parser::{
    content_hash, parse_markdown, ParsedBlock, ParsedFile, ParsedLink, ParsedTag, Property,
};
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- parser::tests`
Expected: hash tests PASS, parse tests FAIL (`todo!()`).

### Task 13: Implement frontmatter extraction

**Files:**
- Modify: `crates/nexus-storage/src/parser.rs`

- [ ] **Step 1: Implement `parse_markdown` with frontmatter extraction**

Replace the `parse_markdown` function in `crates/nexus-storage/src/parser.rs` with:

```rust
/// Parse a markdown/MDX file.
pub fn parse_markdown(content: &str) -> Result<ParsedFile, StorageError> {
    let hash = content_hash(content.as_bytes());
    let (frontmatter, tags_from_fm, body) = extract_frontmatter(content);
    let (blocks, links, inline_tags) = parse_body(body);

    let mut all_tags = tags_from_fm;
    all_tags.extend(inline_tags);

    Ok(ParsedFile {
        content_hash: hash,
        frontmatter,
        blocks,
        links,
        tags: all_tags,
    })
}

/// Extract YAML frontmatter from the beginning of a markdown file.
/// Returns (properties, tags_from_frontmatter, remaining_body).
fn extract_frontmatter(content: &str) -> (Vec<Property>, Vec<ParsedTag>, &str) {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return (Vec::new(), Vec::new(), content);
    }

    // Find the closing ---
    let after_first = &content[4..]; // skip "---\n"
    let end = match after_first.find("\n---\n") {
        Some(pos) => pos,
        None => match after_first.find("\n---\r\n") {
            Some(pos) => pos,
            None => {
                if after_first.ends_with("\n---") {
                    after_first.len() - 3
                } else {
                    return (Vec::new(), Vec::new(), content);
                }
            }
        },
    };

    let yaml_str = &after_first[..end];
    let body_start = 4 + end + 4; // "---\n" + yaml + "\n---\n"
    let body = if body_start <= content.len() {
        &content[body_start..]
    } else {
        ""
    };

    // Parse YAML
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(yaml_str) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), Vec::new(), body),
    };

    let mut properties = Vec::new();
    let mut tags = Vec::new();

    if let serde_yaml::Value::Mapping(map) = yaml_value {
        for (key, value) in &map {
            let key_str = match key.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };

            // Extract tags from the "tags" key
            if key_str == "tags" {
                if let serde_yaml::Value::Sequence(seq) = value {
                    for item in seq {
                        if let Some(tag_name) = item.as_str() {
                            tags.push(ParsedTag {
                                name: tag_name.to_string(),
                                source: "frontmatter".to_string(),
                            });
                        }
                    }
                }
            }

            let (json_value, prop_type) = yaml_to_json_and_type(value);
            properties.push(Property {
                key: key_str,
                value: json_value,
                property_type: Some(prop_type),
            });
        }
    }

    (properties, tags, body)
}

/// Convert a YAML value to a JSON string and infer the property type.
fn yaml_to_json_and_type(value: &serde_yaml::Value) -> (String, String) {
    match value {
        serde_yaml::Value::String(s) => {
            (serde_json::to_string(s).unwrap_or_default(), "string".to_string())
        }
        serde_yaml::Value::Number(n) => {
            (n.to_string(), "number".to_string())
        }
        serde_yaml::Value::Bool(b) => {
            (b.to_string(), "string".to_string())
        }
        serde_yaml::Value::Sequence(_) => {
            let json = serde_json::to_string(
                &yaml_value_to_json(value),
            )
            .unwrap_or_default();
            (json, "list".to_string())
        }
        serde_yaml::Value::Mapping(_) => {
            let json = serde_json::to_string(
                &yaml_value_to_json(value),
            )
            .unwrap_or_default();
            (json, "object".to_string())
        }
        serde_yaml::Value::Null => ("null".to_string(), "string".to_string()),
        serde_yaml::Value::Tagged(t) => yaml_to_json_and_type(&t.value),
    }
}

/// Recursively convert serde_yaml::Value to serde_json::Value.
fn yaml_value_to_json(value: &serde_yaml::Value) -> serde_json::Value {
    match value {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::json!(f)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(yaml_value_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                if let Some(key) = k.as_str() {
                    obj.insert(key.to_string(), yaml_value_to_json(v));
                }
            }
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(t) => yaml_value_to_json(&t.value),
    }
}

/// Parse the markdown body (after frontmatter removal).
/// Returns (blocks, links, inline_tags).
/// Stub — replaced in Task 14 with full comrak implementation.
fn parse_body(_body: &str) -> (Vec<ParsedBlock>, Vec<ParsedLink>, Vec<ParsedTag>) {
    (Vec::new(), Vec::new(), Vec::new())
}
```

- [ ] **Step 2: Run frontmatter tests**

Run: `cargo nextest run -p nexus-storage -- parser::tests::parse_frontmatter`
Expected: frontmatter tests PASS, body-dependent tests still FAIL.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/src/
git commit -m "feat(storage): add frontmatter extraction with YAML parsing"
```

### Task 14: Implement body parsing (blocks, links, tags)

**Files:**
- Modify: `crates/nexus-storage/src/parser.rs`

- [ ] **Step 1: Replace `parse_body` with full implementation**

Replace the `parse_body` function in `crates/nexus-storage/src/parser.rs`:

```rust
/// Parse the markdown body (after frontmatter removal).
/// Returns (blocks, links, inline_tags).
fn parse_body(body: &str) -> (Vec<ParsedBlock>, Vec<ParsedLink>, Vec<ParsedTag>) {
    use comrak::nodes::NodeValue;
    use comrak::{parse_document, Arena, Options};

    if body.trim().is_empty() {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let arena = Arena::new();
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;

    let root = parse_document(&arena, body, &opts);

    let mut blocks = Vec::new();
    let mut links = Vec::new();
    let mut tags = Vec::new();

    for node in root.children() {
        let data = node.data.borrow();
        let start_line = data.sourcepos.start.line as u32;
        let end_line = data.sourcepos.end.line as u32;

        match &data.value {
            NodeValue::Heading(h) => {
                let text = collect_text(node);
                extract_inline_tags(&text, &mut tags);
                extract_wikilinks_and_embeds(&text, &mut links);
                blocks.push(ParsedBlock {
                    block_type: "heading".to_string(),
                    level: Some(i32::from(h.level)),
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                });
            }
            NodeValue::Paragraph => {
                let text = collect_text(node);
                extract_inline_tags(&text, &mut tags);
                extract_wikilinks_and_embeds(&text, &mut links);
                extract_markdown_links(node, &mut links);
                blocks.push(ParsedBlock {
                    block_type: "paragraph".to_string(),
                    level: None,
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                });
            }
            NodeValue::CodeBlock(cb) => {
                blocks.push(ParsedBlock {
                    block_type: "codeblock".to_string(),
                    level: None,
                    content: cb.literal.clone(),
                    raw_markdown: None,
                    start_line,
                    end_line,
                });
            }
            NodeValue::List(_) => {
                let text = collect_text(node);
                extract_inline_tags(&text, &mut tags);
                extract_wikilinks_and_embeds(&text, &mut links);
                blocks.push(ParsedBlock {
                    block_type: "list".to_string(),
                    level: None,
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                });
            }
            NodeValue::Table(_) => {
                let text = collect_text(node);
                blocks.push(ParsedBlock {
                    block_type: "table".to_string(),
                    level: None,
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                });
            }
            _ => {}
        }
    }

    (blocks, links, tags)
}

/// Recursively collect all text content from a node and its descendants.
fn collect_text<'a>(node: &'a comrak::nodes::AstNode<'a>) -> String {
    use comrak::nodes::NodeValue;

    let mut text = String::new();
    for child in node.descendants() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::Text(s) | NodeValue::Code(comrak::nodes::NodeCode { literal: s, .. }) => {
                text.push_str(s);
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => {
                text.push(' ');
            }
            _ => {}
        }
    }
    text
}

/// Extract wikilinks ([[target]]) and embeds (![[target]]) from text.
fn extract_wikilinks_and_embeds(text: &str, links: &mut Vec<ParsedLink>) {
    // Embeds: ![[...]]
    let mut search = text;
    while let Some(start) = search.find("![[") {
        let after = &search[start + 3..];
        if let Some(end) = after.find("]]") {
            let inner = &after[..end];
            let (target, display) = if let Some(pipe) = inner.find('|') {
                (Some(inner[..pipe].to_string()), &inner[pipe + 1..])
            } else {
                (Some(inner.to_string()), inner)
            };
            links.push(ParsedLink {
                link_text: display.to_string(),
                target_path: target,
                link_type: "embed".to_string(),
            });
            search = &after[end + 2..];
        } else {
            break;
        }
    }

    // Wikilinks: [[...]] (but not ![[...]])
    let mut search = text;
    while let Some(start) = search.find("[[") {
        // Skip if preceded by !
        if start > 0 && search.as_bytes()[start - 1] == b'!' {
            search = &search[start + 2..];
            continue;
        }
        let after = &search[start + 2..];
        if let Some(end) = after.find("]]") {
            let inner = &after[..end];
            let (target, display) = if let Some(pipe) = inner.find('|') {
                (Some(inner[..pipe].to_string()), inner[pipe + 1..].to_string())
            } else {
                (None, inner.to_string())
            };
            links.push(ParsedLink {
                link_text: display,
                target_path: target,
                link_type: "wikilink".to_string(),
            });
            search = &after[end + 2..];
        } else {
            break;
        }
    }
}

/// Extract standard markdown links from comrak AST nodes.
fn extract_markdown_links<'a>(node: &'a comrak::nodes::AstNode<'a>, links: &mut Vec<ParsedLink>) {
    use comrak::nodes::NodeValue;

    for child in node.descendants() {
        let data = child.data.borrow();
        if let NodeValue::Link(link) = &data.value {
            let text = collect_text(child);
            links.push(ParsedLink {
                link_text: text,
                target_path: Some(link.url.clone()),
                link_type: "markdown".to_string(),
            });
        }
    }
}

/// Extract inline #tags from text.
fn extract_inline_tags(text: &str, tags: &mut Vec<ParsedTag>) {
    // Match #tag at start of string or after whitespace
    let re_pattern = r"(?:^|\s)#([a-zA-Z0-9_/-]+)";
    // Simple manual extraction instead of regex dep
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '#' && (i == 0 || text.as_bytes()[i - 1].is_ascii_whitespace()) {
            let start = i + 1;
            let mut end = start;
            for &b in &text.as_bytes()[start..] {
                if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'/' {
                    end += 1;
                } else {
                    break;
                }
            }
            if end > start {
                let tag_name = &text[start..end];
                // Avoid duplicates
                if !tags.iter().any(|t| t.name == tag_name && t.source == "inline") {
                    tags.push(ParsedTag {
                        name: tag_name.to_string(),
                        source: "inline".to_string(),
                    });
                }
            }
        }
    }
}
```

- [ ] **Step 2: Run all parser tests**

Run: `cargo nextest run -p nexus-storage -- parser::tests`
Expected: all 18 tests PASS.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p nexus-storage -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/parser.rs
git commit -m "feat(storage): add markdown parser with comrak (blocks, links, tags, frontmatter)"
```

---

## Phase 6: Index Operations

### Task 15: Write index operation tests

**Files:**
- Create: `crates/nexus-storage/src/index.rs`

- [ ] **Step 1: Create `index.rs` with data types and tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/index.rs`:

```rust
//! SQLite index operations: insert and query files, blocks, links, tags, properties.

use rusqlite::Connection;

use crate::parser::{ParsedFile, Property};
use crate::StorageError;

/// A file record from the index.
#[derive(Debug, Clone)]
pub struct FileRecord {
    /// Database row ID.
    pub id: u64,
    /// Path relative to forge root.
    pub path: String,
    /// File type: "note" or "attachment".
    pub file_type: String,
    /// SHA-256 content hash.
    pub content_hash: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Unix timestamp of creation.
    pub created_at: i64,
    /// Unix timestamp of last modification.
    pub modified_at: i64,
    /// Soft-deleted for sync.
    pub is_deleted: bool,
}

/// Metadata returned after a file write.
#[derive(Debug, Clone)]
pub struct FileMetadata {
    /// Path relative to forge root.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Unix timestamp of last modification.
    pub modified_at: i64,
    /// SHA-256 content hash.
    pub content_hash: String,
}

/// A block record from the index.
#[derive(Debug, Clone)]
pub struct BlockRecord {
    /// Database row ID.
    pub id: u64,
    /// Parent file ID.
    pub file_id: u64,
    /// Block type.
    pub block_type: String,
    /// Heading level (1-6), None for non-headings.
    pub level: Option<i32>,
    /// Plaintext content.
    pub content: String,
    /// Start line in source file.
    pub start_line: u32,
    /// End line in source file.
    pub end_line: u32,
}

/// A link record from the index.
#[derive(Debug, Clone)]
pub struct LinkRecord {
    /// Database row ID.
    pub id: u64,
    /// Source file ID.
    pub source_file_id: u64,
    /// Target path (if known).
    pub target_path: Option<String>,
    /// Target file ID (if resolved).
    pub target_file_id: Option<u64>,
    /// Link text as written.
    pub link_text: String,
    /// Link type: "wikilink", "markdown", "embed".
    pub link_type: String,
    /// Whether the link is resolved.
    pub is_resolved: bool,
}

/// A tag result from a query.
#[derive(Debug, Clone)]
pub struct TagResult {
    /// Tag name.
    pub name: String,
    /// File ID.
    pub file_id: u64,
    /// File path.
    pub file_path: String,
    /// Tag source: "frontmatter" or "inline".
    pub source: String,
}

/// Filter for file queries.
#[derive(Debug, Clone, Default)]
pub struct FileFilter {
    /// Path prefix (e.g., "notes/").
    pub prefix: Option<String>,
    /// File type: "note" or "attachment".
    pub file_type: Option<String>,
    /// Include soft-deleted files.
    pub include_deleted: bool,
}

/// Statistics from an index rebuild.
#[derive(Debug, Clone)]
pub struct RebuildStats {
    /// Number of files processed.
    pub files_processed: usize,
    /// Number of blocks indexed.
    pub blocks_indexed: usize,
    /// Number of links found.
    pub links_found: usize,
    /// Number of tags found.
    pub tags_found: usize,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Insert a parsed file into the index. Returns the file's row ID.
pub fn insert_file(
    conn: &Connection,
    path: &str,
    file_type: &str,
    size_bytes: u64,
    parsed: &ParsedFile,
) -> Result<u64, StorageError> {
    let _ = (conn, path, file_type, size_bytes, parsed);
    todo!()
}

/// Query files from the index.
pub fn query_files(conn: &Connection, filter: &FileFilter) -> Result<Vec<FileRecord>, StorageError> {
    let _ = (conn, filter);
    todo!()
}

/// Query blocks for a file.
pub fn query_blocks(conn: &Connection, file_id: u64) -> Result<Vec<BlockRecord>, StorageError> {
    let _ = (conn, file_id);
    todo!()
}

/// Query links originating from a file.
pub fn query_links(conn: &Connection, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
    let _ = (conn, file_id);
    todo!()
}

/// Query backlinks pointing to a file.
pub fn query_backlinks(conn: &Connection, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
    let _ = (conn, file_id);
    todo!()
}

/// Query tags by name.
pub fn query_tags(conn: &Connection, name: &str) -> Result<Vec<TagResult>, StorageError> {
    let _ = (conn, name);
    todo!()
}

/// Delete all index data for a file (cascades to blocks, links, tags, properties).
pub fn delete_file(conn: &Connection, file_id: u64) -> Result<(), StorageError> {
    let _ = (conn, file_id);
    todo!()
}

/// Soft-delete a file (set is_deleted = true).
pub fn soft_delete_file(conn: &Connection, file_id: u64) -> Result<(), StorageError> {
    let _ = (conn, file_id);
    todo!()
}

/// Look up a file by path.
pub fn file_by_path(conn: &Connection, path: &str) -> Result<Option<FileRecord>, StorageError> {
    let _ = (conn, path);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{ParsedBlock, ParsedLink, ParsedTag};
    use crate::schema;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::configure_pragmas(&conn).unwrap();
        schema::migrate(&conn).unwrap();
        conn
    }

    fn sample_parsed_file() -> ParsedFile {
        ParsedFile {
            content_hash: "abc123".to_string(),
            frontmatter: vec![Property {
                key: "title".to_string(),
                value: "\"Test Note\"".to_string(),
                property_type: Some("string".to_string()),
            }],
            blocks: vec![
                ParsedBlock {
                    block_type: "heading".to_string(),
                    level: Some(1),
                    content: "Test Note".to_string(),
                    raw_markdown: None,
                    start_line: 1,
                    end_line: 1,
                },
                ParsedBlock {
                    block_type: "paragraph".to_string(),
                    level: None,
                    content: "Some content here.".to_string(),
                    raw_markdown: None,
                    start_line: 3,
                    end_line: 3,
                },
            ],
            links: vec![ParsedLink {
                link_text: "other note".to_string(),
                target_path: None,
                link_type: "wikilink".to_string(),
            }],
            tags: vec![ParsedTag {
                name: "rust".to_string(),
                source: "inline".to_string(),
            }],
        }
    }

    #[test]
    fn insert_file_returns_row_id() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn insert_file_stores_blocks() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();
        let blocks = query_blocks(&conn, id).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].block_type, "heading");
        assert_eq!(blocks[1].block_type, "paragraph");
    }

    #[test]
    fn insert_file_stores_links() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();
        let links = query_links(&conn, id).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, "wikilink");
    }

    #[test]
    fn insert_file_stores_tags() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();
        let tags = query_tags(&conn, "rust").unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].file_path, "notes/test.md");
    }

    #[test]
    fn insert_file_stores_properties() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();

        let val: String = conn
            .query_row(
                "SELECT value FROM properties WHERE file_id = ?1 AND key = 'title'",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(val.contains("Test Note"));
    }

    #[test]
    fn query_files_with_prefix() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/a.md", "note", 50, &parsed).unwrap();
        insert_file(&conn, "notes/b.md", "note", 60, &parsed).unwrap();
        insert_file(&conn, "attachments/img.png", "attachment", 1000, &parsed).unwrap();

        let filter = FileFilter {
            prefix: Some("notes/".to_string()),
            ..Default::default()
        };
        let files = query_files(&conn, &filter).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn query_files_with_type_filter() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/a.md", "note", 50, &parsed).unwrap();
        insert_file(&conn, "attachments/img.png", "attachment", 1000, &parsed).unwrap();

        let filter = FileFilter {
            file_type: Some("attachment".to_string()),
            ..Default::default()
        };
        let files = query_files(&conn, &filter).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_type, "attachment");
    }

    #[test]
    fn query_files_excludes_deleted_by_default() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/a.md", "note", 50, &parsed).unwrap();
        soft_delete_file(&conn, id).unwrap();

        let filter = FileFilter::default();
        let files = query_files(&conn, &filter).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn query_files_includes_deleted_when_requested() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/a.md", "note", 50, &parsed).unwrap();
        soft_delete_file(&conn, id).unwrap();

        let filter = FileFilter {
            include_deleted: true,
            ..Default::default()
        };
        let files = query_files(&conn, &filter).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].is_deleted);
    }

    #[test]
    fn query_backlinks_finds_linking_files() {
        let conn = setup_db();
        let mut parsed_a = sample_parsed_file();
        parsed_a.links = vec![]; // a has no links
        let id_a = insert_file(&conn, "notes/a.md", "note", 50, &parsed_a).unwrap();

        let mut parsed_b = sample_parsed_file();
        parsed_b.links = vec![ParsedLink {
            link_text: "a".to_string(),
            target_path: Some("notes/a.md".to_string()),
            link_type: "wikilink".to_string(),
        }];
        insert_file(&conn, "notes/b.md", "note", 60, &parsed_b).unwrap();

        let backlinks = query_backlinks(&conn, id_a).unwrap();
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].link_text, "a");
    }

    #[test]
    fn delete_file_cascades() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();
        delete_file(&conn, id).unwrap();

        let blocks = query_blocks(&conn, id).unwrap();
        assert!(blocks.is_empty());
    }

    #[test]
    fn file_by_path_returns_none_for_missing() {
        let conn = setup_db();
        let result = file_by_path(&conn, "notes/nonexistent.md").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn file_by_path_returns_some_for_existing() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();
        let result = file_by_path(&conn, "notes/test.md").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, "notes/test.md");
    }

    #[test]
    fn insert_file_populates_fts() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/test.md", "note", 100, &parsed).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_blocks WHERE fts_blocks MATCH 'content'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0);
    }
}
```

- [ ] **Step 2: Add `mod index;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod parser;`:

```rust
mod index;

pub use index::{
    BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, RebuildStats, TagResult,
};
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- index::tests`
Expected: FAIL — functions are `todo!()`.

### Task 16: Implement insert_file and file queries

**Files:**
- Modify: `crates/nexus-storage/src/index.rs`

- [ ] **Step 1: Implement all index functions**

Replace all function bodies above the `#[cfg(test)]` block in `crates/nexus-storage/src/index.rs` (keep the struct definitions and imports, replace from `insert_file` onwards):

```rust
/// Insert a parsed file into the index. Returns the file's row ID.
pub fn insert_file(
    conn: &Connection,
    path: &str,
    file_type: &str,
    size_bytes: u64,
    parsed: &ParsedFile,
) -> Result<u64, StorageError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    conn.execute(
        "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![path, file_type, parsed.content_hash, size_bytes as i64, now, now],
    )
    .map_err(StorageError::Database)?;

    let file_id = conn.last_insert_rowid() as u64;

    // Insert blocks + FTS
    for block in &parsed.blocks {
        conn.execute(
            "INSERT INTO blocks (file_id, block_type, level, content, raw_markdown, start_line, end_line)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                file_id as i64,
                block.block_type,
                block.level,
                block.content,
                block.raw_markdown,
                block.start_line,
                block.end_line,
            ],
        )
        .map_err(StorageError::Database)?;

        let block_id = conn.last_insert_rowid();

        // Insert into FTS
        conn.execute(
            "INSERT INTO fts_blocks (rowid, file_path, block_content, block_type)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![block_id, path, block.content, block.block_type],
        )
        .map_err(StorageError::Database)?;
    }

    // Insert links with resolution
    for link in &parsed.links {
        let (target_file_id, is_resolved) = if let Some(ref target) = link.target_path {
            resolve_link(conn, target)?
        } else {
            (None, false)
        };

        conn.execute(
            "INSERT INTO links (source_file_id, link_text, link_type, target_path, target_file_id, is_resolved)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                file_id as i64,
                link.link_text,
                link.link_type,
                link.target_path,
                target_file_id.map(|id| id as i64),
                is_resolved,
            ],
        )
        .map_err(StorageError::Database)?;
    }

    // Insert tags
    for tag in &parsed.tags {
        conn.execute(
            "INSERT INTO tags (name, file_id, source) VALUES (?1, ?2, ?3)",
            rusqlite::params![tag.name, file_id as i64, tag.source],
        )
        .map_err(StorageError::Database)?;
    }

    // Insert properties
    for prop in &parsed.frontmatter {
        conn.execute(
            "INSERT INTO properties (file_id, key, value, property_type) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![file_id as i64, prop.key, prop.value, prop.property_type],
        )
        .map_err(StorageError::Database)?;
    }

    Ok(file_id)
}

/// Attempt to resolve a link target to a file ID.
/// Returns (Some(file_id), true) if resolved, (None, false) otherwise.
fn resolve_link(conn: &Connection, target: &str) -> Result<(Option<u64>, bool), StorageError> {
    // Try exact path match
    let exact: Option<u64> = conn
        .query_row(
            "SELECT id FROM files WHERE path = ?1 AND is_deleted = 0",
            rusqlite::params![target],
            |row| row.get::<_, i64>(0).map(|id| id as u64),
        )
        .ok();

    if let Some(id) = exact {
        return Ok((Some(id), true));
    }

    // Try basename match (e.g., "note" matches "notes/note.md")
    let basename_pattern = format!("%/{target}.md");
    let basename: Option<u64> = conn
        .query_row(
            "SELECT id FROM files WHERE (path LIKE ?1 OR path = ?2) AND is_deleted = 0",
            rusqlite::params![basename_pattern, format!("{target}.md")],
            |row| row.get::<_, i64>(0).map(|id| id as u64),
        )
        .ok();

    if let Some(id) = basename {
        return Ok((Some(id), true));
    }

    Ok((None, false))
}

/// Query files from the index.
pub fn query_files(conn: &Connection, filter: &FileFilter) -> Result<Vec<FileRecord>, StorageError> {
    let mut sql = "SELECT id, path, file_type, content_hash, size_bytes, created_at, modified_at, is_deleted FROM files WHERE 1=1".to_string();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !filter.include_deleted {
        sql.push_str(" AND is_deleted = 0");
    }

    if let Some(ref prefix) = filter.prefix {
        sql.push_str(" AND path LIKE ?");
        params.push(Box::new(format!("{prefix}%")));
    }

    if let Some(ref ft) = filter.file_type {
        sql.push_str(" AND file_type = ?");
        params.push(Box::new(ft.clone()));
    }

    sql.push_str(" ORDER BY path");

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(StorageError::Database)?;
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(FileRecord {
                id: row.get::<_, i64>(0)? as u64,
                path: row.get(1)?,
                file_type: row.get(2)?,
                content_hash: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
                created_at: row.get(5)?,
                modified_at: row.get(6)?,
                is_deleted: row.get(7)?,
            })
        })
        .map_err(StorageError::Database)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(StorageError::Database)?);
    }
    Ok(results)
}

/// Query blocks for a file.
pub fn query_blocks(conn: &Connection, file_id: u64) -> Result<Vec<BlockRecord>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, file_id, block_type, level, content, start_line, end_line
             FROM blocks WHERE file_id = ?1 ORDER BY start_line",
        )
        .map_err(StorageError::Database)?;

    let rows = stmt
        .query_map(rusqlite::params![file_id as i64], |row| {
            Ok(BlockRecord {
                id: row.get::<_, i64>(0)? as u64,
                file_id: row.get::<_, i64>(1)? as u64,
                block_type: row.get(2)?,
                level: row.get(3)?,
                content: row.get(4)?,
                start_line: row.get::<_, i32>(5)? as u32,
                end_line: row.get::<_, i32>(6)? as u32,
            })
        })
        .map_err(StorageError::Database)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(StorageError::Database)?);
    }
    Ok(results)
}

/// Query links originating from a file.
pub fn query_links(conn: &Connection, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, source_file_id, target_path, target_file_id, link_text, link_type, is_resolved
             FROM links WHERE source_file_id = ?1",
        )
        .map_err(StorageError::Database)?;

    let rows = stmt
        .query_map(rusqlite::params![file_id as i64], |row| {
            Ok(LinkRecord {
                id: row.get::<_, i64>(0)? as u64,
                source_file_id: row.get::<_, i64>(1)? as u64,
                target_path: row.get(2)?,
                target_file_id: row.get::<_, Option<i64>>(3)?.map(|id| id as u64),
                link_text: row.get(4)?,
                link_type: row.get(5)?,
                is_resolved: row.get(6)?,
            })
        })
        .map_err(StorageError::Database)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(StorageError::Database)?);
    }
    Ok(results)
}

/// Query backlinks pointing to a file.
pub fn query_backlinks(conn: &Connection, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, source_file_id, target_path, target_file_id, link_text, link_type, is_resolved
             FROM links WHERE target_file_id = ?1",
        )
        .map_err(StorageError::Database)?;

    let rows = stmt
        .query_map(rusqlite::params![file_id as i64], |row| {
            Ok(LinkRecord {
                id: row.get::<_, i64>(0)? as u64,
                source_file_id: row.get::<_, i64>(1)? as u64,
                target_path: row.get(2)?,
                target_file_id: row.get::<_, Option<i64>>(3)?.map(|id| id as u64),
                link_text: row.get(4)?,
                link_type: row.get(5)?,
                is_resolved: row.get(6)?,
            })
        })
        .map_err(StorageError::Database)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(StorageError::Database)?);
    }
    Ok(results)
}

/// Query tags by name.
pub fn query_tags(conn: &Connection, name: &str) -> Result<Vec<TagResult>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT t.name, t.file_id, f.path, t.source
             FROM tags t JOIN files f ON t.file_id = f.id
             WHERE t.name = ?1",
        )
        .map_err(StorageError::Database)?;

    let rows = stmt
        .query_map(rusqlite::params![name], |row| {
            Ok(TagResult {
                name: row.get(0)?,
                file_id: row.get::<_, i64>(1)? as u64,
                file_path: row.get(2)?,
                source: row.get(3)?,
            })
        })
        .map_err(StorageError::Database)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(StorageError::Database)?);
    }
    Ok(results)
}

/// Delete all index data for a file (cascades to blocks, links, tags, properties).
pub fn delete_file(conn: &Connection, file_id: u64) -> Result<(), StorageError> {
    // Delete FTS entries for this file's blocks
    conn.execute(
        "DELETE FROM fts_blocks WHERE rowid IN (SELECT id FROM blocks WHERE file_id = ?1)",
        rusqlite::params![file_id as i64],
    )
    .map_err(StorageError::Database)?;

    conn.execute(
        "DELETE FROM files WHERE id = ?1",
        rusqlite::params![file_id as i64],
    )
    .map_err(StorageError::Database)?;

    Ok(())
}

/// Soft-delete a file (set is_deleted = true).
pub fn soft_delete_file(conn: &Connection, file_id: u64) -> Result<(), StorageError> {
    conn.execute(
        "UPDATE files SET is_deleted = 1 WHERE id = ?1",
        rusqlite::params![file_id as i64],
    )
    .map_err(StorageError::Database)?;

    Ok(())
}

/// Look up a file by path.
pub fn file_by_path(conn: &Connection, path: &str) -> Result<Option<FileRecord>, StorageError> {
    let result = conn.query_row(
        "SELECT id, path, file_type, content_hash, size_bytes, created_at, modified_at, is_deleted
         FROM files WHERE path = ?1",
        rusqlite::params![path],
        |row| {
            Ok(FileRecord {
                id: row.get::<_, i64>(0)? as u64,
                path: row.get(1)?,
                file_type: row.get(2)?,
                content_hash: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
                created_at: row.get(5)?,
                modified_at: row.get(6)?,
                is_deleted: row.get(7)?,
            })
        },
    );

    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StorageError::Database(e)),
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- index::tests`
Expected: all 14 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/src/
git commit -m "feat(storage): add index operations (insert, query, delete, FTS sync)"
```

---

### Task 17: Run full workspace tests

**Files:** (none — verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: no warnings.

- [ ] **Step 2: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass (125 existing + new storage tests).

---

## Phase 7: Tantivy Search

### Task 18: Write Tantivy search tests

**Files:**
- Create: `crates/nexus-storage/src/search.rs`

- [ ] **Step 1: Create `search.rs` with tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/search.rs`:

```rust
//! Tantivy full-text search: schema, index build, query.

use std::path::Path;

use crate::StorageError;

/// A search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// File path.
    pub file_path: String,
    /// Block ID in the SQLite index.
    pub block_id: u64,
    /// Block type.
    pub block_type: String,
    /// Text excerpt.
    pub excerpt: String,
    /// BM25 relevance score.
    pub score: f32,
}

/// Manages the Tantivy search index.
pub struct SearchIndex {
    index: tantivy::Index,
    path_field: tantivy::schema::Field,
    block_id_field: tantivy::schema::Field,
    block_type_field: tantivy::schema::Field,
    content_field: tantivy::schema::Field,
}

impl SearchIndex {
    /// Open or create a search index at the given directory.
    pub fn open(dir: &Path) -> Result<Self, StorageError> {
        let _ = dir;
        todo!()
    }

    /// Create an in-memory search index (for testing).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        todo!()
    }

    /// Add a block to the search index.
    pub fn add_block(
        &self,
        file_path: &str,
        block_id: u64,
        block_type: &str,
        content: &str,
    ) -> Result<(), StorageError> {
        let _ = (file_path, block_id, block_type, content);
        todo!()
    }

    /// Commit pending changes to the index.
    pub fn commit(&self) -> Result<(), StorageError> {
        todo!()
    }

    /// Search the index.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError> {
        let _ = (query, limit);
        todo!()
    }

    /// Delete all documents and rebuild from scratch.
    pub fn clear(&self) -> Result<(), StorageError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_succeeds() {
        let idx = SearchIndex::open_in_memory().unwrap();
        let _ = idx;
    }

    #[test]
    fn add_and_search_single_block() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/test.md", 1, "paragraph", "hello world of rust programming")
            .unwrap();
        idx.commit().unwrap();

        let results = idx.search("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "notes/test.md");
        assert_eq!(results[0].block_id, 1);
    }

    #[test]
    fn search_returns_empty_for_no_match() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/test.md", 1, "paragraph", "hello world").unwrap();
        idx.commit().unwrap();

        let results = idx.search("nonexistent", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_limit() {
        let idx = SearchIndex::open_in_memory().unwrap();
        for i in 0..20 {
            idx.add_block(
                &format!("notes/{i}.md"),
                i,
                "paragraph",
                "common search term here",
            )
            .unwrap();
        }
        idx.commit().unwrap();

        let results = idx.search("common", 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn search_phrase_query() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/a.md", 1, "paragraph", "machine learning is great").unwrap();
        idx.add_block("notes/b.md", 2, "paragraph", "learning about machines").unwrap();
        idx.commit().unwrap();

        let results = idx.search("\"machine learning\"", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "notes/a.md");
    }

    #[test]
    fn clear_removes_all_documents() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/a.md", 1, "paragraph", "some content").unwrap();
        idx.commit().unwrap();

        idx.clear().unwrap();
        idx.commit().unwrap();

        let results = idx.search("content", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn open_on_disk_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let search_dir = tmp.path().join("search");
        let idx = SearchIndex::open(&search_dir).unwrap();
        idx.add_block("notes/test.md", 1, "paragraph", "disk content").unwrap();
        idx.commit().unwrap();

        assert!(search_dir.exists());
    }

    #[test]
    fn search_returns_score() {
        let idx = SearchIndex::open_in_memory().unwrap();
        idx.add_block("notes/a.md", 1, "paragraph", "rust rust rust programming").unwrap();
        idx.add_block("notes/b.md", 2, "paragraph", "rust is nice").unwrap();
        idx.commit().unwrap();

        let results = idx.search("rust", 10).unwrap();
        assert!(results.len() >= 2);
        // Higher TF should score higher
        assert!(results[0].score >= results[1].score);
    }
}
```

- [ ] **Step 2: Add `mod search;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod index;`:

```rust
mod search;

pub use search::{SearchIndex, SearchResult};
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- search::tests`
Expected: FAIL — methods are `todo!()`.

### Task 19: Implement SearchIndex

**Files:**
- Modify: `crates/nexus-storage/src/search.rs`

- [ ] **Step 1: Replace the struct and method implementations**

Replace everything above the `#[cfg(test)]` block in `crates/nexus-storage/src/search.rs`:

```rust
//! Tantivy full-text search: schema, index build, query.

use std::path::Path;
use std::sync::Mutex;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, STORED, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};

use crate::StorageError;

/// A search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// File path.
    pub file_path: String,
    /// Block ID in the SQLite index.
    pub block_id: u64,
    /// Block type.
    pub block_type: String,
    /// Text excerpt.
    pub excerpt: String,
    /// BM25 relevance score.
    pub score: f32,
}

/// Manages the Tantivy search index.
pub struct SearchIndex {
    index: Index,
    writer: Mutex<IndexWriter>,
    path_field: tantivy::schema::Field,
    block_id_field: tantivy::schema::Field,
    block_type_field: tantivy::schema::Field,
    content_field: tantivy::schema::Field,
    #[allow(dead_code)]
    mtime_field: tantivy::schema::Field,
}

/// Build the Tantivy schema.
fn build_schema() -> (
    Schema,
    tantivy::schema::Field,
    tantivy::schema::Field,
    tantivy::schema::Field,
    tantivy::schema::Field,
    tantivy::schema::Field,
) {
    let mut builder = Schema::builder();
    let path_field = builder.add_text_field("path", STORED);
    let block_id_field = builder.add_u64_field("block_id", tantivy::schema::STORED);
    let block_type_field = builder.add_text_field("block_type", STORED);
    let content_field = builder.add_text_field("content", TEXT);
    let mtime_field = builder.add_date_field("mtime", tantivy::schema::STORED | tantivy::schema::INDEXED);
    (
        builder.build(),
        path_field,
        block_id_field,
        block_type_field,
        content_field,
        mtime_field,
    )
}

impl SearchIndex {
    /// Open or create a search index at the given directory.
    pub fn open(dir: &Path) -> Result<Self, StorageError> {
        std::fs::create_dir_all(dir)?;
        let (schema, path_field, block_id_field, block_type_field, content_field, mtime_field) = build_schema();
        let index = Index::create_in_dir(dir, schema)
            .or_else(|_| Index::open_in_dir(dir))
            .map_err(StorageError::Search)?;
        let writer = index.writer(50_000_000).map_err(StorageError::Search)?;

        Ok(Self {
            index,
            writer: Mutex::new(writer),
            path_field,
            block_id_field,
            block_type_field,
            content_field,
            mtime_field,
        })
    }

    /// Create an in-memory search index (for testing).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let (schema, path_field, block_id_field, block_type_field, content_field, mtime_field) = build_schema();
        let index = Index::create_in_ram(schema);
        let writer = index.writer(50_000_000).map_err(StorageError::Search)?;

        Ok(Self {
            index,
            writer: Mutex::new(writer),
            path_field,
            block_id_field,
            block_type_field,
            content_field,
            mtime_field,
        })
    }

    /// Add a block to the search index.
    pub fn add_block(
        &self,
        file_path: &str,
        block_id: u64,
        block_type: &str,
        content: &str,
    ) -> Result<(), StorageError> {
        let writer = self.writer.lock().unwrap();
        writer
            .add_document(doc!(
                self.path_field => file_path,
                self.block_id_field => block_id,
                self.block_type_field => block_type,
                self.content_field => content,
            ))
            .map_err(StorageError::Search)?;
        Ok(())
    }

    /// Commit pending changes to the index.
    pub fn commit(&self) -> Result<(), StorageError> {
        let mut writer = self.writer.lock().unwrap();
        writer.commit().map_err(StorageError::Search)?;
        Ok(())
    }

    /// Search the index.
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(StorageError::Search)?;
        reader.reload().map_err(StorageError::Search)?;
        let searcher = reader.searcher();

        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field]);
        let query = query_parser
            .parse_query(query_str)
            .map_err(|e| StorageError::Search(tantivy::TantivyError::InvalidArgument(e.to_string())))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(StorageError::Search)?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let doc: tantivy::TantivyDocument = searcher.doc(doc_address).map_err(StorageError::Search)?;

            let file_path = doc
                .get_first(self.path_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let block_id = doc
                .get_first(self.block_id_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let block_type = doc
                .get_first(self.block_type_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            results.push(SearchResult {
                file_path,
                block_id,
                block_type,
                excerpt: String::new(), // excerpt generation deferred
                score,
            });
        }

        Ok(results)
    }

    /// Delete all documents and rebuild from scratch.
    pub fn clear(&self) -> Result<(), StorageError> {
        let mut writer = self.writer.lock().unwrap();
        writer.delete_all_documents().map_err(StorageError::Search)?;
        writer.commit().map_err(StorageError::Search)?;
        Ok(())
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- search::tests`
Expected: all 8 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/src/search.rs
git commit -m "feat(storage): add Tantivy search index with BM25 scoring"
```

---

### Task 20: Run full workspace tests

**Files:** (none — verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: no warnings.

- [ ] **Step 2: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

---

## Phase 8: File Watcher

### Task 21: Write watcher types

**Files:**
- Create: `crates/nexus-storage/src/watcher.rs`

- [ ] **Step 1: Create `watcher.rs` with types and basic tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/watcher.rs`:

```rust
//! File watcher: notify + debouncer, rename detection, git batch mode.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::StorageError;

/// Default debounce window in milliseconds.
pub const DEFAULT_DEBOUNCE_MS: u64 = 300;

/// Events emitted by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageEvent {
    /// A file was created.
    FileCreated {
        /// Path relative to forge root.
        path: String,
        /// SHA-256 content hash.
        content_hash: String,
    },
    /// A file was modified.
    FileModified {
        /// Path relative to forge root.
        path: String,
        /// New SHA-256 content hash.
        content_hash: String,
    },
    /// A file was deleted.
    FileDeleted {
        /// Path relative to forge root.
        path: String,
    },
    /// A file was renamed (detected via hash match).
    FileRenamed {
        /// Old path relative to forge root.
        from: String,
        /// New path relative to forge root.
        to: String,
        /// Content hash (unchanged across rename).
        content_hash: String,
    },
}

/// Patterns to ignore when watching.
const IGNORE_PATTERNS: &[&str] = &[
    ".git",
    ".forge/temp",
    "node_modules",
];

/// File extensions to ignore.
const IGNORE_EXTENSIONS: &[&str] = &[
    "~",
    ".swp",
    ".DS_Store",
];

/// Check if a path should be ignored by the watcher.
pub fn should_ignore(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    for pattern in IGNORE_PATTERNS {
        if path_str.contains(pattern) {
            return true;
        }
    }

    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        for ext in IGNORE_EXTENSIONS {
            if name.ends_with(ext) {
                return true;
            }
        }
    }

    false
}

/// Convert an absolute path to a relative path from the forge root.
pub fn relative_path(forge_root: &Path, absolute: &Path) -> Option<String> {
    absolute
        .strip_prefix(forge_root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_ignore_git_directory() {
        assert!(should_ignore(Path::new("/forge/.git/objects/abc")));
    }

    #[test]
    fn should_ignore_forge_temp() {
        assert!(should_ignore(Path::new("/forge/.forge/temp/uuid.tmp")));
    }

    #[test]
    fn should_ignore_node_modules() {
        assert!(should_ignore(Path::new("/forge/node_modules/pkg/index.js")));
    }

    #[test]
    fn should_ignore_swap_files() {
        assert!(should_ignore(Path::new("/forge/notes/test.md.swp")));
    }

    #[test]
    fn should_ignore_backup_files() {
        assert!(should_ignore(Path::new("/forge/notes/test.md~")));
    }

    #[test]
    fn should_ignore_ds_store() {
        assert!(should_ignore(Path::new("/forge/notes/.DS_Store")));
    }

    #[test]
    fn should_not_ignore_markdown() {
        assert!(!should_ignore(Path::new("/forge/notes/test.md")));
    }

    #[test]
    fn should_not_ignore_attachments() {
        assert!(!should_ignore(Path::new("/forge/attachments/image.png")));
    }

    #[test]
    fn relative_path_strips_root() {
        let root = Path::new("/forge");
        let abs = Path::new("/forge/notes/test.md");
        assert_eq!(relative_path(root, abs), Some("notes/test.md".to_string()));
    }

    #[test]
    fn relative_path_returns_none_for_outside() {
        let root = Path::new("/forge");
        let abs = Path::new("/other/file.md");
        assert_eq!(relative_path(root, abs), None);
    }

    #[test]
    fn storage_event_variants_are_eq() {
        let e1 = StorageEvent::FileCreated {
            path: "a.md".to_string(),
            content_hash: "abc".to_string(),
        };
        let e2 = StorageEvent::FileCreated {
            path: "a.md".to_string(),
            content_hash: "abc".to_string(),
        };
        assert_eq!(e1, e2);
    }
}
```

- [ ] **Step 2: Add `mod watcher;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod search;`:

```rust
mod watcher;

pub use watcher::StorageEvent;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- watcher::tests`
Expected: all 11 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/
git commit -m "feat(storage): add watcher types, ignore patterns, and path helpers"
```

---

### Task 22: Implement Watcher struct

**Files:**
- Modify: `crates/nexus-storage/src/watcher.rs`

- [ ] **Step 1: Add the Watcher struct and start method**

Add before the `#[cfg(test)]` block in `crates/nexus-storage/src/watcher.rs`:

```rust
/// File watcher that monitors forge directories for changes.
pub struct Watcher {
    /// Receiver for storage events.
    rx: mpsc::Receiver<StorageEvent>,
    /// Handle to the debouncer (keeps it alive).
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl Watcher {
    /// Start watching the given forge root.
    ///
    /// Watches `notes/` and `attachments/` directories.
    /// Returns events on the receiver from `events()`.
    pub fn start(forge_root: &Path, debounce_ms: u64) -> Result<Self, StorageError> {
        let (event_tx, event_rx) = mpsc::channel();
        let root = forge_root.to_path_buf();

        let (notify_tx, notify_rx) = mpsc::channel();
        let debouncer = notify_debouncer_mini::new_debouncer(
            Duration::from_millis(debounce_ms),
            notify_tx,
        )
        .map_err(StorageError::Watcher)?;

        // Spawn processing thread
        let root_clone = root.clone();
        std::thread::spawn(move || {
            process_events(notify_rx, &root_clone, &event_tx);
        });

        // Watch notes/ and attachments/
        let notes_dir = forge_root.join("notes");
        let attachments_dir = forge_root.join("attachments");

        let watcher = debouncer.watcher();
        if notes_dir.exists() {
            watcher
                .watch(&notes_dir, notify::RecursiveMode::Recursive)
                .map_err(StorageError::Watcher)?;
        }
        if attachments_dir.exists() {
            watcher
                .watch(&attachments_dir, notify::RecursiveMode::Recursive)
                .map_err(StorageError::Watcher)?;
        }

        Ok(Self {
            rx: event_rx,
            _debouncer: debouncer,
        })
    }

    /// Get the receiver for storage events.
    pub fn events(&self) -> &mpsc::Receiver<StorageEvent> {
        &self.rx
    }
}

/// Process raw debounced events into StorageEvents.
///
/// Handles rename detection (deleted + created with matching hash within a
/// debounce batch) and git batch mode (.git/index.lock suppression).
fn process_events(
    rx: mpsc::Receiver<Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>>,
    forge_root: &Path,
    tx: &mpsc::Sender<StorageEvent>,
) {
    use crate::parser::content_hash;
    use std::collections::HashMap;

    // Track recently deleted file hashes for rename detection
    let mut deleted_hashes: HashMap<String, String> = HashMap::new(); // hash -> old_rel_path
    let git_lock = forge_root.join(".git/index.lock");
    let mut git_batch_mode = false;

    for result in rx {
        let events = match result {
            Ok(events) => events,
            Err(_) => continue,
        };

        // Git batch mode: if .git/index.lock exists, suppress events
        if git_lock.exists() {
            git_batch_mode = true;
            continue;
        }

        // If we were in git batch mode and the lock is gone, signal a full reconcile
        if git_batch_mode {
            git_batch_mode = false;
            // Emit a sentinel event that the StorageEngine interprets as "run reconcile"
            let _ = tx.send(StorageEvent::FileModified {
                path: String::new(), // empty path = reconcile signal
                content_hash: String::new(),
            });
            deleted_hashes.clear();
            continue;
        }

        // First pass: collect deletions and their index hashes
        let mut batch_deleted: Vec<String> = Vec::new();
        let mut batch_alive: Vec<(PathBuf, String, String)> = Vec::new(); // (abs, rel, hash)

        for event in &events {
            let path = &event.path;
            if should_ignore(path) {
                continue;
            }
            let rel = match relative_path(forge_root, path) {
                Some(r) => r,
                None => continue,
            };

            if path.exists() {
                let hash = match std::fs::read(path) {
                    Ok(bytes) => content_hash(&bytes),
                    Err(_) => continue,
                };
                batch_alive.push((path.clone(), rel, hash));
            } else {
                batch_deleted.push(rel);
            }
        }

        // Attempt rename detection: for each deleted file, check if any
        // created/modified file in the same batch has a matching hash.
        // We need the deleted file's hash from the index, which we don't
        // have here. Instead, we store hashes from previous events.
        // For within-batch renames (delete + create in same debounce window),
        // check if any newly-alive file's hash matches a known deleted hash.
        for del_path in &batch_deleted {
            deleted_hashes.remove(del_path); // clean stale
        }

        // Emit events, checking for renames
        for (_, rel, hash) in &batch_alive {
            if let Some(old_path) = deleted_hashes.remove(hash) {
                // Rename detected: same hash appeared after a deletion
                let _ = tx.send(StorageEvent::FileRenamed {
                    from: old_path,
                    to: rel.clone(),
                    content_hash: hash.clone(),
                });
            } else {
                let _ = tx.send(StorageEvent::FileModified {
                    path: rel.clone(),
                    content_hash: hash.clone(),
                });
            }
        }

        for del_path in batch_deleted {
            // Store the deletion for potential cross-batch rename detection.
            // We don't have the hash here (file is gone), so we can only
            // detect renames within the same debounce batch. Cross-batch
            // renames are caught by the reconciler.
            let _ = tx.send(StorageEvent::FileDeleted { path: del_path });
        }
    }
}
```

- [ ] **Step 2: Add watcher integration test**

Add to the `#[cfg(test)] mod tests` block in `crates/nexus-storage/src/watcher.rs`:

```rust
    #[test]
    fn watcher_detects_file_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let watcher = Watcher::start(tmp.path(), 100).unwrap();

        // Create a file
        std::fs::write(notes_dir.join("test.md"), "hello").unwrap();

        // Wait for event (with timeout)
        let event = watcher
            .events()
            .recv_timeout(std::time::Duration::from_secs(5));

        assert!(event.is_ok(), "should receive an event within 5s");
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo nextest run -p nexus-storage -- watcher::tests`
Expected: all 12 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/watcher.rs
git commit -m "feat(storage): add file watcher with notify debouncer and ignore patterns"
```

---

### Task 23: Run full workspace tests

**Files:** (none — verification only)

- [ ] **Step 1: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

---

## Phase 9: Reconciliation

### Task 24: Write reconciliation tests

**Files:**
- Create: `crates/nexus-storage/src/reconcile.rs`

- [ ] **Step 1: Create `reconcile.rs` with types and tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/reconcile.rs`:

```rust
//! Full directory scan, hash-based delta, index sync.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use crate::index::{self, FileRecord};
use crate::parser::{self, content_hash};
use crate::StorageError;

/// Summary of changes found during reconciliation.
#[derive(Debug, Clone, Default)]
pub struct ReconcileDelta {
    /// Files newly found on disk but not in index.
    pub created: usize,
    /// Files whose content hash changed.
    pub modified: usize,
    /// Files whose path changed but hash remained the same.
    pub renamed: usize,
    /// Files in the index but not on disk (soft-deleted).
    pub deleted: usize,
}

/// Reconcile the index against the filesystem.
///
/// Scans all files under `notes/` and `attachments/`, compares against
/// the SQLite index, and applies the minimal delta.
pub fn reconcile(conn: &Connection, forge_root: &Path) -> Result<ReconcileDelta, StorageError> {
    let _ = (conn, forge_root);
    todo!()
}

/// Collect all files in a directory tree, returning (relative_path, content_hash, size).
fn scan_directory(forge_root: &Path) -> Result<Vec<(String, String, u64)>, StorageError> {
    let _ = forge_root;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::configure_pragmas(&conn).unwrap();
        schema::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn reconcile_empty_forge_empty_index() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("notes")).unwrap();
        std::fs::create_dir_all(tmp.path().join("attachments")).unwrap();

        let conn = setup_db();
        let delta = reconcile(&conn, tmp.path()).unwrap();

        assert_eq!(delta.created, 0);
        assert_eq!(delta.modified, 0);
        assert_eq!(delta.renamed, 0);
        assert_eq!(delta.deleted, 0);
    }

    #[test]
    fn reconcile_detects_new_files() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("hello.md"), "# Hello").unwrap();
        std::fs::write(notes.join("world.md"), "# World").unwrap();

        let conn = setup_db();
        let delta = reconcile(&conn, tmp.path()).unwrap();

        assert_eq!(delta.created, 2);
        assert_eq!(delta.modified, 0);

        // Verify files are in the index
        let files = index::query_files(&conn, &index::FileFilter::default()).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn reconcile_detects_modified_files() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("test.md"), "# Original").unwrap();

        let conn = setup_db();
        // First reconcile
        reconcile(&conn, tmp.path()).unwrap();

        // Modify the file
        std::fs::write(notes.join("test.md"), "# Modified").unwrap();

        // Second reconcile
        let delta = reconcile(&conn, tmp.path()).unwrap();
        assert_eq!(delta.modified, 1);
        assert_eq!(delta.created, 0);
    }

    #[test]
    fn reconcile_detects_deleted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("test.md"), "# Will Delete").unwrap();

        let conn = setup_db();
        reconcile(&conn, tmp.path()).unwrap();

        // Delete the file
        std::fs::remove_file(notes.join("test.md")).unwrap();

        let delta = reconcile(&conn, tmp.path()).unwrap();
        assert_eq!(delta.deleted, 1);

        // File should be soft-deleted
        let filter = index::FileFilter {
            include_deleted: true,
            ..Default::default()
        };
        let files = index::query_files(&conn, &filter).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].is_deleted);
    }

    #[test]
    fn reconcile_detects_renamed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("old.md"), "# Rename Me").unwrap();

        let conn = setup_db();
        reconcile(&conn, tmp.path()).unwrap();

        // Rename
        std::fs::rename(notes.join("old.md"), notes.join("new.md")).unwrap();

        let delta = reconcile(&conn, tmp.path()).unwrap();
        assert_eq!(delta.renamed, 1);
        assert_eq!(delta.created, 0);
        assert_eq!(delta.deleted, 0);

        // Verify path updated
        let file = index::file_by_path(&conn, "notes/new.md").unwrap();
        assert!(file.is_some());
    }

    #[test]
    fn reconcile_idempotent_no_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("test.md"), "# Stable").unwrap();

        let conn = setup_db();
        reconcile(&conn, tmp.path()).unwrap();

        // Run again without changes
        let delta = reconcile(&conn, tmp.path()).unwrap();
        assert_eq!(delta.created, 0);
        assert_eq!(delta.modified, 0);
        assert_eq!(delta.renamed, 0);
        assert_eq!(delta.deleted, 0);
    }

    #[test]
    fn reconcile_handles_nested_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("notes/sub/deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("deep.md"), "# Deep").unwrap();

        let conn = setup_db();
        let delta = reconcile(&conn, tmp.path()).unwrap();
        assert_eq!(delta.created, 1);

        let file = index::file_by_path(&conn, "notes/sub/deep/deep.md").unwrap();
        assert!(file.is_some());
    }

    #[test]
    fn scan_ignores_git_and_forge_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("good.md"), "keep").unwrap();

        // Create files that should be ignored
        let git_dir = tmp.path().join(".git/objects");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("abc"), "ignored").unwrap();

        let temp_dir = tmp.path().join(".forge/temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        std::fs::write(temp_dir.join("uuid.tmp"), "ignored").unwrap();

        let files = scan_directory(tmp.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "notes/good.md");
    }
}
```

- [ ] **Step 2: Add `mod reconcile;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`, add after `mod watcher;`:

```rust
mod reconcile;

pub use reconcile::ReconcileDelta;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-storage -- reconcile::tests`
Expected: FAIL — functions are `todo!()`.

### Task 25: Implement reconciliation

**Files:**
- Modify: `crates/nexus-storage/src/reconcile.rs`

- [ ] **Step 1: Implement `scan_directory` and `reconcile`**

Replace everything above the `#[cfg(test)]` block in `crates/nexus-storage/src/reconcile.rs`:

```rust
//! Full directory scan, hash-based delta, index sync.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use crate::index::{self, FileRecord};
use crate::parser::{self, content_hash};
use crate::watcher::should_ignore;
use crate::StorageError;

/// Summary of changes found during reconciliation.
#[derive(Debug, Clone, Default)]
pub struct ReconcileDelta {
    /// Files newly found on disk but not in index.
    pub created: usize,
    /// Files whose content hash changed.
    pub modified: usize,
    /// Files whose path changed but hash remained the same.
    pub renamed: usize,
    /// Files in the index but not on disk (soft-deleted).
    pub deleted: usize,
}

/// Reconcile the index against the filesystem.
pub fn reconcile(conn: &Connection, forge_root: &Path) -> Result<ReconcileDelta, StorageError> {
    let mut delta = ReconcileDelta::default();

    // Scan filesystem
    let disk_files = scan_directory(forge_root)?;

    // Get all files from index (including deleted, for rename detection)
    let filter = index::FileFilter {
        include_deleted: false,
        ..Default::default()
    };
    let index_files = index::query_files(conn, &filter)?;

    // Build lookup maps
    let mut index_by_path: HashMap<String, FileRecord> = HashMap::new();
    let mut index_by_hash: HashMap<String, Vec<FileRecord>> = HashMap::new();
    for file in index_files {
        index_by_path.insert(file.path.clone(), file.clone());
        index_by_hash
            .entry(file.content_hash.clone())
            .or_default()
            .push(file);
    }

    let mut disk_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (rel_path, hash, size) in &disk_files {
        disk_paths.insert(rel_path.clone());

        if let Some(existing) = index_by_path.get(rel_path) {
            // Path exists in index — check if content changed
            if existing.content_hash != *hash {
                // Modified: re-parse and update
                let abs_path = forge_root.join(rel_path);
                let content = std::fs::read_to_string(&abs_path).unwrap_or_default();
                let parsed = parser::parse_markdown(&content)?;

                // Delete old data and re-insert
                index::delete_file(conn, existing.id)?;
                let file_type = infer_file_type(rel_path);
                index::insert_file(conn, rel_path, &file_type, *size, &parsed)?;
                delta.modified += 1;
            }
            // else: unchanged, no-op
        } else {
            // Path not in index — check for rename (hash match with different path)
            let mut was_rename = false;
            if let Some(hash_matches) = index_by_hash.get(hash) {
                for candidate in hash_matches {
                    if !disk_paths.contains(&candidate.path)
                        && !forge_root.join(&candidate.path).exists()
                    {
                        // Rename detected: same hash, old path gone
                        conn.execute(
                            "UPDATE files SET path = ?1 WHERE id = ?2",
                            rusqlite::params![rel_path, candidate.id as i64],
                        )
                        .map_err(StorageError::Database)?;
                        delta.renamed += 1;
                        was_rename = true;
                        break;
                    }
                }
            }

            if !was_rename {
                // New file
                let abs_path = forge_root.join(rel_path);
                let content = std::fs::read_to_string(&abs_path).unwrap_or_default();
                let parsed = parser::parse_markdown(&content)?;
                let file_type = infer_file_type(rel_path);
                index::insert_file(conn, rel_path, &file_type, *size, &parsed)?;
                delta.created += 1;
            }
        }
    }

    // Soft-delete files in index but not on disk
    for (path, record) in &index_by_path {
        if !disk_paths.contains(path) {
            index::soft_delete_file(conn, record.id)?;
            delta.deleted += 1;
        }
    }

    Ok(delta)
}

/// Collect all files in `notes/` and `attachments/` directories.
/// Returns (relative_path, content_hash, size_bytes).
fn scan_directory(forge_root: &Path) -> Result<Vec<(String, String, u64)>, StorageError> {
    let mut results = Vec::new();

    let dirs_to_scan = ["notes", "attachments"];
    for dir_name in &dirs_to_scan {
        let dir = forge_root.join(dir_name);
        if dir.exists() {
            scan_dir_recursive(&dir, forge_root, &mut results)?;
        }
    }

    Ok(results)
}

/// Recursively scan a directory and collect file info.
fn scan_dir_recursive(
    dir: &Path,
    forge_root: &Path,
    results: &mut Vec<(String, String, u64)>,
) -> Result<(), StorageError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if should_ignore(&path) {
            continue;
        }

        if path.is_dir() {
            scan_dir_recursive(&path, forge_root, results)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(forge_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let bytes = std::fs::read(&path)?;
            let hash = content_hash(&bytes);
            let size = bytes.len() as u64;
            results.push((rel, hash, size));
        }
    }
    Ok(())
}

/// Infer file type from relative path.
fn infer_file_type(path: &str) -> String {
    if path.starts_with("attachments/") {
        "attachment".to_string()
    } else {
        "note".to_string()
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-storage -- reconcile::tests`
Expected: all 8 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/src/reconcile.rs
git commit -m "feat(storage): add reconciliation engine with hash-based rename detection"
```

---

## Phase 10: StorageEngine Facade & Smoke Test

### Task 26: Build StorageEngine facade

**Files:**
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Add StorageConfig and StorageEngine**

Replace the contents of `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs` with:

```rust
//! Nexus storage engine: forge layout, atomic writes, SQLite index,
//! markdown parsing, file watching, and Tantivy full-text search.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-03-storage-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod atomic;
mod error;
mod forge;
mod index;
mod parser;
mod reconcile;
mod schema;
mod search;
mod watcher;

pub use error::StorageError;
pub use forge::{Forge, ForgeLock};
pub use index::{
    BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, RebuildStats, TagResult,
};
pub use parser::{content_hash, parse_markdown, ParsedBlock, ParsedFile, ParsedLink, ParsedTag, Property};
pub use reconcile::ReconcileDelta;
pub use search::{SearchIndex, SearchResult};
pub use watcher::StorageEvent;

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};

/// Configuration for the storage engine.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// SQLite connection pool size (default: 4).
    pub pool_size: u32,
    /// File watcher debounce window in milliseconds (default: 300).
    pub debounce_ms: u64,
    /// Rayon thread pool size (default: 0 = auto-detect).
    pub rayon_threads: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            pool_size: 4,
            debounce_ms: 300,
            rayon_threads: 0,
        }
    }
}

/// The main storage engine facade. Owns the forge, database, search index,
/// file watcher, and forge lock.
pub struct StorageEngine {
    forge: Forge,
    _lock: ForgeLock,
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    write_conn: Mutex<rusqlite::Connection>,
    search_index: SearchIndex,
    _watcher: Option<watcher::Watcher>,
}

impl StorageEngine {
    /// Initialize a new forge and open it.
    pub fn init(root: &Path) -> Result<Self, StorageError> {
        let forge = Forge::new(root);
        forge.init()?;

        Self::open_internal(forge, &StorageConfig::default(), true)
    }

    /// Open an existing forge.
    pub fn open(root: &Path, config: &StorageConfig) -> Result<Self, StorageError> {
        let forge = Forge::new(root);
        if !forge.forge_dir().exists() {
            return Err(StorageError::FileNotFound(
                "no .forge directory found — run init first".to_string(),
            ));
        }

        Self::open_internal(forge, config, false)
    }

    /// Internal open implementation.
    fn open_internal(forge: Forge, config: &StorageConfig, is_new: bool) -> Result<Self, StorageError> {
        // Acquire exclusive forge lock
        let lock = forge.acquire_lock()?;

        // Clean stale temp files
        forge.clean_temp()?;

        // Set up read pool
        let manager = r2d2_sqlite::SqliteConnectionManager::file(forge.index_db_path());
        let pool = r2d2::Pool::builder()
            .max_size(config.pool_size)
            .build(manager)
            .map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;

        // Configure and migrate via a pool connection
        {
            let conn = pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
            schema::configure_pragmas(&conn)?;
            schema::migrate(&conn)?;
        }

        // Separate write connection
        let write_conn = rusqlite::Connection::open(forge.index_db_path())
            .map_err(StorageError::Database)?;
        schema::configure_pragmas(&write_conn)?;

        // Search index
        let search_index = SearchIndex::open(&forge.search_dir())?;

        // File watcher (best-effort — non-fatal if it fails)
        let watcher_handle = watcher::Watcher::start(forge.root(), config.debounce_ms).ok();

        // Initial reconcile for existing forges
        if !is_new {
            reconcile::reconcile(&write_conn, forge.root())?;
        }

        Ok(Self {
            forge,
            _lock: lock,
            pool,
            write_conn: Mutex::new(write_conn),
            search_index,
            _watcher: watcher_handle,
        })
    }

    /// Get the watcher's event receiver. Returns `None` if the watcher
    /// failed to start (e.g., on platforms without inotify support).
    pub fn watch_changes(&self) -> Option<&mpsc::Receiver<StorageEvent>> {
        self._watcher.as_ref().map(|w| w.events())
    }

    /// Write a file atomically, parse it, and update the index.
    pub fn write_file(&self, path: &str, content: &[u8]) -> Result<FileMetadata, StorageError> {
        let abs_path = self.forge.root().join(path);
        atomic::atomic_write(&abs_path, content, &self.forge.temp_dir())?;

        let content_str = String::from_utf8_lossy(content);
        let parsed = parser::parse_markdown(&content_str)?;
        let size = content.len() as u64;
        let file_type = if path.starts_with("attachments/") {
            "attachment"
        } else {
            "note"
        };

        let conn = self.write_conn.lock().unwrap();

        // Remove old entry if exists
        if let Some(existing) = index::file_by_path(&conn, path)? {
            index::delete_file(&conn, existing.id)?;
        }

        let _file_id = index::insert_file(&conn, path, file_type, size, &parsed)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        Ok(FileMetadata {
            path: path.to_string(),
            size_bytes: size,
            modified_at: now,
            content_hash: parsed.content_hash,
        })
    }

    /// Read a file from the forge.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        let abs_path = self.forge.root().join(path);
        std::fs::read(&abs_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::FileNotFound(path.to_string())
            } else {
                StorageError::Io(e)
            }
        })
    }

    /// Delete a file from the forge and index.
    pub fn delete_file(&self, path: &str) -> Result<(), StorageError> {
        let abs_path = self.forge.root().join(path);
        if abs_path.exists() {
            std::fs::remove_file(&abs_path)?;
        }

        let conn = self.write_conn.lock().unwrap();
        if let Some(existing) = index::file_by_path(&conn, path)? {
            index::delete_file(&conn, existing.id)?;
        }

        Ok(())
    }

    /// List files matching a prefix.
    pub fn list_files(&self, prefix: &str) -> Result<Vec<FileMetadata>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        let filter = FileFilter {
            prefix: Some(prefix.to_string()),
            ..Default::default()
        };
        let records = index::query_files(&conn, &filter)?;
        Ok(records
            .into_iter()
            .map(|r| FileMetadata {
                path: r.path,
                size_bytes: r.size_bytes,
                modified_at: r.modified_at,
                content_hash: r.content_hash,
            })
            .collect())
    }

    /// Check if a file exists.
    pub fn file_exists(&self, path: &str) -> Result<bool, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        Ok(index::file_by_path(&conn, path)?.is_some())
    }

    /// Query files from the index.
    pub fn query_files(&self, filter: &FileFilter) -> Result<Vec<FileRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        index::query_files(&conn, filter)
    }

    /// Query blocks for a file.
    pub fn query_blocks(&self, file_id: u64) -> Result<Vec<BlockRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        index::query_blocks(&conn, file_id)
    }

    /// Query links originating from a file.
    pub fn query_links(&self, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        index::query_links(&conn, file_id)
    }

    /// Query backlinks pointing to a file.
    pub fn query_backlinks(&self, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        index::query_backlinks(&conn, file_id)
    }

    /// Query tags by name.
    pub fn query_tags(&self, name: &str) -> Result<Vec<TagResult>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        index::query_tags(&conn, name)
    }

    /// Rebuild the index from scratch.
    pub fn rebuild_index(&self) -> Result<RebuildStats, StorageError> {
        let start = std::time::Instant::now();
        let conn = self.write_conn.lock().unwrap();

        // Clear existing data
        conn.execute_batch(
            "DELETE FROM fts_blocks;
             DELETE FROM blocks;
             DELETE FROM links;
             DELETE FROM tags;
             DELETE FROM properties;
             DELETE FROM files;"
        )
        .map_err(StorageError::Database)?;

        // Reconcile re-inserts everything
        let delta = reconcile::reconcile(&conn, self.forge.root())?;

        // Count blocks, links, tags from the freshly-rebuilt index
        let blocks_indexed: i64 = conn
            .query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))
            .unwrap_or(0);
        let links_found: i64 = conn
            .query_row("SELECT COUNT(*) FROM links", [], |row| row.get(0))
            .unwrap_or(0);
        let tags_found: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .unwrap_or(0);

        Ok(RebuildStats {
            files_processed: delta.created + delta.modified,
            blocks_indexed: blocks_indexed as usize,
            links_found: links_found as usize,
            tags_found: tags_found as usize,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Full-text search.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError> {
        self.search_index.search(query, limit)
    }

    /// Rebuild the Tantivy search index from SQLite data.
    pub fn rebuild_search_index(&self) -> Result<(), StorageError> {
        self.search_index.clear()?;

        let conn = self.pool.get().map_err(|e| StorageError::ConfigInvalid(e.to_string()))?;
        let files = index::query_files(&conn, &FileFilter::default())?;

        for file in &files {
            let blocks = index::query_blocks(&conn, file.id)?;
            for block in &blocks {
                self.search_index.add_block(
                    &file.path,
                    block.id,
                    &block.block_type,
                    &block.content,
                )?;
            }
        }

        self.search_index.commit()?;
        Ok(())
    }

    /// Get the forge handle.
    pub fn forge(&self) -> &Forge {
        &self.forge
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p nexus-storage`
Expected: compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): add StorageEngine facade composing all subsystems"
```

---

### Task 27: Write StorageEngine integration tests

**Files:**
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Add tests module**

Add at the end of `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_working_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();
        assert!(tmp.path().join(".forge/index.db").exists());
        assert!(tmp.path().join("notes").is_dir());
        let _ = engine;
    }

    #[test]
    fn write_and_read_file() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();

        let meta = engine
            .write_file("notes/test.md", b"# Hello\n\nWorld")
            .unwrap();
        assert_eq!(meta.path, "notes/test.md");
        assert_eq!(meta.size_bytes, 15);

        let content = engine.read_file("notes/test.md").unwrap();
        assert_eq!(content, b"# Hello\n\nWorld");
    }

    #[test]
    fn write_file_is_indexed() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();

        engine
            .write_file("notes/test.md", b"# Hello\n\nWorld")
            .unwrap();

        assert!(engine.file_exists("notes/test.md").unwrap());

        let files = engine.list_files("notes/").unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn delete_file_removes_from_index() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();

        engine
            .write_file("notes/test.md", b"# Hello")
            .unwrap();
        engine.delete_file("notes/test.md").unwrap();

        assert!(!engine.file_exists("notes/test.md").unwrap());
        assert!(!tmp.path().join("notes/test.md").exists());
    }

    #[test]
    fn query_blocks_after_write() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();

        engine
            .write_file("notes/test.md", b"# Title\n\nParagraph text.")
            .unwrap();

        let files = engine
            .query_files(&FileFilter::default())
            .unwrap();
        assert_eq!(files.len(), 1);

        let blocks = engine.query_blocks(files[0].id).unwrap();
        assert!(blocks.len() >= 2); // heading + paragraph
    }

    #[test]
    fn query_tags_after_write() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();

        engine
            .write_file("notes/test.md", b"# Tagged\n\nThis has #rust tag.")
            .unwrap();

        let tags = engine.query_tags("rust").unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].file_path, "notes/test.md");
    }

    #[test]
    fn rebuild_index_reindexes_all() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();

        engine
            .write_file("notes/a.md", b"# A")
            .unwrap();
        engine
            .write_file("notes/b.md", b"# B")
            .unwrap();

        let stats = engine.rebuild_index().unwrap();
        assert_eq!(stats.files_processed, 2);
    }

    #[test]
    fn read_nonexistent_file_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = StorageEngine::init(tmp.path()).unwrap();

        let result = engine.read_file("notes/missing.md");
        assert!(matches!(result, Err(StorageError::FileNotFound(_))));
    }

    #[test]
    fn open_nonexistent_forge_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = StorageEngine::open(tmp.path(), &StorageConfig::default());
        assert!(matches!(result, Err(StorageError::FileNotFound(_))));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-storage -- tests::`
Expected: all integration tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-storage/src/lib.rs
git commit -m "test(storage): add StorageEngine integration tests"
```

---

### Task 28: PRD 03 smoke test

**Files:**
- Create: `crates/nexus-storage/tests/prd-03-smoke.rs`

- [ ] **Step 1: Create smoke test**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-storage/tests/prd-03-smoke.rs`:

```rust
//! PRD 03 smoke test: verifies the public API surface and key integration paths.

use nexus_storage::{
    BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, ParsedBlock, ParsedFile,
    ParsedLink, ParsedTag, Property, ReconcileDelta, SearchIndex, SearchResult, StorageConfig,
    StorageEngine, StorageError, StorageEvent, TagResult,
};

#[test]
fn public_type_surface_is_accessible() {
    // Verify all public types are importable
    fn _assert_types() {
        let _: Option<StorageError> = None;
        let _: Option<StorageConfig> = None;
        let _: Option<FileMetadata> = None;
        let _: Option<FileRecord> = None;
        let _: Option<FileFilter> = None;
        let _: Option<BlockRecord> = None;
        let _: Option<LinkRecord> = None;
        let _: Option<TagResult> = None;
        let _: Option<ParsedFile> = None;
        let _: Option<ParsedBlock> = None;
        let _: Option<ParsedLink> = None;
        let _: Option<ParsedTag> = None;
        let _: Option<Property> = None;
        let _: Option<SearchResult> = None;
        let _: Option<StorageEvent> = None;
        let _: Option<ReconcileDelta> = None;
    }
}

#[test]
fn init_write_read_delete_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(tmp.path()).unwrap();

    // Write
    let meta = engine.write_file("notes/hello.md", b"# Hello\n\nWorld").unwrap();
    assert_eq!(meta.path, "notes/hello.md");

    // Read
    let content = engine.read_file("notes/hello.md").unwrap();
    assert_eq!(content, b"# Hello\n\nWorld");

    // Exists
    assert!(engine.file_exists("notes/hello.md").unwrap());

    // List
    let files = engine.list_files("notes/").unwrap();
    assert_eq!(files.len(), 1);

    // Delete
    engine.delete_file("notes/hello.md").unwrap();
    assert!(!engine.file_exists("notes/hello.md").unwrap());
}

#[test]
fn index_queries_return_parsed_data() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(tmp.path()).unwrap();

    let md = b"---\ntags:\n  - rust\n---\n\n# Title\n\nSee [[other]] for details. Has #inline tag.";
    engine.write_file("notes/test.md", md).unwrap();

    // Query files
    let files = engine.query_files(&FileFilter::default()).unwrap();
    assert_eq!(files.len(), 1);
    let file = &files[0];

    // Query blocks
    let blocks = engine.query_blocks(file.id).unwrap();
    assert!(!blocks.is_empty());
    assert!(blocks.iter().any(|b| b.block_type == "heading"));

    // Query tags
    let tags = engine.query_tags("rust").unwrap();
    assert!(!tags.is_empty());

    let inline_tags = engine.query_tags("inline").unwrap();
    assert!(!inline_tags.is_empty());

    // Query links
    let links = engine.query_links(file.id).unwrap();
    assert!(!links.is_empty());
}

#[test]
fn search_index_standalone() {
    let idx = SearchIndex::open_in_memory().unwrap();
    idx.add_block("notes/a.md", 1, "paragraph", "rust programming language").unwrap();
    idx.add_block("notes/b.md", 2, "paragraph", "python scripting").unwrap();
    idx.commit().unwrap();

    let results = idx.search("rust", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_path, "notes/a.md");
}

#[test]
fn rebuild_search_index_syncs_from_sqlite() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(tmp.path()).unwrap();

    engine.write_file("notes/a.md", b"# Searchable Content\n\nRust is great.").unwrap();

    engine.rebuild_search_index().unwrap();

    let results = engine.search("rust", 10).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn parser_handles_complex_markdown() {
    let md = r#"---
title: Complex Note
tags:
  - complex
  - test
---

# Main Title

Some text with a [[wikilink]] and a [markdown link](https://example.com).

## Code Example

```rust
fn main() {}
```

| Column A | Column B |
|----------|----------|
| Cell 1   | Cell 2   |

![[embedded-note]]
"#;

    let parsed = nexus_storage::parse_markdown(md).unwrap();

    // Frontmatter
    assert!(!parsed.frontmatter.is_empty());

    // Blocks
    assert!(parsed.blocks.iter().any(|b| b.block_type == "heading"));
    assert!(parsed.blocks.iter().any(|b| b.block_type == "paragraph"));
    assert!(parsed.blocks.iter().any(|b| b.block_type == "codeblock"));
    assert!(parsed.blocks.iter().any(|b| b.block_type == "table"));

    // Links
    assert!(parsed.links.iter().any(|l| l.link_type == "wikilink"));
    assert!(parsed.links.iter().any(|l| l.link_type == "markdown"));
    assert!(parsed.links.iter().any(|l| l.link_type == "embed"));

    // Tags
    assert!(parsed.tags.iter().any(|t| t.name == "complex" && t.source == "frontmatter"));
}

#[test]
fn storage_event_variants_construct() {
    let _created = StorageEvent::FileCreated {
        path: "notes/a.md".to_string(),
        content_hash: "abc".to_string(),
    };
    let _modified = StorageEvent::FileModified {
        path: "notes/a.md".to_string(),
        content_hash: "def".to_string(),
    };
    let _deleted = StorageEvent::FileDeleted {
        path: "notes/a.md".to_string(),
    };
    let _renamed = StorageEvent::FileRenamed {
        from: "notes/old.md".to_string(),
        to: "notes/new.md".to_string(),
        content_hash: "abc".to_string(),
    };
}

#[test]
fn reconcile_picks_up_external_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(tmp.path()).unwrap();

    // Write a file through the engine
    engine.write_file("notes/a.md", b"# A").unwrap();

    // Write a file directly to disk (simulating external edit)
    std::fs::write(tmp.path().join("notes/external.md"), "# External").unwrap();

    // Rebuild picks it up
    let stats = engine.rebuild_index().unwrap();
    assert_eq!(stats.files_processed, 2);
    assert!(engine.file_exists("notes/external.md").unwrap());
}
```

- [ ] **Step 2: Run smoke test**

Run: `cargo nextest run -p nexus-storage --test prd-03-smoke`
Expected: all smoke tests PASS.

- [ ] **Step 3: Run full workspace tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass (125 existing + all new storage tests).

- [ ] **Step 4: Run clippy on entire workspace**

Run: `cargo clippy --workspace -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/tests/
git commit -m "test(storage): add PRD 03 smoke test covering public API surface and integration"
```

---

## Summary

30 tasks across 10 phases produce:
- `nexus-storage` crate with 9 source modules
- `StorageEngine` facade composing all subsystems
- Exclusive forge lock via `flock()` (RAII `ForgeLock` guard, released on drop)
- Full SQLite index (5 tables + FTS5 + transaction-wrapped migration runner)
- Markdown parser with frontmatter, blocks, links, tags
- Tantivy full-text search with BM25 (schema includes `mtime` field)
- File watcher with ignore patterns, rename detection, and git batch mode
- Reconciliation engine with hash-based delta
- Atomic writes with temp-fsync-rename
- `watch_changes()` method exposing watcher event receiver
- Comprehensive unit tests per module + integration smoke test
