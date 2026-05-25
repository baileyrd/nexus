# nexus-tui

> Kind: lib + bin (`nexus-tui`) · IPC plugin id: — · CorePlugin: no · As of: 2026-05-25

## Overview

`nexus-tui` is the terminal-UI frontend for Nexus, built on [ratatui](https://crates.io/crates/ratatui) + [crossterm](https://crates.io/crates/crossterm). It draws a two-pane IDE — a forge file tree on the left, a markdown viewer on the right — plus a set of right-pane overlays (full-text search, in-file find, task list, backlinks, an in-app terminal, an AI chat panel, an agent panel, and a kernel-metrics modal). It is a thin presentation layer: it owns no storage, no AI logic, no git logic beyond reading branch state. Everything that touches the forge goes through the kernel.

Like every other frontend, the TUI consumes `nexus-bootstrap`. `TuiApp::new` calls `build_tui_runtime(forge_root)` to get a `Runtime` (kernel + all registered core plugins + an invoker context), and every data operation is a `context.ipc_call(plugin_id, command, args, timeout)` or a typed wrapper around it from `nexus_bootstrap::storage` / `nexus_bootstrap::terminal`. There are no direct subsystem calls and no `CorePlugin` of its own — the TUI is purely an **IPC caller**. This is the microkernel discipline applied to the frontend: the same handlers the CLI, MCP server, and Tauri shell hit are the handlers the TUI hits.

The crate is usable two ways (documented in `lib.rs`): as the standalone `nexus-tui` binary (`main.rs` is a 9-line wrapper around `run_tui()`), and as a library entry point called by `nexus-cli`'s `nexus tui` subcommand without subprocess overhead. All terminal setup/teardown (raw mode, alternate screen, mouse capture, ratatui init/restore) is self-contained inside `run_tui` behind an RAII guard, so it is safe to call from a dispatcher that runs unrelated code before and after.

The render model is synchronous-draw-over-async-data. The event loop is a blocking poll/draw loop on the main thread; async `ipc_call`s are driven through a small multi-threaded tokio runtime via `rt.block_on` for one-shot fetches (file reads, search, tasks, status) and via `rt.spawn` + non-blocking pumps for long-running streams (AI streaming, agent sessions, the PTY). Live streaming and agent events arrive over the kernel event bus, which the loop drains between frames.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-bootstrap` (`build_tui_runtime`, `Runtime`, the `storage` and `terminal` typed IPC helper modules, `dream_cycle`), `nexus-kernel` (`Ipc` trait, `EventBus`/`EventFilter`/`EventSubscription`, `NexusEvent`), `nexus-types` (`constants::KERNEL_BLOCKING_POOL_SIZE`), `nexus-git` (`GitEngine` — read-only branch/dirty state for the status bar).
- **Notable external deps:** `ratatui` + `crossterm` (TUI + terminal control), `tokio` (multi-thread runtime to block on / spawn IPC), `uuid` (per-submit AI stream session ids), `anyhow`, `serde`/`serde_json`, `tracing`/`tracing-subscriber` (file-only logging).
- **What depends on it:** `nexus-cli` launches it via the `nexus tui` subcommand arm, importing `run_tui` directly. Nothing depends on the TUI as a library beyond that. The kernel never depends on it (microkernel invariant 2).

## Public API surface / module layout

The crate's only public symbol is `run_tui() -> Result<()>` in `lib.rs`. Everything else is crate-private. Module map:

- **`lib.rs`** — public `run_tui`; forge-path resolution (`resolve_forge_path`); file-tracing init (`init_file_tracing`); `TerminalGuard` (RAII raw-mode/alt-screen/mouse setup + restore on drop); the `run` event loop (`terminal.draw` → `event::poll(16ms)` → `input::handle_event`, then the terminal/AI/agent pumps, then the quit check).
- **`main.rs`** — binary entry point; calls `nexus_tui::run_tui()`.
- **`app.rs`** — all application state. `TuiApp` (the god-struct) plus per-feature state structs and the helper methods that fire IPC: `Mode`, `Focus`, `TreeEntry`/`TreeState`, `ViewerState`, `SearchState`, `FindState`, `BacklinksState`, `KernelStatsState`, `TaskEntry`/`TaskViewState`, `StatusInfo`, `TerminalPanelState`, `AiMessage`/`AiRole`/`StreamingSession`/`AiPanelState`, `ProposedToolCall`/`PendingApproval`/`AgentLine`/`AgentLineKind`/`AgentSession`/`AgentPanelState`. Free helpers: `classify_round`, `parse_round_proposed`, `is_modal_expired`, `render_session_into_transcript`, `extract_stream_ask_text`.
- **`input.rs`** — crossterm event dispatch. `handle_event` routes by `Mode` (and intercepts the approval-modal keys first); per-mode handlers (`handle_normal_key`, `handle_tree_key`, `handle_viewer_key`, `handle_search_key`, `handle_find_key`, `handle_terminal_key`, `handle_ai_input_key`, `handle_agent_input_key`); `handle_mouse` (scroll); `open_in_editor` (suspend TUI, spawn `$VISUAL`/`$EDITOR`/`vi`, resume, reload); pure helper `key_to_approval_decision`.
- **`streaming.rs`** — pure parsing of `com.nexus.ai.stream_*` bus payloads: `STREAM_START_TOPIC`/`STREAM_CHUNK_TOPIC`/`STREAM_DONE_TOPIC` constants, `parse_chunk_event`, `parse_done_event`, `matches_start_event`. All `session_id`-filtered so concurrent streams don't cross-contaminate.
- **`ui/mod.rs`** — `render(frame, app)`: the master layout (body + 1-line status bar; body split 25%/75% tree/right). Right-pane priority: agent > AI > terminal > tasks > viewer(+backlinks split). Overlays layered on top: find bar, search popup, kernel-stats modal, approval modal. `centered_rect` helper.
- **`ui/file_tree.rs`** — left-pane tree (icons, indentation, focus-colored border; substring-filters file entries while the search overlay has a query).
- **`ui/viewer.rs`** — right-pane file viewer with line-number gutter and hand-rolled markdown highlighting (`highlight_line` block-level, `highlight_inline` for `` `code` ``, `[[wikilinks]]`, `#tags`).
- **`ui/status_bar.rs`** — bottom line: mode badge, git branch + dirty flag, file path, scroll position, file/link/task counts, help hint.
- **`ui/terminal.rs`**, **`ui/ai.rs`**, **`ui/agent.rs`**, **`ui/agent_approval.rs`**, **`ui/kernel_stats.rs`**, **`ui/backlinks.rs`**, **`ui/tasks.rs`** — one renderer per overlay (see Views below).

## Views / screens

The right pane shows exactly one of these at a time, chosen by priority in `ui::render`: **agent > AI > terminal > tasks > viewer**. Overlays (search popup, kernel-stats modal, approval modal) and the find bar layer on top.

| View | What it shows | IPC handler(s) | Key interactions |
|------|---------------|----------------|------------------|
| **File tree** (left, always) | Forge files + synthesised directory entries from the storage index; expand/collapse, icons, focus border. Filters to matching files while the search overlay has a query. | `com.nexus.storage` `query_files` (via `ipc::query_files`, on startup and `refresh_tree`) | j/k/↑/↓ move; Enter/l/→ open file or expand dir; h/← collapse; Tab to viewer |
| **Viewer** (default right) | Currently opened file, line-number gutter, markdown highlight. Title = file path or `Preview`. | `read_file` (via `ipc::read_file`) on open | j/k scroll; g/Home top; G/End bottom; Ctrl+D/Ctrl+U page; e open in `$EDITOR`; mouse scroll |
| **Search overlay** (popup) | Centered 60%×50% popup: query line + ranked FTS results; selecting opens the top hit. Also live-filters the tree. | `search(query, 50)` (via `ipc::search`); `read_file` for the top result | typing edits query; Enter runs search + opens top hit; ↑/↓ move selection; Esc closes |
| **Find bar** (1-line, under viewer) | In-file case-insensitive substring matches with `current/total` counter; computed locally over `viewer.lines` (no IPC). | none (pure client-side) | typing updates matches; Enter/n next; N prev; Esc closes |
| **Task list** (right) | All tasks: checkbox, content, `path:line`; title shows pending/total. | `query_tasks(TaskFilter::default())` (via `ipc::query_tasks`) | `t` toggles; list is read-only (no completion toggle wired) |
| **Backlinks** (bottom 30% of right, under viewer) | Files linking to the current file: source path (cyan) + link text (gray). | `backlinks(path)` (via `ipc::backlinks`); refreshed on file open | `b` toggles; refreshes on open while visible |
| **Terminal** (right) | In-app PTY: ANSI-stripped output (last N lines) + a line-buffered `$` prompt; title shows short session id, line count, and a 5-entry key-log. | `com.nexus.terminal` via `nexus_bootstrap::terminal`: `create_session`, `pump(50ms)`, `read_output`, `send_input`, `send_raw_input`, `close_session` | `T` opens + enters Terminal mode; typing line-buffers; Enter sends line; Backspace edits; Ctrl+C sends `\x03`; Ctrl+D kills session; Esc hides (session survives) |
| **AI chat** (right) | Transcript (user/assistant turns, plain text) + status line (`thinking…`/`streaming…`/error) + prompt. Header shows provider/model. | `com.nexus.ai` `status` (header), `stream_ask` (submit; full transcript sent as `messages`, last user msg is the RAG question); streamed via `com.nexus.ai.stream_*` bus events | `a` toggles + enters AiInput; typing edits; Enter submits (non-blocking); Esc leaves input; ↑/↓ no longer wired to scroll (auto-pins to bottom) |
| **Agent** (right) | Goal/round/tool-call/outcome transcript with per-kind styling; status line (`agent running…`/`awaiting approval`/error). | `com.nexus.agent` `session_run` (`auto_approve=false`), `round_decide`; observes `com.nexus.agent.*` bus events incl. `round_proposed` | `g` toggles (when focus ≠ viewer) + enters AgentInput; typing edits goal; Enter submits (drops to Normal); Esc leaves input |
| **Approval modal** (centered popup, top-most) | Per-tool-call rows with `DESTRUCTIVE`/`UNREGISTERED`/`safe` badges, model text, and an auto-reject countdown. Mirrors the CLI interactive prompt. | drives `round_decide` on decision | y/Y/Enter approve; n/N/Esc reject; auto-rejects after 30 min |
| **Kernel-stats modal** (centered 80%×80% popup) | Read-only snapshot: queue-depth + dropped-metrics gauge, top-10 IPC calls, top-10 event-bus publishes, top-10 capability checks (denied in red). | `com.nexus.security` `metrics_snapshot` (fresh fetch on each open) | Shift+K toggles |

## Keybindings

Modal. The active `Mode` (`Normal`, `Search`, `Find`, `Terminal`, `AiInput`, `AgentInput`) selects the handler. When an approval modal is pending, its decision keys are intercepted before any mode dispatch (other keys still fall through).

**Approval modal (intercepts, any mode, when `agent.pending` is set):**

| Key | Action |
|-----|--------|
| `y` / `Y` / Enter | Approve pending round |
| `n` / `N` / Esc | Reject pending round |

**Normal mode — global:**

| Key | Action |
|-----|--------|
| `q` / Ctrl+C | Quit |
| Ctrl+F | Open search overlay (→ Search mode) |
| `/` | Open find bar (→ Find mode) |
| Tab | Toggle focus FileTree ↔ Viewer |
| `b` | Toggle backlinks panel |
| `t` | Toggle task list view |
| `K` (Shift+K) | Toggle kernel-stats modal |
| `T` (Shift+T) | Open terminal panel (→ Terminal mode) |
| `a` | Toggle AI panel (→ AiInput on open) |
| `g` | Toggle agent panel — only when focus ≠ Viewer |

**Normal mode — FileTree focus:** `j`/↓ down · `k`/↑ up · Enter/`l`/→ open file or expand dir · `h`/← collapse dir.

**Normal mode — Viewer focus:** `j`/↓ down · `k`/↑ up · `g`/Home top · `G`/End bottom · Ctrl+D/PageDown page down 20 · Ctrl+U/PageUp page up 20 · `e` open in external editor.

**Search mode:** type to edit query · Backspace delete · Enter run search + open top hit · ↑/↓ move selection · Esc → Normal.

**Find mode:** type to edit query (live match recompute) · Backspace · Enter/`n` next match · `N` previous match · Esc → Normal.

**Terminal mode:** any printable char line-buffers · Backspace edits buffer · Enter sends line to PTY · Ctrl+C sends `\x03` (SIGINT to child) · Ctrl+D kills session → Normal · Esc hides panel (session survives) → Normal.

**AiInput mode:** printable chars edit prompt · Backspace · Enter submit (non-blocking) · Esc → Normal.

**AgentInput mode:** printable chars edit goal · Backspace · Enter submit (→ Normal so hands are free for the modal) · Esc → Normal.

**Mouse (any focus):** scroll up/down moves tree selection (FileTree) or scrolls viewer by 3 (Viewer).

## IPC handlers

**None.** `nexus-tui` registers no `CorePlugin` and exposes no IPC commands. It is exclusively an IPC *caller*, reaching every backend capability through `runtime.context.ipc_call(...)` (or the typed `nexus_bootstrap::{storage,terminal}` wrappers). Handlers it calls: `com.nexus.storage` (`query_files`, `read_file`, `search`, `query_tasks`, `backlinks`, `graph_stats`), `com.nexus.terminal` (`create_session`, `pump`, `read_output`, `send_input`, `send_raw_input`, `close_session`), `com.nexus.ai` (`status`, `stream_ask`), `com.nexus.agent` (`session_run`, `round_decide`), `com.nexus.security` (`metrics_snapshot`).

## Capabilities

The TUI does not declare or check capabilities itself; it inherits the invoker context produced by `build_tui_runtime`. Capability enforcement happens kernel-side on each `ipc_call` against the calling context's grants (invariant 4). Any operation the TUI attempts that its invoker lacks a capability for fails at the kernel boundary and surfaces as an `Err` in the relevant fetch/pump, which the views render as an error string or silently fall back to empty (e.g. `load_backlinks`/`load_tasks` load empty on `Err`).

## Settings / Config

- **Forge path** resolved by `resolve_forge_path`, in order: (1) first CLI positional argument, (2) `NEXUS_FORGE_PATH` env var, (3) `~/.nexus/default` (`HOME`/`USERPROFILE`). Passed to `build_tui_runtime`.
- **Logging** goes to a file (never the alternate screen): path from `NEXUS_TUI_LOG`, default `/tmp/nexus-tui.log` (Unix) / `%TEMP%\nexus-tui.log` (Windows). Default filter `nexus_tui=debug`; `RUST_LOG` overrides.
- **Editor** for `e`: `$VISUAL` → `$EDITOR` → `vi`.
- **No TUI-specific config file.** Theme/colors are hardcoded ratatui `Style`s in each `ui/*` module (e.g. `Color::Blue` focus borders, `Color::Rgb(30,30,40)` status-bar background, the markdown heading palette). Layout proportions (25/75 split, overlay percentages, the 70/30 viewer/backlinks split) are hardcoded in `ui/mod.rs`. Timeouts are hardcoded constants: AI stream 180s, agent IPC 600s, modal auto-reject 1800s, terminal pump 50ms. The tokio runtime uses 1 worker thread + `KERNEL_BLOCKING_POOL_SIZE` blocking threads. **Gap:** none of these are promotable settings today; if surfaced they'd belong in a future TUI config tracked under `docs/0.1.2/settings/`.

## Events

The TUI subscribes to the kernel event bus for two live surfaces (it does not subscribe to file-change events — the tree/status are refreshed manually, not on a watcher):

- **AI streaming** — on each `submit_ai`, subscribes to `EventFilter::CustomPrefix("com.nexus.ai.stream_")` *before* firing `stream_ask` (broadcast channels drop pre-subscription events). `pump_ai` drains `stream_start`/`stream_chunk`/`stream_done` (filtered by per-submit UUID `session_id`) into the placeholder assistant message between frames.
- **Agent sessions** — on each `submit_agent`, subscribes to `EventFilter::CustomPrefix("com.nexus.agent.")` before firing `session_run`. `pump_agent` drains `round_proposed` events, surfaces the approval modal for destructive rounds (auto-skips safe rounds the server-side bus bridge handles), and harvests the final session result.

**No file-watcher subscription:** the tree is rebuilt only via `refresh_tree` (startup) and the status only via `refresh_status` (startup). External edits made through the `e` editor path are reloaded explicitly on resume; other on-disk changes are not picked up live. This is a known gap relative to the file-as-truth invariant's live-reload expectations.

## Internals & notable implementation details

- **Render loop & frame budget:** `run` loops `terminal.draw(...)` then `event::poll(16ms)`. The 16ms poll gives a ~60fps idle cap; `event::read` only fires when poll returns true. After input, three conditional pumps run: `pump_terminal` (when terminal active), `pump_ai` (when an AI stream is live), `pump_agent` (when an agent session or pending modal exists). Each pump is a no-op / cheap try-recv when its subsystem is idle.
- **Terminal setup/teardown safety:** `TerminalGuard::enter` enables raw mode + alternate screen + mouse capture; its `Drop` leaves the alternate screen, disables raw mode, and calls `ratatui::restore()`. Because it's an RAII guard, teardown runs even if `run` returns `Err` or panics — important since the TUI can be invoked from the long-lived `nexus` CLI process. `open_in_editor` does a manual leave/re-enter of raw mode + alt screen around the `Command::status()` call.
- **Sync render over async IPC:** one-shot data uses `rt.block_on` (e.g. `open_selected_file`, `refresh_tree`, `load_tasks`, `toggle_kernel_stats`) — these briefly freeze the loop; the AI panel pre-paints "thinking…" so the freeze is narrated. Long-running work (AI stream, agent session) is `rt.spawn`'d and harvested non-blocking via `JoinHandle::is_finished` + a final `block_on` only once finished. The PTY is pumped with a 50ms internal timeout that returns immediately when idle.
- **State management:** `TuiApp` is a single owned struct; `runtime` is `Arc<Runtime>` so spawned IPC futures can hold a clone by move while sync helpers still use `&*self.runtime`. Streaming uses an index-captured placeholder message (`placeholder_idx`) so chunks append without a linear scan, relying on `in_flight` to block concurrent submits from growing the vec underneath.
- **Conservative approval defaults:** `classify_round`/`parse_round_proposed` default a missing `requires_approval` to `true` and missing `registered` to `false`, mirroring the kernel's conservative stance — unregistered/unknown tools surface the modal rather than auto-approving. A local 30-minute auto-reject timer (`is_modal_expired`) matches the server's `DEFAULT_APPROVAL_TIMEOUT_SECS` as belt-and-braces if the bus reply is lost.
- **Git view:** the status bar's branch/dirty info comes from a direct `nexus_git::GitEngine::open(forge_root).state()` call in `refresh_status` — a read-only convenience, the one place the TUI touches a subsystem crate directly rather than via IPC. It's best-effort (`None` when not a repo).
- **Dream cycle:** `TuiApp::new` spawns `nexus_bootstrap::dream_cycle::spawn`; the handle is held only for its `Drop` (signals + joins the worker on app teardown). Gated on `[dream_cycle].enabled` server-side, so an opted-out forge does nothing but a 60s config-poll.
- **Markdown rendering** is a hand-rolled span highlighter (`ui/viewer.rs`), not a real markdown engine — ratatui ships none. Block-level headings/quotes/fences/rules plus inline `` `code` ``, `[[wikilinks]]`, `#tags`. AI/agent transcripts render markdown as plain text.

## Tests

All tests are inline `#[cfg(test)]` modules; there is **no `tests/` directory**.

- **`app.rs` — `aig07_tests`:** `extract_stream_ask_text` (text field, legacy `answer` fallback, bare-string fallback, unknown-shape rejection); `AiPanelState` input editing (insert at end / mid-string, multibyte/`é` grapheme handling, backspace no-op at 0, backspace mid-string).
- **`app.rs` — `bl132_tests`:** `classify_round` (no calls → auto-approve, all-safe → auto-approve, any-destructive → destructive, unregistered → destructive); `parse_round_proposed` (None on missing `session_id`/`round`, full-payload extraction, conservative field defaults); `is_modal_expired` (before/at/past timeout, and the non-monotonic-clock guard).
- **`input.rs` — `bl132_key_tests`:** `key_to_approval_decision` mapping (approve keys → true, reject keys → false, unrelated → None).
- **`streaming.rs` — `tests`:** `parse_chunk_event` (match, session mismatch, missing/non-string fields, empty-chunk passthrough), `parse_done_event` (match, mismatch, missing text), `matches_start_event` (match/mismatch), and a topic-constant pin test asserting the strings track `crates/nexus-ai/src/core_plugin.rs`.
- **`ui/agent_approval.rs` — `tests`:** `remaining_secs` countdown (counts down, saturates at 0, full at open).

Coverage is concentrated on the pure helpers (parsing, classification, timers, text editing). The render loop, IPC fetch methods, event-loop pumps, and the ratatui renderers have no automated tests — consistent with the difficulty of testing terminal output, but a gap for the `pump_ai`/`pump_agent` state-machine logic which is only exercised through its pure sub-helpers.
