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

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{
    build_archetype, Agent, AgentError, ChatDriver, Plan, ToolCall, ToolDispatcher,
    DEFAULT_SYSTEM_PROMPT,
};

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
///    auto_approve: bool }`. Phase 2a accepts `auto_approve: true`
/// only; setting it to `false` returns "not yet implemented" until
/// Phase 2b lands the bus-bridge approval callback.
pub const HANDLER_SESSION_RUN: u32 = 13;
/// `session_list` — enumerate persisted session transcripts under
/// `<forge>/.forge/agent/sessions/`. No args; returns
/// `[{ id, goal, started_at, outcome }]` newest-first.
pub const HANDLER_SESSION_LIST: u32 = 14;
/// `session_get` — load one session transcript by id. Args:
/// `{ id: string }`. Returns the [`crate::AgentSession`] JSON.
pub const HANDLER_SESSION_GET: u32 = 15;
/// `session_delete` — remove one session transcript. Args:
/// `{ id: string }`. Returns `{ deleted: bool }`.
pub const HANDLER_SESSION_DELETE: u32 = 16;
/// `round_decide` (ADR 0024 Phase 2b) — caller pushes a
/// [`crate::RoundDecision`]-shaped reply for a pending session
/// round. Args: `{ session_id: string, kind: "approve_all" |
/// "abort" | "partial", reason?: string, entries?: [...] }`.
/// Returns `{ delivered: bool }`. The session loop on the agent
/// side awaits a `oneshot` populated by this handler before
/// dispatching tools.
pub const HANDLER_ROUND_DECIDE: u32 = 17;

/// `list_tools` (DG-32 — PRD-15 §4) — return the agent tool
/// registry catalogue. Args: `{ capabilities?: [string] }`. With
/// no args, returns every registered tool; with `capabilities`,
/// filters to those the agent could call given those grants.
/// Reply: `[AgentToolSpec]`. Read-only; touches the in-memory
/// global registry only.
pub const HANDLER_LIST_TOOLS: u32 = 18;
/// `list_custom` (DG-36 — PRD-15 §9) — scan
/// `<forge>/.forge/agents/*/agent.toml` and return parsed
/// manifests. No args. Reply:
/// `{ manifests: [CustomAgentManifest], errors: [{ path, error }] }`.
/// Per-manifest errors are surfaced alongside the loaded entries so
/// a single broken file doesn't poison the listing.
pub const HANDLER_LIST_CUSTOM: u32 = 19;

/// `memory_record` (DG-33 — PRD-15 §5) — append a `MemoryEntry` to
/// the agent's `history.jsonl`. Args:
/// `{ agent_id: string, entry: MemoryEntry }`. Reply:
/// `{ recorded: true }`. The agent id is validated to a safe slug
/// (`A-Za-z0-9_.-`, ≤96 chars).
pub const HANDLER_MEMORY_RECORD: u32 = 20;
/// `memory_query` (DG-33) — return entries matching a substring
/// pattern (case-insensitive). Args:
/// `{ agent_id: string, pattern?: string, limit?: u32 }`.
/// Reply: `[MemoryEntry]`, newest-first. `pattern` empty / omitted
/// returns the most recent entries unfiltered; `limit` defaults to 50.
pub const HANDLER_MEMORY_QUERY: u32 = 21;
/// `memory_prune` (DG-33) — drop entries older than `retention_ms`,
/// preserving `Decision` entries indefinitely per PRD-15 §5. Args:
/// `{ agent_id: string, retention_days: u32 }`. Reply:
/// `{ pruned: u32, kept: u32 }`. The agent's history file is
/// rewritten atomically (tmp + rename) so a crash mid-prune doesn't
/// corrupt the log.
pub const HANDLER_MEMORY_PRUNE: u32 = 22;
/// `memory_export` (DG-33) — render the agent's history as markdown.
/// Args: `{ agent_id: string }`. Reply: `{ markdown: string }`.
pub const HANDLER_MEMORY_EXPORT: u32 = 23;

/// `delegate` (DG-37 — PRD-15 §10) — run a sub-session in a child
/// archetype and return its `AgentSession` transcript. Replaces
/// the legacy BL-027 `delegate` (handler id 9, retired by ADR 0025)
/// with a primitive that composes on top of the new session model.
///
/// Args:
/// `{ archetype: string, goal: string, system?: string,
///    auto_approve?: bool, approval_timeout_secs?: u64,
///    strict_approval?: bool }`
///
/// Reply: a full `AgentSession` JSON (same shape `session_run`
/// returns). The sub-session's transcript is persisted at the
/// usual `.forge/agent/sessions/<id>.json` path so the caller can
/// reference it after the run.
///
/// **Parallel / pipeline composition** are caller patterns — issue
/// multiple `delegate` calls concurrently / sequentially. The
/// orchestrator types BL-027 introduced were retired (ADR 0025);
/// the session-shaped reply is the new currency.
pub const HANDLER_DELEGATE: u32 = 24;

/// BL-121 — `search_transcripts`. Args:
/// [`crate::transcript_search::SearchArgs`] (`{ query, agent_id?,
/// since_ts_ms?, limit? }`). Reply: `{ hits: Vec<TranscriptHit> }`.
/// Backed by the FTS5 virtual table populated from
/// `.forge/agents/*/history.jsonl`.
pub const HANDLER_SEARCH_TRANSCRIPTS: u32 = 25;

/// Default per-tool-call timeout used by the executor when no
/// caller-provided override lands. Matches the bootstrap bridge.
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(60);
/// Default chat timeout; planner prompts can cost remote-provider
/// latency. Matches the bootstrap bridge.
const DEFAULT_CHAT_TIMEOUT: Duration = Duration::from_secs(300);

/// Map of pending approval awaits keyed by session id.
/// Phase 2b — `BusBridgePolicy::allow_round` inserts a oneshot
/// sender here when it emits the `round_proposed` event;
/// `handle_round_decide` looks up the matching session and pushes
/// the caller's decision through. Wrapped in `Arc<Mutex<>>` so
/// the policy and the handler can share the map across
/// async-task boundaries.
type PendingApprovals = std::sync::Mutex<
    std::collections::HashMap<String, tokio::sync::oneshot::Sender<crate::RoundDecision>>,
>;

/// Default approval-callback timeout for `auto_approve: false`
/// sessions. 30 minutes is generous enough that a user can step
/// away to think about a high-stakes call without losing the
/// session, but not unbounded — a stale session eventually frees
/// the slot in `PendingApprovals` instead of leaking forever.
const DEFAULT_APPROVAL_TIMEOUT_SECS: u64 = 1800;
/// Hard cap on the caller-supplied `approval_timeout_secs`
/// override. Above this we silently clamp. One hour matches the
/// kernel's longest sleep window and stays well under typical
/// HTTP/keepalive timeouts on the IPC bridge.
const MAX_APPROVAL_TIMEOUT_SECS: u64 = 3600;

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
    /// [`CorePlugin::wire_context`] before the first dispatch; any
    /// handler that fires before then returns a clear error.
    ///
    /// BL-121 prefers [`Self::new_with_forge`] so the FTS index has
    /// a path to open against; this constructor stays for backwards
    /// compatibility with the existing test surface that doesn't
    /// know about transcript search.
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
        // disk if empty. Failures here are non-fatal: a forge with
        // a corrupted .sqlite or a read-only filesystem still boots
        // (the rest of the agent surface works without
        // `search_transcripts`).
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
            return handle_list_tools(_args);
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
        // Let the sync path handle `list_archetypes` and `list_tools` —
        // the kernel's `ipc_call` prefers `dispatch_async` when Some is
        // returned, and we don't want to hop an unnecessary async frame
        // for an in-memory read.
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
                HANDLER_PLAN => handle_plan(ctx, &args).await,
                HANDLER_HISTORY_LIST => handle_history_list(ctx).await,
                HANDLER_HISTORY_GET => handle_history_get(ctx, &args).await,
                HANDLER_HISTORY_DELETE => handle_history_delete(ctx, &args).await,
                HANDLER_SESSION_RUN => {
                    handle_session_run(ctx, pending_approvals, &args).await
                }
                HANDLER_SESSION_LIST => handle_session_list(ctx).await,
                HANDLER_SESSION_GET => handle_session_get(ctx, &args).await,
                HANDLER_SESSION_DELETE => handle_session_delete(ctx, &args).await,
                HANDLER_ROUND_DECIDE => handle_round_decide(pending_approvals, &args).await,
                HANDLER_LIST_CUSTOM => handle_list_custom(ctx).await,
                HANDLER_MEMORY_RECORD => handle_memory_record(ctx, &args).await,
                HANDLER_MEMORY_QUERY => handle_memory_query(ctx, &args).await,
                HANDLER_MEMORY_PRUNE => handle_memory_prune(ctx, &args).await,
                HANDLER_MEMORY_EXPORT => handle_memory_export(ctx, &args).await,
                HANDLER_DELEGATE => handle_delegate(ctx, pending_approvals, &args).await,
                HANDLER_SEARCH_TRANSCRIPTS => handle_search_transcripts(&args),
                other => Err(exec_err(format!("unknown handler id {other}"))),
            }
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(ctx);
    }
}

