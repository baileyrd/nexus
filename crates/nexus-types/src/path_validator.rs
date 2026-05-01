//! Forge-root path validation and symlink enforcement.
//!
//! All plugin file operations pass through `ForgePathValidator::validate`
//! (for read) or `validate_for_write` (for write) to ensure the resolved
//! path stays within the forge root. Symlinks are followed only if the
//! final canonical path is still inside the root.
//!
//! This module lives in `nexus-types` (the leaf crate) so that
//! `nexus-kernel` and `nexus-plugins` can both reach it without creating
//! a cycle with the higher-level `nexus-security` crate that owns the
//! policy-aware error types. `nexus-security` re-exports the validator
//! and provides `From<PathValidationError>` conversions so its callers
//! see a unified `SecurityError`.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors returned by [`ForgePathValidator`].
///
/// This is a minimal, dependency-free error type so that low-level crates
/// can use the validator without pulling in the broader security policy
/// surface.
#[derive(Debug, Error)]
pub enum PathValidationError {
    /// Path traversal attempt detected — resolved path escapes forge root.
    #[error("path traversal denied: {} escapes forge root", .0.display())]
    PathTraversal(PathBuf),

    /// Path contains invalid characters (null bytes, etc.) or could not
    /// be resolved.
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

/// Validates that file paths resolve within a forge root directory.
///
/// Constructed once per forge, canonicalizes the root at creation time.
/// Thread-safe (immutable after construction).
#[derive(Debug, Clone)]
pub struct ForgePathValidator {
    forge_root: PathBuf,
}

impl ForgePathValidator {
    /// Create a new validator. Canonicalizes `forge_root` immediately.
    ///
    /// `forge_root` **must exist on disk** at construction time —
    /// `Path::canonicalize` requires a real path to resolve. Callers
    /// that want to validate paths against a not-yet-created forge
    /// root should create the directory first, or use the
    /// component-only [`crate::paths::resolve_within`] helper, which
    /// performs no I/O.
    ///
    /// # Errors
    /// Returns `PathValidationError::InvalidPath` if `forge_root` does not
    /// exist or cannot be canonicalized.
    pub fn new(forge_root: &Path) -> Result<Self, PathValidationError> {
        let canonical = forge_root.canonicalize().map_err(|e| {
            PathValidationError::InvalidPath(format!(
                "forge root '{}' cannot be canonicalized: {e}",
                forge_root.display()
            ))
        })?;
        Ok(Self {
            forge_root: canonical,
        })
    }

    /// The canonical forge root path.
    #[must_use]
    pub fn forge_root(&self) -> &Path {
        &self.forge_root
    }

    /// Validate a requested path. Returns the canonical resolved path if it
    /// is within the forge root.
    ///
    /// # Behavior
    /// 1. Rejects paths containing null bytes.
    /// 2. Strips leading `/` (absolute paths treated as relative to forge root).
    /// 3. Normalizes `.` and `..` components, rejecting `..` past the root.
    /// 4. Joins with forge root and canonicalizes (follows symlinks).
    /// 5. Verifies the canonical path starts with the canonical forge root.
    ///
    /// This is the **permissive** validator: leading `/` is dropped,
    /// `.` is dropped, and `..` is allowed as long as it doesn't walk
    /// past the forge root. For strict, component-only validation
    /// (no I/O, no normalization), use
    /// [`crate::paths::resolve_within`] — see the table on that
    /// helper for the full comparison.
    ///
    /// # Errors
    /// - `PathValidationError::InvalidPath` for null bytes or non-existent paths.
    /// - `PathValidationError::PathTraversal` if the resolved path escapes the root.
    pub fn validate(&self, requested: &Path) -> Result<PathBuf, PathValidationError> {
        let requested_str = requested.to_string_lossy();

        // 1. Reject null bytes
        if requested_str.contains('\0') {
            return Err(PathValidationError::InvalidPath(
                "path contains null byte".to_string(),
            ));
        }

        // 2. Normalize path components
        let normalized = Self::normalize(requested)?;

        // 3. Join with forge root
        let joined = self.forge_root.join(&normalized);

        // 4. Canonicalize (resolves symlinks)
        let canonical = joined.canonicalize().map_err(|e| {
            PathValidationError::InvalidPath(format!(
                "path '{}' cannot be resolved: {e}",
                requested.display()
            ))
        })?;

        // 5. Verify within forge root
        if !canonical.starts_with(&self.forge_root) {
            return Err(PathValidationError::PathTraversal(canonical));
        }

        Ok(canonical)
    }

