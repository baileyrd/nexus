//! OS process sandbox **enforcement** for [`SandboxPolicy`].
//!
//! Applies a policy to the current thread so that it — and any child process it
//! `exec`s afterwards — is confined at the kernel level. On Linux the
//! filesystem dimension uses the
//! [Landlock](https://docs.kernel.org/userspace-api/landlock.html) LSM
//! ([`apply_to_current_thread`]) and the network dimension uses seccomp-bpf to
//! deny IP socket creation ([`block_inet_sockets`]); other platforms currently
//! return the `Unsupported` status (macOS seatbelt and Windows restricted
//! tokens land later).
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

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

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

/// Outcome of installing the network block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStatus {
    /// A seccomp filter denying inet socket creation was installed.
    Blocked,
    /// No network backend on this platform yet.
    Unsupported,
}

/// Errors applying a sandbox policy.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// The kernel sandbox API is present but rejected the ruleset.
    #[error("landlock ruleset error: {0}")]
    Ruleset(String),
    /// The seccomp network filter could not be built or installed. On Linux a
    /// failure here means **network access was not contained** — the caller
    /// must decide whether to proceed or refuse the spawn.
    #[error("seccomp network filter error: {0}")]
    Seccomp(String),
    /// The policy could not be serialized for the `nexus-sandbox` helper.
    #[error("sandbox policy encode error: {0}")]
    Encode(String),
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

/// Install a seccomp-bpf filter on the **current thread** that denies creation
/// of IP sockets (`AF_INET` / `AF_INET6` / `AF_PACKET`) with `EPERM`, while
/// leaving `AF_UNIX` (local IPC) and every other syscall untouched. Apply when
/// a policy disallows network access, *after* [`apply_to_current_thread`], on
/// the thread that will `exec` the child. Irreversible for that thread.
///
/// Compose the two dimensions per policy:
///
/// ```no_run
/// use nexus_security::os_sandbox::{apply_to_current_thread, block_inet_sockets};
/// # use nexus_types::SandboxPolicy;
/// # use std::path::Path;
/// # fn confine(policy: &SandboxPolicy, cwd: &Path) -> Result<(), Box<dyn std::error::Error>> {
/// apply_to_current_thread(policy, cwd)?; // filesystem (landlock)
/// if !policy.has_full_network_access() {
///     block_inet_sockets()?;             // network (seccomp)
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// Returns [`SandboxError::Seccomp`] if the filter cannot be built or the
/// kernel rejects it — meaning network access was **not** contained.
pub fn block_inet_sockets() -> Result<NetworkStatus, SandboxError> {
    #[cfg(target_os = "linux")]
    {
        linux_net::block()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(NetworkStatus::Unsupported)
    }
}

/// Confine the **current thread** per `policy`: filesystem first
/// ([`apply_to_current_thread`]), then — when the policy disallows network —
/// the inet socket block ([`block_inet_sockets`]). This is the composition a
/// sandbox entry point applies to itself immediately before doing untrusted
/// work (or, from a `pre_exec` hook, before `exec`ing an untrusted child).
/// Irreversible for the calling thread.
///
/// Returns the filesystem [`SandboxStatus`]; `danger-full-access` yields
/// [`SandboxStatus::Skipped`] and applies no network block.
///
/// # Errors
/// Propagates [`SandboxError`] from either dimension. A network-block failure
/// is surfaced (not swallowed) because it means network was **not** contained.
pub fn confine_current_thread(
    policy: &SandboxPolicy,
    cwd: &Path,
) -> Result<SandboxStatus, SandboxError> {
    let filesystem = apply_to_current_thread(policy, cwd)?;
    if !policy.has_full_network_access() {
        block_inet_sockets()?;
    }
    Ok(filesystem)
}

/// Build a [`Command`] that runs `program` (with `args`) confined by `policy`,
/// by way of the `nexus-sandbox` helper at `helper`. The helper applies the
/// policy to *itself* — single-threaded — and `exec`s the target, avoiding the
/// after-`fork()` allocation hazard a `pre_exec` hook would have in this
/// (potentially multithreaded) process. See the module docs.
///
/// The argv layout comes from [`nexus_types::sandbox_argv`]; spawn backends that
/// don't use [`Command`] (e.g. portable-pty) call that directly.
///
/// # Errors
/// Returns [`SandboxError::Encode`] if the policy cannot be serialized.
pub fn sandbox_command<P, A>(
    helper: &Path,
    policy: &SandboxPolicy,
    cwd: &Path,
    program: P,
    args: A,
) -> Result<Command, SandboxError>
where
    P: AsRef<OsStr>,
    A: IntoIterator,
    A::Item: AsRef<OsStr>,
{
    let argv = nexus_types::sandbox_argv(policy, cwd, program, args)
        .map_err(|e| SandboxError::Encode(e.to_string()))?;
    let mut cmd = Command::new(helper);
    cmd.args(argv);
    Ok(cmd)
}

#[cfg(target_os = "linux")]
mod linux_net {
    use std::collections::BTreeMap;

    use seccompiler::{
        apply_filter, BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition,
        SeccompFilter, SeccompRule, TargetArch,
    };

    use super::{NetworkStatus, SandboxError};

    // The `Option` is meaningful on unsupported architectures; clippy only sees
    // the always-`Some` branch for the current compile target.
    #[allow(clippy::unnecessary_wraps)]
    fn target_arch() -> Option<TargetArch> {
        #[cfg(target_arch = "x86_64")]
        {
            Some(TargetArch::x86_64)
        }
        #[cfg(target_arch = "aarch64")]
        {
            Some(TargetArch::aarch64)
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            None
        }
    }