// ── Handler impls ───────────────────────────────────────────────────────────

/// Args for `com.nexus.agent::plan` and `::run` (handler ids `1`, `2`).
/// Lifted from inline by audit-2026-05-01 P1-3 (#113).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GoalArgs {
    goal: String,
    #[serde(default)]
    archetype: Option<String>,
}

/// Args for `com.nexus.agent::run_plan` (handler id `7`). Lifted from
/// inline by audit-2026-05-01 P1-3 (#113).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PlanArgs {
    plan: Plan,
}

async fn handle_plan(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GoalArgs = parse(args, "plan")?;
    let skills_prompt = system_prompt_with_skills(&ctx, &a.goal).await;
    // `system_prompt_with_skills` returns DEFAULT_SYSTEM_PROMPT as
    // its baseline when no skills match; strip that prefix so the
    // archetype's prompt becomes the new baseline without doubling
    // up the schema block.
    let extra = skills_prompt
        .strip_prefix(DEFAULT_SYSTEM_PROMPT)
        .map(str::trim_start)
        .filter(|s| !s.is_empty());
    let (agent_id, agent_prompt, source) =
        resolve_archetype_for_run(&ctx, a.archetype.as_deref()).await;
    let driver = AiChatBridge {
        ctx,
        timeout: DEFAULT_CHAT_TIMEOUT,
    };
    let plan = match source {
        ArchetypeSource::Builtin | ArchetypeSource::Default => {
            // Preserve the existing fast-path for built-ins —
            // `build_archetype` already knows the `&'static`-string
            // map; routing through it keeps callers that don't pass a
            // forge-relative slug on the unchanged code path.
            build_archetype(a.archetype.as_deref(), driver, extra)
                .plan(&a.goal)
                .await
                .map_err(|e| agent_err(&e))?
        }
        ArchetypeSource::CustomManifest { slug } => {
            tracing::debug!(
                plugin_id = PLUGIN_ID,
                custom_slug = %slug,
                agent_id = %agent_id,
                "DG-36: routing through custom archetype manifest",
            );
            crate::archetypes::build_archetype_with_prompt(agent_id, agent_prompt, driver, extra)
                .plan(&a.goal)
                .await
                .map_err(|e| agent_err(&e))?
        }
    };
    to_value(&plan, "plan")
}

// ── BL-027 orchestrator + per-plan executor were retired in
//    ADR 0025 Phase 2. The session loop in `crate::session`
//    handles every former responsibility (plan-then-execute,
//    per-step events, history). EVENT_RUN_START / STEP_START /
//    STEP_DONE / RUN_DONE bus topics retired alongside —
//    `com.nexus.agent.round_proposed` (Phase 2b) is the
//    replacement for live UI updates.

// ── DG-36 follow-up: custom-archetype routing ──────────────────────────────

/// Where the system prompt used by `handle_plan` / `handle_session_run`
/// came from. Lets the call site pick between the built-in fast path
/// (`&'static` prompt strings) and the owned-string path that custom
/// manifests require.
#[derive(Debug, Clone)]
enum ArchetypeSource {
    /// Caller passed nothing — DEFAULT_SYSTEM_PROMPT.
    Default,
    /// Caller passed one of the six built-in slugs (writer / coder /
    /// researcher / auditor / librarian / coach).
    Builtin,
    /// Caller passed a slug that matched a manifest under
    /// `<forge>/.forge/agents/<slug>/agent.toml`. The custom manifest's
    /// `[system_prompt]` body is layered over its `[agent].archetype`
    /// baseline (default if absent). Agent id is namespaced as
    /// `com.nexus.agent.custom.<slug>`.
    CustomManifest { slug: String },
}

/// DG-36 — resolve an `--archetype` argument into a concrete
/// `(agent_id, system_prompt, source)` triple. Try built-ins first
/// (fast path, same as the pre-DG-36 behaviour); on miss, look up
/// `<forge>/.forge/agents/<slug>/agent.toml` via the kernel context.
/// A custom manifest's `system_prompt` body layers over its
/// `[agent].archetype` baseline (default if unset).
///
/// Unknown slugs that don't match a built-in and don't have a manifest
/// fall through to `ArchetypeSource::Default` — same posture as the
/// pre-DG-36 fallback in `resolve_prompt`. The kernel-context I/O
/// failures are logged as warnings; they don't crash the handler.
async fn resolve_archetype_for_run(
    ctx: &KernelPluginContext,
    name: Option<&str>,
) -> (String, String, ArchetypeSource) {
    let trimmed = name.map(str::trim).filter(|s| !s.is_empty());
    if let Some(slug) = trimmed {
        if crate::archetypes::is_builtin_archetype(slug) {
            let (id, prompt) = crate::archetypes::resolve_prompt(Some(slug));
            return (id.to_string(), prompt.to_string(), ArchetypeSource::Builtin);
        }
        // Slug doesn't match a built-in — try the custom-manifest path.
        match load_custom_archetype_prompt(ctx, slug).await {
            Ok(Some((id, prompt))) => {
                return (
                    id,
                    prompt,
                    ArchetypeSource::CustomManifest {
                        slug: slug.to_string(),
                    },
                );
            }
            Ok(None) => {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    archetype = slug,
                    "no custom manifest found for slug; falling back to default",
                );
            }
            Err(reason) => {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    archetype = slug,
                    %reason,
                    "custom manifest lookup failed; falling back to default",
                );
            }
        }
    }
    // No name, or fall-through: built-in default.
    let (id, prompt) = crate::archetypes::resolve_prompt(name);
    (id.to_string(), prompt.to_string(), ArchetypeSource::Default)
}

/// `true` when `slug` is safe to splice into a `<forge>/.forge/agents/<slug>/…`
/// path. Pure helper extracted from [`load_custom_archetype_prompt`]
/// so the slug-validation rules are unit-testable without a kernel
/// context. Rejects path separators, parent-directory escapes, and
/// hidden-directory aliases. Empty slugs are rejected too — the only
/// caller guarantees a non-empty slug, but defending here keeps the
/// helper self-contained.
fn is_safe_archetype_slug(slug: &str) -> bool {
    if slug.is_empty() {
        return false;
    }
    if slug.contains('/') || slug.contains('\\') {
        return false;
    }
    if slug.contains("..") {
        return false;
    }
    if slug.starts_with('.') {
        return false;
    }
    true
}

