//! Persistent JSON projection of the in-memory [`SkillRegistry`].
//!
//! PRD-13 §3.1 specifies a `<root>/REGISTRY.json` index alongside the
//! `.skill.md` files. The index is a *cold-start optimization* — it
//! lets external CLIs skim the catalogue without walking the
//! directory and parsing every YAML frontmatter block. The on-disk
//! `.skill.md` files remain authoritative; this index is rebuildable
//! from them at any time.
//!
//! Atomic write semantics: serialize to a sibling `*.json.tmp` file
//! first, then rename over the destination. A serde failure leaves
//! the destination untouched and removes the tmp file.
//!
//! Path normalization: every entry's `path` is stored relative to
//! `root` with forward-slash separators so the file is portable
//! across Windows / macOS / Linux forges.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::SkillRegistry;

/// PRD-pinned schema version for the on-disk index.
pub const REGISTRY_INDEX_VERSION: &str = "1.0";

fn default_visibility() -> String {
    "public".to_string()
}

/// On-disk shape of `<root>/REGISTRY.json` (PRD-13 §3.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryIndex {
    /// Schema version; pinned to `"1.0"` for this implementation.
    pub version: String,
    /// RFC3339 timestamp of the last write (`YYYY-MM-DDTHH:MM:SSZ`).
    pub last_updated: String,
    /// One entry per loaded skill, in id-sorted order.
    pub skills: Vec<RegistryIndexEntry>,
}

/// One row of the [`RegistryIndex::skills`] list. Carries the
/// minimum surface needed to look the skill up without re-parsing
/// the `.skill.md` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryIndexEntry {
    /// Stable kebab-case identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Forward-slash, root-relative path to the `.skill.md` file.
    pub path: String,
    /// Semantic version string.
    pub version: String,
    /// Category tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Auto-activation contexts.
    #[serde(default)]
    pub applicable_contexts: Vec<String>,
    /// Author or organization.
    pub author: String,
    /// `public` (default) or `private`.
    #[serde(default = "default_visibility")]
    pub visibility: String,
}

