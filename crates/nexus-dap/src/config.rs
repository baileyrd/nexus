//! `<forge>/.forge/dap.toml` parser.
//!
//! Schema:
//!
//! ```toml
//! [[adapters]]
//! name = "rust"
//! command = "codelldb"
//! args = ["--port", "0"]            # optional
//! type = "lldb"                     # optional cosmetic hint
//! file_types = ["rs", "c", "cpp"]   # optional
//! disabled = false                  # optional
//!
//! [adapters.env]                    # optional
//! RUST_BACKTRACE = "1"
//! ```
//!
//! Same shape as `lsp.toml`: array-of-tables keyed by `name`. A missing
//! file produces an empty config so a forge with no adapters boots
//! cleanly.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

/// Errors returned by [`DapHostConfig::read_from`].
#[derive(Debug, Error)]
pub enum DapConfigError {
    /// The config file exists but cannot be read.
    #[error("io reading {path}: {source}")]
    Io {
        /// Path that failed to open.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// TOML failed to parse.
    #[error("parsing {path}: {source}")]
    Parse {
        /// Path that failed to parse.
        path: String,
        /// Underlying parse error.
        #[source]
        source: toml::de::Error,
    },
    /// Two adapter entries share the same `name`.
    #[error("duplicate adapter name '{name}' in dap.toml")]
    DuplicateAdapter {
        /// Conflicting adapter name.
        name: String,
    },
    /// A required field is empty.
    #[error("adapter entry missing required field '{field}'")]
    MissingField {
        /// Field that was empty / absent.
        field: &'static str,
    },
}

/// One configured DAP adapter.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DapAdapterSpec {
    /// Stable identifier used as the `adapter` IPC argument.
    pub name: String,
    /// Executable to spawn — looked up on `$PATH` if not absolute.
    pub command: String,
    /// CLI args appended to `command`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Cosmetic hint (`"lldb"`, `"node"`, `"python"`, …). Not used for
    /// routing; consumers may surface it in the UI.
    #[serde(default)]
    #[serde(rename = "type")]
    pub adapter_type: Option<String>,
    /// File extensions this adapter understands. Used by
    /// [`DapHostConfig::adapter_for_path`].
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Set `true` to keep the entry in the file but skip spawning.
    #[serde(default)]
    pub disabled: bool,
    /// Extra environment passed to the child process. Merged on top of
    /// the host process's environment.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Parsed `dap.toml`.
#[derive(Debug, Clone, Default)]
pub struct DapHostConfig {
    /// Adapters keyed by [`DapAdapterSpec::name`] for O(1) lookup.
    pub adapters: HashMap<String, DapAdapterSpec>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDapConfig {
    #[serde(default)]
    adapters: Vec<DapAdapterSpec>,
}

impl DapHostConfig {
    /// Read `dap.toml` from `path`. A missing file produces an empty
    /// config rather than an error so a forge without DAP adapters
    /// boots cleanly.
    ///
    /// # Errors
    /// Returns [`DapConfigError`] for I/O failures other than
    /// not-found, parse failures, duplicate names, or empty required
    /// fields.
    pub fn read_from(path: &Path) -> Result<Self, DapConfigError> {
        let raw_text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(DapConfigError::Io {
                    path: path.display().to_string(),
                    source: e,
                });
            }
        };
        let raw: RawDapConfig = toml::from_str(&raw_text).map_err(|e| DapConfigError::Parse {
            path: path.display().to_string(),
            source: e,
        })?;
        let mut adapters = HashMap::with_capacity(raw.adapters.len());
        for spec in raw.adapters {
            if spec.name.trim().is_empty() {
                return Err(DapConfigError::MissingField { field: "name" });
            }
            if spec.command.trim().is_empty() {
                return Err(DapConfigError::MissingField { field: "command" });
            }
            if adapters.contains_key(&spec.name) {
                return Err(DapConfigError::DuplicateAdapter { name: spec.name });
            }
            adapters.insert(spec.name.clone(), spec);
        }
        Ok(Self { adapters })
    }

    /// Find the first enabled adapter that handles the extension of
    /// `path`. Iteration is in arbitrary `HashMap` order; convention
    /// is one adapter per extension.
    #[must_use]
    pub fn adapter_for_path(&self, path: &str) -> Option<&DapAdapterSpec> {
        let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        self.adapters
            .values()
            .find(|s| !s.disabled && s.file_types.iter().any(|t| t.eq_ignore_ascii_case(&ext)))
    }

