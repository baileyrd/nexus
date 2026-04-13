//! Filename validation helpers.

use crate::error::UtilError;

/// Maximum byte length for a single filename component (POSIX).
pub const MAX_FILENAME_BYTES: usize = 255;

/// Maximum byte length for a full path (Windows compatibility).
pub const MAX_PATH_BYTES: usize = 260;

/// Characters forbidden in filenames on any supported platform.
const FORBIDDEN_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Windows-reserved device names (case-insensitive).
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate a single filename component (not a full path).
///
/// Rejects names that:
/// - Contain forbidden characters (`/ \ : * ? " < > |`)
/// - Are Windows reserved device names (`CON`, `NUL`, `COM1`, …)
/// - Are `.` or `..`
/// - Exceed [`MAX_FILENAME_BYTES`] bytes
///
/// # Errors
///
/// Returns [`UtilError::InvalidFilename`] if the name is rejected.
pub fn validate_filename(name: &str) -> Result<(), UtilError> {
    if name.is_empty() {
        return Err(UtilError::InvalidFilename {
            name: name.to_string(),
            reason: "filename cannot be empty".to_string(),
        });
    }

    if name == "." || name == ".." {
        return Err(UtilError::InvalidFilename {
            name: name.to_string(),
            reason: "reserved directory reference".to_string(),
        });
    }

    if let Some(fc) = name.chars().find(|c| FORBIDDEN_CHARS.contains(c)) {
        return Err(UtilError::InvalidFilename {
            name: name.to_string(),
            reason: format!("contains forbidden character '{fc}'"),
        });
    }

    // Strip extension for reserved-name check.
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_uppercase();
    if RESERVED_NAMES.contains(&upper.as_str()) {
        return Err(UtilError::InvalidFilename {
            name: name.to_string(),
            reason: format!("'{stem}' is a reserved device name"),
        });
    }

    if name.len() > MAX_FILENAME_BYTES {
        return Err(UtilError::InvalidFilename {
            name: name[..32].to_string(),
            reason: format!("exceeds maximum filename length ({MAX_FILENAME_BYTES} bytes)"),
        });
    }

    Ok(())
}

/// Validate a full file path string.
///
/// # Errors
///
/// Returns [`UtilError::PathTooLong`] if the path exceeds [`MAX_PATH_BYTES`].
pub fn validate_path(path: &str) -> Result<(), UtilError> {
    if path.len() > MAX_PATH_BYTES {
        return Err(UtilError::PathTooLong {
            path: path[..64].to_string(),
            max: MAX_PATH_BYTES,
        });
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_filename() {
        assert!(validate_filename("my-note.md").is_ok());
        assert!(validate_filename("README").is_ok());
        assert!(validate_filename("snake_case.txt").is_ok());
        assert!(validate_filename("123.json").is_ok());
    }

    #[test]
    fn empty_is_invalid() {
        assert!(validate_filename("").is_err());
    }

    #[test]
    fn dot_and_dotdot_are_invalid() {
        assert!(validate_filename(".").is_err());
        assert!(validate_filename("..").is_err());
    }

    #[test]
    fn forbidden_chars_rejected() {
        for c in ['/', '\\', ':', '*', '?', '"', '<', '>', '|'] {
            let name = format!("file{c}name");
            assert!(validate_filename(&name).is_err(), "should reject '{c}'");
        }
    }

    #[test]
    fn reserved_names_rejected() {
        assert!(validate_filename("CON").is_err());
        assert!(validate_filename("con").is_err()); // case-insensitive
        assert!(validate_filename("NUL.txt").is_err());
        assert!(validate_filename("COM1").is_err());
        assert!(validate_filename("LPT9").is_err());
    }

    #[test]
    fn normal_name_with_same_prefix_as_reserved_is_ok() {
        // "CONSOLE" starts with "CON" but is not reserved.
        assert!(validate_filename("CONSOLE.md").is_ok());
    }

    #[test]
    fn too_long_filename_rejected() {
        let long = "a".repeat(256);
        assert!(validate_filename(&long).is_err());
    }

    #[test]
    fn path_too_long_rejected() {
        let long = "a".repeat(261);
        assert!(validate_path(&long).is_err());
    }

    #[test]
    fn short_path_ok() {
        assert!(validate_path("notes/my-file.md").is_ok());
    }
}
