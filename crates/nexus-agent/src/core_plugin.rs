//! Core plugin wrapping the agent library.
//!
//! Registers as `com.nexus.agent`. Holds a [`KernelPluginContext`]
//! (supplied via [`CorePlugin::wire_context`] at bootstrap) so its
//! handlers can drive two bridges against the live runtime:
//!
//! - [`nexus_bootstrap::agent::AiChatDriver`]-shaped adapter over
//!   `com.nexus.ai::stream_chat` for planning.
//! - [`nexus_bootstrap::agent::KernelToolDispatcher`]-shaped adapter
//!   over `PluginContext::ipc_call` for executing plan steps.
//!
//! Because this module lives in `nexus-agent`, it re-implements the
//! two adapter shapes locally — keeping the library itself
//! kernel-free would otherwise force a circular dep on
//! `nexus-bootstrap`. The bridges here and in bootstrap are
//! intentionally identical in behaviour.
//!
//! # Handlers
//!
//! | Handler id | Command             | Purpose                               |
//! |-----------:|---------------------|---------------------------------------|
//! | 1          | `plan`              | Produce a [`Plan`] from a goal        |
//! | 2          | `run`               | Plan + execute; return Observation    |
//! | 3          | `run_plan`          | Execute a preset [`Plan`]             |
//! | 4          | `execute_step`      | Execute a single preset-plan step     |
//! | 5          | `history_list`      | List persisted plan histories         |
//! | 6          | `history_get`       | Load one persisted history entry      |
//! | 7          | `history_delete`    | Remove one persisted history entry    |
//! | 8          | `list_archetypes`   | Return the catalogue of archetype ids |
//!
//! Ids are append-only.

use std::sync::Arc;

use nexus_kernel::KernelPluginContext;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};

use crate::handlers;
use crate::handlers::shared::{exec_err, PendingApprovals};

// Re-export the public handler args / helpers historically exposed
// from this module so downstream callers and integration tests keep
// compiling without churn.
pub use crate::handlers::delegate::DelegateArgs;
pub use crate::handlers::history::PlanIdArgs;
pub use crate::handlers::list_tools::ListToolsArgs;
pub use crate::handlers::memory::{
    MemoryExportArgs, MemoryPruneArgs, MemoryQueryArgs, MemoryRecordArgs,
};
pub use crate::handlers::plan::{GoalArgs, PlanArgs};
pub use crate::handlers::shared::round_requires_approval;

/// Short archetype names accepted by [`crate::archetypes::resolve_prompt`].
/// Exposed via the `list_archetypes` handler so the shell's picker can
/// send any of these back as the `archetype` arg to `plan` / `run`
/// without guessing the expected case or prefix.
const ARCHETYPE_NAMES: &[&str] = &[
    "writer",
    "coder",
    "researcher",
    "auditor",
    "librarian",
    "coach",
];

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.agent";

/// `plan` handler id — produce a plan for the given goal.
pub const HANDLER_PLAN: u32 = 1;
// Handler ids 2 (`run`), 3 (`run_plan`), 4 (`execute_step`),
// 9 (`delegate`), 10 (`parallel`), 11 (`pipeline`), 12 (`trace_get`)
// were retired by ADR 0025 Phase 2. The ids stay reserved — adding
// a new handler here should pick a fresh id rather than re-using
// these slots.
/// `history_list` handler id — enumerate persisted plan histories
/// under `<forge>/.forge/agent/history/`.
pub const HANDLER_HISTORY_LIST: u32 = 5;
/// `history_get` handler id — load one persisted history entry by
/// plan id.
pub const HANDLER_HISTORY_GET: u32 = 6;
/// `history_delete` handler id — remove one persisted history entry.
pub const HANDLER_HISTORY_DELETE: u32 = 7;
/// `list_archetypes` handler id — return the catalogue of archetype
/// ids the agent library knows about (OI-04). Payload: `[]`. Result:
/// `Vec<String>` — fully-qualified archetype ids (e.g.
/// `com.nexus.agent.writer`). The shell uses this to populate the
/// archetype picker without a hardcoded catalogue.
pub const HANDLER_LIST_ARCHETYPES: u32 = 8;

