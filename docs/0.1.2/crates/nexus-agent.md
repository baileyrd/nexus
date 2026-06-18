# nexus-agent

> Kind: lib · IPC plugin id: com.nexus.agent · CorePlugin: yes · Has settings: yes (`[agent].auto_notify_threshold_s` in `<forge>/.forge/config.toml`; per-call `SessionConfig`/`agent.toml` manifests) · As of: 2026-05-25

## Overview

`nexus-agent` is the agent system (PRD-15). It owns the abstractions every agent
archetype specializes — the `Agent` trait, the `Plan`/`Step`/`ToolCall` model, and
the multi-round session loop (`run_session`) that runs the model → dispatch →
approval cycle in lockstep. A goal comes in; the planner (`LlmAgent`) asks an LLM
for provider-native tool-use blocks; the session loop dispatches the approved
subset through the kernel, feeds results back, and loops until the model emits a
text-only turn, the policy aborts, every call in a round is denied, or the
iteration cap is hit. Each finished run is persisted as a JSON transcript and, for
archetype-tagged runs, distilled into the agent's append-only memory log.

The crate builds on `nexus-ai-runtime` and `nexus-ai` through `ipc_call`, never
through a direct dependency. Planning calls `com.nexus.ai::propose_tool_calls`
(via the local `AiChatBridge` `ChatDriver` adapter); tool steps dispatch through
`com.nexus.ai.runtime` and the targeted service plugins (via the `KernelToolBridge`
`ToolDispatcher`). After BL-134 Phase 2b, `delegate` no longer runs sub-sessions
inline — it submits an `AgentTaskKind::Session` envelope to
`com.nexus.ai.runtime::submit` and blocks on `wait_for`, so the sub-task runs on the
runtime's dedicated worker pool (off the kernel's tokio runtime), stays observable
through the runtime's `list`/`events`, and records parent/child linkage.

Note that despite the `lib.rs` "What this is NOT" header (a doc-comment that
predates the IPC bridge), the crate **is** a CorePlugin today: `AgentCorePlugin`
registers as `com.nexus.agent` and exposes 18 handlers. The library core stays
kernel-free — `Agent`, `Plan`, `ToolDispatcher`, `ChatDriver`, `SessionPolicy` are
trait/data types with no kernel dependency — and `core_plugin.rs` + `handlers/`
provide the live-runtime adapters. This keeps the microkernel invariant: the kernel
never depends on the agent crate; the agent crate reaches storage / AI / git /
terminal / skills / notifications only over IPC.

Beyond the core loop, the crate carries: six built-in archetypes plus a custom
`agent.toml` manifest format (DG-35/DG-36); an agent-side tool registry distinct
from the AI registry (PRD-15 §4); agent-scoped persistent memory keyed by agent id
(DG-33); BL-120 context compression; BL-131 pre-invocation context sanitisation;
BL-121 FTS5 transcript search; and a BL-133 auto-notify subscriber.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (`KernelPluginContext`, `Ipc`,
  `FileSystem`, `Events` traits, `EventFilter`, `NexusEvent`, `RecvError`),
  `nexus-plugins` (`CorePlugin`, `CorePluginFuture`, `PluginError`,
  `define_dispatch_helpers!`).
- **Notable external deps:** `tokio` (async + `oneshot` approval channels),
  `rusqlite` (bundled SQLite for the FTS5 transcript index), `regex-lite` (BL-131
  base64/snapshot scanners), `futures`, `async-trait`, `chrono` (RFC 3339
  timestamps), `uuid` (plan/session ids), `serde`/`serde_json`, `toml` (config +
  manifest parsing), `thiserror`, `tracing`. Optional `ts-rs` + `schemars` behind
  the off-by-default `ts-export` feature (emits TS bindings + JSON Schema for IPC
  arg/reply types into `packages/nexus-extension-api/src/generated/ipc/`).
