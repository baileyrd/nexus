//! `<forge>/.forge/lsp.toml` parser.
//!
//! Schema (PRD-08 / BL-076 §"Server config"):
//!
//! ```toml
//! [[servers]]
//! name = "rust-analyzer"
//! command = "rust-analyzer"
//! args = []
//! file_types = ["rs"]
//! root_markers = ["Cargo.toml"]   # optional
//! disabled = false                # optional, default false
//!
//! [servers.env]                   # optional
//! RUST_LOG = "error"
//! ```
//!
//! The shape is array-of-tables rather than the keyed-map shape `mcp.toml`
//! uses because LSP server names map 1:1 to a single command-line tool —
//! there's no community-known short name we want to lift to a TOML key.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

/// Errors returned by [`LspHostConfig::read_from`].
#[derive(Debug, Error)]
pub enum LspConfigError {
    /// The config file does not exist or cannot be read.
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
    /// Two server entries share the same `name`.
    #[error("duplicate server name '{name}' in lsp.toml")]
    DuplicateServer {
        /// Conflicting server name.
        name: String,
    },
    /// A server entry has an empty `name` or `command`.
    #[error("server entry missing required field '{field}'")]
    MissingField {
        /// Field that was empty / absent.
        field: &'static str,
    },
}

/// One configured LSP server.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LspServerSpec {
    /// Stable identifier (e.g. `"rust-analyzer"`). Used as the IPC
    /// argument name in [`crate::core_plugin`] handlers.
    pub name: String,
    /// Executable to spawn — looked up on `$PATH` if not absolute.
    pub command: String,
    /// CLI args appended to `command`.
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions (no leading dot) this server handles.
    /// E.g. `["rs"]` for rust-analyzer; `["ts", "tsx", "js", "jsx"]` for
    /// `typescript-language-server`. The host uses this to route a file
    /// path to the right server.
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Workspace-root marker file names (e.g. `Cargo.toml`,
    /// `package.json`). The host walks parents of the opened file and
    /// uses the deepest directory containing any marker as the
    /// `rootUri` in the LSP `initialize` request. Defaults to the forge
    /// root when no marker matches.
    #[serde(default)]
    pub root_markers: Vec<String>,
    /// Set `true` to keep the entry in the file but skip spawning.
    #[serde(default)]
    pub disabled: bool,
    /// Extra environment passed to the child process. Merged on top of
    /// the host process's environment.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Parsed `lsp.toml`.
#[derive(Debug, Clone, Default)]
pub struct LspHostConfig {
    /// Servers keyed by [`LspServerSpec::name`] for O(1) lookup.
    pub servers: HashMap<String, LspServerSpec>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLspConfig {
    #[serde(default)]
    servers: Vec<LspServerSpec>,
}

impl LspHostConfig {
    /// Read `lsp.toml` from `path`. A missing file produces an empty
    /// config (`Ok(Self::default())`) rather than an error so a forge
    /// without LSP servers boots cleanly.
    ///
    /// # Errors
    /// Returns [`LspConfigError`] for I/O failures other than not-found,
    /// parse failures, duplicate server names, or empty required fields.
    pub fn read_from(path: &Path) -> Result<Self, LspConfigError> {
        let raw_text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(LspConfigError::Io {
                    path: path.display().to_string(),
                    source: e,
                });
            }
        };
        let raw: RawLspConfig = toml::from_str(&raw_text).map_err(|e| LspConfigError::Parse {
            path: path.display().to_string(),
            source: e,
        })?;
        let mut servers = HashMap::with_capacity(raw.servers.len());
        for spec in raw.servers {
            if spec.name.trim().is_empty() {
                return Err(LspConfigError::MissingField { field: "name" });
            }
            if spec.command.trim().is_empty() {
                return Err(LspConfigError::MissingField { field: "command" });
            }
            if servers.contains_key(&spec.name) {
                return Err(LspConfigError::DuplicateServer { name: spec.name });
            }
            servers.insert(spec.name.clone(), spec);
        }
        Ok(Self { servers })
    }

    /// Find the server (if any) whose `file_types` covers the file
    /// extension of `path`. Disabled servers are skipped. Iteration is
    /// in arbitrary [`HashMap`] order — if a forge wants a deterministic
    /// pick the convention is one server per extension.
    #[must_use]
    pub fn server_for_path(&self, path: &str) -> Option<&LspServerSpec> {
        let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        self.servers
            .values()
            .find(|s| !s.disabled && s.file_types.iter().any(|t| t.eq_ignore_ascii_case(&ext)))
    }

    /// BL-113 / ADR 0027 — merge plugin-contributed adapters into the
    /// in-memory server map. Each input pair is `(spec, plugin_id)`
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
        contributions: Vec<(LspServerSpec, String)>,
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
            if self.servers.contains_key(&spec.name) {
                skipped.push(MergeSkip {
                    name: spec.name,
                    plugin_id,
                    reason: MergeSkipReason::TomlOverride,
                });
                continue;
            }
            self.servers.insert(spec.name.clone(), spec);
        }
        skipped
    }
}

