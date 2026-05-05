# ADR 0024: Agent Session Tool-Loop (ADR-0023 Phase 2)

**Date:** 2026-05-05
**Status:** Accepted (Phase 2a). Phase 2b — bus-bridge approval
callback — tracked under §"Open follow-ups".
**Supersedes-section:** ADR 0023 §"Phase 2 — deferred follow-up ADR"

## Context

[ADR 0023] (G7-1b) unified the agent's planner on the AI tool
registry: `LlmAgent` calls `com.nexus.ai::propose_tool_calls`,
maps each provider tool-use block to a `Step`, and hands the
resulting `Plan` to `PlanExecutor` for approval-gated dispatch.

That migration preserved the agent's plan-then-execute split
because it was the smaller change. ADR 0023 §"Phase 2" flagged the
split as a load-bearing assumption that no longer matches how the
underlying provider works:

[ADR 0023]: 0023-unify-agent-on-ai-tool-registry.md

- **Plans aren't real anymore.** A single call to
  `propose_tool_calls` returns whatever tool calls the model
  decided to issue *for the first turn*. The model can't see future
  results. So a plan with N steps is either:
  - One first-turn batch the model intends to run before seeing any
    output (rare for non-trivial goals), or
  - The model's best-effort imagination of what the whole flow
    looks like, which is wrong as soon as a step's output suggests
    something different.
- **The user can't review what isn't planned.** When the model
  emits one tool call, executes it, *then* emits another, the agent
  today doesn't see the second call until it issues a fresh
  `plan(goal)` call. The natural multi-round flow gets sliced into
  an N-times-call-the-planner workflow, which the model has to
  learn to navigate.
- **`stream_chat` already does this right.** `mode=chat` runs the
  tool-dispatch loop until the model is done (up to
  `MAX_TOOL_ROUNDS = 8`). The only thing missing for the agent's
  use case is **per-call approval policy** between rounds.

## Decision

Replace the agent's `plan(goal) → execute(plan)` model with an
**agent session** model: one call drives a multi-round tool loop
where each round optionally pauses for user approval before
dispatching tools.

### Approval UX

A single round of the loop produces zero or more tool calls. The
session offers the **whole round's worth** of tool calls to the
caller-supplied `StepPolicy` at once, mirroring today's
`StepPolicy::allow(step)` interface but at round granularity:

```rust
trait SessionPolicy: Send + Sync {
    fn allow_round(&self, round: &ProposedRound) -> RoundDecision;
}

enum RoundDecision {
    /// Run every tool call in the round.
    ApproveAll,
    /// Run only the approved subset; deny the rest with a reason.
    /// Denied calls feed back to the model as ToolResult turns
    /// flagged `is_error: true` so the model can adjust on the
    /// next round.
    Partial(Vec<RoundDecisionEntry>),
    /// Stop the session entirely — no more tool calls dispatched,
    /// no more rounds requested.
    Abort(String),
}

struct RoundDecisionEntry {
    /// Provider-issued id from the corresponding ProposedToolCall.
    pub tool_use_id: String,
    /// Approve to run; Deny to feed an error back instead.
    pub decision: PolicyDecision,
}
```

This is strictly more expressive than today's per-step
`StepPolicy::allow` — the existing `AutoApprove` impl maps to
`SessionPolicy::allow_round → ApproveAll`. The ADR keeps
`StepPolicy` for back-compat with the pre-Phase-2 `PlanExecutor`
surface, which we don't delete in this phase (see §"Migration").

The policy receives enough metadata to render an approval prompt:
the tool name, args, the round's narration text, and the
session-so-far context. Concretely:

```rust
struct ProposedRound {
    /// Round index (1-based).
    pub round: u32,
    /// Narration text the model emitted alongside this round.
    pub text: String,
    /// Tool calls proposed in this round, ready for dispatch.
    pub tool_calls: Vec<ProposedToolCall>,
    /// Compact transcript of prior rounds for UX rendering.
    pub history: Vec<HistoryItem>,
}
```

### Persistence

Sessions persist as a turn-by-turn transcript JSON under
`<forge>/.forge/agent/sessions/<session_id>.json`:

```json
{
  "id": "uuid-v4",
  "goal": "summarise notes from this week",
  "archetype": "researcher",
  "started_at": "2026-05-05T18:21:00Z",
  "ended_at": "2026-05-05T18:23:11Z",
  "rounds": [
    {
      "round": 1,
      "text": "I'll start by listing this week's notes.",
      "proposals": [
        {
          "id": "toolu_…",
          "name": "search_forge",
          "tool_call": { /* full ToolCall */ },
          "decision": "approve",
          "result": { /* response or error */ },
          "is_error": false
        }
      ]
    },
    { "round": 2, … }
  ],
  "outcome": "complete" | "aborted" | "errored" | "max_rounds"
}
```

This shape is a strict superset of the existing `Observation`. The
old `Plan` shape doesn't survive — it was only meaningful for the
plan-then-execute model. Existing on-disk plan-history JSON stays
readable via the legacy `history_get` handler (preserved as a
read-only compatibility surface; no new entries written under that
shape after this ADR lands).

Session storage is reachable through new IPC handlers that mirror
the AI plugin's session surface:

- `com.nexus.agent::session_run` — kick off a session.
- `com.nexus.agent::session_get` — load one transcript by id.
- `com.nexus.agent::session_list` — enumerate.
- `com.nexus.agent::session_delete` — remove one.

`session_run` is the only one that mutates beyond storage; gating
it under `ai.chat` (the same as `propose_tool_calls`) keeps the cap
story consistent.

### Loop mechanics

