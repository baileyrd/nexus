# nexus-ai-runtime

> Kind: lib · IPC plugin id: `com.nexus.ai.runtime` · CorePlugin: yes · Has settings: no (TOML); a few compile-time constants · As of: 2026-05-25

## Overview

`nexus-ai-runtime` is the shared AI/agent execution substrate for the workspace (BL-134 / ADR 0028). It collapses three primitives that previously lived as ad-hoc prototypes scattered across `nexus-agent`, `nexus-ai`, and `nexus-workflow` into one crate:

1. **A task scheduler** — typed `AgentTask` submissions land in an in-memory run store (`scheduler::Store`) keyed by a server-allocated `task_id`, with per-run status, timestamps, and a bounded event ring.
2. **A typed AI event channel** — every existing `com.nexus.ai.stream_*` / `com.nexus.agent.*` bus topic is wrapped into a typed `AiEvent` envelope and republished under `com.nexus.ai.runtime.*`, so a UI / router can consume one typed cross-subsystem stream instead of stitching together raw producer topics.
3. **A dedicated worker pool** — a multi-thread tokio runtime (`pool::WorkerPool`) owned by the plugin, so long-running LLM rounds run off the host runtime serving UI / IPC traffic and can't starve it.

The crate exposes these through a `CorePlugin` (`com.nexus.ai.runtime`) registered by `nexus-bootstrap`. The task brief's "NOT a CorePlugin" is **incorrect** — `core_plugin::AiRuntimeCorePlugin` implements `nexus_plugins::CorePlugin` and is registered as a core plugin with nine IPC handlers; bootstrap wires it via `crates/nexus-bootstrap/src/plugins/ai_runtime.rs`.

It is a separate crate (rather than living inside `nexus-ai` or `nexus-agent`) precisely to keep its dependency surface minimal and avoid an architectural cycle. The runtime drives `com.nexus.agent::session_run`, `com.nexus.ai::stream_chat`/`ask`, and arbitrary workflow steps **purely over IPC** — it carries their argument bodies as `serde_json::Value` and never links `nexus-agent` or `nexus-ai` as a Cargo dependency. This is the microkernel "IPC over direct calls" invariant in practice: the runtime knows only `(target_plugin, command, args)`, not the producers' Rust types.

How the siblings build on it (all over IPC / the shared accessor, no reverse edge into this crate's internals):
- **`nexus-agent`** — `handlers/delegate.rs` submits a child `Session` task via `com.nexus.ai.runtime::submit` and blocks on `com.nexus.ai.runtime::wait_for` until terminal. This is the BL-134 delegate composition path.
- **`nexus-ai`** — `indexing_daemon.rs` calls `nexus_ai_runtime::shared_pool_handle()` to reuse the runtime's worker pool for background indexing instead of building a second tokio runtime (ADR 0028 Phase 4). `nexus-ai` also depends on the crate directly in its `Cargo.toml` for that accessor.
- **`nexus-workflow`** — async (`async = true`) steps are packaged as `WorkflowAiStep` tasks and submitted so the step doesn't block the workflow's per-step await loop (Phase 3).

Persistence is **not** wired: the store is purely in-memory and is dropped on plugin shutdown. ADR 0028 §Open follow-ups tracks `<forge>/.forge/ai-runtime/runs.db`.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-kernel` (KernelPluginContext, EventBus, Ipc/Events traits, CancellationToken integration), `nexus-plugin-api`, `nexus-plugins` (`CorePlugin`, `PluginError`, `define_dispatch_helpers!`), `nexus-types`.
- **Notable external deps:** `tokio` + `tokio-util` (multi-thread runtime, `Notify`, `CancellationToken`), `serde` / `serde_json`, `uuid`, `chrono`, `thiserror`, `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature for TS/JSON-schema binding emission.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin), `nexus-ai` (uses `shared_pool_handle`). `nexus-agent` and `nexus-workflow` consume it only over the IPC boundary — no Cargo edge.

## Public API surface

