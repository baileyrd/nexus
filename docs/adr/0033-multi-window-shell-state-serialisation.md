# ADR 0033: Multi-Window Shell-State Serialisation — Advisory Mutex

**Date:** 2026-05-23
**Status:** Accepted (implemented 2026-05-21 via `d1aa8b82`)
**Related:** BL-029 (popout windows), commit `d1aa8b82`, ADR 0011 (plugin-first shell)

## Context

The Tauri shell persists its configuration in `shell-state.json`, located under the application config directory. The state tracks: the last forge path opened, remote recent items, window geometry, session state, and other per-user runtime data.

When the legacy shell was retired (ADR 0011), migration introduced four Tauri commands that read-modify-write this file:

1. `write_last_forge_path` — updates the last-opened forge path
2. `forget_forge_path` — clears the last-opened forge path
3. `write_remote_recent` — appends/approves a remote forge to the recent list
4. `forget_remote_recent` — removes a remote forge from the recent list

Each command follows the **load-modify-save** pattern:
1. `fs::read_json` → `ShellState`
2. Apply mutation in memory
3. Write to a temp file, then `fs::rename` (atomic overwrite)

The atomic `rename` protects against a half-written file (power loss, crash) but **does not protect against concurrent writers**. In a multi-window Tauri application:

- Two windows can call the same command simultaneously.
- Each loads the same baseline `ShellState`.
- Each applies its own mutation independently.
- Both race the atomic `rename`.
- The second writer's mutation silently **overwrites** the first call's edit.

This is a classic lost-update race condition. It only manifests when a user has multiple windows open and interacts with shell state (open forge, forget forge, etc.) across them.

## Decision

Add a **module-scoped advisory mutex** (`SHELL_STATE_WRITE_LOCK`) and factor the load-modify-save pattern into a `with_lock_update` helper. The helper holds the lock across all three steps (load → modify → save), ensuring serialised writes.

```
Tauri Command A ────> with_lock_update ──> [load] ──> [modify] ──> [save + rename] ──> return
Tauri Command B ────> with_lock_update ──> waits for lock
```

`s save_shell_state` (the full-state write used by frontend code) takes the same lock, so frontend-driven full-state writes cannot interleave with backend read-modify-write operations.

The lock is **process-local** (`std::sync::Mutex`), which is sufficient because `shell-state.json` lives under the app's config directory and is only ever written by this process's Tauri commands.

If a future change adds an external writer (an out-of-process helper, say), promote to a file lock or use a shared-mutex-compatible type that can cross process boundaries.

### Poison-recovery

If the mutex is poisoned (which can arise from a future panic inside `with_lock_update`), the code falls back to the inner guard with an `eprintln` warning rather than propagating an `Err` the Tauri command cannot usefully return — `save_to` itself does not panic, so poisoning would only arise from a future bug. The runtime stays alive.

### Regression guard

Added the test `concurrent_updates_do_not_lose_mutations` which spawns 16 threads each pushing a distinct entry through `with_lock_update`, asserting every entry survives in the post-state. Without the lock the test fails with the second writer clobbering the first.

## Consequences

### Positive

- **Silent data loss eliminated.** Concurrent writes to shell-state no longer lose mutations.
- **Minimal surface area.** One lock, one helper function. The pattern is explicit and documentable.
- **Backward-compatible.** No changes to the `ShellState` type, the file format, or the Tauri command signatures.

### Negative / costs

- **Process-local only.** If shell-state is ever written by another process (e.g., a config-sync daemon, a background migration tool), this lock provides no protection. The guard on `SHELL_STATE_WRITE_LOCK` flags this boundary for future migration.
- **Serialisation of the entire load-modify-save.** The lock is held across the file read and write, so concurrent readers wait unnecessarily. If read concurrency becomes a profiling bottleneck (unlikely given the small file size and infrequent writes), migrate to a reader-writer lock.
- **Poison-recovery masks bugs.** If a panic inside `with_lock_update` is later introduced (e.g., a panic-inducing code path through serde), the first call to the command silently recovers with stale data rather than surfacing the underlying bug.

## Open follow-ups

1. **Monitor poison events.** If any `eprintln` from the poison-recovery path surfaces in user reports, investigate immediately — it indicates a panic in the inner guard.
2. **Test migration.** When/if shell-state is ever written by an external process, replace the process-local mutex with a file lock (`filelock` crate) and ensure the migration test covers the cross-process case.
3. **Consider moving shell-state into workspace.** The audit (ADR 0032) flagged the shell's exclusion from the Cargo workspace as a risk. If shell-state moves into the workspace, the lock could be tested alongside the service crates.