/// Errors surfaced from [`write_index`] / [`read_index`].
#[derive(Debug, Error)]
pub enum RegistryIndexError {
    /// Filesystem failure (missing parent dir, permission, etc).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON encode/decode failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Serialize `reg` to `path` atomically. Writes to a sibling
/// `*.json.tmp` file first, then renames over the destination.
///
/// `root` is used to compute forward-slash relative paths for each
/// entry. Skills whose source path is *not* under `root` are stored
/// with their absolute path verbatim (forward-slashed) — this should
/// not happen in practice because [`SkillRegistry::load`] only
/// records files that live under the walked root.
///
/// # Errors
/// Returns [`RegistryIndexError::Io`] on filesystem failure or
/// [`RegistryIndexError::Json`] on serialization failure. On error
/// the destination is left untouched and the tmp file is best-effort
/// removed.
pub fn write_index(
    path: &Path,
    root: &Path,
    reg: &SkillRegistry,
) -> Result<(), RegistryIndexError> {
    let entries: Vec<RegistryIndexEntry> = reg
        .entries()
        .map(|(p, skill)| RegistryIndexEntry {
            id: skill.meta.id.clone(),
            name: skill.meta.name.clone(),
            path: relative_forward_slash(root, p),
            version: skill.meta.version.clone(),
            tags: skill.meta.tags.clone(),
            applicable_contexts: skill.meta.applicable_contexts.clone(),
            author: skill.meta.author.clone(),
            visibility: skill
                .meta
                .visibility
                .clone()
                .unwrap_or_else(default_visibility),
        })
        .collect();

    let index = RegistryIndex {
        version: REGISTRY_INDEX_VERSION.to_string(),
        last_updated: now_rfc3339_utc(),
        skills: entries,
    };

    let tmp = path.with_extension("json.tmp");
    let write_result: Result<(), RegistryIndexError> = (|| {
        let json = serde_json::to_vec_pretty(&index)?;
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();

    if write_result.is_err() {
        // Best-effort cleanup; ignore secondary failures.
        let _ = std::fs::remove_file(&tmp);
    }
    write_result
}

/// Read and decode the JSON index at `path`.
///
/// # Errors
/// Returns [`RegistryIndexError::Io`] (with `kind() == NotFound` when
/// the index is missing) or [`RegistryIndexError::Json`] on a
/// malformed index.
pub fn read_index(path: &Path) -> Result<RegistryIndex, RegistryIndexError> {
    let bytes = std::fs::read(path)?;
    let index: RegistryIndex = serde_json::from_slice(&bytes)?;
    Ok(index)
}

/// Render `path` relative to `root` with `/` separators.
fn relative_forward_slash(root: &Path, path: &Path) -> String {
    let rel: PathBuf = path
        .strip_prefix(root)
        .map_or_else(|_| path.to_path_buf(), std::path::Path::to_path_buf);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Hand-rolled RFC3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`).
///
/// Avoids pulling `chrono` / `time` for what amounts to one
/// formatted string per index write. Deterministic shape; tests can
/// regex-match `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$`.
fn now_rfc3339_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_rfc3339_utc(secs)
}

/// Convert a unix-epoch seconds value into an RFC3339 UTC string.
/// Pure / panic-free / no allocations beyond the final `String`.
fn format_rfc3339_utc(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_seconds_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Civil-from-days algorithm (Howard Hinnant) — converts a unix
/// epoch seconds value into the (Y, M, D, h, m, s) UTC tuple.
/// Valid for any seconds value the platform's `SystemTime` can yield.
///
/// All `as` casts are bounded by the algorithm: `secs % 86_400` fits
/// in `u32`, `secs / 86_400` fits in `i64` for any plausible
/// `SystemTime`, `doe ∈ [0, 146_096]` fits in `u64`, `yoe ∈ [0, 399]`
/// fits in `i64`, `m ∈ [1, 12]` and `d ∈ [1, 31]` fit in `u32`, and
/// the resulting year fits in `i32` for any forge mtime.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn epoch_seconds_to_ymdhms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days_since_epoch = (secs / 86_400) as i64;
    let secs_of_day = (secs % 86_400) as u32;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Hinnant: shift origin from 1970-01-01 to 0000-03-01.
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = (y + i64::from(m <= 2)) as i32;

    (year, m as u32, d as u32, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    const SKILL_A: &str = r"---
name: A
id: skill-a
description: first
version: 1.0.0
author: alice
created: 2026-04-01
tags: [alpha]
applicable_contexts: [ai-chat]
visibility: public
---
body A
";

    const SKILL_B: &str = r"---
name: B
id: skill-b
description: second
version: 2.0.0
author: bob
created: 2026-04-02
tags: [beta, gamma]
applicable_contexts: [editor, agent]
---
body B
";

    fn write(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn write_then_read_roundtrips_two_skills() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "a.skill.md", SKILL_A);
        write(tmp.path(), "b.skill.md", SKILL_B);
        let reg = SkillRegistry::load(tmp.path()).unwrap();

        let index_path = tmp.path().join("REGISTRY.json");
        write_index(&index_path, tmp.path(), &reg).unwrap();
        let parsed = read_index(&index_path).unwrap();

        assert_eq!(parsed.version, "1.0");
        assert_eq!(parsed.skills.len(), 2);

        let a = parsed.skills.iter().find(|e| e.id == "skill-a").unwrap();
        assert_eq!(a.id, "skill-a");
        assert_eq!(a.name, "A");
        assert_eq!(a.path, "a.skill.md");
        assert_eq!(a.version, "1.0.0");
        assert_eq!(a.tags, vec!["alpha".to_string()]);
        assert_eq!(a.applicable_contexts, vec!["ai-chat".to_string()]);
        assert_eq!(a.author, "alice");
        assert_eq!(a.visibility, "public");

        let b = parsed.skills.iter().find(|e| e.id == "skill-b").unwrap();
        assert_eq!(b.id, "skill-b");
        assert_eq!(b.name, "B");
        assert_eq!(b.path, "b.skill.md");
        assert_eq!(b.version, "2.0.0");
        assert_eq!(b.tags, vec!["beta".to_string(), "gamma".to_string()]);
        assert_eq!(
            b.applicable_contexts,
            vec!["editor".to_string(), "agent".to_string()]
        );
        assert_eq!(b.author, "bob");
        // No `visibility` declared in the YAML ⇒ default "public".
        assert_eq!(b.visibility, "public");
    }

    #[test]
    fn write_uses_forward_slash_paths_on_all_platforms() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        write(&tmp.path().join("sub"), "b.skill.md", SKILL_B);
        let reg = SkillRegistry::load(tmp.path()).unwrap();

        let index_path = tmp.path().join("REGISTRY.json");
        write_index(&index_path, tmp.path(), &reg).unwrap();
        let parsed = read_index(&index_path).unwrap();

        let entry = parsed.skills.iter().find(|e| e.id == "skill-b").unwrap();
        assert_eq!(entry.path, "sub/b.skill.md");
        assert!(!entry.path.contains('\\'));
    }

    #[test]
    fn write_is_atomic_no_partial_file_on_serde_error() {
        // Pass a destination whose parent directory doesn't exist.
        // The std::fs::write call on the sibling `.tmp` will fail,
        // we should leave neither the destination nor the tmp file
        // behind.
        let tmp = TempDir::new().unwrap();
        let reg = SkillRegistry::empty();
        let bogus = tmp.path().join("does-not-exist").join("REGISTRY.json");
        let err = write_index(&bogus, tmp.path(), &reg).unwrap_err();
        match err {
            RegistryIndexError::Io(_) => {}
            RegistryIndexError::Json(_) => panic!("expected Io error"),
        }
        assert!(!bogus.exists(), "destination must not be created on error");
        let tmp_path = bogus.with_extension("json.tmp");
        assert!(!tmp_path.exists(), "tmp file must be cleaned up on error");
    }

    #[test]
    fn read_missing_file_returns_io_not_found() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("REGISTRY.json");
        let err = read_index(&missing).unwrap_err();
        match err {
            RegistryIndexError::Io(io) => {
                assert_eq!(io.kind(), std::io::ErrorKind::NotFound);
            }
            RegistryIndexError::Json(_) => panic!("expected Io error"),
        }
    }

    #[test]
    fn version_field_is_one_dot_zero() {
        let tmp = TempDir::new().unwrap();
        let reg = SkillRegistry::empty();
        let index_path = tmp.path().join("REGISTRY.json");
        write_index(&index_path, tmp.path(), &reg).unwrap();
        let parsed = read_index(&index_path).unwrap();
        assert_eq!(parsed.version, "1.0");
        assert_eq!(REGISTRY_INDEX_VERSION, "1.0");
    }

    #[test]
    fn last_updated_is_valid_rfc3339() {
        let tmp = TempDir::new().unwrap();
        let reg = SkillRegistry::empty();
        let index_path = tmp.path().join("REGISTRY.json");
        write_index(&index_path, tmp.path(), &reg).unwrap();
        let parsed = read_index(&index_path).unwrap();
        let s = &parsed.last_updated;
        // Shape `YYYY-MM-DDTHH:MM:SSZ`.
        assert_eq!(s.len(), 20, "wrong length: {s}");
        let bytes = s.as_bytes();
        for (i, c) in bytes.iter().enumerate() {
            match i {
                4 | 7 => assert_eq!(*c, b'-', "expected '-' at idx {i} in {s}"),
                10 => assert_eq!(*c, b'T', "expected 'T' at idx {i} in {s}"),
                13 | 16 => assert_eq!(*c, b':', "expected ':' at idx {i} in {s}"),
                19 => assert_eq!(*c, b'Z', "expected 'Z' at idx {i} in {s}"),
                _ => assert!(c.is_ascii_digit(), "expected digit at idx {i} in {s}"),
            }
        }
    }

    #[test]
    fn epoch_zero_formats_as_unix_epoch() {
        // Sanity check on the civil-from-days math.
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        // 2026-04-28T12:34:56Z = 1777379696
        assert_eq!(format_rfc3339_utc(1_777_379_696), "2026-04-28T12:34:56Z");
    }
}
