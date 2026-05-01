//! Plugin-contract purity guardrail (Phase 3 WI-34, MK F-2.1.1).
//!
//! Community-tier plugin crates are expected to pin **only** to the stable
//! contract crate `nexus-plugin-api`. They must not pull in the engine
//! internals behind the contract — `nexus-kernel`, `nexus-storage`,
//! `nexus-editor`, and friends. If a future "convenient" re-export ever
//! leaks an engine type through `nexus-plugin-api` (e.g.
//! `pub use nexus_kernel::EventBus;` in a moment of weakness), this test
//! fires at CI time.
//!
//! Three invariants are enforced:
//!
//! 1. `nexus-plugin-api` has no `[dependencies]` entry for any impl crate.
//! 2. `nexus-plugin-api/src/` contains no `pub use nexus_<impl>::…`
//!    re-exports (literal-string grep — complements the Cargo check).
//! 3. Every crate listed in `PLUGIN_PURITY_CRATES` (the opt-in roster of
//!    community-tier crates) has no direct dependency on any impl crate.
//!    The list is empty today — no community-tier crates ship in-tree yet
//!    per `docs/planning/PHASE-3-IMPLEMENTATION-PLAN.md` §3.3 and INTEGRATION-REVIEW
//!    §6. A stand-alone validator self-test (see `self_test_*`) still
//!    exercises the positive and negative paths so the guardrail is proven
//!    to work before the first community plugin lands.
//!
//! Precedent: `crates/nexus-bootstrap/tests/dep_invariants.rs` (Phase 1/B
//! dependency invariants) and `crates/nexus-bootstrap/tests/legacy_freeze.rs`
//! (Phase 1 WI-22 legacy-shell freeze) — same workspace-walk pattern.

use std::path::{Path, PathBuf};

/// The stable plugin-contract crate. Community plugins should pin this.
const CONTRACT_CRATE: &str = "nexus-plugin-api";

/// Engine / service impl crates that live behind the contract. A direct
/// `[dependencies]` entry on any of these from a community-tier crate (or
/// from the contract crate itself) is a layering violation — the contract
/// is meant to be the only surface plugin authors link.
///
/// Keep this list in sync with `crates/` whenever a new service crate
/// lands. Host / bootstrap / invoker crates (nexus-bootstrap, nexus-cli,
/// nexus-tui, nexus-plugins, nexus-types, nexus-formats, nexus-git,
/// nexus-linkpreview) are intentionally *not* listed — they are Tier 0/1
/// and may depend on engine internals. The legacy `nexus-app` host crate
/// was retired under Phase 4 WI-37 (2026-04-24).
const FORBIDDEN_IMPL_DEPS: &[&str] = &[
    "nexus-kernel",
    "nexus-storage",
    "nexus-editor",
    "nexus-ai",
    "nexus-agent",
    "nexus-terminal",
    "nexus-workflow",
    "nexus-skills",
    "nexus-mcp",
    "nexus-theme",
    "nexus-database",
    "nexus-kv",
    "nexus-security",
];

/// Opt-in list of community-tier crates. Empty today: no community-tier
/// crates ship in-tree. Future community crates are added here when they
/// land — shipping one becomes an explicit plan-doc decision because you
/// have to edit this list.
///
/// (Alternative considered: a `[package.metadata.nexus.tier = "community"]`
/// marker in each crate's Cargo.toml. Rejected for now — nothing to
/// migrate, opt-in list is simpler and equally forward-compatible.)
const PLUGIN_PURITY_CRATES: &[&str] = &[];

#[test]
fn contract_crate_does_not_depend_on_impl_crates() {
    let manifest_path = workspace_root()
        .join("crates")
        .join(CONTRACT_CRATE)
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", manifest_path.display()));
    let violations = find_forbidden_deps(&text, FORBIDDEN_IMPL_DEPS)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", manifest_path.display()));

    assert!(
        violations.is_empty(),
        "{CONTRACT_CRATE} must stay free of engine-impl deps but declared: {violations:?}\n\
         \n\
         The contract crate is the stable surface community plugins pin.\n\
         Pulling engine internals into it defeats the purpose — route any\n\
         new type through a trait or data-class defined in nexus-plugin-api\n\
         itself. See docs/planning/PHASE-3-IMPLEMENTATION-PLAN.md §3.3."
    );
}