/// `session_run` (ADR 0024 Phase 2a) — drive a multi-round
/// tool-loop session and persist the transcript. Args:
/// `{ goal: string, archetype?: string, system?: string,
///    auto_approve: bool }`.
pub const HANDLER_SESSION_RUN: u32 = 13;
/// `session_list` — enumerate persisted session transcripts under
/// `<forge>/.forge/agent/sessions/`.
pub const HANDLER_SESSION_LIST: u32 = 14;
/// `session_get` — load one session transcript by id.
pub const HANDLER_SESSION_GET: u32 = 15;
/// `session_delete` — remove one session transcript.
pub const HANDLER_SESSION_DELETE: u32 = 16;
/// `round_decide` (ADR 0024 Phase 2b) — caller pushes a
/// [`crate::RoundDecision`]-shaped reply for a pending session round.
pub const HANDLER_ROUND_DECIDE: u32 = 17;

/// `list_tools` (DG-32 — PRD-15 §4) — return the agent tool registry.
pub const HANDLER_LIST_TOOLS: u32 = 18;
/// `list_custom` (DG-36 — PRD-15 §9) — scan
/// `<forge>/.forge/agents/*/agent.toml` and return parsed manifests.
pub const HANDLER_LIST_CUSTOM: u32 = 19;

/// `memory_record` (DG-33 — PRD-15 §5) — append a `MemoryEntry`.
pub const HANDLER_MEMORY_RECORD: u32 = 20;
/// `memory_query` (DG-33) — return entries matching a substring pattern.
pub const HANDLER_MEMORY_QUERY: u32 = 21;
/// `memory_prune` (DG-33) — drop entries older than `retention_ms`,
/// preserving `Decision` entries indefinitely per PRD-15 §5.
pub const HANDLER_MEMORY_PRUNE: u32 = 22;
/// `memory_export` (DG-33) — render the agent's history as markdown.
pub const HANDLER_MEMORY_EXPORT: u32 = 23;

/// `delegate` (DG-37 — PRD-15 §10) — run a sub-session in a child
/// archetype and return its `AgentSession` transcript.
pub const HANDLER_DELEGATE: u32 = 24;

/// BL-121 — `search_transcripts`.
pub const HANDLER_SEARCH_TRANSCRIPTS: u32 = 25;

/// Core plugin instance.
pub struct AgentCorePlugin {
    context: Option<Arc<KernelPluginContext>>,
    pending_approvals: Arc<PendingApprovals>,
    /// BL-121 — absolute forge root used to open the FTS index at
    /// `<forge>/.forge/agent/transcripts.sqlite`. `None` for the
    /// legacy default constructor (which keeps every existing
    /// caller compiling); bootstrap calls [`Self::new_with_forge`]
    /// so the index actually lands.
    forge_root: Option<std::path::PathBuf>,
}

impl AgentCorePlugin {
    /// Construct an unwired plugin. Bootstrap must call
    /// [`CorePlugin::wire_context`] before the first dispatch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            context: None,
            pending_approvals: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            forge_root: None,
        }
    }

    /// Like [`Self::new`] but captures the forge root so BL-121's
    /// transcript-search store opens against the right `.sqlite`
    /// file at `on_init`.
    #[must_use]
    pub fn new_with_forge(forge_root: std::path::PathBuf) -> Self {
        Self {
            context: None,
            pending_approvals: Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            forge_root: Some(forge_root),
        }
    }
}

