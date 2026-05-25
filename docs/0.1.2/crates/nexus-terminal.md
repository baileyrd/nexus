# nexus-terminal

> Kind: lib · IPC plugin id: com.nexus.terminal · CorePlugin: yes · Has settings: no dedicated TOML (config is constructor/bootstrap-driven; persists `procmgr.sqlite` + `sessions.sqlite`) · As of: 2026-05-25

## Overview

`nexus-terminal` is the terminal & process-manager service for Nexus and the in-tree realisation of PRD-09. It owns the PTY primitive (spawn / write / read / resize / kill a child shell via `portable-pty`), a two-tier output capture (raw byte ring + ANSI-stripped line buffer), a per-OS signal-delivery ladder, a per-process memory monitor, a saved-command sidebar store, an ad-hoc command-history store, and a cross-session full-text scrollback search. Everything reachable from a frontend goes through the kernel IPC dispatcher under the plugin id `com.nexus.terminal`; CLI, TUI, MCP, the Tauri shell, and WASM/script plugins all reach the terminal surface the same way via `context.ipc_call("com.nexus.terminal", command, args)`.

The crate is deliberately layered into a pure library core and a single plugin bridge. The library tier (`session`, `manager`, `server`, `buffer`, `lines`, `memory`, `shell`, `ansi`, `urls`, `env`, `compound`, `precmd`, `profile`, `persist`, `procmgr`, `saved`, `adhoc`, `ai`, `external_terminal`, `job_object`) has zero coupling to the kernel bus or capability system — it is synchronous, runtime-agnostic, and every method takes `&self`/`&mut self`. The `core_plugin` module (`TerminalCorePlugin`) is the **only** part that touches `nexus-plugins`; it wraps an `InMemoryTerminalServer` behind a `CorePlugin` dispatcher, owns the SQLite stores, spawns background threads, and forwards each handler id to a `dispatch_*` method. Per-domain handler logic lives in `handlers/{session,io,info,saved,adhoc,run,repl,ai,shared}.rs` (the SD-03 split that mirrors `nexus-storage`/`nexus-git`); `core_plugin::dispatch` is a thin `match` on handler id.

The threading model is shaped by `portable-pty` handles being `Send` but not `Sync`. A `Session` therefore owns its master PTY + child + writer behind exclusive `&mut self`, and runs a dedicated **reader thread** per session that drains the blocking PTY reader into an mpsc channel so timeout-bounded reads never hang on an idle shell. The `TerminalCorePlugin` holds the server behind `Arc<Mutex<InMemoryTerminalServer>>` so IPC dispatch (single-threaded) and three background threads — a byte-stream **drainer**, a **lifecycle forwarder**, and a **memory poller** — can all contend for the same lock with short critical sections.

