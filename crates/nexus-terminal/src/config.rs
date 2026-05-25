//! `<forge>/.forge/terminal.toml` — terminal-service forge config.
//!
//! Today this carries a single `[spawn]` block: the authoritative
//! env-hygiene default ([`nexus_types::SpawnPolicy`]) applied to every
//! session the plugin spawns. A per-call `create_session` policy may
//! only *tighten* this default, never loosen it — so a forge can mandate
//! a clean environment for all terminals and no IPC caller can opt back
//! out.
//!
//! Missing file → [`TerminalConfig::default`] (a permissive, no-op
//! policy), so a forge without the file behaves exactly as it did before
//! this config existed.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use nexus_types::SpawnPolicy;

/// Parsed contents of `<forge>/.forge/terminal.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Authoritative env-hygiene default for spawned sessions.
    pub spawn: SpawnPolicy,
}

/// Failure modes for loading `terminal.toml`.
#[derive(Debug, Error)]
pub enum TerminalConfigError {
    /// The file exists but could not be read.
    #[error("reading {path}: {source}")]
    Io {
        /// Display path of the file we tried to read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The file exists but is not valid TOML / does not match the schema.
    #[error("parsing {path}: {source}")]
    Toml {
        /// Display path of the file we tried to parse.
        path: String,
        /// Underlying TOML deserialization error.
        source: toml::de::Error,
    },
}

impl TerminalConfig {
    /// Parse TOML text. `source` is the display path used in errors.
    ///
    /// # Errors
    /// Returns [`TerminalConfigError::Toml`] on malformed TOML.
    pub fn from_str(text: &str, source: &str) -> Result<Self, TerminalConfigError> {
        toml::from_str(text).map_err(|e| TerminalConfigError::Toml {
            path: source.to_string(),
            source: e,
        })
    }

    /// Read and parse a file on disk. A missing file yields
    /// [`TerminalConfig::default`] — the terminal service must remain
    /// functional without the config present.
    ///
    /// # Errors
    /// Returns [`TerminalConfigError::Io`] on read failures other than
    /// `NotFound`, or [`TerminalConfigError::Toml`] on parse failures.
    pub fn read_from(path: &Path) -> Result<Self, TerminalConfigError> {
        let source = path.display().to_string();
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(TerminalConfigError::Io {
                    path: source,
                    source: e,
                })
            }
        };
        Self::from_str(&text, &source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_default_to_permissive() {
        let cfg = TerminalConfig::from_str("", "<test>").unwrap();
        assert!(cfg.spawn.is_noop());
    }

    #[test]
    fn parses_spawn_block() {
        let toml = r#"
            [spawn]
            clean_env = true
            env_allowlist = ["PATH", "HOME"]
            env_denylist = ["AWS_SECRET_ACCESS_KEY"]
        "#;
        let cfg = TerminalConfig::from_str(toml, "<test>").unwrap();
        assert!(cfg.spawn.clean_env);
        assert_eq!(cfg.spawn.env_allowlist, vec!["PATH", "HOME"]);
        assert_eq!(cfg.spawn.env_denylist, vec!["AWS_SECRET_ACCESS_KEY"]);
    }

    #[test]
    fn read_from_missing_path_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = TerminalConfig::read_from(&dir.path().join("terminal.toml")).unwrap();
        assert_eq!(cfg, TerminalConfig::default());
    }

    #[test]
    fn malformed_toml_errors() {
        let err = TerminalConfig::from_str("spawn = [not valid", "<test>");
        assert!(matches!(err, Err(TerminalConfigError::Toml { .. })));
    }
}
