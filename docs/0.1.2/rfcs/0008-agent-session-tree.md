# RFC 0008 — Phase 5.4: agent session tree (resume / branch / rewind / checkpoint)

- **Status:** Draft — design (immutable fork-nodes; non-destructive rewind)
- **Owner:** unassigned
- **Created:** 2026-06-17
- **Tracks:** [RFC 0005](0005-omp-agentic-loop-phase5.md) Phase 5.4; omp blueprint (session JSONL tree with branch/rewind)
- **Touches:** `crates/nexus-agent/` (`session.rs` run loop, `handlers/session.rs`, `core_plugin.rs` IPC), the `AgentSession` persistence shape, `crates/nexus-bootstrap/cap_matrix.toml`, `docs/0.1.2/ipc-handlers.md`; later `crates/nexus-cli` + `shell/` frontends
- **Related:** ADR 0024 (session model — parked resume/branch as "the data shape supports it"); [RFC 0007](0007-subagent-process-isolation.md)

---

## Summary

Phase 5.4 turns the agent's flat, single-pass sessions into a **session tree**:
**resume** a finished session with a follow-up message, **branch** from any past
round into a parallel line, **rewind** to an earlier round (non-destructively),
and **checkpoint** a round for later navigation. The result is a forest of
sessions linked parent → child, navigable and resumable.

## Where Nexus stands today

| Fact | Evidence |
|---|---|
| A session is `AgentSession { id, goal, archetype, started_at, ended_at, rounds, outcome, compactions }` | `session.rs:362` |
| Written **once, immutably** at completion to `.forge/agent/sessions/<id>.json` (`write_vault_file`) | `handlers/session.rs:219–243` |
| The loop's prompt each round is rebuilt **purely from `goal` + `rounds`** (`compose_followup_prompt_compressed`) — so a session's state *is* `goal + rounds` | `session.rs:698`, `:534–767` |
| `session_run/list/get/delete` exist (handlers 13–16); **no resume** | `core_plugin.rs:106–113` |
| Passing an existing `session_id` to `session_run` **overwrites** the transcript — it's a correlation id, not a resume key | `handlers/session.rs:170–173` |
| No parent / tree linkage on `AgentSession` | `session.rs:362` |
| ADR 0024 parked this: *"Resume / branch … the data shape supports it. Not in scope here."* | ADR 0024 |

## The unifying insight

**Resume, branch, and rewind are one primitive** — *fork a child session seeded
with the parent's rounds up to round `k`, plus an optional new user message, then
continue the loop.*

- **resume** = fork at the tip (`k` = last round) + new message
- **branch** = fork at an earlier round `k` + new message
- **rewind** = fork at round `k` (non-destructive) ± message

So the whole feature reduces to two capabilities, and everything else is *which
`k`*:

1. Make the run loop **seedable** with a prefix of prior rounds + an optional
   follow-up user message (round numbering continues from the prefix).
2. **Link** each child to its parent.

## Decisions (this RFC)

1. **Immutable fork-nodes.** Every resume/branch/rewind creates a *new* session
   node; existing nodes are never mutated. This preserves the current write-once
   transcript shape and the **append-only** memory (`history.jsonl`) + FTS
   (`transcripts.sqlite`) layers, and gives git-like provenance.
2. **Non-destructive rewind.** Rewind forks a new line truncated at `k`; the
   original full transcript is preserved. (This is automatic under the immutable
   model.)

## Persistence shape

Extend `AgentSession` with two `#[serde(default)]` fields (so existing session
JSON still deserializes):

```rust
pub struct AgentSession {
    // … existing fields …
    /// Parent session this node forked from; `None` for a root session.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// The parent round index this node inherited up to (its fork point);
    /// `None` for a root session.
    #[serde(default)]
    pub branch_point: Option<u32>,
}
```

**Delta storage.** A forked node persists **only its own new rounds** (rounds
after `branch_point`); the inherited prefix lives in the parent. `session_get`
returns the **assembled** transcript by walking the parent chain and
concatenating `parent.rounds[1..=branch_point] + node.rounds`. Chains are short
in practice and the walk is cheap + cacheable; this avoids the O(n²) blow-up of
copying a growing prefix on every resume. Root sessions are unchanged
(`parent_id = None`, full `rounds`).

## Operations → IPC

All lower to one internal `fork(parent_id, up_to_round, follow_up)`:

| Verb | Handler (new) | `up_to_round` | `follow_up` |
|---|---|---|---|
| resume | `session_resume { session_id, message, … }` | parent tip | required |
| branch | `session_branch { session_id, at_round, message, … }` | `at_round` | required |
| rewind | `session_rewind { session_id, at_round, message? }` | `at_round` | optional |
| checkpoint | `session_checkpoint { session_id, round, name }` | — | — (names a `(session, round)` marker) |
| tree view | `session_tree { root_id? }` / `session_list` gains `parent_id` | — | — |

Each fork: assemble the parent's transcript up to `up_to_round`, run the
**seeded loop** with `follow_up`, then persist the child node (`parent_id`,
`branch_point = up_to_round`, delta rounds) and return the assembled child.

## Loop change (the foundation — PR 1)

`run_session_with_compressor` gains a seedable core taking `seed_rounds:
Vec<RoundRecord>` + `follow_up: Option<String>`:

- `session.rounds` starts as `seed_rounds`; the next round index continues from
  the seed's last (`seed.len() + 1`).
- The first prompt: with a `follow_up`, compose from `goal` + `seed_rounds` + the
  new user message; without one, the normal continue-from-history prompt. With
  an empty seed + no follow-up, behaviour is **identical to today** (the initial
  goal prompt) — existing `run_session*` entry points pass `(vec![], None)`.
- The returned `AgentSession.rounds` is the **full** in-memory transcript (seed +
  new); the persistence layer (PR 2) slices `rounds[branch_point..]` as the
  node's delta.

## Memory / FTS interaction

A forked node records **only its own new rounds** to `history.jsonl` + FTS
(`record_session_memory` operates on `rounds` after `branch_point`), so the
inherited prefix isn't re-indexed. Append-only invariants hold.

## Phasing (small PRs)

- **PR 1 — resumable loop core.** Seed the loop with a round prefix + optional
  follow-up message; continue numbering. Existing entry points pass `(vec![],
  None)` (behaviour-preserving). Unit-tested; **no new IPC, no persistence
  change.** The foundation every operation reuses.
- **PR 2 — tree persistence + `session_resume`.** Add `parent_id`/`branch_point`
  (serde-default) + delta storage + parent-walking `session_get`; the
  `session_resume` handler (fork at tip). cap-matrix + ipc-handlers docs + IPC
  bindings.
- **PR 3 — `session_branch` + `session_rewind` + tree view.** Fork at an
  arbitrary `k`; `session_list` surfaces `parent_id`/`branch_point` so a UI can
  render the forest. *(Shipped — the dedicated nested `session_tree` convenience
  was deferred to the shell-UI PR, where the actual rendering shapes it.)*
- **PR 4 — CLI session surface.** `nexus agent sessions` (list, with fork
  markers) / `show <id>` (assembled transcript) / `resume <id> <msg>` /
  `branch <id> <round> <msg>` / `rewind <id> <round> [msg]` — the whole backend,
  usable from the terminal. *(Shipped.)*
- **PR 5 — checkpoints + shell tree UI (remaining).** `session_checkpoint`
  markers (a per-forge `checkpoints.json` index); a shell plugin under
  `shell/src/plugins/nexus/sessions/` that renders the forest from
  `session_list` and drives resume/branch/rewind via `ipc_call`.

## Open questions

- **Multi-turn prompt weaving.** Exactly how the follow-up user message threads
  into `compose_followup_prompt_compressed` alongside the inherited rounds (a
  synthetic user turn vs a re-stated goal) — settled in PR 1 against the real
  prompt composer.
- **Checkpoint storage.** A per-forge `.forge/agent/sessions/checkpoints.json`
  index vs a field on the node. Decided in PR 4; likely a small index file
  (documented in `settings/README.md`).
- **Compaction across a fork.** A child inherits the parent's `compactions`
  conceptually; for the MVP the child recomputes compaction over the assembled
  prefix as needed rather than inheriting `CompactionEvent`s verbatim.

## Non-goals (this phase)

- Wire-compatibility with omp's session JSONL v3 format — Nexus owns its shape.
- Concurrent/parallel execution of sibling branches as a scheduler feature — the
  tree is a navigation + provenance structure; running a branch is just
  `session_run` on a forked node.
- A visual graph editor — PR 4's shell UI is a tree list, not a canvas.
