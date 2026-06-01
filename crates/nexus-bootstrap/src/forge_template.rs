//! Forge scaffold templates (BL-054 Phase 1).
//!
//! Lifted out of `nexus-cli` so both the CLI's `nexus forge init
//! --template os` and the shell's Tauri `init_forge` command can share
//! a single implementation. The scaffolder runs against an
//! already-initialised forge (i.e. `init_forge` has created `.forge/`)
//! and is idempotent — re-running it on a populated tree is a no-op
//! that never overwrites pre-existing user files.

use std::path::Path;

use anyhow::Result;

/// The supported template identifiers. The string form is what the CLI
/// `--template` flag and the Tauri command's `template` arg accept;
/// callers parse via [`ForgeTemplate::from_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeTemplate {
    /// BL-054 §Phased-implementation Phase 1 layout: raw / wiki /
    /// output / projects / ops / personal / archive plus a memory-map
    /// `CLAUDE.md` and an `architecture.md` placeholder.
    Os,
}

impl ForgeTemplate {
    /// Parse a template name from a CLI flag value or IPC arg. Unknown
    /// names return `None` — callers turn that into a user-facing error
    /// in whichever style suits their surface.
    // Inherent `from_str` returns `Option`, not the `Result`-based `std::str::FromStr`.
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "os" => Some(Self::Os),
            _ => None,
        }
    }

    /// The canonical wire / CLI form. Mirrors [`Self::from_str`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Os => "os",
        }
    }
}

/// Apply `template` to the forge rooted at `root`. Idempotent: each
/// directory `create_dir_all`s and each seeded file is written via
/// [`write_if_absent`] so an existing `CLAUDE.md` / `architecture.md`
/// is preserved verbatim.
///
/// # Errors
/// Returns the underlying I/O error annotated with the offending path.
pub fn apply(root: &Path, template: ForgeTemplate) -> Result<()> {
    match template {
        ForgeTemplate::Os => scaffold_os(root),
    }
}

const OS_DIRS: &[&str] = &[
    "raw", "wiki", "output", "projects", "ops", "personal", "archive",
];

const OS_CLAUDE_MD: &str = include_str!("../templates/os/CLAUDE.md");
const OS_ARCHITECTURE_MD: &str = include_str!("../templates/os/architecture.md");

fn scaffold_os(root: &Path) -> Result<()> {
    for dir in OS_DIRS {
        let path = root.join(dir);
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow::anyhow!("create '{}': {e}", path.display()))?;
        // Empty directories don't survive `git add`; drop a .gitkeep so
        // the OS layout round-trips through version control.
        let keep = path.join(".gitkeep");
        if !keep.exists() {
            std::fs::write(&keep, b"")
                .map_err(|e| anyhow::anyhow!("write '{}': {e}", keep.display()))?;
        }
    }
    write_if_absent(&root.join("CLAUDE.md"), OS_CLAUDE_MD)?;
    write_if_absent(&root.join("architecture.md"), OS_ARCHITECTURE_MD)?;
    Ok(())
}

fn write_if_absent(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("write '{}': {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_round_trips() {
        assert_eq!(ForgeTemplate::from_str("os"), Some(ForgeTemplate::Os));
        assert_eq!(ForgeTemplate::Os.as_str(), "os");
        assert_eq!(ForgeTemplate::from_str("blank"), None);
        assert_eq!(ForgeTemplate::from_str(""), None);
    }

    #[test]
    fn apply_os_creates_all_directories() {
        let tmp = tempfile::tempdir().unwrap();
        apply(tmp.path(), ForgeTemplate::Os).unwrap();
        for dir in OS_DIRS {
            let path = tmp.path().join(dir);
            assert!(path.is_dir(), "expected directory at {}", path.display());
            assert!(
                path.join(".gitkeep").exists(),
                ".gitkeep missing in {}",
                path.display(),
            );
        }
    }

    #[test]
    fn apply_os_seeds_root_files() {
        let tmp = tempfile::tempdir().unwrap();
        apply(tmp.path(), ForgeTemplate::Os).unwrap();
        let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert!(claude.contains("Memory map"));
        assert!(claude.contains("raw/"));
        let arch = std::fs::read_to_string(tmp.path().join("architecture.md")).unwrap();
        assert!(arch.contains("Architecture"));
    }

    #[test]
    fn apply_os_preserves_pre_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("CLAUDE.md");
        std::fs::write(&claude, "user-authored content").unwrap();
        apply(tmp.path(), ForgeTemplate::Os).unwrap();
        assert_eq!(
            std::fs::read_to_string(&claude).unwrap(),
            "user-authored content",
        );
    }

    #[test]
    fn apply_os_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        apply(tmp.path(), ForgeTemplate::Os).unwrap();
        apply(tmp.path(), ForgeTemplate::Os).unwrap();
        for dir in OS_DIRS {
            assert!(tmp.path().join(dir).is_dir());
        }
    }
}