```text
turns = [User { goal }]
for round in 1..=MAX_AGENT_ROUNDS {
    proposal = ai.propose_tool_calls(turns, system, tools_policy)
    if proposal.tool_calls is empty AND proposal.text is non-empty:
        record final-text round; break
    if proposal.tool_calls is empty AND proposal.text is empty:
        record empty round; break
    decision = policy.allow_round(proposal)
    match decision:
        Abort(reason): record + break
        ApproveAll | Partial: dispatch each approved call,
                              feed results back as ToolResult turns
}
```

`MAX_AGENT_ROUNDS` = 8 to mirror the AI plugin's tool-loop cap. A
session that hits the cap returns with `outcome: "max_rounds"`; the
caller can resume with a follow-up `session_run` if it wants more.

### Migration

- `LlmAgent::plan` and the `PlanExecutor` surface stay; deprecated
  but functional. No deletions in this ADR.
- New types: `AgentSession`, `ProposedRound`, `RoundDecision`,
  `SessionPolicy`. Live in `nexus-agent`.
- New `AgentOrchestrator::run_session(goal, policy)` is the
  preferred entry point. The existing `delegate` / `parallel` /
  `pipeline` surface (BL-027) keeps working and gets re-implemented
  on top of session-mode internally — they use a thin
  `AutoApproveAll` policy.
- Shell + CLI consumers migrate at their own pace; the legacy
  `plan` / `run` / `run_plan` IPC handlers stay for now.
- Tests: a new `session_e2e.rs` mirrors today's `orchestrator_e2e`
  but drives `session_run`. Existing tests keep passing during the
  migration window.

## Alternatives considered

1. **Keep plan-then-execute, drive multi-round via re-planning.**
   The model produces a fresh plan each time it needs more tools;
   the executor calls `plan()` again after every batch. Maps poorly
   to the underlying provider (each plan call is a fresh
   conversation; no `tool_use` ↔ `tool_result` linkage), and the
   model can't see prior tool results. Rejected.
2. **Whole-plan approval gate.** Model emits a complete N-step plan
   up front; user reviews everything; executor runs. Same problem
   as today: the model is guessing future results. Rejected.
3. **Per-call interactive approval (one approval per tool call,
   one tool call per "page").** Smoother for high-stakes flows but
   triples the prompt count for normal multi-tool rounds. Doesn't
   compose with `AutoApprove` use cases either. Rejected; the
   round-level policy can degrade to per-call by returning
   `Partial` with mixed decisions.
4. **Persist as a derived view of the AI plugin's chat sessions.**
   Tempting because the AI side already persists chat history; but
   the agent's session has different semantics (approval decisions,
   per-tool-call results) and gating it under `ai.session.*` would
   conflate two surfaces. Rejected.

## Consequences

### Wins
- One mechanism for tool-using LLM work — `stream_chat` chat mode,
  the agent's session, and the underlying tool registry are all the
  same loop with different approval policies and surface
  affordances.
- The model handles dynamic flow naturally — no more
  pre-planning fiction.
- Session transcripts are richer than `Observation` for both UI
  rendering and audit.

### Costs / disruption
- New IPC types (`AgentSession`, `ProposedRound`, `RoundDecision`)
  enter the contract — drift regen mandatory.
- Deprecated-but-not-deleted `plan` / `run` / etc. — slight surface
  pollution. A follow-up "remove legacy planner surface" ADR can
  clean this up once shell/CLI callers migrate.
- `StepPolicy` and `SessionPolicy` coexist for now. Round decision
  is the better primitive but converting all in-tree
  `StepPolicy` impls in one drop bloats this ADR's scope.
- Loop runs server-side (in the agent core plugin) but approval is
  caller-side. Need a callback path back to the caller's policy
  during the session — see §"Open question: callback path" below.

### Open question: callback path

`StepPolicy` is a Rust trait, not an IPC contract. Today
`PlanExecutor` consults it inside its own process. For a session
run via `com.nexus.agent::session_run`, the policy lives in the
shell or CLI — the agent core plugin can't call back across the
IPC boundary directly.

Two viable shapes:

- **A. Bus events + `decide_round` reply IPC.** The agent emits a
  `com.nexus.agent.round_proposed` event with the round's tool
  calls; the caller's UI prompts the user; the caller calls a new
  `com.nexus.agent::round_decide(session_id, decision)` to
  unblock. The agent's session loop awaits the decision before
  dispatching. Adds latency = approval round-trip; matches how the
  AI activity stream works.
- **B. Session = library construct.** Sessions only run inside the
  caller's process; `nexus-agent` exposes a `run_session(driver,
  policy)` library API and the IPC surface is just for headless
  / auto-approve sessions. `AutoApprove` becomes the only built-in
  policy reachable through IPC.

Option **A** is more honest — the user genuinely is part of the
loop on every approval. It also fits Phase-2's value proposition
(plans never had to span an IPC round-trip; sessions do).
**Recommend A**, with B available as an optimisation for
auto-approve flows that don't need callback latency.

## Open follow-ups
- **Phase 2b** — bus-bridge approval callback. Today (Phase 2a)
  the `session_run` IPC handler accepts `auto_approve: true` only;
  passing `false` returns "not yet implemented". Phase 2b lands
  the `com.nexus.agent.round_proposed` event + `round_decide` IPC
  + per-session oneshot wiring with a configurable timeout. Shell
  approval-prompt UX is a separate piece of work.
- A small ADR to delete the legacy `plan` / `run` / `run_plan` /
  `execute_step` IPC handlers once shell + CLI callers migrate.
- Token-budget integration — sessions can run long; the existing
  budget redactor (G1) covers RAG, but per-round messages don't
  carry the same budgeting story. Needs a separate look.
- Resume / branch — a session that hit `max_rounds` could be
  resumable. Not in scope here; the data shape supports it.
