//! BL-138 — completeness guard for the per-handler capability matrix.
//!
//! Boots a full runtime, enumerates every `(plugin, command)` pair in
//! the live IPC registry, and fails if any handler lacks a
//! classification entry recorded by `cap_matrix::apply`. The
//! intended failure mode is "you added a new handler without
//! classifying it in `cap_matrix.toml` — go pick `caps = [...]` or
//! `unrestricted = \"…\"`."
//!
//! ## Phase 1 vs full migration
//!
//! BL-138 Phase 1 ships the matrix infrastructure plus the 17
//! historical `add_cap_requirement` entries. The remaining ~150+ IPC
//! handlers across `nexus-storage`, `nexus-editor`, `nexus-git`,
//! `nexus-comments`, etc. are still in the legacy "implicit
//! unrestricted, requires only `ipc.call`" state — moving them to
//! explicit `unrestricted = "<why>"` rows is the per-service-plugin
//! follow-up tracked under BL-138's DoD.
//!
//! Until those follow-ups land this test is `#[ignore]`d so the rest
//! of the BL-138 infra can ship. Run it locally with:
//!
//! ```text
//! cargo test -p nexus-bootstrap --test cap_matrix_complete -- --ignored
//! ```
//!
//! The body prints a sorted list of unclassified handlers, so the CI
//! failure for the eventual un-`ignore` is the entire fix.

use std::collections::BTreeSet;

use nexus_bootstrap::build_cli_runtime;

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

#[test]
#[ignore = "BL-138 Phase 1 — completeness gated on per-service-plugin unrestricted classifications; run with --ignored to see the punch list"]
fn every_ipc_handler_is_classified() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build cli runtime");

    let registered: Vec<(String, String)> = runtime.loader.lock().list_ipc_commands();

    let mut unclassified: BTreeSet<(String, String)> = BTreeSet::new();
    for (plugin, command) in &registered {
        if !runtime.loader.is_handler_classified(plugin, command) {
            unclassified.insert((plugin.clone(), command.clone()));
        }
    }

    if !unclassified.is_empty() {
        let mut msg = String::from(
            "\nBL-138 — the following IPC handlers are not classified in cap_matrix.toml.\n\
             Each one needs either `caps = [...]` or `unrestricted = \"<why>\"`:\n\n",
        );
        for (plugin, command) in &unclassified {
            msg.push_str(&format!("  - {plugin}::{command}\n"));
        }
        msg.push_str(&format!(
            "\n({} unclassified of {} registered)\n",
            unclassified.len(),
            registered.len(),
        ));
        panic!("{msg}");
    }
}

/// Phase-1 sanity check — runs unconditionally. Asserts that the
/// classification map is non-empty after a successful runtime build,
/// proving the matrix applied at least one row (i.e. nothing
/// regressed the wiring). This catches the failure mode where the
/// matrix file becomes empty / `apply` is silently skipped, without
/// requiring the full completeness sweep.
#[test]
fn cap_matrix_applies_at_least_one_classification() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build cli runtime");
    let classifications = runtime.loader.handler_classifications();
    assert!(
        !classifications.is_empty(),
        "cap_matrix::apply produced zero classifications — \
         is cap_matrix.toml empty or did bootstrap skip the apply pass?"
    );
}

/// Phase-1 regression guard — assert each of the 17 historical
/// `add_cap_requirement` entries is still classified after the
/// migration to `cap_matrix.toml`. Catches the failure mode where a
/// matrix row is accidentally deleted or its `(plugin, command)`
/// key drifts during a refactor.
#[test]
fn historical_cap_requirements_survive_migration() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build cli runtime");

    // Every handler that pre-BL-138 had an explicit `add_cap_requirement`
    // call. Listing the bare command (not its `.v1` alias) is enough —
    // the apply pass mirrors the classification onto every version
    // alias of the same handler.
    let historical: &[(&str, &str)] = &[
        ("com.nexus.terminal", "create_session"),
        ("com.nexus.mcp.host", "connect"),
        ("com.nexus.ai", "stream_chat"),
        ("com.nexus.ai", "stream_ask"),
        ("com.nexus.ai", "ask"),
        ("com.nexus.ai", "semantic_search"),
        ("com.nexus.ai", "enrich_file"),
        ("com.nexus.ai", "enrich_entity"),
        ("com.nexus.ai", "infer_entity_relations"),
        ("com.nexus.ai", "propose_tool_calls"),
        ("com.nexus.ai", "generate_docs"),
        ("com.nexus.ai", "index_file"),
        ("com.nexus.ai", "index_trigger"),
        ("com.nexus.ai", "session_load"),
        ("com.nexus.ai", "session_list"),
        ("com.nexus.ai", "session_save"),
        ("com.nexus.ai", "session_delete"),
        ("com.nexus.ai", "set_config"),
        ("com.nexus.ai", "activity_clear"),
        ("com.nexus.agent", "session_run"),
        ("com.nexus.agent", "round_decide"),
        ("com.nexus.audio", "transcribe"),
        ("com.nexus.audio", "synthesize"),
        ("com.nexus.ai.runtime", "submit"),
        ("com.nexus.ai.runtime", "cancel"),
        ("com.nexus.ai.runtime", "pause"),
        ("com.nexus.ai.runtime", "resume"),
        ("com.nexus.ai.runtime", "get"),
        ("com.nexus.ai.runtime", "list"),
        ("com.nexus.ai.runtime", "events"),
        ("com.nexus.ai.runtime", "pool_stats"),
        ("com.nexus.notifications", "inbox_list"),
        ("com.nexus.notifications", "inbox_stats"),
        ("com.nexus.notifications", "inbox_mark_read"),
        ("com.nexus.notifications", "inbox_dismiss"),
    ];

    let mut missing: Vec<(&&str, &&str)> = Vec::new();
    for (plugin, command) in historical {
        if !runtime.loader.is_handler_classified(plugin, command) {
            missing.push((plugin, command));
        }
    }

    assert!(
        missing.is_empty(),
        "BL-138 regression: historical cap_requirements lost classification: {missing:?}"
    );
}
