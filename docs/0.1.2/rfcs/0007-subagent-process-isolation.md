# RFC 0007 — Phase 5.3 Step 2: process-level subagent isolation (build design)

- **Status:** Draft — design (PR 1 = spawn primitive; merge policy = merge-the-branch)
- **Owner:** unassigned
- **Created:** 2026-06-17
- **Tracks:** [RFC 0006](0006-subagent-workspace-isolation.md) Step 2 (decision = Option A); [RFC 0005](0005-omp-agentic-loop-phase5.md) Phase 5.3; omp blueprint `docs/17-subagents-and-tasks.md`
- **Touches:** `crates/nexus-agent/` (`subagent` spawn primitive, `delegate`), `crates/nexus-cli/` (`agent run --format json`), `crates/nexus-git/` (worktree + merge, shipped in #313), `crates/nexus-security/` (OS-sandbox), `docs/0.1.2/settings/`
- **Related:** [RFC 0002 — bundled shell](0002-bundled-shell-rush.md), [RFC 0003 — `rusty_term`](0003-terminal-emulator-rusty-term.md) (OS-sandbox tie-ins)

---

## Summary

RFC 0006 chose **Option A** (process-level isolation) for subagent workspace
isolation and shipped the option-agnostic git-worktree primitives (#313). This
RFC is the **build design** for Option A: a delegated subagent runs as a
**headless child `nexus` process** pointed at a **git-worktree forge root**,
optionally under the **OS-sandbox**, and the parent **merges the worktree branch
back** into its HEAD.

The research that grounds this design closed RFC 0006's open question #3
(in-process vs process) decisively: **it must be a child process.** Two
independent facts force it.

1. **Layering forbids in-process.** Building a second `Runtime` on the worktree
   forge root needs `nexus-bootstrap`, which sits *above* every service crate.
   `nexus-agent` / `nexus-ai-runtime` (where delegation lives) cannot depend on
   bootstrap without inverting the microkernel dependency rule that
   `crates/nexus-bootstrap/tests/dep_invariants.rs` enforces. An in-tree
   orchestrator literally cannot construct a child runtime in-process.
2. **OS-sandbox confinement is irreversible and per-thread.**
   `nexus_security::os_sandbox::confine_current_thread` applies Landlock +
   seccomp to the calling thread permanently; you cannot confine a subagent's
   threads without confining the parent. Confined isolation *requires* a separate
   OS process — which `nexus_security::os_sandbox::sandbox_command(helper, policy,
   cwd, program, args)` already spawns via the `nexus-sandbox` sidecar.

A child process also gives clean `StorageEngine` / kernel / `EventBus` separation
for free (each `Runtime` owns its own), and the headless entry point already
exists: `nexus agent run <goal> --forge-path <root>` runs a session to
completion and (with this RFC's small `--format json` addition) prints a
machine-readable transcript.

## What the parent orchestrates

Delegation stays in `nexus-agent`'s `delegate` handler. A new `isolation` arg
selects behaviour; everything routes through IPC + one OS-process boundary, so
no layering rule is touched.

- `isolation: "none"` (default) — today's behaviour: pack `AgentTaskKind::Session`,
  submit to `com.nexus.ai.runtime`, child shares the parent forge. Unchanged.
- `isolation: "worktree"`:
  1. Require a git-backed forge (`com.nexus.git` is passive on `NotARepo`) — else
     a clear error (no surprise `git init`). See open question #1.
  2. `com.nexus.git::worktree_create { name: "subagent-<id>", branch:
     "nexus/subagent/<id>" }` → checkout under `<forge>/.forge/worktrees/<name>/`.
  3. Resolve the sandbox policy via `com.nexus.security::sandbox_policy`; derive a
     `WorkspaceWrite { writable_roots: [worktree] }` policy (PR 3).
  4. Spawn `nexus agent run --forge-path <worktree> --format json "<goal>"`
     headless (auto-approve), under `sandbox_command` once PR 3 lands. Capture
     stdout (transcript), stderr, exit code, with a timeout.
  5. Stage + commit the worktree delta to the task branch.
  6. `com.nexus.git::merge { branch: "nexus/subagent/<id>" }` into parent HEAD.
     **Merge-the-branch** is the decided policy (preserves the subagent's commit
     graph + provenance). On conflict → `abort_merge`, return
     `{merged:false, conflicts:[…], branch}` (branch kept for resolution);
     on success → `{merged:true, commit}`.
  7. `com.nexus.git::worktree_remove { name, force:true }` (keep the branch).
  8. Return `{ outcome, merged, commit?, conflicts? }`.

## Phasing (small PRs)

- **PR 1 — headless child-spawn primitive** (*this PR*). A self-contained
  `nexus-agent::subagent` module: resolve the `nexus` binary, build the
  `agent run --forge-path … --format json` argv, spawn with a timeout, capture
  stdout / stderr / exit / `timed_out`, best-effort-parse the transcript JSON.
  Plus the minimal `agent run --format json` output path so the captured
  transcript is structured. No git, no sandbox, not yet wired into `delegate`.
- **PR 2 — worktree isolation harness.** Wire steps 2/5/6/7 onto PR 1 behind
  `delegate`'s `isolation: "worktree"`: worktree → commit → merge-the-branch →
  cleanup, with conflict surfacing. Delivers true workspace isolation end-to-end
  (sandbox still inherits the parent).
- **PR 3 — OS-sandbox the child.** Wrap the spawn in `sandbox_command` + derive
  the `WorkspaceWrite` policy from the parent's `sandbox.toml`. Linux-enforced
  (Landlock + seccomp); graceful no-op on macOS/Windows.
- **PR 4 — polish.** Structured conflict surfacing to the parent agent loop,
  `max_concurrent_sub_agents` admission enforcement, and the `nexus_bin` setting
  (below) for shell/MCP frontends.

## Resolved open questions (from RFC 0006)

1. **Isolation model** → **A (process-level)**, decided in RFC 0006.
2. **Require git?** → **Require a git-backed forge** when `isolation:
   "worktree"` is requested (hard error otherwise). `isolation: "none"` keeps
   today's shared-forge behaviour, so isolation is opt-in and non-git forges are
   never blocked from delegating.
3. **In-process vs process** → **Child process**, forced by the layering rule +
   the irreversible per-thread sandbox (see Summary).
4. **Merge policy** → **Merge the branch** (`com.nexus.git::merge`), preserving
   the subagent's commit graph. `patch` / cherry-pick is not used.

## New questions this design introduces

- **Locating the `nexus` binary.** `std::env::current_exe()` is the `nexus` CLI
  when delegation runs under the CLI/TUI, but **not** under the Tauri shell or
  the MCP server (their `current_exe()` is a different binary). PR 1 resolves an
  explicit override first, then falls back to `current_exe()`; PR 4 promotes the
  override to a documented `agent.subagent.nexus_bin` setting required for
  shell/MCP. (New setting ⇒ `docs/0.1.2/settings/` entry + hardcoded-row delete,
  per the repo guardrail, when PR 4 lands.)
- **Committing the worktree delta (step 5).** The orchestrator stages + commits
  the worktree explicitly (deterministic) rather than relying on the agent
  loop's optional auto-commit. Finalised in PR 2.
- **Failure semantics.** Non-zero child exit / timeout ⇒ no merge; the worktree +
  branch are kept for inspection and a failure outcome is returned. Finalised in
  PR 2.

## Non-goals (this phase)

- fuse-overlay / ProjFS / APFS-clone / reflink isolation backends (omp's native
  fast paths) — worktree is the portable MVP.
- IRC-style inter-subagent messaging (omp's `irc`) — separate concern.
- In-process per-session storage routing (RFC 0006 Option B) — ruled out by the
  layering rule above.