- **Crates depending on it:** only `nexus-bootstrap`, which registers the
  `AgentCorePlugin` (boot order #16, after `com.nexus.skills` at #8 to break the
  load-time half of the runtime cycle) and wires the `(command, handler-id)` table
  from `IPC_HANDLERS`. There is **no** dependency back into `nexus-ai`,
  `nexus-ai-runtime`, `nexus-skills`, etc. — all those are reached over IPC, and the
  `core_plugin.rs` adapters are re-implemented locally to avoid a circular dep on
  `nexus-bootstrap`.

## Public API surface

Module-by-module (one line each):

- **`lib.rs`** — core types: `Step`, `Plan` (+ `Plan::new`), `ToolCall`,
  `ToolDispatcher` trait (returns `Result<Value, ToolDispatchError>`),
  `ToolDispatchError` + `ToolErrorKind` (typed retry classification —
  `Transient`/`Permanent`/`Unknown`, with `From<String>`/`From<&str>` →
  `Unknown`), `Agent` trait, `AgentError`. Re-exports everything below.
- **`agents.rs`** (`EchoAgent`) — trivial one-step `Agent` for smoke tests/scaffolding.
- **`llm.rs`** — `LlmAgent` (the only in-tree real planner), `ChatDriver` trait,
  `Proposal`, `ProposedToolCall`, `DEFAULT_SYSTEM_PROMPT`. Maps a `Proposal` to a
  `Plan` (one `Step` per tool call; narration-only turns become a single
  informational step).
- **`archetypes.rs`** — six built-in archetypes (`writer`/`coder`/`researcher`/
  `auditor`/`librarian`/`coach`), each an `LlmAgent` with a swapped system prompt +
  id. `build_archetype`, `build_archetype_with_prompt`, `resolve_prompt`,
  `is_builtin_archetype`, and the `*_ID` / `*_SYSTEM_PROMPT` consts.
- **`session.rs`** — the run loop. `run_session` / `run_session_with_id` /
  `run_session_with_config` / `run_session_with_compressor`; `SessionConfig`,
  `SessionOutcome`, `ProposedRound`, `RoundDecision`, `RoundDecisionEntry`,
  `SessionPolicy` trait, `AutoApproveAll`, `RoundRecord`, `ToolCallRecord`,
  `AgentSession`. Caps: `DEFAULT_MAX_ITERATIONS` (32), `MAX_AGENT_ROUNDS` /
  `LEGACY_MAX_AGENT_ROUNDS` (8), `DEFAULT_MAX_TOOL_CALLS_PER_ITERATION` (16).
- **`tool_registry.rs`** — agent-side `AgentToolRegistry` (process-global),
  `AgentToolSpec`, `Capability` (10 domains), `AgentToolError`,
  `AgentToolAccessRecord`, `seed_default_tools`, `default_tool_catalog`,
  `measure_dispatch`. Distinct from `nexus_ai::tools::ToolRegistry` — adds
  capability/approval/duration metadata for policy gating.
- **`memory.rs`** — `MemoryEntry` enum (9 variants), `MemoryError`,
  `normalize_agent_id`, `agent_dir`/`history_path`, `events_from_session`,
  `format_memory_preamble`, `query_entries`, `prune_entries`, `export_markdown`,
  `serialize_entries_jsonl`, `append_entry_to_path`, `read_entries_from_path`.
  `AGENTS_DIR` = `.forge/agents`.
