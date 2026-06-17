//! OS-level process sandbox policy.
//!
//! This is **distinct from the WASM/iframe _plugin_ sandbox** elsewhere in the
//! tree: it describes what a spawned *operating-system* child process (a shell
//! command, an agent tool) may read, write, and reach over the network. The
//! model mirrors the Codex CLI's three escalating modes.
//!
//! Only the policy *model* lives here, in the leaf `nexus-types` crate, so that
//! `nexus-terminal`, `nexus-agent`, and `nexus-security` can share one
//! definition without a dependency cycle — exactly as [`crate::ForgePathValidator`]
//! does. The platform enforcement backends (Linux landlock + seccomp, macOS
//! seatbelt, Windows restricted tokens) live in `nexus-security`.
//!
//! Path checks here are **lexical** (`Path::starts_with`) — the model does no
//! canonicalization or symlink resolution. The enforcement layer is responsible
//! for canonicalizing paths before consulting the policy.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What a sandboxed OS process is permitted to do. Three escalating modes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum SandboxPolicy {
    /// No containment — full disk read/write and network. The escape hatch,
    /// used only with explicit operator opt-in.
    #[serde(rename = "danger-full-access")]
    DangerFullAccess,

    /// Read anything on disk; write nothing; no network. The safe default.
    #[serde(rename = "read-only")]
    ReadOnly,

    /// Read anything; write only under the workspace roots (the working
    /// directory plus any extra `writable_roots`, plus the system temp
    /// directories unless excluded); network is off unless `network_access`.
    #[serde(rename = "workspace-write")]
    WorkspaceWrite {
        /// Extra roots (beyond the cwd) the process may write to.
        #[serde(default)]
        writable_roots: Vec<PathBuf>,
        /// Allow outbound network access.
        #[serde(default)]
        network_access: bool,
        /// Do not treat `$TMPDIR` as writable.
        #[serde(default)]
        exclude_tmpdir_env_var: bool,
        /// Do not treat `/tmp` as writable.
        #[serde(default)]
        exclude_slash_tmp: bool,
    },
}

impl Default for SandboxPolicy {
    /// The safe default: [`SandboxPolicy::ReadOnly`].
    fn default() -> Self {
        Self::ReadOnly
    }
}

/// A directory a sandboxed process may write to, minus any read-only carve-outs
/// (e.g. a top-level `.git` so the agent can't rewrite VCS history).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WritableRoot {
    /// The writable directory.
    pub root: PathBuf,
    /// Subpaths under `root` that remain read-only.
    pub read_only_subpaths: Vec<PathBuf>,
}

impl WritableRoot {
    /// A writable root with no read-only carve-outs.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root, read_only_subpaths: Vec::new() }
    }

    /// True if `path` is within this root and not within a read-only subpath.
    /// Lexical only — the caller is expected to canonicalize first.
    #[must_use]
    pub fn is_path_writable(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
            && !self.read_only_subpaths.iter().any(|ro| path.starts_with(ro))
    }
}

impl SandboxPolicy {
    /// The safe default: read-only, no network.
    #[must_use]
    pub fn new_read_only() -> Self {
        Self::ReadOnly
    }

    /// Workspace-write with the given extra roots; network off, temp dirs
    /// included.
    #[must_use]
    pub fn new_workspace_write(writable_roots: Vec<PathBuf>) -> Self {
        Self::WorkspaceWrite {
            writable_roots,
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        }
    }

    /// Every mode may read the whole disk.
    #[must_use]
    pub fn has_full_disk_read_access(&self) -> bool {
        true
    }

    /// Only [`SandboxPolicy::DangerFullAccess`] may write anywhere on disk.
    #[must_use]
    pub fn has_full_disk_write_access(&self) -> bool {
        matches!(self, Self::DangerFullAccess)
    }

    /// Network is allowed for full-access, or workspace-write with the flag set.
    #[must_use]
    pub fn has_full_network_access(&self) -> bool {
        match self {
            Self::DangerFullAccess => true,
            Self::WorkspaceWrite { network_access, .. } => *network_access,
            Self::ReadOnly => false,
        }
    }

