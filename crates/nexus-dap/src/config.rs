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
    /// BL-113 — opaque shell-facing payload set at contribution-wire
    /// time. The host never interprets it; it survives a round-trip
    /// through `list_adapters` so the shell can render a typed
    /// launch-config form from the contributing plugin's schema.
    ///
    /// Currently populated by `nexus-bootstrap::dap_contribution_to_spec`
    /// with `{"launch_config_schema": <inline JSON Schema>, ...}` when
    /// the contributing plugin declares `launch_config_schema` in its
    /// manifest. TOML-loaded entries always have `metadata = None`
    /// since `dap.toml` has no equivalent field — keeps the existing
    /// `deny_unknown_fields` posture intact for TOML.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Parsed `dap.toml` plus runtime-merged plugin contributions.
///
/// TOML-loaded entries and plugin-contributed entries share the
/// [`adapters`] map; the [`contributed_by`] map distinguishes them
/// for unregister authorisation. See BL-113 / ADR 0027.
///
/// [`adapters`]: Self::adapters
/// [`contributed_by`]: Self::contributed_by
#[derive(Debug, Clone, Default)]
pub struct DapHostConfig {
    /// Adapters keyed by [`DapAdapterSpec::name`] for O(1) lookup.
    pub adapters: HashMap<String, DapAdapterSpec>,
    /// BL-113 Phase 1b — maps adapter `name` to the contributing plugin's
    /// reverse-DNS id for adapters that came through
    /// [`merge_contributed`] / [`register_contributed`]. TOML-loaded
    /// entries do not appear here, so the host can refuse a plugin's
    /// `unregister_adapter` against a TOML-pinned name.
    ///
    /// [`merge_contributed`]: Self::merge_contributed
    /// [`register_contributed`]: Self::register_contributed
    pub contributed_by: HashMap<String, String>,
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
        Ok(Self {
            adapters,
            contributed_by: HashMap::new(),
        })
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
            if let Err(reason) = self.register_contributed(spec.clone(), plugin_id.clone()) {
                skipped.push(MergeSkip {
                    name: spec.name,
                    plugin_id,
                    reason,
                });
            }
        }
        skipped
    }

    /// BL-113 Phase 1b — single-spec variant of [`merge_contributed`],
    /// the inner per-contribution rule the batch merge calls into and
    /// the entry point the `com.nexus.dap::register_adapter` IPC verb
    /// dispatches to at runtime.
    ///
    /// Validates `spec.name` / `spec.command` (same rules as
    /// [`merge_contributed`]), refuses a name that any existing entry
    /// already owns (TOML or plugin-contributed alike — plugins must
    /// `unregister_adapter` before re-registering), inserts on success,
    /// and records the contributing plugin in
    /// [`contributed_by`](Self::contributed_by).
    ///
    /// # Errors
    /// Returns a [`MergeSkipReason`] when the spec fails validation or
    /// collides with an existing entry. On `Err`, the config is
    /// unchanged.
    pub fn register_contributed(
        &mut self,
        spec: DapAdapterSpec,
        plugin_id: String,
    ) -> Result<(), MergeSkipReason> {
        if spec.name.trim().is_empty() {
            return Err(MergeSkipReason::InvalidName);
        }
        if spec.command.trim().is_empty() {
            return Err(MergeSkipReason::InvalidCommand);
        }
        if self.adapters.contains_key(&spec.name) {
            return Err(MergeSkipReason::TomlOverride);
        }
        let name = spec.name.clone();
        self.adapters.insert(name.clone(), spec);
        self.contributed_by.insert(name, plugin_id);
        Ok(())
    }

    /// BL-113 Phase 1b — remove a previously contributed adapter. The
    /// `com.nexus.dap::unregister_adapter` IPC verb's host entry point.
    ///
    /// `plugin_id` must match the contributing plugin recorded in
    /// [`contributed_by`](Self::contributed_by); this gates plugins
    /// from unregistering adapters they don't own (including any
    /// TOML-pinned entry, which has no `contributed_by` row).
    ///
    /// # Errors
    /// Returns [`UnregisterError::NotFound`] when no adapter exists for
    /// `name`, [`UnregisterError::TomlEntry`] when the entry is
    /// TOML-loaded (not in `contributed_by`), and
    /// [`UnregisterError::NotOwnedByPlugin`] when the row exists but
    /// was contributed by a different plugin.
    pub fn unregister_contributed(
        &mut self,
        name: &str,
        plugin_id: &str,
    ) -> Result<DapAdapterSpec, UnregisterError> {
        match self.contributed_by.get(name) {
            None if self.adapters.contains_key(name) => Err(UnregisterError::TomlEntry),
            None => Err(UnregisterError::NotFound),
            Some(owner) if owner != plugin_id => Err(UnregisterError::NotOwnedByPlugin {
                actual_owner: owner.clone(),
            }),
            Some(_) => {
                self.contributed_by.remove(name);
                // Invariant: register_contributed inserts both maps
                // atomically under the &mut. The NotFound fallback
                // here is defensive — only reachable if a caller
                // mutates the public `adapters` field directly.
                self.adapters.remove(name).ok_or(UnregisterError::NotFound)
            }
        }
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

/// Why [`DapHostConfig::unregister_contributed`] refused. Distinguishes
/// "this name was never registered" from "this name belongs to TOML /
/// another plugin" so the IPC layer can surface a precise reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnregisterError {
    /// No adapter exists under that name.
    NotFound,
    /// The adapter exists but came from `dap.toml`, not a plugin
    /// contribution — plugins can't unregister TOML-pinned entries.
    TomlEntry,
    /// The adapter exists and was plugin-contributed, but the calling
    /// plugin isn't the one that contributed it.
    NotOwnedByPlugin {
        /// Reverse-DNS id of the plugin that actually owns the entry.
        actual_owner: String,
    },
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
        assert!(matches!(
            err,
            DapConfigError::MissingField { field: "command" }
        ));
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
            metadata: None,
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

    #[test]
    fn merge_contributed_populates_contributed_by_for_accepted_entries() {
        let mut cfg = DapHostConfig::default();
        cfg.adapters
            .insert("toml-pinned".into(), spec("toml-pinned", "x"));
        let skipped = cfg.merge_contributed(vec![
            (spec("contrib-a", "x"), "plugin.a".into()),
            (spec("contrib-b", "x"), "plugin.b".into()),
            (spec("toml-pinned", "y"), "plugin.c".into()), // rejected
        ]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(cfg.contributed_by.len(), 2);
        assert_eq!(cfg.contributed_by["contrib-a"], "plugin.a");
        assert_eq!(cfg.contributed_by["contrib-b"], "plugin.b");
        assert!(!cfg.contributed_by.contains_key("toml-pinned"));
    }

    // ── BL-113 Phase 1b — register_contributed / unregister_contributed ────────

    #[test]
    fn register_contributed_happy_path_inserts_and_records_provenance() {
        let mut cfg = DapHostConfig::default();
        assert!(cfg
            .register_contributed(spec("rust", "codelldb"), "community.rust".into())
            .is_ok());
        assert_eq!(cfg.adapters["rust"].command, "codelldb");
        assert_eq!(cfg.contributed_by["rust"], "community.rust");
    }

    #[test]
    fn register_contributed_rejects_invalid_and_collisions() {
        let mut cfg = DapHostConfig::default();
        cfg.adapters.insert("taken".into(), spec("taken", "x"));
        assert_eq!(
            cfg.register_contributed(spec("", "ok"), "p".into())
                .unwrap_err(),
            MergeSkipReason::InvalidName,
        );
        assert_eq!(
            cfg.register_contributed(spec("ok", "  "), "p".into())
                .unwrap_err(),
            MergeSkipReason::InvalidCommand,
        );
        assert_eq!(
            cfg.register_contributed(spec("taken", "y"), "p".into())
                .unwrap_err(),
            MergeSkipReason::TomlOverride,
        );
        // Plugin-contributed entries also collide.
        cfg.register_contributed(spec("contrib", "x"), "p1".into())
            .unwrap();
        assert_eq!(
            cfg.register_contributed(spec("contrib", "x"), "p2".into())
                .unwrap_err(),
            MergeSkipReason::TomlOverride,
        );
        // Failed registrations leave config untouched aside from what
        // successfully landed.
        assert_eq!(cfg.adapters.len(), 2); // "taken" + "contrib"
        assert_eq!(cfg.contributed_by.len(), 1);
        assert_eq!(cfg.contributed_by["contrib"], "p1");
    }

    #[test]
    fn unregister_contributed_removes_when_owner_matches() {
        let mut cfg = DapHostConfig::default();
        cfg.register_contributed(spec("rust", "codelldb"), "community.rust".into())
            .unwrap();
        let removed = cfg
            .unregister_contributed("rust", "community.rust")
            .unwrap();
        assert_eq!(removed.command, "codelldb");
        assert!(!cfg.adapters.contains_key("rust"));
        assert!(!cfg.contributed_by.contains_key("rust"));
    }

    #[test]
    fn unregister_contributed_distinguishes_not_found_toml_and_wrong_owner() {
        let mut cfg = DapHostConfig::default();
        cfg.adapters.insert("toml".into(), spec("toml", "x"));
        cfg.register_contributed(spec("contrib", "x"), "plugin.owner".into())
            .unwrap();
        assert_eq!(
            cfg.unregister_contributed("ghost", "anyone").unwrap_err(),
            UnregisterError::NotFound,
        );
        assert_eq!(
            cfg.unregister_contributed("toml", "anyone").unwrap_err(),
            UnregisterError::TomlEntry,
        );
        match cfg.unregister_contributed("contrib", "plugin.intruder") {
            Err(UnregisterError::NotOwnedByPlugin { actual_owner }) => {
                assert_eq!(actual_owner, "plugin.owner");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
        // Original entries untouched.
        assert!(cfg.adapters.contains_key("toml"));
        assert!(cfg.adapters.contains_key("contrib"));
        assert_eq!(cfg.contributed_by["contrib"], "plugin.owner");
    }
}
