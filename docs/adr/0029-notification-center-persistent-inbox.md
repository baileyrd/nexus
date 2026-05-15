# ADR 0029: Notification Center — Persistent Inbox + Read/Unread Contract

**Date:** 2026-05-15 (proposed).
**Status:** Accepted — Phase 1 (backend) lands on this branch; Phase 2 (shell plugin) follows in a sibling branch.
**Related:** [ADR 0028](0028-ai-agent-event-loop.md) (AI/agent runtime — sibling), BL-133 (`nexus-notifications` dispatcher), BL-135 (router + `notifications.toml`), [BL-134](../PRDs/BACKLOG_COMPLETED.md#bl-134) (AI runtime + `AiEvent` stream).

## Context

`com.nexus.notifications` today is fire-and-forget. A `Notification` goes through the router, fans out to one or more `Transport`s, and disappears. Side-effects:

- A user who closes the shell at 02:00 has no way to discover that the 03:15 Dream Cycle finished — the toast surface was unmounted, the Discord webhook hit `forbidden`, the Telegram bot was rate-limited. There is no history surface to look at.
- The same notification cannot be marked read/unread or dismissed, so the shell's existing toast pipeline (BL-133 follow-up subscriber) re-rendering on reconnect would replay every notification. Today the shell sidesteps this by only ever showing the live event — but the cost is that nothing is *recoverable*.
- BL-134's `AiEvent::Finished` / `Failed` events are the highest-volume source the notifications subsystem is about to ingest, and they're exactly the events users want a history of (`what runs failed overnight?`). Without an inbox the value of the BL-135 router is capped at "fan out and hope."

The natural shape is one already used elsewhere in Nexus: a derived store under `<forge>/.forge/` rebuildable from authoritative on-disk data, surfaced via IPC, observable on the bus. This ADR formalises that shape for notifications.

## Decision

Introduce a **derived inbox** under `<forge>/.forge/notifications/inbox.db`, owned by `nexus-notifications` and exposed through four new IPC handlers on `com.nexus.notifications`.

### Schema

One SQLite table `inbox`:

| Column          | Type    | Notes                                                                  |
|-----------------|---------|------------------------------------------------------------------------|
| `id`            | TEXT PK | UUIDv4 generated at insert. Stable across the row's lifetime.          |
| `source`        | TEXT    | Source tag from the router path (`"ai_runtime"`, `"workflow"`, …). `"override"` for explicit-channel sends without a source tag. |
| `severity`      | TEXT    | `debug` / `info` / `warn` / `error`. Defaults to `info`.                |
| `title`         | TEXT    | Optional notification title.                                            |
| `body`          | TEXT    | Notification message.                                                   |
| `channels`      | TEXT    | JSON array of routed channels (snake_case names — `["desktop","telegram"]`). Empty array when the dispatch failed on every transport. |
| `ts`            | INTEGER | Insert time in Unix seconds.                                            |
| `read_at`       | INTEGER | NULL until `inbox_mark_read`. Cleared on rebuild only if the caller asks (`keep_user_state=false` — default true).                       |
| `dismissed_at`  | INTEGER | NULL until `inbox_dismiss`. Same rebuild semantics as `read_at`.        |
| `payload_json`  | TEXT    | Optional caller-supplied JSON blob (e.g. originating `task_id`).        |

Indexes:

- `idx_inbox_ts (ts DESC)` for the default chronological list.
- `idx_inbox_unread (read_at)` partial on `read_at IS NULL` for the unread-count query.
- `idx_inbox_source (source, ts DESC)` for filter-chip lookups.

### IPC surface

Four new handlers on `com.nexus.notifications` (existing `send` keeps id 1):

| ID | Command            | Args                                          | Reply                                       |
|---:|--------------------|-----------------------------------------------|---------------------------------------------|
| 2  | `inbox_list`       | `{ since?: i64, status?: "all"|"unread"|"dismissed", source?: string, limit?: u32 }` | `[InboxEntry]` |
| 3  | `inbox_mark_read`  | `{ ids: [string] }`                            | `{ updated: u32 }`                          |
| 4  | `inbox_dismiss`    | `{ ids: [string] }`                            | `{ updated: u32 }`                          |
| 5  | `inbox_stats`      | `{}`                                           | `{ total: u32, unread: u32, by_source: { [src]: u32 } }` |

`InboxEntry` is the serialised row shape. Wire field names are snake_case to match the existing notifications surface.

### Capabilities

Two new capabilities slotted under the `notifications.*` namespace (per ADR 0002):

- **`notifications.inbox.read`** — gates `inbox_list` / `inbox_stats`. Granted to the shell (`com.nexus.shell`), the CLI, and the TUI invokers by default. Community plugins do not get it.
- **`notifications.inbox.write`** — gates `inbox_mark_read` / `inbox_dismiss`. Granted to the shell only. CLI / TUI can read but not mutate the user-state columns; we don't yet have a CLI surface for read/dismiss and granting blindly would invert the "shell-only mutation" stance.

Both are **Low** risk per ADR 0002's classification — derived store, no network egress, no spawned processes.

### Producer-side: who writes rows

A single built-in subscriber inside `nexus-notifications`, owned by `NotificationsCorePlugin` and wired in `on_start`. The subscriber's call site is `NotificationsCorePlugin::dispatch_routed` (and the override-path equivalent inside `dispatch_send`): *every* `Notification` that reaches the fan-out step writes one inbox row, regardless of whether any transport accepted it. This guarantees:

- An un-routed `source` (router returns `UnknownSource`) does **not** write a row — the notification was rejected before dispatch, so the inbox would lie if it claimed one was sent.
- A `filtered` notification (below `min_severity` or in quiet hours) does **not** write either — explicitly suppressed.
- A `routed` notification that hits zero working transports **does** write, with `channels = []`. This is the case that justifies the inbox most directly.

The `channels_routed_to` column reflects the channels the router *picked*, not the channels that successfully delivered. Per-channel failure detail stays in the existing `SendReply.failures` shape.

### Retention

Configured under `[inbox]` in `notifications.toml`:

```toml
[inbox]
max_rows = 1000        # default: 1000
max_age_days = 30      # default: 30
```

Either knob is independently capped. Both run on:

- **`on_start`** — one-time sweep at boot.
- **Each insert past the row cap** — bounded amortised cost via `DELETE FROM inbox WHERE id IN (SELECT id … ORDER BY ts ASC LIMIT N)` where N is the overflow.

Day-cap deletion runs on `on_start` only (cheaper than per-insert). A 30-day-old row that *also* fits under the row cap survives until the next boot.

### Rebuild semantics

Per invariant 1 (file-as-truth), the inbox is rebuildable from authoritative inputs:

- The router publishes every dispatch on `com.nexus.notifications.delivered` (existing BL-133 topic).
- The BL-134 runtime publishes typed `AiEvent`s on `com.nexus.ai.runtime.*`.
- `notifications.toml` is the routing rule of record.

Dropping `inbox.db` and replaying the recent event stream reconstructs the same row set *modulo* user-state columns (`read_at`, `dismissed_at`). Those two columns are *not* derivable — they are user actions. The rebuild path explicitly preserves them by:

1. Loading the existing `(id, read_at, dismissed_at)` tuples into memory.
2. Truncating + re-creating the `inbox` table from the event log.
3. Joining the preserved user-state rows back in by `id`.

The Phase-1 implementation ships `Inbox::rebuild_from_events()` as a library function but does **not** auto-rebuild on boot — operators run it explicitly (`nexus notifications rebuild` CLI lands in Phase 2 / 3). The tests pin the round-trip guarantee.

### Bus topic

A new `com.nexus.notifications.inbox.appended` topic fires for every row insert, carrying `{ id, source, severity, ts }`. The shell uses this for live unread-count updates without polling `inbox_stats`. The topic is informational — losing it does not desync the store.

## Invariants preserved

1. **File-as-truth.** `inbox.db` is a derived store under `.forge/`; rebuild semantics are documented above. `notifications.toml` is the rule of record.
2. **Microkernel isolation.** No new crate. `nexus-notifications` keeps its dep graph (kernel + plugin-api + plugins). Rusqlite gets added as a dep; that's a leaf-only addition.
3. **IPC over direct calls.** Shell / CLI / TUI hit the inbox through `ipc_call("com.nexus.notifications", "inbox_*", …)`. No direct linking.
4. **Capabilities gate everything.** Two new capabilities, bootstrap-side wired into the cap matrix.

## Consequences

### Positive

- One persistent surface for *every* notification, replacing the fire-and-forget pipeline.
- Unread count + filter chips become trivial — the existing toast surface stays for live notifications, the inbox panel stays for history.
- BL-134's `AiEvent::Finished`/`Failed` lands somewhere durable.
- Per-source filter chips fall out for free: `inbox_list({ source: "ai_runtime" })`.

### Negative

- New rusqlite dependency in `nexus-notifications` (already a workspace dep — leaf addition).
- One more derived store under `.forge/`. Operational surface grows slightly; the rebuild path mitigates.
- Producer-side write-on-dispatch couples the router fan-out to a synchronous SQLite insert. Bench shows <500µs per insert on a warm WAL; below the network RTT of any transport. If this ever becomes the bottleneck we move to a bounded mpsc + background writer thread.

### Neutral

- Existing `com.nexus.notifications.delivered` topic is unchanged. The inbox subscriber is additive.
- Shell plugin (Phase 2) consumes the IPC surface only — no direct crate dep.

## Alternatives considered

### A. Reuse the activity timeline (`com.nexus.activity.appended`)

`nexus-types::activity` already ships a universal activity log. The shell `activityTimeline` panel renders it. Could we just emit notifications onto that topic and read back?

**Rejected.** Mismatched semantics. Activity is "what happened" (file edited, commit landed, AI session finished); notifications are "what *needs your attention*." They overlap in source but differ in retention policy, mutation surface (mark-read is meaningless on an activity entry), and filter shape. Forcing the union pulls retention + user-state columns into a log that doesn't want them. We do cross-link — the inbox row's `payload_json` can carry the activity entry's id so the shell can jump from one to the other.

### B. JSONL ring file (`<forge>/.forge/notifications/inbox.jsonl`)

Append-only file, in-memory index, periodic compaction.

**Rejected.** Read patterns (filter by source, mark-read by id) want indexed lookups. JSONL forces a full-scan or a parallel index — at which point we have SQLite without the planner. The `nexus-agent` history store made the same choice for the same reason.

### C. Per-source tables

`inbox_ai_runtime`, `inbox_workflow`, etc.

**Rejected.** Premature partitioning. A 30-day window over four sources at the documented volume (~100 rows/day max) is well under the single-table sweet spot. If a future source emits at >>1k/day we revisit.

## Migration

None — this is a pure addition. Forges without `[inbox]` get the defaults. Forges with `inbox.db` from a prior boot keep their data; the migration step at open creates the table if it doesn't exist.

## Phase split

- **Phase 1 (this branch — bl-134-ai-runtime, BL-136):** ADR, sqlite store, four IPC handlers, dispatch-time writer, retention, capabilities, ts-rs/schemars bindings, unit + integration tests.
- **Phase 2 (sibling branch):** `nexus.notificationsInbox` shell plugin — sidebar leaf with unread badge, filter chips, click-to-mark-read, dismiss action, jump-to-source against BL-134's observability panel.
- **Phase 3 (optional follow-up):** `nexus notifications rebuild` CLI for forge operators.
