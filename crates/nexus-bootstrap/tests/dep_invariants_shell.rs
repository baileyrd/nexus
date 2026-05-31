//! AA-02 / P3-02 — dependency invariants for the Tauri shell crate.
//!
//! The shell (`shell/src-tauri/Cargo.toml`) sits outside the Cargo
//! workspace (`Cargo.toml` has `exclude = ["shell"]`), so the existing
//! `dep_invariants.rs` walk over `crates/<name>/Cargo.toml` cannot see
//! it. The shell is the highest-impact IPC consumer in the tree — if a
//! subsystem engine ever gets linked into the desktop binary, every
//! Tauri command bypasses the kernel's capability gate. That's exactly
//! the regression `dep_invariants.rs` was built to prevent.
//!
//! This sibling test reads the shell manifest at a path relative to
//! `CARGO_MANIFEST_DIR` (`<workspace>/crates/nexus-bootstrap`) and
//! enforces the same posture: only kernel + bootstrap + plugin contract
//! crates may be linked. Subsystem engines must be reached through
//! `kernel_invoke` → `context.ipc_call(...)` exactly like CLI / TUI /
//! MCP.
//!
//! The single intentional exception is `nexus-remote`: BL-140 Phase 3
//! wires `boot_remote` / `kernel_invoke` over `ssh://` URIs through it
//! from the shell directly, matching the same "JSON-RPC proxy"
//! posture documented in `dep_invariants.rs`. Add similar exceptions
//! here only with a comment explaining the architectural rationale.

use std::path::PathBuf;

/// Subsystem crates that must NOT appear in
/// `shell/src-tauri/Cargo.toml::[dependencies]`. Linking any of these
/// directly into the Tauri binary bypasses the kernel — route through
/// `kernel_invoke` (which calls `context.ipc_call(...)`) instead.
const FORBIDDEN_FOR_SHELL: &[&str] = &[
    "nexus-storage",
    "nexus-ai",
    "nexus-ai-runtime",
    "nexus-editor",
    "nexus-git",
    "nexus-database",
    "nexus-terminal",
    "nexus-mcp",
    "nexus-acp",
    "nexus-lsp",
    "nexus-dap",
    "nexus-agent",
    "nexus-skills",
    "nexus-templates",
    "nexus-workflow",
    "nexus-linkpreview",
    "nexus-notifications",
    "nexus-comments",
    "nexus-audio",
    "nexus-collab",
    "nexus-theme",
    "nexus-crdt",
    "nexus-security",
    "nexus-formats",
    "nexus-kv",
];

#[test]
fn shell_tauri_crate_does_not_link_subsystem_engines() {
    let manifest = shell_manifest_path();
    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", manifest.display());
    });
    let parsed: toml::Value = toml::from_str(&text).unwrap_or_else(|e| {
        panic!("failed to parse {}: {e}", manifest.display());
    });

    let mut violations = Vec::new();

    // Top-level `[dependencies]`.
    if let Some(deps) = parsed.get("dependencies").and_then(toml::Value::as_table) {
        for forbidden in FORBIDDEN_FOR_SHELL {
            if deps.contains_key(*forbidden) {
                violations.push(format!(
                    "  shell/src-tauri/Cargo.toml: [dependencies].{forbidden} \
                     is forbidden — route through kernel_invoke + \
                     context.ipc_call() via nexus-bootstrap instead."
                ));
            }
        }
    }

    // Target-conditional `[target.'cfg(...)'.dependencies]` — mirror
    // the loophole closed in `dep_invariants.rs` (issue #83).
    if let Some(target) = parsed.get("target").and_then(toml::Value::as_table) {
        for (cfg_key, cfg_block) in target {
            if let Some(deps) = cfg_block
                .get("dependencies")
                .and_then(toml::Value::as_table)
            {
                for forbidden in FORBIDDEN_FOR_SHELL {
                    if deps.contains_key(*forbidden) {
                        violations.push(format!(
                            "  shell/src-tauri/Cargo.toml: \
                             [target.'{cfg_key}'.dependencies].{forbidden} \
                             is forbidden — route through kernel_invoke + \
                             context.ipc_call() via nexus-bootstrap instead."
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "shell dependency invariants violated:\n{}",
        violations.join("\n"),
    );
}

/// Walk up from `CARGO_MANIFEST_DIR` (`<workspace>/crates/nexus-bootstrap`)
/// to the workspace root, then resolve `shell/src-tauri/Cargo.toml`.
/// Mirrors the `workspace_root` helper in `dep_invariants.rs`.
fn shell_manifest_path() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                if text.contains("[workspace]") {
                    return dir.join("shell").join("src-tauri").join("Cargo.toml");
                }
            }
        }
        if !dir.pop() {
            panic!("could not locate workspace root starting from CARGO_MANIFEST_DIR");
        }
    }
}
