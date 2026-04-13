//! Format version type with semver-style compatibility checks.
//!
//! Formats declare versions as `"1.0"` or `"1.0.0"` strings.
//! Compatibility rule: same major, `self >= required`.

use std::fmt;
use std::str::FromStr;

use crate::error::VersionError;

/// A three-part semantic version for file formats.
///
/// Parse from `"1.0"` (patch defaults to `0`) or `"1.0.0"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FormatVersion(pub u32, pub u32, pub u32);

impl FormatVersion {
    /// Parse a version string (`"1.0"` or `"1.0.0"`).
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::ParseFailed`] if the string is not a valid
    /// one- or two-dot decimal version.
    pub fn parse(s: &str) -> Result<Self, VersionError> {
        let parts: Vec<&str> = s.split('.').collect();
        match parts.as_slice() {
            [maj, min] => {
                let major = maj.parse::<u32>().map_err(|_| VersionError::ParseFailed { input: s.to_string() })?;
                let minor = min.parse::<u32>().map_err(|_| VersionError::ParseFailed { input: s.to_string() })?;
                Ok(Self(major, minor, 0))
            }
            [maj, min, pat] => {
                let major = maj.parse::<u32>().map_err(|_| VersionError::ParseFailed { input: s.to_string() })?;
                let minor = min.parse::<u32>().map_err(|_| VersionError::ParseFailed { input: s.to_string() })?;
                let patch = pat.parse::<u32>().map_err(|_| VersionError::ParseFailed { input: s.to_string() })?;
                Ok(Self(major, minor, patch))
            }
            _ => Err(VersionError::ParseFailed { input: s.to_string() }),
        }
    }

    /// Returns `true` if `self` is compatible with `required`.
    ///
    /// Compatibility requires the same major version AND `self >= required`.
    #[must_use]
    pub fn is_compatible_with(&self, required: &Self) -> bool {
        self.0 == required.0 && *self >= *required
    }

    /// Major version component.
    #[must_use]
    pub fn major(&self) -> u32 { self.0 }

    /// Minor version component.
    #[must_use]
    pub fn minor(&self) -> u32 { self.1 }

    /// Patch version component.
    #[must_use]
    pub fn patch(&self) -> u32 { self.2 }
}

impl fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl FromStr for FormatVersion {
    type Err = VersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_two_part() {
        let v = FormatVersion::parse("1.0").unwrap();
        assert_eq!(v, FormatVersion(1, 0, 0));
    }

    #[test]
    fn parse_three_part() {
        let v = FormatVersion::parse("2.3.4").unwrap();
        assert_eq!(v, FormatVersion(2, 3, 4));
    }

    #[test]
    fn parse_invalid_returns_error() {
        assert!(FormatVersion::parse("abc").is_err());
        assert!(FormatVersion::parse("1").is_err());
        assert!(FormatVersion::parse("1.2.3.4").is_err());
        assert!(FormatVersion::parse("").is_err());
    }

    #[test]
    fn display_always_three_parts() {
        let v = FormatVersion(1, 0, 0);
        assert_eq!(v.to_string(), "1.0.0");
    }

    #[test]
    fn from_str_roundtrip() {
        let s = "2.1.3";
        let v: FormatVersion = s.parse().unwrap();
        assert_eq!(v.to_string(), s);
    }

    #[test]
    fn compatibility_same_major_newer_minor() {
        let reader = FormatVersion(1, 2, 0);
        let file   = FormatVersion(1, 0, 0);
        // reader can read file (reader >= file, same major)
        assert!(reader.is_compatible_with(&file));
    }

    #[test]
    fn compatibility_same_version() {
        let v = FormatVersion(1, 0, 0);
        assert!(v.is_compatible_with(&v));
    }

    #[test]
    fn compatibility_different_major_fails() {
        let reader = FormatVersion(2, 0, 0);
        let file   = FormatVersion(1, 0, 0);
        assert!(!reader.is_compatible_with(&file));
    }

    #[test]
    fn compatibility_older_reader_fails() {
        let reader = FormatVersion(1, 0, 0);
        let file   = FormatVersion(1, 2, 0);
        assert!(!reader.is_compatible_with(&file));
    }

    #[test]
    fn ordering_works() {
        assert!(FormatVersion(1, 1, 0) > FormatVersion(1, 0, 9));
        assert!(FormatVersion(2, 0, 0) > FormatVersion(1, 9, 9));
    }
}
