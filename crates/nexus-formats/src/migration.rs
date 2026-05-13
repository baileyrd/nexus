//! Frontmatter versioning + migration runner (PRD-06 §9 — DG-43).
//!
//! PRD-06 §9 specifies a `version:` field on every text format and a
//! migration tool that walks the forge and applies migrations between
//! versions. Today no `v2.0` exists, so there are no migrations to
//! run — but the infrastructure is required before any
//! forge-format-breaking change can land.
//!
//! ## Conventions
//!
//! - Files without a `version:` field are treated as **implicitly
//!   v1.0** (the only version that exists at time of writing). The
//!   migration runner doesn't rewrite them — they're already valid
//!   under the current schema.
//! - Versions are parsed as semver-shaped strings (`MAJOR.MINOR`).
//!   `MAJOR` bumps signal breaking changes that require explicit
//!   migration; `MINOR` bumps are safe rolls (additive fields).
//! - Migrations are functions `(content) -> Result<content>`. Each
//!   migration carries a `(from, to)` pair so the runner can chain
//!   them (e.g. v1.0 → v2.0 might be decomposed into v1.0→v1.5→v2.0).
//!
//! ## Today's state
//!
//! `MigrationRegistry::new()` returns an empty registry. No migration
//! is registered because no breaking forge-format change has been
//! published. When one lands, register the migration via
//! `registry.register(from, to, Box::new(my_fn))` and the runner
//! picks it up automatically.

use std::collections::HashMap;

use thiserror::Error;

use crate::markdown::frontmatter::extract as extract_frontmatter;

/// Implicit version used when a file carries no `version:` frontmatter
/// field. Matches the v1.0 forge format that's been the only shipped
/// version since launch.
pub const DEFAULT_VERSION: &str = "1.0";

/// Parsed `MAJOR.MINOR` version string. Bigger than just a `String`
/// because the migration runner needs to compare versions to pick a
/// migration path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatVersion {
    /// Major version — bump = breaking schema change requiring migration.
    pub major: u32,
    /// Minor version — bump = additive change, safe roll.
    pub minor: u32,
}

impl FormatVersion {
    /// Construct a version directly. Used by tests + migration registries.
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Parse a `MAJOR.MINOR` string. Accepts a trailing `.PATCH`
    /// component which is ignored (the migration system doesn't
    /// care about patch-level — it can't be breaking by definition).
    ///
    /// # Errors
    /// [`MigrationError::InvalidVersion`] when the input isn't a
    /// recognisable version string.
    pub fn parse(s: &str) -> Result<Self, MigrationError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(MigrationError::InvalidVersion(s.to_string()));
        }
        let mut parts = trimmed.split('.');
        let major = parts
            .next()
            .and_then(|p| p.parse::<u32>().ok())
            .ok_or_else(|| MigrationError::InvalidVersion(s.to_string()))?;
        let minor = parts
            .next()
            .and_then(|p| p.parse::<u32>().ok())
            .ok_or_else(|| MigrationError::InvalidVersion(s.to_string()))?;
        // patch (if any) ignored; reject extra dotted segments
        // beyond major.minor.patch to keep round-trip honest.
        let _patch = parts.next(); // discard
        if parts.next().is_some() {
            return Err(MigrationError::InvalidVersion(s.to_string()));
        }
        Ok(Self { major, minor })
    }

    /// String form (`"MAJOR.MINOR"`). Round-trips via `parse`.
    #[must_use]
    pub fn to_string_compact(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl std::fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Errors surfaced by the migration runner.
#[derive(Debug, Error)]
pub enum MigrationError {
    /// Couldn't parse a version string.
    #[error("invalid version string: '{0}'")]
    InvalidVersion(String),
    /// No migration registered for the requested step.
    #[error("no migration available for {from} → {to}")]
    NoMigration {
        /// Source version.
        from: FormatVersion,
        /// Target version.
        to: FormatVersion,
    },
    /// Migration ran but raised an error.
    #[error("migration {from} → {to} failed: {reason}")]
    MigrationFailed {
        /// Source version.
        from: FormatVersion,
        /// Target version.
        to: FormatVersion,
        /// Underlying error from the migration function.
        reason: String,
    },
    /// Frontmatter parse failure (delegated to markdown layer).
    #[error("frontmatter parse failed: {0}")]
    Frontmatter(String),
}

/// Function pointer shape for one migration step.
///
/// Takes the full file content (frontmatter + body) and returns the
/// post-migration content. Migrations are expected to be **idempotent
/// on already-migrated input** — re-running v1.0→v2.0 on a v2.0 file
/// should be a no-op (or fail loudly), per PRD-06 §9.3.
pub type MigrationFn =
    Box<dyn Fn(&str) -> Result<String, MigrationError> + Send + Sync + 'static>;

/// Registry of migrations keyed by `(from, to)`.
///
/// `MigrationRegistry::new()` is empty; consumers register migrations
/// before invoking the runner. The runner uses a greedy single-hop
/// strategy: it looks for a direct `(from, to)` entry. Chained
/// migrations require the caller to invoke the runner stepwise.
pub struct MigrationRegistry {
    entries: HashMap<(FormatVersion, FormatVersion), MigrationFn>,
}

impl MigrationRegistry {
    /// New empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a migration `from → to`.
    pub fn register(&mut self, from: FormatVersion, to: FormatVersion, f: MigrationFn) {
        self.entries.insert((from, to), f);
    }

    /// Number of registered migrations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry has no migrations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Pairs the registry currently covers.
    #[must_use]
    pub fn pairs(&self) -> Vec<(FormatVersion, FormatVersion)> {
        let mut pairs: Vec<_> = self.entries.keys().copied().collect();
        pairs.sort();
        pairs
    }

    /// Apply the registered migration for `(from, to)` to the supplied
    /// content. Returns the post-migration content.
    ///
    /// # Errors
    /// - [`MigrationError::NoMigration`] when no entry exists for the
    ///   requested step.
    /// - Whatever the migration function returns otherwise.
    pub fn migrate(
        &self,
        from: FormatVersion,
        to: FormatVersion,
        content: &str,
    ) -> Result<String, MigrationError> {
        let f = self
            .entries
            .get(&(from, to))
            .ok_or(MigrationError::NoMigration { from, to })?;
        f(content)
    }
}

impl Default for MigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MigrationRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MigrationRegistry")
            .field("len", &self.entries.len())
            .field("pairs", &self.pairs())
            .finish()
    }
}

