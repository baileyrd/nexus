//! Shared spawn-policy type for process-spawning services.
//!
//! `SpawnPolicy` carries *env-hygiene* rules that constrain the
//! environment a freshly-spawned child process inherits. It is the
//! leaf-crate home (rather than `nexus-terminal`) because it crosses the
//! IPC boundary and the same shape applies to every service that spawns
//! a child — terminal today, agent / lsp / dap / mcp later. Keeping one
//! type here avoids a fourth divergent per-crate copy and gives the
//! deferred resource-limit fields (cpu/rlimit) a single place to land.
//!
//! # What it is, and is NOT
//!
//! This is **resource governance and env hygiene, not a security
//! boundary.** A scrubbed child can still open sockets and read any file
//! its uid can reach. The fields here stop *accidental* leakage of the
//! parent environment (API keys, tokens) into child processes and let a
//! forge express a clean-env default; they do not jail the process. Real
//! OS-level isolation (namespaces, seccomp) would be a separate, larger
//! mechanism layered at the spawn syscall.
//!
//! # Authority & precedence
//!
//! The authoritative default lives in forge config (`.forge/*.toml`),
//! resolved server-side. A per-call argument may only ever *tighten*
//! that default via [`SpawnPolicy::tighten`] — it can never loosen a
//! forge-mandated restriction. Callers compute `forge_default.tighten(
//! per_call)` and feed the result to the spawn path.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Env-hygiene rules applied to a child process's inherited environment.
///
/// Resolution order applied to the *inherited* parent env (see
/// [`SpawnPolicy::filter_inherited`]):
///
/// 1. If [`clean_env`](Self::clean_env) is set, the inherited env is
///    discarded entirely (start from nothing).
/// 2. Otherwise, if [`env_allowlist`](Self::env_allowlist) is non-empty,
///    only inherited keys on the allowlist survive.
/// 3. Any key matching [`env_denylist`](Self::env_denylist) is then
///    removed.
///
/// All key comparisons are case-insensitive. This struct governs only
/// the *inherited* env — a caller's explicit per-spawn variables and
/// service-mandated vars (e.g. `TERM`) are layered on top by the spawn
/// path *after* this filter and are not subject to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SpawnPolicy {
    /// Discard the inherited parent environment entirely; the child
    /// starts from an empty env plus whatever the spawn path layers on
    /// explicitly. The strongest hygiene knob.
    #[serde(default)]
    pub clean_env: bool,
    /// When non-empty, only inherited keys whose name matches an entry
    /// here (case-insensitive) survive. Ignored when
    /// [`clean_env`](Self::clean_env) is set (nothing inherited to
    /// allow). An empty list means "no allowlist restriction".
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    /// Inherited keys matching an entry here (case-insensitive) are
    /// removed. Applied after the allowlist, so a key can be allowed in
    /// bulk and then individually denied.
    #[serde(default)]
    pub env_denylist: Vec<String>,
    /// Wall-clock runtime budget in seconds. When set, the spawning
    /// service kills the session once it has been alive longer than this
    /// (enforced out-of-band by the terminal's memory poller, so the
    /// kill lands within roughly one poll interval of the deadline).
    /// `None` means no time limit. Enforcement requires the service's
    /// background monitor to be running.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// CPU-time budget in seconds — the session is killed once the
    /// child has *consumed* this much CPU time (user + system),
    /// independent of wall-clock. `None` means no limit. Like
    /// `timeout_secs` this is enforced by the terminal's memory poller
    /// (sample-and-kill), so it lands within ~one poll interval of the
    /// breach and observes only the direct child process, not its whole
    /// subtree — the same soft-limit caveat as the RSS memory cap.
    #[serde(default)]
    pub cpu_secs: Option<u64>,
    /// Best-effort confinement of the child's **initial** working
    /// directory: when set, the resolved working dir must canonicalize
    /// to a path inside this root, otherwise the spawn is rejected. A
    /// session spawned with no explicit working dir defaults to this
    /// root. `None` imposes no confinement.
    ///
    /// **Not a jail.** This constrains only the starting cwd — the child
    /// can `cd` elsewhere immediately, and the canonicalize-then-check is
    /// inherently TOCTOU. It stops accidents, not a determined process.
    #[serde(default)]
    pub root_dir: Option<String>,
    /// Best-effort allowlist of shell programs that may be spawned. When
    /// `Some`, the spawn program must match an entry by exact path or by
    /// basename, otherwise the spawn is rejected. `None` imposes no
    /// restriction; `Some(empty)` denies every spawn.
    ///
    /// **Not a sandbox.** A shell on the allowlist can still launch
    /// anything via `sh -c`, `$(...)`, symlinks, or a wrapper binary —
    /// this only gates the immediate program name.
    #[serde(default)]
    pub command_allowlist: Option<Vec<String>>,
}

