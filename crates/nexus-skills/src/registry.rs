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
                        failures.push(format!(
                            "duplicate skill id '{id}' at {}",
                            path.display()
                        ));
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
                Err(err) => failures.push(format!(
                    "failed to parse {}: {err}",
                    path.display()
                )),
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

fn is_skill_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    name.ends_with(".skill.md")
}

fn visit_dir(
    root: &Path,
    visitor: &mut dyn FnMut(&Path),
) -> Result<(), SkillRegistryError> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
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

    const SKILL_B: &str = r#"---
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
"#;

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
            other => panic!("expected duplicate failure, got {other:?}"),
        }
    }

    #[test]
    fn load_of_missing_root_is_empty() {
        let reg = SkillRegistry::load(Path::new("/definitely/not/a/dir")).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn by_context_and_triggered_by_filter_correctly() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        write_skill(tmp.path(), "b.skill.md", SKILL_B);
        let reg = SkillRegistry::load(tmp.path()).unwrap();

        let chat_skills: Vec<_> = reg.by_context("ai-chat").map(|s| s.meta.id.clone()).collect();
        assert_eq!(chat_skills, vec!["skill-a"]);

        let triggered: Vec<_> = reg.triggered_by("please DO A now").map(|s| s.meta.id.clone()).collect();
        assert_eq!(triggered, vec!["skill-a"]);
    }
}
