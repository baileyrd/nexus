//! In-memory skill registry built from a directory walk.
//!
//! Matches the `.forge/skills/` layout from PRD-13 §2.1 / §3.1.
//! Sub-directories are recursed so `personal/` and `org/` live under
//! the same lookup surface; file extension filter is strictly
//! `.skill.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{parse_skill_file, Skill};

/// Errors surfaced from [`SkillRegistry`] operations.
#[derive(Debug, Error)]
pub enum SkillRegistryError {
    /// Root directory didn't exist or couldn't be read.
    #[error("io error reading skills dir: {0}")]
    Io(#[from] std::io::Error),
    /// One or more files failed to parse.
    #[error("{count} skill file(s) failed to parse; first error: {first}")]
    PartialParseFailure {
        /// Total files that failed to parse in this load.
        count: usize,
        /// First failure's human-readable message.
        first: String,
    },
}

/// In-memory registry. Construct via [`SkillRegistry::load`] or
/// [`SkillRegistry::empty`] and mutate through [`Self::insert`] /
/// [`Self::remove`].
#[derive(Debug, Default)]
pub struct SkillRegistry {
    /// Skill id → (source path, parsed skill).
    skills: BTreeMap<String, RegistryEntry>,
}

#[derive(Debug, Clone)]
struct RegistryEntry {
    path: PathBuf,
    skill: Skill,
}

impl SkillRegistry {
    /// Fresh empty registry. Useful for tests and for callers
    /// building a registry from plugin-supplied skills without a
    /// filesystem scan.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Walk `root` recursively, parsing every `.skill.md` file into
    /// the registry. Duplicate ids are rejected — the second
    /// occurrence is recorded as a parse failure so the caller can
    /// rename / remove the offending file.
    ///
    /// # Errors
    /// Returns [`SkillRegistryError::Io`] if `root` can't be
    /// enumerated, or [`SkillRegistryError::PartialParseFailure`]
    /// when one or more files failed. The registry always contains
    /// every successfully-parsed skill regardless of partial
    /// failures — callers decide whether to surface the error or
    /// accept the loaded subset.
    pub fn load(root: &Path) -> Result<Self, SkillRegistryError> {
        let mut reg = Self::empty();
        let mut failures: Vec<String> = Vec::new();
        visit_dir(root, &mut |path| {
            if !is_skill_file(path) {
                return;
            }
            match parse_skill_file(path) {
                Ok(skill) => {
                    let id = skill.meta.id.clone();
                    if reg.skills.contains_key(&id) {
                        failures.push(format!("duplicate skill id '{id}' at {}", path.display()));
                        return;
                    }
                    reg.skills.insert(
                        id,
                        RegistryEntry {
                            path: path.to_path_buf(),
                            skill,
                        },
                    );
                }
                Err(err) => failures.push(format!("failed to parse {}: {err}", path.display())),
            }
        })?;
        if !failures.is_empty() {
            let count = failures.len();
            let first = failures.into_iter().next().unwrap_or_default();
            return Err(SkillRegistryError::PartialParseFailure { count, first });
        }
        Ok(reg)
    }

    /// Number of skills currently in the registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether the registry holds no skills.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Look up a skill by its id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id).map(|e| &e.skill)
    }

    /// Source path for a skill, if it was loaded from disk.
    #[must_use]
    pub fn path_for(&self, id: &str) -> Option<&Path> {
        self.skills.get(id).map(|e| e.path.as_path())
    }

    /// Iterate every loaded skill in id-sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values().map(|e| &e.skill)
    }

    /// Iterate `(source path, parsed skill)` pairs in id-sorted
    /// order. Used by the [`registry_index`](crate::registry_index)
    /// writer to project the in-memory registry into the on-disk
    /// `REGISTRY.json` index.
    pub fn entries(&self) -> impl Iterator<Item = (&Path, &Skill)> {
        self.skills.values().map(|e| (e.path.as_path(), &e.skill))
    }

    /// Cold-start variant of [`Self::load`] that prefers the
    /// `<root>/REGISTRY.json` index when it is fresh, falling back
    /// to a directory walk otherwise.
    ///
    /// Freshness is conservative: the index is rejected (and the
    /// walk runs) if any of the listed `.skill.md` files is missing
    /// on disk *or* if any `.skill.md` under `root` has an mtime
    /// newer than the index's `last_updated` timestamp. The on-disk
    /// `.skill.md` files remain authoritative — this is a read-side
    /// optimization for external CLIs only and must not be used to
    /// short-circuit handler dispatch in the core plugin.
    ///
    /// # Errors
    /// Mirrors [`Self::load`] when the walk path is taken. When the
    /// index path is taken successfully, never returns
    /// [`SkillRegistryError::PartialParseFailure`].
    pub fn load_with_index(root: &Path) -> Result<Self, SkillRegistryError> {
        let index_path = root.join("REGISTRY.json");
        if let Some(reg) = try_load_from_index(root, &index_path) {
            return Ok(reg);
        }
        Self::load(root)
    }

    /// Every skill whose `applicable_contexts` list contains
    /// `context`. O(n); expected to be small.
    pub fn by_context<'a>(&'a self, context: &'a str) -> impl Iterator<Item = &'a Skill> {
        self.iter()
            .filter(move |s| s.meta.applicable_contexts.iter().any(|c| c == context))
    }

    /// Every skill whose `triggers` list contains a phrase that
    /// appears in `text` (case-insensitive substring match).
    pub fn triggered_by<'a>(&'a self, text: &'a str) -> impl Iterator<Item = &'a Skill> {
        let lower = text.to_lowercase();
        self.iter().filter(move |s| {
            s.meta
                .triggers
                .iter()
                .any(|t| !t.is_empty() && lower.contains(&t.to_lowercase()))
        })
    }

    /// Insert a pre-parsed skill under its declared id. Returns
    /// `true` when this displaced an existing entry — callers that
    /// want strict duplicate rejection should check with
    /// [`Self::get`] first.
    pub fn insert(&mut self, path: PathBuf, skill: Skill) -> bool {
        let id = skill.meta.id.clone();
        self.skills
            .insert(id, RegistryEntry { path, skill })
            .is_some()
    }

    /// Remove a skill by id; returns the removed entry's path if
    /// present.
    pub fn remove(&mut self, id: &str) -> Option<PathBuf> {
        self.skills.remove(id).map(|e| e.path)
    }
}

