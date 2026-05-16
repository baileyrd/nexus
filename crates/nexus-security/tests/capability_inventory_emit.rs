//! BL-137 — emit the canonical capability inventory as
//! `docs/generated/capabilities.md`.
//!
//! `Capability::ALL` is the single source of truth; `nexus_security::risk`
//! is the canonical risk-level mapping. This test reads both, materialises
//! a markdown table, and writes it to the docs tree. `scripts/check_ipc_drift.sh`
//! then runs `git diff --exit-code` on the docs tree, so an unrun
//! generator fails CI the same way an unregenerated TS binding does.
//!
//! When this test fails locally, run `scripts/check_ipc_drift.sh` and
//! commit the regenerated `docs/generated/capabilities.md`.

use std::fs;
use std::path::PathBuf;

use nexus_kernel::Capability;
use nexus_security::{risk_level, RiskLevel};

const OUTPUT_REL: &str = "docs/generated/capabilities.md";

#[test]
fn capability_inventory_table_is_in_sync() {
    let workspace = workspace_root();
    let out_path = workspace.join(OUTPUT_REL);
    let generated = generate();

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            panic!("create_dir_all {}: {}", parent.display(), e);
        });
    }
    fs::write(&out_path, &generated).unwrap_or_else(|e| {
        panic!("write {}: {}", out_path.display(), e);
    });

    // The drift check runs `git diff` after this test; the assert here is
    // a developer-friendly fast-fail if the file on disk didn't match what
    // we just wrote (e.g. permissions issue). Real drift is caught by
    // `check_ipc_drift.sh`.
    let on_disk = fs::read_to_string(&out_path).unwrap();
    assert_eq!(on_disk, generated, "wrote-vs-read mismatch (filesystem issue?)");
}

fn generate() -> String {
    let mut out = String::new();
    out.push_str("# Capability Inventory\n\n");
    out.push_str(
        "> Auto-generated from `nexus_kernel::Capability::ALL` + \
         `nexus_security::risk::risk_level`. Do not edit by hand — \
         regenerate via `scripts/check_ipc_drift.sh`.\n\n",
    );
    out.push_str("Filed under [BL-137](../PRDs/backlog/BL-137.md).\n\n");
    out.push_str("This is the canonical surface used at install time and at every kernel-mediated\n");
    out.push_str("operation. ADR 0002 and ADR 0022 carry the rationale; this file is the live\n");
    out.push_str("mirror.\n\n");
    out.push_str("| String | Variant | Risk |\n");
    out.push_str("|--------|---------|------|\n");

    for &cap in Capability::ALL {
        let risk = match risk_level(cap) {
            RiskLevel::Low => "Low",
            RiskLevel::Medium => "Medium",
            RiskLevel::High => "**High**",
        };
        // `{:?}` on the enum yields the variant name, e.g. `FsRead`.
        out.push_str(&format!(
            "| `{}` | `{:?}` | {} |\n",
            cap.as_str(),
            cap,
            risk,
        ));
    }

    out.push_str(&format!("\n_Total: {} capabilities._\n", Capability::ALL.len()));
    out
}

fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(text) = fs::read_to_string(&candidate) {
                if text.contains("[workspace]") {
                    return dir;
                }
            }
        }
        if !dir.pop() {
            panic!("failed to locate workspace root from {}", env!("CARGO_MANIFEST_DIR"));
        }
    }
}
