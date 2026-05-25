# nexus-git

> Kind: lib · IPC plugin id: com.nexus.git · CorePlugin: yes · Has settings: yes (`[git]` in `app.toml`) · As of: 2026-05-25

## Overview

`nexus-git` is the git integration service for Nexus. It wraps libgit2 (via the `git2` crate) and exposes a broad git surface — status, diff, blame, log, staging, commit, branch / tag / stash management, merge / rebase / cherry-pick, conflict resolution, push / fetch / pull, and Git-LFS inspection — over the kernel IPC dispatcher under the plugin id `com.nexus.git`. Every frontend (CLI, TUI, MCP, Tauri shell) reaches git through one path: `context.ipc_call("com.nexus.git", command, args)`. There are 38 IPC handlers.

The crate is layered. [`GitEngine`](#public-api-surface) is the synchronous libgit2 wrapper holding a `git2::Repository`; it is deliberately neither `Send` nor `Sync` (libgit2 state is not thread-safe). [`GitWorker`](#internals--notable-implementation-details) confines one `GitEngine` to a dedicated OS thread and hands out a cheap, `Send + Sync + Clone` [`GitWorkerHandle`] that submits closures over a bounded channel and blocks on the reply — this is what makes git usable from `async` Tauri commands and from the kernel's blocking dispatch. [`GitCorePlugin`](#internals--notable-implementation-details) owns the worker, registers the IPC handlers, runs a background state poller that publishes bus events, and (when enabled) an auto-commit thread. The per-handler logic lives in the `handlers/` modules (`status`, `log`, `branches`, `staging`, `stash`, `tags`, `merge`, plus `shared` helpers); `core_plugin::dispatch` is a thin match on handler id.

The crate honours file-as-truth: it operates on the on-disk git repository at the forge root and never treats the SQLite/Tantivy index as authoritative. The repository is opened with `Repository::open(path)` (not `discover()`) so a forge nested inside an unrelated parent git repo fails fast with `NotARepo` instead of silently operating on the parent's history (pre-#85 behaviour). When the forge root is not a git repo the plugin runs in **passive mode**: `on_init` logs and continues, `status` returns JSON `null`, and all other handlers return an explicit error.

Credential handling for network operations (fetch / push / pull / push_tags) goes through a libgit2 `RemoteCallbacks` credentials callback that tries the SSH agent first, then default SSH keys (`~/.ssh/id_ed25519`, `id_rsa`), looking up encrypted-key passphrases from the OS keyring via `nexus_security::CredentialVault` under the key `ssh-passphrase:<key_name>` (BL-090), then falls back to libgit2 default (HTTPS) credentials. This is the only `nexus-security` use.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (`EventBus`, `EventFilter`, `NexusEvent`), `nexus-plugins` (`CorePlugin`, `PluginError`, `define_dispatch_helpers!` macro), `nexus-security` (`CredentialVault` for SSH passphrase retrieval), `nexus-types` (path validation `paths::resolve_within`; `activity::*` for the universal activity timeline).
- **Notable external deps:** `git2` (libgit2 bindings — the core), `chrono` (commit/blame timestamps), `toml` + `serde`/`serde_json` (config + IPC JSON), `thiserror` (`GitError`), `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature emit TS bindings + JSON Schema for the wire-mirror types in `ipc.rs` (audit P1-3, #113). `tempfile` is a dev-dependency.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin — see `crates/nexus-bootstrap/src/plugins/git.rs`), `nexus-cli`, and `nexus-tui`. The `MANIFEST_DEPS` declares `com.nexus.security` must be loaded first.

## Public API surface

Re-exported from `lib.rs`: `GitError`, `GitEngine`, all of `types::*`, `AutoCommitter`/`AutoCommitResult`, `GitWorker`/`GitWorkerHandle`, `GitCorePlugin`, plus the `core_plugin` and `ipc` modules.

### `engine` — `GitEngine` (libgit2 wrapper)
Read ops: `open` · `repo_root` · `state` (branch, short head oid, dirty flag, repo_state, upstream tracking oid+name) · `file_statuses` · `file_status` · `diff_file` (HEAD vs workdir+index) · `diff_staged` (index vs HEAD, per-file) · `blame` · `log(limit)` · `log_file(path, limit)` (commits that changed a path).
Staging/commit: `stage_file` (LFS-aware) · `stage_all` · `unstage_file` · `unstage_all` · `stage_hunks` · `unstage_hunks` · `discard_hunks` · `commit(message) -> short hash`.
Branches: `branches` · `create_branch` · `switch_branch` (force checkout, no dirty check) · `delete_branch` (refuses current HEAD).
Remotes: `remotes` · `fetch` · `push` · `push_tags` · `pull` (fetch + merge).
Tags: `list_tags` · `create_tag` (annotated if message present, else lightweight) · `delete_tag`.
Stash (`&mut self`): `stash_push` · `stash_list` · `stash_pop` · `stash_apply` · `stash_drop`.
Merge/rebase/cherry-pick: `merge` (up-to-date / fast-forward / conflict / merge-commit) · `conflict_files` · `abort_merge` (`reset --hard` + `cleanup_state`) · `conflict_versions(relpath) -> base/ours/theirs blob bytes` · `rebase(onto)` (non-interactive, replays commits, pauses on conflict) · `abort_rebase` · `cherry_pick(hash)` · `abort_cherry_pick`.

### `types` — wire/impl data types
`GitState`, `RepoState` (Clean/Merge/Rebase/RebaseInteractive/CherryPick/Revert/Bisect), `FileStatus` (+`marker()`), `DiffLine`/`DiffLineKind`, `HunkDiff`, `BlameEntry`, `LogEntry`, `MergeResult`, `RebaseResult`, `CherryPickResult`, `ConflictVersions`, `BranchInfo`, `StatusEntry`, `StashEntry`, `TagInfo`. Note these impl types do **not** derive `Serialize` — handlers build JSON ad hoc.

### `ipc` — wire-mirror types (serde + optional ts-rs/schemars)
Authoritative wire contract for shell plugin authors and the schema generator: `GitLogArgs`, `GitPathArgs`, `GitCommitArgs`, `GitBranchArgs`, `GitStashPushArgs`, `GitStashIndexArgs`, `GitHunkArgs`, `GitCreateTagArgs`, `GitPushArgs` (args); `GitStatusReply`, `GitLogEntry`, `GitBranch`, `GitDiffLine`, `GitDiffHunk`, `GitOk`, `GitCommitReply`, `GitFileStatus`, `GitFileDiff`, `GitStashEntry`, `GitTagInfo`, `GitBlameEntry` (replies). All use `#[serde(deny_unknown_fields)]`.

### `auto_commit` — `AutoCommitter`
`new(repo_root, debounce_secs)` · `check_and_commit() -> AutoCommitResult` (stages all + commits if dirty and past debounce; message generated from changed file names) · `reset_debounce` · `notify_save` (no-op placeholder).

### `worker` — `GitWorker` / `GitWorkerHandle`
`GitWorker::spawn(path)` opens the repo on a worker thread (surfacing `NotARepo` at spawn) · `handle()` mints a clone · `GitWorkerHandle::with(|engine| ...)` runs a closure on the worker thread and blocks. Bounded channel depth 32 (backpressure over unbounded queueing). `Drop` sends a `Shutdown` sentinel and joins the thread.

### `lfs` — Git-LFS staging routing (BL-091)
`is_lfs_tracked(cwd, path)` (shells `git check-attr filter`) · `stage_via_git_cli(repo_root, path)` (shells `git add` so the LFS `clean` filter runs). Degrade-gracefully when git isn't on `PATH`.

### `core_plugin`
`GitCorePlugin::new(forge_root, event_bus)`; constants `PLUGIN_ID`, the `HANDLER_*` ids, `MANIFEST_DEPS`, `IPC_HANDLERS` (the `(name, id)` table consumed by bootstrap), `DEFAULT_POLL_INTERVAL` (2s), `DEFAULT_AUTO_COMMIT_TICK` (30s), and `lfs_status_for_forge` (hidden CLI entry point).

## IPC handlers

All handlers route via `core_plugin::dispatch`. Args/returns are ad-hoc `serde_json` (mirrored by `ipc.rs` types). Capability column: none of these handlers perform an explicit kernel capability check inside the crate — see [Capabilities](#capabilities). Path args are validated with `nexus_types::paths::resolve_within(forge_root, path)`, rejecting `..` / absolute paths.

| id | command | args | returns | capability | description |
|----|---------|------|---------|------------|-------------|
| 1 | `status` | none | `{branch, head, is_dirty, repo_state}` (or `null` in passive mode) | — | Current repo state. |
| 2 | `log` | `{limit?: u64=20}` | `[{hash, author, date(rfc3339), message, parents}]` | — | Commit log from HEAD. |
| 3 | `branches` | none | `[{name, is_head, upstream}]` | — | Local branches. |
| 4 | `file_status` | `{path}` | status marker string (`"M"`, `"?"`, …) | — | One file's status. |
| 5 | `diff_file` | `{path}` | `[hunk]` (HEAD vs workdir+index) | — | Per-file working diff. |
| 6 | `stage_file` | `{path}` | `{ok:true}` | — | Stage one file (LFS-aware). |
| 7 | `unstage_file` | `{path}` | `{ok:true}` | — | Reset one file's index entry to HEAD. |
| 8 | `commit` | `{message}` | `{hash}` (short) | — | Commit the index. |
| 9 | `stage_all` | none | `{ok:true}` | — | Stage all changes. |
| 10 | `unstage_all` | none | `{ok:true}` | — | Reset index to HEAD. |
| 11 | `file_statuses` | none | `[{path, status}]` (Debug-string status) | — | All changed files. |
| 12 | `diff_staged` | none | `[{path, hunks}]` | — | Staged diff (index vs HEAD). |
| 13 | `switch_branch` | `{name}` | `{ok:true}` | — | Force checkout a branch (no dirty check). |
| 14 | `create_branch` | `{name}` | `{ok:true}` | — | Branch from HEAD. |
| 15 | `delete_branch` | `{name}` | `{ok:true}` | — | Delete a branch (not current HEAD). |
| 16 | `push` | `{remote, branch}` | `{ok:true}` | net + credential read | Push a branch. |
| 17 | `stage_hunks` | `{path, hunk_indices:[u64]}` | `{ok:true}` | — | Stage selected hunks via partial patch. |
| 18 | `unstage_hunks` | `{path, hunk_indices}` | `{ok:true}` | — | Unstage selected hunks (reversed patch). |
| 19 | `list_tags` | none | `[{name, target_hash, is_annotated, message}]` | — | Local tags, sorted. |
| 20 | `create_tag` | `{name, message?}` | `{ok:true}` | — | Annotated (message present) or lightweight tag at HEAD. |
| 21 | `delete_tag` | `{name}` | `{ok:true}` | — | Delete a local tag. |
| 22 | `push_tags` | `{remote}` | `{ok:true}` | net + credential read | Push `refs/tags/*`. |
| 23 | `stash_push` | `{message?}` | `{ok:true, index}` | — | Save working tree to stash. |
| 24 | `stash_list` | none | `[{index, message, oid}]` | — | List stash entries. |
| 25 | `stash_pop` | `{index?: usize=0}` | `{ok:true}` | — | Apply + remove a stash entry. |
| 26 | `stash_drop` | `{index?: usize=0}` | `{ok:true}` | — | Discard a stash entry. |
| 27 | `lfs_status` | none | `{tracked_patterns, pointer_files, available_files, git_lfs_installed}` | — | Inspect `.gitattributes` + `git lfs ls-files`. |
| 28 | `rebase` | `{onto}` | `{commits_rebased, conflicts}` | — | Non-interactive rebase onto a branch. |
| 29 | `abort_rebase` | none | `{ok:true}` | — | Abort in-progress rebase. |
| 30 | `cherry_pick` | `{commit}` | `{commit_hash, conflicts}` | — | Cherry-pick a commit onto HEAD. |
| 31 | `abort_cherry_pick` | none | `{ok:true}` | — | Abort in-progress cherry-pick. |
| 32 | `conflict_files` | none | `{files:[...]}` | — | Paths with unresolved conflicts. |
| 33 | `abort_merge` | none | `{ok:true}` | — | `reset --hard` + `cleanup_state`. |
| 34 | `conflict_versions` | `{path}` | `{base, ours, theirs}` (byte arrays or null) | — | Three index-side blob versions. |
| 35 | `merge` | `{branch}` | `{fast_forward, conflicts, commit_hash}` | — | Merge a branch into HEAD. |
| 36 | `blame` | `{path}` | `[{commit_hash, author, date, message, start_line, end_line}]` | — | Blame annotations. |
| 37 | `discard_hunks` | `{path, hunk_indices}` | `{ok:true}` | — | Revert selected workdir hunks to HEAD. |
| 38 | `file_log` | `{path, limit?: u64=20}` | `[log entry]` | — | Commit history for one file. |

(Bootstrap also registers `v1` aliases for these command names via `with_v1_aliases`.)

## Capabilities

This crate does **not** call the kernel capability API directly — there is no `Capability`/`ctx.check` usage in `crates/nexus-git/src/`. Operations are gated indirectly: the plugin is a trusted in-tree core plugin, IPC reachability is controlled at the kernel/dispatcher layer, and path arguments are sanitised with `nexus_types::paths::resolve_within` (rejecting traversal / absolute paths). The only credential access is the libgit2 credentials callback for network ops, which reads the OS keyring through `nexus_security::CredentialVault` (and the SSH agent / `~/.ssh` keys); the manifest dependency `com.nexus.security` formalises that relationship. The `net` / `credential read` markers in the IPC table above are logical descriptions of what `push` / `push_tags` / `pull` do, not in-crate capability checks.

## Settings / Config

Read from `[git]` in `<forge>/.forge/app.toml` by `read_git_settings`, via a minimal local mirror struct (`AutoCommitAppConfig`) so the crate avoids a `nexus-formats` dependency:

- `auto_commit: bool` (default `false`) — spawn the background auto-commit thread.
- `auto_commit_interval_secs: u64` (default `1800` = 30 min) — idle window before an auto-commit fires.
- `poll_interval_secs: Option<u64>` (default `DEFAULT_POLL_INTERVAL` = 2s) — git-state poller cadence (P2-06).
- `auto_commit_tick_secs: Option<u64>` (default `DEFAULT_AUTO_COMMIT_TICK` = 30s) — wake cadence within the auto-commit idle loop (P2-06).

A missing or unparseable `app.toml` falls back to defaults silently.

## Events

**Published** to the kernel `EventBus` by the background poller (`run_poller` → `publish_changes`, default every 2s) and the auto-committer:

- `com.nexus.git.state` — initial snapshot on first poll (`{branch, head, is_dirty, repo_state, tracking, upstream}`).
- `com.nexus.git.branch_changed` — `{from, to, head}` when the branch shorthand changes.
- `com.nexus.git.commit` — `{branch, head, prev_head}` when HEAD oid changes.
- `com.nexus.git.dirty_changed` — `{is_dirty, branch, head}` when the dirty flag toggles.
- `com.nexus.git.remote_changed` — `{branch, upstream, head, tracking, prev_tracking}` when the upstream tracking-branch oid changes (fetch/push detection; skipped on first observation). BL-052 follow-up.
- `ACTIVITY_APPENDED_TOPIC` (`nexus_types::activity`) — universal activity-timeline entries (origin `Git`, surface `Git`) for HEAD changes (`commit`), remote changes (`remote_changed`), and auto-commits (with a synthetic `ActivityToolCall` carrying the file count). BL-052.

**Subscribed:** the auto-commit thread subscribes to `EventFilter::CustomPrefix("com.nexus.storage.file_modified")` — each drained event refreshes an idle timer; after `auto_commit_interval_secs` of no modifications it stages all and commits.

## Internals & notable implementation details

- **libgit2 via `git2`.** All read/write ops are libgit2 calls. Short hashes are the first 7 chars of the oid. `state()` resolves the upstream tracking oid via `find_branch(...).upstream()`.
- **Repo discovery.** `Repository::open(path)` only — no parent traversal (deliberate, #85). Bare repos are unsupported (`repo_root` panics on a bare repo).
- **Diff/hunk handling.** `collect_hunks` / `collect_file_hunks` walk `git2::Patch` per delta. Partial staging (`stage_hunks` / `unstage_hunks` / `discard_hunks`) builds a synthetic unified-diff patch (`build_patch_for_hunks`) for the selected hunk indices — reversing prefixes and transposing `@@` counts for the unstage/discard direction — then `repo.apply(...)` to `ApplyLocation::Index` (stage/unstage) or `ApplyLocation::WorkDir` (discard). Out-of-range hunk indices are silently skipped; an empty patch is a no-op.
- **Blame** uses `repo.blame_file` and groups by `final_commit_id`, emitting one `BlameEntry` per contiguous line range with 1-based inclusive lines.
- **Merge/rebase/cherry-pick** mirror libgit2 semantics; conflicts are surfaced as path lists (`collect_conflict_paths`) rather than auto-resolved, leaving the working tree in the in-progress state for the caller to resolve or abort. `conflict_versions` reads stage-1/2/3 blob bytes directly from the index.
- **Credential callback** (`make_callbacks`): SSH agent → unencrypted default keys → keyring passphrase via `CredentialVault` (`ssh-passphrase:<key>`) → libgit2 default (HTTPS). A fresh vault is constructed per probe (cheap).
- **LFS staging** (`stage_file`): if the path exists and `is_lfs_tracked`, routes through `git add` (CLI) so the `clean` filter writes the pointer instead of raw bytes (BL-091 write-side twin). Deletes take the libgit2 `remove_path` fast path.
- **Threading model.** `GitWorker` confines the non-`Send` engine to one thread; `GitWorkerHandle::with` is the only way handlers touch the engine. Bounded channel (depth 32) gives backpressure; `Drop` sends `Shutdown` and joins.
- **Doc drift to flag.** The `worker.rs` module docs still claim "Git is an invoker-local capability — the kernel does not expose git as an IPC surface … `GitWorker` … does not become a core plugin." This is stale: `GitCorePlugin` *is* a registered core plugin with 38 IPC handlers wrapping exactly this worker. The implementation matches the rest of the crate; only the comment is out of date.

## Tests

- **`src/engine.rs` `#[cfg(test)]`** — `open` (repo/non-repo), `state` (fresh/after-commit), `file_statuses` (untracked/modified), `diff_file`, `blame`, `log`. Uses tempdir + libgit2 init.
- **`src/core_plugin.rs` `#[cfg(test)]`** — plugin-id constant, `on_init` in repo / non-repo, dispatch `status` (repo + passive null), unknown-handler error, passive non-status error, `on_start`/`on_stop` thread lifecycle, `publish_changes` (initial state, branch_changed, remote_changed on tracking advance, skip-on-first-observation), `state` upstream/tracking population, `blame` handler shape. Uses the system `git` binary for repo setup (disables `commit.gpgsign`).
- **`src/worker.rs` `#[cfg(test)]`** — spawn opens repo / errors on non-repo, handle clone + concurrent fan-out, drop closes channel → `WorkerGone`.
- **`src/auto_commit.rs` `#[cfg(test)]`** — commit when dirty/clean, debounce window, message format + truncation (`+N more`), non-repo error.
- **`src/lfs.rs` `#[cfg(test)]`** — `is_lfs_tracked` (filter detected / uncovered path / no gitattributes / non-lfs filter / outside repo), `stage_via_git_cli` (stages normal file / errors on nonexistent path). All gated on `git_available()`.
- **`src/error.rs` `#[cfg(test)]`** — Display strings for `NotARepo` / `FileNotFound`.
- **`tests/integration.rs`** — end-to-end `GitEngine` against real repos: `full_lifecycle` (empty → commits → status → diff → log → log_file → blame), `open_non_repo`, `log_limit`, staging/commit workflow, branch create/switch/delete, `unstage_all`, merge (fast-forward / merge-commit / conflicts / abort), local push+pull through a bare repo, auto-commit workflow, rebase (clean replay / conflict + abort), cherry-pick (clean / conflict + abort), `conflict_versions` (three sides / clean-file error).
