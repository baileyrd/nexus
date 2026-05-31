//! Filename utilities for Notion exports.
//!
//! Notion suffixes every page filename with a 32-character hex UUID
//! (no separators), separated from the title by a single space:
//!
//!   `Page Title abc123def456abc123def456abc12345.md`
//!
//! Folders that hold a page's children get the same suffix. Internal
//! mention links use the URL-encoded full filename.
//!
//! These helpers strip the UUID for display and rebuild a clean relative
//! path that preserves the directory structure.

use std::path::PathBuf;

/// 32 lowercase hex characters.
const UUID_LEN: usize = 32;

/// Strip the trailing Notion UUID from a filename or directory name. Returns
/// `(cleaned, uuid)`. Pure function — no I/O. Preserves any extension.
///
/// Examples:
///
/// - `"Page Title abc…32hex.md"` → `("Page Title.md", Some("abc…32hex"))`
/// - `"Page Title abc…32hex"`    → `("Page Title", Some("abc…32hex"))`
/// - `"Plain.md"`                → `("Plain.md", None)`
#[must_use]
pub fn strip_notion_uuid(name: &str) -> (String, Option<String>) {
    // Split off extension first so we can match on the stem.
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], Some(&name[i..])),
        _ => (name, None),
    };

    // The UUID is the last token if it's exactly 32 lowercase hex chars and
    // preceded by a single space.
    let bytes = stem.as_bytes();
    if bytes.len() < UUID_LEN + 1 {
        return (name.to_string(), None);
    }
    let uuid_start = bytes.len() - UUID_LEN;
    if bytes[uuid_start - 1] != b' ' {
        return (name.to_string(), None);
    }
    let candidate = &stem[uuid_start..];
    if !is_hex_lower(candidate) {
        return (name.to_string(), None);
    }
    let cleaned_stem = &stem[..uuid_start - 1];
    let mut cleaned = cleaned_stem.to_string();
    if let Some(ext) = ext {
        cleaned.push_str(ext);
    }
    (cleaned, Some(candidate.to_string()))
}

/// Extract just the UUID, if present.
#[must_use]
pub fn extract_uuid(name: &str) -> Option<String> {
    strip_notion_uuid(name).1
}

/// Clean every component of a relative path. Each segment passes through
/// [`strip_notion_uuid`] independently so that nested page folders are also
/// renamed.
#[must_use]
pub fn clean_path(path: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        let (cleaned, _) = strip_notion_uuid(part);
        out.push(cleaned);
    }
    out
}

fn is_hex_lower(s: &str) -> bool {
    s.len() == UUID_LEN && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UID: &str = "abc123def456abc123def456abc12345";

    #[test]
    fn strips_uuid_from_md_filename() {
        let (name, uid) = strip_notion_uuid(&format!("Page Title {UID}.md"));
        assert_eq!(name, "Page Title.md");
        assert_eq!(uid.as_deref(), Some(UID));
    }

    #[test]
    fn strips_uuid_from_directory_name() {
        let (name, uid) = strip_notion_uuid(&format!("Page Title {UID}"));
        assert_eq!(name, "Page Title");
        assert_eq!(uid.as_deref(), Some(UID));
    }

    #[test]
    fn leaves_filenames_without_uuid_unchanged() {
        let (name, uid) = strip_notion_uuid("Plain Title.md");
        assert_eq!(name, "Plain Title.md");
        assert!(uid.is_none());
    }

    #[test]
    fn rejects_non_hex_suffix() {
        let (name, uid) =
            strip_notion_uuid(&format!("Title not_hex_ZZZZZZZZZZZZZZZZZZZZZZZZZZZ.md"));
        assert_eq!(
            name,
            format!("Title not_hex_ZZZZZZZZZZZZZZZZZZZZZZZZZZZ.md")
        );
        assert!(uid.is_none());
    }

    #[test]
    fn rejects_uppercase_hex() {
        // Notion always lowercases its UUID suffixes; uppercase shouldn't match.
        let (name, uid) = strip_notion_uuid(&format!("Title ABCDEF1234567890ABCDEF1234567890.md"));
        assert!(uid.is_none(), "must not strip uppercase suffix");
        assert!(name.contains("ABCDEF"));
    }

    #[test]
    fn rejects_short_suffix() {
        // 31 chars — one short.
        let (name, uid) = strip_notion_uuid("Title abc123def456abc123def456abc1234.md");
        assert_eq!(name, "Title abc123def456abc123def456abc1234.md");
        assert!(uid.is_none());
    }

    #[test]
    fn cleans_nested_path() {
        let p = clean_path(&format!("Parent {UID}/Child {UID}.md"));
        assert_eq!(p, PathBuf::from("Parent/Child.md"));
    }

    #[test]
    fn clean_path_handles_csv() {
        let p = clean_path(&format!("Notion DB {UID}.csv"));
        assert_eq!(p, PathBuf::from("Notion DB.csv"));
    }
}