/// Why a single contribution was dropped during
/// [`LspHostConfig::merge_contributed`]. Carries the conflicting
/// `name` + the contributing `plugin_id` for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeSkip {
    /// The contribution's `name` (may be empty when [`MergeSkipReason::InvalidName`]).
    pub name: String,
    /// Reverse-DNS id of the contributing plugin.
    pub plugin_id: String,
    /// Reason the contribution was not accepted.
    pub reason: MergeSkipReason,
}

/// Per-contribution skip reason surfaced by [`LspHostConfig::merge_contributed`].
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
        let p = dir.join("lsp.toml");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn missing_file_yields_empty_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");
        let cfg = LspHostConfig::read_from(&path).unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn parses_two_server_block() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[servers]]
name = "rust-analyzer"
command = "rust-analyzer"
file_types = ["rs"]
root_markers = ["Cargo.toml"]

[[servers]]
name = "tsserver"
command = "typescript-language-server"
args = ["--stdio"]
file_types = ["ts", "tsx", "js", "jsx"]
disabled = true
"#,
        );
        let cfg = LspHostConfig::read_from(&path).unwrap();
        assert_eq!(cfg.servers.len(), 2);
        let ra = &cfg.servers["rust-analyzer"];
        assert_eq!(ra.command, "rust-analyzer");
        assert_eq!(ra.file_types, vec!["rs"]);
        assert_eq!(ra.root_markers, vec!["Cargo.toml"]);
        assert!(!ra.disabled);
        let ts = &cfg.servers["tsserver"];
        assert!(ts.disabled);
        assert_eq!(ts.args, vec!["--stdio"]);
    }

    #[test]
    fn duplicate_server_name_errors() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[servers]]
name = "ra"
command = "rust-analyzer"

[[servers]]
name = "ra"
command = "another"
"#,
        );
        let err = LspHostConfig::read_from(&path).unwrap_err();
        assert!(matches!(err, LspConfigError::DuplicateServer { .. }));
    }

    #[test]
    fn empty_command_rejected() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[servers]]
name = "ra"
command = ""
"#,
        );
        let err = LspHostConfig::read_from(&path).unwrap_err();
        assert!(matches!(err, LspConfigError::MissingField { field: "command" }));
    }

    #[test]
    fn server_for_path_picks_by_extension_case_insensitive() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[servers]]
name = "ra"
command = "rust-analyzer"
file_types = ["rs"]
"#,
        );
        let cfg = LspHostConfig::read_from(&path).unwrap();
        assert_eq!(cfg.server_for_path("/tmp/x.rs").unwrap().name, "ra");
        assert_eq!(cfg.server_for_path("/tmp/X.RS").unwrap().name, "ra");
        assert!(cfg.server_for_path("/tmp/x.py").is_none());
        assert!(cfg.server_for_path("/tmp/x").is_none());
    }

    #[test]
    fn server_for_path_skips_disabled() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[servers]]
name = "ra"
command = "rust-analyzer"
file_types = ["rs"]
disabled = true
"#,
        );
        let cfg = LspHostConfig::read_from(&path).unwrap();
        assert!(cfg.server_for_path("/tmp/x.rs").is_none());
    }

    // ── BL-113 / ADR 0027 — merge_contributed ──────────────────────────────────

    fn spec(name: &str, command: &str) -> LspServerSpec {
        LspServerSpec {
            name: name.to_string(),
            command: command.to_string(),
            args: vec![],
            file_types: vec![],
            root_markers: vec![],
            disabled: false,
            env: HashMap::new(),
        }
    }

    #[test]
    fn merge_contributed_inserts_new_entries() {
        let mut cfg = LspHostConfig::default();
        let skipped = cfg.merge_contributed(vec![
            (spec("rust-analyzer", "rust-analyzer"), "community.rust".into()),
            (spec("pyright", "pyright"), "community.python".into()),
        ]);
        assert!(skipped.is_empty());
        assert_eq!(cfg.servers.len(), 2);
        assert!(cfg.servers.contains_key("rust-analyzer"));
        assert!(cfg.servers.contains_key("pyright"));
    }

    #[test]
    fn merge_contributed_toml_wins_on_name_collision() {
        let dir = tempdir().unwrap();
        let path = write_toml(
            dir.path(),
            r#"
[[servers]]
name = "ra"
command = "rust-analyzer-from-toml"
file_types = ["rs"]
"#,
        );
        let mut cfg = LspHostConfig::read_from(&path).unwrap();
        let skipped = cfg.merge_contributed(vec![(
            spec("ra", "rust-analyzer-from-plugin"),
            "community.rust".into(),
        )]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "ra");
        assert_eq!(skipped[0].plugin_id, "community.rust");
        assert_eq!(skipped[0].reason, MergeSkipReason::TomlOverride);
        // TOML entry untouched.
        assert_eq!(cfg.servers["ra"].command, "rust-analyzer-from-toml");
    }

    #[test]
    fn merge_contributed_rejects_empty_name_and_command() {
        let mut cfg = LspHostConfig::default();
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
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn merge_contributed_preserves_input_order_in_skipped() {
        let mut cfg = LspHostConfig::default();
        cfg.servers.insert("a".into(), spec("a", "from-toml"));
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
        // Only "b" landed.
        assert_eq!(cfg.servers.len(), 2);
        assert!(cfg.servers.contains_key("b"));
    }
}