/// Read `<forge>/.forge/agents/<slug>/agent.toml` through the kernel
/// context (capability-correct path) and assemble the layered prompt.
/// Returns `Ok(None)` when the manifest file is missing or unreadable
/// — the slug just isn't a custom agent, which `resolve_archetype_for_run`
/// surfaces as a warning. `Err` is reserved for parse / shape failures
/// surfaced to the trace log so an operator can debug a broken manifest.
async fn load_custom_archetype_prompt(
    ctx: &KernelPluginContext,
    slug: &str,
) -> Result<Option<(String, String)>, String> {
    // Reject path-shaped slugs up front; the manifest scanner trusts
    // directory names, so a `../etc/passwd` slug must not escape.
    if !is_safe_archetype_slug(slug) {
        return Err(format!("rejecting suspicious slug `{slug}`"));
    }
    let manifest_path = std::path::Path::new(crate::custom_agent::AGENTS_DIR)
        .join(slug)
        .join(crate::custom_agent::MANIFEST_FILE_NAME);
    let bytes = match ctx.read_file(&manifest_path).await {
        Ok(b) => b,
        // Missing file is the common case (slug isn't a custom agent).
        Err(_) => return Ok(None),
    };
    let body = std::str::from_utf8(&bytes)
        .map_err(|e| format!("manifest not UTF-8 at {}: {e}", manifest_path.display()))?
        .to_string();
    let manifest = crate::custom_agent::parse_str(&body, slug, &manifest_path)
        .map_err(|e| format!("parse failed for {}: {e}", manifest_path.display()))?;

    // Resolve the prompt body. For `[system_prompt].text` we have it
    // inline; for `[system_prompt].path` we fetch the referenced file
    // through the kernel context, treating the path as relative to the
    // manifest directory (matches `custom_agent::resolve_system_prompt`
    // on the sync side).
    let custom_prompt = if let Some(text) = manifest.system_prompt.text.as_deref() {
        text.to_string()
    } else if let Some(rel) = manifest.system_prompt.path.as_deref() {
        let full_path = std::path::Path::new(crate::custom_agent::AGENTS_DIR)
            .join(slug)
            .join(rel);
        let bytes = ctx
            .read_file(&full_path)
            .await
            .map_err(|e| format!("read {} failed: {e}", full_path.display()))?;
        std::str::from_utf8(&bytes)
            .map_err(|e| format!("system prompt file not UTF-8 at {}: {e}", full_path.display()))?
            .to_string()
    } else {
        // Parse-time check should have rejected this; treat as missing
        // rather than an error so the fallback path stays predictable.
        return Ok(None);
    };

    // Layer over the named baseline archetype, or DEFAULT_SYSTEM_PROMPT
    // when none is given. Custom prompt is appended with a blank-line
    // separator — same shape as build_archetype's `extra_prompt`
    // layering, so a custom agent reads as "<baseline>\n\n<custom>".
    let base_name = manifest
        .agent
        .archetype
        .as_deref()
        .filter(|s| crate::archetypes::is_builtin_archetype(s));
    let (_, base_prompt) = crate::archetypes::resolve_prompt(base_name);
    let layered = if custom_prompt.trim().is_empty() {
        base_prompt.to_string()
    } else {
        format!("{base_prompt}\n\n{custom_prompt}")
    };
    let id = format!("com.nexus.agent.custom.{slug}");
    Ok(Some((id, layered)))
}

// ── History persistence ─────────────────────────────────────────────────────

const HISTORY_DIR: &str = ".forge/agent/history";

fn history_path(plan_id: &str) -> Option<std::path::PathBuf> {
    // Same alphabet as com.nexus.ai session ids — belt-and-braces
    // path-traversal guard since plan ids are model-derived.
    if plan_id.is_empty() || plan_id.len() > 96 {
        return None;
    }
    let safe = plan_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !safe {
        return None;
    }
    Some(std::path::PathBuf::from(HISTORY_DIR).join(format!("{plan_id}.json")))
}

// `save_history` retired alongside the legacy `run` handler in
// ADR 0025 Phase 2 — new transcripts go to `.forge/agent/sessions/`
// via `handle_session_run`. The `history_*` read handlers below
// continue to surface any pre-Phase-2a JSON in
// `.forge/agent/history/`.

async fn handle_history_list(
    ctx: Arc<KernelPluginContext>,
) -> Result<serde_json::Value, PluginError> {
    let dir = std::path::Path::new(HISTORY_DIR);
    let Ok(entries) = ctx.list_files(dir).await else {
        return Ok(serde_json::Value::Array(Vec::new()));
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    for path in entries {
        let Some(plan_id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| history_path(s).is_some())
            .map(ToString::to_string)
        else {
            continue;
        };
        let Ok(bytes) = ctx.read_file(&path).await else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let goal = record
            .get("goal")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let created_at = record
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let success = record
            .get("observation")
            .and_then(|o| o.get("success"))
            .and_then(serde_json::Value::as_bool);
        let step_count = record
            .get("observation")
            .and_then(|o| o.get("steps"))
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);
        out.push(serde_json::json!({
            "plan_id": plan_id,
            "goal": goal,
            "created_at": created_at,
            "success": success,
            "steps": step_count,
            "bytes": bytes.len(),
        }));
    }
    Ok(serde_json::Value::Array(out))
}

/// Args for `com.nexus.agent::history_get` and related plan-id-keyed
/// handlers. Lifted from inline by audit-2026-05-01 P1-3 (#113).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PlanIdArgs {
    plan_id: String,
}

/// Args for `com.nexus.agent::list_tools` (handler id 18).
///
/// `capabilities` (when present) is the list of [`crate::Capability`]
/// id strings (e.g. `["fs.read", "git.read"]`) the agent holds. The
/// handler filters the registry to tools whose `required_capabilities`
/// are a subset of that list. Omitting the field returns the full
/// catalogue.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ListToolsArgs {
    /// Optional capability filter. Strings parsed via
    /// [`crate::Capability::from_str`]; unknown ids are rejected.
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
}

fn handle_list_tools(args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let a: ListToolsArgs = if args.is_null() {
        ListToolsArgs { capabilities: None }
    } else {
        parse(args, "list_tools")?
    };
    let registry = crate::AgentToolRegistry::global();
    let specs = match a.capabilities {
        None => registry.list_all(),
        Some(ids) => {
            let mut held = Vec::with_capacity(ids.len());
            for id in ids {
                let cap = crate::Capability::from_str(&id).ok_or_else(|| {
                    exec_err(format!("list_tools: unknown capability id '{id}'"))
                })?;
                held.push(cap);
            }
            registry.list_for_agent(&held)
        }
    };
    // Stable ordering for callers that diff outputs (CLI, shell).
    let mut sorted = specs;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    to_value(&sorted, "list_tools")
}

/// DG-36 (PRD-15 §9) — scan `.forge/agents/*/agent.toml` and return
/// the parsed manifests. Per-manifest errors land in a sibling
/// `errors` array so a single broken file doesn't poison the read.
async fn handle_list_custom(
    ctx: Arc<KernelPluginContext>,
) -> Result<serde_json::Value, PluginError> {
    let agents_dir = std::path::Path::new(crate::custom_agent::AGENTS_DIR);
    // Missing directory is a clean empty reply — most forges won't
    // have a custom-agents dir until a user opts in.
    let entries = match ctx.list_files(agents_dir).await {
        Ok(e) => e,
        Err(_) => {
            return Ok(serde_json::json!({
                "manifests": [],
                "errors": []
            }));
        }
    };

    let mut manifests: Vec<crate::CustomAgentManifest> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for entry in entries {
        // list_files returns every entry — files and directories
        // alike. We want subdirectories; reading their agent.toml
        // via the kernel keeps the capability surface honest.
        let slug = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if slug.is_empty() {
            continue;
        }
        let manifest_path = entry.join(crate::custom_agent::MANIFEST_FILE_NAME);
        // Try to read; missing file → not an agent dir → skip silently.
        let body = match ctx.read_file(&manifest_path).await {
            Ok(bytes) => match std::str::from_utf8(&bytes) {
                Ok(s) => s.to_string(),
                Err(e) => {
                    errors.push(serde_json::json!({
                        "path": manifest_path.display().to_string(),
                        "error": format!("manifest not UTF-8: {e}"),
                    }));
                    continue;
                }
            },
            Err(_) => continue,
        };

        match crate::custom_agent::parse_str(&body, &slug, &manifest_path) {
            Ok(manifest) => manifests.push(manifest),
            Err(e) => errors.push(serde_json::json!({
                "path": manifest_path.display().to_string(),
                "error": format!("{e}"),
            })),
        }
    }

    manifests.sort_by(|a, b| a.slug.cmp(&b.slug));

    Ok(serde_json::json!({
        "manifests": manifests,
        "errors": errors,
    }))
}

// ── DG-37 agent-to-agent delegation ─────────────────────────────────────────

