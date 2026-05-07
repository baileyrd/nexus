//! ADR 0021 (audit P1-2) — IPC handler versioning convention.
//!
//! Two complementary checks:
//!
//! 1. **End-to-end alias transparency** — proves that calling
//!    `com.nexus.storage::list_dir` and `com.nexus.storage::list_dir.v1`
//!    against the live runtime returns the same result. Locks in the
//!    semantic contract that the bare alias tracks the current
//!    version (today: v1) and that callers can opt in to explicit
//!    pinning at their own pace.
//!
//! 2. **Forward-deprecation guard (synthetic input)** — exercises the
//!    rule that for any `cmd.v<N>` with `N > 1`, either `cmd.v(N-1)`
//!    is also registered (deprecation window in effect) or there is
//!    a documented removal marker. With no `v2` handlers in the
//!    workspace today, the live-registry scan passes vacuously; the
//!    synthetic check ensures the guard logic itself is correct so
//!    a regression won't go unnoticed when v2 lands.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::PluginContext;

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

#[tokio::test]
async fn storage_list_dir_bare_and_v1_aliases_return_identical_results() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Seed a couple of files so the listing has stable content.
    for name in ["alpha.md", "beta.md"] {
        runtime
            .context
            .ipc_call(
                STORAGE_PLUGIN_ID,
                "write_file",
                serde_json::json!({
                    "path": name,
                    "bytes": b"x".to_vec(),
                }),
                CALL_TIMEOUT,
            )
            .await
            .unwrap_or_else(|e| panic!("seed {name}: {e:?}"));
    }

    let bare = runtime
        .context
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "list_dir",
            serde_json::json!({ "relpath": "" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("bare list_dir");

    let v1 = runtime
        .context
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "list_dir.v1",
            serde_json::json!({ "relpath": "" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("list_dir.v1");

    assert_eq!(
        bare, v1,
        "bare alias and .v1 must point at the same handler (ADR 0021)"
    );
}

#[tokio::test]
async fn storage_unknown_version_suffix_is_command_not_found() {
    // The convention says: only versions explicitly registered are
    // resolvable. `.v999` is not registered, so the dispatcher must
    // surface a clean CommandNotFound rather than silently routing to
    // the bare alias.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = runtime
        .context
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "list_dir.v999",
            serde_json::json!({ "relpath": "" }),
            CALL_TIMEOUT,
        )
        .await
        .expect_err("unregistered version must error");

    use nexus_kernel::IpcError;
    assert!(
        matches!(err, IpcError::CommandNotFound { .. }),
        "expected CommandNotFound, got {err:?}"
    );
}

// ─── Forward-deprecation guard ────────────────────────────────────────────

/// For every `cmd.v<N>` with `N > 1` in `commands`, either `cmd.v(N-1)`
/// must also be present (deprecation window in effect) or `cmd.v<N>` is
/// a fresh first-versioned handler (impossible by construction since
/// `N > 1`). Returns a list of violation messages — empty means OK.
///
/// Operates on a flat list of registered command names so the same
/// logic can be applied to the live storage manifest in a future PR
/// (today the workspace has no `v2` handlers, so the check is a
/// forward guard).
fn deprecation_window_violations(commands: &[&str]) -> Vec<String> {
    let mut violations = Vec::new();
    for &cmd in commands {
        let Some((base, version)) = parse_version_suffix(cmd) else {
            continue;
        };
        if version <= 1 {
            continue;
        }
        let predecessor = format!("{base}.v{}", version - 1);
        if !commands.contains(&predecessor.as_str()) {
            violations.push(format!(
                "{cmd}: deprecation window violated — predecessor {predecessor} \
                 is not registered (ADR 0021 requires N-1 stays registered \
                 for at least two minor releases)"
            ));
        }
    }
    violations
}

/// `("storage.search.v2") -> Some(("storage.search", 2))`. Returns
/// `None` when the suffix is missing or not a valid version digit.
fn parse_version_suffix(cmd: &str) -> Option<(&str, u32)> {
    let dot = cmd.rfind(".v")?;
    let (base, suffix) = cmd.split_at(dot);
    let n = suffix.strip_prefix(".v")?.parse::<u32>().ok()?;
    Some((base, n))
}

#[test]
fn deprecation_guard_passes_for_v1_and_bare() {
    // Today's storage shape: every command exists as both bare and `.v1`.
    let commands = ["search", "search.v1", "read_file", "read_file.v1"];
    assert!(
        deprecation_window_violations(&commands).is_empty(),
        "v1-only registrations must not trigger the guard"
    );
}

#[test]
fn deprecation_guard_passes_when_v1_and_v2_coexist() {
    // After v2 ships: bare → v2 handler, .v1 → legacy, .v2 → new.
    // The bare name is just a string — no `.vN` suffix on it, so it
    // skips the version check, exactly as intended.
    let commands = [
        "search",
        "search.v1",
        "search.v2",
        "read_file",
        "read_file.v1",
    ];
    assert!(
        deprecation_window_violations(&commands).is_empty(),
        "v1+v2 coexistence is the supported deprecation-window state"
    );
}

#[test]
fn deprecation_guard_flags_v2_without_v1() {
    // Regression: someone removed `.v1` while shipping `.v2`. The
    // ADR mandates two-minor-release window, so this must surface.
    let commands = ["search", "search.v2"];
    let violations = deprecation_window_violations(&commands);
    assert_eq!(violations.len(), 1, "must flag the missing predecessor");
    assert!(
        violations[0].contains("search.v2") && violations[0].contains("search.v1"),
        "violation message must name both the offender and its missing predecessor: {:?}",
        violations[0]
    );
}

#[test]
fn live_registry_has_v1_alias_for_every_bare_command() {
    // BL-097 — every subsystem opted into `with_v1_aliases` (ADR
    // 0021 §"Other subsystems"). Walk the live IPC registry and
    // assert the invariant: for any registered bare `<command>`
    // there must also be a `<command>.v1` registration on the
    // same plugin.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let commands = runtime.loader.lock().list_ipc_commands();

    use std::collections::HashSet;
    let pairs: HashSet<(String, String)> = commands.iter().cloned().collect();

    let mut missing: Vec<String> = Vec::new();
    for (plugin_id, cmd) in &commands {
        // Only check bare names — entries that already end in `.v<N>`
        // are themselves the explicit pin.
        if cmd.contains(".v") {
            continue;
        }
        let v1 = format!("{cmd}.v1");
        if !pairs.contains(&(plugin_id.clone(), v1.clone())) {
            missing.push(format!("{plugin_id}::{cmd} (no {plugin_id}::{v1})"));
        }
    }
    assert!(
        missing.is_empty(),
        "BL-097 / ADR 0021: every bare IPC command must have a \
         matching `.v1` alias. Missing pairs: {missing:#?}"
    );
}

#[test]
fn live_registry_passes_deprecation_window_guard() {
    // BL-097 — run the synthetic-input deprecation guard against
    // the live registry. With no `.v2` handlers today this passes
    // vacuously; the moment a future PR ships a `.v2` without
    // keeping `.v1` registered, this test surfaces it before
    // merge.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let commands = runtime.loader.lock().list_ipc_commands();
    let names: Vec<String> = commands.into_iter().map(|(_, c)| c).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let violations = deprecation_window_violations(&refs);
    assert!(
        violations.is_empty(),
        "BL-097 / ADR 0021: live registry has deprecation-window \
         violations: {violations:#?}"
    );
}

#[test]
fn deprecation_guard_flags_v3_when_only_v1_present() {
    // Skipping a version is also a violation — the predecessor of v3
    // is v2, not v1. Ship v2 first, retire it later.
    let commands = ["search.v1", "search.v3"];
    let violations = deprecation_window_violations(&commands);
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].contains("search.v2"),
        "violation must name the *immediate* predecessor"
    );
}