    /// True when no OS containment is applied at all.
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        matches!(self, Self::DangerFullAccess)
    }

    /// Relative permissiveness for escalation ordering: `ReadOnly` (0) <
    /// `WorkspaceWrite` (1) < `DangerFullAccess` (2). Useful for deciding
    /// whether a requested policy is an escalation over the current one.
    #[must_use]
    pub fn permissiveness(&self) -> u8 {
        match self {
            Self::ReadOnly => 0,
            Self::WorkspaceWrite { .. } => 1,
            Self::DangerFullAccess => 2,
        }
    }

    /// True if `other` grants strictly more access than `self` (an escalation
    /// that should require operator approval).
    #[must_use]
    pub fn is_escalation_over(&self, other: &Self) -> bool {
        self.permissiveness() > other.permissiveness()
    }

    /// Effective writable roots for a working directory, reading `$TMPDIR` from
    /// the environment. Empty for `ReadOnly` (writes nothing) and
    /// `DangerFullAccess` (writes everywhere, so the list is not consulted).
    #[must_use]
    pub fn writable_roots_with_cwd(&self, cwd: &Path) -> Vec<WritableRoot> {
        let tmpdir = std::env::var_os("TMPDIR").map(PathBuf::from);
        self.writable_roots_impl(cwd, tmpdir.as_deref())
    }

    /// Pure core of [`Self::writable_roots_with_cwd`] — `tmpdir` is passed
    /// explicitly so the computation is deterministic and unit-testable.
    #[must_use]
    pub fn writable_roots_impl(&self, cwd: &Path, tmpdir: Option<&Path>) -> Vec<WritableRoot> {
        let Self::WorkspaceWrite {
            writable_roots,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            ..
        } = self
        else {
            return Vec::new();
        };

        let mut out = Vec::new();
        // Workspace roots (cwd + extras) keep a read-only `.git` carve-out.
        for root in std::iter::once(cwd.to_path_buf()).chain(writable_roots.iter().cloned()) {
            let git = root.join(".git");
            out.push(WritableRoot { root, read_only_subpaths: vec![git] });
        }
        // System temp dirs are fully writable (no `.git` guard).
        if !exclude_slash_tmp && cfg!(unix) {
            out.push(WritableRoot::new(PathBuf::from("/tmp")));
        }
        if !exclude_tmpdir_env_var {
            if let Some(t) = tmpdir {
                if !t.as_os_str().is_empty() {
                    out.push(WritableRoot::new(t.to_path_buf()));
                }
            }
        }
        out
    }

    /// Whether a sandboxed process running in `cwd` may write to `path`.
    #[must_use]
    pub fn is_path_writable(&self, path: &Path, cwd: &Path) -> bool {
        match self {
            Self::DangerFullAccess => true,
            Self::ReadOnly => false,
            Self::WorkspaceWrite { .. } => {
                self.writable_roots_with_cwd(cwd).iter().any(|r| r.is_path_writable(path))
            }
        }
    }
}

/// Best-effort path to the `nexus-sandbox` helper sidecar: the binary named
/// `nexus-sandbox` alongside the current executable. Spawn sites that ship the
/// helper elsewhere should supply their own path. Pure path math — does no I/O
/// beyond reading the current exe path.
#[must_use]
pub fn default_helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name("nexus-sandbox"))
}