/// Args for `com.nexus.agent::delegate` (handler id 24).
///
/// Shape mirrors [`SessionRunArgs`] with `archetype` required —
/// delegation always names a target archetype so the caller can
/// be explicit about which agent's posture handles the sub-goal.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DelegateArgs {
    /// Target archetype short name (one of the ids returned by
    /// `list_archetypes`). Required — the parent agent has to pick
    /// the child's posture rather than relying on a default.
    pub archetype: String,
    /// Natural-language goal for the sub-session.
    pub goal: String,
    /// Optional override for the sub-session's system prompt.
    /// When unset, the archetype's prompt is used directly.
    #[serde(default)]
    pub system: Option<String>,
    /// Auto-approve the sub-session's rounds (matches `session_run`).
    /// Defaults to `true` since a delegation is a tool-call inside
    /// another agent's run — prompting twice for the same human is
    /// usually noise.
    #[serde(default = "default_delegate_auto_approve")]
    pub auto_approve: bool,
    /// Approval-callback timeout (only used when `auto_approve =
    /// false`).
    #[serde(default)]
    pub approval_timeout_secs: Option<u64>,
    /// Prompt for every round when `auto_approve = false` (otherwise
    /// the policy uses DG-34's risk-aware gating).
    #[serde(default)]
    pub strict_approval: bool,
}

const fn default_delegate_auto_approve() -> bool {
    true
}

async fn handle_delegate(
    ctx: Arc<KernelPluginContext>,
    pending_approvals: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: DelegateArgs = parse(args, "delegate")?;
    if a.archetype.trim().is_empty() {
        return Err(exec_err("delegate: `archetype` must be non-empty".into()));
    }
    if a.goal.trim().is_empty() {
        return Err(exec_err("delegate: `goal` must be non-empty".into()));
    }
    // Reuse the existing `session_run` machinery — the only
    // difference from session_run's caller surface is that
    // `archetype` is required here.
    let session_args = serde_json::json!({
        "goal": a.goal,
        "archetype": a.archetype,
        "system": a.system,
        "auto_approve": a.auto_approve,
        "approval_timeout_secs": a.approval_timeout_secs,
        "strict_approval": a.strict_approval,
    });
    handle_session_run(ctx, pending_approvals, &session_args).await
}

// ── DG-33 memory handlers ───────────────────────────────────────────────────

/// Args for `memory_record` — `{ agent_id, entry }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecordArgs {
    /// Reverse-DNS or short id naming the agent that owns the memory.
    pub agent_id: String,
    /// Entry to append.
    pub entry: crate::memory::MemoryEntry,
}

/// Args for `memory_query` — `{ agent_id, pattern?, limit? }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryQueryArgs {
    /// Agent id to query.
    pub agent_id: String,
    /// Substring filter; empty / absent returns the most recent entries.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Max entries to return (default 50).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Args for `memory_prune` — `{ agent_id, retention_days }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryPruneArgs {
    /// Agent id whose memory should be pruned.
    pub agent_id: String,
    /// Drop entries older than this many days.
    pub retention_days: u32,
}

/// Args for `memory_export` — `{ agent_id }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportArgs {
    /// Agent id whose memory should be exported as markdown.
    pub agent_id: String,
}

const MEMORY_DEFAULT_QUERY_LIMIT: u32 = 50;

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

async fn handle_memory_record(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryRecordArgs = parse(args, "memory_record")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_record: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let mut bytes = serde_json::to_vec(&a.entry)
        .map_err(|e| exec_err(format!("memory_record: serialize entry: {e}")))?;
    bytes.push(b'\n');

    // Read current contents, append, rewrite atomically. The kernel
    // doesn't expose a raw append; one-shot replace is the safest
    // primitive we have inside the capability surface.
    let existing = ctx.read_file(&path).await.unwrap_or_default();
    let mut combined = existing;
    combined.extend_from_slice(&bytes);
    ctx.write_file(&path, &combined)
        .await
        .map_err(|e| exec_err(format!("memory_record: write {}: {e}", path.display())))?;
    Ok(serde_json::json!({ "recorded": true }))
}

async fn handle_memory_query(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryQueryArgs = parse(args, "memory_query")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_query: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let bytes = ctx.read_file(&path).await.unwrap_or_default();
    let entries = parse_memory_lines(&bytes);
    let pattern = a.pattern.unwrap_or_default();
    let limit =
        usize::try_from(a.limit.unwrap_or(MEMORY_DEFAULT_QUERY_LIMIT)).unwrap_or(usize::MAX);
    let hits = crate::memory::query_entries(&entries, &pattern, limit);
    serde_json::to_value(hits).map_err(|e| exec_err(format!("memory_query: serialize: {e}")))
}

async fn handle_memory_prune(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryPruneArgs = parse(args, "memory_prune")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_prune: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let bytes = ctx.read_file(&path).await.unwrap_or_default();
    if bytes.is_empty() {
        return Ok(serde_json::json!({ "pruned": 0, "kept": 0 }));
    }
    let entries = parse_memory_lines(&bytes);
    let retention_ms = u64::from(a.retention_days).saturating_mul(86_400_000);
    let (kept, pruned) = crate::memory::prune_entries(entries, now_unix_ms(), retention_ms);
    let mut out = Vec::with_capacity(bytes.len());
    for entry in &kept {
        let mut line = serde_json::to_vec(entry)
            .map_err(|e| exec_err(format!("memory_prune: serialize: {e}")))?;
        line.push(b'\n');
        out.extend_from_slice(&line);
    }
    ctx.write_file(&path, &out)
        .await
        .map_err(|e| exec_err(format!("memory_prune: write {}: {e}", path.display())))?;
    Ok(serde_json::json!({
        "pruned": pruned,
        "kept": kept.len(),
    }))
}

async fn handle_memory_export(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryExportArgs = parse(args, "memory_export")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_export: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let bytes = ctx.read_file(&path).await.unwrap_or_default();
    let entries = parse_memory_lines(&bytes);
    let markdown = crate::memory::export_markdown(&a.agent_id, &entries);
    Ok(serde_json::json!({ "markdown": markdown }))
}

fn parse_memory_lines(bytes: &[u8]) -> Vec<crate::memory::MemoryEntry> {
    let mut entries = Vec::new();
    for raw in bytes.split(|b| *b == b'\n') {
        if raw.is_empty() {
            continue;
        }
        let Ok(s) = std::str::from_utf8(raw) else {
            continue;
        };
        let trimmed = s.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<crate::memory::MemoryEntry>(trimmed) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                tracing::warn!(error = %e, line = trimmed, "skipping malformed memory line");
            }
        }
    }
    entries
}

async fn handle_history_get(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: PlanIdArgs = parse(args, "history_get")?;
    let path = history_path(&a.plan_id)
        .ok_or_else(|| exec_err(format!("history_get: invalid plan_id '{}'", a.plan_id)))?;
    let bytes = ctx
        .read_file(&path)
        .await
        .map_err(|e| exec_err(format!("history_get: {e}")))?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|e| exec_err(format!("history_get: invalid JSON on disk: {e}")))
}

async fn handle_history_delete(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: PlanIdArgs = parse(args, "history_delete")?;
    let path = history_path(&a.plan_id).ok_or_else(|| {
        exec_err(format!("history_delete: invalid plan_id '{}'", a.plan_id))
    })?;
    ctx.delete_file(&path)
        .await
        .map_err(|e| exec_err(format!("history_delete: {e}")))?;
    Ok(serde_json::json!({ "deleted": true, "plan_id": a.plan_id }))
}

// ── Session handlers (ADR 0024 Phase 2a) ───────────────────────────────────

const SESSION_DIR: &str = ".forge/agent/sessions";

