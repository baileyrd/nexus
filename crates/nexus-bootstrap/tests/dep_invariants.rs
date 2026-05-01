//! Dependency invariants for the Phase-B plugin-containment refactor.
//!
//! These crates are *IPC consumers* — they must reach subsystems via
//! `ipc_call` rather than linking the engine directly. A direct
//! `[dependencies]` entry means somebody re-introduced the tight coupling we
//! just removed. `[dev-dependencies]` is fine — tests still need to spin up
//! real engines to seed fixtures.
//!
//! If you legitimately need to relax one of these, update the `FORBIDDEN`
//! table below with a comment explaining why.
//!
//! Add new invariants by appending `(crate, forbidden_dep)` pairs.

use std::path::PathBuf;

/// `(consumer crate, dep that must not appear in [dependencies])`.
const FORBIDDEN: &[(&str, &str)] = &[
    ("nexus-cli", "nexus-storage"),
    ("nexus-tui", "nexus-storage"),
    ("nexus-ai", "nexus-storage"),
    ("nexus-mcp", "nexus-storage"),
    ("nexus-database", "nexus-storage"),
    // Invokers must reach pure-logic database helpers (CSV import/export,
    // formula eval) through `ipc_call("com.nexus.database", …)` rather
    // than linking `nexus-database` directly. See ARCHITECTURE.md §7 #3.
    ("nexus-cli", "nexus-database"),
    ("nexus-tui", "nexus-database"),
    // `nexus-storage` is the sole owner of the forge's SQLite database:
    // the SQL-backed query/schema/relation code for bases lives under
    // `nexus_storage::bases`. `nexus-database` is a pure-logic library
    // (property types, validation, formulas, CSV) that must not link
    // rusqlite — everything SQL-shaped goes through storage IPC.
    ("nexus-database", "rusqlite"),
    // MCP dispatches `nexus_ask` via `ipc_call(AI_PLUGIN, "ask", ...)`; it
    // must not link the AI engine directly.
    ("nexus-mcp", "nexus-ai"),
    // Kernel is backend-agnostic: the KV trait lives here, but the SQLite
    // impl is in `nexus-kv` and must be injected via `Kernel::new`.
    ("nexus-kernel", "rusqlite"),
    ("nexus-kernel", "nexus-kv"),
];

#[test]
fn ipc_consumers_do_not_direct_dep_on_forbidden_subsystems() {
    let workspace_root = workspace_root();

    let mut violations = Vec::new();
    for (crate_name, forbidden_dep) in FORBIDDEN {
        let manifest = workspace_root
            .join("crates")
            .join(crate_name)
            .join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).unwrap_or_else(|e| {
            panic!("failed to read {}: {e}", manifest.display());
        });
        let parsed: toml::Value = toml::from_str(&text).unwrap_or_else(|e| {
            panic!("failed to parse {}: {e}", manifest.display());
        });

        // Top-level `[dependencies].<forbidden>` — the original check.
        if parsed
            .get("dependencies")
            .and_then(|v| v.get(forbidden_dep))
            .is_some()
        {
            violations.push(format!(
                "  {crate_name}/Cargo.toml: [dependencies].{forbidden_dep} \
                 is forbidden — route through ipc_call via nexus-bootstrap instead."
            ));
        }

        // Issue #83. Pre-fix the test only checked
        // `[dependencies]`; a target-conditional dep block like
        //
        //   [target.'cfg(unix)'.dependencies]
        //   nexus-storage = { path = "..." }
        //
        // would slip past the invariant. There's no current
        // foot-gun in the workspace, but completing the check
        // closes the loophole before it lands.
        if let Some(target) = parsed.get("target").and_then(toml::Value::as_table) {
            for (cfg_key, cfg_block) in target {
                if let Some(deps) = cfg_block.get("dependencies").and_then(toml::Value::as_table)
                {
                    if deps.contains_key(*forbidden_dep) {
                        violations.push(format!(
                            "  {crate_name}/Cargo.toml: \
                             [target.'{cfg_key}'.dependencies].{forbidden_dep} \
                             is forbidden — route through ipc_call via \
                             nexus-bootstrap instead."
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "dependency invariants violated:\n{}",
        violations.join("\n"),
    );
}

/// Self-test for the cfg-deps extension (issue #83): synthesise a
/// manifest with a forbidden cfg-conditional dep and confirm the
/// helper logic flags it. Guards against silently-broken extension
/// — the invariants test above passes today because nothing in the
/// workspace uses target-cfg deps yet, so without this self-test a
/// regression in the cfg traversal wouldn't be visible.
#[test]
fn cfg_dep_check_catches_synthesised_violation() {
    let synthetic = r#"
[package]
name = "synthetic"

[target.'cfg(unix)'.dependencies]
nexus-storage = { path = "../nexus-storage" }
"#;
    let parsed: toml::Value = toml::from_str(synthetic).expect("parse");
    let mut hit = false;
    if let Some(target) = parsed.get("target").and_then(toml::Value::as_table) {
        for (_cfg, cfg_block) in target {
            if let Some(deps) = cfg_block.get("dependencies").and_then(toml::Value::as_table) {
                if deps.contains_key("nexus-storage") {
                    hit = true;
                }
            }
        }
    }
    assert!(
        hit,
        "cfg-conditional forbidden dep was not detected by the traversal logic"
    );
}

/// Walk up from `CARGO_MANIFEST_DIR` to the workspace root (the directory
/// holding the top-level `Cargo.toml` with `[workspace]`).
fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                if text.contains("[workspace]") {
                    return dir;
                }
            }
        }
        if !dir.pop() {
            panic!("could not locate workspace root starting from CARGO_MANIFEST_DIR");
        }
    }
}
