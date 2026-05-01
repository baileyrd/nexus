//! Forge-relative path confinement.
//!
//! Shared between any crate that needs to resolve a caller-supplied
//! relative path under a forge root without trusting the caller to
//! have already validated it. Used by `nexus-storage` and
//! `nexus-editor`; see also [`nexus_kernel::KernelPluginContext`]'s
//! `confine_path` (which takes `Path` instead of a `&str` relpath and
//! also resolves symlinks).
//!
//! This helper deliberately does *not* canonicalize the resolved path
//! — callers that create non-existent files (atomic-write, create,
//! rename-target) must be able to resolve paths that don't exist yet.
//! Callers that need symlink resolution should canonicalize the
//! result themselves.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Error returned by [`resolve_within`] when a relative path is
/// structurally unsafe.
#[derive(Debug, Error)]
pub enum PathError {
    /// The relative path contains a component that would escape the
    /// root: a root dir, a parent (`..`), a current-dir (`.`), or a
    /// Windows drive prefix. The offending relpath is included for
    /// diagnostics.
    #[error("invalid relpath: {0}")]
    Invalid(String),
}

/// Resolve `relpath` under `root`, rejecting anything that escapes
/// the root via path-component inspection alone.
///
/// Accepts only [`Component::Normal`] components. Absolute paths,
/// `..`, `.`, root dirs, and Windows drive prefixes are all rejected.
/// An empty `relpath` resolves to `root` itself (callers that want to
/// reject empty relpaths should check before calling).
///
/// # When to use this vs [`crate::ForgePathValidator::validate`]
///
/// Both helpers exist and have **different semantics by design** —
/// not every caller wants the same thing:
///
/// | | `resolve_within` (this fn) | [`crate::ForgePathValidator::validate`] |
/// |---|---|---|
/// | Leading `/` | rejected | silently stripped |
/// | `.` component | rejected | silently dropped |
/// | `..` component | rejected unconditionally | accepted iff stays in root |
/// | Symlink resolution | none (path joined verbatim) | follows symlinks via `canonicalize` |
/// | Existence requirement | none | path must exist |
///
/// Use **`resolve_within`** when the caller has a forge-relative
/// path that must be a strictly normalised, leaf-only relpath
/// (storage IPC handlers, atomic-write target paths, places where
/// `..`-traversal is always wrong). Use
/// [`crate::ForgePathValidator::validate`] when the caller may have
/// a user-typed path that's been through
/// a UI, can be absolute, can use `..` to walk in-root, and must
/// resolve symlinks before being trusted (kernel context's
/// `confine_path` for `read_file`/`write_file`).
///
/// # Errors
///
/// Returns [`PathError::Invalid`] if `relpath` contains any
/// non-`Normal` component.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use nexus_types::paths::resolve_within;
///
/// let root = Path::new("/forge");
/// assert_eq!(resolve_within(root, "notes/a.md").unwrap(), Path::new("/forge/notes/a.md"));
/// assert!(resolve_within(root, "../etc/passwd").is_err());
/// assert!(resolve_within(root, "/etc/passwd").is_err());
/// ```
pub fn resolve_within(root: &Path, relpath: &str) -> Result<PathBuf, PathError> {
    if relpath.is_empty() {
        return Ok(root.to_path_buf());
    }
    let rel = Path::new(relpath);
    for c in rel.components() {
        match c {
            Component::Normal(_) => {}
            _ => return Err(PathError::Invalid(relpath.to_string())),
        }
    }
    Ok(root.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_relpath_yields_root() {
        let root = Path::new("/forge");
        assert_eq!(resolve_within(root, "").unwrap(), root);
    }

    #[test]
    fn normal_relpath_joins_under_root() {
        let root = Path::new("/forge");
        assert_eq!(
            resolve_within(root, "a/b/c.md").unwrap(),
            root.join("a").join("b").join("c.md")
        );
    }

    #[test]
    fn parent_traversal_is_rejected() {
        let root = Path::new("/forge");
        assert!(resolve_within(root, "../outside").is_err());
        assert!(resolve_within(root, "a/../../outside").is_err());
    }

    #[test]
    fn current_dir_component_is_rejected() {
        let root = Path::new("/forge");
        // `.` is not a Normal component.
        assert!(resolve_within(root, "./a.md").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn absolute_path_is_rejected_unix() {
        let root = Path::new("/forge");
        assert!(resolve_within(root, "/etc/passwd").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn absolute_path_is_rejected_windows() {
        let root = Path::new(r"C:\forge");
        assert!(resolve_within(root, r"C:\Windows\System32\cmd.exe").is_err());
        assert!(resolve_within(root, r"\Windows\System32").is_err());
    }
}