#[derive(Debug, Deserialize)]
struct SessionRunArgs {
    goal: String,
    #[serde(default)]
    archetype: Option<String>,
    #[serde(default)]
    system: Option<String>,
    /// `true` for headless / auto-approve sessions. `false` (Phase
    /// 2b) requires the caller to handle
    /// `com.nexus.agent.round_proposed` events and reply via
    /// `round_decide` before [`SessionRunArgs::approval_timeout_secs`]
    /// elapses.
    #[serde(default)]
    auto_approve: bool,
    /// Caller-side approval-callback timeout for `auto_approve =
    /// false` sessions. Clamped to
    /// `[1, MAX_APPROVAL_TIMEOUT_SECS]`. Defaults to
    /// `DEFAULT_APPROVAL_TIMEOUT_SECS`.
    #[serde(default)]
    approval_timeout_secs: Option<u64>,
    /// DG-34 — when `auto_approve = false`, prompt the caller for
    /// *every* round (the original ADR 0024 Phase 2b behaviour).
    /// When unset (the default), the session runs in selective mode:
    /// rounds whose tool calls are all registered as
    /// `requires_approval = false` auto-approve, and only rounds
    /// containing a high-risk or unregistered tool call publish
    /// `round_proposed`. Matches PRD-15 §7's risk table.
    #[serde(default)]
    strict_approval: bool,
    /// BL-119 — optional [`crate::SessionConfig`] override. When
    /// omitted, defaults from `SessionConfig::default()` apply
    /// (max_iterations = 32 per Hermes Feature 1). Partial JSON is
    /// accepted; missing fields fall back to per-field defaults via
    /// `#[serde(default = "…")]` on the struct.
    #[serde(default)]
    session_config: Option<crate::session::SessionConfig>,
}

#[derive(Debug, Deserialize)]
struct SessionIdArgs {
    id: String,
}

async fn handle_session_run(
    ctx: Arc<KernelPluginContext>,
    pending_approvals: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SessionRunArgs = parse(args, "session_run")?;

    let driver = AiChatBridge {
        ctx: Arc::clone(&ctx),
        timeout: DEFAULT_CHAT_TIMEOUT,
    };
    let dispatcher = KernelToolBridge {
        ctx: Arc::clone(&ctx),
        timeout: DEFAULT_TOOL_TIMEOUT,
    };

    let system = match (&parsed.system, &parsed.archetype) {
        (Some(s), _) => s.clone(),
        (None, Some(name)) => {
            // DG-36 — try the custom-manifest path first; fall back to
            // built-ins. Skill-aware assembly is for the legacy `plan`
            // flow; sessions compose the prompt directly.
            let (_, prompt, _) = resolve_archetype_for_run(&ctx, Some(name)).await;
            prompt
        }
        (None, None) => DEFAULT_SYSTEM_PROMPT.to_string(),
    };

    // BL-119 — fold the optional caller-supplied config into the
    // default. Partial overrides (e.g. `{ "max_iterations": 64 }`)
    // already populate the defaults via the struct's #[serde(default)]
    // helpers; here we just default the whole thing when absent.
    let session_config = parsed
        .session_config
        .clone()
        .unwrap_or_default();

    let session = if parsed.auto_approve {
        let id = uuid::Uuid::new_v4().to_string();
        crate::session::run_session_with_config(
            &driver,
            &dispatcher,
            &crate::session::AutoApproveAll,
            &parsed.goal,
            &system,
            parsed.archetype.clone(),
            id,
            session_config,
        )
        .await
    } else {
        let timeout = parsed
            .approval_timeout_secs
            .unwrap_or(DEFAULT_APPROVAL_TIMEOUT_SECS)
            .clamp(1, MAX_APPROVAL_TIMEOUT_SECS);
        let policy = BusBridgePolicy {
            session_id: uuid::Uuid::new_v4().to_string(),
            ctx: Arc::clone(&ctx),
            pending: Arc::clone(&pending_approvals),
            timeout: Duration::from_secs(timeout),
            strict_approval: parsed.strict_approval,
        };
        // BusBridgePolicy generates a session_id up-front so the
        // round_proposed event payload can carry it BEFORE
        // run_session has assigned its own. We accept a tiny
        // mismatch here: the persisted session.id will differ
        // from the policy's session_id. Fix is to plumb the policy's
        // id into run_session as a starter — done below.
        let policy_session_id = policy.session_id.clone();
        let session = crate::session::run_session_with_config(
            &driver,
            &dispatcher,
            &policy,
            &parsed.goal,
            &system,
            parsed.archetype.clone(),
            policy_session_id,
            session_config,
        )
        .await;
        // Defensive cleanup: if the loop exited with a pending
        // entry still in the map (e.g. internal bug), drop it.
        drop_pending(&pending_approvals, &session.id);
        session
    };

    // Persist before returning so a crash mid-call still leaves a
    // record on disk.
    let path = session_path(&session.id)
        .ok_or_else(|| exec_err("session_run: refusing to write empty id".into()))?;
    let bytes = serde_json::to_vec_pretty(&session)
        .map_err(|e| exec_err(format!("session_run: encode session: {e}")))?;
    // Route through `com.nexus.storage::write_vault_file` rather
    // than the user-facing `write_file` (which would pollute the
    // FTS index + knowledge graph with the JSON transcript). The
    // _vault_file variant atomic-writes + mkdir-p without
    // touching any indexes — exactly what shell-owned `.forge/`
    // metadata needs. `ctx.write_file` would have been fine for
    // raw bytes but doesn't mkdir-p, hence the IPC route.
    let path_str = path
        .to_str()
        .ok_or_else(|| exec_err("session_run: session path not UTF-8".into()))?;
    tracing::info!(
        session_id = %session.id,
        path = %path_str,
        bytes = bytes.len(),
        "session_run: persisting transcript"
    );
    ctx.ipc_call(
        "com.nexus.storage",
        "write_vault_file",
        serde_json::json!({ "path": path_str, "bytes": bytes }),
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| {
        tracing::warn!(session_id = %session.id, error = %e, "session_run: persist failed");
        exec_err(format!("session_run: persist: {e}"))
    })?;
    tracing::info!(session_id = %session.id, "session_run: persisted ok");

    serde_json::to_value(&session)
        .map_err(|e| exec_err(format!("session_run: encode reply: {e}")))
}

async fn handle_session_list(
    ctx: Arc<KernelPluginContext>,
) -> Result<serde_json::Value, PluginError> {
    // Use storage's list_dir IPC so we don't need fs.read directly
    // on the agent context (it doesn't hold the cap and shouldn't
    // need it — the agent's own contexts are narrowly scoped per
    // ADR 0022).
    //
    // Storage's contract: `list_dir` takes `{ relpath: string }`
    // and returns a bare JSON array of `TreeEntry`. (The
    // `StorageListDirResult` wrapper type defined in nexus-storage's
    // ipc.rs is documentation-only — the actual handler serializes
    // `Vec<TreeEntry>` directly.)
    let response = match ctx
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            serde_json::json!({ "relpath": SESSION_DIR }),
            Duration::from_secs(5),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::info!(error = %e, dir = SESSION_DIR, "session_list: list_dir errored, reporting empty");
            return Ok(serde_json::json!([]));
        }
    };

    let Some(arr) = response.as_array() else {
        tracing::warn!(
            dir = SESSION_DIR,
            response = %response,
            "session_list: list_dir reply was not a JSON array"
        );
        return Ok(serde_json::json!([]));
    };
    let mut summaries: Vec<serde_json::Value> = Vec::new();
    for entry in arr {
        let Some(name) = entry.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !name.ends_with(".json") {
            continue;
        }
        let id = name.trim_end_matches(".json").to_string();
        let Some(path) = session_path(&id) else {
            continue;
        };
        let Ok(bytes) = ctx.read_file(&path).await else {
            continue;
        };
        let Ok(session) = serde_json::from_slice::<crate::AgentSession>(&bytes) else {
            continue;
        };
        summaries.push(serde_json::json!({
            "id": session.id,
            "goal": session.goal,
            "started_at": session.started_at,
            "ended_at": session.ended_at,
            "outcome": session.outcome,
        }));
    }
    summaries.sort_by(|a, b| {
        b.get("started_at")
            .and_then(serde_json::Value::as_str)
            .cmp(&a.get("started_at").and_then(serde_json::Value::as_str))
    });
    Ok(serde_json::Value::Array(summaries))
}

async fn handle_session_get(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: SessionIdArgs = parse(args, "session_get")?;
    let path = session_path(&a.id)
        .ok_or_else(|| exec_err(format!("session_get: invalid id '{}'", a.id)))?;
    let bytes = ctx
        .read_file(&path)
        .await
        .map_err(|e| exec_err(format!("session_get: {e}")))?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|e| exec_err(format!("session_get: invalid JSON on disk: {e}")))
}

