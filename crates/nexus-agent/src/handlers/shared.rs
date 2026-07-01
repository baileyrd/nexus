//! Shared infrastructure used across the agent's IPC handlers.
//!
//! Lives under `handlers/` (rather than `core_plugin.rs`) so each
//! per-handler module can pull in just the helpers it needs. Nothing
//! here is part of the plugin's public API — every item is
//! `pub(crate)` for use by the dispatcher (`core_plugin.rs`) and the
//! sibling handler modules.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{Events as _, FileSystem as _, Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::Deserialize;

use crate::{
    AgentError, ChatDriver, ToolCall, ToolDispatchError, ToolDispatcher, DEFAULT_SYSTEM_PROMPT,
};

/// Reverse-DNS identifier of the agent core plugin. Re-exported by
/// `core_plugin.rs` as the public `PLUGIN_ID`.
pub(crate) const PLUGIN_ID: &str = "com.nexus.agent";

/// Default per-tool-call timeout used by the executor when no
/// caller-provided override lands. Matches the bootstrap bridge.
pub(crate) const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(60);
/// Default chat timeout; planner prompts can cost remote-provider
/// latency. Matches the bootstrap bridge.
pub(crate) const DEFAULT_CHAT_TIMEOUT: Duration = Duration::from_secs(300);

/// Default approval-callback timeout for `auto_approve: false`
/// sessions.
pub(crate) const DEFAULT_APPROVAL_TIMEOUT_SECS: u64 = 1800;
/// Hard cap on the caller-supplied `approval_timeout_secs` override.
pub(crate) const MAX_APPROVAL_TIMEOUT_SECS: u64 = 3600;

/// Maximum entries retained in the pending-approvals map. BL-137:
/// caps the previously-unbounded `HashMap` so a stuck shell can no
/// longer leak `oneshot::Sender`s. Subsumed by BL-134 Phase 5 (the
/// queue will move into the runtime); kept here until that lands.
pub(crate) const PENDING_APPROVALS_CAP: usize = 64;

/// Single entry in the pending-approvals map, paired with the
/// `Instant` it was inserted so the cleanup pass can age stale
/// entries out past [`MAX_APPROVAL_TIMEOUT_SECS`]. The sender is
/// `Option`-wrapped because the cleanup path takes it without
/// removing the map entry (the take returns ownership of the channel
/// so the closing-side notifies the awaiter).
pub(crate) struct PendingEntry {
    pub(crate) tx: tokio::sync::oneshot::Sender<crate::RoundDecision>,
    pub(crate) inserted_at: std::time::Instant,
}

/// Map of pending approval awaits keyed by session id. Bounded at
/// [`PENDING_APPROVALS_CAP`] with entries pruned past
/// [`MAX_APPROVAL_TIMEOUT_SECS`] on each insert. Use
/// [`insert_pending_bounded`] for inserts, not raw `HashMap::insert`.
pub(crate) type PendingApprovals =
    std::sync::Mutex<std::collections::HashMap<String, PendingEntry>>;

/// Bounded insert into the pending-approvals map. Prunes entries
/// older than [`MAX_APPROVAL_TIMEOUT_SECS`] first; if the map is
/// still at capacity, evicts the oldest entry (the receiver on that
/// side observes a closed channel and aborts cleanly). Returns the
/// number of evicted entries for observability.
pub(crate) fn insert_pending_bounded(
    map: &mut std::collections::HashMap<String, PendingEntry>,
    session_id: String,
    tx: tokio::sync::oneshot::Sender<crate::RoundDecision>,
) -> usize {
    let now = std::time::Instant::now();
    let max_age = std::time::Duration::from_secs(MAX_APPROVAL_TIMEOUT_SECS);
    let before = map.len();
    map.retain(|_, entry| now.duration_since(entry.inserted_at) < max_age);
    let aged = before - map.len();

    let mut evicted = aged;
    while map.len() >= PENDING_APPROVALS_CAP {
        // Evict the entry with the earliest `inserted_at`. There is
        // at most `PENDING_APPROVALS_CAP` entries to scan, so this is
        // bounded-cost.
        if let Some(oldest_key) = map
            .iter()
            .min_by_key(|(_, e)| e.inserted_at)
            .map(|(k, _)| k.clone())
        {
            map.remove(&oldest_key);
            evicted += 1;
        } else {
            break;
        }
    }

    map.insert(
        session_id,
        PendingEntry {
            tx,
            inserted_at: now,
        },
    );
    evicted
}

