# com.nexus.git

- **Path:** `crates/nexus-git/`
- **Tier:** Core Rust
- **Bootstrap order:** 7

## Architecture

- Entry point: `crates/nexus-git/src/core_plugin.rs` — `GitCorePlugin` implements `nexus_plugins::CorePlugin`.
- Key modules: `engine.rs` (libgit2 wrapper), `worker.rs` (thread-confined `GitWorker` — libgit2 isn't `Send`), `handlers/` (one file per category: `status`, `log`, `branches`, `staging`, `stash`, `tags`, `merge`), `auto_commit.rs`, `lfs.rs`, `ipc.rs` (wire-mirror types with optional `ts-rs` / `schemars` derives).
- Manifest is built from `IPC_HANDLERS` in `core_plugin.rs`; bootstrap wraps it via `core_manifest_with_ipc("com.nexus.git", "Git", ...)` in `crates/nexus-bootstrap/src/plugins/git.rs`. Lifecycle: `on_init` + `on_start` + `on_stop`.
- `on_init` opens the repo via `GitWorker::spawn(forge_root)`. If the forge root is not a git repository, the plugin enters **passive mode** — `HANDLER_STATUS` returns JSON null, every other handler returns `ExecutionFailed`.
- `on_start` spawns a `nexus-git-poller` thread that re-reads `git status` on a tick and publishes diff events to the bus; optionally spawns a `nexus-git-auto-commit` thread when `[git] auto_commit = true`.
- Persistence: the plugin writes nothing of its own to `.forge/`. All state lives in the underlying `.git/` directory; reads `<forge>/.forge/app.toml` for the `[git]` settings block.
- Bus topics published: `com.nexus.git.state`, `com.nexus.git.branch_changed`, `com.nexus.git.commit`, `com.nexus.git.dirty_changed`, `com.nexus.git.remote_changed`, plus `ActivityEntry` envelopes on the shared `ACTIVITY_APPENDED_TOPIC` for commit / remote events.
- External dependencies: `git2` (libgit2 native), optional Git-LFS (resolved via `.gitattributes` walk + `git lfs` shellout detection in `lfs.rs`).

## Surface

37 IPC handlers (full list at `crates/nexus-git/src/core_plugin.rs:223`):

`status`, `log`, `branches`, `file_status`, `diff_file`, `stage_file`, `unstage_file`, `commit`, `stage_all`, `unstage_all`, `file_statuses`, `diff_staged`, `switch_branch`, `create_branch`, `delete_branch`, `push`, `stage_hunks`, `unstage_hunks`, `stash_push`, `stash_list`, `stash_pop`, `stash_drop`, `list_tags`, `create_tag`, `delete_tag`, `push_tags`, `lfs_status`, `rebase`, `abort_rebase`, `cherry_pick`, `abort_cherry_pick`, `conflict_files`, `abort_merge`, `conflict_versions`, `merge`, `blame`, `discard_hunks`, `file_log`.

## Necessity

- **Verdict:** Essential
- **Required for basic capabilities?** Yes — the minimum-viable spec calls out "commit and push via git" as a basic-capability operation, and that path goes through `commit` + `push` on this plugin. The shell's `gitStatus` and `gitPanel` plugins are the only routes to those operations.
- **Depended on by:** shell-nexus `gitPanel` / `gitStatus`, `com.nexus.editor` (BL-007 pull-landing CRDT subscriber listens on `com.nexus.git.commit`), shell `activityTimeline` (via `ACTIVITY_APPENDED_TOPIC`), `nexus-mcp` (exposes git tools to LLMs).
- **Depends on:** `nexus-kernel` (event bus + IPC), `nexus-plugins`, `nexus-security`, `nexus-types`. No other service plugin.
- **What breaks if removed:** the shell's git panel and status indicators go dark; auto-commit stops; `nexus-editor`'s pull-landing CRDT merger has no trigger event; users lose every in-app path to staging, diffing, committing, and pushing — the workflow the basic-capability list calls out by name.

## Notes

- Passive-mode return-`null`-for-status path (`core_plugin.rs:398`) is deliberate so a non-git forge still mounts the shell without firing the `PluginCrashedDuringCall` error in `gitStatus`.
- `lfs_status` is a best-effort working-tree scan + `git lfs` binary probe — it returns shape-stable JSON whether or not LFS is installed.
- BL-052 wires git into the universal activity timeline by publishing `ActivityEntry` payloads alongside the `com.nexus.git.*` events.
- `auto_commit` is opt-in and gated on `[git] auto_commit = true` in `app.toml`; default `auto_commit_interval_secs = 1800`. Poll interval defaults to `DEFAULT_POLL_INTERVAL = 2s` (`core_plugin.rs:267`); these are listed in `docs/0.1.2/settings/forge-config.md`.