#[test]
fn contract_crate_source_does_not_reexport_impl_crates() {
    let src_root = workspace_root()
        .join("crates")
        .join(CONTRACT_CRATE)
        .join("src");

    let mut violations = Vec::new();
    for forbidden in FORBIDDEN_IMPL_DEPS {
        // `nexus-kernel` on the dep side is `nexus_kernel` in Rust source.
        let crate_ident = forbidden.replace('-', "_");
        let needle = format!("pub use {crate_ident}::");
        find_literal_in_tree(&src_root, &needle, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "{CONTRACT_CRATE} must not re-export impl-crate items; found:\n{}\n\n\
         The contract crate is the stable plugin surface — re-exporting an\n\
         engine type pulls the engine into every plugin's dep tree. Define\n\
         the surface locally instead.",
        violations.join("\n"),
    );
}

#[test]
fn community_plugin_crates_do_not_depend_on_impl_crates() {
    let workspace_root = workspace_root();
    let mut violations = Vec::new();

    for crate_name in PLUGIN_PURITY_CRATES {
        let manifest_path = workspace_root
            .join("crates")
            .join(crate_name)
            .join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest_path).unwrap_or_else(|e| {
            panic!(
                "community-tier crate {crate_name} listed in PLUGIN_PURITY_CRATES \
                 but Cargo.toml not readable ({}): {e}",
                manifest_path.display()
            )
        });
        let forbidden = find_forbidden_deps(&text, FORBIDDEN_IMPL_DEPS)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", manifest_path.display()));
        for dep in forbidden {
            violations.push(format!(
                "  {crate_name}/Cargo.toml: [dependencies].{dep} is forbidden \
                 — community plugins pin nexus-plugin-api only."
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "plugin-contract purity violated:\n{}\n\n\
         Community-tier crates must reach the engine through nexus-plugin-api \
         / ipc_call only. See docs/planning/PHASE-3-IMPLEMENTATION-PLAN.md §3.3.",
        violations.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// Self-tests: prove the validator catches a forbidden dep and clears a clean
// manifest. These run with the rest of the test binary and guard against
// silently-broken checks (e.g. if someone rewrites `find_forbidden_deps` so
// it always returns empty).
// ---------------------------------------------------------------------------

#[test]
fn self_test_validator_flags_forbidden_dep() {
    let synthetic = r#"
[package]
name = "pretend-community-plugin"
version = "0.1.0"

[dependencies]
nexus-plugin-api = "0.1"
nexus-kernel = "0.1"   # forbidden — must be caught
"#;
    let hits = find_forbidden_deps(synthetic, FORBIDDEN_IMPL_DEPS)
        .expect("synthetic manifest should parse");
    assert_eq!(
        hits,
        vec!["nexus-kernel".to_string()],
        "validator failed to flag a forbidden direct dep"
    );
}

#[test]
fn self_test_validator_passes_clean_manifest() {
    let synthetic = r#"
[package]
name = "pretend-community-plugin"
version = "0.1.0"

[dependencies]
nexus-plugin-api = "0.1"
serde = "1"
"#;
    let hits = find_forbidden_deps(synthetic, FORBIDDEN_IMPL_DEPS)
        .expect("synthetic manifest should parse");
    assert!(
        hits.is_empty(),
        "validator incorrectly flagged clean manifest: {hits:?}"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a Cargo.toml and return the subset of `forbidden` that appear under
/// `[dependencies]`. `[dev-dependencies]` is intentionally *not* checked —
/// tests may spin up a real engine.
fn find_forbidden_deps(manifest_text: &str, forbidden: &[&str]) -> Result<Vec<String>, String> {
    let parsed: toml::Value = toml::from_str(manifest_text).map_err(|e| e.to_string())?;
    let deps = match parsed.get("dependencies") {
        Some(d) => d,
        None => return Ok(Vec::new()),
    };
    let mut hits = Vec::new();
    for name in forbidden {
        if deps.get(name).is_some() {
            hits.push((*name).to_string());
        }
    }
    Ok(hits)
}

/// Recursively walk `dir` looking for `needle` in any `.rs` file. Matches are
/// appended to `out` as `path:line-number: line-text`.
///
/// Substring matching is intentional and complementary to the Cargo.toml
/// dependency check elsewhere in this file. A determined evader could
/// dodge a `nexus_storage::` literal scan by writing
/// `use nexus_storage as backend; pub use backend::Engine;`, but the
/// re-export still requires `nexus-storage` in `[dependencies]`, which
/// the Cargo.toml-side test catches. Belt-and-suspenders: the literal
/// scan catches the common case (someone types the name directly) and
/// the dep-list check catches the exotic case (someone aliases). See
/// issue #83 for the audit context.
fn find_literal_in_tree(dir: &Path, needle: &str, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => panic!("failed to read {}: {e}", dir.display()),
    };
    for entry in entries {
        let entry = entry.expect("failed to read dir entry");
        let path = entry.path();
        let ft = entry.file_type().expect("failed to stat entry");
        if ft.is_dir() {
            find_literal_in_tree(&path, needle, out);
        } else if ft.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            for (idx, line) in text.lines().enumerate() {
                if line.contains(needle) {
                    out.push(format!(
                        "  {}:{}: {}",
                        path.display(),
                        idx + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
}

/// Walk up from `CARGO_MANIFEST_DIR` to the workspace root (directory holding
/// the top-level `Cargo.toml` with `[workspace]`).
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