/// Default interactive `ask` wait — how long the handler blocks for the
/// user's answers before returning `timed_out`. The `ask` tool carries a
/// longer per-tool dispatch timeout
/// ([`crate::tool_registry::ASK_DISPATCH_TIMEOUT_MS`]) so the bridge's
/// `ipc_call` deadline never cuts this wait off mid-flight; the invariant
/// (`ASK_DISPATCH_TIMEOUT_MS` > this) is guarded by a test in
/// [`super::ask`].
pub(crate) const DEFAULT_ASK_TIMEOUT_SECS: u64 = 300;

/// One pending `ask`, paired with its insert time for aging — mirrors
/// [`PendingEntry`]. The oneshot carries the frontend's answers as raw JSON.
pub(crate) struct AskEntry {
    pub(crate) tx: tokio::sync::oneshot::Sender<serde_json::Value>,
    pub(crate) inserted_at: std::time::Instant,
}

/// Map of pending `ask` awaits keyed by a fresh ask id. Same bound + aging
/// posture as [`PendingApprovals`].
pub(crate) type PendingAsks = std::sync::Mutex<std::collections::HashMap<String, AskEntry>>;

/// Bounded insert into the pending-asks map; mirrors [`insert_pending_bounded`].
pub(crate) fn insert_ask_bounded(
    map: &mut std::collections::HashMap<String, AskEntry>,
    ask_id: String,
    tx: tokio::sync::oneshot::Sender<serde_json::Value>,
) -> usize {
    let now = std::time::Instant::now();
    let max_age = std::time::Duration::from_secs(MAX_APPROVAL_TIMEOUT_SECS);
    let before = map.len();
    map.retain(|_, entry| now.duration_since(entry.inserted_at) < max_age);
    let mut evicted = before - map.len();
    while map.len() >= PENDING_APPROVALS_CAP {
        if let Some(oldest) = map
            .iter()
            .min_by_key(|(_, e)| e.inserted_at)
            .map(|(k, _)| k.clone())
        {
            map.remove(&oldest);
            evicted += 1;
        } else {
            break;
        }
    }
    map.insert(
        ask_id,
        AskEntry {
            tx,
            inserted_at: now,
        },
    );
    evicted
}

// ── Error / serde plumbing — SD-01 ───────────────────────────────────────────

nexus_plugins::define_dispatch_helpers!(pub(crate));

pub(crate) fn agent_err(e: &AgentError) -> PluginError {
    exec_err(e.to_string())
}

// ── DG-36 follow-up: custom-archetype routing ──────────────────────────────

/// Where the system prompt used by `handle_plan` / `handle_session_run`
/// came from. Lets the call site pick between the built-in fast path
/// (`&'static` prompt strings) and the owned-string path that custom
/// manifests require.
#[derive(Debug, Clone)]
pub(crate) enum ArchetypeSource {
    /// Caller passed nothing — DEFAULT_SYSTEM_PROMPT.
    Default,
    /// Caller passed one of the six built-in slugs.
    Builtin,
    /// Caller passed a slug that matched a manifest under
    /// `<forge>/.forge/agents/<slug>/agent.toml`.
    CustomManifest { slug: String },
}

/// Resolution result for an `--archetype <slug>` argument.
pub(crate) struct ResolvedArchetype {
    pub(crate) agent_id: String,
    pub(crate) system_prompt: String,
    pub(crate) source: ArchetypeSource,
    /// Full parsed manifest for custom slugs.
    pub(crate) manifest: Option<crate::CustomAgentManifest>,
}

