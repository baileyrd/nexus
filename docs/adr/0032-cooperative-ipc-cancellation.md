# ADR 0032: Cooperative IPC Cancellation — Track A

**Date:** 2026-05-23
**Status:** Accepted (partial — Track A implemented, Track B planned)
**Related:** BL-134 (AI runtime), BL-113 (nexus-lsp), BL-081 (DAP), ADR 0021 (IPC handler versioning)

## Context

The Nexus kernel exposes a single IPC channel through which all service crates communicate. As of this writing, there are four major outbound protocol adapters (LSP, DAP, ACP) and one inbound (MCP) running over this channel. Long-running service calls — code intelligence lookups, agent planning loops, language server indexing — can hold IPC endpoints open for seconds or minutes.

The problem: when a user cancels an action (window close, command palette abort, explicit cancel), there was no way to propagate that cancellation downstream through the IPC stack. The cancelled caller would hang waiting for a response that would never come, blocked on an RPC that the callee could not be prompted to abort.

ADR 0021 established the IPC handler versioning convention but did not address the cancellation dimension at all.

## Decision

A **two-track cancellation model** across the IPC boundary:

### Track A — Cooperative cancellation (shipped via ADR 0032)

- **`CancellationToken`** — a lightweight, cloneable `Arc` carrying a `SharedCancel` flag and a set of `Subscriber`s. Any component in the IPC stack can hold a token, clone it, and pass it through nested calls. The token's `is_cancelled()` is checked at natural breakpoints.
- **`CancelGate`** — a channel-based gate in `nexus-ai-runtime` that blocks on a `tokio::sync::mpsc` when a cancellation is signalled. Provides a clean unblock point for long-running AI provider calls that poll the gate.
- **Channel back-pressure via `tokio::sync::mpsc` with `Sender::try_send()`** — when the receiver is overwhelmed, `try_send()` on a full channel returns `TrySendError::Full`, allowing the caller to drop the message and treat it as a soft cancellation. Applied to LSP/DAP/ACP senders.
- **Per-handler cancellation scope** in `nexus-bootstrap` — each IPC handler owns a `CancellationTokenSource`. The kernel's `cancel()` method calls `token.cancel()` on all live scopes.

### Track B — Enforced cancellation (planned, not yet implemented)

- **`cancel_on_drop` guard** — a `Drop`-based mechanism that tears down the entire call graph for a given IPC scope when the handle is dropped (window close, session end). Not yet implemented because it requires rewriting several handler entry-points to own scopes.
- **Timeout gates on all outbound IPC** — every `nexus_mcp` tool call, `nexus_lsp` request, and `nexus_dap` command gets a deadline. The kernel enforces the deadline; the callee must report success, failure, or timeout.

### Architecture

```
Caller (front-end) —[CancellationTokenSource::token()]—> Handler
    Handler —[CancellationToken.clone()]—> Service crate
        Service crate —[CancelGate / try_send / token.is_cancelled()]—> Provider
```

The cancellation token flows **top-down only**. There is no mechanism for a callee to signal "I would like to cancel this call" — that's outside the scope of Track A.

## Consequences

### Positive

- **Clean abort paths** exist today for IPC calls. User-initiated cancellations propagate to the AI runtime and back-pressure the channel without requiring the callee to be cancellation-aware.
- **Zero-cost for non-cancelled paths** — `CancellationToken` is an `Arc` with a `SharedAtomic` flag. Checking `is_cancelled()` is a single atomic load when not cancelled.
- **Composable** — tokens can be cloned, passed through any number of layers, and checked at the caller's discretion. Each service crate decides its own cancellation breakpoints.
- **No ABI changes** — cancellation flows through existing `IPCContext`, not through a new message type.

### Negative / costs

- **Cooperative, not enforced.** A callee that ignores `is_cancelled()` checks will run to completion. There is no hard deadline enforcement in Track A. Track B is needed for this.
- **No backward-compatibility path for old IPC handlers.** Handlers that don't accept or propagate `CancellationToken` cannot participate in cancellation. The handler versioning system (ADR 0021) handles this, but old code will simply not support cancellation.
- **`CancelGate` in `nexus-ai-runtime` is a crate-specific pattern.** Other services (LSP, DAP, MCP) use the generic token + channel back-pressure pattern. Future services must choose which mechanism to use.

## Alternatives considered

### A. Require all callees to check cancellation at every entry point

**Rejected.** This makes every handler a cancellation-aware participant — a large surface area for a low-frequency failure mode. We don't need guaranteed cancellation at every point; we need graceful degradation.

### B. Use a dedicated "cancel" IPC message type

**Rejected.** Requires every handler to listen for the cancel message, adds a new message direction, and introduces race conditions (cancel arrives after result). The existing `CancellationToken` model avoids these with zero new protocol overhead.

### C. Enforce hard deadlines on all IPC

**Deferred to Track B.** Hard deadlines are necessary but introduce complexity (timeout propagation, retry policies, deadline unrolling). Leave for Track B when the handler rewrite lands.

## Open follow-ups

1. **Track B implementation:** wrap all handler entry-points in scope-based guards with `cancel_on_drop` semantics and per-handler deadlines.
2. **Test:** add integration tests for the cancellation path through the IPC stack. The audit noted seven critical crates with no tests — cancellation in any of them is unverified.
3. **Monitor:** Track A is functional but not exhaustively tested. Add telemetry for cancellation events (count, avg_latency_to_cancel) in production deployments.
