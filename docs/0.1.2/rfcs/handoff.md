# Handoff — Phase 5.4 (agent session tree) complete; next: Phase 5.5

You are continuing work on **Nexus** (microkernel Rust workspace + Tauri/React
shell) at `/home/user/nexus`. Read [`CLAUDE.md`](../../../CLAUDE.md) and
[`docs/0.1.2/README.md`](../README.md) first.

## Context: what was just finished

**Phase 5.4 — the agent session tree ([RFC 0008](0008-agent-session-tree.md)) is
fully shipped** across 6 small PRs (all merged to `main` from branch
`claude/lucid-turing-8w72qg`):

| PR | Scope | Merged |
|----|-------|--------|
| #318 | resumable loop core | ✅ |
| #319 | tree persistence + `session_resume` | ✅ |
| #320 | `session_branch` + `session_rewind` | ✅ |
| #321 | CLI session surface | ✅ |
| #322 | checkpoints | ✅ |
| #323 | `nexus.sessions` shell tree UI | ✅ |

**The unifying insight:** resume / branch / rewind are one primitive —
`fork(parent, k, message)` (assemble the parent transcript, truncate to round
`k`, seed the loop with that prefix + an optional follow-up message, continue).
Decisions: **immutable fork-nodes** (delta-stored) + **non-destructive rewind**.
Design doc: [`0008-agent-session-tree.md`](0008-agent-session-tree.md) (status:
Implemented).

**Where it lives:**

- **Loop:** `crates/nexus-agent/src/session.rs` —
  `run_session_resumed[_with_compressor]` (seedable; empty seed = old
  behaviour). `AgentSession` gained `parent_id`/`branch_point`; new
  `SessionCheckpoint` type (both ts-exported).
- **Handlers** (`com.nexus.agent`, ids 28–33): `session_resume`/`branch`/`rewind`
  in `handlers/session.rs` (shared `fork_session` + `run_and_persist_session`
  core); `session_checkpoint[s]`/`_delete` in `handlers/checkpoint.rs`. Forked
  nodes persist only delta rounds; `session_get` assembles by walking parents.
  `checkpoints.json` per forge.
- **CLI:** `crates/nexus-cli/src/commands/agent.rs` —
  `nexus agent sessions|show|resume|branch|rewind|checkpoint|checkpoints|checkpoint-rm`.
- **Shell:** `shell/src/plugins/nexus/sessions/` (`sessionTree.ts` pure forest
  logic, `sessionsRuntime.ts`, `sessionsStore.ts`, `SessionTreeView.tsx`);
  registered in `shell/src/plugins/catalog.ts`.

## Suggested next work

**Phase 5.5 — loop hardening** (from the RFC 0005 ladder,
[`0005-omp-agentic-loop-phase5.md`](0005-omp-agentic-loop-phase5.md)):
provider-native multi-turn chat (Phase 2c — replace the restate-the-goal prompt
formulation with real `ChatTurn` linkage) and tool error/retry policies. Touches
`nexus-ai`, `nexus-agent`. Confirm priorities with the user before starting.

**Known follow-ups / limitations (optional):**

- Resume re-resolves the system prompt from `archetype` — a custom `system`
  override isn't persisted on `AgentSession`.
- Branch-from-checkpoint-by-name isn't wired (checkpoints are bookmarks; the user
  reads coords and runs `branch`). Possible UX enhancement.
- No dedicated nested `session_tree` IPC handler — `session_list` carries
  `parent_id`/`branch_point` and clients build the forest.
- The shell React view was verified by typecheck/lint/unit-test only, not
  visually.
- End-to-end resume/branch/rewind with a live AI provider wasn't exercised (none
  configured in the dev environment).

## Workflow norms (this effort established these)

- Develop on branch `claude/lucid-turing-8w72qg`; small, fully-verified PRs; open
  **and** merge each before starting the next.
- New IPC handler ⇒ add a `cap_matrix.toml` entry + bump the count in
  [`ipc-handlers.md`](../ipc-handlers.md) (the `bootstrap_coverage` test gates
  handler ↔ matrix).
- Any IPC-boundary type change ⇒ run `scripts/check_ipc_drift.sh`, then
  `git add` the regenerated `*.ts` / `*.json`.
- Commit trailers: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` and
  `Claude-Session: …`. Don't put the model id in pushed artifacts.

## Verification

- **Rust:** `cargo test -p nexus-agent` / `-p nexus-bootstrap` / `-p nexus-cli`;
  `cargo clippy --workspace --all-targets`.
- **Shell** (from `shell/`): `pnpm install`, then `pnpm typecheck` / `pnpm lint`
  / `pnpm test`. Colocated `src/**/*.test.ts` are **not** CI-gated unless
  re-exported by a `tests/plugins-nexus-*.test.ts` shim.

## Environment gotchas

- The Bash shell resets cwd to `/home/user` between calls — **always prefix
  `cd /home/user/nexus &&`**.
- Disk fills from `ts-export` rebuilds; `cargo clean` reclaims ~30 GiB. **Never**
  `rm -rf target/debug/build/*/out` (deletes build-script outputs and breaks the
  build). The task-output tmpfs is tiny — pipe cargo through `grep` or redirect
  to a logfile and `tail`.
- Keep the local branch at your own commits; don't fast-forward it onto GitHub
  merge commits.
- GitHub only via `mcp__github__*` tools; scope currently includes
  `baileyrd/nexus`.