impl Default for AgentCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePlugin for AgentCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        // BL-121 — open the FTS5 transcript index and rebuild from
        // disk if empty. Failures here are non-fatal.
        if let Some(forge_root) = &self.forge_root {
            match crate::transcript_search::initialize(forge_root) {
                Ok(_) => tracing::debug!(
                    plugin_id = PLUGIN_ID,
                    "BL-121 transcript search index ready",
                ),
                Err(err) => tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    %err,
                    "BL-121: transcript search index unavailable; search_transcripts will return an empty result"
                ),
            }
        }
        Ok(())
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        // `list_archetypes` and `list_tools` are the two sync handlers
        // on this plugin — both read only from compile-time / in-memory
        // state, so there's no reason to burn an async hop. Every other
        // handler is kernel-context-dependent and lives in
        // `dispatch_async`.
        if handler_id == HANDLER_LIST_ARCHETYPES {
            return Ok(serde_json::json!(ARCHETYPE_NAMES));
        }
        if handler_id == HANDLER_LIST_TOOLS {
            return handlers::list_tools::handle_list_tools(_args);
        }
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "handler {handler_id}: agent commands are async; caller should use dispatch_async"
            ),
        })
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        // Let the sync path handle `list_archetypes` and `list_tools`.
        if handler_id == HANDLER_LIST_ARCHETYPES || handler_id == HANDLER_LIST_TOOLS {
            return None;
        }

        let ctx = self.context.clone();
        let pending_approvals = Arc::clone(&self.pending_approvals);
        let args = args.clone();
        Some(Box::pin(async move {
            let ctx = ctx.ok_or_else(|| {
                exec_err("agent plugin context not wired (bootstrap incomplete)".into())
            })?;
            match handler_id {
                HANDLER_PLAN => handlers::plan::handle_plan(ctx, &args).await,
                HANDLER_HISTORY_LIST => handlers::history::handle_history_list(ctx).await,
                HANDLER_HISTORY_GET => {
                    handlers::history::handle_history_get(ctx, &args).await
                }
                HANDLER_HISTORY_DELETE => {
                    handlers::history::handle_history_delete(ctx, &args).await
                }
                HANDLER_SESSION_RUN => {
                    handlers::session::handle_session_run(ctx, pending_approvals, &args).await
                }
                HANDLER_SESSION_LIST => handlers::session::handle_session_list(ctx).await,
                HANDLER_SESSION_GET => {
                    handlers::session::handle_session_get(ctx, &args).await
                }
                HANDLER_SESSION_DELETE => {
                    handlers::session::handle_session_delete(ctx, &args).await
                }
                HANDLER_ROUND_DECIDE => {
                    handlers::round::handle_round_decide(pending_approvals, &args).await
                }
                HANDLER_LIST_CUSTOM => handlers::custom::handle_list_custom(ctx).await,
                HANDLER_MEMORY_RECORD => {
                    handlers::memory::handle_memory_record(ctx, &args).await
                }
                HANDLER_MEMORY_QUERY => {
                    handlers::memory::handle_memory_query(ctx, &args).await
                }
                HANDLER_MEMORY_PRUNE => {
                    handlers::memory::handle_memory_prune(ctx, &args).await
                }
                HANDLER_MEMORY_EXPORT => {
                    handlers::memory::handle_memory_export(ctx, &args).await
                }
                HANDLER_DELEGATE => {
                    handlers::delegate::handle_delegate(ctx, pending_approvals, &args).await
                }
                HANDLER_SEARCH_TRANSCRIPTS => {
                    handlers::search_transcripts::handle_search_transcripts(&args)
                }
                other => Err(exec_err(format!("unknown handler id {other}"))),
            }
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(ctx);
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::handlers::round::handle_round_decide;
    use crate::handlers::shared::{format_entity_preamble, is_safe_archetype_slug};

    // ── BL-128 thin slice — entity preamble renderer ─────────────────────────

    #[test]
    fn format_entity_preamble_returns_none_for_empty_hits() {
        assert!(format_entity_preamble(&[]).is_none());
    }

    #[test]
    fn format_entity_preamble_skips_hits_missing_id() {
        let hits = vec![serde_json::json!({ "entity_type": "person" })];
        assert!(format_entity_preamble(&hits).is_none());
    }

    #[test]
    fn format_entity_preamble_renders_known_entities_block() {
        let hits = vec![
            serde_json::json!({
                "id": "alice",
                "entity_type": "person",
                "description": "Engineer working on nexus.",
            }),
            serde_json::json!({
                "id": "nexus",
                "entity_type": "project",
                "description": "",
            }),
        ];
        let out = format_entity_preamble(&hits).expect("preamble");
        assert!(out.starts_with("Known entities relevant to this goal"));
        assert!(out.contains("- alice (person): Engineer working on nexus."));
        assert!(out.contains("- nexus (project)"));
        assert!(!out.contains("- nexus (project):"));
    }

    /// DG-37 — `delegate` rejects an empty archetype name.
    #[tokio::test]
    async fn delegate_rejects_empty_archetype() {
        let pending: Arc<PendingApprovals> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let args = serde_json::json!({
            "archetype": "",
            "goal": "do a thing",
        });
        let parsed: DelegateArgs = serde_json::from_value(args).expect("decode");
        assert!(parsed.archetype.trim().is_empty());
        let _ = pending;
    }

    /// DG-37 — `delegate` rejects an empty goal.
    #[test]
    fn delegate_rejects_empty_goal_at_parse() {
        let args = serde_json::json!({
            "archetype": "coder",
            "goal": "   ",
        });
        let parsed: DelegateArgs = serde_json::from_value(args).expect("decode");
        assert!(parsed.goal.trim().is_empty());
    }

    /// DG-37 — `auto_approve` defaults to true.
    #[test]
    fn delegate_defaults_auto_approve_to_true() {
        let args = serde_json::json!({
            "archetype": "coder",
            "goal": "do thing",
        });
        let parsed: DelegateArgs = serde_json::from_value(args).expect("decode");
        assert!(parsed.auto_approve);
    }

    // ── DG-36 follow-up — slug validation ──────────────────────────────────

    #[test]
    fn is_safe_archetype_slug_accepts_typical_slugs() {
        for s in ["rust-reviewer", "auditor_v2", "code_review_2", "abc"] {
            assert!(is_safe_archetype_slug(s), "{s} should be safe");
        }
    }

    #[test]
    fn is_safe_archetype_slug_rejects_path_shaped_slugs() {
        for s in [
            "",
            "../etc/passwd",
            "..",
            "foo/bar",
            "foo\\bar",
            ".hidden",
            ".",
            "..pwn",
            "ok/..",
        ] {
            assert!(!is_safe_archetype_slug(s), "{s} should be rejected");
        }
    }

    /// DG-37 — the agent tool registry lists `delegate_to_agent`.
    #[test]
    fn delegate_to_agent_tool_seeded() {
        crate::seed_default_tools();
        let spec = crate::AgentToolRegistry::global()
            .lookup("delegate_to_agent")
            .expect("delegate_to_agent registered");
        assert_eq!(spec.target_plugin_id, "com.nexus.agent");
        assert_eq!(spec.command_id, "delegate");
        assert!(spec.requires_approval);
    }

    /// DG-34 — empty rounds never need approval.
    #[test]
    fn round_with_no_tool_calls_does_not_require_approval() {
        crate::seed_default_tools();
        let registry = crate::AgentToolRegistry::global();
        let round = crate::ProposedRound {
            round: 1,
            text: "all done".into(),
            tool_calls: Vec::new(),
        };
        assert!(!round_requires_approval(&round, &registry));
    }

    /// DG-34 — a round made up of read-only tools auto-approves.
    #[test]
    fn round_with_only_read_only_tools_does_not_require_approval() {
        crate::seed_default_tools();
        let registry = crate::AgentToolRegistry::global();
        let round = crate::ProposedRound {
            round: 1,
            text: String::new(),
            tool_calls: vec![
                crate::ProposedToolCall {
                    id: "1".into(),
                    name: "read_file".into(),
                    tool_call: crate::ToolCall {
                        target_plugin_id: "com.nexus.storage".into(),
                        command_id: "read_file".into(),
                        args: serde_json::json!({ "path": "x.md" }),
                    },
                },
                crate::ProposedToolCall {
                    id: "2".into(),
                    name: "search_forge".into(),
                    tool_call: crate::ToolCall {
                        target_plugin_id: "com.nexus.storage".into(),
                        command_id: "search".into(),
                        args: serde_json::json!({ "query": "foo" }),
                    },
                },
            ],
        };
        assert!(!round_requires_approval(&round, &registry));
    }

    /// DG-34 — a single high-risk tool call flips the round.
    #[test]
    fn round_with_any_write_tool_requires_approval() {
        crate::seed_default_tools();
        let registry = crate::AgentToolRegistry::global();
        let round = crate::ProposedRound {
            round: 1,
            text: String::new(),
            tool_calls: vec![
                crate::ProposedToolCall {
                    id: "1".into(),
                    name: "read_file".into(),
                    tool_call: crate::ToolCall {
                        target_plugin_id: "com.nexus.storage".into(),
                        command_id: "read_file".into(),
                        args: serde_json::json!({ "path": "x.md" }),
                    },
                },
                crate::ProposedToolCall {
                    id: "2".into(),
                    name: "write_file".into(),
                    tool_call: crate::ToolCall {
                        target_plugin_id: "com.nexus.storage".into(),
                        command_id: "write_file".into(),
                        args: serde_json::json!({
                            "path": "x.md",
                            "content": "..."
                        }),
                    },
                },
            ],
        };
        assert!(round_requires_approval(&round, &registry));
    }

    /// DG-34 — unknown tool names are conservatively treated as high-risk.
    #[test]
    fn round_with_unregistered_tool_requires_approval() {
        crate::seed_default_tools();
        let registry = crate::AgentToolRegistry::global();
        let round = crate::ProposedRound {
            round: 1,
            text: String::new(),
            tool_calls: vec![crate::ProposedToolCall {
                id: "1".into(),
                name: "exotic_tool_never_registered".into(),
                tool_call: crate::ToolCall {
                    target_plugin_id: "com.unknown".into(),
                    command_id: "do".into(),
                    args: serde_json::json!({}),
                },
            }],
        };
        assert!(round_requires_approval(&round, &registry));
    }

    /// OI-04 — `list_archetypes` returns the short-name catalogue.
    #[test]
    fn list_archetypes_returns_short_names() {
        let mut plugin = AgentCorePlugin::new();
        let v = plugin
            .dispatch(HANDLER_LIST_ARCHETYPES, &serde_json::Value::Null)
            .expect("list_archetypes dispatch");
        let names: Vec<String> = serde_json::from_value(v).expect("decode");
        assert_eq!(
            names,
            vec!["writer", "coder", "researcher", "auditor", "librarian", "coach"]
        );
    }

    /// OI-04 — `dispatch_async` returns `None` for `list_archetypes`.
    #[test]
    fn dispatch_async_yields_to_sync_for_list_archetypes() {
        let mut plugin = AgentCorePlugin::new();
        let fut = plugin.dispatch_async(HANDLER_LIST_ARCHETYPES, &serde_json::Value::Null);
        assert!(fut.is_none(), "list_archetypes must not return an async future");
    }

    /// DG-32 — `list_tools` returns the agent tool registry's seeded catalogue.
    #[test]
    fn list_tools_returns_seeded_catalog() {
        crate::seed_default_tools();
        let mut plugin = AgentCorePlugin::new();
        let v = plugin
            .dispatch(HANDLER_LIST_TOOLS, &serde_json::Value::Null)
            .expect("list_tools dispatch");
        let arr = v.as_array().expect("array reply");
        let names: std::collections::HashSet<_> = arr
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        for expected in [
            "read_file",
            "write_file",
            "search_forge",
            "git_log",
            "terminal_run_saved",
        ] {
            assert!(
                names.contains(expected),
                "list_tools missing tool: {expected}"
            );
        }
    }

    /// DG-32 — `list_tools` honours the `capabilities` filter.
    #[test]
    fn list_tools_with_unknown_capability_errors() {
        crate::seed_default_tools();
        let mut plugin = AgentCorePlugin::new();
        let err = plugin
            .dispatch(
                HANDLER_LIST_TOOLS,
                &serde_json::json!({ "capabilities": ["bogus"] }),
            )
            .expect_err("should reject unknown capability");
        let msg = format!("{err}");
        assert!(msg.contains("unknown capability"), "got: {msg}");
    }

    /// DG-32 — Filtering by a held capability narrows the result.
    #[test]
    fn list_tools_with_capability_filter_narrows_catalog() {
        crate::seed_default_tools();
        let mut plugin = AgentCorePlugin::new();
        let v = plugin
            .dispatch(
                HANDLER_LIST_TOOLS,
                &serde_json::json!({ "capabilities": ["fs.read"] }),
            )
            .expect("list_tools dispatch");
        let arr = v.as_array().expect("array reply");
        let names: std::collections::HashSet<_> = arr
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert!(names.contains("read_file"));
        assert!(!names.contains("write_file"));
    }

    /// Phase 2b — `round_decide` routes the caller's decision.
    #[tokio::test]
    async fn round_decide_delivers_approve_all_to_pending_session() {
        let pending: Arc<PendingApprovals> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let (tx, rx) = tokio::sync::oneshot::channel::<crate::RoundDecision>();
        crate::handlers::shared::insert_pending_bounded(
            &mut pending.lock().unwrap(),
            "sess-abc".to_string(),
            tx,
        );

        let args = serde_json::json!({ "session_id": "sess-abc", "kind": "approve_all" });
        let reply = handle_round_decide(Arc::clone(&pending), &args)
            .await
            .expect("round_decide ok");
        assert_eq!(reply["delivered"], true);
        assert_eq!(reply["session_id"], "sess-abc");

        match rx.await.expect("oneshot recv") {
            crate::RoundDecision::ApproveAll => {}
            other => panic!("unexpected decision: {other:?}"),
        }
        assert!(pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn round_decide_errors_when_no_pending_session() {
        let pending: Arc<PendingApprovals> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let args = serde_json::json!({ "session_id": "ghost", "kind": "approve_all" });
        let err = handle_round_decide(pending, &args).await.unwrap_err();
        assert!(
            format!("{err:?}").contains("no pending approval"),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn round_decide_partial_threads_entries_through() {
        let pending: Arc<PendingApprovals> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let (tx, rx) = tokio::sync::oneshot::channel::<crate::RoundDecision>();
        crate::handlers::shared::insert_pending_bounded(
            &mut pending.lock().unwrap(),
            "sess-1".to_string(),
            tx,
        );

        let args = serde_json::json!({
            "session_id": "sess-1",
            "kind": "partial",
            "entries": [
                { "tool_use_id": "u1", "approve": true },
                { "tool_use_id": "u2", "approve": false, "reason": "too risky" }
            ]
        });
        let _ = handle_round_decide(Arc::clone(&pending), &args)
            .await
            .expect("decide ok");

        match rx.await.expect("recv") {
            crate::RoundDecision::Partial(entries) => {
                assert_eq!(entries.len(), 2);
                assert!(entries[0].approve);
                assert!(!entries[1].approve);
                assert_eq!(entries[1].reason, "too risky");
            }
            other => panic!("unexpected decision: {other:?}"),
        }
    }

    #[tokio::test]
    async fn round_decide_errors_when_receiver_already_dropped() {
        let pending: Arc<PendingApprovals> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let (tx, rx) = tokio::sync::oneshot::channel::<crate::RoundDecision>();
        crate::handlers::shared::insert_pending_bounded(
            &mut pending.lock().unwrap(),
            "sess-2".to_string(),
            tx,
        );
        drop(rx);

        let args = serde_json::json!({ "session_id": "sess-2", "kind": "approve_all" });
        let err = handle_round_decide(pending, &args).await.unwrap_err();
        assert!(
            format!("{err:?}").contains("no longer awaiting"),
            "{err:?}"
        );
    }
}