- **`custom_agent.rs`** — `CustomAgentManifest` + sections (`AgentSection`,
  `ExecutionSection`, `ToolsSection`, `MemorySection`, `SystemPromptSection`),
  `ManifestToolPolicy`, `ManifestPolicyGate` (a `SessionPolicy` decorator that
  filters tool calls against a manifest's allow/deny lists), `parse_str`,
  `load_from_path`, `scan_forge`, `resolve_system_prompt`. `MANIFEST_FILE_NAME` =
  `agent.toml`, `AGENTS_DIR` = `.forge/agents`.
- **`compression.rs`** — `Compressor` trait + `LlmCompressor` /
  `KeepDecisionsCompressor` / `NoopCompressor`, `CompactionEvent`, `estimate_tokens`.
- **`context_sanitize.rs`** — `SanitizeOptions`, `SanitizeMetrics`, `sanitize_prompt`
  and four pure passes (dedup repeated results, strip base64 data URIs, compress
  stale browser snapshots, hard-trim oldest to budget).
- **`transcript_search.rs`** — `TranscriptStore`, `TranscriptHit`, `SearchArgs`,
  `TranscriptError`, `RebuildStats`, `initialize`/`global`/`rebuild_from_disk`.
  `TRANSCRIPTS_DB_PATH` = `.forge/agent/transcripts.sqlite`.
- **`auto_notify.rs`** — `load_threshold_secs`, `duration_ms_between`,
  `format_message`, `spawn`; `SESSION_COMPLETED_TOPIC`, `DEFAULT_THRESHOLD_S` (30).
- **`core_plugin.rs`** — `AgentCorePlugin` (+ `new`/`new_with_forge`), `PLUGIN_ID`,
  all `HANDLER_*` id consts, `IPC_HANDLERS` table, `MANIFEST_DEPS`.
- **`handlers/`** — per-handler IPC modules (`plan`, `session`, `round`, `history`,
  `memory`, `custom`, `list_tools`, `search_transcripts`, `delegate`) + `shared`
  (the `AiChatBridge`/`KernelToolBridge`/`BusBridgePolicy` adapters, prompt-assembly
  helpers, the bounded pending-approvals map).

## IPC handlers

`AgentCorePlugin` exposes 18 handlers. `list_archetypes` (8) and `list_tools` (18)
run on the synchronous `dispatch` path (compile-time / in-memory state only);
`search_transcripts` (25) runs synchronously too but is routed via the async path's
`dispatch_async`; everything else is async (`dispatch_async`) and needs the wired
`KernelPluginContext`. Handler ids are append-only; ids 2/3/4/9/10/11/12
(`run`, `run_plan`, `execute_step`, old `delegate`/`parallel`/`pipeline`/`trace_get`)
were retired by ADR 0025 Phase 2 and stay reserved. No handler declares a capability
constant in this crate (the kernel-side capability mapping lives in bootstrap;
`ipc-handlers.md` flags `session_run`/`round_decide` as `ai.chat` and `delegate`/
`plan` as audit candidates).

| Command | Handler id | Args | Returns | Capability | Description |
|---------|-----------|------|---------|------------|-------------|
| `plan` | 1 | `{ goal: string, archetype?: string }` (`GoalArgs`) | `Plan` (JSON) | — (drives chat) | Plan a goal; layers skill/MCP/memory/entity preambles into the system prompt; routes built-in vs. custom-manifest archetype. |
| `history_list` | 5 | `[]` | array of `{ plan_id, goal, created_at, success, steps, bytes }` | — | Enumerate legacy pre-Phase-2a plan transcripts under `.forge/agent/history/`. |
| `history_get` | 6 | `{ plan_id: string }` (`PlanIdArgs`) | stored history JSON | — | Load one legacy history entry. |
| `history_delete` | 7 | `{ plan_id: string }` | `{ deleted: true, plan_id }` | — | Delete one legacy history entry. |
| `list_archetypes` | 8 | `[]` | `["writer","coder","researcher","auditor","librarian","coach"]` | — | Short-name archetype catalogue (sync). |
| `session_run` | 13 | `{ goal, archetype?, system?, auto_approve, approval_timeout_secs?, strict_approval?, session_config?, session_id? }` | `AgentSession` (JSON) | — (`ai.chat`) | Run the multi-round tool loop; persist transcript via `storage::write_vault_file`; auto-record memory; publish `session_completed`. |
| `session_list` | 14 | `[]` | array of `{ id, goal, started_at, ended_at, outcome }` (newest-first) | — | List persisted session transcripts under `.forge/agent/sessions/`. |
| `session_get` | 15 | `{ id: string }` (`SessionIdArgs`) | session JSON from disk | — | Load one session transcript. |
| `session_delete` | 16 | `{ id: string }` | `{ deleted: true, id }` | — | Delete one session transcript. |
| `round_decide` | 17 | `{ session_id, kind: "approve_all" \| "abort" \| "partial", reason?, entries? }` | `{ delivered: true, session_id }` | — (`ai.chat`) | Phase 2b: deliver a caller's `RoundDecision` to a pending interactive session round. |
| `list_tools` | 18 | `{ capabilities?: string[] }` (`ListToolsArgs`) | array of `AgentToolSpec` (sorted by name) | — | Agent tool catalogue, optionally filtered to a held-capability set (unknown id → error). Sync. |
| `list_custom` | 19 | `[]` | `{ manifests: CustomAgentManifest[], errors: [{path, error}] }` | — | Scan `.forge/agents/*/agent.toml`; return parsed manifests + per-file parse errors. |
| `memory_record` | 20 | `{ agent_id, entry: MemoryEntry }` (`MemoryRecordArgs`) | `{ recorded: true }` | — | Append one entry to `<agent>/history.jsonl`. |
| `memory_query` | 21 | `{ agent_id, pattern?, limit? }` (`MemoryQueryArgs`) | array of `MemoryEntry` (newest-first) | — | Substring filter over the agent's memory (default limit 50). |
| `memory_prune` | 22 | `{ agent_id, retention_days }` (`MemoryPruneArgs`) | `{ pruned, kept }` | — | Drop entries older than `retention_days`; `Decision` entries survive unconditionally. |
| `memory_export` | 23 | `{ agent_id }` (`MemoryExportArgs`) | `{ markdown }` | — | Render the agent's history as markdown. |
| `delegate` | 24 | `{ archetype, goal, system?, auto_approve=true, approval_timeout_secs?, strict_approval? }` (`DelegateArgs`) | the sub-session's `session_run` reply JSON | — (drives chat) | Run a sub-goal in a child archetype via `ai.runtime::submit` + `wait_for`; returns the child transcript. |
| `search_transcripts` | 25 | `{ query, agent_id?, since_ts_ms?, limit? }` (`SearchArgs`) | `{ hits: TranscriptHit[], available: bool, reason? }` | — | FTS5 search over indexed `history.jsonl` content. Sync (in-process index). |

## Capabilities

The crate does not register or check kernel capability constants itself — capability
enforcement happens kernel-side when handlers issue `ipc_call`s, and the
`(command → capability)` mapping is owned by bootstrap (see
`docs/0.1.2/ipc-handlers.md`, which marks `session_run`/`round_decide` as `ai.chat`
and flags `delegate`/`plan` as audit candidates).

Internally the crate has its own `Capability` enum (`tool_registry.rs`) — a
**registry-level** notion, separate from kernel capabilities. Ten domains:
`fs.read`, `fs.write`, `terminal.execute`, `search.forge`, `web.fetch`, `mcp.host`,
`git.read`, `git.write`, `database.read`, `database.write` (string form via
`Capability::as_str` / `from_str`, used in `agent.toml` and the `list_tools`
filter). Each `AgentToolSpec` declares `required_capabilities`; the session policy
reads `requires_approval` to gate dispatch, and `list_for_agent` filters the catalog
to tools an agent's held capabilities satisfy. `MANIFEST_DEPS` declares the plugin
ids agent invokes: `com.nexus.storage`, `com.nexus.ai.runtime`, `com.nexus.ai`,
`com.nexus.skills`, `com.nexus.notifications` (`mcp.host` omitted — loads after
agent; `skills` is an intentional runtime cycle).

## Settings / Config

**`SessionConfig`** (per-`session_run` call, also the wire shape): `max_iterations`
(default 32, clamped to ≥1), `max_tool_calls_per_iteration` (default 16, excess
calls truncated with a warning), `max_context_tokens` (default 0 = unbounded;
when > 0, enables BL-120 compression and BL-131 trim using a chars≈4×tokens
heuristic), `provider_hint` (accepted, not yet consulted). `legacy_phase2a()` pins
`max_iterations = 8`.

**Timeouts** (consts in `handlers/shared.rs`, hardcoded — not config): tool dispatch
`DEFAULT_TOOL_TIMEOUT` 60s, chat `DEFAULT_CHAT_TIMEOUT` 300s, approval
`DEFAULT_APPROVAL_TIMEOUT_SECS` 1800s (cap `MAX_APPROVAL_TIMEOUT_SECS` 3600s),
pending-approvals map cap `PENDING_APPROVALS_CAP` 64. `delegate` uses
`SUBMIT_TIMEOUT` 30s and `WAIT_FOR_TIMEOUT` 3h (safety net above the runtime's
2h ceiling).

**`[agent].auto_notify_threshold_s`** — read from `<forge>/.forge/config.toml` by
`auto_notify::load_threshold_secs`; default 30s, `0` disables the subscriber.

**`agent.toml` manifests** (`custom_agent.rs`) — per-custom-agent config under
`.forge/agents/<slug>/agent.toml`: `[agent]` (name/version/description/base
archetype), `[execution]` (`max_steps`, `token_budget`, `time_limit_secs`,
`requires_approval_for`), `[tools]` (`allowed`/`denied`), `[memory]`,
`[system_prompt]`. Unknown keys in known typed tables are rejected.

**transcripts.sqlite / FTS5.** `<forge>/.forge/agent/transcripts.sqlite` holds a
single FTS5 virtual table:

```sql
CREATE VIRTUAL TABLE agent_history_fts USING fts5(
    agent_id  UNINDEXED,
    entry_idx UNINDEXED,   -- 0-based position in the agent's history.jsonl
    role      UNINDEXED,   -- synthesised: user/assistant/tool/error/artifact
    content,               -- the only tokenised column (porter stemmer)
    ts_ms     UNINDEXED,
    tokenize = 'porter'
);
```

It is **derived state** — fully rebuildable from each agent's `history.jsonl` via
`rebuild_from_disk`. `initialize` opens it at `on_init`, rebuilds if empty, and
stashes the handle in a process-global `OnceLock`. Search uses BM25
(`bm25(...)` ASC, lower = more relevant), `snippet()` with `**…**` markers, `limit`
clamped to `[1, 200]` (default 50); `agent_id` and `since_ts_ms` filters AND-combine
with the MATCH expression. The `content` is synthesised per `MemoryEntry` variant by
`render_entry`; empty content (e.g. some `ToolCall` records) is skipped.

Other on-disk locations: legacy plan transcripts `.forge/agent/history/<plan_id>.json`;
session transcripts `.forge/agent/sessions/<id>.json`; agent memory
`.forge/agents/<agent_id>/history.jsonl` (+ `snapshots/`, `artifacts/`). All path
helpers validate the id is non-empty and `[A-Za-z0-9_.-]`-only to prevent traversal.

## Events

- **Published:** `com.nexus.agent.round_proposed` — emitted by `BusBridgePolicy` per
  interactive (non-auto-approve) round with `{ session_id, round, text, tool_calls[]
  (each annotated with requires_approval/registered) }`; the caller answers via the
  `round_decide` handler. `com.nexus.agent.session_completed`
  (`auto_notify::SESSION_COMPLETED_TOPIC`) — emitted by `handle_session_run` after
  every run with `{ session_id, duration_ms, outcome, archetype, goal }`.
- **Subscribed:** `com.nexus.agent.session_completed` — the `auto_notify` subscriber
  (spawned at `on_start` when a tokio runtime is present and threshold > 0) listens
  and fires `com.nexus.notifications::send` when `duration_ms ≥ threshold`. Lagged
  events are skipped; channel close stops the loop.

## Internals & notable implementation details

**Plan/Step execution model.** `LlmAgent::plan` calls the `ChatDriver`, maps each
proposed tool call to one `Step` (`step-1`, `step-2`, …); a narration-only turn
becomes a single informational step with no `tool_call`; the empty case is a planning
failure. The `Plan` carries a UUID + original goal so re-planners/UI don't thread it
separately. The standalone `plan` IPC handler produces a `Plan`; actual execution
goes through the session loop.

**Session run loop (`run_session_with_compressor`).** For up to `max_iterations`
rounds: (1) BL-131 `sanitize_turns` over the tool-result turn contents; (2)
`driver.propose_turns(system, current_turns)` — driver error ends the session
`Errored`; (3) truncate excess tool calls to `max_tool_calls_per_iteration`; (4)
empty tool calls → terminal text round, `Complete`; (5) build a `ProposedRound`,
ask `policy.allow_round`; (6) `execute_round` applies the decision —
`ApproveAll`/`Partial` dispatch the approved subset via `dispatch_one` (each timed
with `Instant`, populating `ToolCallRecord.duration_ms`; Phase 5.5 — a *transient*
dispatch error is retried up to `SessionConfig::max_tool_retries` times with
exponential backoff `tool_retry_backoff_ms`, opt-in/off by default. Retryability
comes from the typed `ToolDispatchError.kind` (`ToolDispatchError::is_retryable`):
`Transient`/`Permanent` are exact, and the kernel bridges set them from
`IpcErrorEnvelope::retryable`; `Unknown` (every `String`/`&str` conversion) falls
back to the `is_retryable_tool_error` message heuristic. Retries are also
idempotency-gated: a transient failure of a tool named in
`SessionConfig::non_idempotent_tools` is reported without a retry so its effect
can't double-apply — the agent service seeds that list from
`AgentToolRegistry::non_idempotent_tool_names` (every catalog tool with
`idempotent = false`: writes, deletes, pushes, terminal exec, delegation, `ask`)
when retries are enabled. Each record carries `ToolCallRecord.attempts` — total
dispatch attempts (`0` = never dispatched, `1` = clean, `1 + N` = `N` retries) —
so consumers read the retry count structurally rather than parsing the
`(after N attempts)` error suffix), `Abort`/`Timeout` stop with a
synthetic narration round; (7) if no call approved → `Aborted`; (8) rebuild
the conversation from the recorded rounds via `compose_turns` — Phase 5.5 (2c)
provider-native multi-turn: a leading `User{goal}` turn then each round as an
`Assistant{text, tool_calls}` turn followed by one `ToolResult` per call (failures
flagged `is_error`), so the model replays its own calls and their real results
rather than a flat goal re-statement; (9) if `max_context_tokens > 0`, run BL-120
compaction (folding oldest live rounds into a summary carried on the goal turn)
while over budget and > `WORKING_SET_ROUNDS` (4) rounds remain unfolded; (10) on
the last iteration set `MaxRounds`. The full transcript (every round, even
compacted ones) is always persisted — compaction only affects the live
conversation.

**Approval & cancellation.** `AutoApproveAll` for `auto_approve: true`;
otherwise `BusBridgePolicy` auto-approves rounds whose every tool call is
`requires_approval = false` (unless `strict_approval`), else publishes
`round_proposed`, parks a `oneshot::Sender` in the bounded `PendingApprovals` map,
and awaits `round_decide` under `approval_timeout_secs`. The map
(`insert_pending_bounded`) prunes entries older than 1h and evicts the oldest past a
64-entry cap so a stuck shell can't leak senders. Timeout → `RoundDecision::Timeout`
→ `ApprovalTimeout`; closed channel → `Abort`. `round_requires_approval` treats
unregistered tool names as high-risk (conservative).

**Tool routing.** Two local adapters in `handlers/shared.rs` mirror the bootstrap
bridges: `AiChatBridge` (`ChatDriver`) calls `com.nexus.ai::propose_tool_calls`
(targets already resolved by the AI plugin's `dispatch_target` mapping);
`KernelToolBridge` (`ToolDispatcher`) forwards each `ToolCall` to
`ctx.ipc_call(target_plugin_id, command_id, args)`, folding any `IpcError` into a
typed `ToolDispatchError` whose `kind` comes from `IpcErrorEnvelope::retryable`
(via `ipc_error_to_dispatch_error`) so the retry policy classifies it exactly. The agent-side
`AgentToolRegistry` is a process-global catalogue (`seed_default_tools` at boot)
holding 13 tools — read/write/search/backlinks/git/terminal/delegate plus three
BL-132 destructive ops (`delete_file`, `replace_in_files`, `git_push`, all
`requires_approval = true`). It is separate from the per-request AI registry; it
adds capability/approval/duration metadata and an in-memory access log (capped at
1024 entries). `validate_params` does cheap structural checks (required fields +
`additionalProperties: false`), not full JSON Schema validation.

**Custom-agent routing.** `resolve_archetype_for_run` tries built-in slugs first,
then loads `.forge/agents/<slug>/agent.toml` (slug validated by
`is_safe_archetype_slug`), layering the manifest's `[system_prompt]` on top of the
base archetype prompt. A non-noop `[tools]` policy becomes a `ManifestPolicyGate`
wrapping the base `SessionPolicy` — manifest denials ride through
`RoundDecision::Partial` so a denied call feeds back as an `is_error` turn the model
can recover from.

**Memory & auto-record.** After each session, `record_session_memory` derives
`MemoryEntry`s via `events_from_session` (compactions first, then one `ToolCall` per
record carrying its dispatch `duration_ms`, an `Error` per failed/denied call, plus a
session-level `Error` for `Errored` outcomes) and appends them to the archetype's
`history.jsonl`. `compose_memory_preamble` reads that log on the *next* invocation to
splice a "recent context" preamble (all decisions up to a cap — decisions survive
prune indefinitely per PRD-15 §5 — plus the most-recent non-decision entries) into
the system prompt. `append_entry_to_path` also live-indexes into the FTS5 store when
a global handle exists.

**Runtime cycle with skills.** `dispatch_run`/prompt assembly invokes
`com.nexus.skills::{triggered_by, compose, render}` while skills invokes
`com.nexus.agent::session_run`. The cycle is deliberate and lock-free/async; boot
order (skills #8 before agent #16) breaks the load-time half.

## Tests

All tests are inline `#[cfg(test)]` modules — there is **no** `tests/` directory.
Coverage by module:

- **`session.rs`** — loop behaviour: auto-approve to terminal text round, empty-goal
  short-circuit, abort/partial/timeout decisions, max-rounds cap (legacy 8 and
  default >8), per-iteration tool-call truncation, driver-error → `Errored`; BL-120
  compression (50-turn session preserves every decision, disabled at
  `max_context_tokens = 0`, skips when working set not full); BL-119 `SessionConfig`
  defaults/sanitize/serde.
- **`llm.rs`** — one-step-per-tool-call mapping, narration fallback, empty-proposal
  and empty-goal rejection.
- **`tool_registry.rs`** — capability round-trip/parse, register/lookup/overwrite,
  capability filtering + denial, `validate_params` (object/required/unknown-field),
  access-log cap (2000→1024), default-catalog coverage, BL-132 destructive-tool
  flags + target handler pins, `write_file` approval flag, global Arc identity.
- **`memory.rs`** — agent-id normalize rules, append/read round-trip, malformed-line
  skip, substring/limit query, prune (keeps decisions), markdown export of every
  variant, `events_from_session` (duration carry, per-record tool calls,
  error-alongside-failed, denied-as-unsuccessful, compactions-first, session-level
  error), `format_memory_preamble` (caps, newest-first, short-form rendering),
  `serialize_entries_jsonl`.
- **`transcript_search.rs`** — `render_entry` roles, replace/search round-trip,
  overwrite, agent-id + since-ts filters, limit clamp (≤200), live `append_entry`,
  `is_empty`, `rebuild_from_disk` (walks agents, zero for missing dir),
  graceful invalid-query (wrapped Sqlite error, no panic).
- **`archetypes.rs`** — unknown→default fallback, case-insensitive resolve, DG-35
  archetype ids/prompts, `build_archetype` id assignment + extra-prompt layering,
  `is_builtin_archetype`, `build_archetype_with_prompt`.
- **`handlers/delegate.rs`** — `extract_session_outcome` projection: finished
  outcome, failed/cancelled events, timed-out envelope, missing terminal event,
  tail-first scan past chunk noise.
- **`core_plugin.rs`** — entity-preamble renderer, delegate arg parsing/defaults,
  slug safety, round-requires-approval classification, `list_archetypes` /
  `list_tools` sync dispatch, `round_decide` delivery (approve-all/partial,
  no-pending/dropped-receiver errors).
- **`handlers/shared.rs`** — `insert_pending_bounded` cap/aging/eviction.
- **`agents.rs`** — `EchoAgent` one-step plan, empty-goal reject, custom id.
- **`auto_notify.rs`** — threshold load (default/explicit/zero/garbage),
  `duration_ms_between`, `format_message`.
