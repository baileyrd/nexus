# Git Implementation Assessment
_Assessed: 2026-05-06_

## Overall: 8/10 — Production-ready core, strong architecture, UX surface incomplete

The git implementation is one of the cleaner subsystems in the codebase — well-scoped, correctly
threaded, and backed by a battle-tested library. The engine is honest about what it does and
doesn't do. The primary gap isn't in the Rust backend; it's in the shell UI, where the backend's
capabilities aren't yet surfaced to users.

---

## What's fully implemented and first-class

**Threading model is correct.** `git2::Repository` is `!Send + !Sync`. The `GitWorker` pattern
solves this cleanly: a single dedicated OS thread owns the `GitEngine`, and callers submit closures
through a bounded channel (depth 32). Drop sends `Msg::Shutdown` and joins the thread —
deterministic cleanup. Every async caller gets a `GitWorkerHandle` that is `Clone + Send + Sync`.

**27-method `GitEngine` covering the full read surface.** `state()`, `file_statuses()`,
`diff_file()`, `diff_staged()`, `blame()`, `log()`, `log_file()`, `branches()`, `remotes()` —
all returning typed structs. Tested against real repositories in 12 integration tests covering
the full lifecycle, fast-forward merges, 3-way merges with conflict detection, and branch
management.

**10 IPC handlers, all wired.** `status`, `log`, `branches`, `file_status`, `diff_file`,
`stage_file`, `unstage_file`, `commit`, `stage_all`, `unstage_all` — the complete set for a
basic commit workflow. Wire-mirror types with TS export for schema generation. Path traversal
validation at the IPC boundary before any git operation runs.

**16 CLI subcommands.** `nexus git info` through `nexus git merge` and `nexus git conflicts`.
CLI goes through `GitEngine` directly — no IPC queue — fast and synchronous.

**Merge detection is correct.** Three outcomes handled: up-to-date (no-op), fast-forward (advance
HEAD), 3-way merge (auto-commit if clean, return conflict list if not). `abort_merge()` restores
pre-merge state. Kernel bus publishes `repo_state: Merge` so the shell can prompt the user.

**Authentication covers the common cases.** SSH agent first, fallback to `~/.ssh/id_ed25519` /
`id_rsa`, HTTPS delegates to the system credential helper (macOS Keychain, Windows Credential
Manager, `pass`).

**Kernel bus events on state changes.** Core plugin polls `state()` every 2 seconds, publishes
`com.nexus.git.branch_changed`, `com.nexus.git.commit`, `com.nexus.git.dirty_changed`. Shell
status bar subscribes and stays current.

**AI integration already wired.** `GitLogTool` in `nexus-ai/src/tools/functions.rs` dispatches
to `com.nexus.git::log` via IPC. An agent can read commit history as part of a planning context.

---

## IPC handler inventory

```
com.nexus.git — 10 handlers

1.  status()                    → { branch, head, is_dirty, repo_state }
2.  log({ limit? })             → [{ hash, author, date, message, parents }]
3.  branches()                  → [{ name, is_head, upstream }]
4.  file_status({ path })       → { marker }
5.  diff_file({ path })         → [HunkDiff]
6.  stage_file({ path })        → { ok }
7.  unstage_file({ path })      → { ok }
8.  commit({ message })         → { hash }
9.  stage_all()                 → { ok }
10. unstage_all()               → { ok }
```

**Events published:**
```
com.nexus.git.state
com.nexus.git.branch_changed
com.nexus.git.commit
com.nexus.git.dirty_changed
```

---

## Feature inventory

| Feature | Status | Notes |
|---|---|---|
| git log (paginated) | ✅ | `limit` arg, topological + time sort |
| diff (working tree) | ✅ | Hunks + lines via `diff_file` |
| diff (staged) | ✅ | Index vs HEAD via `diff_staged` |
| status (staged/unstaged) | ✅ | 8-variant `FileStatus` enum |
| blame | ✅ | Author/date/message per line range |
| commit | ✅ | From index with message |
| stage/unstage (file) | ✅ | `stage_file`, `stage_all` |
| stage/unstage (hunk) | ❌ | Deferred |
| branch (list/create/switch/delete) | ✅ | All local ops |
| branch (rename/track upstream) | ❌ | Not needed yet |
| merge (detect + fast-forward) | ✅ | Correct for all 3 cases |
| merge conflict resolution UI | ❌ | Detection only; BL-079 |
| rebase | ❌ | Deferred |
| cherry-pick | ❌ | Deferred |
| stash | ❌ | Deferred |
| tags | ❌ | Deferred |
| push/pull | ✅ | With SSH + HTTPS auth |
| fetch | ✅ | Remote sync |
| auth (SSH agent) | ✅ | Agent + key path fallback |
| auth (HTTPS) | ✅ | System credential helper |
| auth (GPG sign) | ❌ | Deferred |
| auto-commit | ⚠️ | Library complete; scheduler not wired |
| shell gutter decorations | ❌ | BL-079 |
| shell diff viewer | ❌ | BL-079 |
| shell commit panel | ❌ | Not scoped |
| shell branch picker | ❌ | Not scoped |
| shell log graph | ❌ | Not scoped |
| submodules | ❌ | Not planned |

