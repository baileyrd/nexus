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
}