**`lib.rs` — wire types & ids**
- `PLUGIN_ID` (`"com.nexus.ai.runtime"`), `BUS_TOPIC_PREFIX` (`"com.nexus.ai.runtime."`), `PER_RUN_EVENT_BUFFER_CAP` (`256`).
- `TaskPriority` — `Background` / `Interactive` (default) / `Critical`. Phase 1 treats all equally (FIFO); per-priority pools are reserved for Phase 5.
- `AgentTaskKind` — tagged enum: `Session { args }` (wired), `AiStream { args }` (reserved Phase 2), `WorkflowAiStep { target_plugin, command, args, workflow, step }` (Phase 3). `.label()` returns a stable short string.
- `AiRuntimeSubmitArgs` / `AiRuntimeSubmitReply` — submit envelope (task, priority, optional `parent`) and `{ task_id }` reply.
- `AgentRun` / `AgentRunSummary` — full run snapshot (incl. embedded `events` ring) and compact list row.
- `RunStatus` — `Queued` / `Running` / `Paused` (reserved) / `Cancelled` / `Completed` / `Failed`.
- `AiRuntimeControlArgs` (cancel/pause/resume), `AiRuntimeGetArgs`, `AiRuntimeListArgs` (filter by status / since / limit), `AiRuntimeEventsArgs` (with `since_seq`), `AiRuntimeWaitForArgs` / `AiRuntimeWaitForReply` (with `timed_out` flag), `PoolStats`.
- `EventRing` / `SharedEventRing` (`pub(crate)`) — bounded FIFO ring buffer with monotonic seq numbers; `push`, `snapshot`, `snapshot_after`.
- Re-export: `shared_pool_handle` (from `pool`).

**`events.rs` — typed lifecycle envelope**
- `AiEvent` — tagged enum (`kind`), every variant carries `task_id`: `Submitted`, `Started`, `TokenChunk`, `ToolCalled` (reserved), `ToolResult` (reserved), `RoundProposed`, `RoundDecided`, `Paused`/`Resumed`/`Cancelled` (reserved/Phase 5), `Finished { outcome }`, `Failed { error, retriable }`.
- `topic_suffix(&AiEvent)` / `topic_for(&AiEvent)` — map a variant to its bus suffix / full topic.
- `AiEvent::suffix()`, `::task_id()`, `::implied_status()` — the last maps an event to the `RunStatus` transition it implies (or `None` for non-status events like token chunks).

**`pool.rs` — worker pool**
- `WorkerPool::start(Option<usize>)` — build the dedicated multi-thread runtime; `handle()`, `metrics()`, `publish_shared_handle()`.
- `PoolMetrics { workers }`.
- `shared_pool_handle() -> Option<Handle>` — process-wide accessor over a `OnceLock<Handle>`; the sibling-subsystem entry point.
- `default_worker_threads()` (private) — `max(2, available_parallelism()/2)`.

**`scheduler.rs` — run store & state machine** (all `pub(crate)`)
- `Store` (Clone, `Arc<Mutex<HashMap<Uuid, RunRow>>>` + a `session_to_task` reverse map) — `insert`, `get`, `list`, `count_status`, `ring_for`, `observe_status`, `is_terminal`, `terminal_notify`, `cancel_gate`, and the session-correlation trio `register_session` / `task_for_session` / `forget_session`.
- `RunRow`, `CancelGate` (token-backed cooperative cancel with first-caller-wins reason), `is_terminal(&RunStatus)`.

**`republisher.rs` — pure translation helpers**
- `extract_session_id(&Value)`, `translate_bus_event(topic, payload, task_id) -> Option<AiEvent>`, `republish_topic(inner_topic)`, plus topic constants `TOPIC_STREAM_CHUNK` (`com.nexus.ai.stream_chunk`) and `TOPIC_ROUND_PROPOSED` (`com.nexus.agent.round_proposed`).

**`core_plugin.rs` — the CorePlugin**
- `AiRuntimeCorePlugin` (`CorePlugin` impl: async dispatch only, `wire_context`, `on_stop`), `wire_pool_for_tests`.
- Handler-id constants `HANDLER_SUBMIT=1` … `HANDLER_WAIT_FOR=9` and `IPC_HANDLERS: &[(&str, u32)]` (the SD-06 single-source-of-truth list bootstrap registers).

## IPC handlers

Nine registered handlers (id ⇄ command from `core_plugin::IPC_HANDLERS`). All dispatch is **async-only** — the sync `dispatch` path returns `HandlerIsAsyncOnly`.

