//! Forge-root path validation and symlink enforcement.
//!
//! All plugin file operations pass through `ForgePathValidator::validate`
//! to ensure the resolved path stays within the forge root. Symlinks are
//! followed only if the final canonical path is still inside the root.

use std::path::{Path, PathBuf};

use crate::SecurityError;

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
    /// # Errors
    /// Returns `SecurityError::InvalidPath` if `forge_root` does not exist
    /// or cannot be canonicalized.
    pub fn new(forge_root: &Path) -> Result<Self, SecurityError> {
        let canonical = forge_root.canonicalize().map_err(|e| {
            SecurityError::InvalidPath(format!(
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
    /// # Errors
    /// - `SecurityError::InvalidPath` for null bytes or non-existent paths.
    /// - `SecurityError::PathTraversal` if the resolved path escapes the root.
    pub fn validate(&self, requested: &Path) -> Result<PathBuf, SecurityError> {
        let requested_str = requested.to_string_lossy();

        // 1. Reject null bytes
        if requested_str.contains('\0') {
            return Err(SecurityError::InvalidPath(
                "path contains null byte".to_string(),
            ));
        }

        // 2. Normalize path components
        let normalized = self.normalize(requested)?;

        // 3. Join with forge root
        let joined = self.forge_root.join(&normalized);

        // 4. Canonicalize (resolves symlinks)
        let canonical = joined.canonicalize().map_err(|e| {
            SecurityError::InvalidPath(format!(
                "path '{}' cannot be resolved: {e}",
                requested.display()
            ))
        })?;

        // 5. Verify within forge root
        if !canonical.starts_with(&self.forge_root) {
            return Err(SecurityError::PathTraversal(canonical));
        }

        Ok(canonical)
    }

    /// Normalize path components: collapse `.`, reject `..` that escapes root.
    /// Strips leading `/` so absolute paths are treated as relative.
    fn normalize(&self, path: &Path) -> Result<PathBuf, SecurityError> {
        let mut components = Vec::new();

        for component in path.components() {
            match component {
                std::path::Component::Normal(c) => {
                    components.push(c);
                }
                std::path::Component::ParentDir => {
                    if components.is_empty() {
                        return Err(SecurityError::PathTraversal(path.to_path_buf()));
                    }
                    components.pop();
                }
                std::path::Component::CurDir | std::path::Component::RootDir => {
                    // Skip `.` and leading `/`
                }
                std::path::Component::Prefix(_) => {
                    // Windows prefix — treat as no-op for forge-relative paths
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
        assert!(matches!(err, SecurityError::InvalidPath(_)));
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
        assert!(matches!(err, SecurityError::PathTraversal(_)));
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
        assert!(matches!(err, SecurityError::InvalidPath(_)));
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
        assert!(matches!(err, SecurityError::PathTraversal(_)));
    }

    #[test]
    fn nonexistent_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("does_not_exist.txt"));
        assert!(result.is_err());
        // canonicalize fails on nonexistent paths
        assert!(matches!(result.unwrap_err(), SecurityError::InvalidPath(_)));
    }
}
