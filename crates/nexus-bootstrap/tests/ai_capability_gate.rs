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
    // ADR 0022 Phase 2: agent planner advertises write_file so the
    // model can produce write tool-use blocks (executor approves
    // per step). MCP reach is opt-in per call, NOT a default.
    assert!(caps.contains(Capability::AiToolsWrite));
    assert!(!caps.contains(Capability::AiToolsMcp));
}

/// ADR 0022 Phase 2 — args-aware tool-policy enforcement on
/// `stream_chat` and `propose_tool_calls`. White-box test against
/// the loader's [`required_caller_caps_for_args`] surface.
#[test]
fn loader_args_aware_tool_policy_requires_correct_caps() {
    use serde_json::json;

    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime = nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf())
        .expect("build runtime");
    let loader: Arc<dyn IpcDispatcher> =
        Arc::clone(&runtime.loader) as Arc<dyn IpcDispatcher>;

    // Helper: caps required for stream_chat with the given `tools` arg.
    let caps_for = |tools: serde_json::Value| {
        let args = json!({ "messages": [], "tools": tools });
        loader.required_caller_caps_for_args("com.nexus.ai", "stream_chat", &args)
    };
    let caps_for_default = || {
        // No `tools` field → policy defaults to Auto.
        let args = json!({ "messages": [] });
        loader.required_caller_caps_for_args("com.nexus.ai", "stream_chat", &args)
    };

    // Phase 1 floor: every variant requires AiChat.
    for case in [json!("auto"), json!("none"), json!("auto_with_mcp"), json!("auto_readonly")] {
        let caps = caps_for(case.clone());
        assert!(caps.contains(&Capability::AiChat), "AiChat missing for {case}: {caps:?}");
    }

    // Phase 2 deltas.
    let auto = caps_for(json!("auto"));
    assert!(auto.contains(&Capability::AiToolsWrite), "auto must require AiToolsWrite");
    assert!(!auto.contains(&Capability::AiToolsMcp), "auto must NOT require AiToolsMcp");

    let auto_mcp = caps_for(json!("auto_with_mcp"));
    assert!(auto_mcp.contains(&Capability::AiToolsWrite));
    assert!(auto_mcp.contains(&Capability::AiToolsMcp));

    let none = caps_for(json!("none"));
    assert!(!none.contains(&Capability::AiToolsWrite));
    assert!(!none.contains(&Capability::AiToolsMcp));

    let readonly = caps_for(json!("auto_readonly"));
    assert!(!readonly.contains(&Capability::AiToolsWrite));
    assert!(!readonly.contains(&Capability::AiToolsMcp));

    // Default = Auto when omitted.
    let default = caps_for_default();
    assert!(default.contains(&Capability::AiToolsWrite));

    // propose_tool_calls shares the same closure.
    let propose_args = json!({ "messages": [], "tools": "auto_with_mcp" });
    let propose_caps =
        loader.required_caller_caps_for_args("com.nexus.ai", "propose_tool_calls", &propose_args);
    assert!(propose_caps.contains(&Capability::AiChat));
    assert!(propose_caps.contains(&Capability::AiToolsWrite));
    assert!(propose_caps.contains(&Capability::AiToolsMcp));
}
