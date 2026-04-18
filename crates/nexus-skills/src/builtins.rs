//! Built-in skill library shipped in-tree.
//!
//! These canonical `.skill.md` files are compiled into `nexus-skills`
//! via `include_str!` and seeded into a fresh forge's
//! `<forge>/.forge/skills/` on first bootstrap. Non-destructive —
//! existing files at the same path are left untouched so users can
//! shadow built-ins without them being overwritten on every launch.

use std::io::ErrorKind;
use std::path::Path;

/// One shipped-in-tree skill file: `(filename, contents)`.
type BuiltIn = (&'static str, &'static str);

/// The current built-in set. Append-only — removing an entry breaks
/// idempotent seeding. Changing a file's body is fine; the seeder
/// never overwrites an existing user file.
const BUILTINS: &[BuiltIn] = &[
    (
        "code-reviewer.skill.md",
        include_str!("../builtins/code-reviewer.skill.md"),
    ),
    (
        "daily-journal.skill.md",
        include_str!("../builtins/daily-journal.skill.md"),
    ),
    (
        "meeting-notes.skill.md",
        include_str!("../builtins/meeting-notes.skill.md"),
    ),
    (
        "commit-message.skill.md",
        include_str!("../builtins/commit-message.skill.md"),
    ),
];

/// Result of a [`seed_builtins`] call.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SeedReport {
    /// Files newly written (didn't exist at target path before).
    pub created: Vec<String>,
    /// Files skipped because a file already exists at the target.
    pub skipped: Vec<String>,
}

/// Write every built-in skill file into `dir` that doesn't already
/// exist there. Creates `dir` if missing. Returns a report
/// distinguishing newly-created from already-present files so callers
/// can log "seeded N / skipped M" without re-scanning the directory.
///
/// # Errors
///
/// Returns `io::Error` if `dir` can't be created or a write fails for
/// a reason other than the target file already existing.
pub fn seed_builtins(dir: &Path) -> std::io::Result<SeedReport> {
    std::fs::create_dir_all(dir)?;
    let mut report = SeedReport::default();
    for (name, body) in BUILTINS {
        let target = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(body.as_bytes())?;
                report.created.push((*name).to_string());
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                report.skipped.push((*name).to_string());
            }
            Err(err) => return Err(err),
        }
    }
    Ok(report)
}

/// Return the list of shipped built-in skill filenames. Useful for
/// diagnostics / UI "about" screens.
#[must_use]
pub fn builtin_filenames() -> Vec<&'static str> {
    BUILTINS.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn every_builtin_parses() {
        for (name, body) in BUILTINS {
            crate::parse_skill_text(body).unwrap_or_else(|e| {
                panic!("built-in {name} failed to parse: {e}")
            });
        }
    }

    #[test]
    fn seed_creates_all_on_fresh_dir() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        let report = seed_builtins(&target).unwrap();
        assert_eq!(report.created.len(), BUILTINS.len());
        assert!(report.skipped.is_empty());
        for (name, _) in BUILTINS {
            assert!(target.join(name).exists(), "missing {name}");
        }
    }

    #[test]
    fn seed_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        seed_builtins(&target).unwrap();
        let report = seed_builtins(&target).unwrap();
        assert!(report.created.is_empty());
        assert_eq!(report.skipped.len(), BUILTINS.len());
    }

    #[test]
    fn seed_does_not_overwrite_user_edits() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("skills");
        std::fs::create_dir_all(&target).unwrap();
        let shadowed = target.join(BUILTINS[0].0);
        std::fs::write(&shadowed, "custom user content").unwrap();
        let report = seed_builtins(&target).unwrap();
        assert!(report.skipped.contains(&BUILTINS[0].0.to_string()));
        assert_eq!(
            std::fs::read_to_string(&shadowed).unwrap(),
            "custom user content"
        );
    }
}