    /// Validate a requested path for a **write** operation. Returns a
    /// canonical target path whose parent is guaranteed to be inside the
    /// forge root **at validation time**.
    ///
    /// Unlike [`validate`](Self::validate), the target file does not have
    /// to exist; only the deepest existing ancestor is canonicalized, then
    /// the canonical ancestor is rebuilt with the remaining normalized
    /// components. This closes the TOCTOU race where canonicalizing the
    /// parent and then writing to the non-canonical path could be steered
    /// through a symlink swap between the two operations.
    ///
    /// # Residual TOCTOU
    /// The "parent guaranteed inside the forge root" invariant only holds
    /// at the moment this function returns. The non-existing tail
    /// components — the part of the path that doesn't exist on disk yet
    /// — can be swapped for symlinks during the gap between validation
    /// and the actual `open(2)`/`write` syscall. A truly TOCTOU-free
    /// write requires `openat2(RESOLVE_BENEATH)` on Linux (or the
    /// equivalent on other platforms); kernel callers that need that
    /// guarantee should layer it on top of the path returned here. See
    /// issue #82.
    ///
    /// # Behavior
    /// 1. Rejects paths containing null bytes.
    /// 2. Normalizes `.` and `..`, rejecting `..` past the root.
    /// 3. Walks up the joined path to the deepest existing ancestor.
    /// 4. Canonicalizes that ancestor (follows symlinks) and verifies it
    ///    starts with the canonical forge root.
    /// 5. Rejoins the remaining non-existing tail onto the canonical
    ///    ancestor and returns the result.
    ///
    /// # Errors
    /// - `PathValidationError::InvalidPath` for null bytes or when no existing
    ///   ancestor can be canonicalized.
    /// - `PathValidationError::PathTraversal` if the canonical ancestor escapes
    ///   the forge root.
    pub fn validate_for_write(&self, requested: &Path) -> Result<PathBuf, PathValidationError> {
        let requested_str = requested.to_string_lossy();
        if requested_str.contains('\0') {
            return Err(PathValidationError::InvalidPath(
                "path contains null byte".to_string(),
            ));
        }

        let normalized = Self::normalize(requested)?;
        let joined = self.forge_root.join(&normalized);

        // Walk up until we find an existing ancestor.
        let mut ancestor = joined.as_path();
        let tail = loop {
            if ancestor.exists() {
                let tail = joined
                    .strip_prefix(ancestor)
                    .map_err(|e| PathValidationError::InvalidPath(e.to_string()))?
                    .to_path_buf();
                break tail;
            }
            match ancestor.parent() {
                Some(p) => ancestor = p,
                None => {
                    return Err(PathValidationError::InvalidPath(format!(
                        "no existing ancestor for '{}'",
                        requested.display()
                    )));
                }
            }
        };

        let canonical_ancestor = ancestor.canonicalize().map_err(|e| {
            PathValidationError::InvalidPath(format!(
                "ancestor '{}' cannot be resolved: {e}",
                ancestor.display()
            ))
        })?;

        if !canonical_ancestor.starts_with(&self.forge_root) {
            return Err(PathValidationError::PathTraversal(canonical_ancestor));
        }

        Ok(canonical_ancestor.join(tail))
    }

    /// Normalize path components: collapse `.`, reject `..` that escapes root.
    /// Strips leading `/` so absolute paths are treated as relative.
    fn normalize(path: &Path) -> Result<PathBuf, PathValidationError> {
        let mut components = Vec::new();

        for component in path.components() {
            match component {
                std::path::Component::Normal(c) => {
                    components.push(c);
                }
                std::path::Component::ParentDir => {
                    if components.is_empty() {
                        return Err(PathValidationError::PathTraversal(path.to_path_buf()));
                    }
                    components.pop();
                }
                std::path::Component::CurDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_) => {
                    // Skip `.`, leading `/`, and Windows drive prefixes
                }
            }
        }

        if components.is_empty() {
            Ok(PathBuf::from("."))
        } else {
            Ok(components.iter().collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn new_fails_on_nonexistent_root() {
        let result = ForgePathValidator::new(Path::new("/nonexistent/path/abc123"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, PathValidationError::InvalidPath(_)));
    }

    #[test]
    fn forge_root_returns_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();
        assert_eq!(validator.forge_root(), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn valid_file_resolves() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("test.txt"));
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(resolved.starts_with(validator.forge_root()));
    }

    #[test]
    fn valid_nested_file_resolves() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub/dir")).unwrap();
        fs::write(dir.path().join("sub/dir/file.md"), "content").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("sub/dir/file.md"));
        assert!(result.is_ok());
    }

    #[test]
    fn dot_dot_traversal_past_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("../../../etc/passwd"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, PathValidationError::PathTraversal(_)));
    }

    #[test]
    fn dot_dot_within_root_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("a/test.txt"), "hello").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        // a/b/../test.txt normalizes to a/test.txt — still in root
        let result = validator.validate(Path::new("a/b/../test.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn null_byte_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("test\0.txt"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, PathValidationError::InvalidPath(_)));
    }

    #[test]
    fn absolute_path_treated_as_relative() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("file.txt"), "hello").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        // /file.txt should be treated as file.txt relative to forge root
        let result = validator.validate(Path::new("/file.txt"));
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with(validator.forge_root()));
    }

    #[test]
    fn empty_path_resolves_to_forge_root() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new(""));
        // Empty path joined with root = root itself, which is valid
        assert!(result.is_ok());
    }

    #[test]
    fn dot_resolves_to_forge_root() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("."));
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_within_root_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.txt"), "hello").unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("real.txt"),
            dir.path().join("link.txt"),
        )
        .unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("link.txt"));
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_outside_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();

        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("escape.txt"),
        )
        .unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("escape.txt"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, PathValidationError::PathTraversal(_)));
    }

    #[test]
    fn validate_for_write_accepts_new_file_in_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();
        let result = validator.validate_for_write(Path::new("new.txt"));
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with(validator.forge_root()));
    }

    #[test]
    fn validate_for_write_accepts_nested_new_file_under_existing_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();
        let result = validator.validate_for_write(Path::new("sub/does/not/exist.txt"));
        assert!(result.is_ok());
        let canon = result.unwrap();
        assert!(canon.starts_with(validator.forge_root()));
        assert!(canon.ends_with("sub/does/not/exist.txt"));
    }

    #[test]
    fn validate_for_write_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();
        let result = validator.validate_for_write(Path::new("../escape.txt"));
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validate_for_write_rejects_symlinked_parent() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();
        let result = validator.validate_for_write(Path::new("escape/victim.txt"));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PathValidationError::PathTraversal(_)
        ));
    }

    #[test]
    fn nonexistent_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("does_not_exist.txt"));
        assert!(result.is_err());
        // canonicalize fails on nonexistent paths
        assert!(matches!(
            result.unwrap_err(),
            PathValidationError::InvalidPath(_)
        ));
    }
}