---

## Where it falls short

### 1. No shell UI for git operations (BL-079)

The backend can diff, blame, stage, and show log history. Users can't access any of this from
the shell. The status bar shows branch + dirty indicator. That's all. No gutter decorations,
no commit panel, no diff viewer, no branch picker, no log graph.

BL-079 covers gutter + diff viewer. A full git panel for staging, committing, and branch
management isn't yet scoped.

### 2. Auto-commit library exists but isn't scheduled

`AutoCommitter` (243 lines) implements debounced "never lose work" commits with a configurable
message template. Wired in the engine and tested. No background task activates it. It's a
library with no activation path from the shell or bootstrap.

### 3. Conflict resolution is detection-only

`merge()` correctly returns the list of conflicted files. There's no mechanism to surface
conflict markers in a three-way diff view. Users must edit marker strings manually or call
`abort_merge()`. Adequate as MVP; not acceptable long-term.

### 4. No hunk-level staging

`stage_file` and `stage_all` exist. Staging a specific hunk from a diff is not implemented.
Meaningful limitation for developers who practice atomic commits.

### 5. No stash, rebase, cherry-pick, or tags

All explicitly deferred. None block basic forge workflows, but a developer using git
professionally will hit them within a week.

### 6. SSH passphrase requires ssh-agent

No `nexus-security` integration to cache SSH key passphrases. Remote operations block the
worker thread if ssh-agent isn't running and the key has a passphrase.

---

## Scorecard

| Dimension | Score | Notes |
|---|---|---|
| Threading model | 10/10 | GitWorker pattern is textbook-correct for !Send |
| Core read operations | 10/10 | Status, diff, blame, log, branches complete |
| Core write operations | 9/10 | Stage/commit/push/pull/branch all work; hunk staging missing |
| Merge handling | 8/10 | Detection + abort correct; resolution UI absent |
| Authentication | 8/10 | SSH agent + HTTPS; no nexus-security integration |
| IPC surface | 9/10 | 10 handlers, typed, validated, TS-exported |
| CLI surface | 9/10 | 16 subcommands; comprehensive |
| Shell UI | 3/10 | Status bar only; no panel, gutter, log graph |
| Auto-commit | 5/10 | Library complete; scheduler not wired |
| Advanced workflows | 2/10 | Stash, rebase, cherry-pick, tags absent |

---

## The honest summary

The Rust backend is done. `GitEngine` covers everything a single-user developer needs for a
basic git workflow — correctly threaded, correctly typed, correctly tested. The AI tool
integration is live.

The problem is that almost none of this is visible to a shell user. The gap between "engine
complete" and "feature complete" is almost entirely UX work: a commit panel, a branch picker,
gutter decorations, a log viewer, and conflict resolution UI. That work is 3–4 weeks. The
backend doesn't need to change to support any of it.

---

## Key source files

```
crates/nexus-git/src/
├── engine.rs          (1,118)  — GitEngine: 27 methods over git2::Repository
├── core_plugin.rs       (539)  — 10 IPC handlers, state poller, event publisher
├── worker.rs            (324)  — GitWorker thread + GitWorkerHandle (Send+Sync)
├── auto_commit.rs       (243)  — AutoCommitter (debounced; not yet scheduled)
├── types.rs             (173)  — GitState, FileStatus, HunkDiff, LogEntry, BranchInfo
├── ipc.rs               (219)  — Wire-mirror types, TS export
└── tests/integration.rs (481)  — 12 integration tests, full lifecycle

shell/src/plugins/nexus/gitStatus/
└── gitStatusStore.ts    — Status bar: branch, HEAD, dirty indicator (read-only)

crates/nexus-cli/src/commands/git.rs
└── 16 subcommands: info, status, diff, blame, log, stage*, unstage*,
    commit, branch*, switch, fetch, push, pull, merge, conflicts, remotes
```
