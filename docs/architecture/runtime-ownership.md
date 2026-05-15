# Tokio Runtime Ownership

A catalogue of every place in the Nexus codebase that builds, captures, or
spawns onto a tokio runtime. Lives here so that "is the system idle?" has a
real answer — today, several subsystems quietly build or assume different
runtimes and there is no single owner. Until a structural fix lands (likely
under [BL-134](../PRDs/BACKLOG.md) Phase 1's runtime consolidation), the rule
of thumb is: **frontends own a runtime; subsystems must use the ambient one
via `Handle::try_current()` and degrade gracefully when it's absent**.

Filed under [BL-137](../PRDs/BACKLOG.md#bl-137-architectural-review-2026-05-14-follow-ups)
("Document tokio runtime ownership").

## Frontend-owned runtimes

These are the **canonical** runtimes. Each frontend builds exactly one,
holds it for the process lifetime, and tears it down at exit.

| Site | Shape | Notes |
|------|-------|-------|
| `crates/nexus-cli/src/app.rs:64` | `Builder::new_multi_thread().worker_threads(1)` | One-shot CLI commands. Single worker is enough; IPC handlers are short-lived. Built lazily inside `App::runtime` so subcommands that don't touch the kernel (e.g. `forge init`) skip the cost. |
| `crates/nexus-tui/src/app.rs:705` | `Builder::new_multi_thread().worker_threads(1)` | Long-running TUI. Same shape as the CLI runtime; dream-cycle scheduler is spawned onto it via `nexus_bootstrap::dream_cycle::spawn`. |
| `crates/nexus-cli/src/commands/mcp.rs:39` | `Runtime::new()` (default multi-thread) | `nexus mcp serve` subcommand. Distinct from the CLI's main runtime because the MCP server takes ownership of the context for its lifetime; the CLI's lazy runtime is built before the MCP server replaces it. |
| Tauri shell (`shell/src-tauri/`) | Tauri's own multi-thread runtime | Constructed by `tauri::Builder::default().run(...)`. All `#[tauri::command]` invocations execute on this runtime; the kernel + plugins are booted under it via `bridge::boot_kernel`. |

## Subsystem-owned runtimes (in-tree exceptions)

Subsystems that build their **own** runtime are exceptions to the rule and
must justify it.

| Site | Why a dedicated runtime exists |
|------|---|
| `crates/nexus-ai-runtime/src/pool.rs:50` | The AI runtime worker pool (BL-134 / ADR 0028). Isolates long-running model inference + retrieval from the frontend runtime so a stuck completion can't starve UI IPC. Worker threads are named `nexus-ai-worker-N` for diagnostics. **Owned justification:** explicit scheduler with its own back-pressure and metrics. |

## Ambient runtime consumers (`Handle::try_current()`)

These subsystems read the ambient runtime when one is present and degrade
to a no-op or synchronous path when called from a context without tokio.

| Site | Behaviour when ambient runtime is absent |
|------|---|
| `crates/nexus-notifications/src/core_plugin.rs:615` | Skips spawning the AI-runtime event subscriber. The BL-134 `AiEvent` stream only matters when a long-running frontend (shell, TUI) is in front of the kernel; CLI single-shot calls don't need it. |
| `crates/nexus-audio/src/provider_backend.rs:290` | Audio provider backend; falls back to blocking I/O. |
| `crates/nexus-workflow/src/core_plugin.rs:359` | Cron scheduler — logs a warning and disables cron triggers. The plugin still loads; only time-based triggers are inert. |
| `crates/nexus-workflow/src/core_plugin.rs:416, 465, 899, 1119, 1240` | Workflow-side async dispatch sites; same fallback pattern. |

The pattern is intentional: a plugin must not crash because it was loaded
under the CLI's single-shot runtime instead of the TUI's persistent one.

## `spawn_blocking` consumers

`spawn_blocking` is the bridge from async IPC handlers into synchronous
subsystem code that holds non-`Send` state (libgit2, fastembed, PTY
sessions).

| Site | What it wraps |
|------|---|
| `crates/nexus-kernel/src/context_impl.rs:166` | The kernel-level IPC dispatcher's fallback for plugins that registered a sync `dispatch` rather than `dispatch_async`. Bounded by the per-call timeout; a stuck handler returns `IpcError::Timeout` rather than wedging the runtime. |
| `crates/nexus-git/src/worker.rs:17` (module doc) | `GitWorker` runs `git2::Repository` on a dedicated OS thread because libgit2 state is not `Send` or `Sync`. Async callers are expected to `spawn_blocking` around `handle.with(...)`. |
| `crates/nexus-terminal/src/lib.rs:35` (module doc) | PTY session loop is sync; the procmgr IPC handler wraps reads in `spawn_blocking`. |
| `crates/nexus-ai/src/local_embedding.rs:60` (doc) | fastembed's `TextEmbedding::embed` is `&mut self` and holds non-`Send` ORT session state. Callers wrap each embed in `spawn_blocking`. |
| `crates/nexus-notifications/src/lib.rs:238` (comment) | Documentation note that the inbox SQLite writes should be wrapped by callers when invoked from async. |
| `crates/nexus-plugins/src/loader.rs:31` (comment) | Notes the loader's sync dispatch path; the kernel above handles the `spawn_blocking` wrap. |

## Direct `tokio::spawn` sites

These are the few places where in-tree code spawns its own task instead of
returning a future for the caller to drive. Each owns its task lifecycle
(stored handle, cancellation token, or known-short body).

| Site | Spawned task |
|------|---|
| `crates/nexus-lsp/src/client.rs:217, 238` | LSP transport reader + writer loops, one per spawned server. Cancelled by `ConnectionPool::shutdown`. |
| `crates/nexus-workflow/src/core_plugin.rs:1334` | Workflow async-step coordinator. Tied to the workflow scheduler's lifetime. |
| `crates/nexus-crdt/src/sync.rs:299` | CRDT sync loop. |
| `crates/nexus-cli/src/commands/ai.rs:585` | CLI-side `stream_chat` consumer; task ends when the stream closes. |
| `crates/nexus-bootstrap/src/dream_cycle.rs` | Dream-cycle scheduler worker. Stored in `DreamCycleScheduler` and joined on drop. |

## What this catalogue is not

It is not a contract — subsystems can be added or removed without updating
this file. It is a snapshot of the current spawn topology to make the
implicit ownership visible and to ground the BL-134 Phase 1 conversation
about consolidating runtime ownership behind a single entry point.

If you add a new spawn site, prefer **ambient + graceful degrade** over a
new dedicated runtime. Reach for a dedicated runtime only when you can
justify the isolation (see `nexus-ai-runtime` for the bar).