/// The argv to pass to the `nexus-sandbox` helper (everything *after* the
/// helper program): `[policy-json, cwd, "--", program, args…]`. Frontend-
/// agnostic, so both `std::process::Command` and portable-pty's `CommandBuilder`
/// can wrap a command the same way. Lives here (not in `nexus-security`) so a
/// spawn site can request sandboxing without linking the enforcement engine.
///
/// # Errors
/// Returns the `serde_json::Error` if the policy cannot be serialized (it
/// cannot, in practice — `SandboxPolicy` is a plain derived `Serialize`).
pub fn sandbox_argv<P, A>(
    policy: &SandboxPolicy,
    cwd: &Path,
    program: P,
    args: A,
) -> Result<Vec<OsString>, serde_json::Error>
where
    P: AsRef<OsStr>,
    A: IntoIterator,
    A::Item: AsRef<OsStr>,
{
    let policy_json = serde_json::to_string(policy)?;
    let mut argv: Vec<OsString> = Vec::with_capacity(5);
    argv.push(OsString::from(policy_json));
    argv.push(cwd.as_os_str().to_owned());
    argv.push(OsString::from("--"));
    argv.push(program.as_ref().to_owned());
    argv.extend(args.into_iter().map(|a| a.as_ref().to_owned()));
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_argv_has_expected_layout() {
        let argv = sandbox_argv(
            &SandboxPolicy::ReadOnly,
            Path::new("/work"),
            "ls",
            ["-la", "/tmp"],
        )
        .unwrap();
        // [policy-json, cwd, "--", program, args…]
        assert_eq!(argv.len(), 6);
        assert!(argv[0].to_str().unwrap().contains("read-only"));
        assert_eq!(argv[1], OsStr::new("/work"));
        assert_eq!(argv[2], OsStr::new("--"));
        assert_eq!(argv[3], OsStr::new("ls"));
        assert_eq!(argv[4], OsStr::new("-la"));
        assert_eq!(argv[5], OsStr::new("/tmp"));
    }

    #[test]
    fn default_is_read_only() {
        assert_eq!(SandboxPolicy::default(), SandboxPolicy::ReadOnly);
    }

    #[test]
    fn access_predicates_per_mode() {
        let ro = SandboxPolicy::ReadOnly;
        assert!(ro.has_full_disk_read_access());
        assert!(!ro.has_full_disk_write_access());
        assert!(!ro.has_full_network_access());
        assert!(!ro.is_unrestricted());

        let full = SandboxPolicy::DangerFullAccess;
        assert!(full.has_full_disk_write_access());
        assert!(full.has_full_network_access());
        assert!(full.is_unrestricted());

        let ws = SandboxPolicy::new_workspace_write(vec![]);
        assert!(ws.has_full_disk_read_access());
        assert!(!ws.has_full_disk_write_access()); // bounded, not full
        assert!(!ws.has_full_network_access()); // off by default
        assert!(!ws.is_unrestricted());

        let ws_net = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            network_access: true,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        };
        assert!(ws_net.has_full_network_access());
    }

    #[test]
    fn permissiveness_orders_modes_and_detects_escalation() {
        let ro = SandboxPolicy::ReadOnly;
        let ws = SandboxPolicy::new_workspace_write(vec![]);
        let full = SandboxPolicy::DangerFullAccess;
        assert!(ro.permissiveness() < ws.permissiveness());
        assert!(ws.permissiveness() < full.permissiveness());
        assert!(ws.is_escalation_over(&ro));
        assert!(full.is_escalation_over(&ws));
        assert!(!ro.is_escalation_over(&ws));
        assert!(!ro.is_escalation_over(&ro));
    }

    #[test]
    fn workspace_write_roots_include_cwd_extras_and_tmp() {
        let cwd = PathBuf::from("/home/user/project");
        let extra = PathBuf::from("/data/scratch");
        let policy = SandboxPolicy::new_workspace_write(vec![extra.clone()]);
        let tmpdir = PathBuf::from("/custom/tmp");
        let roots = policy.writable_roots_impl(&cwd, Some(&tmpdir));

        let paths: Vec<&PathBuf> = roots.iter().map(|r| &r.root).collect();
        assert!(paths.contains(&&cwd));
        assert!(paths.contains(&&extra));
        assert!(paths.contains(&&PathBuf::from("/tmp"))); // unix
        assert!(paths.contains(&&tmpdir));

        // The cwd root carves out a read-only `.git`.
        let cwd_root = roots.iter().find(|r| r.root == cwd).unwrap();
        assert_eq!(cwd_root.read_only_subpaths, vec![cwd.join(".git")]);
    }

    #[test]
    fn exclusions_drop_tmp_roots() {
        let cwd = PathBuf::from("/work");
        let policy = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            network_access: false,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        };
        let roots = policy.writable_roots_impl(&cwd, Some(Path::new("/custom/tmp")));
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root, cwd);
    }

    #[test]
    fn is_path_writable_respects_roots_and_git_carveout() {
        let cwd = PathBuf::from("/work");
        let policy = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            network_access: false,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        };
        let roots = policy.writable_roots_impl(&cwd, None);
        let cwd_root = &roots[0];
        assert!(cwd_root.is_path_writable(Path::new("/work/src/main.rs")));
        assert!(!cwd_root.is_path_writable(Path::new("/etc/passwd"))); // outside
        assert!(!cwd_root.is_path_writable(Path::new("/work/.git/config"))); // carved out

        // Read-only and full-access ignore the roots entirely.
        assert!(!SandboxPolicy::ReadOnly.is_path_writable(Path::new("/work/x"), &cwd));
        assert!(SandboxPolicy::DangerFullAccess.is_path_writable(Path::new("/etc/x"), &cwd));
    }

    #[test]
    fn serde_round_trips_with_kebab_tags() {
        let ws = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![PathBuf::from("/a")],
            network_access: true,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        };
        let json = serde_json::to_string(&ws).unwrap();
        assert!(json.contains("\"mode\":\"workspace-write\""), "got: {json}");
        assert!(json.contains("network_access"), "fields stay snake_case: {json}");
        assert_eq!(serde_json::from_str::<SandboxPolicy>(&json).unwrap(), ws);

        // Tags for the unit variants.
        assert_eq!(
            serde_json::to_string(&SandboxPolicy::ReadOnly).unwrap(),
            "{\"mode\":\"read-only\"}"
        );
        assert_eq!(
            serde_json::from_str::<SandboxPolicy>("{\"mode\":\"danger-full-access\"}").unwrap(),
            SandboxPolicy::DangerFullAccess
        );
    }
}