/// DG-36 — resolve an `--archetype` argument into a concrete
/// [`ResolvedArchetype`].
pub(crate) async fn resolve_archetype_for_run(
    ctx: &KernelPluginContext,
    name: Option<&str>,
) -> ResolvedArchetype {
    let trimmed = name.map(str::trim).filter(|s| !s.is_empty());
    if let Some(slug) = trimmed {
        if crate::archetypes::is_builtin_archetype(slug) {
            let (id, prompt) = crate::archetypes::resolve_prompt(Some(slug));
            return ResolvedArchetype {
                agent_id: id.to_string(),
                system_prompt: prompt.to_string(),
                source: ArchetypeSource::Builtin,
                manifest: None,
            };
        }
        // Slug doesn't match a built-in — try the custom-manifest path.
        match load_custom_archetype_prompt(ctx, slug).await {
            Ok(Some((id, prompt, manifest))) => {
                return ResolvedArchetype {
                    agent_id: id,
                    system_prompt: prompt,
                    source: ArchetypeSource::CustomManifest {
                        slug: slug.to_string(),
                    },
                    manifest: Some(manifest),
                };
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
    ResolvedArchetype {
        agent_id: id.to_string(),
        system_prompt: prompt.to_string(),
        source: ArchetypeSource::Default,
        manifest: None,
    }
}

/// `true` when `slug` is safe to splice into a `<forge>/.forge/agents/<slug>/…`
/// path.
pub(crate) fn is_safe_archetype_slug(slug: &str) -> bool {
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
/// context and assemble the layered prompt.
pub(crate) async fn load_custom_archetype_prompt(
    ctx: &KernelPluginContext,
    slug: &str,
) -> Result<Option<(String, String, crate::CustomAgentManifest)>, String> {
    if !is_safe_archetype_slug(slug) {
        return Err(format!("rejecting suspicious slug `{slug}`"));
    }
    let manifest_path = std::path::Path::new(crate::custom_agent::AGENTS_DIR)
        .join(slug)
        .join(crate::custom_agent::MANIFEST_FILE_NAME);
    let bytes = match ctx.read_file(&manifest_path).await {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    let body = std::str::from_utf8(&bytes)
        .map_err(|e| format!("manifest not UTF-8 at {}: {e}", manifest_path.display()))?
        .to_string();
    let manifest = crate::custom_agent::parse_str(&body, slug, &manifest_path)
        .map_err(|e| format!("parse failed for {}: {e}", manifest_path.display()))?;

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
            .map_err(|e| {
                format!(
                    "system prompt file not UTF-8 at {}: {e}",
                    full_path.display()
                )
            })?
            .to_string()
    } else {
        return Ok(None);
    };

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
    Ok(Some((id, layered, manifest)))
}

// ── Memory helpers used by `memory` and `session` handlers ─────────────────

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub(crate) fn parse_memory_lines(bytes: &[u8]) -> Vec<crate::memory::MemoryEntry> {
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

/// DG-33 follow-up — read the agent's memory log via the kernel
/// context and render the prompt-time recall preamble.
pub(crate) async fn compose_memory_preamble(
    ctx: &KernelPluginContext,
    agent_id: &str,
) -> Option<String> {
    const DECISION_CAP: usize = 8;
    const RECENT_CAP: usize = 6;
    if crate::memory::normalize_agent_id(agent_id).is_err() {
        return None;
    }
    let path = crate::memory::history_path(agent_id);
    let bytes = ctx.read_file(&path).await.ok()?;
    let entries = parse_memory_lines(&bytes);
    if entries.is_empty() {
        return None;
    }
    crate::memory::format_memory_preamble(&entries, DECISION_CAP, RECENT_CAP)
}

/// BL-128 thin slice — call `com.nexus.storage::entity_search` (with
/// FAISS-backed fallback) and render the matching entities as a
/// prompt-time recall preamble.
pub(crate) async fn compose_entity_preamble(
    ctx: &KernelPluginContext,
    goal: &str,
) -> Option<String> {
    const ENTITY_RECALL_CAP: u64 = 5;

    if let Ok(response) = ctx
        .ipc_call(
            "com.nexus.ai",
            "entity_recall",
            serde_json::json!({
                "query": goal,
                "limit": ENTITY_RECALL_CAP,
            }),
            Duration::from_secs(15),
        )
        .await
    {
        if let Some(hits) = response
            .get("results")
            .and_then(serde_json::Value::as_array)
        {
            if !hits.is_empty() {
                if let Some(rendered) = format_entity_preamble(hits) {
                    return Some(rendered);
                }
            }
        }
    }

    let response = ctx
        .ipc_call(
            "com.nexus.storage",
            "entity_search",
            serde_json::json!({
                "query": goal,
                "limit": ENTITY_RECALL_CAP,
            }),
            Duration::from_secs(5),
        )
        .await
        .ok()?;
    let hits = response.get("results")?.as_array()?;
    if hits.is_empty() {
        return None;
    }
    format_entity_preamble(hits)
}

/// Pure renderer for [`compose_entity_preamble`].
pub(crate) fn format_entity_preamble(hits: &[serde_json::Value]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for hit in hits {
        let id = hit.get("id").and_then(serde_json::Value::as_str)?;
        let entity_type = hit
            .get("entity_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("entity");
        let description = hit
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if description.is_empty() {
            lines.push(format!("- {id} ({entity_type})"));
        } else {
            lines.push(format!("- {id} ({entity_type}): {description}"));
        }
    }
    if lines.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Known entities relevant to this goal (from the forge's `entities/` directory):\n",
    );
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    Some(out.trim_end().to_string())
}

// ── Skill-aware system prompt assembly ─────────────────────────────────────

/// Build a planner system prompt that layers in any skill whose
/// triggers match the goal text.
pub(crate) async fn system_prompt_with_skills(
    ctx: &KernelPluginContext,
    goal: &str,
    agent_id: Option<&str>,
) -> String {
    let mut prompt = String::from(DEFAULT_SYSTEM_PROMPT);
    append_mcp_hint(ctx, &mut prompt).await;

    if let Some(slug) = agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(memory_preamble) = compose_memory_preamble(ctx, slug).await {
            prompt.push_str("\n\n");
            prompt.push_str(&memory_preamble);
        }
    }

    if let Some(entity_preamble) = compose_entity_preamble(ctx, goal).await {
        prompt.push_str("\n\n");
        prompt.push_str(&entity_preamble);
    }

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

/// BL-021 — call `com.nexus.skills::compose` and return the merged body.
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
    if let Some(arr) = response
        .get("conflicts")
        .and_then(serde_json::Value::as_array)
    {
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

/// Query `com.nexus.mcp.host` and append an MCP advertisement.
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

/// Best-effort call to `com.nexus.skills::render` with no override values.
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
pub(crate) struct AiChatBridge {
    pub(crate) ctx: Arc<KernelPluginContext>,
    pub(crate) timeout: Duration,
}

#[async_trait]
impl ChatDriver for AiChatBridge {
    async fn propose(&self, system: &str, user_message: &str) -> Result<crate::Proposal, String> {
        let args = serde_json::json!({
            "messages": [{ "role": "user", "content": user_message }],
            "system": system,
        });
        propose_call(&self.ctx, self.timeout, args).await
    }

    async fn propose_turns(
        &self,
        system: &str,
        turns: &[crate::AgentChatTurn],
    ) -> Result<crate::Proposal, String> {
        // Phase 5.5 (2c) — forward the structured conversation so the
        // provider replays real `tool_use` / `tool_result` turns instead
        // of a restated-goal blob. `messages` stays present (empty) so
        // the wire decode keeps a valid (back-compat) shape; the AI
        // handler prefers `turns` when non-empty.
        let args = serde_json::json!({
            "messages": [],
            "turns": turns,
            "system": system,
        });
        propose_call(&self.ctx, self.timeout, args).await
    }
}

/// Shared `propose_tool_calls` IPC dance: dispatch `args` at
/// `com.nexus.ai` and decode the reply into a [`crate::Proposal`].
/// Callers build `args` (legacy `messages` or rich `turns`); the decode
/// is identical either way.
async fn propose_call(
    ctx: &KernelPluginContext,
    timeout: Duration,
    args: serde_json::Value,
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
pub(crate) struct KernelToolBridge {
    pub(crate) ctx: Arc<KernelPluginContext>,
    pub(crate) timeout: Duration,
}

#[async_trait]
impl ToolDispatcher for KernelToolBridge {
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, ToolDispatchError> {
        // A registered tool may override the default dispatch ceiling
        // (e.g. `ask` waits minutes for a human); unknown routes use the
        // bridge default.
        let timeout = crate::AgentToolRegistry::global()
            .dispatch_timeout_for(&call.target_plugin_id, &call.command_id)
            .unwrap_or(self.timeout);
        self.ctx
            .ipc_call(
                &call.target_plugin_id,
                &call.command_id,
                call.args.clone(),
                timeout,
            )
            .await
            .map_err(|e| ipc_error_to_dispatch_error(&e))
    }
}

/// Fold an [`nexus_kernel::IpcError`] into a typed [`ToolDispatchError`],
/// taking the retry classification from the kernel's authoritative
/// [`nexus_kernel::IpcErrorEnvelope::retryable`] flag (Timeout / Cancelled
/// are retryable; not-found, capability denial, crashes, serialization are
/// not). This replaces the previous `e.to_string()` flattening that forced
/// the session loop to re-derive retryability by sniffing the message.
pub(crate) fn ipc_error_to_dispatch_error(err: &nexus_kernel::IpcError) -> ToolDispatchError {
    let retryable = nexus_kernel::IpcErrorEnvelope::from_ipc_error(err).retryable;
    ToolDispatchError::classified(err.to_string(), retryable)
}

// ── BusBridgePolicy (Phase 2b approval callback) ───────────────────────────

/// Defensive helper: drop any leftover pending entry for `id`.
pub(crate) fn drop_pending(pending: &Arc<PendingApprovals>, id: &str) {
    if let Ok(mut map) = pending.lock() {
        map.remove(id);
    }
}

/// Bus-bridge approval policy (ADR 0024 Phase 2b).
pub(crate) struct BusBridgePolicy {
    pub(crate) session_id: String,
    pub(crate) ctx: Arc<KernelPluginContext>,
    pub(crate) pending: Arc<PendingApprovals>,
    pub(crate) timeout: Duration,
    pub(crate) strict_approval: bool,
}

/// DG-34 — classify a [`crate::ProposedRound`] against the agent
/// tool registry.
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
        if !self.strict_approval {
            let registry = crate::AgentToolRegistry::global();
            if !round_requires_approval(round, &registry) {
                return crate::RoundDecision::ApproveAll;
            }
        }

        let (tx, rx) = tokio::sync::oneshot::channel::<crate::RoundDecision>();
        match self.pending.lock() {
            Ok(mut map) => {
                let evicted = insert_pending_bounded(&mut map, self.session_id.clone(), tx);
                if evicted > 0 {
                    tracing::warn!(
                        session_id = %self.session_id,
                        evicted,
                        "pending-approvals map evicted aged/oldest entries on insert"
                    );
                }
            }
            Err(e) => {
                return crate::RoundDecision::Abort(format!("session approval map poisoned: {e}"));
            }
        };

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
        if let Err(e) = self.ctx.publish("com.nexus.agent.round_proposed", payload) {
            drop_pending(&self.pending, &self.session_id);
            return crate::RoundDecision::Abort(format!("publish round_proposed: {e}"));
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(decision)) => decision,
            Ok(Err(_recv_err)) => {
                drop_pending(&self.pending, &self.session_id);
                crate::RoundDecision::Abort("approval channel closed without a decision".into())
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

/// DG-36 follow-up + RFC 0008 (Phase 5.4) — run a (possibly resumed / forked)
/// session with an optional [`crate::ManifestPolicyGate`] wrapping the base
/// policy. Drives [`crate::session::run_session_resumed`] with an inherited
/// `seed_rounds` prefix and an optional `follow_up` message; an empty seed +
/// no follow-up is an ordinary fresh run.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_session_optionally_gated_resumed<D, P, T>(
    driver: &D,
    dispatcher: &T,
    base_policy: P,
    manifest_policy: Option<crate::ManifestToolPolicy>,
    goal: &str,
    system: &str,
    archetype: Option<String>,
    id: String,
    mut config: crate::SessionConfig,
    seed_rounds: Vec<crate::RoundRecord>,
    follow_up: Option<String>,
) -> crate::session::AgentSession
where
    D: crate::ChatDriver + ?Sized,
    P: crate::SessionPolicy,
    T: crate::ToolDispatcher + ?Sized,
{
    // Phase 5.5 follow-up — make opt-in retries idempotency-aware. When a
    // caller turns retries on but doesn't name the non-idempotent tools,
    // seed the deny-list from the registry so a transient failure of a
    // mutating / side-effecting tool isn't blindly re-dispatched. A caller
    // that supplied its own list (even an empty one is treated as "use the
    // registry default" here) keeps non-empty overrides intact.
    if config.max_tool_retries > 0 && config.non_idempotent_tools.is_empty() {
        config.non_idempotent_tools =
            crate::AgentToolRegistry::global().non_idempotent_tool_names();
    }
    match manifest_policy {
        Some(mp) => {
            let wrapped = crate::ManifestPolicyGate::new(base_policy, mp);
            crate::session::run_session_resumed(
                driver,
                dispatcher,
                &wrapped,
                goal,
                system,
                archetype,
                id,
                config,
                seed_rounds,
                follow_up,
            )
            .await
        }
        None => {
            crate::session::run_session_resumed(
                driver,
                dispatcher,
                &base_policy,
                goal,
                system,
                archetype,
                id,
                config,
                seed_rounds,
                follow_up,
            )
            .await
        }
    }
}

#[cfg(test)]
mod pending_bounded_tests {
    use super::*;

    /// Inserting up to `PENDING_APPROVALS_CAP` entries keeps every one.
    #[test]
    fn under_cap_keeps_every_entry() {
        let mut map = std::collections::HashMap::new();
        for i in 0..PENDING_APPROVALS_CAP {
            let (tx, _rx) = tokio::sync::oneshot::channel();
            let evicted = insert_pending_bounded(&mut map, format!("sess-{i}"), tx);
            assert_eq!(evicted, 0);
        }
        assert_eq!(map.len(), PENDING_APPROVALS_CAP);
    }

    /// Past the cap, the oldest entry is evicted on each new insert.
    #[test]
    fn over_cap_evicts_oldest() {
        let mut map = std::collections::HashMap::new();
        // Fill to cap.
        for i in 0..PENDING_APPROVALS_CAP {
            let (tx, _rx) = tokio::sync::oneshot::channel();
            insert_pending_bounded(&mut map, format!("sess-{i}"), tx);
        }
        assert!(map.contains_key("sess-0"));
        // One more push — sess-0 is the oldest, so it gets evicted.
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let evicted = insert_pending_bounded(&mut map, "sess-new".into(), tx);
        assert_eq!(evicted, 1);
        assert_eq!(map.len(), PENDING_APPROVALS_CAP);
        assert!(!map.contains_key("sess-0"));
        assert!(map.contains_key("sess-new"));
    }

    /// An entry older than `MAX_APPROVAL_TIMEOUT_SECS` is aged out by
    /// the next insert, even when the map is under cap.
    #[test]
    fn aged_entry_pruned_on_insert() {
        let mut map = std::collections::HashMap::new();
        let (tx_old, _rx_old) = tokio::sync::oneshot::channel();
        map.insert(
            "sess-stale".into(),
            PendingEntry {
                tx: tx_old,
                inserted_at: std::time::Instant::now()
                    - std::time::Duration::from_secs(MAX_APPROVAL_TIMEOUT_SECS + 1),
            },
        );
        let (tx_new, _rx_new) = tokio::sync::oneshot::channel();
        let evicted = insert_pending_bounded(&mut map, "sess-fresh".into(), tx_new);
        assert_eq!(evicted, 1);
        assert!(!map.contains_key("sess-stale"));
        assert!(map.contains_key("sess-fresh"));
    }
}