/// Try to populate a [`SkillRegistry`] from `<root>/REGISTRY.json`.
/// Returns `None` whenever the index is missing, malformed, points
/// to a vanished file, or is older than any on-disk `.skill.md`. The
/// caller falls back to a directory walk on `None`.
fn try_load_from_index(root: &Path, index_path: &Path) -> Option<SkillRegistry> {
    let index = crate::registry_index::read_index(index_path).ok()?;
    let last_updated = parse_rfc3339_seconds(&index.last_updated)?;

    // Reject if any skill_md file under `root` is newer than the
    // index, even if it isn't listed (newly-added skill case).
    if any_skill_newer_than(root, last_updated).ok()? {
        return None;
    }

    let mut reg = SkillRegistry::empty();
    for entry in &index.skills {
        // PRD-13 §3.1 path is forward-slash, root-relative.
        let mut abs = root.to_path_buf();
        for part in entry.path.split('/') {
            abs.push(part);
        }
        if !abs.is_file() {
            return None;
        }
        let skill = parse_skill_file(&abs).ok()?;
        // The frontmatter id must match the index entry id; if the
        // file was edited out-of-band, force a walk.
        if skill.meta.id != entry.id {
            return None;
        }
        reg.skills
            .insert(skill.meta.id.clone(), RegistryEntry { path: abs, skill });
    }
    Some(reg)
}

/// Walk `root` and return `Ok(true)` if any `.skill.md` has an mtime
/// strictly newer than `cutoff_secs` (unix epoch seconds).
fn any_skill_newer_than(root: &Path, cutoff_secs: u64) -> std::io::Result<bool> {
    fn walk(dir: &Path, cutoff_secs: u64) -> std::io::Result<bool> {
        if !dir.exists() {
            return Ok(false);
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            // Skip symlinks (issue #85) — same rationale as `visit_dir`
            // below. We don't want a symlink to make us declare files
            // outside the skills root "newer", which would force a
            // reload that walks (and skips) those symlinks anyway.
            let ft = std::fs::symlink_metadata(&path)?.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if walk(&path, cutoff_secs)? {
                    return Ok(true);
                }
            } else if ft.is_file() && is_skill_file(&path) {
                let meta = entry.metadata()?;
                let mtime = meta
                    .modified()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if mtime > cutoff_secs {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    walk(root, cutoff_secs)
}

/// Parse a `YYYY-MM-DDTHH:MM:SSZ` UTC RFC3339 string into unix epoch
/// seconds. Returns `None` on any shape mismatch.
fn parse_rfc3339_seconds(s: &str) -> Option<u64> {
    if s.len() != 20 || !s.ends_with('Z') {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    let minute: u32 = s.get(14..16)?.parse().ok()?;
    let second: u32 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(ymdhms_to_epoch_seconds(
        year, month, day, hour, minute, second,
    ))
}

/// Inverse of `registry_index::epoch_seconds_to_ymdhms`.
///
/// All `as` casts are bounded: `yoe ∈ [0, 399]` fits in `u64`,
/// `doe ∈ [0, 146_096]` fits in `i64`, and the final `total` is
/// clamped to non-negative before the `as u64` cast.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn ymdhms_to_epoch_seconds(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> u64 {
    // Hinnant: days from civil.
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let m = u64::from(month);
    let d = u64::from(day);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146_097 + doe as i64 - 719_468;
    let total = days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second);
    total.max(0) as u64
}

fn is_skill_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".skill.md")
}

