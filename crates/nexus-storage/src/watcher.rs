//! File watcher for the forge storage directory.
//!
//! Wraps `notify-debouncer-mini` to emit [`StorageEvent`]s for changes inside
//! `notes/` and `attachments/` subdirectories of a forge root.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind, new_debouncer};

use crate::StorageError;

// ── Public types ──────────────────────────────────────────────────────────────

/// An event emitted by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageEvent {
    /// A new file was created.
    FileCreated {
        /// Relative path from the forge root.
        path: String,
        /// SHA-256 hex digest of the file content.
        content_hash: String,
    },
    /// An existing file was modified.
    FileModified {
        /// Relative path from the forge root, or empty string for reconcile signal.
        path: String,
        /// SHA-256 hex digest of the file content, or empty string for reconcile signal.
        content_hash: String,
    },
    /// A file was deleted.
    FileDeleted {
        /// Relative path from the forge root.
        path: String,
    },
    /// A file was renamed.
    FileRenamed {
        /// Previous relative path.
        from: String,
        /// New relative path.
        to: String,
        /// SHA-256 hex digest of the new file content.
        content_hash: String,
    },
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// Check if a path should be ignored by the watcher.
///
/// Ignores paths containing `.git`, `.forge/temp`, or `node_modules`,
/// and filenames ending with `~`, `.swp`, or `.DS_Store`.
#[must_use]
pub fn should_ignore(path: &Path) -> bool {
    // Check path components for ignored directories.
    let path_str = path.to_string_lossy();
    if path_str.contains(".git")
        || path_str.contains(".forge/temp")
        || path_str.contains("node_modules")
    {
        return true;
    }

    // Check filename suffixes.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.ends_with('~')
            || Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("swp"))
            || name == ".DS_Store"
        {
            return true;
        }
    }

    false
}

