//! Forge-root path validation — re-exports from `nexus-types`.
//!
//! The validator itself lives in the leaf `nexus-types` crate so that
//! `nexus-kernel` and `nexus-plugins` (both of which need to call it on
//! the plugin write paths) can depend on it without creating a cycle
//! with `nexus-security`. This module re-exports the type and provides
//! a [`From`] conversion from [`PathValidationError`] to [`SecurityError`]
//! so that call sites inside `nexus-security` can continue to use the
//! unified security error surface.

pub use nexus_types::{ForgePathValidator, PathValidationError};

use crate::SecurityError;

impl From<PathValidationError> for SecurityError {
    fn from(err: PathValidationError) -> Self {
        match err {
            PathValidationError::PathTraversal(p) => SecurityError::PathTraversal(p),
            PathValidationError::InvalidPath(msg) => SecurityError::InvalidPath(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn path_traversal_error_converts_to_security_error() {
        let err = PathValidationError::PathTraversal(PathBuf::from("/bad"));
        let sec: SecurityError = err.into();
        assert!(matches!(sec, SecurityError::PathTraversal(p) if p == Path::new("/bad")));
    }

    #[test]
    fn invalid_path_error_converts_to_security_error() {
        let err = PathValidationError::InvalidPath("null byte".to_string());
        let sec: SecurityError = err.into();
        assert!(matches!(sec, SecurityError::InvalidPath(msg) if msg.contains("null")));
    }
}
