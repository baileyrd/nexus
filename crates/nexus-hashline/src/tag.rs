//! Content TAG hashing.
//!
//! A TAG binds a patch to a precise file state. Nexus is **not** wire-compatible
//! with omp's `~/.omp` formats, so we are free to pick a clean scheme: the first
//! two bytes of the SHA-256 of the line-ending-normalized content, rendered as
//! four uppercase hex digits. The TAG is only a fast-path equality check — a
//! collision degrades to the 3-way-merge path, never to silent corruption,
//! because the merge re-validates against the recorded base content.

use sha2::{Digest, Sha256};

/// Number of hex digits in a TAG.
pub const TAG_HEX_LEN: usize = 4;

/// Normalize line endings to `\n` (CRLF and lone CR collapse to LF).
///
/// Hashing the normalized form means a file that differs only by line-ending
/// style still produces the same TAG, matching how editors and VCS round-trip
/// content.
#[must_use]
pub fn normalize(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

/// Compute the 4-uppercase-hex TAG of `content`.
#[must_use]
pub fn tag(content: &str) -> String {
    let digest = Sha256::digest(normalize(content).as_bytes());
    format!("{:02X}{:02X}", digest[0], digest[1])
}

/// Whether `content` hashes to `candidate` (case-insensitive hex compare).
#[must_use]
pub fn tag_matches(content: &str, candidate: &str) -> bool {
    tag(content).eq_ignore_ascii_case(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_is_four_uppercase_hex() {
        let t = tag("hello world\n");
        assert_eq!(t.len(), TAG_HEX_LEN);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(t, t.to_ascii_uppercase());
    }

    #[test]
    fn tag_is_deterministic() {
        assert_eq!(tag("fn main() {}\n"), tag("fn main() {}\n"));
    }

    #[test]
    fn line_endings_are_normalized() {
        assert_eq!(tag("a\r\nb\r\n"), tag("a\nb\n"));
        assert_eq!(tag("a\rb"), tag("a\nb"));
    }

    #[test]
    fn tag_matches_is_case_insensitive() {
        let t = tag("xyz");
        assert!(tag_matches("xyz", &t));
        assert!(tag_matches("xyz", &t.to_ascii_lowercase()));
        assert!(!tag_matches("xyz", "0000"));
    }
}
