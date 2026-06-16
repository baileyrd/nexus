//! OS process sandbox **enforcement** for [`SandboxPolicy`].
//!
//! Applies a policy to the current thread so that it — and any child process it
//! `exec`s afterwards — is confined at the kernel level. On Linux this uses the
//! [Landlock](https://docs.kernel.org/userspace-api/landlock.html) LSM for
//! filesystem path restrictions; other platforms currently return
//! [`SandboxStatus::Unsupported`] (macOS seatbelt and Windows restricted tokens
//! land later, as does Linux seccomp for the network dimension).
//!
//! Landlock restrictions are **irreversible for the calling thread**, so apply
//! them on the thread that will immediately `exec` the sandboxed child (e.g. a
//! `pre_exec` hook) — never on a shared worker thread.
//!
//! Landlock is *grant-only*: it cannot revoke access to a subpath of a writable
//! root, so the policy's `.git` read-only carve-out is **not** enforced here
//! (it is honoured by the macOS seatbelt backend, which supports deny rules,
//! and by higher-layer edit tooling). Filesystem confinement — "write only
//! under the workspace roots" — is enforced.

use std::path::Path;

use nexus_types::SandboxPolicy;

/// Outcome of applying a sandbox policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxStatus {
    /// The kernel enforced every requested restriction.
    FullyEnforced,
    /// The kernel enforced some restrictions; others were unavailable (older
    /// kernel / ABI). Meaningfully confined, but weaker than requested.
    PartiallyEnforced,
    /// The platform has a backend but the kernel enforced nothing (Landlock
    /// disabled or unavailable). The process is **not** confined.
    NotEnforced,
    /// `DangerFullAccess` — no confinement was requested.
    Skipped,
    /// No sandbox backend on this platform yet.
    Unsupported,
}

impl SandboxStatus {
    /// True if the policy is being enforced to some degree.
    #[must_use]
    pub fn is_enforced(self) -> bool {
        matches!(self, Self::FullyEnforced | Self::PartiallyEnforced)
    }
}

/// Errors applying a sandbox policy.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// The kernel sandbox API is present but rejected the ruleset.
    #[error("landlock ruleset error: {0}")]
    Ruleset(String),
}