/// Read the `version:` field out of a markdown file's frontmatter.
/// Returns the parsed [`FormatVersion`], or `DEFAULT_VERSION` (v1.0)
/// when the file has no version tag.
///
/// # Errors
/// [`MigrationError::Frontmatter`] when the frontmatter block is
/// malformed; [`MigrationError::InvalidVersion`] when the `version:`
/// value is present but unparseable.
pub fn detect_version(content: &str) -> Result<FormatVersion, MigrationError> {
    let (fm, _body) = extract_frontmatter(content)
        .map_err(|e| MigrationError::Frontmatter(e.to_string()))?;
    match fm.version {
        Some(v) => FormatVersion::parse(&v),
        None => FormatVersion::parse(DEFAULT_VERSION),
    }
}

/// One entry in the version distribution returned by [`scan_versions`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VersionTally {
    /// Version `"MAJOR.MINOR"`. `"unknown"` when frontmatter parsing
    /// failed on a particular file.
    pub version: String,
    /// Number of files at this version.
    pub count: u64,
}

/// Walk a forge root and tally how many markdown files sit at each
/// version. Walks only `.md` files; skips `.forge/`, `.git/`, and
/// hidden directories.
///
/// Returns a tally sorted by version (descending major.minor).
pub fn scan_versions(forge_root: &std::path::Path) -> std::io::Result<Vec<VersionTally>> {
    let mut counts: HashMap<String, u64> = HashMap::new();
    walk_markdown(forge_root, &mut |path| {
        let key = match std::fs::read_to_string(path) {
            Ok(content) => match detect_version(&content) {
                Ok(v) => v.to_string_compact(),
                Err(_) => "unknown".to_string(),
            },
            Err(_) => return,
        };
        *counts.entry(key).or_insert(0) += 1;
    })?;

    let mut tally: Vec<VersionTally> = counts
        .into_iter()
        .map(|(version, count)| VersionTally { version, count })
        .collect();
    // Sort descending by parsed version; unparseable strings sort last.
    tally.sort_by(|a, b| {
        let av = FormatVersion::parse(&a.version).ok();
        let bv = FormatVersion::parse(&b.version).ok();
        match (av, bv) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.version.cmp(&b.version),
        }
    });
    Ok(tally)
}

