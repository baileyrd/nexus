# com.nexus.ai.runtime

- **Path:** `crates/nexus-ai-runtime/`
- **Tier:** Core Rust
- **Bootstrap order:** 9

## Architecture

- Entry point: `crates/nexus-ai-runtime/src/core_plugin.rs` — `AiRuntimeCorePlugin`. Bootstrap registration: `crates/nexus-bootstrap/src/plugins/ai_runtime.rs`. Lifecycle: `LifecycleFlags::NONE` — the plugin does its work in `wire_context` (start worker pool + spawn bus republisher loop) and `on_stop` (drop the captured context).
- Key modules:
  - `pool.rs` — `WorkerPool` builds a dedicated multi-thread tokio runtime; `publish_shared_handle()` exposes the handle to `nexus-ai::indexing_daemon` so it doesn't build a second runtime (BL-134 Phase 4).
  - `scheduler.rs` — in-memory `Store` of `AgentRun` rows + per-run bounded `EventRing` (cap 256, `PER_RUN_EVENT_BUFFER_CAP`), plus session-id ↔ task-id correlation map and per-run cancel gates / terminal notifications.
  - `republisher.rs` — bus subscriber translating `com.nexus.ai.stream_chunk` and `com.nexus.agent.round_proposed` to typed `AiEvent::{TokenChunk, RoundProposed}` envelopes on `com.nexus.ai.runtime.*`.
  - `events.rs` — typed `AiEvent` variants (`Submitted`, `Started`, `TokenChunk`, `RoundProposed`, `Finished`, `Failed`, `Cancelled`) with one bus topic per variant via `topic_for`.
- Persistence: **in-memory only**. The runtime carries no on-disk state — runs / events / pool stats reset on every process restart.
- Settings owned: none. The pool size is hardcoded inside `WorkerPool::start(None)`; per-call timeouts are `core_plugin.rs:79` (`SESSION_RUN_TIMEOUT = 2h`) and `core_plugin.rs:85` (`WORKFLOW_STEP_TIMEOUT = 5m`).
- External dependencies: `tokio` (dedicated runtime), `uuid`, `chrono`.

## Surface

9 IPC handlers (full table at `crates/nexus-ai-runtime/src/core_plugin.rs:57`):

`submit`, `cancel`, `pause` (reserved, returns unsupported), `resume` (reserved, returns unsupported), `get`, `list`, `events`, `pool_stats`, `wait_for`.

Bus topics published: every variant of `AiEvent` is republished under the `com.nexus.ai.runtime.*` prefix (see `BUS_TOPIC_PREFIX` constant).

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No — opening, browsing, editing, searching, and committing all complete without the AI runtime ever booking work. It exists to schedule and observe AI / agent / workflow tasks; remove AI and you remove its only producers.
- **Depended on by:** `com.nexus.ai` (Cargo dep on `nexus-ai-runtime` for the shared tokio handle), `nexus-workflow` (async steps dispatch via `WorkflowAiStep` tasks), shell-nexus `agent` panel for status / event replay, MCP server `agent_session_submit` family.
- **Depends on:** `com.nexus.agent` (the `submit` handler dispatches into `com.nexus.agent::session_run`); transitively `com.nexus.ai` once anything actually runs.
- **What breaks if removed:** no centralised task scheduler / cancellation surface / per-task event replay buffer. Agent runs would have to be driven inline by callers (the model that existed before BL-134). The shell agent panel's live event replay goes blank. Workflow async-step dispatch falls back to inline.

## Notes

- Phase 5 cancellation is wired (cancel-gate is `biased` against the inner ipc_call's reply); `pause`/`resume` remain reserved because a `session_run` ipc_call has no resumable midpoint.
- `wire_context` spawns the republisher loop onto the runtime's own pool when available, otherwise the ambient tokio runtime. Subscriber is process-lifetime — it dies at process exit.
- `inject_session_id` honours a caller-supplied `session_id` in the args (caller wins) so a delegate fan-out can thread a specific id through.
- `publish_shared_handle()` ordering matters — bootstrap must register `ai_runtime` before `ai`, which it does (orders 9 then 8 here is wrong; the bootstrap order in `core.md` is 9, after `ai` — confirmed: `nexus-ai`'s `wire_context` already tolerates the handle not being installed yet and falls back to its own runtime).
