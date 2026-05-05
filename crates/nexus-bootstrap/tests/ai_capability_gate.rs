//! Regression test for ADR 0022 — per-handler capabilities for the
//! AI plugin.
//!
//! Pre-0022, any caller holding `IpcCall` could invoke any
//! `com.nexus.ai` handler. That meant a user-authored
//! `.workflows/*.toml` step or an LLM-generated agent tool call
//! could rotate provider credentials (`set_config`), wipe the
//! activity log (`activity_clear`), or destroy a chat session
//! (`session_delete`).
//!
//! Bootstrap now registers per-(target, command) caps via
//! `SharedPluginLoader::add_cap_requirement` for every gated AI
//! handler. This test pins the loader-side wiring directly — if a
//! future refactor accidentally drops one of the
//! `add_cap_requirement` calls, this test fails before any of the
//! integration paths can mask the regression.

use std::sync::Arc;

use nexus_kernel::{Capability, IpcDispatcher};

/// Mapping of `com.nexus.ai` handler -> required caller cap, per
/// ADR 0022 §"Capability inventory".
const AI_GATES: &[(&str, Capability)] = &[
    ("stream_chat", Capability::AiChat),
    ("stream_ask", Capability::AiChat),
    ("ask", Capability::AiChat),
    ("semantic_search", Capability::AiChat),
    ("enrich_file", Capability::AiChat),
    ("propose_tool_calls", Capability::AiChat),
    ("index_file", Capability::AiIndex),
    ("index_trigger", Capability::AiIndex),
    ("session_load", Capability::AiSessionRead),
    ("session_list", Capability::AiSessionRead),
    ("session_save", Capability::AiSessionWrite),
    ("session_delete", Capability::AiSessionWrite),
    ("set_config", Capability::AiConfigWrite),
    ("activity_clear", Capability::AiActivityWrite),
];

/// Handlers explicitly NOT gated by ADR 0022 — read-only or
/// downstream-gated. Pinned so a future "let's gate everything"
/// refactor doesn't quietly tighten these without an ADR update.
const AI_NOT_GATED: &[&str] = &[
    "status",
    "config",
    "index_status",
    "vectorstore_count",
    "activity_list",
    "apply",
];

#[test]
fn loader_gates_every_ai_handler_per_adr_0022() {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime = nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf())
        .expect("build runtime");
    let loader: Arc<dyn IpcDispatcher> =
        Arc::clone(&runtime.loader) as Arc<dyn IpcDispatcher>;

    for (command, expected) in AI_GATES {
        let caps = loader.required_caller_caps("com.nexus.ai", command);
        assert!(
            caps.contains(expected),
            "com.nexus.ai::{command} must require {expected:?}; got: {caps:?}"
        );
    }
}

#[test]
fn loader_does_not_gate_read_only_ai_handlers() {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime = nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf())
        .expect("build runtime");
    let loader: Arc<dyn IpcDispatcher> =
        Arc::clone(&runtime.loader) as Arc<dyn IpcDispatcher>;

    for command in AI_NOT_GATED {
        let caps = loader.required_caller_caps("com.nexus.ai", command);
        assert!(
            caps.is_empty(),
            "com.nexus.ai::{command} must NOT have a cap requirement (read-only / downstream-gated); got: {caps:?}"
        );
    }
}

#[test]
fn workflow_caps_include_ai_chat_but_not_ai_writes() {
    // ADR 0022 §"Rollout" Phase 1 — workflow contexts gain ai.chat so
    // existing `ai_prompt` steps keep working, but explicitly do NOT
    // get ai.config.write / ai.activity.write / ai.session.write.
    let caps = nexus_bootstrap::workflow_capabilities();
    assert!(caps.contains(Capability::AiChat));
    assert!(!caps.contains(Capability::AiConfigWrite));
    assert!(!caps.contains(Capability::AiActivityWrite));
    assert!(!caps.contains(Capability::AiSessionWrite));
}

#[test]
fn agent_caps_include_ai_chat_but_not_ai_writes() {
    let caps = nexus_bootstrap::agent_capabilities();
    assert!(caps.contains(Capability::AiChat));
    assert!(!caps.contains(Capability::AiConfigWrite));
    assert!(!caps.contains(Capability::AiActivityWrite));
    assert!(!caps.contains(Capability::AiSessionWrite));
}