fn walk_markdown(
    root: &std::path::Path,
    visit: &mut dyn FnMut(&std::path::Path),
) -> std::io::Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(root)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Skip hidden directories (`.forge`, `.git`, …) and files.
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_markdown(&path, visit)?;
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            visit(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_major_minor() {
        assert_eq!(FormatVersion::parse("1.0").unwrap(), FormatVersion::new(1, 0));
        assert_eq!(FormatVersion::parse("2.5").unwrap(), FormatVersion::new(2, 5));
    }

    #[test]
    fn parse_accepts_and_drops_patch() {
        assert_eq!(
            FormatVersion::parse("1.0.3").unwrap(),
            FormatVersion::new(1, 0)
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        for bad in ["", "1", "v1.0", "1.x", "a.b", "1.0.0.0"] {
            assert!(FormatVersion::parse(bad).is_err(), "should reject `{bad}`");
        }
    }

    #[test]
    fn version_round_trips_through_string() {
        let v = FormatVersion::new(2, 3);
        assert_eq!(v.to_string_compact(), "2.3");
        assert_eq!(format!("{v}"), "2.3");
    }

    #[test]
    fn version_orders_lexicographically_by_major_then_minor() {
        let v10 = FormatVersion::new(1, 0);
        let v15 = FormatVersion::new(1, 5);
        let v20 = FormatVersion::new(2, 0);
        assert!(v10 < v15);
        assert!(v15 < v20);
        assert!(v10 < v20);
    }

    #[test]
    fn detect_version_defaults_to_v1_0_when_absent() {
        let md = "# No frontmatter here\n";
        assert_eq!(detect_version(md).unwrap(), FormatVersion::new(1, 0));
    }

    #[test]
    fn detect_version_reads_frontmatter_field() {
        let md = "---\nversion: 2.0\n---\n# Body\n";
        assert_eq!(detect_version(md).unwrap(), FormatVersion::new(2, 0));
    }

    #[test]
    fn detect_version_errors_on_invalid_version_value() {
        let md = "---\nversion: nope\n---\n# Body\n";
        assert!(detect_version(md).is_err());
    }

    #[test]
    fn registry_is_empty_by_default() {
        let reg = MigrationRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.pairs().is_empty());
    }

    #[test]
    fn registry_dispatches_registered_migration() {
        let mut reg = MigrationRegistry::new();
        reg.register(
            FormatVersion::new(1, 0),
            FormatVersion::new(2, 0),
            Box::new(|c| Ok(c.replace("v1", "v2"))),
        );
        let out = reg
            .migrate(FormatVersion::new(1, 0), FormatVersion::new(2, 0), "hello v1")
            .unwrap();
        assert_eq!(out, "hello v2");
    }

    #[test]
    fn registry_returns_no_migration_for_unknown_pair() {
        let reg = MigrationRegistry::new();
        let err = reg
            .migrate(FormatVersion::new(1, 0), FormatVersion::new(2, 0), "x")
            .expect_err("should fail");
        match err {
            MigrationError::NoMigration { from, to } => {
                assert_eq!(from, FormatVersion::new(1, 0));
                assert_eq!(to, FormatVersion::new(2, 0));
            }
            other => panic!("expected NoMigration, got {other:?}"),
        }
    }

    #[test]
    fn registry_surfaces_migration_function_errors() {
        let mut reg = MigrationRegistry::new();
        reg.register(
            FormatVersion::new(1, 0),
            FormatVersion::new(2, 0),
            Box::new(|_| Err(MigrationError::MigrationFailed {
                from: FormatVersion::new(1, 0),
                to: FormatVersion::new(2, 0),
                reason: "boom".to_string(),
            })),
        );
        let err = reg
            .migrate(FormatVersion::new(1, 0), FormatVersion::new(2, 0), "x")
            .expect_err("should fail");
        assert!(matches!(err, MigrationError::MigrationFailed { .. }));
    }

    #[test]
    fn scan_walks_only_markdown_files_and_skips_hidden_dirs() {
        let tmp = std::env::temp_dir().join(format!(
            "nexus-migration-scan-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(tmp.join("notes")).unwrap();
        std::fs::create_dir_all(tmp.join(".forge")).unwrap();
        std::fs::write(tmp.join("notes/a.md"), "# untagged\n").unwrap();
        std::fs::write(
            tmp.join("notes/b.md"),
            "---\nversion: 2.0\n---\n# body\n",
        )
        .unwrap();
        std::fs::write(tmp.join("notes/c.md"), "---\nversion: 1.0\n---\n# body\n").unwrap();
        std::fs::write(tmp.join("notes/skip.txt"), "not markdown\n").unwrap();
        std::fs::write(tmp.join(".forge/internal.md"), "# hidden\n").unwrap();

        let tally = scan_versions(&tmp).unwrap();
        // Three .md files visible (the hidden one + the .txt are
        // skipped). Two at v1.0 (untagged + explicit), one at v2.0.
        let lookup: HashMap<String, u64> =
            tally.iter().map(|t| (t.version.clone(), t.count)).collect();
        assert_eq!(lookup.get("1.0").copied(), Some(2));
        assert_eq!(lookup.get("2.0").copied(), Some(1));
        assert_eq!(lookup.get("unknown").copied(), None);
        // Sort places higher versions first.
        assert_eq!(tally[0].version, "2.0");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_records_unknown_for_unparseable_frontmatter_versions() {
        let tmp = std::env::temp_dir().join(format!(
            "nexus-migration-scan-bad-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("bad.md"), "---\nversion: not-a-version\n---\n# body\n")
            .unwrap();
        let tally = scan_versions(&tmp).unwrap();
        assert_eq!(tally.len(), 1);
        assert_eq!(tally[0].version, "unknown");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