| id | command | status | behaviour |
|---:|---------|--------|-----------|
| 1 | `submit` | wired | Allocates `task_id`, inserts the run, emits `Submitted`, injects/registers a `session_id` for `Session` tasks, spawns the worker on the pool, returns `{ task_id }`. |
| 2 | `cancel` | wired (Phase 5) | Flips the run's `CancelGate`; idempotent (`{ cancelled: bool }`); errors if the run is already terminal or unknown. |
| 3 | `pause` | unsupported | Returns a typed "not supported — a Session is a single ipc_call with no resumable midpoint" error. |
| 4 | `resume` | unsupported | Same as pause. |
| 5 | `get` | wired | Returns the full `AgentRun` (incl. event ring) or a "not found" error. |
| 6 | `list` | wired | Returns `{ runs: [AgentRunSummary] }`, filtered by status/since, capped by limit, sorted newest-first. |
| 7 | `events` | wired | Returns `{ events: [...] }` from the run's replay ring (`since_seq` optional). |
| 8 | `pool_stats` | wired | `{ workers, queued, running, max }` (live counts from the store, dimensions from the pool). |
| 9 | `wait_for` | wired (Phase 2) | Blocks the IPC reply until the run reaches terminal status or `timeout_ms` elapses; reply carries `{ run, timed_out }`. |

Bootstrap registers these with `with_v1_aliases(...)`, so each also has a `v1.<command>` alias.

## Capabilities

The plugin's own `KernelPluginContext` is granted (`nexus_bootstrap::ai_runtime_capabilities()`): `IpcCall` (to dispatch `session_run` / `stream_chat` / workflow steps — the runtime impersonates the caller's caps; per-call snapshotting is a Phase-2 follow-up), `AiChat` (because `session_run` is gated on it per ADR 0022/0024), and `EventsPublish` (to emit `com.nexus.ai.runtime.*` topics).

Callers reaching the runtime are gated by dedicated capabilities defined in `nexus-kernel`/`nexus-types` and granted in bootstrap: `Capability::AiRuntimeSubmit` and `Capability::AiRuntimeObserve` (granted to `workflow` and `agent` contexts so they can `submit` + `wait_for`). There is no `ai.runtime.control` capability granted to those callers, so they cannot `cancel`/`pause`/`resume`. The exact per-handler capability-to-handler-id mapping is owned by bootstrap's manifest/cap layer, not by this crate.

## Settings / Config

No `.forge/*.toml` file and no `Config` struct. Tunables are compile-time constants:

- Worker thread count — `default_worker_threads()` = `max(2, available_parallelism()/2)` (overridable only via `WorkerPool::start(Some(n))`, used in tests). Surfaced at runtime via `pool_stats.workers` / `.max`.
- `PER_RUN_EVENT_BUFFER_CAP` = 256 — per-run replay ring cap.
- `SESSION_RUN_TIMEOUT` = 2h — per-call IPC timeout for `session_run`.
- `WORKFLOW_STEP_TIMEOUT` = 5min — mirrors `nexus_workflow::DEFAULT_STEP_TIMEOUT` for `WorkflowAiStep`.
- `SESSION_CORRELATION_GRACE` = 500ms — hold on the `session_id → task_id` entry past the terminal event so late inner bus events still translate.

## Events

**Published** — all under `BUS_TOPIC_PREFIX` (`com.nexus.ai.runtime.`), one topic per `AiEvent` variant suffix: `submitted`, `started`, `token_chunk`, `tool_called`, `tool_result`, `round_proposed`, `round_decided`, `paused`, `resumed`, `cancelled`, `finished`, `failed`. Emitted by `record_and_publish`, which also pushes to the run's ring and advances run status via `observe_status`.