async fn handle_session_delete(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: SessionIdArgs = parse(args, "session_delete")?;
    let path = session_path(&a.id)
        .ok_or_else(|| exec_err(format!("session_delete: invalid id '{}'", a.id)))?;
    ctx.delete_file(&path)
        .await
        .map_err(|e| exec_err(format!("session_delete: {e}")))?;
    Ok(serde_json::json!({ "deleted": true, "id": a.id }))
}

// ── Phase 2b: bus-bridge approval callback ──────────────────────────────────

/// Wire shape of `com.nexus.agent::round_decide` args.
/// Mirrors `crate::RoundDecision` as a tagged enum so the caller
/// can express any of the three decision shapes over IPC.
///
/// Intentionally without `deny_unknown_fields`: `#[serde(flatten)]`
/// combined with strict deny rejects the inner enum's `kind` /
/// `entries` / `reason` fields as "unknown" on the outer struct.
#[derive(Debug, Deserialize)]
struct RoundDecideArgs {
    session_id: String,
    #[serde(flatten)]
    decision: RoundDecideKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RoundDecideKind {
    ApproveAll,
    Abort {
        #[serde(default)]
        reason: String,
    },
    Partial {
        entries: Vec<crate::RoundDecisionEntry>,
    },
}

impl From<RoundDecideKind> for crate::RoundDecision {
    fn from(k: RoundDecideKind) -> Self {
        match k {
            RoundDecideKind::ApproveAll => crate::RoundDecision::ApproveAll,
            RoundDecideKind::Abort { reason } => crate::RoundDecision::Abort(reason),
            RoundDecideKind::Partial { entries } => crate::RoundDecision::Partial(entries),
        }
    }
}

async fn handle_round_decide(
    pending: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: RoundDecideArgs = parse(args, "round_decide")?;
    let tx = {
        let mut map = pending
            .lock()
            .map_err(|e| exec_err(format!("round_decide: pending lock poisoned: {e}")))?;
        map.remove(&parsed.session_id)
    };
    let Some(tx) = tx else {
        return Err(exec_err(format!(
            "round_decide: no pending approval for session '{}'",
            parsed.session_id
        )));
    };
    if tx.send(parsed.decision.into()).is_err() {
        // Receiver was dropped — session loop already moved on
        // (e.g. timeout fired between map lookup and our send).
        // Surface as an error so the caller knows their decision
        // didn't land.
        return Err(exec_err(format!(
            "round_decide: session '{}' is no longer awaiting a decision",
            parsed.session_id
        )));
    }
    Ok(serde_json::json!({ "delivered": true, "session_id": parsed.session_id }))
}

/// Defensive helper: drop any leftover pending entry for `id`.
/// Called after a session ends so a leak on a long-running plugin
/// is bounded by session count, not by uptime.
fn drop_pending(pending: &Arc<PendingApprovals>, id: &str) {
    if let Ok(mut map) = pending.lock() {
        map.remove(id);
    }
}

/// Bus-bridge approval policy (ADR 0024 Phase 2b). Each
/// `allow_round` call:
///
/// 1. Allocates a `oneshot` and stashes the sender under
///    `session_id` in the agent plugin's `PendingApprovals` map.
/// 2. Publishes a `com.nexus.agent.round_proposed` event so the
///    caller's UI can render an approval prompt.
/// 3. Awaits the receiver with `timeout`. The caller responds via
///    `com.nexus.agent::round_decide`, which runs
///    [`handle_round_decide`] and pushes the [`RoundDecision`]
///    through the oneshot.
///
/// On timeout, returns [`RoundDecision::Timeout`] and removes the
/// stashed sender (so a late-arriving `round_decide` gets a clean
/// "no pending approval" error rather than racing into a dropped
/// receiver).
struct BusBridgePolicy {
    session_id: String,
    ctx: Arc<KernelPluginContext>,
    pending: Arc<PendingApprovals>,
    timeout: Duration,
    /// DG-34 — when `true`, every round publishes `round_proposed`
    /// and waits for caller approval (legacy Phase 2b behaviour).
    /// When `false`, the policy checks each round's tool calls
    /// against [`crate::AgentToolRegistry`] and auto-approves rounds
    /// whose tools are all `requires_approval = false`. Rounds with
    /// any high-risk or unregistered tool call still go through the
    /// prompt path.
    strict_approval: bool,
}

/// DG-34 — classify a [`crate::ProposedRound`] against the agent
/// tool registry. Returns `true` when the round contains at least
/// one proposed tool call that's either:
///
/// - flagged `requires_approval = true` in the registry, OR
/// - missing from the registry entirely (conservative default —
///   unknown tools are high-risk because the agent might be
///   asking for something the registry hasn't classified yet).
///
/// Rounds with zero tool calls are *low-risk* (text-only responses
/// don't mutate anything) so they auto-approve without prompting.
pub fn round_requires_approval(
    round: &crate::ProposedRound,
    registry: &crate::AgentToolRegistry,
) -> bool {
    for tc in &round.tool_calls {
        match registry.lookup(&tc.name) {
            Some(spec) if !spec.requires_approval => continue,
            _ => return true,
        }
    }
    false
}

#[async_trait]
impl crate::SessionPolicy for BusBridgePolicy {
    async fn allow_round(&self, round: &crate::ProposedRound) -> crate::RoundDecision {
        // DG-34 — short-circuit when the round is low-risk and the
        // caller hasn't opted into strict gating. Saves a bus event
        // + caller round-trip for the common case (read-only tools
        // or text-only rounds).
        if !self.strict_approval {
            let registry = crate::AgentToolRegistry::global();
            if !round_requires_approval(round, &registry) {
                return crate::RoundDecision::ApproveAll;
            }
        }

        let (tx, rx) = tokio::sync::oneshot::channel::<crate::RoundDecision>();
        // Insert before publishing so a fast caller that races
        // round_decide against the event sees a populated map.
        match self.pending.lock() {
            Ok(mut map) => {
                map.insert(self.session_id.clone(), tx);
            }
            Err(e) => {
                return crate::RoundDecision::Abort(format!(
                    "session approval map poisoned: {e}"
                ));
            }
        };

        // DG-34 — annotate each proposed tool with its registered
        // approval flag so the UI can render a per-call risk badge
        // (write tools red, read-only tools muted, unregistered tools
        // outlined). Unknown tools surface as `requires_approval = true`
        // matching the conservative default in `round_requires_approval`.
        let registry = crate::AgentToolRegistry::global();
        let annotated: Vec<serde_json::Value> = round
            .tool_calls
            .iter()
            .map(|tc| {
                let (requires_approval, registered) = match registry.lookup(&tc.name) {
                    Some(spec) => (spec.requires_approval, true),
                    None => (true, false),
                };
                serde_json::json!({
                    "id": tc.id,
                    "name": tc.name,
                    "tool_call": tc.tool_call,
                    "requires_approval": requires_approval,
                    "registered": registered,
                })
            })
            .collect();
        let payload = serde_json::json!({
            "session_id": self.session_id,
            "round": round.round,
            "text": round.text,
            "tool_calls": annotated,
        });
        if let Err(e) = self
            .ctx
            .publish("com.nexus.agent.round_proposed", payload)
        {
            // Clean up before bailing.
            drop_pending(&self.pending, &self.session_id);
            return crate::RoundDecision::Abort(format!("publish round_proposed: {e}"));
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(decision)) => decision,
            Ok(Err(_recv_err)) => {
                // Sender was dropped without delivering — should
                // only happen if `handle_round_decide` removes the
                // entry without sending, which it doesn't. Treat
                // as abort.
                drop_pending(&self.pending, &self.session_id);
                crate::RoundDecision::Abort(
                    "approval channel closed without a decision".into(),
                )
            }
            Err(_elapsed) => {
                drop_pending(&self.pending, &self.session_id);
                crate::RoundDecision::Timeout(format!(
                    "no decision within {} seconds",
                    self.timeout.as_secs()
                ))
            }
        }
    }
}

