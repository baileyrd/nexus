# Terminal Integration Assessment
_Assessed: 2026-05-06_

## Overall: 7.5/10 — Production-ready foundation, strong architecture, weak composability

The terminal subsystem is one of the most thoroughly engineered parts of Nexus. The infrastructure
is genuinely deep. What it lacks isn't implementation quality — it's *wiring* into the rest of the
system, particularly the AI and agent layers.

---

## What's fully implemented and first-class

**PTY and session management.** `Session` wraps `portable-pty` with a dedicated reader thread
(blocking I/O on a background thread, non-blocking snapshots for callers), proper
`SIGINT → SIGTERM → SIGKILL` signal ladder, Unix process groups, Windows Job Objects, and
SIGWINCH-based resize. `SessionManager` handles up to 50 concurrent sessions with 10 MB ring
buffers per session.

**Process state machine.** `ManagedState` (Stopped → PreCommand → Starting → Running → Crashed
→ Restarting) is a clean, validated FSM with exponential backoff (`[2s, 5s, 10s]` default) and
configurable `max_restart_attempts`. Pre-commands run in a live shell with state inheritance (`cd`,
`export` carry forward) and sentinel-based exit code capture — a detail that's easy to get wrong
and hard to notice when it is.

**Output buffering.** Two-layer design: `OutputBuffer` (raw byte ring, ANSI codes intact, for
streaming to xterm.js) and `LineBuffer` (ANSI-stripped structured lines with deduplication for
spinners/progress bars, for search/history). Substring and regex search both implemented; benchmarks
show sub-10ms on 100K-line buffers.

**Persistence.** SQLite for session metadata and saved commands (`procmgr_commands` table).
Scrollback blobs written to `.forge/.terminal/<id>/scrollback.bin`. Ad-hoc command history with
run-count deduplication and status tracking. Command promotion (ad-hoc → saved) implemented.

**Saved commands as first-class objects.** The `SavedCommand` schema is richer than most: slug,
name, shell, shell_cmd, working_dir, env_vars (HashMap), env_file, icon, auto_restart,
auto_restart_delay_ms, memory_limit_mb, sidebar_order, pre_commands. Full CRUD exposed as IPC
handlers 11–15, CLI via `nexus proc`, UI via `SavedCommandsView`.

**Desktop and TUI both wired.** `TerminalView.tsx` renders via xterm.js with per-token kernel-bus
event streaming (`com.nexus.terminal.output.<id>`). TUI has a `Mode::Terminal` pane. Both reach
through IPC — no direct `nexus-terminal` linkage anywhere in consumers.

**18 IPC handlers, 2 background threads, 239 unit tests.** The autonomous drainer thread pumps all
sessions every 5ms and publishes byte-stream events. The lifecycle forwarder bridges in-process mpsc
to the kernel bus. Microkernel discipline is clean: the library has zero kernel bus knowledge; the
plugin is the only boundary.

---

## Where it's second-class

### 1. Not composable with agents or workflows

This is the biggest gap. `com.nexus.terminal` is not in the agent's tool registry.
`com.nexus.workflow` has no terminal step type. The AI suggestion engine (`ai.rs`) has 5 built-in
rules (cargo compile failure, npm package not found, command not found, git SSH key, port in use)
but the LLM bridge is unbuilt — suggestions are pattern-matched strings, not routed through
`com.nexus.ai`.

An agent that needs to run a dev server, execute a build command, or send a signal to a process has
no IPC path to do it. That means the "AI that operates the forge" vision is incomplete — the
terminal is the most common execution surface for developer work and it's not in the agent's hands.

### 2. URL extraction is a library ghost

`urls.rs` (410 lines) detects HTTP(S), FTP, SSH, and `file://` URLs with regex, classifies them by
kind, and has a `resolve_url` stub for opening them. None of this is wired to the shell UI. The
CommandBook URL-pin pattern — top-5 links always visible above the output pane, single-click, never
hidden by scrolling — is the most distinctive UI idea in that app. Nexus has the library for it but
no surface.

### 3. Ad-hoc history not IPC-exposed

`SqliteAdHocStore` and its deduplication logic are shipped. The IPC handlers for it are not. Users
can't browse or search their command history from the shell UI or CLI.

### 4. Activity timeline not connected

`BL-052` (universal activity timeline) calls for terminal process events (`process started`,
`command exited`, `crash detected`) to flow through the same `com.nexus.activity.appended` topic as
AI tool calls. This hookup doesn't exist. A running forge can have AI, agents, and terminal
processes all active, and the user can only audit the AI half.

### 5. Memory backpressure is monitoring, not policy

`MemoryMonitor` tracks RSS per process and exposes `SoftExceeded`/`HardExceeded` thresholds.
Nothing reads those thresholds and acts on them. A long-running process that leaks memory will
accumulate indefinitely.

### 6. No "open in external terminal" escape hatch