    /// BL-113 / ADR 0027 — merge plugin-contributed adapters into the
    /// in-memory adapter map. Each input pair is `(spec, plugin_id)`
    /// where `plugin_id` is the contributing plugin's reverse-DNS id
    /// (used for diagnostics + future capability gating).
    ///
    /// Precedence is **TOML wins**: when a contributed adapter shares
    /// its `name` with a TOML-loaded entry, the TOML entry stays and
    /// the contribution is reported as skipped. This matches the
    /// ADR 0027 §Migration "legacy fallback during the deprecation
    /// window" stance.
    ///
    /// Contributed adapters whose `name` or `command` is empty after
    /// trimming are also skipped (same validation as
    /// [`read_from`]). The returned [`Vec<MergeSkip>`] is empty when
    /// every contribution was accepted; it preserves the input order
    /// otherwise so a caller can log conflicts in the order plugins
    /// surfaced them.
    pub fn merge_contributed(
        &mut self,
        contributions: Vec<(DapAdapterSpec, String)>,
    ) -> Vec<MergeSkip> {
        let mut skipped = Vec::new();
        for (spec, plugin_id) in contributions {
            if spec.name.trim().is_empty() {
                skipped.push(MergeSkip {
                    name: spec.name,
                    plugin_id,
                    reason: MergeSkipReason::InvalidName,
                });
                continue;
            }
            if spec.command.trim().is_empty() {
                skipped.push(MergeSkip {
                    name: spec.name,
                    plugin_id,
                    reason: MergeSkipReason::InvalidCommand,
                });
                continue;
            }
            if self.adapters.contains_key(&spec.name) {
                skipped.push(MergeSkip {
                    name: spec.name,
                    plugin_id,
                    reason: MergeSkipReason::TomlOverride,
                });
                continue;
            }
            self.adapters.insert(spec.name.clone(), spec);
        }
        skipped
    }
}

/// Why a single contribution was dropped during
/// [`DapHostConfig::merge_contributed`]. Carries the conflicting
/// `name` + the contributing `plugin_id` for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeSkip {
    /// The contribution's `name` (may be empty when
    /// [`MergeSkipReason::InvalidName`]).
    pub name: String,
    /// Reverse-DNS id of the contributing plugin.
    pub plugin_id: String,
    /// Reason the contribution was not accepted.
    pub reason: MergeSkipReason,
}

/// Per-contribution skip reason surfaced by
/// [`DapHostConfig::merge_contributed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeSkipReason {
    /// A TOML-loaded entry already owns this `name`.
    TomlOverride,
    /// `name` was empty / whitespace-only.
    InvalidName,
    /// `command` was empty / whitespace-only.
    InvalidCommand,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_toml(dir: &Path, body: &str) -> std::path::PathBuf {
        let p = dir.join("dap.toml");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn missing_file_yields_empty_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");
        let cfg = DapHostConfig::read_from(&path).unwrap();
        assert!(cfg.adapters.is_empty());
    }

    #[test]
    fn parses_two_adapter_block() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[adapters]]
name = "rust"
command = "codelldb"
file_types = ["rs"]

[[adapters]]
name = "node"
command = "js-debug"
args = ["--server", "0"]
type = "node"
file_types = ["js", "ts"]
disabled = true
"#,
        );
        let cfg = DapHostConfig::read_from(&path).unwrap();
        assert_eq!(cfg.adapters.len(), 2);
        let rust = &cfg.adapters["rust"];
        assert_eq!(rust.command, "codelldb");
        assert_eq!(rust.file_types, vec!["rs"]);
        assert!(!rust.disabled);
        let node = &cfg.adapters["node"];
        assert!(node.disabled);
        assert_eq!(node.args, vec!["--server", "0"]);
        assert_eq!(node.adapter_type.as_deref(), Some("node"));
    }

    #[test]
    fn duplicate_adapter_name_errors() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[adapters]]
name = "x"
command = "a"

[[adapters]]
name = "x"
command = "b"
"#,
        );
        let err = DapHostConfig::read_from(&path).unwrap_err();
        assert!(matches!(err, DapConfigError::DuplicateAdapter { .. }));
    }

    #[test]
    fn empty_command_rejected() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[adapters]]
name = "x"
command = ""
"#,
        );
        let err = DapHostConfig::read_from(&path).unwrap_err();
        assert!(matches!(err, DapConfigError::MissingField { field: "command" }));
    }

    #[test]
    fn adapter_for_path_picks_by_extension_case_insensitive() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[adapters]]
