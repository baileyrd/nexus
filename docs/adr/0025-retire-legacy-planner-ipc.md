# ADR 0025: Retire Legacy Planner IPC

**Date:** 2026-05-05
**Status:** Accepted (Phase 1). Phase 2 deletion deferred to a
follow-up PR per §"Phase 2".
**Supersedes-section:** ADR 0024 §"Open follow-ups" (legacy
planner retirement)

## Context

[ADR 0024] landed an agent session model that supersedes the
plan-then-execute split. After Phase 2a + 2b, `com.nexus.agent`
exposes 17 handlers; six are vestigial:

[ADR 0024]: 0024-agent-session-tool-loop.md

| id | command       | replaced by                          |
|----|---------------|---------------------------------------|
| 2  | `run`         | `session_run` with `auto_approve: true` (headless) or `auto_approve: false` (interactive) |
| 3  | `run_plan`    | no direct replacement — preset-plan replay doesn't fit the session model |
| 4  | `execute_step`| no direct replacement — see `run_plan` |
| 9  | `delegate`    | session_run with archetype id, scratch via session transcript |
| 10 | `parallel`    | future: caller fans out N session_runs concurrently |
| 11 | `pipeline`    | future: caller chains session_runs and feeds prior session.outcome forward |
| 12 | `trace_get`   | superseded by `session_list` + `session_get` |

Caller audit (`grep -rn 'com\.nexus\.agent.*"<cmd>"' shell crates`):

- **CLI** (`crates/nexus-cli/src/commands/agent.rs`) — actively
  drives `plan` (handler 1), `run` (2), `run_plan` (3) via
  `nexus agent plan|run|run-plan`. The only external consumer of
  the legacy surface today.
- **Shell** — no callers.
- **Workflow / Skills / etc.** — no callers.
- **Tests** — only the agent crate's own tests; no cross-crate
  use.

The BL-027 orchestrator handlers (`delegate` / `parallel` /
`pipeline` / `trace_get`) had a manifest gap until ADR-0024
Phase 2a inadvertently registered them. They have **never been
called by name** through IPC — the type machinery exists only
inside `nexus-agent` and the `AgentOrchestrator` test suite.

## Decision

Two-phase retirement, mirroring how ADR 0022 handled its rollout:

### Phase 1 (this ADR)

Mark the six handlers as deprecated **without deleting them**.
Migrate the CLI:

- **`nexus agent run`** repoints at `com.nexus.agent::session_run`
  with `auto_approve: true`. Output renders the
  `AgentSession` transcript instead of the legacy `Observation`.
- **`nexus agent run-plan`** is removed from the CLI surface —
  no session-model equivalent exists, and the on-disk plan files
  it consumed were always ad-hoc (no documented producer beyond
  `nexus agent plan > plan.json`). Rationale documented in the
  CLI help.
- **`nexus agent plan`** stays — the "one-shot tool-call
  proposal" use case is still meaningful (review without
  executing), and `LlmAgent::plan` driving `propose_tool_calls`
  is the cheapest way to provide it. No CLI change.