CommandBook's `Run in Terminal` (routing a command to iTerm2/Warp/Ghostty when PTY interactivity is
needed) isn't in the PRD-09 spec and isn't in the codebase. For a forge-integrated process manager,
this is a real gap: users who need `vim`, `htop`, or an interactive REPL have no path back to the
forge's saved command context while using a real terminal. Flagged as a BL candidate in the
CommandBook evaluation (`docs/research/commandbook-evaluation.md`).

### 7. Sidebar hover buttons missing

`SavedCommandsView` has run/copy buttons but not the CommandBook-style contextual hover buttons
(start/stop/restart/dismiss inline on each sidebar row). Minor friction but consistently present in
every "feels first-class" process manager UI.

---

## Scorecard

| Dimension | Score | Notes |
|---|---|---|
| PTY fundamentals | 9/10 | Session + manager solid; process groups cross-platform |
| Output buffering | 9/10 | Ring buffer + line buffer + ANSI stripping all present |
| Process state machine | 8/10 | FSM complete; pre-commands working; Windows partial |
| Session persistence | 8/10 | SQLite metadata + scrollback blobs; LRU policy pending |
| Saved commands | 9/10 | Full CRUD + IPC + UI; "run saved" execution handler pending |
| Microkernel integrity | 10/10 | Single boundary, zero leakage, IPC-only consumers |
| Desktop UI | 7/10 | xterm.js + ANSI + sidebar present; URL chips + hover buttons missing |
| Agent/workflow composability | 4/10 | Not in tool registry; no workflow step type; LLM bridge missing |
| Activity timeline integration | 3/10 | Not connected; no terminal event emitters on the bus |
| URL extraction surface | 2/10 | Library complete; UI surface doesn't exist |

---

## The core gap in one sentence

The terminal service is a first-class citizen **within its own subsystem** — the PTY layer, state
machine, persistence, and IPC surface are all deeply implemented — but it's a **second-class
citizen across the system**, because agents can't dispatch to it, workflows can't trigger it, the AI
can't observe it, and the activity timeline doesn't see it.

---

## How to close the gap

Closing it doesn't require new infrastructure. It requires three targeted wiring tasks:

**1. Add terminal commands to the agent tool registry** (highest leverage)

Register `com.nexus.terminal::run_saved` (start a saved command by slug), `com.nexus.terminal::send_input`
(write to a running session), and `com.nexus.terminal::get_session_info` (check process status) as
agent-callable tools. The library and IPC handlers already exist; the tool registry entry is the
only missing piece. With this, an agent can start a dev server, check if it's running, and send
SIGINT — the most common automation pattern in any development workflow.

**2. Add a `terminal` step type to the workflow executor** (medium leverage)

Add `type = "terminal"` alongside the existing `ipc` step type in `nexus-workflow/src/executor.rs`.
It maps to `com.nexus.terminal::run_saved` with the slug as the step argument. Foundation-class
workflows (always-on dev services) and capability-class workflows (build triggers, test runners)
both need this.

**3. Emit `com.nexus.activity.appended` on process lifecycle events** (required for parity with AI)

The `TerminalCorePlugin` already has a lifecycle forwarder thread that publishes to
`com.nexus.terminal.events.<id>`. Add a parallel publish to the universal activity topic with a
`origin: "terminal"` discriminator on `SessionCreated`, `ProcessCrashed`, and `SessionClosed`
events. This makes the activity timeline complete.

**4. Wire URL chip extraction to the shell UI** (UX win, low effort)

The `urls.rs` library is complete. Add a `useUrlExtraction` hook in `TerminalView.tsx` that
subscribes to the output stream, calls `detect_urls` on new lines, and maintains a top-5 deduped
list pinned above the output pane. One afternoon of work.

---

## Key source files

```
crates/nexus-terminal/src/core_plugin.rs   — 18 IPC handlers, drainer + forwarder threads
crates/nexus-terminal/src/session.rs       — PTY wrapper, signal ladder, reader thread
crates/nexus-terminal/src/manager.rs       — SessionManager, 50-session cap
crates/nexus-terminal/src/procmgr.rs       — ManagedState FSM, backoff schedule
crates/nexus-terminal/src/precmd.rs        — pre-command runner, sentinel exit codes
crates/nexus-terminal/src/saved.rs         — SavedCommand schema + SQLite CRUD
crates/nexus-terminal/src/persist.rs       — session metadata + scrollback blobs
crates/nexus-terminal/src/buffer.rs        — OutputBuffer ring
crates/nexus-terminal/src/lines.rs         — LineBuffer, ANSI stripping, dedup
crates/nexus-terminal/src/urls.rs          — URL detection (library only, not wired to UI)
crates/nexus-terminal/src/ai.rs            — AiSuggestionEngine (5 rules, LLM bridge missing)
crates/nexus-terminal/src/memory.rs        — MemoryMonitor (monitoring only, no policy)
shell/src/plugins/nexus/terminal/TerminalView.tsx       — xterm.js render + event streaming
shell/src/plugins/nexus/terminal/SavedCommandsView.tsx  — sidebar CRUD UI
crates/nexus-cli/src/commands/term.rs      — nexus term env / run / shell
crates/nexus-cli/src/commands/proc.rs      — nexus proc list / add / delete / reorder
```
