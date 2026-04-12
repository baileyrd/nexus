//! File watcher for the forge storage directory.
//!
//! Wraps `notify-debouncer-mini` to emit [`StorageEvent`]s for changes inside
//! `notes/` and `attachments/` subdirectories of a forge root.

use std::path::Path;

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
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && (name.ends_with('~')
            || Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("swp"))
            || name == ".DS_Store")
    {
        return true;
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
}