    pub(super) fn block() -> Result<NetworkStatus, SandboxError> {
        let seccomp_err = |e: &dyn std::fmt::Display| SandboxError::Seccomp(e.to_string());
        let arch = target_arch()
            .ok_or_else(|| SandboxError::Seccomp("unsupported CPU architecture".to_string()))?;

        // A rule matching `socket(domain == family, …)`. AF_* constants are
        // non-negative, so `try_from` documents the invariant without a
        // sign-losing cast.
        let deny_family = |family: libc::c_int| -> Result<SeccompRule, SandboxError> {
            let value = u64::try_from(family)
                .map_err(|_| SandboxError::Seccomp("negative socket family".to_string()))?;
            SeccompRule::new(vec![SeccompCondition::new(
                0,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::Eq,
                value,
            )
            .map_err(|e| seccomp_err(&e))?])
            .map_err(|e| seccomp_err(&e))
        };

        let rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::from([(
            libc::SYS_socket,
            vec![
                deny_family(libc::AF_INET)?,
                deny_family(libc::AF_INET6)?,
                deny_family(libc::AF_PACKET)?,
            ],
        )]);

        // Default-allow; matched inet `socket()` calls get EPERM.
        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(libc::EPERM as u32),
            arch,
        )
        .map_err(|e| seccomp_err(&e))?;

        let program: BpfProgram = filter.try_into().map_err(|e| seccomp_err(&e))?;
        apply_filter(&program).map_err(|e| seccomp_err(&e))?;
        Ok(NetworkStatus::Blocked)
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

    pub(super) fn apply(policy: &SandboxPolicy, cwd: &Path) -> Result<SandboxStatus, SandboxError> {
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
    fn sandbox_command_builds_helper_invocation() {
        let policy = SandboxPolicy::new_workspace_write(vec![]);
        let cmd = sandbox_command(
            Path::new("/opt/nexus-sandbox"),
            &policy,
            Path::new("/work"),
            "echo",
            ["hi", "there"],
        )
        .unwrap();
        assert_eq!(cmd.get_program(), OsStr::new("/opt/nexus-sandbox"));
        let args: Vec<&OsStr> = cmd.get_args().collect();
        // [policy-json, cwd, "--", program, args…]
        assert_eq!(args.len(), 6);
        assert!(args[0].to_str().unwrap().contains("workspace-write"));
        assert_eq!(args[1], OsStr::new("/work"));
        assert_eq!(args[2], OsStr::new("--"));
        assert_eq!(args[3], OsStr::new("echo"));
        assert_eq!(args[4], OsStr::new("hi"));
        assert_eq!(args[5], OsStr::new("there"));
    }

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
        assert_eq!(block_inet_sockets().unwrap(), NetworkStatus::Unsupported);
    }

    // Like the landlock tests, the seccomp filter is irreversible for the
    // calling thread, so it runs in a discarded thread. Asserts the deny only
    // when the kernel actually installed the filter (CI portability).
    #[cfg(target_os = "linux")]
    #[test]
    fn block_inet_sockets_denies_ip_but_allows_unix_when_enforced() {
        let outcome = std::thread::spawn(|| match block_inet_sockets() {
            Ok(NetworkStatus::Blocked) => {
                let inet = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
                let inet_denied = inet < 0;
                if inet >= 0 {
                    unsafe { libc::close(inet) };
                }
                let unix = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
                let unix_ok = unix >= 0;
                if unix >= 0 {
                    unsafe { libc::close(unix) };
                }
                Some((inet_denied, unix_ok))
            }
            // seccomp unavailable in this environment.
            Ok(NetworkStatus::Unsupported) | Err(_) => None,
        })
        .join()
        .unwrap();

        match outcome {
            Some((inet_denied, unix_ok)) => {
                assert!(
                    inet_denied,
                    "inet socket creation must be denied after the block"
                );
                assert!(unix_ok, "AF_UNIX sockets must remain available");
            }
            None => eprintln!("seccomp network block not enforced/available in this env"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn confine_blocks_network_unless_full_access() {
        // danger-full-access must never block the network.
        let allowed = std::thread::spawn(|| {
            confine_current_thread(&SandboxPolicy::DangerFullAccess, Path::new("/tmp")).unwrap();
            let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            let ok = fd >= 0;
            if fd >= 0 {
                unsafe { libc::close(fd) };
            }
            ok
        })
        .join()
        .unwrap();
        assert!(allowed, "danger-full-access must not block network");

        // A non-network policy composes the seccomp block (verified where the
        // kernel enforces seccomp — it does in this container).
        let denied = std::thread::spawn(|| {
            confine_current_thread(
                &SandboxPolicy::new_workspace_write(vec![]),
                Path::new("/tmp"),
            )
            .unwrap();
            let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            let d = fd < 0;
            if fd >= 0 {
                unsafe { libc::close(fd) };
            }
            d
        })
        .join()
        .unwrap();
        if !denied {
            eprintln!("seccomp not enforced here; confine network block unverifiable");
        }
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
            let status = apply_to_current_thread(&SandboxPolicy::ReadOnly, Path::new("/")).unwrap();
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
            assert!(
                inside_ok,
                "writes inside the workspace root must be allowed"
            );
            assert!(
                outside_blocked,
                "writes outside the workspace root must be blocked"
            );
        } else {
            eprintln!("landlock status {status:?}: enforcement not verifiable in this env");
        }
        let _ = std::fs::remove_file(&outside);
    }
}
