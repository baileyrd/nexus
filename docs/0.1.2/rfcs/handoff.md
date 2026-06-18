# Handoff — Phase 5.5 (loop hardening) complete; the RFC 0005 ladder is fully shipped

You are continuing work on **Nexus** (microkernel Rust workspace + Tauri/React
shell) at `/home/user/nexus`. Read [`CLAUDE.md`](../../../CLAUDE.md) and
[`docs/0.1.2/README.md`](../README.md) first.

## Context: what was just finished

**Phase 5.5 — loop hardening ([RFC 0005](0005-omp-agentic-loop-phase5.md)) is
fully shipped**, completing the omp-parity ladder (5.1 hashline → 5.2 tool
catalog → 5.3 subagent isolation → 5.4 session tree → **5.5 loop hardening**).
Two PRs, both merged to `main` from branch `claude/exciting-maxwell-2x404u`:

| PR | Scope | Merged |
|----|-------|--------|
| #325 | provider-native multi-turn chat (Phase 2c) | ✅ |
| #326 | tool error/retry policies | ✅ |

**The unifying change:** the agent loop used to feed the model **one
restated-goal user message per round** (`compose_followup_prompt_compressed`),
digesting prior work into lossy `- round N: read_file ok` lines that discarded
both the real tool output and the assistant `tool_use` ↔ `tool_result` linkage.
It now **replays a real conversation** and **retries transient tool failures**.

**Where it lives:**

- **Multi-turn loop:** `crates/nexus-agent/src/session.rs` — `compose_turns`
  builds `User{goal}` (carrying any BL-120 compacted summary) → per round
  `Assistant{text, tool_calls}` + one `ToolResult` per call (failures/denials via
  `ToolResult.is_error`). `ChatDriver::propose_turns` (default flattens to the
  legacy `propose`, so test/bootstrap drivers are untouched) is overridden by
  `AiChatBridge` (`handlers/shared.rs`) to forward the turns. BL-120 compaction
  folds into the goal turn; BL-131 `sanitize_turns` runs per-`ToolResult`.
- **Wire types:** `crates/nexus-ai/src/ipc.rs` — `AiChatTurn` / `AiTurnToolCall`
  + an optional `turns` field on `AiProposeArgs`; `handle_propose_tool_calls`
  (`handlers/propose.rs`) prefers `turns` (→ provider-native `ChatTurn`s) and
  falls back to the legacy text-only `messages`. `anthropic.rs` merges a user
  turn that follows tool-results into one message (no consecutive user turns).
  **No new IPC handler** — `propose_tool_calls` was reused; the only regenerated
  bindings are `AiChatTurn.ts` / `AiTurnToolCall.ts` / `AiProposeArgs.ts` /
  `SessionConfig.ts` + the propose turn schemas.
- **Retry:** `session.rs` `dispatch_one` retries a *transient* dispatch error
  (`is_retryable_tool_error` — timeouts, resets, `unavailable`, `429`/`5xx`) up
  to `SessionConfig::max_tool_retries` (default **0 = off**) times with
  exponential backoff (`tool_retry_backoff_ms`, default 250). Permanent errors
  (not-found, validation, capability/policy denial) are never retried; the
  exhausted error is annotated `(after N attempts)`.

## Suggested next work

The RFC 0005 phased ladder (5.1–5.5) is **done**. Remaining threads, ordered
roughly by self-containment — **confirm priorities with the user before
starting**:

- **Typed tool-dispatch errors — ✅ shipped** (branch
  `claude/find-handoff-md-t1bvnp`). `ToolDispatcher::dispatch` now returns
  `Result<Value, ToolDispatchError>` (`message` + a `ToolErrorKind` of
  `Transient`/`Permanent`/`Unknown`). The kernel bridges (`KernelToolBridge`,
  `KernelToolDispatcher`) fold `IpcError` into an exact kind via
  `IpcErrorEnvelope::retryable`, so the session loop retries transient IPC
  failures without string-sniffing. `is_retryable_tool_error` remains the
  `Unknown` fallback (every `String`/`&str` conversion lands there).
  *Per-tool idempotency-aware retry* is now **✅ shipped** too: `AgentToolSpec`
  carries an `idempotent` flag (mutating / side-effecting tools — writes,
  deletes, pushes, terminal exec, delegation, `ask` — are `false`);
  `SessionConfig::non_idempotent_tools` is a deny-list the registry-free loop
  consults so a *transient* failure of a non-idempotent tool is reported
  without a retry; the agent service seeds it from
  `AgentToolRegistry::non_idempotent_tool_names` when `max_tool_retries > 0`.
  (Spun out of #326.) Structured retry count is now **✅ shipped** too:
  `ToolCallRecord.attempts` records total dispatch attempts (`0` = never
  dispatched, `1` = clean, `1 + N` = `N` retries), so consumers read the retry
  count structurally instead of parsing the `(after N attempts)` error suffix.
  Considered and **declined (YAGNI)**: promoting `ToolDispatchError` from its
  `{message, kind}` struct to a richer error *enum* — no consumer needs finer
  branching than `is_retryable()` today; revisit if a per-cause retry policy
  (e.g. retry timeouts but not cancellations) is ever wanted.
- **`ask` frontend wiring — ✅ shipped; per-tool dispatch timeout still open.**
  The `ask` backend (publishes `com.nexus.agent.ask_requested` / awaits
  `ask_respond`) is now rendered by the shell: the `nexus.agent` plugin
  subscribes to `ask_requested` (same `com.nexus.agent.` bus subscription as
  `round_proposed`), shows an inline **AskCard** (radio / checkbox / free-text
  per question, mirroring `ApprovalCard`), and posts `ask_respond` with
  `[{ id, selected, custom_input? }]` answers. Frontend-only (the `ask` payload
  isn't ts-exported, so the question/answer types are hand-written in
  `sessionStore.ts`; no binding regen). **Still open:** the *per-tool dispatch
  timeout* — `ask` can only wait `DEFAULT_ASK_TIMEOUT_SECS` (50 s) under the
  shared 60 s `DEFAULT_TOOL_TIMEOUT` ceiling, so a human has <50 s to answer.
  Give `ask` (and other slow tools) a per-tool dispatch timeout so the wait can
  exceed the default tool ceiling. (RFC 0005 backlog.)
- **Subagent isolation — orchestration (RFC 0006 Step 2 / [RFC 0007](0007-subagent-process-isolation.md)).**
  PR 1–2 (headless spawn, worktree harness + merge-back) are in; PR 3 (OS-sandbox
  the child) and PR 4 (conflict surfacing, concurrency, `nexus_bin` setting) are
  queued. PR 3 is the natural first consumer of the bundled-shell work below.
- **Other open RFCs** (see [`rfcs/README.md`](README.md)):
  [RFC 0001](0001-workflow-cap-delegation.md) workflow cap delegation (security;
  no code yet); [RFC 0002](0002-bundled-shell-rush.md) / [RFC 0003](0003-terminal-emulator-rusty-term.md)
  bundled shell + headless VT core, both gated behind the OS-sandbox (follow
  RFC 0007 PR 3).

**Known follow-ups / limitations from this effort (optional):**

- `compose_turns` assumes the loop invariant that text-only rounds are terminal,
  so real transcripts never contain consecutive same-role turns; the Anthropic
  adapter additionally coalesces a user turn after tool-results. A hand-crafted /
  corrupted seed could still produce consecutive assistant turns (not coalesced).
- The per-round "reconsider on error / finish when done" guidance moved from the
  flat-prompt nudge into the **default planner system prompt**; custom archetypes
  keep their own prompts and don't inherit it.
- Retries are opt-in (default 0) and don't record a structured retry count on
  `ToolCallRecord` (only a tracing log + the error-string annotation).
- No live-AI-provider end-to-end run of either change (none configured here);
  both are covered by scripted/capturing drivers + per-provider serialization and
  classification unit tests.

## Workflow norms (this effort followed these)

- Develop on branch `claude/exciting-maxwell-2x404u`; small, fully-verified
  commits; open a PR per cohesive unit and merge before the next.
- New IPC handler ⇒ add a `cap_matrix.toml` entry + bump the count in
  [`ipc-handlers.md`](../ipc-handlers.md) (the `bootstrap_coverage` test gates
  handler ↔ matrix). (Phase 5.5 added **no** handler — it reused
  `propose_tool_calls`.)
- Any IPC-boundary type change ⇒ run `scripts/check_ipc_drift.sh`, then
  `git add` the regenerated `*.ts` / `*.json`. (ts-rs emits a `.ts` per
  `#[ts(export)]` type on `cargo test --features ts-export`; the schemars side is
  explicit in `crates/nexus-bootstrap/tests/ipc_schema_emit.rs`.)
- Commit trailers: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` and
  `Claude-Session: …`. Don't put the model id in pushed artifacts.

## Verification

- **Rust:** `cargo test -p nexus-agent` / `-p nexus-ai` / `-p nexus-bootstrap` /
  `-p nexus-cli`; `cargo clippy --workspace --all-targets` (the crates carry
  pre-existing pedantic warnings — CI is not `-D warnings`; just don't add new
  ones in touched files).
- **Shell** (from `shell/`): `pnpm install`, then `pnpm typecheck` / `pnpm lint`
  / `pnpm test`. Colocated `src/**/*.test.ts` are **not** CI-gated unless
  re-exported by a `tests/plugins-nexus-*.test.ts` shim.

## Environment gotchas

- The Bash shell resets cwd to `/home/user` between calls — **always prefix
  `cd /home/user/nexus &&`**.
- Disk fills from `ts-export` rebuilds; `cargo clean` reclaims ~30 GiB. **Never**
  `rm -rf target/debug/build/*/out` (deletes build-script outputs and breaks the
  build). A full `--features ts-export` rebuild of a crate is ~5 min; redirect
  cargo to a logfile and `tail`/`grep` it (the task-output tmpfs is tiny).
- Keep the local branch at your own commits; don't fast-forward it onto GitHub
  merge commits.
- GitHub only via `mcp__github__*` tools; scope currently includes
  `baileyrd/nexus`.
