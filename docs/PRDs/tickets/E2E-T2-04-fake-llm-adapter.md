# E2E-T2-04 — Fake-LLM adapter for chat + agent E2E

**Status**: open
**Opened**: 2026-04-23
**Context**: follow-up from Tier-2 testability pass (shell/ commit `8a4ce44`, PR #1).
**Unblocks**: 2 `it.skip` blocks:
- [shell/e2e/specs/tier2/chat.spec.ts](../../../shell/e2e/specs/tier2/chat.spec.ts) — "streaming-in-progress indicator appears during a send".
- [shell/e2e/specs/tier2/agent.spec.ts](../../../shell/e2e/specs/tier2/agent.spec.ts) — "history ordering is newest-first after two runs".

## Problem

Both specs depend on an LLM round-trip. In CI the real provider is unreachable, so:
- chat's pending row (`[data-streaming="true"]`) is torn down before WDIO observes it;
- agent history can't accrue two distinguishable runs.

A fake, deterministic responder behind a runtime flag fixes both.

## Scope

### Adapter

- New test-only adapter registered behind an env flag (e.g. `NEXUS_FAKE_LLM=1`).
- Replaces the `com.nexus.ai` provider client when the flag is set.
- Behaviour:
  - **Chat (`ask`)**: stream with a configurable delay (default ~300 ms) between start and completion so WDIO can observe the pending indicator. Emit a deterministic canned response and empty sources array.
  - **Agent (`plan` / `run`)**: return a plan with ≥1 step and a deterministic-but-time-varying plan_id so two consecutive runs persist as separate history entries. Observation emits `success: true`.
- Must not reach the network.

### CI wiring

- Set `NEXUS_FAKE_LLM=1` in [shell/e2e/wdio.conf.ts](../../../shell/e2e/wdio.conf.ts) env so every Tier-2 run picks it up.
- Gate behind the flag only — dev runs still hit the real provider.

### Specs

- Un-skip "streaming-in-progress indicator appears during a send" in `chat.spec.ts`. Assert `[data-streaming="true"]` is visible after send, then disappears after the fake completes.
- Un-skip "history ordering is newest-first after two runs" in `agent.spec.ts`. Drive two goals through `AgentPage.planAndRun`, refresh history, assert the newer `plan_id` sorts first.

## Non-goals

- Reaching the production LLM path in CI — the fake adapter is explicitly the approach.
- Replaying real transcripts — canned responses are sufficient for Tier-2.

## Selectors (already landed)

| Element | Selector |
| --- | --- |
| Chat streaming indicator | `[data-streaming="true"]`, `role="status"` |
| Chat RAG toggle | `button[aria-label="RAG mode"]` with `aria-pressed` |
| Agent empty history | `[aria-label="No history"]` |