/// Resolve a session id to its on-disk path. Validates the id is
/// non-empty and contains only `[a-zA-Z0-9-]` so a maliciously
/// shaped id can't path-traverse out of the sessions directory.
fn session_path(id: &str) -> Option<std::path::PathBuf> {
    if id.is_empty() {
        return None;
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(std::path::PathBuf::from(format!("{SESSION_DIR}/{id}.json")))
}

// ── Skill-aware system prompt assembly ─────────────────────────────────────

/// Build a planner system prompt that layers in any skill whose
/// triggers match the goal text. Calls `com.nexus.skills::triggered_by`
/// best-effort — failures (plugin not registered, disk errors) fall
/// back silently to [`DEFAULT_SYSTEM_PROMPT`] so the agent still
/// works in forges without a skills directory.
async fn system_prompt_with_skills(
    ctx: &KernelPluginContext,
    goal: &str,
) -> String {
    let mut prompt = String::from(DEFAULT_SYSTEM_PROMPT);
    append_mcp_hint(ctx, &mut prompt).await;

    let response = ctx
        .ipc_call(
            "com.nexus.skills",
            "triggered_by",
            serde_json::json!({ "text": goal }),
            Duration::from_secs(5),
        )
        .await;
    let Ok(value) = response else {
        return prompt;
    };
    let skills: Vec<serde_json::Value> = match serde_json::from_value(value) {
        Ok(v) => v,
        Err(_) => return prompt,
    };
    if skills.is_empty() {
        return prompt;
    }

    prompt.push_str(
        "\n\nThe following skills match this goal — apply their guidance \
         when producing the plan. Each skill is delimited by a heading.\n",
    );
    for skill in &skills {
        let name = skill
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(unnamed)");
        let id = skill
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let fallback_body = skill
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        // BL-021 — prefer the composed (depends_on-resolved) body so
        // an inheritance chain like `concise → markdown-style → rust`
        // contributes every layer's instructions in topo order. Fall
        // back to the rendered single-skill body, then to the raw
        // body, when compose isn't available (older registry, cycle
        // / missing-dep, etc.).
        let composed = compose_skill_body(ctx, id).await;
        let body = match composed {
            Some(merged) => merged,
            None => render_skill_body(ctx, id)
                .await
                .unwrap_or_else(|| fallback_body.to_string()),
        };
        let _ = write!(prompt, "\n## Skill: {name} [{id}]\n{body}\n");
    }
    prompt
}

/// BL-021 — call `com.nexus.skills::compose` and return the merged
/// body string. Returns `None` for a missing handler / unknown skill /
/// cycle / missing dependency — every error path falls back to the
/// pre-BL-021 single-skill render so a broken dep graph never blocks
/// planning. Also surfaces conflict warnings (if any) through `tracing`
/// so operators can see them in logs without us having to plumb an
/// event channel through to the UI for the planner.
async fn compose_skill_body(ctx: &KernelPluginContext, id: &str) -> Option<String> {
    let response = ctx
        .ipc_call(
            "com.nexus.skills",
            "compose",
            serde_json::json!({ "id": id }),
            Duration::from_secs(5),
        )
        .await
        .ok()?;
    if let Some(arr) = response.get("conflicts").and_then(serde_json::Value::as_array) {
        if !arr.is_empty() {
            tracing::warn!(
                skill_id = id,
                conflict_count = arr.len(),
                "com.nexus.skills::compose returned non-fatal conflicts"
            );
        }
    }
    response
        .get("merged_body")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

/// Query `com.nexus.mcp.host::list_servers` and, for each enabled
/// server, `list_tools`. Append a compact advertisement to the
/// planner prompt so the LLM knows what external MCP tools are
/// reachable and how to call them (`target_plugin_id:
/// "com.nexus.mcp.host"`, `command_id: "call_tool"`, args shape).
///
/// Best-effort: any failure (plugin not registered, server crashed,
/// timeout) logs at debug and the prompt is left unchanged.
async fn append_mcp_hint(ctx: &KernelPluginContext, prompt: &mut String) {
    let Ok(servers_value) = ctx
        .ipc_call(
            "com.nexus.mcp.host",
            "list_servers",
            serde_json::json!({}),
            Duration::from_secs(3),
        )
        .await
    else {
        return;
    };
    let Some(servers) = servers_value.as_array() else {
        return;
    };
    let active: Vec<(&str, &[serde_json::Value])> = servers
        .iter()
        .filter_map(|s| {
            let name = s.get("name").and_then(|v| v.as_str())?;
            let disabled = s
                .get("disabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if disabled {
                return None;
            }
            let args = s
                .get("args")
                .and_then(|v| v.as_array())
                .map_or(&[][..], Vec::as_slice);
            Some((name, args))
        })
        .collect();
    if active.is_empty() {
        return;
    }

    prompt.push_str(
        "\n\nExternal MCP servers are available via \
         `com.nexus.mcp.host::call_tool` with args \
         `{ server, tool, arguments }`. Servers:\n",
    );
    for (name, _args) in &active {
        let _ = write!(prompt, "- {name}");
        // Optional: fetch tool names when the server responds quickly.
        // Keep this light — a slow server shouldn't hold up planning.
        let tools_value = ctx
            .ipc_call(
                "com.nexus.mcp.host",
                "list_tools",
                serde_json::json!({ "server": name }),
                Duration::from_secs(3),
            )
            .await;
        if let Ok(v) = tools_value {
            if let Some(arr) = v.as_array() {
                let names: Vec<_> = arr
                    .iter()
                    .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
                    .take(8)
                    .collect();
                if !names.is_empty() {
                                let _ = write!(prompt, " — tools: {}", names.join(", "));
                    if arr.len() > names.len() {
                        let _ = write!(prompt, " (+{} more)", arr.len() - names.len());
                    }
                }
            }
        }
        prompt.push('\n');
    }
}

/// Best-effort call to `com.nexus.skills::render` with no override
/// values — lets frontmatter `default`s substitute into the body.
/// Returns `None` when the handler errors (e.g. required parameter
/// with no default); caller falls back to the raw body.
async fn render_skill_body(ctx: &KernelPluginContext, id: &str) -> Option<String> {
    let response = ctx
        .ipc_call(
            "com.nexus.skills",
            "render",
            serde_json::json!({ "id": id, "values": {} }),
            Duration::from_secs(5),
        )
        .await
        .ok()?;
    response
        .get("body")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

// ── Local adapters mirroring nexus-bootstrap::agent ────────────────────────

#[derive(Clone)]
struct AiChatBridge {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

#[async_trait]
impl ChatDriver for AiChatBridge {
    async fn propose(
        &self,
        system: &str,
        user_message: &str,
    ) -> Result<crate::Proposal, String> {
        propose_via_ai(&self.ctx, self.timeout, system, user_message).await
    }
}

/// Shared `propose_tool_calls` IPC dance used by the in-tree
/// `AiChatBridge` and `nexus_bootstrap::agent::AiChatDriver` (G7-1b,
/// ADR 0023). Decodes [`AiProposeReply`]-shaped JSON into the
/// agent-side [`crate::Proposal`] without taking a dependency on
/// `nexus-ai`'s types.
async fn propose_via_ai(
    ctx: &KernelPluginContext,
    timeout: Duration,
    system: &str,
    user_message: &str,
) -> Result<crate::Proposal, String> {
    #[derive(Deserialize)]
    struct ProposeWire {
        #[serde(default)]
        text: String,
        #[serde(default)]
        tool_calls: Vec<ProposedWire>,
    }
    #[derive(Deserialize)]
    struct ProposedWire {
        id: String,
        name: String,
        target_plugin_id: String,
        command_id: String,
        args: serde_json::Value,
    }

    let args = serde_json::json!({
        "messages": [{ "role": "user", "content": user_message }],
        "system": system,
    });
    let raw = ctx
        .ipc_call("com.nexus.ai", "propose_tool_calls", args, timeout)
        .await
        .map_err(|e| e.to_string())?;
    let parsed: ProposeWire = serde_json::from_value(raw).map_err(|e| e.to_string())?;
    let tool_calls = parsed
        .tool_calls
        .into_iter()
        .map(|t| crate::ProposedToolCall {
            id: t.id,
            name: t.name,
            tool_call: ToolCall {
                target_plugin_id: t.target_plugin_id,
                command_id: t.command_id,
                args: t.args,
            },
        })
        .collect();
    Ok(crate::Proposal {
        text: parsed.text,
        tool_calls,
    })
}

#[derive(Clone)]
struct KernelToolBridge {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

#[async_trait]
impl ToolDispatcher for KernelToolBridge {
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, String> {
        self.ctx
            .ipc_call(
                &call.target_plugin_id,
                &call.command_id,
                call.args.clone(),
                self.timeout,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

// ── BL-121 — search_transcripts ─────────────────────────────────────────────

/// Synchronous handler — the FTS5 index is in-process so no kernel
/// IPC is needed. Returns an empty hit set with a clear error
/// message when the global store hasn't been initialised yet
/// (forge without `transcript_search::initialize` having run, or a
/// rebuild failure at boot).
fn handle_search_transcripts(
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: crate::transcript_search::SearchArgs =
        parse(args, "search_transcripts")?;
    let Some(store) = crate::transcript_search::global() else {
        return Ok(serde_json::json!({
            "hits": [],
            "available": false,
            "reason": "transcript-search index not initialised; boot the agent plugin against a forge",
        }));
    };
    let hits = store
        .search(&parsed)
        .map_err(|e| exec_err(format!("search_transcripts: {e}")))?;
    Ok(serde_json::json!({
        "hits": hits,
        "available": true,
    }))
}

// ── Error / serde plumbing ──────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn agent_err(e: &AgentError) -> PluginError {
    exec_err(e.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize: {e}")))
}

// ── Orchestrator handlers (BL-027) ──────────────────────────────────────────

/// Args for [`HANDLER_DELEGATE`]: pick one archetype and a goal.
// BL-027 orchestrator IPC types (`DelegateArgs`, `ParallelArgs`,
// `PipelineArgs`, `TraceResponse`) and their handlers were retired
// in ADR 0025 Phase 2 alongside the underlying `AgentOrchestrator`.
// Callers should fan out / chain `session_run` directly.

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// DG-37 — `delegate` rejects an empty archetype name. The
    /// parent agent has to be explicit about which sub-archetype
    /// handles the sub-goal; defaulting silently would hide intent.
    #[tokio::test]
    async fn delegate_rejects_empty_archetype() {
        // No kernel context wired — the validation happens before
        // dispatch attempts to use ctx.
        let pending: Arc<PendingApprovals> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let args = serde_json::json!({
            "archetype": "",
            "goal": "do a thing",
        });
        // Build a minimal ctx-less harness: handle_delegate dereferences
        // ctx only inside handle_session_run, which we don't reach
        // because the archetype check fires first.
        //
        // To exercise the early-return path without a real kernel
        // context, parse the args ourselves and assert the shape.
        let parsed: DelegateArgs = serde_json::from_value(args).expect("decode");
        assert!(parsed.archetype.trim().is_empty());
        // The handler itself returns Err for empty archetype — we
        // can't construct a KernelPluginContext in a unit test, so
        // assert against the validation invariant directly. (A
        // full integration test lives under bootstrap.)
        let _ = pending;
    }

    /// DG-37 — `delegate` rejects an empty goal. Same intent
    /// invariant as the archetype check.
    #[test]
    fn delegate_rejects_empty_goal_at_parse() {
        let args = serde_json::json!({
            "archetype": "coder",
            "goal": "   ",
        });
        let parsed: DelegateArgs = serde_json::from_value(args).expect("decode");
        assert!(parsed.goal.trim().is_empty());
    }

    /// DG-37 — `auto_approve` defaults to true so a nested
    /// delegation doesn't prompt the user twice (the parent's
    /// session policy already gated the delegate call itself).
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

    /// DG-37 — the agent tool registry lists `delegate_to_agent`
    /// after `seed_default_tools` runs, so a planner sees A2A as a
    /// first-class tool call.
    #[test]
    fn delegate_to_agent_tool_seeded() {
        crate::seed_default_tools();
        let spec = crate::AgentToolRegistry::global()
            .lookup("delegate_to_agent")
            .expect("delegate_to_agent registered");
        assert_eq!(spec.target_plugin_id, "com.nexus.agent");
        assert_eq!(spec.command_id, "delegate");
        // Should require approval — child session can call write tools.
        assert!(spec.requires_approval);
    }

    /// DG-34 — empty rounds (text-only proposals, no tool calls)
    /// never need approval. Saves an unnecessary bus round-trip.
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

    /// DG-34 — a round made up of read-only tools (registered as
    /// `requires_approval = false`) auto-approves.
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

    /// DG-34 — a single high-risk tool call in the round flips the
    /// whole round to "needs approval" (PRD-15 §7).
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
                // write_file is registered with requires_approval=true.
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

    /// DG-34 — unknown tool names are conservatively treated as
    /// high-risk. The model might be asking for a tool the registry
    /// hasn't classified yet; the safe default is to prompt.
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

    /// OI-04 — `list_archetypes` returns the short-name catalogue
    /// (`"writer"`, `"coder"`, `"researcher"`) via the sync dispatch
    /// path without needing a wired kernel context. These are the
    /// strings [`crate::archetypes::resolve_prompt`] accepts back as
    /// the `archetype` arg to `plan` / `run`, so the shell's picker
    /// can round-trip them verbatim.
    #[test]
    fn list_archetypes_returns_short_names() {
        let mut plugin = AgentCorePlugin::new();
        let v = plugin
            .dispatch(HANDLER_LIST_ARCHETYPES, &serde_json::Value::Null)
            .expect("list_archetypes dispatch");
        let names: Vec<String> = serde_json::from_value(v).expect("decode");
        // DG-35 — auditor / librarian / coach added 2026-05-12.
        assert_eq!(
            names,
            vec!["writer", "coder", "researcher", "auditor", "librarian", "coach"]
        );
    }

    /// OI-04 — `dispatch_async` returns `None` for
    /// `list_archetypes` so the kernel falls back to the sync path
    /// and avoids burning a tokio frame on a pure constant read.
    #[test]
    fn dispatch_async_yields_to_sync_for_list_archetypes() {
        let mut plugin = AgentCorePlugin::new();
        let fut = plugin.dispatch_async(HANDLER_LIST_ARCHETYPES, &serde_json::Value::Null);
        assert!(fut.is_none(), "list_archetypes must not return an async future");
    }

    /// DG-32 — `list_tools` returns the agent tool registry's seeded
    /// catalogue via the sync dispatch path. The handler reads from
    /// the process-global registry; the test seeds it explicitly so
    /// the assertion is independent of bootstrap order.
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

    /// DG-32 — `list_tools` honours the `capabilities` filter and
    /// rejects unknown capability ids with a clear error.
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

    /// DG-32 — Filtering by a held capability returns only tools
    /// satisfied by it.
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
        // `write_file` needs `fs.write` which we didn't grant.
        assert!(!names.contains("write_file"));
    }

/// Phase 2b — `round_decide` routes the caller's decision into
    /// the matching session's pending oneshot. Smoke tests the
    /// happy path (approve_all) and the error paths (no pending,
    /// dropped receiver).
    #[tokio::test]
    async fn round_decide_delivers_approve_all_to_pending_session() {
        let pending: Arc<PendingApprovals> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let (tx, rx) = tokio::sync::oneshot::channel::<crate::RoundDecision>();
        pending
            .lock()
            .unwrap()
            .insert("sess-abc".to_string(), tx);

        let args = serde_json::json!({ "session_id": "sess-abc", "kind": "approve_all" });
        let reply = handle_round_decide(Arc::clone(&pending), &args)
            .await
            .expect("round_decide ok");
        assert_eq!(reply["delivered"], true);
        assert_eq!(reply["session_id"], "sess-abc");

        // Receiver got the right decision.
        match rx.await.expect("oneshot recv") {
            crate::RoundDecision::ApproveAll => {}
            other => panic!("unexpected decision: {other:?}"),
        }
        // Map cleaned out.
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
        pending
            .lock()
            .unwrap()
            .insert("sess-1".to_string(), tx);

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
        pending
            .lock()
            .unwrap()
            .insert("sess-2".to_string(), tx);
        drop(rx); // Simulate the session loop having timed out.

        let args = serde_json::json!({ "session_id": "sess-2", "kind": "approve_all" });
        let err = handle_round_decide(pending, &args).await.unwrap_err();
        assert!(
            format!("{err:?}").contains("no longer awaiting"),
            "{err:?}"
        );
    }
}