/// Apply `policy` (resolved against working directory `cwd`) to the **current
/// thread**. See the module docs for the irreversibility caveat.
///
/// # Errors
/// Returns [`SandboxError`] if the kernel sandbox API is present but rejects
/// the ruleset. A kernel that simply lacks Landlock yields
/// [`SandboxStatus::NotEnforced`], not an error.
pub fn apply_to_current_thread(
    policy: &SandboxPolicy,
    cwd: &Path,
) -> Result<SandboxStatus, SandboxError> {
    if policy.is_unrestricted() {
        return Ok(SandboxStatus::Skipped);
    }
    #[cfg(target_os = "linux")]
    {
        linux::apply(policy, cwd)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cwd;
        Ok(SandboxStatus::Unsupported)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{SandboxError, SandboxStatus};
    use std::path::{Path, PathBuf};

    use landlock::{
        path_beneath_rules, Access, AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr,
        RulesetStatus, ABI,
    };
    use nexus_types::SandboxPolicy;

    pub(super) fn apply(
        policy: &SandboxPolicy,
        cwd: &Path,
    ) -> Result<SandboxStatus, SandboxError> {
        let err = |e: landlock::RulesetError| SandboxError::Ruleset(e.to_string());
        // ABI::V1 governs read/write/exec on files and dirs — broad kernel
        // compatibility (5.13+). Newer rights (truncate, refer) are not
        // governed here.
        let abi = ABI::V1;
        let read_all = AccessFs::from_read(abi);
        let read_write = AccessFs::from_all(abi);

        // Whole disk readable; everything else denied unless granted below.
        let mut ruleset = Ruleset::default()
            .handle_access(AccessFs::from_all(abi))
            .map_err(err)?
            .create()
            .map_err(err)?
            .add_rules(path_beneath_rules(["/"], read_all))
            .map_err(err)?;

        // Grant read+write on each existing writable root (`path_beneath_rules`
        // errors on a missing path, so filter first — this is where the model's
        // lexical roots meet the real filesystem).
        let writable: Vec<PathBuf> = policy
            .writable_roots_with_cwd(cwd)
            .into_iter()
            .map(|w| w.root)
            .filter(|p| p.exists())
            .collect();
        if !writable.is_empty() {
            ruleset = ruleset
                .add_rules(path_beneath_rules(writable, read_write))
                .map_err(err)?;
        }

        let restriction = ruleset.restrict_self().map_err(err)?;
        Ok(match restriction.ruleset {
            RulesetStatus::FullyEnforced => SandboxStatus::FullyEnforced,
            RulesetStatus::PartiallyEnforced => SandboxStatus::PartiallyEnforced,
            RulesetStatus::NotEnforced => SandboxStatus::NotEnforced,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrestricted_policy_is_skipped() {
        // Safe to run on the main thread: no restriction is actually applied.
        let st = apply_to_current_thread(&SandboxPolicy::DangerFullAccess, Path::new("/")).unwrap();
        assert_eq!(st, SandboxStatus::Skipped);
        assert!(!st.is_enforced());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_is_unsupported() {
        let st = apply_to_current_thread(&SandboxPolicy::ReadOnly, Path::new("/")).unwrap();
        assert_eq!(st, SandboxStatus::Unsupported);
    }

    // Landlock restricts the *calling thread* irreversibly, so every enforcing
    // test runs inside a freshly-spawned thread that is then discarded. Where
    // the kernel lacks Landlock the status is NotEnforced and we assert only
    // that the code path ran cleanly (CI portability).
    #[cfg(target_os = "linux")]
    #[test]
    fn read_only_blocks_writes_when_enforced() {
        use std::io::Write;
        let outside = std::env::temp_dir().join("nexus_ro_sandbox_probe");
        let probe = outside.clone();
        let (status, write_failed) = std::thread::spawn(move || {
            let status =
                apply_to_current_thread(&SandboxPolicy::ReadOnly, Path::new("/")).unwrap();
            let failed = std::fs::File::create(&probe)
                .and_then(|mut f| f.write_all(b"x"))
                .is_err();
            (status, failed)
        })
        .join()
        .unwrap();

        if status == SandboxStatus::FullyEnforced {
            assert!(write_failed, "read-only sandbox must block all writes");
        } else {
            eprintln!("landlock status {status:?}: enforcement not verifiable in this env");
        }
        let _ = std::fs::remove_file(&outside);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn workspace_write_allows_inside_but_blocks_outside_when_enforced() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let inside = root.join("ok.txt");
        // /tmp is excluded so a temp-dir write is genuinely "outside".
        let outside = std::env::temp_dir().join("nexus_ws_sandbox_outside_probe");
        let (root_c, inside_c, outside_c) = (root.clone(), inside.clone(), outside.clone());

        let (status, inside_ok, outside_blocked) = std::thread::spawn(move || {
            let policy = SandboxPolicy::WorkspaceWrite {
                writable_roots: vec![root_c.clone()],
                network_access: false,
                exclude_tmpdir_env_var: true,
                exclude_slash_tmp: true,
            };
            let status = apply_to_current_thread(&policy, &root_c).unwrap();
            let inside_ok = std::fs::File::create(&inside_c)
                .and_then(|mut f| f.write_all(b"x"))
                .is_ok();
            let outside_blocked = std::fs::File::create(&outside_c).is_err();
            (status, inside_ok, outside_blocked)
        })
        .join()
        .unwrap();

        if status == SandboxStatus::FullyEnforced {
            assert!(inside_ok, "writes inside the workspace root must be allowed");
            assert!(outside_blocked, "writes outside the workspace root must be blocked");
        } else {
            eprintln!("landlock status {status:?}: enforcement not verifiable in this env");
        }
        let _ = std::fs::remove_file(&outside);
    }
}