Microkernel fit: the crate spawns arbitrary processes, which is the most dangerous capability in the system. Process spawn is capability-gated at the manifest boundary (`create_session` / `repl_start` require `process.spawn`); the architecture invariant is that invokers reach terminal features only through `com.nexus.terminal`, never by linking the library. The library honours file-as-truth indirectly: the SQLite stores (`procmgr.sqlite`, `sessions.sqlite`) and the FTS5 scrollback index are derived/recoverable state, not a source of record for user markdown.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (`EventBus`, `KernelPluginContext`, `EventFilter`, `NexusEvent`, `Ipc` trait for the nested `com.nexus.ai` call), `nexus-plugins` (`CorePlugin`, `CorePluginFuture`, `PluginError`, `define_dispatch_helpers!` macro), `nexus-types` (BL-057 universal activity timeline — `activity::ActivityEntry` / `ACTIVITY_APPENDED_TOPIC` for the BL-052 timeline).
- **Notable external deps:** `portable-pty` (PTY allocation + spawn), `rusqlite` (the three SQLite stores), `regex-lite` (substring/regex line + cross-session search), `tokio` (only the `time` feature — wraps the AI-suggest IPC in a 10 s timeout), `uuid` (session ids), `chrono` (timestamps), `thiserror` (`TerminalError`), `serde`/`serde_json`, `tracing`. Platform-specific: `libc` on Unix (SIGINT/SIGTERM delivery via `kill(-pgid, …)`), `windows-sys` on Windows (Job Objects). Optional `ts-rs` + `schemars` behind the `ts-export` feature emit TS bindings + JSON Schema for IPC arg/reply types (audit P1-3, #113). `tempfile` + `criterion` are dev-dependencies.
- **Crates depending on it:** `nexus-bootstrap` registers the core plugin (`crates/nexus-bootstrap/src/plugins/terminal.rs`). `MANIFEST_DEPS = ["com.nexus.ai"]` — ai loads first so the soft `suggest` enrichment path is available (it degrades gracefully when absent).

## Public API surface

Re-exported from `lib.rs`, module by module:

- **`session` — the PTY primitive.** `Session` (one PTY master + one child; spawn/write/read/resize/kill, reader thread, leftover-byte `pending`, latched `ProcessState`, `request_shutdown` ladder, `read_into`/`read_into_buffer` capture helpers). `SessionConfig` (shell override, working dir, initial size, env). `SessionId` (UUID wrapper). `ProcessState` (`Running` / `Exited{code}` / `Killed{signal,code}`). `Signal` (`Int` / `Term` / `Kill`).
- **`manager` — `SessionManager`.** In-memory registry of `Session + OutputBuffer + LineBuffer` keyed by `SessionId`; enforces `DEFAULT_MAX_SESSIONS = 50`; `spawn_or_evict` (LRU eviction of stopped sessions), drain, write, resize, snapshot, search, `pid`, `created_at`/`name`/`set_name`, `buffer_read_since`, `reap_exited`, `request_shutdown`, `last_accessed`.
- **`server` — `TerminalServer` trait + `InMemoryTerminalServer`.** The programmable surface (PRD-09 §11): `create_session` · `close_session` · `send_input` · `send_raw_input` · `pump` · `read_raw_since` · `read_output` · `search_output` · `subscribe_events` · `wait_for_pattern` · `rename_session` · `get_session_info` · `list_sessions` · `resize`. `ServerSpawnConfig`, `SessionInfo` (incl. `rss_bytes`), `OutputLine` (timestamp_ms + ANSI-stripped content + raw bytes + repeats), `TerminalEvent` (lifecycle enum), `EvictionPersister` callback type.
- **`buffer` — `OutputBuffer`.** Fixed-capacity FIFO raw-byte ring (default 10 MB); `push`/`evict`/`snapshot`/`slices`/`len`/`dropped`/`find`/`contains`. `dropped()` is the monotonic "bytes ever evicted" counter underpinning the `read_raw_since` cursor.
- **`lines` — `Line` + `LineBuffer`.** ANSI-stripped, timestamped, adjacent-duplicate-collapsed line view (default `max_lines = 10_000`); `push`/`flush_pending`/`iter`/`find`/`find_regex`; `Line::urls()`.
- **`memory` — `MemoryMonitor`.** Per-pid RSS sampling (`read_process_rss`: `/proc/<pid>/status` on Unix, `GetProcessMemoryInfo` on Windows), rolling history (`DEFAULT_HISTORY_SAMPLES = 60`), soft/hard limit evaluation. `MemoryLimits`, `MemoryLimitAction`, `MemorySample`, `RECOMMENDED_POLL_INTERVAL = 1s`.
- **`shell` — shell detection.** `detect_default_shell()` (explicit → `$SHELL` → platform fallback), `ShellSpec` (`bare`/`interactive`).
- **`ansi` — `strip_ansi`.** CSI / OSC / 2-byte-ESC sequence stripper, lossy-UTF-8 output.
- **`urls` — URL detection.** `detect_urls`, `resolve_url` (rewrites `localhost:N` → `http://127.0.0.1:N`), `UrlMatch`, `UrlKind`.
- **`env` — env resolution.** `parse_env_file`/`parse_env_text`, `resolve_env` (4-layer precedence), `interpolate_env` (`${VAR}` up to 10 passes), `is_secret_key` / `mask_secrets` / `REDACTED`.
- **`compound` — command-chain splitter.** `parse_command_chain` (quote-aware split on `&&`/`||`/`;`), `CommandStep`, `Operator`, `execute_chain`, `ChainOutcome`, `StepOutcome`, `SkipReason`, `requires_single_shell`.
- **`precmd` — pre-command pipeline.** `run_pre_commands` (drives a `ManagedProcess` through `pre_commands` against a `TerminalServer` via sentinel-line exit-code detection), `PreCommandOptions`, `PreCommandOutcome`, `ShellFamily`, `DEFAULT_STEP_TIMEOUT = 30s`.
- **`profile` — shell-profile sourcing.** `profile_source_command(_for_path)`, `profile_path_for_shell`, `supports_profile_sourcing` — returns the rc-source command string for the caller to write post-spawn.
- **`procmgr` — managed-process FSM.** `ManagedProcess`, `ManagedState` (`Stopped`/`PreCommand`/`Starting`/`Running`/`Crashed`/`Restarting`), `ManagedConfig`, `TransitionError`, `DEFAULT_PRE_COMMAND_TIMEOUT = 30s`, `DEFAULT_AUTO_RESTART_BACKOFF_MS = [2000,5000,10000]`. Validation-only state machine; the *when* is the caller's job.
- **`saved` — saved-command store.** `SqliteSavedCommandStore` (CRUD + reorder over `procmgr_commands`), `SavedCommand`, `promote_adhoc_to_saved`, `PromoteOptions`, `slugify`, `DEFAULT_ICON = "terminal"`, `DEFAULT_AUTO_RESTART_DELAY_MS = 2000`.
- **`adhoc` — ad-hoc history store.** `SqliteAdHocStore` (record/recent/get/delete, dedup by `(command, working_dir)`), `AdHocRecord`, `AdHocStatus` (`Success`/`Failure`/`Timeout`).
- **`persist` — session persistence + FTS.** `SqliteSessionStore` (metadata + on-disk scrollback blobs + FTS5 `cross_session_search`), `SessionMetadata`, `ScrollbackHit`.
- **`external_terminal` — BL-059 escape hatch.** `TerminalKind`, `launch_spec`, `pick_first_available`, `which_in_path`, `spawn_detached`, `parse_kind`, `DEFAULT_PRIORITY`.
- **`job_object` — `JobObject`.** Windows-only RAII wrapper around a Win32 Job Object for tree-kill (the POSIX-process-group analogue).
- **`ai` — suggestion engine.** `AiSuggestionEngine` + `SuggestionRule` trait + `default_suggestion_rules` (Cargo compile failure, npm package-not-found, command-not-found, git public-key, address-in-use). `SuggestedCommand`, `SuggestionSeverity`.
- **`core_plugin` — the plugin bridge.** `TerminalCorePlugin`, all IPC arg/reply DTOs, the `HANDLER_*` id constants, `IPC_HANDLERS`, `MANIFEST_DEPS`, `PLUGIN_ID`, `EVENT_OUTPUT_PREFIX`, `EVENT_LIFECYCLE_PREFIX`.

## IPC handlers

29 handlers, registered from `IPC_HANDLERS`. Handler ids are append-only (never reused) because loaded-plugin manifests bake them in (note the gap: ids 1–17 are sequential, 18–30 continue, no id is reused). `create_session` and `repl_start` are the spawn-gated entry points; the others operate on already-spawned sessions or the SQLite stores. Args/returns are JSON; `Null` means the dispatch returns `serde_json::Value::Null`.

| Command | id | Args | Returns | Capability | Description |
|---------|----|------|---------|------------|-------------|
| `create_session` | 1 | `CreateSessionArgs` `{name?, shell?, shell_args[], working_dir?, env[(k,v)]}` | `CreateSessionResponse {id}` | `process.spawn` | Spawn a PTY child shell. |
| `close_session` | 2 | `SessionIdArgs {id}` | `Null` | — | Graceful SIGINT→SIGTERM→SIGKILL shutdown ladder (500 ms/step); drops emitter state. |
| `send_input` | 3 | `SendInputArgs {id, input}` | `Null` | — (audit: candidate for `process.spawn`) | Write string + auto-newline to child stdin. |
| `send_raw_input` | 4 | `SendRawInputArgs {id, data: bytes}` | `Null` | — (audit) | Write raw bytes (control sequences, no newline). |
| `pump` | 5 | `PumpArgs {id, timeout_ms?=100}` | `PumpResponse {bytes}` | — | Drain PTY into line+byte buffers; emits `OutputReceived` per new line; publishes output event when bus wired. |
| `read_output` | 6 | `ReadOutputArgs {id, start?, count?}` | `Vec<OutputLine>` | — | Snapshot structured lines `[start, start+count)`. |
| `read_raw_since` | 16 | `ReadRawSinceArgs {id, cursor, timeout_ms?=30}` | `ReadRawSinceResponse {cursor, data: bytes}` | — | Pump + return raw bytes past a monotonic cursor (xterm path). |
| `search_output` | 7 | `SearchOutputArgs {id, query, is_regex?}` | `Vec<usize>` (line indices) | — | Substring or regex-lite search of the line buffer. |
| `wait_for_pattern` | 8 | `WaitForPatternArgs {id, pattern, is_regex?, timeout_ms}` | `WaitForPatternResponse {matched}` | — | Pump-until-match or timeout; emits `PatternMatched`. |
| `get_session_info` | 9 | `SessionIdArgs {id}` | `SessionInfo` (incl. cached `rss_bytes`) | — | Metadata for one session. |
| `list_sessions` | 10 | `{}` | `Vec<SessionInfo>` (RSS layered when monitor wired) | — | Metadata for every session. |
| `saved_list` | 11 | `{}` | `Vec<SavedCommand>` | — | All `procmgr_commands` rows, ordered by `sidebar_order` (nulls last) then `name`. |
| `saved_create` | 12 | `SavedCommand` | `SavedCommand` | — | Insert a saved command (fails on duplicate slug). |
| `saved_update` | 13 | `SavedCommand` (slug = key) | refreshed `SavedCommand` | — | Update by slug. |
| `saved_delete` | 14 | `{slug}` | `{slug}` | — | Delete by slug. |
| `saved_reorder` | 15 | `{slug, sidebar_order?}` | `{slug, sidebar_order}` | — | Set the sidebar order index. |
| `resize` | 17 | `ResizeArgs {id, cols, rows}` | `Null` | — | Update PTY size (SIGWINCH); clamps 0 dims to 1×1. |
| `open_in_terminal` | 18 | `{slug, priority?: [string]}` | `{kind, program, args, working_dir}` | — (host-level invocation) | BL-059 — launch the saved command's `working_dir` in an external emulator. |
| `adhoc_list` | 19 | `AdHocListArgs {limit?=100}` | `Vec<AdHocRecord>` | — | Recent ad-hoc history, newest-first. |
| `adhoc_get` | 20 | `AdHocIdArgs {id}` | `AdHocRecord` or `Null` | — | Single ad-hoc row by id. |
| `adhoc_delete` | 21 | `AdHocIdArgs {id}` | `{id}` | — | Idempotent forget of an ad-hoc row. |
| `adhoc_promote` | 22 | `AdHocPromoteArgs {id, name, slug?, icon?, shell?}` | `SavedCommand` | — (audit) | BL-060 — promote ad-hoc → saved command. |
| `run_saved` | 23 | `RunSavedArgs {slug, working_dir?, command?}` | `CreateSessionResponse {id}` | — (audit; spawns) | BL-055/056 — spawn a session running the saved cmd (or `command` override) under its shell/cwd/env; pins `memory_limit_mb`. |
| `suggest` | 24 | `SuggestArgs {session_id, line_count?=50}` | `SuggestResponse` or `Null` | — | BL-064 — **async-only** handler; rule match + optional `com.nexus.ai::stream_chat` enrichment. Sync `dispatch` returns `HandlerIsAsyncOnly`. |
| `cross_session_search` | 25 | `CrossSessionSearchArgs {query, is_regex?, session_ids?, since_ts?, limit?=100}` | `Vec<ScrollbackHit>` | — | BL-063 — FTS5 (or regex) search across persisted scrollback; errors if session store not wired. |
| `repl_start` | 26 | `ReplStartArgs {lang, program, args[], working_dir?, env[]}` | `ReplStartResponse {id, lang}` | `process.spawn` | BL-142 — spawn a language kernel as a session and register it in the REPL map. |
| `repl_eval` | 27 | `ReplEvalArgs {id, code}` | `Null` | — (audit) | BL-142 — write code to a registered REPL's stdin (rejects non-REPL ids). |
| `repl_stop` | 28 | `SessionIdArgs {id}` | `Null` | — | BL-142 — close + unregister a REPL session (rejects non-REPL ids). |
| `repl_list` | 29 | `{}` | `Vec<ReplInfo>` (sorted by `started_at_ms`) | — | BL-142 — snapshot of registered REPLs. |
| `rename_session` | 30 | `RenameSessionArgs {id, name}` | `Null` | — | Update a session's display label; emits `SessionRenamed`. |

`with_v1_aliases(IPC_HANDLERS)` is applied at registration so legacy `v1.*` command aliases resolve to the same ids.

## Capabilities

The dangerous operation is process spawn, gated at the manifest boundary (per `docs/0.1.2/ipc-handlers.md`):

- **`process.spawn`** — required by `create_session` and `repl_start`. These accept guest-supplied shell / working_dir / env and launch an arbitrary child process.
- **Audit candidates (currently un-gated, flagged in ipc-handlers.md):** `send_input` / `send_raw_input` write into an already-spawned PTY; `run_saved` / `adhoc_promote` / `repl_eval` have the same posture. They are noted as candidates for `process.spawn` but not yet gated.
- Everything else (`pump`, `read_*`, `search_*`, `wait_for_pattern`, `list_sessions`, `get_session_info`, `rename_session`, `close_session`, `saved_*`, `adhoc_*` reads, `cross_session_search`, `suggest`, `open_in_terminal`, `repl_stop`/`repl_list`) is read / session-control / KV and carries no capability.

Note: the crate source itself contains no capability checks — gating is enforced by the kernel/plugin loader from the manifest, not inside `nexus-terminal`.

## Settings / Config

There is **no dedicated `.forge/*.toml`** for this crate; configuration is supplied programmatically by `nexus-bootstrap` through the `TerminalCorePlugin` builder and by per-call IPC args. Defaults:

- **Session cap:** `DEFAULT_MAX_SESSIONS = 50` (`SessionManager`). Over-cap spawns evict the LRU *stopped* session; a running session is never auto-killed.
- **Raw ring capacity:** `OutputBuffer::DEFAULT_CAPACITY` = 10 MB per session.
- **Line buffer:** default `max_lines = 10_000`.
- **Memory limits (bootstrap default):** `MemoryLimits::default_recommended()` = 250 MB soft / 500 MB hard (PRD-09 §7.3); poll interval `RECOMMENDED_POLL_INTERVAL = 1s`, history `DEFAULT_HISTORY_SAMPLES = 60`. Per-saved-command `memory_limit_mb` overrides the hard threshold for sessions spawned via `run_saved`.
- **Default PTY size:** 80×24; `TERM=xterm-256color`, `COLORTERM=truecolor` injected at spawn.
- **Shutdown ladder step:** 500 ms per signal (server `close_session`); `DEFAULT_STEP_TIMEOUT` / `DEFAULT_PRE_COMMAND_TIMEOUT` = 30 s; auto-restart backoff `[2000, 5000, 10000]` ms.
- **Suggest:** default tail `50` lines, AI enrichment hard timeout 10 s.

**Persistence files** (all under `<forge>/.forge/`, created by bootstrap):

- **`procmgr.sqlite`** — shared by the saved-command and ad-hoc stores (separate tables, separate `rusqlite::Connection`s):
  - `procmgr_commands` (saved sidebar): `slug PK, name, shell, shell_cmd, working_dir, env_vars TEXT='{}', env_file, icon='terminal', auto_restart INTEGER=0, auto_restart_delay_ms=2000, memory_limit_mb, sidebar_order, pre_commands TEXT='[]', created_at, updated_at`; index `idx_procmgr_commands_sidebar_order`.
  - `procmgr_adhoc_history`: `id PK, command, working_dir, executed_at, exit_code, duration_ms=0, run_count=1, status='success'`; index on `executed_at DESC`; unique dedupe index on `(command, IFNULL(working_dir,''))`.
- **`sessions.sqlite`** — `SqliteSessionStore` session metadata + FTS:
  - `terminal_sessions`: `id PK, name, slug UNIQUE, shell, working_dir, created_at, last_accessed_at, is_active INTEGER=0, buffer_size_bytes INTEGER=10485760`; index on `last_accessed_at DESC`.
  - `scrollback_fts` (FTS5 virtual table, `porter` tokenizer): searchable `line_text`; `session_id` / `ts_ms` / `line_index` UNINDEXED. Rebuildable from blobs.
- **`sessions/<session_id>/scrollback.bin`** — on-disk raw scrollback blobs for LRU-evicted sessions (SQLite holds only the path + byte length).

If any store fails to open, bootstrap logs a warning and loads the plugin without that handler family — session IPC stays usable.

## Events

Published on the kernel bus via `bus.publish_plugin(PLUGIN_ID, …)`:

- **`com.nexus.terminal.output.<session_id>`** (`EVENT_OUTPUT_PREFIX`) — `OutputStreamPayload {data: bytes, seq: u64, ts_ms: i64}`. Raw PTY bytes (verbatim, no UTF-8 decode / no ANSI strip), one chunk per publish, `seq` per-session monotonic (first chunk = 1) for drop detection. Emitted by both the autonomous drainer and the on-demand `pump` / `read_raw_since` paths, sharing per-session cursor + seq state so the same bytes are never published twice. Designed for xterm.js frontends; lets the shell drop its 100 ms poll.
- **`com.nexus.terminal.events.<session_id>`** (`EVENT_LIFECYCLE_PREFIX`) — the `TerminalEvent` itself (`serde(tag="kind")`): `session_created` / `session_renamed` / `output_received` / `pattern_matched` / `session_closed` / `memory_limit_exceeded` / `session_evicted`. Forwarded from the in-memory server's mpsc by the lifecycle forwarder. Designed for AI / agent consumers wanting structured lines + lifecycle.
- **`com.nexus.activity.appended`** (BL-057, kernel-owned topic) — session-boundary events (`SessionCreated` / `SessionClosed` / `MemoryLimitExceeded`) are also fanned out as `nexus_types::activity::ActivityEntry` (surface `Process`, origin `Terminal(<id>)`) so the BL-052 activity timeline sees terminal activity. Streaming/internal variants (`OutputReceived`, `PatternMatched`, `SessionRenamed`, `SessionEvicted`) intentionally don't reach the activity log.

Subscribed: the plugin subscribes to the `InMemoryTerminalServer`'s in-process `mpsc::Receiver<TerminalEvent>` (via `subscribe_events`) in `with_event_bus`, before any session can be created, so no `SessionCreated` is missed. It also issues a nested `ipc_call("com.nexus.ai", "stream_chat", …)` from the `suggest` handler.

## Internals & notable implementation details

- **PTY spawn (`session::Session::spawn`).** `NativePtySystem::openpty` → `CommandBuilder` with the resolved `ShellSpec` + cwd + injected `TERM`/`COLORTERM` + caller env → `slave.spawn_command`. The slave fd is dropped immediately so child exit propagates EOF to the reader. A per-session reader thread (`nexus-terminal-reader/<shell>`) does the blocking `reader.read` into an 8 KiB scratch and forwards `Vec<u8>` chunks (or `Err`, or a 0-byte EOF marker) over an mpsc channel; `Session::read` uses `recv_timeout` so callers honour their `Duration` budget instead of hanging on an idle shell. Leftover bytes that overflow the caller buffer stay in `pending` for the next read.
- **Resize.** `MasterPty::resize(PtySize{cols,rows})` drives SIGWINCH; the IPC handler clamps zero dims to 1×1 because most tty ioctls reject zero.
- **Output ring + cursor (`buffer::OutputBuffer`).** Fixed-capacity byte ring; `dropped()` is a monotonic "total bytes ever evicted" counter. `read_raw_since(cursor)` returns `(next_cursor, bytes)` where the cursor domain is "total bytes ever written"; a stale cursor (behind the oldest retained byte) silently clamps to the ring start — xterm prefers a gap to an error.
- **Line capture (`lines::LineBuffer`).** Parallel to the ring; ANSI-stripped, timestamped `Line`s with adjacent-duplicate collapse (`repeats`) so spinner/progress output doesn't dominate. `pump` diffs the live line count against a per-session `emitted_lines` counter to emit `OutputReceived` only for genuinely new lines (resyncs on wrap).
- **Per-OS signal handling (`session::send_signal` / `request_shutdown`).** Unix: `libc::kill(-pid, SIG…)` targets the whole process group (portable-pty `setsid()`s the child so pgid == pid) — Ctrl-C semantics that reach backgrounded subprocesses. The graceful ladder is SIGINT → SIGTERM → SIGKILL with a wait between steps (`wait_for_exit` polls every 20 ms). Windows has no portable SIGINT/SIGTERM that portable-pty exposes, so `Int`/`Term` degrade to the force-kill path (`TerminateProcess`-equivalent); the `job_object` module provides Win32 Job Objects for tree-kill. `kill()` reaps the child (`wait`) and latches `ProcessState::Killed`. `Drop` best-effort kills + waits but does **not** join the reader thread (an unresponsive child could block forever; the OS reaps the fd).
- **Process lifecycle / state.** `ProcessState` (`Running`/`Exited`/`Killed`) is the session-level machine, latched on signal + on the `try_wait_exit` that observes termination. `procmgr::ManagedState` is the richer FSM (pre-commands, starting, restarting, crashed) layered on top by an external scheduler — validation-only, no spawning.
- **LRU eviction + scrollback persistence (BL-062).** `create_session` at-cap calls `manager.spawn_or_evict`; if a stopped session is evicted, the optional `EvictionPersister` (bootstrap wires one delegating to `SqliteSessionStore::save_scrollback`) durably stashes the bytes, then `SessionEvicted` is emitted **before** the new `SessionCreated` (causal order). Saving scrollback also auto-indexes its ANSI-stripped lines into `scrollback_fts`, so an evicted session is immediately searchable via `cross_session_search`.
- **Background threads (spawned by `with_event_bus`).** (1) **Drainer** (`nexus-terminal-drainer`): round-robin pumps every session with a 5 ms timeout, sleeps 10 ms, publishes new bytes; worst-case latency ≈ `n×5ms + 10ms` ≈ 260 ms at the 50-session cap. (2) **Lifecycle forwarder** (`nexus-terminal-lifecycle`): `recv_timeout(100ms)` on the server mpsc, republishes onto the bus, exits on stop flag or sender-disconnect. (3) **Memory poller** (`nexus-terminal-memory-poller`, only when a monitor is also configured): each interval reconciles live (id,pid) pairs, samples RSS, and on `HardExceeded` publishes `MemoryLimitExceeded` **before** issuing `close_session` (so the breach precedes `SessionClosed`); refreshes a per-session RSS cache read by `get_session_info`/`list_sessions`. All three handles join their thread on `Drop` to avoid leaking `Arc`s past plugin teardown.
- **Lock discipline.** `pump` / `read_raw_since` hold the server `Mutex` only long enough to drain + read the new chunk, then release before publishing so a slow subscriber can't back-pressure the next dispatch. `suggest` reads the line tail under a brief lock, drops it, then makes the (possibly slow) AI IPC call.
- **Async suggest.** `suggest` is the one handler routed through `CorePlugin::dispatch_async` (`HANDLER_SUGGEST` returns `HandlerIsAsyncOnly` from the sync path). The free `async fn handle_suggest` runs the rule engine over the tail (first match wins, newest-first), and if a `KernelPluginContext` is wired (captured by `wire_context`), enriches the explanation via `com.nexus.ai::stream_chat` (mode=complete, tools=none, max_tokens=200) wrapped in a 10 s `tokio::time::timeout`; on failure/timeout it falls back to the rule's static reason (`llm_used=false`).
- **REPL bookkeeping (BL-142).** `repl_start` reuses the normal `create_session` path (so PTY/scrollback/output-bus/memory all apply) and records a `ReplInfo` in `self.repls`; `repl_eval`/`repl_stop` reject ids not in that map so a keybinding mis-fire can't drop code into a user's shell. Sessions are named `repl:<lang>` so list views can distinguish them.
- **Tab rename + auto-naming.** `rename_session` (handler id 30) updates the `SessionManager` label and emits `SessionRenamed`; it is the backend half of the recent terminal-tab-rename work. The **auto-naming** logic (deriving a label from the running command) is shell-local — the comment on `HANDLER_RENAME_SESSION` notes "the auto-naming path stays shell-local", i.e. it lives in `shell/`, not this crate.
- **Run-saved shell flag selection.** `run_saved` picks the one-shot flag by shell basename: `cmd` → `/C`, `pwsh`/`powershell` → `-Command`, everything else → `-c`.

## Tests

There is no `tests/` integration directory; all tests are `#[cfg(test)]` unit modules colocated with their source, most gated by a `unix_only(name)` skip helper because they spawn real `/bin/sh` PTYs.

- **`session.rs`** — spawn/read/echo, write→cat round-trip, resize, write-after-kill `NotRunning`, `read_into_buffer` capture, the SIGINT-first shutdown ladder, an `#[ignore]`d escalation-to-SIGKILL test (Python signal-handler startup race), process-group-leader invariant (`getpgid == pid`), group-signal termination of a backgrounded subprocess, `ProcessState`/`Signal` helpers, and nonexistent-shell `Spawn` error.
- **`server.rs`** — `OutputLine` conversion, `SessionCreated` emit + name registration, rename emit + unknown-id `NotRunning`, `OutputReceived` per line, `wait_for_pattern` timeout vs substring match, `read_output` windowing, literal+regex search (incl. bad-regex `Persist`), `list_sessions`, unknown-id error coverage, `read_raw_since` cursor advance / clamp-on-eviction / past-end behaviour, subscriber cleanup, and the BL-062 at-cap LRU eviction → persister + ordered-event test.
- **`core_plugin.rs`** — dispatch-level: unknown handler id, create+list round-trip, pump→read structured lines, search via dispatch, invalid-args `ExecutionFailed`, unknown-id info error, `pump` monotonic-seq output event, autonomous drainer publish, silent publish path without a bus, resize clamp/unknown-id, zero-timeout `wait_for_pattern`, and lifecycle-event publication (`SessionCreated`/`OutputReceived`/`PatternMatched`) on the kernel bus.
- **Other modules** (`buffer`, `lines`, `memory`, `shell`, `ansi`, `urls`, `env`, `compound`, `precmd`, `profile`, `saved`, `adhoc`, `persist`, `procmgr`, `external_terminal`, `ai`, `job_object`) carry their own colocated unit tests.
- **`benches/`** — `buffers.rs` (push 100k lines throughput; per-buffer memory footprint vs PRD-09 §17.1 targets) and `lines.rs`, both Criterion harnesses (`cargo bench -p nexus-terminal`), stable-toolchain-friendly (no nightly `#[bench]`).