impl SpawnPolicy {
    /// A policy that changes nothing — every inherited variable passes
    /// through untouched. Equivalent to [`SpawnPolicy::default`]; named
    /// for intent at call sites.
    #[must_use]
    pub fn permissive() -> Self {
        Self::default()
    }

    /// Does this policy change the *inherited environment* of a spawned
    /// child? Drives whether the spawn path performs the `env_clear` +
    /// filtered re-add. A timeout-only policy returns `false` here — the
    /// env is untouched even though the policy is not a full no-op.
    #[must_use]
    pub fn affects_env(&self) -> bool {
        self.clean_env || !self.env_allowlist.is_empty() || !self.env_denylist.is_empty()
    }

    /// Does this policy do nothing at all — no env changes, no runtime
    /// limit, no confinement, no command restriction?
    #[must_use]
    pub fn is_noop(&self) -> bool {
        !self.affects_env()
            && self.timeout_secs.is_none()
            && self.cpu_secs.is_none()
            && self.root_dir.is_none()
            && self.command_allowlist.is_none()
    }

    /// Is `program` permitted by [`command_allowlist`](Self::command_allowlist)?
    /// `None` (no allowlist) permits everything. Otherwise the program is
    /// allowed when an entry equals its full path or its file name.
    #[must_use]
    pub fn command_allowed(&self, program: &std::path::Path) -> bool {
        let Some(allow) = self.command_allowlist.as_ref() else {
            return true;
        };
        let full = program.to_string_lossy();
        let base = program
            .file_name()
            .map(|s| s.to_string_lossy().into_owned());
        allow
            .iter()
            .any(|entry| entry.as_str() == full || base.as_deref() == Some(entry.as_str()))
    }

    /// Merge `self` (a baseline, e.g. the forge default) with `other`
    /// (e.g. a per-call argument) so the result is **at least as
    /// restrictive as both** — never looser. This is the only sanctioned
    /// way to combine an authoritative default with untrusted caller
    /// input: a permissive `other` cannot weaken `self`.
    ///
    /// - `clean_env`: set if *either* side sets it.
    /// - `env_allowlist`: intersection when both restrict; the
    ///   restricting side when only one does (empty = unrestricted).
    /// - `env_denylist`: union (more keys removed is tighter).
    #[must_use]
    pub fn tighten(&self, other: &SpawnPolicy) -> SpawnPolicy {
        SpawnPolicy {
            clean_env: self.clean_env || other.clean_env,
            env_allowlist: tighten_allowlist(&self.env_allowlist, &other.env_allowlist),
            env_denylist: union_ci(&self.env_denylist, &other.env_denylist),
            timeout_secs: tighten_min_secs(self.timeout_secs, other.timeout_secs),
            cpu_secs: tighten_min_secs(self.cpu_secs, other.cpu_secs),
            // A forge-mandated confinement root stands; a caller can add
            // one when the forge sets none, but cannot swap out the
            // forge's. (A pure merge can't fs-check subpath containment,
            // so "base wins" is the monotonic-safe choice.)
            root_dir: self.root_dir.clone().or_else(|| other.root_dir.clone()),
            command_allowlist: tighten_command_allowlist(
                self.command_allowlist.as_deref(),
                other.command_allowlist.as_deref(),
            ),
        }
    }

    /// Apply the env-hygiene rules to a snapshot of the parent's
    /// inherited environment, returning the variables that survive in
    /// original order. Pure — the spawn path passes `std::env::vars()`
    /// (or a fixture in tests); this never reads the process env itself.
    #[must_use]
    pub fn filter_inherited(&self, inherited: &[(String, String)]) -> Vec<(String, String)> {
        if self.clean_env {
            return Vec::new();
        }
        let allow_active = !self.env_allowlist.is_empty();
        inherited
            .iter()
            .filter(|(k, _)| {
                if allow_active && !contains_ci(&self.env_allowlist, k) {
                    return false;
                }
                !contains_ci(&self.env_denylist, k)
            })
            .cloned()
            .collect()
    }
}

/// Case-insensitive membership test.
fn contains_ci(haystack: &[String], needle: &str) -> bool {
    haystack.iter().any(|h| h.eq_ignore_ascii_case(needle))
}

/// Case-insensitive union preserving first-seen order from `a` then `b`.
fn union_ci(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = a.to_vec();
    for k in b {
        if !contains_ci(&out, k) {
            out.push(k.clone());
        }
    }
    out
}

