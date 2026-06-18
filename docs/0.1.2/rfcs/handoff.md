# Handoff — agent retry + interaction follow-ups shipped; RFC 0007 subagent isolation fully landed

You are continuing work on **Nexus** (microkernel Rust workspace + Tauri/React
shell) at `/home/user/nexus`. Read [`CLAUDE.md`](../../../CLAUDE.md) and
[`docs/0.1.2/README.md`](../README.md) first.

## Context: what was just finished

Phase 5.5 ([RFC 0005](0005-omp-agentic-loop-phase5.md)) shipped the omp-parity
loop (multi-turn replay + opt-in retry). This session closed out **every
follow-up that ladder spun off** — five PRs, all merged to `main` from branch
`claude/find-handoff-md-t1bvnp`:

| PR | Scope | Merged |
|----|-------|--------|
| #328 | typed tool-dispatch errors (exact retry classification) | ✅ |
| #329 | idempotency-aware tool retry | ✅ |
| #330 | structured retry count on `ToolCallRecord` | ✅ |
| #331 | shell(agent): render interactive `ask` prompts | ✅ |
| #332 | per-tool dispatch timeout for slow/interactive tools | ✅ |

**Where it lives (new public surface to build on):**

- **Typed dispatch errors (`#328`)** — `crates/nexus-agent/src/lib.rs`:
  `ToolDispatcher::dispatch` returns `Result<Value, ToolDispatchError>`
  (`message` + `ToolErrorKind::{Transient,Permanent,Unknown}`). The kernel
  bridges (`KernelToolBridge` in `handlers/shared.rs`, `KernelToolDispatcher`
  in `nexus-bootstrap/src/agent.rs`) fold `IpcError` into an exact kind via
  `IpcErrorEnvelope::retryable`. `session.rs::dispatch_one` calls
  `e.is_retryable()`; `Unknown` falls back to the `is_retryable_tool_error`
  string heuristic (every `String`/`&str` → `Unknown`).
- **Idempotency-aware retry (`#329`)** — `AgentToolSpec.idempotent`
  (`tool_registry.rs`; mutating/side-effecting tools are `false`).
  `SessionConfig::non_idempotent_tools` is a deny-list the **registry-free**
  session loop consults; the agent service seeds it from
  `AgentToolRegistry::non_idempotent_tool_names()` in
  `run_session_optionally_gated_resumed` when `max_tool_retries > 0`. A
  transient failure of a listed tool is reported without a retry.
- **Retry count (`#330`)** — `ToolCallRecord.attempts` (`0` = never dispatched,
  `1` = clean, `1 + N` = N retries). The `(after N attempts)` error suffix
  stays for the model's transcript; `attempts` is the structured form.
- **`ask` shell panel (`#331`)** — `shell/src/plugins/nexus/agent/`: the
  `com.nexus.agent.` bus subscription routes `ask_requested` → `setPendingAsk`
  (`agentRuntime.ts`); `AskCard` (`AgentSessionView.tsx`) renders radio /
  checkbox / free-text per question; `submitAnswer` posts `ask_respond` with
  `[{ id, selected, custom_input? }]`. Question/answer types are hand-written
  in `sessionStore.ts` (the `ask` payload is **not** ts-exported).
- **Per-tool timeout (`#332`)** — `AgentToolSpec.dispatch_timeout_ms` (default
  `DEFAULT_TOOL_DISPATCH_TIMEOUT_MS` = 60 s). Both bridges resolve it via
  `AgentToolRegistry::dispatch_timeout_for(target, command)`; `ask` is
  `ASK_DISPATCH_TIMEOUT_MS` (330 s) and its handler wait
  `DEFAULT_ASK_TIMEOUT_SECS` was raised 50 s → 300 s (an `handlers::ask` test
  guards `ASK_DISPATCH_TIMEOUT_MS` > the handler wait). Unregistered routes use
  the bridge default.

**Considered and declined (YAGNI):** promoting `ToolDispatchError` from its
`{message, kind}` struct to a richer error *enum* — no consumer needs finer
branching than `is_retryable()` today. Revisit only if a per-cause retry policy
(e.g. retry timeouts but not cancellations) is ever wanted.

## Suggested next work

The RFC 0005 ladder and all its follow-ups are done — **and so is the entire
RFC 0007 subagent-isolation ladder.** (A prior draft of this handoff listed its
PR 3/PR 4 as "queued, PR 3 is the natural next step"; that was wrong — all four
PRs were merged before the Phase 5.5 retry work even started.) Verified in the
tree, `cargo test -p nexus-agent` green (`subagent::*` 17 passed / 2 ignored):

| Commit | PR | What landed |
|--------|----|-------------|
| `843e8eb` | PR 1 | headless child-spawn primitive (`crates/nexus-agent/src/subagent.rs`) |
| `8f86043` | PR 2 | worktree isolation harness (`delegate isolation="worktree"` → `delegate_isolated`) |
| `a1b10ae` | PR 3 | OS-sandbox the child (`apply_subagent_sandbox`, `resolve_parent_policy`, `derive_subagent_policy`, `nexus-sandbox` helper wrap) |
| `76c50be` | PR 4 | polish: concurrency cap (`NEXUS_SUBAGENT_MAX_CONCURRENT`), `NEXUS_SUBAGENT_BIN`, conflict `summary` in `build_isolated_result` |

