//! Attachment naming and content-hashing utilities.

use sha2::{Digest, Sha256};

/// Compute the SHA-256 hex digest of `data`.
///
/// Returns a 64-character lowercase hex string.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// Generate a deterministic attachment filename.
///
/// Format: `{file_type}-{timestamp_ms}-{hash8}.{ext}`
///
/// - `file_type` — category string, e.g. `"image"`, `"video"`, `"document"`.
/// - `timestamp_ms` — Unix timestamp in milliseconds (from file creation time).
/// - `content` — raw file bytes used to derive the short hash suffix.
/// - `ext` — file extension without the leading dot (e.g. `"png"`, `"pdf"`).
///
/// The first 8 hex characters of the SHA-256 digest of `content` are appended
/// to avoid collisions when multiple files share the same type and timestamp.
#[must_use]
pub fn attachment_name(file_type: &str, timestamp_ms: u64, content: &[u8], ext: &str) -> String {
    let hash8 = &sha256_hex(content)[..8];
    format!("{file_type}-{timestamp_ms}-{hash8}.{ext}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_is_64_chars() {
        let h = sha256_hex(b"hello");
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn sha256_hex_is_lowercase_hex() {
        let h = sha256_hex(b"hello");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "not all hex: {h}");
        assert_eq!(h, h.to_lowercase());
    }

    #[test]
    fn sha256_hex_is_deterministic() {
        assert_eq!(sha256_hex(b"hello"), sha256_hex(b"hello"));
    }

    #[test]
    fn sha256_hex_differs_for_different_input() {
        assert_ne!(sha256_hex(b"hello"), sha256_hex(b"world"));
    }

    #[test]
    fn sha256_hex_known_value() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let h = sha256_hex(b"");
        assert!(h.starts_with("e3b0c442"));
    }

    #[test]
    fn attachment_name_format() {
        let name = attachment_name("image", 1_000_000, b"data", "png");
        // Should be: "image-1000000-{8 hex chars}.png"
        let parts: Vec<&str> = name.splitn(3, '-').collect();
        assert_eq!(parts[0], "image");
        assert_eq!(parts[1], "1000000");
        assert!(std::path::Path::new(parts[2])
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png")));
    }

    #[test]
    fn attachment_name_is_deterministic() {
        let n1 = attachment_name("doc", 999, b"content", "pdf");
        let n2 = attachment_name("doc", 999, b"content", "pdf");
        assert_eq!(n1, n2);
    }

    #[test]
    fn attachment_name_differs_for_different_content() {
        let n1 = attachment_name("image", 0, b"aaa", "png");
        let n2 = attachment_name("image", 0, b"bbb", "png");
        assert_ne!(n1, n2);
    }

    #[test]
    fn attachment_name_hash_is_8_chars() {
        let name = attachment_name("video", 12345, b"test", "mp4");
        // "video-12345-{hash8}.mp4" → split on last `.`
        let stem = name.rsplit_once('.').unwrap().0;
        let hash_part = stem.rsplit('-').next().unwrap();
        assert_eq!(hash_part.len(), 8);
        assert!(hash_part.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
