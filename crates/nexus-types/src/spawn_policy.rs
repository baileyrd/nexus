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
}

impl SpawnPolicy {
    /// A policy that changes nothing — every inherited variable passes
    /// through untouched. Equivalent to [`SpawnPolicy::default`]; named
    /// for intent at call sites.
    #[must_use]
    pub fn permissive() -> Self {
        Self::default()
    }

    /// Does this policy leave the inherited env completely untouched?
    /// A no-op policy can skip the `env_clear` + re-add dance on the
    /// spawn path.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        !self.clean_env && self.env_allowlist.is_empty() && self.env_denylist.is_empty()
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