So the **OS-sandbox confinement that RFC 0002/0003 were gated behind now
exists**, with a working consumer to copy (`subagent.rs::spawn_invocation` →
`nexus_types::sandbox_argv` → `nexus-sandbox` sidecar). Remaining threads,
ordered roughly by self-containment — **confirm priorities with the user before
starting** (these are larger architectural efforts, not quick follow-ups):

- **RFC 0002 / RFC 0003 — bundled shell + headless VT** (see
  [`rfcs/README.md`](README.md)): [RFC 0002](0002-bundled-shell-rush.md) vendors
  `baileyrd/rush` as a workspace lib and runs it as the bundled shell for
  *sandboxed* terminal sessions (system shell stays default);
  [RFC 0003](0003-terminal-emulator-rusty-term.md) adopts the headless VT grid
  core + OSC 133 command/exit-code capture for agent-observable terminals. Both
  were gated behind the OS-sandbox — **now unblocked** (RFC 0007 PR 3 landed).
- **RFC 0001 — workflow cap delegation**
  ([RFC 0001](0001-workflow-cap-delegation.md)): close the capability-laundering
  surface where workflow steps dispatch through the workflow plugin's own caps
  rather than the triggering principal's (security; no code yet). Self-contained.

**Smaller optional threads left open this session:**

- `compose_turns` assumes text-only rounds are terminal, so real transcripts
  never contain consecutive same-role turns (the Anthropic adapter also
  coalesces a user turn after tool-results). A hand-crafted / corrupted seed
  could still produce consecutive assistant turns (not coalesced).
- Retries are opt-in (`max_tool_retries` default 0). No live-AI-provider
  end-to-end run of the retry/idempotency paths (none configured here); covered
  by scripted/capturing drivers + unit tests.
- `ASK_DISPATCH_TIMEOUT_MS` / `DEFAULT_ASK_TIMEOUT_SECS` are hardcoded (330 s /
  300 s). Promote to config if an operator ever needs to tune the interactive
  wait — see [`settings/hardcoded-rust.md`](../settings/hardcoded-rust.md).

## Workflow norms (this session followed these)

- Develop on your assigned `claude/…` branch; small, fully-verified commits;
  open a PR per cohesive unit and merge before the next. Keep the local branch
  at your own commits — don't fast-forward it onto GitHub merge commits.
- New IPC handler ⇒ add a `cap_matrix.toml` entry + bump the count in
  [`ipc-handlers.md`](../ipc-handlers.md) (the `bootstrap_coverage` test gates
  handler ↔ matrix). None of this session's PRs added a handler.
- Any IPC-boundary type change ⇒ regenerate bindings. Most IPC structs are
  ts-rs-only (`AgentToolSpec`, `SessionConfig`, `ToolCallRecord` were
  regenerated this session via `cargo test -p <crate> --features ts-export
  --tests`); only the curated set in
  `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` also emits a JSON schema.
  `scripts/check_ipc_drift.sh` regenerates everything but rebuilds ~25 crates
  with `ts-export` (slow here) — prefer the single-crate ts-export run + a
  targeted `git diff` of `packages/nexus-extension-api/src/generated/`.
- Commit trailers: `Co-Authored-By: …` and `Claude-Session: …`. Don't put the
  raw model id in pushed artifacts.

## Verification

- **Rust:** `cargo test -p nexus-agent` / `-p nexus-bootstrap` (etc.);
  `cargo check --workspace --all-targets` to catch downstream breakage;
  `cargo clippy -p <crate> --all-targets`. The crates carry pre-existing
  pedantic warnings (e.g. `missing_panics_doc` on every registry method that
  `.expect()`s the mutex) — CI is **not** `-D warnings`; just don't add a *new
  kind* of warning in touched files. Match the surrounding convention.
- **Shell** (from repo root): `pnpm install`, then
  `pnpm --filter nexus-shell typecheck` / `lint` / `test`. The full suite is
  ~1700 tests / ~80 s. Colocated `src/**/*.test.ts` only run when re-exported
  by a `tests/<name>.test.ts` shim (e.g. `tests/agent.test.ts` imports the
  agent plugin's colocated tests). Lint emits ~290 pre-existing a11y warnings
  (0 errors) — check your new lines aren't among them.

## Environment gotchas

- The Bash shell resets cwd to `/home/user` between calls — **always prefix
  `cd /home/user/nexus &&`**.
- Disk fills from `ts-export` rebuilds; `cargo clean` reclaims ~30 GiB. **Never**
  `rm -rf target/debug/build/*/out` (deletes build-script outputs and breaks the
  build). A full `--features ts-export` rebuild of a crate is ~5 min; redirect
  cargo to a logfile and `tail`/`grep` it (the task-output tmpfs is tiny). Run
  long builds/tests in the background and poll the output file.
- GitHub only via `mcp__github__*` tools (load via ToolSearch — the server
  drops/reconnects between turns); scope currently includes `baileyrd/nexus`.
  GitNexus MCP tools are **not** wired in this environment despite the CLAUDE.md
  guidance — trace impact manually.
