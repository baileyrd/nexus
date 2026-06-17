# RFC 0006 — Phase 5.3: subagent workspace isolation

- **Status:** Step 1 shipped (#313); decision = Option A; Step 2 build design = [RFC 0007](0007-subagent-process-isolation.md)
- **Owner:** unassigned
- **Created:** 2026-06-17
- **Tracks:** [RFC 0005](0005-omp-agentic-loop-phase5.md) Phase 5.3; omp blueprint `docs/17-subagents-and-tasks.md`
- **Touches (depends on the option chosen):** `crates/nexus-git/` (worktree handlers), `crates/nexus-agent/` (`delegate`), `crates/nexus-ai-runtime/` (child forge root), `crates/nexus-security` (OS-sandbox), `crates/nexus-bootstrap`
- **Related:** [RFC 0002 — bundled shell](0002-bundled-shell-rush.md), [RFC 0003 — `rusty_term`](0003-terminal-emulator-rusty-term.md) (OS-sandbox tie-ins)

---

## Summary

Phase 5.3 is **subagent workspace isolation**: a delegated subagent should make
its edits in an *isolated* copy of the forge and have the parent **merge the
delta** back, so parallel subagents don't clobber each other or the parent's
working tree (omp's `task.isolation`: worktree → patch/branch merge).

Nexus makes this **architecturally deeper than omp**, and the right approach is a
genuine decision rather than a default. This RFC lays out the tension, three
options, and a recommended phased plan whose **first PR is foundational and
option-agnostic** (git-worktree primitives in `nexus-git`).

## Where Nexus stands today

| Fact | Evidence |
|---|---|
| Subagents **share the parent forge** | `delegate` (`handlers/delegate.rs`) packs args into `AgentTaskKind::Session` and submits to `com.nexus.ai.runtime`; there is **no** workspace / forge-root override. The child session edits the same files. |
| Storage is **forge-root-bound** | One `StorageEngine` per runtime; every file tool routes to the single `com.nexus.storage`. There is no per-call forge selection. |
| The ai-runtime runs children on the **shared** storage | `nexus-ai-runtime` spawns the child `session_run` on its worker pool against the same plugin set. |
| Git is **optional** | `nexus-git` goes *passive* when the forge root is not a repo (`NotARepo`). Worktrees presuppose a git-backed forge. |
| Merge primitives exist | `nexus-git` has `cherry_pick` / `abort_cherry_pick` / auto-commit — but **no worktree** handlers. |

## The core tension

omp isolates subagents **in-process** because its file tools are **cwd-scoped**:
point a subagent's tools at a worktree directory and it edits there. Nexus's file
tools are **forge-root-bound through a single storage plugin**, so "operate on a
different directory" is not a `cwd` — it requires routing the child's storage to
a *different forge root*. That is the crux every option has to answer.

## Options

### A. Process-level isolation (child runtime per subagent)

Spawn a child `nexus` runtime pointed at a **git-worktree forge root**, optionally
under the **OS-sandbox** (`SandboxPolicy`), run the subagent there headless, then
merge the worktree delta (`patch` via `git apply`, or `branch` via cherry-pick)
back into the parent forge.

- ✅ True isolation; the cleanest model; **ties directly to the OS-sandbox** and
  the bundled-shell vision (RFC 0002/0003) — a subagent becomes a confined,
  observable, mergeable unit.
- ✅ No change to storage's one-engine-per-root model.
- ✗ Biggest build: child-process orchestration, headless run + result plumbing,
  worktree lifecycle, merge. Requires a git-backed forge.

### B. Per-subagent storage routing (in-process)

Thread a forge-root / worktree override through the child session so its
`com.nexus.storage` calls resolve against a *second* `StorageEngine` bound to the
worktree.

- ✅ In-process (lighter than spawning); matches omp's logical-subprocess model.
- ✗ Invasive to the storage model: a single plugin id must now serve multiple
  forge roots (per-session engine selection), touching the kernel IPC dispatch
  or every storage handler. High blast radius on the busiest service crate.

### C. Branch-based outcome tracking (lightest, weaker isolation)

Keep the subagent on the **shared** forge, but bracket its run with git: create a
task branch, let it edit, commit its changes to `omp/task/<id>`, and surface the
branch for the parent to review / merge.

- ✅ Small; reuses existing commit/cherry-pick; useful task provenance.
- ✗ **Not real isolation** — concurrent subagents still share the working tree
  during the run; only the *outcome* is branch-tracked. Requires a git-backed
  forge.

## Recommended plan

**Step 1 (option-agnostic, ship first): git-worktree primitives in `nexus-git`.**
✅ **Shipped in #313.** A small, self-contained PR added `worktree_create` /
`worktree_list` / `worktree_remove` over `git2::Worktree` (handlers 39–41),
creating worktrees under `<forge>/.forge/worktrees/<name>`, with a create/list/
remove round-trip test. No agent-loop change, no storage change. (`worktree_merge`
— `patch` / `branch` — folds into Step 2, where the merge policy is decided.)

**Step 2: pick the isolation model (A / B / C) and build it.** ✅ **Decided: A
(process-level)** — the only option that gives *true* isolation, it avoids
reshaping the storage model, and it is the natural home for the OS-sandbox +
bundled-shell work already scoped in RFCs 0002/0003: a subagent becomes "a
confined runtime on a worktree whose delta we merge." It is the biggest build
(child-process orchestration, headless run + result plumbing, worktree
merge / conflict surfacing), so its build design is captured separately in
[RFC 0007](0007-subagent-process-isolation.md), which resolves the open
questions below (child process, require-git, merge-the-branch) and phases the
build PR 1 (spawn primitive) → 2 (worktree harness) → 3 (OS-sandbox) → 4 (polish).

## Open questions (for the decision)

1. **Isolation model** — A (process), B (storage routing), or C (branch
   tracking)? This RFC recommends A, with the worktree primitives (Step 1) first
   regardless.
2. **Require git?** Worktree isolation needs a git-backed forge. Do we (a) require
   `git init` for subagent isolation and fall back to today's shared-forge
   behavior otherwise, or (b) gate the whole feature on a git forge?
3. **In-process vs process** for A — a full child `nexus` process (strongest
   isolation, heaviest) vs a lighter in-runtime child bound to a worktree root.
4. **Merge policy default** — `patch` (`git apply`) vs `branch` (cherry-pick), and
   conflict surfacing back to the parent agent (reuse the hashline conflict
   shape?).

## Non-goals (this phase)

- fuse-overlay / ProjFS / APFS-clone / reflink isolation backends (omp's native
  fast paths) — worktree is the portable MVP.
- IRC-style inter-subagent messaging (omp's `irc`) — separate concern.