/// Tightening merge for an optional second-budget (wall-clock or CPU):
/// the shorter limit wins, and `None` (no limit) never overrides a
/// concrete limit.
fn tighten_min_secs(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Tightening merge for the optional command allowlist: `None` means
/// unrestricted, so the restricting side wins; when both restrict, the
/// result is their intersection (permits a subset of both).
fn tighten_command_allowlist(a: Option<&[String]>, b: Option<&[String]>) -> Option<Vec<String>> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a.to_vec()),
        (None, Some(b)) => Some(b.to_vec()),
        (Some(a), Some(b)) => Some(a.iter().filter(|e| b.contains(e)).cloned().collect()),
    }
}

/// Tightening merge for allowlists where an empty list means
/// "unrestricted". If either side is unrestricted, the other side's
/// restriction stands; if both restrict, the result is their
/// intersection (allows a subset of both).
fn tighten_allowlist(a: &[String], b: &[String]) -> Vec<String> {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_vec(),
        (_, true) => a.to_vec(),
        (false, false) => a
            .iter()
            .filter(|k| contains_ci(b, k))
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn permissive_is_noop_and_passes_everything() {
        let p = SpawnPolicy::permissive();
        assert!(p.is_noop());
        let env = kv(&[("HOME", "/home/me"), ("PATH", "/usr/bin")]);
        assert_eq!(p.filter_inherited(&env), env);
    }

    #[test]
    fn clean_env_drops_all_inherited() {
        let p = SpawnPolicy {
            clean_env: true,
            ..Default::default()
        };
        assert!(!p.is_noop());
        let env = kv(&[("HOME", "/home/me"), ("SECRET", "x")]);
        assert!(p.filter_inherited(&env).is_empty());
    }

    #[test]
    fn allowlist_keeps_only_listed_keys_case_insensitive() {
        let p = SpawnPolicy {
            env_allowlist: vec!["path".into(), "Home".into()],
            ..Default::default()
        };
        let env = kv(&[("HOME", "/h"), ("PATH", "/p"), ("TOKEN", "secret")]);
        let out = p.filter_inherited(&env);
        assert_eq!(out, kv(&[("HOME", "/h"), ("PATH", "/p")]));
    }

    #[test]
    fn denylist_removes_listed_keys_case_insensitive() {
        let p = SpawnPolicy {
            env_denylist: vec!["api_key".into()],
            ..Default::default()
        };
        let env = kv(&[("PATH", "/p"), ("API_KEY", "sk-1")]);
        assert_eq!(p.filter_inherited(&env), kv(&[("PATH", "/p")]));
    }

    #[test]
    fn denylist_applies_after_allowlist() {
        let p = SpawnPolicy {
            env_allowlist: vec!["A".into(), "B".into()],
            env_denylist: vec!["B".into()],
            ..Default::default()
        };
        let env = kv(&[("A", "1"), ("B", "2"), ("C", "3")]);
        assert_eq!(p.filter_inherited(&env), kv(&[("A", "1")]));
    }

    #[test]
    fn tighten_clean_env_is_sticky() {
        let base = SpawnPolicy {
            clean_env: true,
            ..Default::default()
        };
        let loose = SpawnPolicy::permissive();
        assert!(base.tighten(&loose).clean_env);
        assert!(loose.tighten(&base).clean_env);
    }

    #[test]
    fn tighten_denylist_unions() {
        let a = SpawnPolicy {
            env_denylist: vec!["X".into()],
            ..Default::default()
        };
        let b = SpawnPolicy {
            env_denylist: vec!["Y".into(), "x".into()],
            ..Default::default()
        };
        let out = a.tighten(&b);
        assert_eq!(out.env_denylist, vec!["X".to_string(), "Y".to_string()]);
    }

    #[test]
    fn tighten_allowlist_intersects_when_both_restrict() {
        let a = SpawnPolicy {
            env_allowlist: vec!["A".into(), "B".into()],
            ..Default::default()
        };
        let b = SpawnPolicy {
            env_allowlist: vec!["B".into(), "C".into()],
            ..Default::default()
        };
        assert_eq!(a.tighten(&b).env_allowlist, vec!["B".to_string()]);
    }

    #[test]
    fn tighten_allowlist_empty_side_does_not_loosen() {
        let restricted = SpawnPolicy {
            env_allowlist: vec!["A".into()],
            ..Default::default()
        };
        let unrestricted = SpawnPolicy::permissive();
        // An empty (unrestricted) per-call arg keeps the base restriction.
        assert_eq!(
            restricted.tighten(&unrestricted).env_allowlist,
            vec!["A".to_string()]
        );
        // And a restriction added by the caller applies even when base is open.
        assert_eq!(
            unrestricted.tighten(&restricted).env_allowlist,
            vec!["A".to_string()]
        );
    }

    #[test]
    fn timeout_only_policy_is_not_env_affecting() {
        let p = SpawnPolicy {
            timeout_secs: Some(30),
            ..Default::default()
        };
        assert!(!p.affects_env());
        assert!(!p.is_noop());
    }

    #[test]
    fn tighten_timeout_picks_shorter_and_none_does_not_loosen() {
        let a = SpawnPolicy {
            timeout_secs: Some(60),
            ..Default::default()
        };
        let b = SpawnPolicy {
            timeout_secs: Some(10),
            ..Default::default()
        };
        assert_eq!(a.tighten(&b).timeout_secs, Some(10));
        // A None (unlimited) caller keeps the base limit.
        let unlimited = SpawnPolicy::permissive();
        assert_eq!(a.tighten(&unlimited).timeout_secs, Some(60));
        assert_eq!(unlimited.tighten(&a).timeout_secs, Some(60));
    }

    #[test]
    fn command_allowed_permits_all_when_unset() {
        let p = SpawnPolicy::permissive();
        assert!(p.command_allowed(std::path::Path::new("/bin/bash")));
    }

    #[test]
    fn command_allowed_matches_basename_or_full_path() {
        let p = SpawnPolicy {
            command_allowlist: Some(vec!["bash".into(), "/usr/bin/zsh".into()]),
            ..Default::default()
        };
        assert!(p.command_allowed(std::path::Path::new("/bin/bash"))); // basename
        assert!(p.command_allowed(std::path::Path::new("/usr/bin/zsh"))); // full path
        assert!(!p.command_allowed(std::path::Path::new("/usr/local/bin/zsh"))); // wrong path
        assert!(!p.command_allowed(std::path::Path::new("/bin/sh")));
    }

    #[test]
    fn empty_command_allowlist_denies_everything() {
        let p = SpawnPolicy {
            command_allowlist: Some(vec![]),
            ..Default::default()
        };
        assert!(!p.command_allowed(std::path::Path::new("/bin/sh")));
    }

    #[test]
    fn tighten_command_allowlist_intersects_and_none_does_not_loosen() {
        let a = SpawnPolicy {
            command_allowlist: Some(vec!["bash".into(), "sh".into()]),
            ..Default::default()
        };
        let b = SpawnPolicy {
            command_allowlist: Some(vec!["sh".into(), "zsh".into()]),
            ..Default::default()
        };
        assert_eq!(a.tighten(&b).command_allowlist, Some(vec!["sh".to_string()]));
        let unrestricted = SpawnPolicy::permissive();
        assert_eq!(
            a.tighten(&unrestricted).command_allowlist,
            Some(vec!["bash".to_string(), "sh".to_string()])
        );
    }

    #[test]
    fn tighten_root_dir_base_wins_when_set() {
        let base = SpawnPolicy {
            root_dir: Some("/srv/forge".into()),
            ..Default::default()
        };
        let caller = SpawnPolicy {
            root_dir: Some("/tmp".into()),
            ..Default::default()
        };
        // A forge-mandated root cannot be swapped out by the caller.
        assert_eq!(base.tighten(&caller).root_dir, Some("/srv/forge".into()));
        // But a caller may add confinement when the forge sets none.
        assert_eq!(
            SpawnPolicy::permissive().tighten(&caller).root_dir,
            Some("/tmp".into())
        );
    }

    #[test]
    fn tighten_cpu_secs_picks_shorter_and_none_does_not_loosen() {
        let a = SpawnPolicy {
            cpu_secs: Some(120),
            ..Default::default()
        };
        let b = SpawnPolicy {
            cpu_secs: Some(30),
            ..Default::default()
        };
        assert_eq!(a.tighten(&b).cpu_secs, Some(30));
        let unlimited = SpawnPolicy::permissive();
        assert_eq!(a.tighten(&unlimited).cpu_secs, Some(120));
        assert_eq!(unlimited.tighten(&a).cpu_secs, Some(120));
    }

    #[test]
    fn cpu_only_policy_is_not_noop_and_not_env_affecting() {
        let p = SpawnPolicy {
            cpu_secs: Some(5),
            ..Default::default()
        };
        assert!(!p.is_noop());
        assert!(!p.affects_env());
    }

    #[test]
    fn confinement_only_policy_is_not_noop_and_not_env_affecting() {
        let p = SpawnPolicy {
            root_dir: Some("/srv".into()),
            ..Default::default()
        };
        assert!(!p.is_noop());
        assert!(!p.affects_env());
    }

    #[test]
    fn tighten_cannot_loosen_via_permissive_caller() {
        // The security-relevant invariant: a permissive caller arg can
        // never widen what a restrictive forge default allows.
        let forge = SpawnPolicy {
            clean_env: true,
            env_denylist: vec!["TOKEN".into()],
            ..Default::default()
        };
        let caller = SpawnPolicy::permissive();
        let merged = forge.tighten(&caller);
        assert!(merged.clean_env);
        assert!(contains_ci(&merged.env_denylist, "token"));
    }
}