**Subscribed** — the republisher loop (spawned in `wire_context`) subscribes with two `EventFilter::CustomExact` filters: `com.nexus.ai.stream_chunk` and `com.nexus.agent.round_proposed`. For each, it extracts `session_id`, correlates to a `task_id` (skipping sessions not owned by the runtime), translates to `TokenChunk` / `RoundProposed`, and republishes through the same record-and-publish path. `stream_start` / `stream_done` are intentionally not translated (covered by the runtime's own `Finished`/`Failed`).

## Internals & notable implementation details

- **Job flow (Session):** `submit` → allocate `task_id` → `Store::insert` (creates `RunRow` with ring, `Notify`, `CancelGate`) → emit `Submitted` → `inject_session_id` (fresh uuid unless caller pinned one; registers correlation) → spawn worker on the pool. Worker: pre-check cancel gate (cancel-before-start → emit `Cancelled`, forget session, bail) → emit `Started` → `tokio::select!` (biased) racing `cancel.cancelled()` against the inner `ipc_call` → emit terminal `Cancelled` / `Finished` / `Failed` → sleep `SESSION_CORRELATION_GRACE` → `forget_session`.

- **Worker pool concurrency:** the pool is a separate multi-thread tokio `Runtime` (named threads `nexus-ai-worker-N`) owned by the plugin and built lazily in `wire_context` (so type-only unit tests don't pay for it). Its handle is published to a process-wide `OnceLock` (`SHARED_POOL_HANDLE`) so `nexus-ai`'s indexing daemon can reuse it; the daemon falls back to its own runtime if the handle isn't installed yet. The republisher loop is also spawned onto this pool.

- **Status machine:** the store derives `RunStatus` purely from `AiEvent::implied_status()` in `observe_status` — `Started` sets `started_at`, terminal events set `finished_at` and fire the per-run `Notify` (after dropping the map lock, to avoid deadlock on `wait_for` re-entry).

- **`wait_for` race-freedom:** a documented three-step pattern — check terminal status, then build the `notified()` future *before* re-checking status, then await — so a transition between steps can't be missed (lost-wakeup avoidance).

- **Cancellation:** `CancelGate` wraps `tokio_util::sync::CancellationToken` (level-triggered, so the select pre/post-check is no longer load-bearing) plus an `AtomicBool` first-caller flag and a reason `Mutex`. The worker's cancel arm is `biased` so a same-tick cancel wins against the inner call's reply. Cancel is idempotent; cancelling a terminal run errors.

- **Backpressure:** none beyond worker-thread saturation — Phase 1 spawns one task per submit; the queue is implicitly the pool's scheduler. `queued`/`running` are reported by counting store rows, not a bounded channel.

- **Correlation grace window:** because the kernel bus is async broadcast delivery, an inner `round_*` / `stream_*` event published just before `session_run` returns may still be in the republisher's channel when the worker emits the terminal event. Holding the correlation 500ms past terminal keeps those late events translatable.

- **Reserved surface:** `AiStream`, `ToolCalled`/`ToolResult`, `Paused`/`Resumed`, retry (`retriable` always `false`), per-priority pools, and persistence are all stubbed with stable wire shapes so later phases don't break the contract. `caller_plugin_id` is currently always `com.nexus.unknown` because the kernel context doesn't yet expose the caller id (Phase-2 follow-up).

## Tests

- **Inline unit tests** (in each module):
  - `lib.rs` — event-ring monotonic seq + cap behaviour, `snapshot_after` filtering, `AgentTaskKind::label`.
  - `events.rs` — every variant maps under the runtime prefix; `implied_status` lifecycle coverage.
  - `pool.rs` — shared-handle install/idempotence, ≥2 workers default, explicit worker count, handle can `block_on`.
  - `scheduler.rs` — insert/get round-trip, status transitions (started→completed, failed), list filters/limit, shared ring handle, terminal notifier identity + wakeup, `is_terminal`, session register/lookup/forget, `count_status`.
  - `core_plugin.rs` — async-only dispatch error, empty `list`, unknown-id errors for `get`/`wait_for`/`cancel`, ctx/pool-unwired errors, `pause`/`resume` unsupported, `wait_for` (immediate terminal / blocks-until-finish / timeout), `inject_session_id` (allocate / honour caller / skip non-Session), cancel flow (unknown / first-signal + idempotent / post-terminal error), unknown-handler, invalid-args, `pool_stats` counts.
- **Integration:** `crates/nexus-bootstrap/tests/ai_runtime_ipc.rs` (8 tests) drives the plugin end-to-end through `build_cli_runtime` + `ipc_call` — empty `list`, `pool_stats` ≥2 workers, unknown `get`, `pause`/`resume` unsupported, and the submit/cancel/wait_for flow.
- There is **no `crates/nexus-ai-runtime/tests/` directory**; all crate-level tests are inline `#[cfg(test)]` modules, with the IPC integration suite living in `nexus-bootstrap`.
