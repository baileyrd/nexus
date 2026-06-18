//! Operator-facing OS-sandbox configuration, loaded from
//! `<forge>/.forge/sandbox.toml`.
//!
//! Pairs the default process [`SandboxPolicy`] with the brokered-download
//! [`DownloadPolicy`]. Both default to the safe/closed settings (read-only,
//! no network; downloads disabled), so a forge with no `sandbox.toml` — or a
//! malformed one — fails *closed*.

use std::path::Path;

use nexus_types::SandboxPolicy;
use serde::{Deserialize, Serialize};

use crate::downloads::DownloadPolicy;

/// Path to the sandbox config, relative to the forge root.
pub const SANDBOX_CONFIG_RELPATH: &str = ".forge/sandbox.toml";

/// The forge's OS-sandbox configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SandboxConfig {
    /// Default policy applied to spawned child processes.
    pub policy: SandboxPolicy,
    /// Permissioned-download broker policy.
    pub downloads: DownloadPolicy,
    /// When true, *sandboxed* terminal sessions launch the bundled `nexus-rush`
    /// shell instead of the detected system shell (RFC 0002). Off by default —
    /// the system shell stays the default everywhere until rush hardens (RFC
    /// 0002 Stage 1). Non-sandboxed sessions are unaffected regardless. The
    /// terminal layer consumes this via `SessionConfig::bundled_shell`, threaded
    /// by the caller that builds the session config (kept out of `nexus-terminal`
    /// itself so it need not depend on `nexus-security`).
    pub bundled_shell_for_sandbox: bool,
}

impl SandboxConfig {
    /// Load from `<forge_root>/.forge/sandbox.toml`. A missing file yields the
    /// defaults; a malformed file logs a warning and *also* yields the defaults
    /// (fail closed — never silently widen the sandbox).
    #[must_use]
    pub fn load(forge_root: &Path) -> Self {
        let path = forge_root.join(SANDBOX_CONFIG_RELPATH);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "sandbox.toml parse failed; falling back to the closed defaults"
                );
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_closed() {
        let cfg = SandboxConfig::default();
        assert_eq!(cfg.policy, SandboxPolicy::ReadOnly);
        assert!(!cfg.downloads.enabled);
        // Bundled shell is opt-in: off by default (system shell is the default).
        assert!(!cfg.bundled_shell_for_sandbox);
    }

    #[test]
    fn parses_bundled_shell_opt_in() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(
            dir.path().join(SANDBOX_CONFIG_RELPATH),
            "bundled_shell_for_sandbox = true\n",
        )
        .unwrap();
        assert!(SandboxConfig::load(dir.path()).bundled_shell_for_sandbox);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = SandboxConfig::load(dir.path());
        assert_eq!(cfg, SandboxConfig::default());
    }

    #[test]
    fn parses_a_workspace_write_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(
            dir.path().join(SANDBOX_CONFIG_RELPATH),
            r#"
[policy]
mode = "workspace-write"
writable_roots = ["/data"]
network_access = false

[downloads]
enabled = true
allowed_hosts = ["static.crates.io"]
max_bytes = 2048
"#,
        )
        .unwrap();
        let cfg = SandboxConfig::load(dir.path());
        assert!(matches!(cfg.policy, SandboxPolicy::WorkspaceWrite { .. }));
        assert!(cfg.downloads.enabled);
        assert_eq!(cfg.downloads.allowed_hosts, vec!["static.crates.io"]);
        assert_eq!(cfg.downloads.max_bytes, 2048);
    }

    #[test]
    fn malformed_file_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(dir.path().join(SANDBOX_CONFIG_RELPATH), "this is not = valid toml [[[")
            .unwrap();
        // Fails closed: defaults, not a panic or a widened sandbox.
        assert_eq!(SandboxConfig::load(dir.path()), SandboxConfig::default());
    }
}