fn visit_dir(root: &Path, visitor: &mut dyn FnMut(&Path)) -> Result<(), SkillRegistryError> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        // Issue #85. Use `symlink_metadata` rather than relying on
        // `DirEntry::file_type` (which on some platforms may follow
        // symlinks). Skip any symlink entry — a symlink in the skills
        // dir pointing at `/etc` or another reachable directory could
        // otherwise smuggle the walker outside the intended root and
        // load arbitrary files as "skills". `is_dir()`/`is_file()` on
        // a `FileType` are mutually exclusive with `is_symlink()`, so
        // this also means the walker only recurses through real
        // directories.
        let file_type = std::fs::symlink_metadata(&path)?.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            visit_dir(&path, visitor)?;
        } else if file_type.is_file() {
            visitor(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SKILL_A: &str = r#"---
name: A
id: skill-a
description: first
version: 1.0.0
author: me
created: 2026-04-01
tags: [alpha]
applicable_contexts: [ai-chat]
triggers: ["do A", "alpha"]
---
body A
"#;

    const SKILL_B: &str = r"---
name: B
id: skill-b
description: second
version: 1.0.0
author: me
created: 2026-04-02
tags: [beta]
applicable_contexts: [editor]
---
body B
";

    fn write_skill(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn load_reads_every_skill_md_under_root() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        write_skill(&tmp.path().join("sub"), "b.skill.md", SKILL_B);
        // Non-skill files are ignored.
        std::fs::write(tmp.path().join("NOTES.md"), "not a skill").unwrap();

        let reg = SkillRegistry::load(tmp.path()).unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.get("skill-a").is_some());
        assert!(reg.get("skill-b").is_some());
    }

    #[test]
    fn load_surfaces_partial_parse_failures() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "good.skill.md", SKILL_A);
        write_skill(tmp.path(), "bad.skill.md", "not yaml at all");
        let err = SkillRegistry::load(tmp.path()).unwrap_err();
        assert!(matches!(
            err,
            SkillRegistryError::PartialParseFailure { count: 1, .. }
        ));
    }

    #[test]
    fn load_rejects_duplicate_ids_via_partial_failure() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        write_skill(tmp.path(), "also-a.skill.md", SKILL_A);
        let err = SkillRegistry::load(tmp.path()).unwrap_err();
        match err {
            SkillRegistryError::PartialParseFailure { count, first } => {
                assert_eq!(count, 1);
                assert!(first.contains("duplicate"));
            }
            SkillRegistryError::Io(_) => panic!("expected duplicate failure, got Io error"),
        }
    }

    #[test]
    fn load_of_missing_root_is_empty() {
        let reg = SkillRegistry::load(Path::new("/definitely/not/a/dir")).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn load_with_index_uses_index_when_fresh() {
        use crate::registry_index::write_index;

        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);

        // Build + persist the index.
        let reg = SkillRegistry::load(tmp.path()).unwrap();
        let index_path = tmp.path().join("REGISTRY.json");
        write_index(&index_path, tmp.path(), &reg).unwrap();

        // Add a second skill on disk, but force its mtime to be
        // older than the index — load_with_index should not see it.
        let stray = tmp.path().join("b.skill.md");
        std::fs::write(&stray, SKILL_B).unwrap();
        let old_mtime = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(60);
        std::fs::File::open(&stray)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();

        let loaded = SkillRegistry::load_with_index(tmp.path()).unwrap();
        // Index only knew about skill-a; the stray b is older than
        // the index so the freshness check still accepts the index.
        assert_eq!(loaded.len(), 1);
        assert!(loaded.get("skill-a").is_some());
        assert!(loaded.get("skill-b").is_none());
    }

    #[test]
    fn load_with_index_falls_back_to_walk_when_missing() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        write_skill(tmp.path(), "b.skill.md", SKILL_B);

        // No REGISTRY.json on disk ⇒ behaves like load().
        let loaded = SkillRegistry::load_with_index(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.get("skill-a").is_some());
        assert!(loaded.get("skill-b").is_some());
    }

    #[test]
    fn load_with_index_falls_back_when_listed_path_missing() {
        use crate::registry_index::write_index;

        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        let reg = SkillRegistry::load(tmp.path()).unwrap();
        let index_path = tmp.path().join("REGISTRY.json");
        write_index(&index_path, tmp.path(), &reg).unwrap();

        // Delete the underlying skill file. Index still lists it ⇒
        // staleness check forces a walk, which finds nothing.
        std::fs::remove_file(tmp.path().join("a.skill.md")).unwrap();
        let loaded = SkillRegistry::load_with_index(tmp.path()).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn by_context_and_triggered_by_filter_correctly() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        write_skill(tmp.path(), "b.skill.md", SKILL_B);
        let reg = SkillRegistry::load(tmp.path()).unwrap();

        let chat_skills: Vec<_> = reg
            .by_context("ai-chat")
            .map(|s| s.meta.id.clone())
            .collect();
        assert_eq!(chat_skills, vec!["skill-a"]);

        let triggered: Vec<_> = reg
            .triggered_by("please DO A now")
            .map(|s| s.meta.id.clone())
            .collect();
        assert_eq!(triggered, vec!["skill-a"]);
    }
}