name = "rust"
command = "codelldb"
file_types = ["rs"]
"#,
        );
        let cfg = DapHostConfig::read_from(&path).unwrap();
        assert_eq!(cfg.adapter_for_path("/tmp/x.rs").unwrap().name, "rust");
        assert_eq!(cfg.adapter_for_path("/tmp/X.RS").unwrap().name, "rust");
        assert!(cfg.adapter_for_path("/tmp/x.py").is_none());
        assert!(cfg.adapter_for_path("/tmp/x").is_none());
    }

    #[test]
    fn adapter_for_path_skips_disabled() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[adapters]]
name = "rust"
command = "codelldb"
file_types = ["rs"]
disabled = true
"#,
        );
        let cfg = DapHostConfig::read_from(&path).unwrap();
        assert!(cfg.adapter_for_path("/tmp/x.rs").is_none());
    }

    // ── BL-113 / ADR 0027 — merge_contributed ──────────────────────────────────

    fn spec(name: &str, command: &str) -> DapAdapterSpec {
        DapAdapterSpec {
            name: name.to_string(),
            command: command.to_string(),
            args: vec![],
            adapter_type: None,
            file_types: vec![],
            disabled: false,
            env: HashMap::new(),
        }
    }

    #[test]
    fn merge_contributed_inserts_new_entries() {
        let mut cfg = DapHostConfig::default();
        let skipped = cfg.merge_contributed(vec![
            (spec("rust", "codelldb"), "community.rust".into()),
            (spec("node", "js-debug"), "community.node".into()),
        ]);
        assert!(skipped.is_empty());
        assert_eq!(cfg.adapters.len(), 2);
        assert!(cfg.adapters.contains_key("rust"));
        assert!(cfg.adapters.contains_key("node"));
    }

    #[test]
    fn merge_contributed_toml_wins_on_name_collision() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[adapters]]
name = "rust"
command = "codelldb-from-toml"
file_types = ["rs"]
"#,
        );
        let mut cfg = DapHostConfig::read_from(&path).unwrap();
        let skipped = cfg.merge_contributed(vec![(
            spec("rust", "codelldb-from-plugin"),
            "community.rust".into(),
        )]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "rust");
        assert_eq!(skipped[0].plugin_id, "community.rust");
        assert_eq!(skipped[0].reason, MergeSkipReason::TomlOverride);
        // TOML entry untouched.
        assert_eq!(cfg.adapters["rust"].command, "codelldb-from-toml");
    }

    #[test]
    fn merge_contributed_rejects_empty_name_and_command() {
        let mut cfg = DapHostConfig::default();
        let skipped = cfg.merge_contributed(vec![
            (spec("", "x"), "community.a".into()),
            (spec("   ", "x"), "community.b".into()),
            (spec("ok", ""), "community.c".into()),
            (spec("ok2", "  "), "community.d".into()),
        ]);
        assert_eq!(skipped.len(), 4);
        assert_eq!(skipped[0].reason, MergeSkipReason::InvalidName);
        assert_eq!(skipped[1].reason, MergeSkipReason::InvalidName);
        assert_eq!(skipped[2].reason, MergeSkipReason::InvalidCommand);
        assert_eq!(skipped[3].reason, MergeSkipReason::InvalidCommand);
        assert!(cfg.adapters.is_empty());
    }

    #[test]
    fn merge_contributed_preserves_input_order_in_skipped() {
        let mut cfg = DapHostConfig::default();
        cfg.adapters.insert("a".into(), spec("a", "from-toml"));
        let skipped = cfg.merge_contributed(vec![
            (spec("a", "from-plugin"), "p1".into()),
            (spec("b", "ok"), "p2".into()),
            (spec("", "bad"), "p3".into()),
            (spec("a", "from-plugin-2"), "p4".into()),
        ]);
        assert_eq!(skipped.len(), 3);
        assert_eq!(skipped[0].plugin_id, "p1");
        assert_eq!(skipped[1].plugin_id, "p3");
        assert_eq!(skipped[2].plugin_id, "p4");
        // Only "b" landed (plus the TOML "a").
        assert_eq!(cfg.adapters.len(), 2);
        assert!(cfg.adapters.contains_key("b"));
        assert_eq!(cfg.adapters["a"].command, "from-toml");
    }
}
