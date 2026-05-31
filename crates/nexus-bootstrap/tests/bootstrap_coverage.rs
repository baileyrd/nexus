//! #188 / R5 — registration-coverage invariant for the bootstrap path.
//!
//! Every workspace member must either:
//!   * have a corresponding `crates/nexus-bootstrap/src/plugins/<name>.rs`
//!     registrar file (i.e. it's registered as a CorePlugin by
//!     `register_all`), **or**
//!   * appear on [`EXEMPT_CRATES`] with a documented reason.
//!
//! This catches the failure mode #188 surfaced — `nexus-memory`,
//! `nexus-context`, `nexus-protocol` shipped as workspace members
//! ahead of being wired into bootstrap, so they undercut the
//! "every subsystem registered by bootstrap" claim. Listing them on
//! `EXEMPT_CRATES` with a comment makes the gap explicit; landing
//! their registrar files lets the corresponding rows be removed.
//!
//! Adding a new service crate to `[workspace] members` without a
//! matching registrar (or an EXEMPT entry) fails this test.

use std::collections::HashSet;
use std::path::PathBuf;

/// Workspace members that legitimately do *not* have a bootstrap
/// registrar. Every row is either:
///   * a leaf crate the kernel/plugin contract is built from
///     (consumed by other crates, never registered as a plugin),
///   * a frontend / IPC-proxy crate (it *consumes* the bootstrap-built
///     `Runtime`, isn't a CorePlugin itself), or
///   * a not-yet-wired subsystem with an issue tracking the
///     integration.
const EXEMPT_CRATES: &[(&str, &str)] = &[
    // Leaf / contract crates — depended on by kernel + plugins.
    ("nexus-types", "shared types; not a plugin"),
    ("nexus-plugin-api", "stable plugin contract; not a plugin"),
    ("nexus-kernel", "the kernel itself; loads plugins"),
    ("nexus-plugins", "plugin loader; loads plugins"),
    ("nexus-kv", "KV-trait sqlite backend; injected into kernel"),
    ("nexus-panic-log", "stand-alone panic hook; not a plugin"),
    ("nexus-fuzz", "security fuzz harness; not a plugin"),
    ("nexus-crdt", "operation-based CRDT library used by editor"),
    // Bootstrap itself.
    ("nexus-bootstrap", "the orchestrator that registers plugins"),
    // Frontend / IPC-proxy crates — consume Runtime, not registered as
    // a CorePlugin. The dep_invariants `IPC_PROXY_ALLOWLIST` covers
    // the per-proxy dep contract.
    ("nexus-cli", "frontend binary"),
    ("nexus-tui", "frontend binary"),
    ("nexus-mcp", "IPC proxy (MCP server)"),
    ("nexus-acp", "IPC proxy (ACP host/server)"),
    ("nexus-remote", "IPC proxy (remote-forge JSON-RPC server)"),
    // Not-yet-wired subsystems tracked by #188 / R5. Removing one of
    // these rows requires landing the corresponding registrar file in
    // `crates/nexus-bootstrap/src/plugins/`.
    (
        "nexus-memory",
        "Move 4 AI memory layer; bootstrap wiring pending (#188)",
    ),
    (
        "nexus-context",
        "Move 6 context-assembly pipeline; bootstrap wiring pending (#188)",
    ),
    (
        "nexus-protocol",
        "Move 7 speech-act protocol; bootstrap wiring pending (#188)",
    ),
];

#[test]
fn every_workspace_member_is_registered_or_exempt() {
    let workspace_root = workspace_root();

    // Workspace members from the root Cargo.toml `[workspace] members`.
    let cargo_toml = workspace_root.join("Cargo.toml");
    let text = std::fs::read_to_string(&cargo_toml)
        .unwrap_or_else(|e| panic!("read {}: {e}", cargo_toml.display()));
    let parsed: toml::Value = toml::from_str(&text)
        .unwrap_or_else(|e| panic!("parse {}: {e}", cargo_toml.display()));
    let members: Vec<String> = parsed
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|p| {
                    // members look like "crates/nexus-foo"; strip the
                    // prefix to get the crate name.
                    p.rsplit('/').next().unwrap_or(p).to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !members.is_empty(),
        "[workspace] members section is empty — root Cargo.toml parse changed shape?"
    );

    // Registrar files in `crates/nexus-bootstrap/src/plugins/*.rs`.
    // Each file `foo.rs` registers the crate `nexus-foo` (or
    // `nexus-ai-runtime` for `ai_runtime.rs` — the underscore/hyphen
    // distinction is handled below). `mod.rs` is the orchestrator,
    // not a per-plugin registrar.
    let registrars_dir = workspace_root.join("crates/nexus-bootstrap/src/plugins");
    let mut registered: HashSet<String> = HashSet::new();
    for entry in std::fs::read_dir(&registrars_dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", registrars_dir.display()))
    {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some("mod") => continue,
            Some(s) => s,
            None => continue,
        };
        // `ai_runtime.rs` → `nexus-ai-runtime`. Replace `_` with `-`.
        let crate_name = format!("nexus-{}", stem.replace('_', "-"));
        registered.insert(crate_name);
    }

    let exempt: HashSet<&str> = EXEMPT_CRATES.iter().map(|(n, _)| *n).collect();

    let mut missing = Vec::new();
    for member in &members {
        if registered.contains(member) || exempt.contains(member.as_str()) {
            continue;
        }
        missing.push(member.clone());
    }
    assert!(
        missing.is_empty(),
        "the following workspace members are neither registered by bootstrap nor on EXEMPT_CRATES:\n  {}\n\
         Either add `crates/nexus-bootstrap/src/plugins/<crate>.rs` wired into `register_all`, or \
         add an EXEMPT_CRATES row with a documented reason.",
        missing.join("\n  "),
    );

    // Symmetry check: every EXEMPT entry must correspond to an actual
    // workspace member, so stale rows don't accumulate as crates get
    // renamed or removed.
    let members_set: HashSet<&str> = members.iter().map(String::as_str).collect();
    let mut stale = Vec::new();
    for (name, _) in EXEMPT_CRATES {
        if !members_set.contains(name) {
            stale.push(*name);
        }
    }
    assert!(
        stale.is_empty(),
        "EXEMPT_CRATES references crates that are no longer workspace members:\n  {}",
        stale.join("\n  "),
    );
}

/// Walk up from `CARGO_MANIFEST_DIR` to the workspace root.
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