/// Convert an absolute path to a relative path string from the forge root.
///
/// Returns `None` if `absolute` is not under `forge_root`.
#[must_use]
pub fn relative_path(forge_root: &Path, absolute: &Path) -> Option<String> {
    absolute
        .strip_prefix(forge_root)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

// ── Watcher ───────────────────────────────────────────────────────────────────

/// A live file watcher that emits [`StorageEvent`]s for changes in the forge.
pub struct Watcher {
    rx: mpsc::Receiver<StorageEvent>,
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl Watcher {
    /// Start watching `notes/` and `attachments/` under `forge_root`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Watcher`] if the underlying notify watcher
    /// cannot be created or a directory cannot be watched.
    pub fn start(forge_root: &Path, debounce_ms: u64) -> Result<Self, StorageError> {
        let (storage_tx, storage_rx) = mpsc::channel::<StorageEvent>();
        let (raw_tx, raw_rx) = mpsc::channel::<DebounceEventResult>();

        let duration = Duration::from_millis(debounce_ms);
        let mut debouncer = new_debouncer(duration, raw_tx)?;

        // Watch notes/ and attachments/ if they exist.
        let dirs = ["notes", "attachments"];
        for dir in &dirs {
            let dir_path = forge_root.join(dir);
            if dir_path.exists() {
                debouncer.watcher().watch(&dir_path, RecursiveMode::Recursive)?;
            }
        }

        let forge_root_owned = forge_root.to_path_buf();

        // Spawn a thread to process debounced events.
        std::thread::spawn(move || {
            process_events(raw_rx, storage_tx, forge_root_owned);
        });

        Ok(Self {
            rx: storage_rx,
            _debouncer: debouncer,
        })
    }

    /// Access the receiver to poll for [`StorageEvent`]s.
    #[must_use]
    pub fn events(&self) -> &mpsc::Receiver<StorageEvent> {
        &self.rx
    }
}

// ── Event processing thread ───────────────────────────────────────────────────

#[allow(clippy::needless_pass_by_value)]
fn process_events(
    raw_rx: mpsc::Receiver<DebounceEventResult>,
    storage_tx: mpsc::Sender<StorageEvent>,
    forge_root: PathBuf,
) {
    let mut git_batch_mode = false;

    for result in &raw_rx {
        let events = match result {
            Ok(evts) => evts,
            Err(_errs) => continue,
        };

        let lock_path = forge_root.join(".git").join("index.lock");
        let lock_exists = lock_path.exists();

        if lock_exists {
            git_batch_mode = true;
            continue;
        }

        if git_batch_mode {
            // Lock is gone — emit reconcile signal and clear state.
            git_batch_mode = false;
            let _ = storage_tx.send(StorageEvent::FileModified {
                path: String::new(),
                content_hash: String::new(),
            });
        }

        for event in events {
            // Only process Any events (AnyContinuous are intermediate states).
            if event.kind != DebouncedEventKind::Any {
                continue;
            }

            let path = &event.path;

            if should_ignore(path) {
                continue;
            }

            let Some(rel) = relative_path(&forge_root, path) else {
                continue;
            };

            if path.exists() {
                // File exists — read and hash it, emit FileModified.
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let hash = crate::parser::content_hash(&bytes);
                        let _ = storage_tx.send(StorageEvent::FileModified {
                            path: rel,
                            content_hash: hash,
                        });
                    }
                    Err(_) => {
                        // Race: file disappeared between exists() and read().
                        let _ = storage_tx.send(StorageEvent::FileDeleted { path: rel });
                    }
                }
            } else {
                let _ = storage_tx.send(StorageEvent::FileDeleted { path: rel });
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── should_ignore ──────────────────────────────────────────────────────

    #[test]
    fn should_ignore_git_directory() {
        assert!(should_ignore(Path::new("/forge/.git/config")));
        assert!(should_ignore(Path::new("/forge/notes/.git/COMMIT_EDITMSG")));
    }

    #[test]
    fn should_ignore_forge_temp() {
        assert!(should_ignore(Path::new("/forge/.forge/temp/scratch.md")));
    }

    #[test]
    fn should_ignore_node_modules() {
        assert!(should_ignore(Path::new(
            "/forge/node_modules/some-pkg/index.js"
        )));
    }

    #[test]
    fn should_ignore_swap_files() {
        assert!(should_ignore(Path::new("/forge/notes/my-note.md.swp")));
        assert!(should_ignore(Path::new("/forge/notes/.note.swp")));
    }

    #[test]
    fn should_ignore_backup_files() {
        assert!(should_ignore(Path::new("/forge/notes/my-note.md~")));
        assert!(should_ignore(Path::new("/forge/notes/old~")));
    }

    #[test]
    fn should_ignore_ds_store() {
        assert!(should_ignore(Path::new("/forge/notes/.DS_Store")));
        assert!(should_ignore(Path::new("/forge/.DS_Store")));
    }

    #[test]
    fn should_not_ignore_markdown() {
        assert!(!should_ignore(Path::new("/forge/notes/my-note.md")));
        assert!(!should_ignore(Path::new("/forge/notes/daily/2026-04-12.md")));
    }

    #[test]
    fn should_not_ignore_attachments() {
        assert!(!should_ignore(Path::new("/forge/attachments/image.png")));
        assert!(!should_ignore(Path::new("/forge/attachments/doc.pdf")));
    }

    // ── relative_path ─────────────────────────────────────────────────────

    #[test]
    fn relative_path_strips_root() {
        let root = Path::new("/forge");
        let abs = Path::new("/forge/notes/my-note.md");
        assert_eq!(
            relative_path(root, abs),
            Some("notes/my-note.md".to_string())
        );
    }

    #[test]
    fn relative_path_returns_none_for_outside() {
        let root = Path::new("/forge");
        let abs = Path::new("/other/notes/my-note.md");
        assert_eq!(relative_path(root, abs), None);
    }

    // ── StorageEvent ───────────────────────────────────────────────────────

    #[test]
    fn storage_event_variants_are_eq() {
        let a = StorageEvent::FileCreated {
            path: "notes/a.md".to_string(),
            content_hash: "abc123".to_string(),
        };
        let b = StorageEvent::FileCreated {
            path: "notes/a.md".to_string(),
            content_hash: "abc123".to_string(),
        };
        assert_eq!(a, b);

        let c = StorageEvent::FileDeleted {
            path: "notes/a.md".to_string(),
        };
        assert_ne!(a, c);

        let d = StorageEvent::FileModified {
            path: "notes/b.md".to_string(),
            content_hash: "def456".to_string(),
        };
        let e = StorageEvent::FileModified {
            path: "notes/b.md".to_string(),
            content_hash: "def456".to_string(),
        };
        assert_eq!(d, e);

        let f = StorageEvent::FileRenamed {
            from: "notes/old.md".to_string(),
            to: "notes/new.md".to_string(),
            content_hash: "ghi789".to_string(),
        };
        let g = StorageEvent::FileRenamed {
            from: "notes/old.md".to_string(),
            to: "notes/new.md".to_string(),
            content_hash: "ghi789".to_string(),
        };
        assert_eq!(f, g);
    }

    // ── Integration: watcher detects file creation ─────────────────────────

    #[test]
    fn watcher_detects_file_creation() {
        use std::time::Duration;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("temp dir");
        let forge_root = dir.path();
        let notes_dir = forge_root.join("notes");
        std::fs::create_dir_all(&notes_dir).expect("create notes dir");

        let watcher = Watcher::start(forge_root, 50).expect("start watcher");

        // Give the watcher a moment to initialise before writing.
        std::thread::sleep(Duration::from_millis(100));

        // Write a new file.
        let note_path = notes_dir.join("hello.md");
        std::fs::write(&note_path, b"# Hello\n").expect("write note");

        // Wait up to 5 seconds for the event.
        let event = watcher
            .events()
            .recv_timeout(Duration::from_secs(5))
            .expect("expected a StorageEvent within 5 seconds");

        match event {
            StorageEvent::FileModified { path, content_hash } => {
                assert!(
                    path.contains("hello.md"),
                    "expected path to contain hello.md, got: {path}"
                );
                assert!(!content_hash.is_empty(), "content_hash should not be empty");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