The IPC handler IDs stay numbered and reachable so any
out-of-tree caller (community plugin, MCP server, …) keeps
working through the deprecation window. Each deprecated handler
gains a `tracing::warn!` on dispatch with a one-time-per-session
flag (so the log doesn't drown the user) and a doc comment
pointing at the replacement.

### Phase 2 (~one release after Phase 1 lands)

Delete:

- **IPC handlers:** `run`, `run_plan`, `execute_step`,
  `delegate`, `parallel`, `pipeline`, `trace_get`. Drop their
  manifest entries and dispatch arms. Strict
  [`#[serde(deny_unknown_fields)]`](../../crates/nexus-agent/src/lib.rs)
  on the legacy `*Args` types means a caller still sending the
  old shape gets a clear `IpcError::CommandNotFound` rather than
  silent passthrough.
- **Library types tied exclusively to the legacy surface:**
  `PlanExecutor`, `Observation`, `StepResult`, `StepStatus`,
  `AutoApprove`, `StepPolicy` (replaced by `SessionPolicy`),
  `AgentOrchestrator`, `TraceEntry`, archetype `build_archetype`
  if no caller remains.
- **Orphaned executor module** (`crates/nexus-agent/src/executor.rs`)
  and orchestrator module
  (`crates/nexus-agent/src/orchestrator.rs`).

Keep (deliberately):

- **`plan`** — drives `LlmAgent::plan` for one-shot proposals.
- **`history_list` / `history_get` / `history_delete`** — read /
  delete pre-Phase-2a plan-history JSON under
  `<forge>/.forge/agent/history/`. Existing files remain readable
  through this surface; new sessions write under
  `<forge>/.forge/agent/sessions/` instead. A future ADR can
  retire `history_*` once a migration tool ships, but we keep
  user data accessible by default.
- **`list_archetypes`** — unrelated to the planner shape; reads
  compile-time constants.
- **`Plan` / `Step` / `ToolCall`** wire types — still referenced
  by `plan`'s reply shape and by the session-side `ToolCallRecord`.
  `LlmAgent` keeps using them.

### CLI surface after Phase 1

```
nexus agent plan       <goal> [--archetype …]   # one-shot proposal
nexus agent run        <goal> [--archetype …]   # session_run, auto-approve
nexus agent session    list | get <id> | delete <id>   # transcript surface
```

`nexus agent run-plan` and any "execute step at index" surface
are gone. Headless callers that need to replay a fixed sequence
of tool calls can build it as a `Vec<ToolCall>` and dispatch
each via `ipc_call` directly — that's what `run_plan` did
internally anyway.

## Alternatives considered

1. **One-shot deletion (no deprecation window).** Simpler, but
   breaks any out-of-tree caller without warning. The
   `tracing::warn!` deprecation gives a paper trail and a clean
   "we told you so" before the actual removal.
2. **Keep `delegate` / `parallel` / `pipeline` for use as
   session-level primitives.** Tempting — pipeline composition
   is a real future need. But the existing implementations
   produce `Observation`s with `StepResult`s, which is the data
   shape we're retiring. Re-implementing them on top of
   `AgentSession` is a fresh feature, not a renaming exercise;
   if we need them later they'll get their own ADR with
   real-world callers driving the design. Leaving them in place
   "just in case" forces us to keep the legacy executor alive.
3. **Retire `plan` too.** `session_run` with the model emitting
   tool calls and the policy denying every round (`Partial` with
   all `approve: false`) gives you a transcript of what the
   model would have done. That's much heavier than a single
   `propose_tool_calls` round-trip and produces a different
   shape. Retain `plan` for the lighter use case.
4. **Retire `history_*`.** No on-disk migration tool exists; old
   forges have plan-history JSON under `.forge/agent/history/`
   that users may still want to read. Keeping the read surface
   is cheap (~30 lines).

## Consequences

### Wins
- `nexus-agent` shrinks by roughly the size of `executor.rs` +
  `orchestrator.rs` + the legacy `plan` / `run` / `run_plan` /
  `execute_step` / BL-027 dispatch arms — order of 1500 lines.
- One mental model for "the agent did some work": a session
  transcript. No second data shape (`Observation`) to keep
  consistent.
- `Plan` / `Step` / `ToolCall` keep their PRD-15 semantics but
  stop having ambiguous dual roles ("input to executor" vs
  "model proposal").

### Costs / disruption
- `nexus agent run-plan` users (if any out-of-tree) lose their
  workflow. Phase 1's deprecation log gives them a release
  window to migrate.
- `tracing::warn!` chatter during the deprecation window will
  show up in production logs. Mitigated by the once-per-session
  flag.
- Phase 2 is an IPC-shape change (handler removal). Callers
  using the bare command name (`com.nexus.agent::run`) get
  `CommandNotFound` after Phase 2 lands. Callers using the
  versioned form (`run.v1`) — none today — get the same.

### Migration table for community-plugin authors

| Old call                                | New call                                           |
|-----------------------------------------|----------------------------------------------------|
| `com.nexus.agent::run`                  | `com.nexus.agent::session_run` (auto_approve: true) |
| `com.nexus.agent::run_plan`             | manual `Vec<ToolCall>` + `ipc_call` per call        |
| `com.nexus.agent::execute_step`         | manual single `ipc_call`                           |
| `com.nexus.agent::delegate`             | `session_run` with `archetype` arg                  |
| `com.nexus.agent::parallel`             | caller fans out N `session_run`s                    |
| `com.nexus.agent::pipeline`             | caller chains `session_run` outputs manually        |
| `com.nexus.agent::trace_get`            | `session_list` then `session_get`                   |

## Open follow-ups

- Phase 2 retirement PR. Targets: ~1 release after Phase 1
  lands. Scope: handler/dispatch deletion, library type
  deletion, dependent test cleanup.
- Optional follow-up ADR for `history_*` retirement once a
  migration tool ships.
- If `parallel` / `pipeline` come back as session-level
  primitives, they get their own ADR — design driven by real
  callers, not by preserving legacy shape.
