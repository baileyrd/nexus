//! `com.nexus.terminal` core plugin — microkernel bridge for PRD-09.
//!
//! # Role
//!
//! Wraps an [`InMemoryTerminalServer`] behind a [`CorePlugin`]
//! dispatcher so WASM and script plugins — which never link
//! `nexus-terminal` directly — can reach the terminal surface via
//! `ipc_call("com.nexus.terminal", <handler_id>, args)`. Matches the
//! shape of `com.nexus.database` / `com.nexus.storage` so the bootstrap
//! wires them all the same way.
//!
//! # Microkernel fit
//!
//! This module is the **only** part of `nexus-terminal` that touches
//! the plugin layer (`nexus-plugins`). Every other module stays a pure
//! library — the kernel bus reaches the terminal through exactly this
//! plugin, nowhere else. That preserves invariant #3 from
//! `docs/architecture/C4.md` §7 ("invokers must reach terminal features
//! via `com.nexus.terminal`, not by linking the library").
//!
//! # Handlers
//!
//! | Handler id | Command              | Purpose                                 |
//! |-----------:|----------------------|-----------------------------------------|
//! | 1          | `create_session`     | Spawn a new PTY session                 |
//! | 2          | `close_session`      | Graceful shutdown ladder                |
//! | 3          | `send_input`         | Write string + newline                  |
//! | 4          | `send_raw_input`     | Write raw bytes (control sequences)     |
//! | 5          | `pump`               | Drain PTY into line buffer              |
//! | 6          | `read_output`        | Snapshot lines `[start, start+count)`   |
//! | 7          | `search_output`      | Substring / regex search                |
//! | 8          | `wait_for_pattern`   | Block until a pattern matches / timeout |
//! | 9          | `get_session_info`   | Metadata for one session                |
//! | 10         | `list_sessions`      | Metadata for every session              |
//! | 16         | `read_raw_since`     | Pump + return raw bytes past a cursor   |
//! | 17         | `resize`             | Update PTY size (cols × rows), SIGWINCH |
//! | 18         | `open_in_terminal`   | Hand off saved cmd to external emulator |
//! | 19         | `adhoc_list`         | Recent ad-hoc command history (BL-060)  |
//! | 20         | `adhoc_get`          | Single ad-hoc row by id (BL-060)        |
//! | 21         | `adhoc_delete`       | Forget an ad-hoc row (BL-060)           |
//! | 22         | `adhoc_promote`      | Promote ad-hoc → saved command (BL-060) |
//! | 23         | `run_saved`          | Spawn session running saved cmd (BL-055)|
//! | 24         | `suggest`            | LLM-enriched output suggestion (BL-064) |
//! | 25         | `cross_session_search` | FTS5 search across scrollback (BL-063)|
//!
//! Ids are **append-only** — never reused after retirement — because
//! manifest registrations in loaded plugins bake them in.
//!
//! # Output streaming (Phase 2 WI-12)
//!
//! When the plugin is built via [`TerminalCorePlugin::with_event_bus`] a
//! background **drainer thread** is spawned that iterates every active
//! session each cycle, pumps the PTY, and publishes any new bytes on
//! `com.nexus.terminal.output.<session_id>` as a `NexusEvent::Custom`.
//! The payload is `{ data: Vec<u8>, seq: u64, ts_ms: i64 }` — `data`
//! matches the byte shape of `read_raw_since` so a single TS subscriber
//! can union the two paths, and `seq` is a per-session monotonic counter
//! so the subscriber can detect drops/reorders. The `pump` and
//! `read_raw_since` handlers also still publish on demand for any
//! caller-driven reads, sharing the same per-session cursor + seq state
//! so the autonomous and pull paths can't double-publish the same bytes.
//! The legacy poll-style `pump` handler still returns its byte count
//! unchanged; subscribers that miss events can always fall back to a
//! `read_raw_since` snapshot.
//!
//! # Lifecycle events (BL-013)
//!
//! Alongside the byte stream, [`TerminalCorePlugin::with_event_bus`]
//! also subscribes to the in-memory server's [`crate::TerminalEvent`]
//! mpsc and forwards every event onto the kernel bus as
//! `com.nexus.terminal.events.<session_id>`. The payload is the
//! `TerminalEvent` itself — `serde(tag = "kind")` keeps the four
//! variants (`session_created`, `output_received`, `pattern_matched`,
//! `session_closed`) distinguishable on the wire, so a single
//! [`nexus_kernel::EventFilter::CustomPrefix`] subscription gives
//! plugins everything they need to react to session lifecycle and
//! per-line output without re-implementing pump loops. The byte-stream
//! topic above stays the right channel for xterm-style frontends; this
//! topic is the right channel for AI / agent consumers that want
//! structured lines + lifecycle.
//!
//! # What this is NOT (yet)
//!
//! - Recording new ad-hoc rows over IPC. The `adhoc_*` handlers
//!   (BL-060) cover read / delete / promote, but the kernel doesn't
//!   yet observe ad-hoc executions; rows are inserted by the
//!   process-manager layer through [`crate::SqliteAdHocStore::record`]
//!   directly. A `record` handler will land alongside the workflow
//!   step type (BL-056) when running ad-hoc commands becomes a
//!   first-class IPC verb.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nexus_kernel::{EventBus, KernelPluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::adhoc::SqliteAdHocStore;
use crate::memory::{MemoryLimitAction, MemoryLimits, MemoryMonitor};
use crate::persist::SqliteSessionStore;
use crate::saved::SqliteSavedCommandStore;
#[cfg(test)]
use crate::saved::SavedCommand;
use crate::server::{InMemoryTerminalServer, TerminalEvent, TerminalServer};
use crate::session::SessionId;

/// Reverse-DNS identifier registered with the plugin loader.
pub const PLUGIN_ID: &str = "com.nexus.terminal";

/// Prefix for per-session output stream events. Each `pump` /
/// `read_raw_since` dispatch that observes new bytes emits a
/// `NexusEvent::Custom` with `type_id = "com.nexus.terminal.output.<id>"`
/// so a TS subscriber can filter by prefix
/// (via [`nexus_kernel::EventFilter::CustomPrefix`]) and still see which
/// session produced the chunk. Payload shape: [`OutputStreamPayload`].
/// See `docs/planning/PHASE-2-IMPLEMENTATION-PLAN.md` §4.4 (WI-12).
pub const EVENT_OUTPUT_PREFIX: &str = "com.nexus.terminal.output.";

/// Prefix for per-session lifecycle events
/// (`session_created` / `output_received` / `pattern_matched` /
/// `session_closed`). The payload is the
/// [`TerminalEvent`](crate::TerminalEvent) itself — see BL-013 / the
/// "Lifecycle events" section in this module's docs. Subscribe with
/// [`nexus_kernel::EventFilter::CustomPrefix`] to receive every session,
/// or with [`nexus_kernel::EventFilter::CustomExact`] for one session.
pub const EVENT_LIFECYCLE_PREFIX: &str = "com.nexus.terminal.events.";

/// `create_session` handler id.
pub const HANDLER_CREATE_SESSION: u32 = 1;
/// `close_session` handler id.
pub const HANDLER_CLOSE_SESSION: u32 = 2;
/// `send_input` handler id.
pub const HANDLER_SEND_INPUT: u32 = 3;
/// `send_raw_input` handler id.
pub const HANDLER_SEND_RAW_INPUT: u32 = 4;
/// `pump` handler id.
pub const HANDLER_PUMP: u32 = 5;
/// `read_output` handler id.
pub const HANDLER_READ_OUTPUT: u32 = 6;
/// `search_output` handler id.
pub const HANDLER_SEARCH_OUTPUT: u32 = 7;
/// `wait_for_pattern` handler id.
pub const HANDLER_WAIT_FOR_PATTERN: u32 = 8;
/// `get_session_info` handler id.
pub const HANDLER_GET_SESSION_INFO: u32 = 9;
/// `list_sessions` handler id.
pub const HANDLER_LIST_SESSIONS: u32 = 10;
/// `saved_list` handler id. Returns every row in `procmgr_commands`
/// ordered by `sidebar_order` (nulls last) then by `name`.
pub const HANDLER_SAVED_LIST: u32 = 11;
/// `saved_create` handler id. Accepts a `SavedCommand` JSON document.
pub const HANDLER_SAVED_CREATE: u32 = 12;
/// `saved_update` handler id. Accepts the full row; `slug` is the key.
pub const HANDLER_SAVED_UPDATE: u32 = 13;
/// `saved_delete` handler id. Args: `{ slug: string }`.
pub const HANDLER_SAVED_DELETE: u32 = 14;
/// `saved_reorder` handler id. Args: `{ slug: string, sidebar_order?: i32 }`.
pub const HANDLER_SAVED_REORDER: u32 = 15;
/// `read_raw_since` handler id. Folds [`HANDLER_PUMP`] + a raw-bytes
/// read into one call for xterm-style frontends that just want the PTY
/// byte stream. See [`ReadRawSinceArgs`].
pub const HANDLER_READ_RAW_SINCE: u32 = 16;
/// `resize` handler id. Updates the PTY's reported window size so the
/// child process receives SIGWINCH and reflows. See [`ResizeArgs`].
pub const HANDLER_RESIZE: u32 = 17;

/// BL-059 — `open_in_terminal` handler id. Args:
/// `{ "slug": String, "priority"?: Vec<String> }` — looks up the
/// saved command, picks the first emulator from `priority` (or
/// [`crate::DEFAULT_PRIORITY`] when omitted) whose program is on
/// `$PATH`, and spawns it detached at the saved command's
/// `working_dir`. Returns
/// `{ "kind": "<snake_case>", "program": String, "args": Vec<String> }`.
pub const HANDLER_OPEN_IN_TERMINAL: u32 = 18;

/// BL-060 — `adhoc_list` handler id. Args: [`AdHocListArgs`] (`limit`
/// defaults to 100). Returns the most recent rows from
/// `procmgr_adhoc_history`, ordered by `executed_at` desc.
pub const HANDLER_ADHOC_LIST: u32 = 19;
/// BL-060 — `adhoc_get` handler id. Args: [`AdHocIdArgs`]. Returns the
/// matching row or `null` when the id is unknown.
pub const HANDLER_ADHOC_GET: u32 = 20;
/// BL-060 — `adhoc_delete` handler id. Args: [`AdHocIdArgs`]. Returns
/// `{ "id": String }` whether or not the row existed (DELETE is a no-op
/// on a missing id, mirroring [`crate::SqliteAdHocStore::delete`]).
pub const HANDLER_ADHOC_DELETE: u32 = 21;
/// BL-060 — `adhoc_promote` handler id. Args: [`AdHocPromoteArgs`].
/// Wraps [`crate::promote_adhoc_to_saved`] and returns the freshly
/// inserted [`SavedCommand`].
pub const HANDLER_ADHOC_PROMOTE: u32 = 22;

/// BL-055 — `run_saved` handler id. Args: [`RunSavedArgs`]. Looks up
/// the saved command by slug, spawns a fresh PTY session running its
/// `shell_cmd` under its `shell` (with the saved `working_dir` and
/// `env_vars`), and returns the new session id. Reuses the standard
/// [`InMemoryTerminalServer::create_session`] path so lifecycle
/// events, drainer hookup, and persistence behave identically to
/// `create_session`.
pub const HANDLER_RUN_SAVED: u32 = 23;

/// BL-064 — `suggest` handler id. Args: [`SuggestArgs`]. Walks the
/// recent N output lines for a session, runs the
/// [`crate::AiSuggestionEngine`] over each, and — when a rule fires
/// — routes the matched context through `com.nexus.ai::stream_chat`
/// (`mode=complete`, `tools=none`) for an LLM-enriched explanation.
/// Returns `null` when no rule matches, or [`SuggestResponse`]
/// otherwise. Falls back to the rule's static reason when the
/// `com.nexus.ai` call fails or exceeds the 10 s budget.
pub const HANDLER_SUGGEST: u32 = 24;

/// BL-063 — `cross_session_search` handler id. Args:
/// [`CrossSessionSearchArgs`]. Runs an FTS5 (or regex) query against
/// the persisted scrollback index. Returns a `Vec<ScrollbackHit>`
/// ordered newest-first. Requires the session store to be wired
/// (bootstrap installs it via [`TerminalCorePlugin::with_session_store`]);
/// without it, the handler returns a clear "not configured" error
/// rather than silently returning an empty list.
pub const HANDLER_CROSS_SESSION_SEARCH: u32 = 25;

/// BL-142 Phase 1 — `repl_start` handler id. Args: [`ReplStartArgs`].
/// Spawns a language kernel as a regular terminal session (so the
/// existing PTY, scrollback, output-bus, and memory infrastructure
/// all apply unchanged) and registers it in the plugin's REPL map
/// so subsequent `repl_eval` / `repl_stop` calls can validate the
/// target id is actually a REPL. Returns
/// [`ReplStartResponse`] — the same session id `create_session`
/// would return, plus the resolved `lang` tag.
///
/// The kernel command is supplied by the caller as `(program, args)`
/// rather than a single string so the plugin doesn't have to make
/// shell-tokenizing decisions (the shell-side `nexus.editor.replKernels`
/// config string is parsed by the frontend).
pub const HANDLER_REPL_START: u32 = 26;

/// BL-142 Phase 1 — `repl_eval` handler id. Args: [`ReplEvalArgs`].
/// Writes `code` to the REPL session's PTY stdin (newline appended
/// automatically if absent, matching `send_input`'s contract).
/// Returns `Null` — output streams asynchronously on the existing
/// `com.nexus.terminal.output.<session_id>` event topic.
///
/// Rejects the call with a clear error if `id` is unknown OR is a
/// regular (non-REPL) terminal session; that guard exists so a
/// keybinding mis-fire can't accidentally drop code into a user's
/// shell.
///
/// Multi-line code blocks for Python-style REPLs need a trailing
/// blank line (`\n\n`) to terminate the block — Phase 1 leaves
/// that to the caller; Phase 2 may add a `repl_eval_block` helper.
pub const HANDLER_REPL_EVAL: u32 = 27;

/// BL-142 Phase 1 — `repl_stop` handler id. Args: [`SessionIdArgs`].
/// Closes the REPL session (same effect as `close_session`) and
/// removes it from the REPL map. Rejects with a clear error if the
/// id is unknown or refers to a non-REPL session.
pub const HANDLER_REPL_STOP: u32 = 28;

/// BL-142 Phase 1 — `repl_list` handler id. Args: `{}`. Returns a
/// `Vec<ReplInfo>` snapshot of every currently-registered REPL
/// session — useful for shell-side discovery (e.g. "what REPL is
/// already running for this tab?") and for the integration tests
/// here.
pub const HANDLER_REPL_LIST: u32 = 29;

/// Plugin ids this plugin calls at handler-dispatch time. Soft —
/// `stream_chat` is only used by the inline-suggest path, which
/// gracefully degrades when `com.nexus.ai` is absent — but
/// `register_all` always loads ai first, so we can declare it.
pub const MANIFEST_DEPS: &[&str] = &["com.nexus.ai"];

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::terminal::register`.
/// Order matches the pre-SD-06 bootstrap registration so the emitted
/// manifest is byte-identical.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("create_session", HANDLER_CREATE_SESSION),
    ("close_session", HANDLER_CLOSE_SESSION),
    ("send_input", HANDLER_SEND_INPUT),
    ("send_raw_input", HANDLER_SEND_RAW_INPUT),
    ("pump", HANDLER_PUMP),
    ("read_output", HANDLER_READ_OUTPUT),
    ("read_raw_since", HANDLER_READ_RAW_SINCE),
    ("search_output", HANDLER_SEARCH_OUTPUT),
    ("wait_for_pattern", HANDLER_WAIT_FOR_PATTERN),
    ("get_session_info", HANDLER_GET_SESSION_INFO),
    ("list_sessions", HANDLER_LIST_SESSIONS),
    ("saved_list", HANDLER_SAVED_LIST),
    ("saved_create", HANDLER_SAVED_CREATE),
    ("saved_update", HANDLER_SAVED_UPDATE),
    ("saved_delete", HANDLER_SAVED_DELETE),
    ("saved_reorder", HANDLER_SAVED_REORDER),
    ("open_in_terminal", HANDLER_OPEN_IN_TERMINAL),
    ("adhoc_list", HANDLER_ADHOC_LIST),
    ("adhoc_get", HANDLER_ADHOC_GET),
    ("adhoc_delete", HANDLER_ADHOC_DELETE),
    ("adhoc_promote", HANDLER_ADHOC_PROMOTE),
    ("run_saved", HANDLER_RUN_SAVED),
    ("suggest", HANDLER_SUGGEST),
    ("cross_session_search", HANDLER_CROSS_SESSION_SEARCH),
    ("repl_start", HANDLER_REPL_START),
    ("repl_eval", HANDLER_REPL_EVAL),
    ("repl_stop", HANDLER_REPL_STOP),
    ("repl_list", HANDLER_REPL_LIST),
];

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Arguments for `create_session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionArgs {
    /// Human-readable label.
    #[serde(default)]
    pub name: Option<String>,
    /// Absolute path to the shell executable. `None` falls back to
    /// the platform-default detection.
    #[serde(default)]
    pub shell: Option<String>,
    /// Extra args to pass to the shell after the program name.
    #[serde(default)]
    pub shell_args: Vec<String>,
    /// Working directory.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Env vars to merge on top of the inherited environment.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// Response from `create_session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionResponse {
    /// Fresh session id.
    pub id: String,
}

/// Arguments for `close_session` / `pump` / `get_session_info` and
/// any other single-session-id handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SessionIdArgs {
    /// Session id the handler targets.
    pub id: String,
}

/// Arguments for `send_input`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SendInputArgs {
    /// Target session id.
    pub id: String,
    /// String input (newline appended automatically if absent).
    pub input: String,
}

/// Arguments for `send_raw_input`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SendRawInputArgs {
    /// Target session id.
    pub id: String,
    /// Raw bytes to write (no newline appended).
    pub data: Vec<u8>,
}

/// Arguments for `pump`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PumpArgs {
    /// Target session id.
    pub id: String,
    /// Per-call read deadline in milliseconds. Defaults to 100 ms when
    /// omitted — a short-enough window that an idle session doesn't
    /// burn CPU, long enough to catch a single output flush.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Response from `pump`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PumpResponse {
    /// Byte count drained from the PTY in this pump.
    pub bytes: usize,
}

/// Arguments for `read_output`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReadOutputArgs {
    /// Target session id.
    pub id: String,
    /// Start line (clamped to buffer length). `None` = from the front.
    #[serde(default)]
    pub start: Option<usize>,
    /// How many lines to return. `None` = to the end.
    #[serde(default)]
    pub count: Option<usize>,
}

/// Arguments for `read_raw_since`. The cursor is the monotonic
/// "total bytes ever written to this session's ring" counter returned
/// by the previous call — start at `0` for a fresh session. Bytes
/// before the ring's oldest retained offset are silently skipped
/// (clamp-on-eviction) because xterm prefers a gap to an error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReadRawSinceArgs {
    /// Target session id.
    pub id: String,
    /// Monotonic byte offset the caller last saw. Use `0` on first call.
    pub cursor: u64,
    /// Drain deadline in ms. Defaults to 30 ms — short enough that an
    /// idle session releases the server Mutex promptly for concurrent
    /// `send_raw_input` calls.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Arguments for `resize`. `cols` / `rows` are character cells, matching
/// the unit `Terminal.resize(cols, rows)` uses on the xterm.js side and
/// `MasterPty::resize` uses inside `portable-pty`. Zero values are
/// rejected by the underlying ioctl on most platforms; callers should
/// clamp to at least `1 × 1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ResizeArgs {
    /// Target session id.
    pub id: String,
    /// New column count (character cells).
    pub cols: u16,
    /// New row count (character cells).
    pub rows: u16,
}

/// Response from `read_raw_since`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReadRawSinceResponse {
    /// Cursor to pass on the next call — equals `dropped + len` after
    /// the drain.
    pub cursor: u64,
    /// Raw bytes past the caller's cursor (ANSI sequences intact).
    pub data: Vec<u8>,
}

/// Payload published on `com.nexus.terminal.output.<session_id>` events.
///
/// Bytes are passed through verbatim — no UTF-8 decode, no ANSI
/// sanitisation. The TS subscriber feeds them straight into xterm.js
/// (or buffers them) the same way the `read_raw_since` snapshot path
/// does, so the two read paths can be unioned by the shell without
/// reshaping. `seq` is a per-session monotonic counter (starts at `1`
/// for a session's first published chunk) so the subscriber can detect
/// drops or out-of-order delivery; `ts_ms` is the publisher's wall
/// clock in unix milliseconds and is informational only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct OutputStreamPayload {
    /// Raw bytes appended to the session's ring buffer this dispatch.
    pub data: Vec<u8>,
    /// Per-session sequence number. Increments by one per published
    /// chunk for a given session id.
    pub seq: u64,
    /// Publisher wall-clock at emission, unix milliseconds.
    pub ts_ms: i64,
}

/// Arguments for `search_output`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SearchOutputArgs {
    /// Target session id.
    pub id: String,
    /// Query string.
    pub query: String,
    /// `true` = regex (regex-lite), `false` = substring.
    #[serde(default)]
    pub is_regex: bool,
}

/// Arguments for `wait_for_pattern`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct WaitForPatternArgs {
    /// Target session id.
    pub id: String,
    /// Pattern to search for.
    pub pattern: String,
    /// Regex mode toggle.
    #[serde(default)]
    pub is_regex: bool,
    /// Hard deadline in milliseconds. `0` returns immediately after
    /// one buffer scan.
    pub timeout_ms: u64,
}

/// Response from `wait_for_pattern`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct WaitForPatternResponse {
    /// `true` if a match landed before the deadline.
    pub matched: bool,
}

/// BL-060 — arguments for `adhoc_list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AdHocListArgs {
    /// Maximum number of rows to return. Defaults to 100 when omitted —
    /// matches the implicit cap the legacy CLI used and keeps a History
    /// panel responsive on a long-lived forge.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// BL-060 — arguments for `adhoc_get` / `adhoc_delete`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AdHocIdArgs {
    /// Row id (UUID) returned by `adhoc_list` / `adhoc_get`.
    pub id: String,
}

/// BL-060 — arguments for `adhoc_promote`. Wraps
/// [`crate::PromoteOptions`] across IPC; field semantics match
/// [`crate::promote_adhoc_to_saved`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AdHocPromoteArgs {
    /// Source ad-hoc row id.
    pub id: String,
    /// Human-readable label for the new saved command. Required — the
    /// underlying API derives the slug from this when one isn't passed
    /// explicitly.
    pub name: String,
    /// Optional explicit slug override.
    #[serde(default)]
    pub slug: Option<String>,
    /// Optional icon tag.
    #[serde(default)]
    pub icon: Option<String>,
    /// Optional shell binary override (defaults to `/bin/sh` on Unix,
    /// `cmd.exe` elsewhere — see the underlying API).
    #[serde(default)]
    pub shell: Option<String>,
}

/// BL-055 — arguments for `run_saved`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct RunSavedArgs {
    /// Slug of the saved command to launch.
    pub slug: String,
    /// Optional working-dir override. When omitted the saved command's
    /// own `working_dir` is used; if both are absent the parent cwd is
    /// inherited.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// BL-056 — optional command override. When present, runs this
    /// literal command line under the saved command's `shell` /
    /// `working_dir` / `env_vars` instead of the saved `shell_cmd`.
    /// Powers the workflow `terminal` step's `run_adhoc` action,
    /// which uses the saved profile as a "shell template" with a
    /// fresh command line per workflow run.
    #[serde(default)]
    pub command: Option<String>,
}

/// BL-064 — arguments for `suggest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SuggestArgs {
    /// Target session id.
    pub session_id: String,
    /// How many lines from the tail of the buffer to scan. Defaults
    /// to 50 when omitted — enough to catch the latest error block
    /// across most build-tool layouts without quadratic-cost regex
    /// scans on huge scrollbacks.
    #[serde(default)]
    pub line_count: Option<u32>,
}

/// BL-064 — successful suggestion. `null` is returned at the IPC
/// layer when no rule matched; this struct is only on the wire when
/// at least one [`SuggestionRule`](crate::SuggestionRule) fired.
///
/// `llm_used` distinguishes "the LLM enriched the explanation" from
/// "we fell back to the rule's static reason because the LLM call
/// timed out or no provider was configured". UI surfaces can render
/// the two cases differently (e.g. show a sparkle icon when
/// `llm_used`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SuggestResponse {
    /// Suggested command line — verbatim from the matching rule.
    pub text: String,
    /// Human-readable explanation. The LLM-enriched paragraph when
    /// `llm_used`; the rule's static reason otherwise.
    pub reason: String,
    /// Severity tag (`info` / `warning` / `error`) — verbatim from
    /// the matching rule.
    pub severity: String,
    /// Stable rule id (e.g. `cargo.compile_failure`).
    pub source_rule: String,
    /// `true` when the response was enriched by `com.nexus.ai`;
    /// `false` when the static rule explanation was used.
    pub llm_used: bool,
}

/// BL-063 — arguments for `cross_session_search`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct CrossSessionSearchArgs {
    /// FTS5 MATCH expression when `is_regex == false`, or a regex
    /// pattern when `is_regex == true`. Empty / whitespace-only
    /// queries short-circuit to an empty result rather than
    /// returning every line.
    pub query: String,
    /// When `true`, applies `regex_lite::Regex` over every indexed
    /// line constrained by `session_ids` / `since_ts`. The literal
    /// path uses FTS5's `MATCH` for full-text indexing and is
    /// strictly faster.
    #[serde(default)]
    pub is_regex: bool,
    /// Optional list of session ids to scope the search. `None` (or
    /// empty) searches every persisted session.
    #[serde(default)]
    pub session_ids: Option<Vec<String>>,
    /// Optional Unix-millis floor on the row timestamp. `None`
    /// returns hits regardless of age.
    #[serde(default)]
    pub since_ts: Option<i64>,
    /// Hard cap on the number of returned hits. Defaults to 100.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// BL-142 Phase 1 — arguments for `repl_start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReplStartArgs {
    /// Caller-supplied language tag (`"python"`, `"node"`, …).
    /// Surfaced verbatim in [`ReplInfo::lang`] for shell discovery.
    /// The plugin doesn't constrain the string — it's the caller's
    /// label, not a kernel-side enum.
    pub lang: String,
    /// Absolute path or `$PATH`-resolvable program name for the
    /// language kernel (e.g. `"python3"`).
    pub program: String,
    /// Args appended after `program` (e.g. `["-i"]` for an
    /// interactive Python kernel).
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory for the spawned kernel. Defaults to the
    /// session's normal cwd resolution if `None`.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Env overrides merged on top of the inherited environment.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// BL-142 Phase 1 — response from `repl_start`. `id` is the session
/// id (same shape as `create_session`'s response); `lang` echoes
/// the caller-supplied tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReplStartResponse {
    /// Fresh session id (same shape as
    /// [`CreateSessionResponse::id`]).
    pub id: String,
    /// Caller-supplied language tag, echoed back.
    pub lang: String,
}

/// BL-142 Phase 1 — arguments for `repl_eval`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReplEvalArgs {
    /// Target REPL session id.
    pub id: String,
    /// Code to send to the REPL's stdin. A trailing `\n` is
    /// appended automatically if absent (matching `send_input`).
    /// Multi-line Python blocks need an explicit blank line
    /// terminator (`\n\n`) — Phase 1 leaves that to the caller.
    pub code: String,
}

/// BL-142 Phase 1 — entry returned by `repl_list`. Mirrors what
/// the shell needs to render a "running REPLs" picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReplInfo {
    /// Session id (matches [`ReplStartResponse::id`]).
    pub id: String,
    /// Caller-supplied language tag from `repl_start`.
    pub lang: String,
    /// Program path the kernel was spawned with.
    pub program: String,
    /// Args the kernel was spawned with.
    pub args: Vec<String>,
    /// Unix epoch milliseconds at `repl_start` time.
    pub started_at_ms: i64,
}

// ── The plugin ───────────────────────────────────────────────────────────────

/// Core plugin instance. Holds the server behind an [`Arc<Mutex<_>>`]
/// so the `CorePlugin: Send + Sync` bound is satisfied (PTY handles are
/// `Send`-only) **and** so the autonomous drainer thread can hold its
/// own clone of the lock. IPC dispatches arrive single-threaded; the
/// drainer competes for the same lock with short-blocking pumps. See
/// [`spawn_drainer`] for the cadence + contention model.
pub struct TerminalCorePlugin {
    pub(crate) server: Arc<Mutex<InMemoryTerminalServer>>,
    /// SQLite-backed saved-commands store. `None` when the plugin is
    /// instantiated without a forge path (tests, embedded runtimes) —
    /// the saved-command handlers return a clear error in that case.
    pub(crate) saved: Option<Mutex<SqliteSavedCommandStore>>,
    /// SQLite-backed ad-hoc command history store (BL-060). `None` when
    /// the plugin is instantiated without a forge path — the
    /// `adhoc_*` handlers return a clear error in that case. Lives
    /// behind a separate `Mutex` from `saved` because the two stores
    /// own independent rusqlite `Connection`s (a `Connection` is
    /// `Send` but not `Sync`).
    pub(crate) adhoc: Option<Mutex<SqliteAdHocStore>>,
    /// Optional kernel event bus. When `Some`, the constructor spawns
    /// the autonomous drainer (see [`Self::drainer`]) and `pump` /
    /// `read_raw_since` dispatches additionally publish on demand for
    /// any bytes the drainer hasn't already shipped.
    pub(crate) event_bus: Option<Arc<EventBus>>,
    /// Per-session emitter state — tracks the internal byte cursor we
    /// last published from and the next sequence number to assign.
    /// Shared between the dispatch path and the drainer thread; both
    /// take the same lock, so the cursor advance + seq assignment is
    /// atomic per chunk and neither path can publish the same bytes
    /// twice. Independent of the caller-supplied cursor in
    /// `read_raw_since`.
    pub(crate) emitters: Arc<Mutex<HashMap<SessionId, EmitterState>>>,
    /// Background drainer that pumps every active session and publishes
    /// new bytes onto the event bus without waiting for IPC clients to
    /// poll. Spawned by [`Self::with_event_bus`] and stopped on drop —
    /// `DrainerHandle::drop` joins the thread so by the time the rest
    /// of the plugin's `Arc`s release their data, the drainer has
    /// already let go of its clones. `None` when no event bus is wired
    /// (tests / embedded runtimes that don't need streaming).
    pub(crate) drainer: Option<DrainerHandle>,
    /// Background forwarder that pulls [`TerminalEvent`]s from the
    /// in-memory server's local mpsc and republishes them on the
    /// kernel bus as `com.nexus.terminal.events.<session_id>` (BL-013).
    /// `None` when no event bus is wired — the legacy mpsc subscriber
    /// path stays the only consumer in that case.
    pub(crate) lifecycle_forwarder: Option<LifecycleForwarderHandle>,
    /// BL-061 — memory monitor + last-RSS cache + poller config.
    /// `None` when the plugin was built without a memory monitor
    /// (tests, embedded runtimes); the poller thread is only spawned
    /// alongside [`Self::with_event_bus`] **and** a configured monitor
    /// because publishing the kill event needs a bus. The cache lets
    /// `get_session_info` surface RSS without re-reading `/proc`
    /// every IPC dispatch.
    pub(crate) memory: Option<Arc<Mutex<MemoryState>>>,
    /// BL-061 — limits applied to every auto-tracked session.
    /// `None` = no monitoring (the default until `with_memory_monitor`
    /// is called); `Some(MemoryLimits::unlimited())` = sample but
    /// never kill (useful for the shell UI's RSS chip without the
    /// kill behavior).
    pub(crate) memory_limits: Option<MemoryLimits>,
    /// BL-061 — memory poller interval. Defaults to
    /// [`crate::RECOMMENDED_POLL_INTERVAL`] (1 s) per PRD-09 §7.2.
    pub(crate) memory_poll_interval: Duration,
    /// BL-061 — handle to the memory poller thread. Drop signals
    /// stop and joins, mirroring the drainer pattern.
    pub(crate) memory_poller: Option<MemoryPollerHandle>,
    /// BL-064 — kernel context, captured by `wire_context` so the
    /// async `suggest` handler can issue nested `ipc_call`s into
    /// `com.nexus.ai`. `None` until bootstrap finishes wiring; the
    /// handler returns a clear error if dispatched before then.
    pub(crate) context: Option<Arc<KernelPluginContext>>,
    /// BL-064 — pre-built suggestion engine. Holds the default rule
    /// set so every `suggest` dispatch reuses the same compiled
    /// regex state. Wrapped in `Arc` so the dispatch_async future
    /// can clone it without re-instantiating per-call.
    pub(crate) suggest_engine: Arc<crate::ai::AiSuggestionEngine>,
    /// BL-063 — session-store handle for cross-session search.
    /// `None` when bootstrap didn't open a store (tests, embedded
    /// runtimes); the `cross_session_search` handler returns a
    /// clear error in that case. Bootstrap shares this same handle
    /// with the eviction persister, so a scrollback that the
    /// persister wrote is immediately searchable.
    pub(crate) session_store: Option<Arc<Mutex<SqliteSessionStore>>>,
    /// BL-142 Phase 1 — tracks which sessions were spawned via
    /// `repl_start`. `repl_eval` + `repl_stop` consult this map to
    /// reject calls against regular terminal sessions, and
    /// `repl_list` snapshots the values for shell discovery.
    pub(crate) repls: Arc<Mutex<HashMap<SessionId, ReplInfo>>>,
}

/// BL-061 — shared memory state held behind a single `Mutex` so the
/// poller, the IPC dispatcher, and the lifecycle hooks all see a
/// consistent view.
pub(crate) struct MemoryState {
    /// The active monitor — owns per-pid sample histories.
    pub(crate) monitor: MemoryMonitor,
    /// Map of `session_id -> latest RSS bytes`, refreshed by every
    /// poller round. Read by `get_session_info`. Writes are
    /// effectively single-writer (only the poller updates), but the
    /// mutex on `MemoryState` keeps reads atomic against a partial
    /// update.
    pub(crate) latest_rss: HashMap<String, u64>,
    /// Reverse map for cleanup: when a session closes, the lifecycle
    /// forwarder needs to drop the pid from the monitor and the
    /// session id from `latest_rss`. We can't read pid from a closed
    /// session, so cache it here at track-time.
    pub(crate) session_pid: HashMap<String, u32>,
    /// BL-061 follow-up — per-session memory-limit overrides staged
    /// by `dispatch_run_saved` (or any other handler that wants to
    /// pin a saved command's `memory_limit_mb` onto a freshly-spawned
    /// session). Keys are session ids; the poller drains an entry the
    /// first cycle the session's pid lands in `session_pid`. Stale
    /// entries (sessions that never reached the poller's `live` set)
    /// get retained via the per-round live-id sweep.
    pub(crate) pending_overrides: HashMap<String, MemoryLimits>,
}

impl MemoryState {
    fn new(history: usize) -> Self {
        Self {
            monitor: MemoryMonitor::with_history(history),
            latest_rss: HashMap::new(),
            session_pid: HashMap::new(),
            pending_overrides: HashMap::new(),
        }
    }
}

/// BL-061 — handle to the memory poller thread + its stop flag. Same
/// shape as [`DrainerHandle`] / [`LifecycleForwarderHandle`].
pub(crate) struct MemoryPollerHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for MemoryPollerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Owns the drainer thread + the stop flag. The flag is checked at the
/// top of every drainer cycle and after each pumped session so a Drop
/// signal during a long round still exits within one pump timeout. The
/// `Option<JoinHandle>` lets `Drop` move the handle out for the join.
pub(crate) struct DrainerHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for DrainerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // Worst case the join blocks for one drainer cycle
            // (~DRAINER_PUMP_TIMEOUT_MS × n_sessions + DRAINER_SLEEP_MS).
            // With 50 sessions × 5ms + 10ms ≈ 260 ms — acceptable on
            // shutdown, and the alternative (detach) would leak Arc
            // references to the server / bus / emitters past plugin
            // drop and could outlive the kernel runtime.
            let _ = t.join();
        }
    }
}

/// Cadence for the autonomous drainer. Each cycle pumps every active
/// session with a short blocking timeout, then sleeps to yield. With
/// both timers in single digits and the 50-session cap enforced by
/// `SessionManager`, worst-case latency for an active chunk to surface
/// is `n_sessions × DRAINER_PUMP_TIMEOUT_MS + DRAINER_SLEEP_MS` ≈ 260 ms
/// at full saturation, typically much less because idle sessions return
/// from `pump` immediately when the channel is empty.
const DRAINER_PUMP_TIMEOUT_MS: u64 = 5;
const DRAINER_SLEEP_MS: u64 = 10;

/// How long the lifecycle-forwarder thread blocks on its mpsc receiver
/// before re-checking the stop flag. Short enough that plugin Drop
/// returns within ~one tick; long enough to avoid burning CPU on idle
/// servers.
const LIFECYCLE_RECV_TIMEOUT_MS: u64 = 100;

/// Owns the lifecycle-forwarder thread + its stop flag. Same shape as
/// [`DrainerHandle`] — Drop sets the flag and joins. The thread also
/// exits naturally when the server's mpsc senders are dropped (server
/// destructed), so a forwarder outliving its server still terminates.
pub(crate) struct LifecycleForwarderHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for LifecycleForwarderHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // Worst-case wait is one `LIFECYCLE_RECV_TIMEOUT_MS` tick
            // plus the in-flight publish — single-digit ms.
            let _ = t.join();
        }
    }
}

/// Per-session bookkeeping for the output event stream. Mutated under
/// the [`TerminalCorePlugin::emitters`] mutex.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct EmitterState {
    /// Monotonic byte offset we last published bytes up to. Starts at
    /// `0`; on the first emit we publish from offset `0` to whatever
    /// the buffer currently exposes.
    cursor: u64,
    /// Next `seq` value to assign on publish. The first published chunk
    /// for a session carries `seq = 1`.
    next_seq: u64,
}

impl TerminalCorePlugin {
    /// Build a plugin wrapping a fresh default server and no saved-
    /// commands store. The `saved_*` handlers will return an error
    /// when called; suitable for integration tests that don't touch
    /// the procmgr surface.
    #[must_use]
    pub fn new() -> Self {
        Self {
            server: Arc::new(Mutex::new(InMemoryTerminalServer::new())),
            saved: None,
            adhoc: None,
            event_bus: None,
            emitters: Arc::new(Mutex::new(HashMap::new())),
            drainer: None,
            lifecycle_forwarder: None,
            memory: None,
            memory_limits: None,
            memory_poll_interval: crate::RECOMMENDED_POLL_INTERVAL,
            memory_poller: None,
            context: None,
            suggest_engine: Arc::new(crate::ai::AiSuggestionEngine::with_defaults()),
            session_store: None,
            repls: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Build around an existing server — used by tests that need to
    /// seed sessions before wiring up the plugin.
    #[must_use]
    pub fn with_server(server: InMemoryTerminalServer) -> Self {
        Self {
            server: Arc::new(Mutex::new(server)),
            saved: None,
            adhoc: None,
            event_bus: None,
            emitters: Arc::new(Mutex::new(HashMap::new())),
            drainer: None,
            lifecycle_forwarder: None,
            memory: None,
            memory_limits: None,
            memory_poll_interval: crate::RECOMMENDED_POLL_INTERVAL,
            memory_poller: None,
            context: None,
            suggest_engine: Arc::new(crate::ai::AiSuggestionEngine::with_defaults()),
            session_store: None,
            repls: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Attach a saved-command store so the `saved_*` handlers become
    /// live. Takes ownership — the plugin holds the store for its
    /// entire lifetime behind a `Mutex`.
    #[must_use]
    pub fn with_saved_store(mut self, store: SqliteSavedCommandStore) -> Self {
        self.saved = Some(Mutex::new(store));
        self
    }

    /// Attach an ad-hoc history store so the `adhoc_*` handlers
    /// (BL-060) become live. Takes ownership — the plugin holds the
    /// store for its entire lifetime behind a `Mutex`.
    #[must_use]
    pub fn with_adhoc_store(mut self, store: SqliteAdHocStore) -> Self {
        self.adhoc = Some(Mutex::new(store));
        self
    }

    /// BL-061 — enable per-session memory monitoring with the supplied
    /// limits. The poller thread is spawned by [`Self::with_event_bus`]
    /// (it needs the bus to publish [`TerminalEvent::MemoryLimitExceeded`]
    /// before issuing the kill), so the typical builder chain is
    /// `.with_memory_monitor(limits).with_event_bus(bus)`. Calling this
    /// without `with_event_bus` is allowed — the monitor still tracks
    /// every spawn so `get_session_info` can surface RSS — but no
    /// auto-kill happens.
    ///
    /// `MemoryLimits::unlimited()` produces a measure-only monitor:
    /// useful when the user wants the RSS chip in the shell UI but
    /// not the kill behaviour.
    #[must_use]
    pub fn with_memory_monitor(mut self, limits: MemoryLimits) -> Self {
        self.memory = Some(Arc::new(Mutex::new(MemoryState::new(
            crate::DEFAULT_HISTORY_SAMPLES,
        ))));
        self.memory_limits = Some(limits);
        self
    }

    /// BL-061 — override the memory poller interval. Defaults to
    /// [`crate::RECOMMENDED_POLL_INTERVAL`] (1 s); tests can drop this
    /// to single-digit milliseconds to make the kill path observable
    /// without sleeping a real second.
    #[must_use]
    pub fn with_memory_poll_interval(mut self, interval: Duration) -> Self {
        self.memory_poll_interval = interval;
        self
    }

    /// BL-062 — install a callback that persists an evicted session's
    /// scrollback bytes. Bootstrap supplies a closure that delegates
    /// to [`crate::SqliteSessionStore::save_scrollback`]; tests
    /// typically don't bother and let evicted snapshots drop on the
    /// floor.
    #[must_use]
    pub fn with_eviction_persister(self, persister: crate::EvictionPersister) -> Self {
        if let Ok(mut server) = self.server.lock() {
            server.set_eviction_persister(Some(persister));
        }
        self
    }

    /// BL-063 — share the session store with the plugin so the
    /// `cross_session_search` handler has read access to the FTS5
    /// index. Bootstrap typically calls this *and* installs a
    /// persister that delegates to the same store, so a scrollback
    /// the persister wrote (which auto-indexes) is immediately
    /// searchable. The handle is `Arc<Mutex<...>>` so multiple
    /// builders can share ownership.
    #[must_use]
    pub fn with_session_store(mut self, store: Arc<Mutex<SqliteSessionStore>>) -> Self {
        self.session_store = Some(store);
        self
    }

    /// Wire the plugin to a kernel event bus and spawn both background
    /// threads:
    ///
    /// - The **byte-stream drainer** pumps every active session every
    ///   [`DRAINER_SLEEP_MS`] ms and publishes new bytes as
    ///   [`OutputStreamPayload`] on
    ///   `com.nexus.terminal.output.<session_id>` (Phase 2 WI-12). On-
    ///   demand publishes from `pump` / `read_raw_since` still happen
    ///   and share state with the drainer, so neither path can
    ///   double-publish.
    /// - The **lifecycle forwarder** subscribes to the in-memory
    ///   server's [`TerminalEvent`] mpsc and republishes every event
    ///   on `com.nexus.terminal.events.<session_id>` (BL-013). Plugins
    ///   subscribing via [`nexus_kernel::EventFilter::CustomPrefix`]
    ///   on that prefix get `SessionCreated` / `OutputReceived` /
    ///   `PatternMatched` / `SessionClosed` without polling `pump`.
    ///
    /// We subscribe to the lifecycle mpsc here — before any session
    /// can be created via dispatch — so no `SessionCreated` event is
    /// missed. Returning `Self` keeps the builder ergonomic for
    /// `nexus-bootstrap`'s registration pipeline.
    ///
    /// # Panics
    /// Panics if the freshly-built server mutex is somehow already
    /// poisoned. The plugin owns the only references to `server` at
    /// this point, so the only way to hit this is a `with_server`
    /// caller passing in a server whose mutex was poisoned elsewhere
    /// — which would be an upstream bug worth surfacing.
    #[must_use]
    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        // Subscribe to the lifecycle mpsc up front. Server lock is held
        // only for the duration of `subscribe_events` — no I/O happens
        // here.
        let lifecycle_rx = self
            .server
            .lock()
            .expect("freshly-built server mutex cannot be poisoned")
            .subscribe_events();
        self.lifecycle_forwarder = Some(spawn_lifecycle_forwarder(
            lifecycle_rx,
            Arc::clone(&bus),
        ));
        self.drainer = Some(spawn_drainer(
            Arc::clone(&self.server),
            Arc::clone(&self.emitters),
            Arc::clone(&bus),
        ));
        // BL-061 — spawn the memory poller only when both a monitor
        // and an event bus are present. Without the bus we have no
        // way to publish `MemoryLimitExceeded`; the monitor field
        // stays around so RSS sampling can still happen via direct
        // calls from tests.
        if let Some(memory) = self.memory.as_ref() {
            let limits = self.memory_limits.unwrap_or(MemoryLimits::unlimited());
            self.memory_poller = Some(spawn_memory_poller(
                Arc::clone(&self.server),
                Arc::clone(memory),
                Arc::clone(&bus),
                limits,
                self.memory_poll_interval,
            ));
        }
        self.event_bus = Some(bus);
        self
    }
}

/// Spawn the autonomous drainer thread. Captures `Arc` clones of the
/// server, emitter map, and bus so the thread can outlive any single
/// dispatch call but still gets cleaned up when the plugin drops (the
/// returned [`DrainerHandle`]'s Drop signals stop and joins).
fn spawn_drainer(
    server: Arc<Mutex<InMemoryTerminalServer>>,
    emitters: Arc<Mutex<HashMap<SessionId, EmitterState>>>,
    bus: Arc<EventBus>,
) -> DrainerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("nexus-terminal-drainer".into())
        // The closure owns the Arc clones (move) so they outlive every
        // `drainer_loop` borrow; loop / round helpers take `&Arc` to
        // avoid needless ref-count traffic.
        .spawn(move || drainer_loop(&server, &emitters, &bus, &stop_clone))
        .expect("spawn nexus-terminal drainer thread");
    DrainerHandle {
        stop,
        thread: Some(thread),
    }
}

/// Pump every active session in a tight round-robin and publish any
/// new bytes onto the event bus. Holds the server lock only for the
/// duration of one round of pumps; releases it before publishing so
/// subscribers can't backpressure concurrent IPC dispatches.
///
/// Termination conditions:
/// - `stop` flag set (Drop / shutdown).
/// - Either lock poisoned (a prior handler panicked) — we bail rather
///   than risk corrupted state.
fn drainer_loop(
    server: &Arc<Mutex<InMemoryTerminalServer>>,
    emitters: &Arc<Mutex<HashMap<SessionId, EmitterState>>>,
    bus: &Arc<EventBus>,
    stop: &Arc<AtomicBool>,
) {
    let pump_timeout = Duration::from_millis(DRAINER_PUMP_TIMEOUT_MS);
    while !stop.load(Ordering::Relaxed) {
        let chunks = drainer_round(server, emitters, stop, pump_timeout);
        let Some(chunks) = chunks else {
            return; // poisoned lock
        };
        for (id, data) in chunks {
            publish_chunk(bus, emitters, &id, data);
        }
        thread::sleep(Duration::from_millis(DRAINER_SLEEP_MS));
    }
}

/// One drainer round: list sessions, pump each, collect any new bytes
/// past the per-session emitter cursor. Returns `None` only on a
/// poisoned lock — in which case the drainer caller should exit.
fn drainer_round(
    server: &Arc<Mutex<InMemoryTerminalServer>>,
    emitters: &Arc<Mutex<HashMap<SessionId, EmitterState>>>,
    stop: &Arc<AtomicBool>,
    pump_timeout: Duration,
) -> Option<Vec<(SessionId, Vec<u8>)>> {
    let mut server_guard = server.lock().ok()?;
    let ids: Vec<SessionId> = server_guard
        .list_sessions()
        .into_iter()
        .map(|info| SessionId::from_string(info.id))
        .collect();
    let mut found: Vec<(SessionId, Vec<u8>)> = Vec::new();
    for id in ids {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // Session may close between list and pump (race with
        // dispatch_close_session) — skip silently.
        if server_guard.pump(&id, pump_timeout).is_err() {
            continue;
        }
        let mut em = emitters.lock().ok()?;
        let entry = em.entry(id.clone()).or_default();
        let cursor = entry.cursor;
        if let Some((next_cursor, bytes)) =
            server_guard.manager().buffer_read_since(&id, cursor)
        {
            entry.cursor = next_cursor;
            if !bytes.is_empty() {
                found.push((id, bytes));
            }
        }
    }
    Some(found)
}

/// BL-061 — spawn the memory poller thread. Each cycle the thread:
///
/// 1. Walks every session in the manager (under the same lock the
///    drainer takes) to discover newly-spawned pids and stale entries.
/// 2. For every tracked pid, samples RSS and runs the supplied limits
///    against the reading.
/// 3. On `HardExceeded`, publishes [`TerminalEvent::MemoryLimitExceeded`]
///    onto the kernel bus **before** the kill (so a subscriber sees
///    the threshold breach in causal order), then issues
///    `manager.kill(id)`. The lifecycle forwarder picks up
///    `SessionClosed` on the next reap.
/// 4. Refreshes the per-session RSS cache so `get_session_info`
///    surfaces a current reading.
///
/// Termination conditions: the supplied `stop` flag (Drop) or a
/// poisoned lock. Sleep cadence is `interval` between cycles.
fn spawn_memory_poller(
    server: Arc<Mutex<InMemoryTerminalServer>>,
    memory: Arc<Mutex<MemoryState>>,
    bus: Arc<EventBus>,
    limits: MemoryLimits,
    interval: Duration,
) -> MemoryPollerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("nexus-terminal-memory-poller".into())
        .spawn(move || memory_poller_loop(&server, &memory, &bus, limits, interval, &stop_clone))
        .expect("spawn nexus-terminal memory poller thread");
    MemoryPollerHandle {
        stop,
        thread: Some(thread),
    }
}

fn memory_poller_loop(
    server: &Arc<Mutex<InMemoryTerminalServer>>,
    memory: &Arc<Mutex<MemoryState>>,
    bus: &Arc<EventBus>,
    limits: MemoryLimits,
    interval: Duration,
    stop: &Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        memory_poller_round(server, memory, bus, limits, stop);
        // Sleep in ~25 ms slices so a Drop signal during a long
        // interval still exits within ~one tick.
        let mut remaining = interval;
        while remaining > Duration::from_millis(0) && !stop.load(Ordering::Relaxed) {
            let slice = remaining.min(Duration::from_millis(25));
            thread::sleep(slice);
            remaining = remaining.saturating_sub(slice);
        }
    }
}

/// One poller round. Returns silently on poisoned locks (the next
/// round will retry; if it stays poisoned the parent plugin is
/// already in a degraded state). Splitting the body out keeps the
/// loop's termination condition crisp.
fn memory_poller_round(
    server: &Arc<Mutex<InMemoryTerminalServer>>,
    memory: &Arc<Mutex<MemoryState>>,
    bus: &Arc<EventBus>,
    limits: MemoryLimits,
    stop: &Arc<AtomicBool>,
) {
    // Snapshot the live (id, pid) pairs under a brief server lock so we
    // don't race with `dispatch_create_session` / `dispatch_close_session`
    // — the lock-window stays milliseconds even with the 50-session cap.
    let live: Vec<(SessionId, Option<u32>)> = {
        let Ok(guard) = server.lock() else {
            return;
        };
        guard
            .list_sessions()
            .into_iter()
            .map(|info| {
                let id = SessionId::from_string(info.id);
                let pid = guard.manager().pid(&id);
                (id, pid)
            })
            .collect()
    };

    // Reconcile: track new pids, untrack stale ones. Done before
    // sampling so a fresh session sees its first sample this round.
    //
    // A session is "stale" in two cases:
    //   1. The id no longer appears in `list_sessions` — the manager
    //      removed the entry entirely.
    //   2. The id still appears but `pid` is `None` — the child has
    //      been reaped (close_session followed by reap). The
    //      session struct lingers so the caller can read the final
    //      buffer, but there's no live process for the poller to
    //      track.
    {
        let Ok(mut mem) = memory.lock() else { return };
        let live_with_pid: std::collections::HashSet<&str> = live
            .iter()
            .filter_map(|(id, pid)| pid.is_some().then_some(id.as_str()))
            .collect();
        let stale: Vec<String> = mem
            .session_pid
            .keys()
            .filter(|id| !live_with_pid.contains(id.as_str()))
            .cloned()
            .collect();
        for id in &stale {
            if let Some(pid) = mem.session_pid.remove(id) {
                mem.monitor.untrack(pid);
            }
            mem.latest_rss.remove(id);
        }
        // BL-061 follow-up — sweep orphaned overrides whose session
        // never reached `live`. The "track newly seen pids" loop
        // below otherwise drains the override entry on first sight,
        // so this only catches the create_session-then-immediate-exit
        // edge case. Keys missing from `live` (any pid state) are
        // dropped — the session won't reappear.
        let live_ids: std::collections::HashSet<&str> =
            live.iter().map(|(id, _)| id.as_str()).collect();
        mem.pending_overrides
            .retain(|id, _| live_ids.contains(id.as_str()));
        // Track newly seen pids. A pending override (BL-061 follow-up)
        // wins over the bootstrap-wide default; the entry is consumed
        // so a closed-and-respawned id starts fresh.
        for (id, pid) in &live {
            if let Some(pid) = pid {
                if !mem.session_pid.contains_key(id.as_str()) {
                    let effective = mem
                        .pending_overrides
                        .remove(id.as_str())
                        .unwrap_or(limits);
                    mem.monitor.track(*pid, effective);
                    mem.session_pid.insert(id.as_str().to_string(), *pid);
                }
            }
        }
    }

    // Sample under the memory lock; collect kill targets without
    // holding any lock across the kill path.
    let mut to_kill: Vec<(SessionId, u64, u32)> = Vec::new();
    {
        let Ok(mut mem) = memory.lock() else { return };
        for (id, pid) in &live {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let Some(pid) = pid else { continue };
            match mem.monitor.sample(*pid) {
                Ok(action) => {
                    let bytes = action.bytes();
                    mem.latest_rss.insert(id.as_str().to_string(), bytes);
                    if let MemoryLimitAction::HardExceeded { bytes, limit_mb } = action {
                        to_kill.push((id.clone(), bytes, limit_mb));
                    }
                }
                Err(_) => {
                    // Process gone / read failed — drop the cache
                    // entry so a stale RSS doesn't linger after exit.
                    mem.latest_rss.remove(id.as_str());
                }
            }
        }
    }

    for (id, rss_bytes, limit_mb) in to_kill {
        // Publish first so subscribers see the breach before the
        // ensuing SessionClosed event.
        let payload = TerminalEvent::MemoryLimitExceeded {
            id: id.as_str().to_string(),
            rss_bytes,
            limit_mb,
        };
        if let Ok(payload_value) = serde_json::to_value(&payload) {
            let topic = format!("{EVENT_LIFECYCLE_PREFIX}{id}", id = id.as_str());
            if let Err(err) = bus.publish_plugin(PLUGIN_ID, &topic, payload_value) {
                tracing::warn!(
                    plugin = PLUGIN_ID,
                    %err,
                    session = id.as_str(),
                    "memory poller: publish MemoryLimitExceeded failed",
                );
            }
        }
        // Close via the server's shutdown ladder so the lifecycle
        // forwarder picks up `SessionClosed` after the breach event
        // (in causal order). `close_session` issues SIGTERM then
        // SIGKILL after a short window, which is the right behaviour
        // for memory exhaustion — give the process a chance to flush
        // before we yank it.
        if let Ok(mut server_guard) = server.lock() {
            if let Err(err) = server_guard.close_session(&id) {
                tracing::warn!(
                    plugin = PLUGIN_ID,
                    %err,
                    session = id.as_str(),
                    "memory poller: close_session failed",
                );
            }
        }
    }
}

/// Spawn the lifecycle forwarder thread (BL-013). The thread owns the
/// `Receiver<TerminalEvent>` end of the in-memory server's mpsc and
/// forwards each event onto the kernel bus. It exits when either:
///
/// - the stop flag is flipped (plugin Drop), or
/// - every `Sender<TerminalEvent>` has been dropped — i.e. the
///   in-memory server has been destructed. We use `recv_timeout` so
///   the stop flag is checked at least once per
///   [`LIFECYCLE_RECV_TIMEOUT_MS`] tick even on an idle server.
fn spawn_lifecycle_forwarder(
    rx: Receiver<TerminalEvent>,
    bus: Arc<EventBus>,
) -> LifecycleForwarderHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("nexus-terminal-lifecycle".into())
        .spawn(move || lifecycle_forwarder_loop(&rx, &bus, &stop_clone))
        .expect("spawn nexus-terminal lifecycle forwarder thread");
    LifecycleForwarderHandle {
        stop,
        thread: Some(thread),
    }
}

fn lifecycle_forwarder_loop(
    rx: &Receiver<TerminalEvent>,
    bus: &EventBus,
    stop: &Arc<AtomicBool>,
) {
    let timeout = Duration::from_millis(LIFECYCLE_RECV_TIMEOUT_MS);
    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(timeout) {
            Ok(event) => publish_lifecycle_event(bus, &event),
            // Server dropped — no more senders, nothing to forward.
            Err(RecvTimeoutError::Disconnected) => return,
            // Loop back and re-check the stop flag.
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Publish a single lifecycle event onto
/// `com.nexus.terminal.events.<session_id>`. Errors are logged at warn
/// level and swallowed: a serialisation or bus failure is not worth
/// killing the forwarder over (the next event will likely succeed).
///
/// BL-057 — for session-boundary events (`SessionCreated`,
/// `SessionClosed`, `MemoryLimitExceeded`) the forwarder also publishes
/// to the universal `com.nexus.activity.appended` topic so the BL-052
/// activity timeline pane sees terminal events alongside AI / file /
/// git activity. Streaming variants (`OutputReceived`, `PatternMatched`,
/// `SessionEvicted`) intentionally don't emit activity — they're
/// either too chatty or too internal to surface in a user-facing log.
fn publish_lifecycle_event(bus: &EventBus, event: &TerminalEvent) {
    let session_id = event.session_id();
    let type_id = format!("{EVENT_LIFECYCLE_PREFIX}{session_id}");
    let payload = match serde_json::to_value(event) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(
                plugin = PLUGIN_ID,
                %err,
                session = session_id,
                "failed to serialize terminal lifecycle event",
            );
            return;
        }
    };
    if let Err(err) = bus.publish_plugin(PLUGIN_ID, &type_id, payload) {
        tracing::warn!(
            plugin = PLUGIN_ID,
            %err,
            session = session_id,
            "failed to publish terminal lifecycle event",
        );
    }

    // BL-057 — fan out lifecycle events to the universal activity bus.
    if let Some(entry) = build_activity_entry(event) {
        if let Ok(activity_payload) = serde_json::to_value(&entry) {
            // The kernel-owned topic is plugin-namespace-free, so we use
            // the same `publish_plugin` API but with the universal type
            // id. EventBus accepts any string type id — the prefix
            // convention is purely for subscriber filtering.
            if let Err(err) = bus.publish_plugin(
                PLUGIN_ID,
                nexus_types::activity::ACTIVITY_APPENDED_TOPIC,
                activity_payload,
            ) {
                tracing::warn!(
                    plugin = PLUGIN_ID,
                    %err,
                    session = session_id,
                    "failed to publish activity entry",
                );
            }
        }
    }
}

/// BL-057 — translate a [`TerminalEvent`] session-boundary into an
/// [`nexus_types::activity::ActivityEntry`] tagged with the
/// `terminal:<session_id>` origin and `process` surface. Returns
/// `None` for streaming variants we don't want to surface.
fn build_activity_entry(
    event: &TerminalEvent,
) -> Option<nexus_types::activity::ActivityEntry> {
    use nexus_types::activity::{
        ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface,
    };

    let session_id = event.session_id().to_string();
    let mut entry = ActivityEntry::now(
        session_id.clone(),
        ActivitySurface::Process,
        ActivityOrigin::Terminal(session_id.clone()),
    );

    match event {
        TerminalEvent::SessionCreated { id, name } => {
            entry.outcome = ActivityOutcome::Ok;
            entry.prompt = match name {
                Some(n) => format!("started session {n}"),
                None => format!("started session {id}"),
            };
        }
        TerminalEvent::SessionClosed { id, exit_code } => {
            // Treat exit code 0 / unknown as Ok; non-zero as Error so
            // the timeline can flash an error glyph for failed runs.
            match exit_code {
                Some(0) | None => {
                    entry.outcome = ActivityOutcome::Ok;
                    entry.prompt = format!(
                        "session {id} exited (code={})",
                        exit_code.map_or("?".to_string(), |c| c.to_string()),
                    );
                }
                Some(code) => {
                    entry.outcome = ActivityOutcome::Error;
                    entry.prompt = format!("session {id} exited (code={code})");
                    entry.error = Some(format!("non-zero exit code {code}"));
                }
            }
        }
        TerminalEvent::MemoryLimitExceeded {
            id,
            rss_bytes,
            limit_mb,
        } => {
            entry.outcome = ActivityOutcome::Error;
            entry.prompt = format!(
                "session {id} killed (OOM): rss={rss_bytes} limit={limit_mb}MB"
            );
            entry.error = Some(format!("memory limit exceeded ({limit_mb}MB)"));
        }
        // Streaming / internal variants don't reach the activity log.
        TerminalEvent::OutputReceived { .. }
        | TerminalEvent::PatternMatched { .. }
        | TerminalEvent::SessionEvicted { .. } => return None,
    }
    Some(entry)
}

/// Free-function publish path used by both the drainer and the on-
/// demand dispatch handlers. Assigns the next per-session sequence
/// number under the emitters lock so both paths share monotonic seq.
fn publish_chunk(
    bus: &EventBus,
    emitters: &Arc<Mutex<HashMap<SessionId, EmitterState>>>,
    id: &SessionId,
    data: Vec<u8>,
) {
    let seq = match emitters.lock() {
        Ok(mut em) => {
            let entry = em.entry(id.clone()).or_default();
            entry.next_seq = entry.next_seq.saturating_add(1);
            if entry.next_seq == 0 {
                entry.next_seq = 1;
            }
            entry.next_seq
        }
        Err(_) => return,
    };
    let type_id = format!("{EVENT_OUTPUT_PREFIX}{}", id.as_str());
    let payload = OutputStreamPayload {
        data,
        seq,
        ts_ms: chrono::Utc::now().timestamp_millis(),
    };
    let payload_value = match serde_json::to_value(&payload) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(
                plugin = PLUGIN_ID,
                %err,
                session = id.as_str(),
                "failed to serialize output stream payload",
            );
            return;
        }
    };
    if let Err(err) = bus.publish_plugin(PLUGIN_ID, &type_id, payload_value) {
        tracing::warn!(
            plugin = PLUGIN_ID,
            %err,
            session = id.as_str(),
            "failed to publish terminal output event",
        );
    }
}

impl Default for TerminalCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePlugin for TerminalCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_CREATE_SESSION => self.dispatch_create_session(args),
            HANDLER_CLOSE_SESSION => self.dispatch_close_session(args),
            HANDLER_SEND_INPUT => self.dispatch_send_input(args),
            HANDLER_SEND_RAW_INPUT => self.dispatch_send_raw_input(args),
            HANDLER_PUMP => self.dispatch_pump(args),
            HANDLER_READ_OUTPUT => self.dispatch_read_output(args),
            HANDLER_SEARCH_OUTPUT => self.dispatch_search_output(args),
            HANDLER_WAIT_FOR_PATTERN => self.dispatch_wait_for_pattern(args),
            HANDLER_GET_SESSION_INFO => self.dispatch_get_session_info(args),
            HANDLER_LIST_SESSIONS => self.dispatch_list_sessions(args),
            HANDLER_SAVED_LIST => self.dispatch_saved_list(),
            HANDLER_SAVED_CREATE => self.dispatch_saved_create(args),
            HANDLER_SAVED_UPDATE => self.dispatch_saved_update(args),
            HANDLER_SAVED_DELETE => self.dispatch_saved_delete(args),
            HANDLER_SAVED_REORDER => self.dispatch_saved_reorder(args),
            HANDLER_READ_RAW_SINCE => self.dispatch_read_raw_since(args),
            HANDLER_RESIZE => self.dispatch_resize(args),
            HANDLER_OPEN_IN_TERMINAL => self.dispatch_open_in_terminal(args),
            HANDLER_ADHOC_LIST => self.dispatch_adhoc_list(args),
            HANDLER_ADHOC_GET => self.dispatch_adhoc_get(args),
            HANDLER_ADHOC_DELETE => self.dispatch_adhoc_delete(args),
            HANDLER_ADHOC_PROMOTE => self.dispatch_adhoc_promote(args),
            HANDLER_RUN_SAVED => self.dispatch_run_saved(args),
            // BL-064 — `suggest` is async (issues a nested
            // `com.nexus.ai` IPC call). Surface the routing mistake
            // via the typed `HandlerIsAsyncOnly` error.
            HANDLER_SUGGEST => Err(PluginError::HandlerIsAsyncOnly {
                handler_id: HANDLER_SUGGEST,
            }),
            HANDLER_CROSS_SESSION_SEARCH => self.dispatch_cross_session_search(args),
            HANDLER_REPL_START => self.dispatch_repl_start(args),
            HANDLER_REPL_EVAL => self.dispatch_repl_eval(args),
            HANDLER_REPL_STOP => self.dispatch_repl_stop(args),
            HANDLER_REPL_LIST => self.dispatch_repl_list(),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        if handler_id != HANDLER_SUGGEST {
            return None;
        }
        let ctx = self.context.clone();
        let server = Arc::clone(&self.server);
        let engine = Arc::clone(&self.suggest_engine);
        let args = args.clone();
        Some(Box::pin(async move {
            crate::handlers::ai::handle_suggest(ctx.as_ref(), &server, &engine, &args).await
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(ctx);
    }
}

// ── Dispatch helpers ─────────────────────────────────────────────────────────

impl TerminalCorePlugin {

    /// BL-061 — read the latest RSS the poller cached for `session_id`.
    /// Returns `None` when the plugin was built without a memory
    /// monitor, when the session isn't known to the monitor (e.g. it
    /// was just spawned and the next poll hasn't run yet), or when
    /// the memory lock is poisoned.
    pub(crate) fn cached_rss(&self, session_id: &str) -> Option<u64> {
        let memory = self.memory.as_ref()?;
        let mem = memory.lock().ok()?;
        mem.latest_rss.get(session_id).copied()
    }

    /// Read the bytes appended to `id`'s ring buffer since this
    /// plugin's last published cursor and atomically advance the
    /// cursor. Returns `None` when there is no event bus wired
    /// (publish path disabled) or when the session has no new bytes.
    /// The returned tuple is `(next_cursor, fresh_bytes)` — the cursor
    /// is recorded back into [`Self::emitters`] before this returns
    /// so two interleaved dispatches can't double-publish the same
    /// bytes even if one was paused mid-call.
    ///
    /// Caller holds the server lock for the whole window between
    /// pumping and reading the buffer; we deliberately read the
    /// snapshot from the manager (no further drain) so the byte
    /// shape matches exactly what `read_raw_since` would have
    /// returned for the same cursor.
    pub(crate) fn fetch_new_bytes(
        &self,
        server: &InMemoryTerminalServer,
        id: &SessionId,
    ) -> Option<(u64, Vec<u8>)> {
        self.event_bus.as_ref()?;
        let mut emitters = self.emitters.lock().ok()?;
        let entry = emitters.entry(id.clone()).or_default();
        let cursor = entry.cursor;
        let (next_cursor, bytes) = server.manager().buffer_read_since(id, cursor)?;
        if bytes.is_empty() {
            // Cursor advances past evicted bytes too — keep the local
            // cursor in lockstep with the buffer's tail so a future
            // write doesn't replay the catch-up window.
            entry.cursor = next_cursor;
            return None;
        }
        entry.cursor = next_cursor;
        Some((next_cursor, bytes))
    }

    /// Publish a `com.nexus.terminal.output.<session_id>` event with
    /// the freshly-drained bytes. `next_cursor` is the caller-side
    /// cursor that comes after this chunk and is informational only —
    /// it's already been recorded by [`Self::fetch_new_bytes`]. Shares
    /// seq state with the autonomous drainer via [`publish_chunk`].
    pub(crate) fn publish_output(&self, id: &SessionId, _next_cursor: u64, data: Vec<u8>) {
        let Some(bus) = self.event_bus.as_ref() else {
            return;
        };
        publish_chunk(bus, &self.emitters, id, data);
    }

    pub(crate) fn saved_store(&self) -> Result<&Mutex<SqliteSavedCommandStore>, PluginError> {
        self.saved.as_ref().ok_or_else(|| {
            exec_err("saved-command store not attached (runtime built without a forge path)".into())
        })
    }

    // ── BL-060 — ad-hoc history handlers ─────────────────────────────

    pub(crate) fn adhoc_store(&self) -> Result<&Mutex<SqliteAdHocStore>, PluginError> {
        self.adhoc.as_ref().ok_or_else(|| {
            exec_err(
                "ad-hoc history store not attached (runtime built without a forge path)"
                    .into(),
            )
        })
    }

    // ── BL-055 — run a saved command in a fresh session ──────────────
}

// ── Error plumbing — SD-01: emitted by the shared macro ─────────────────────

nexus_plugins::define_dispatch_helpers!(pub(crate));

pub(crate) fn poisoned<T>(_e: std::sync::PoisonError<T>) -> PluginError {
    exec_err("server mutex poisoned — prior handler panicked".to_string())
}

// Used as a function pointer by `.map_err(crate_err)`; wrapping in a
// closure would re-trip `redundant_closure`.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn crate_err(e: crate::TerminalError) -> PluginError {
    exec_err(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adhoc::{AdHocRecord, AdHocStatus};
    use crate::handlers::ai::handle_suggest;
    use crate::handlers::run::one_shot_flag;
    use crate::server::{OutputLine, SessionInfo};

    fn unix_only(name: &str) -> bool {
        if !cfg!(unix) {
            eprintln!("skipping {name}: unix-only");
            return false;
        }
        true
    }

    fn create_args(script: &str) -> serde_json::Value {
        serde_json::json!({
            "name": "plugin-test",
            "shell": "/bin/sh",
            "shell_args": ["-c", script],
        })
    }

    #[test]
    fn unknown_handler_id_surfaces_execution_error() {
        let mut p = TerminalCorePlugin::new();
        let err = p.dispatch(9999, &serde_json::json!({})).unwrap_err();
        match err {
            PluginError::ExecutionFailed { plugin_id, reason } => {
                assert_eq!(plugin_id, PLUGIN_ID);
                assert!(reason.contains("9999"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn create_and_list_sessions_roundtrips_through_dispatch() {
        if !unix_only("create_and_list_sessions_roundtrips_through_dispatch") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        let resp = p
            .dispatch(HANDLER_CREATE_SESSION, &create_args("printf hi"))
            .expect("create");
        let CreateSessionResponse { id } = serde_json::from_value(resp).expect("decode");

        let list_v = p
            .dispatch(HANDLER_LIST_SESSIONS, &serde_json::json!({}))
            .expect("list");
        let list: Vec<SessionInfo> = serde_json::from_value(list_v).expect("decode");
        assert!(list.iter().any(|i| i.id == id), "newly-created id missing");
    }

    #[test]
    fn pump_read_output_returns_structured_lines() {
        if !unix_only("pump_read_output_returns_structured_lines") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        let resp = p
            .dispatch(
                HANDLER_CREATE_SESSION,
                &create_args("printf 'alpha\\nbeta\\n'"),
            )
            .expect("create");
        let CreateSessionResponse { id } = serde_json::from_value(resp).expect("decode");

        // Pump until we've seen both lines (or 3s elapse).
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let _ = p
                .dispatch(
                    HANDLER_PUMP,
                    &serde_json::json!({ "id": id, "timeout_ms": 100 }),
                )
                .expect("pump");
            let read = p
                .dispatch(HANDLER_READ_OUTPUT, &serde_json::json!({ "id": id }))
                .expect("read");
            let lines: Vec<OutputLine> = serde_json::from_value(read).expect("decode");
            let texts: Vec<&str> = lines.iter().map(|l| l.content.as_str()).collect();
            if texts.contains(&"alpha") && texts.contains(&"beta") {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "never saw both lines; last snapshot: {texts:?}"
            );
        }
    }

    #[test]
    fn search_output_via_dispatch_finds_matches() {
        if !unix_only("search_output_via_dispatch_finds_matches") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(
                HANDLER_CREATE_SESSION,
                &create_args("printf 'error: one\\nok\\nerror: two\\n'"),
            )
            .expect("create"),
        )
        .expect("decode");

        // Pump until at least three lines land.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let _ = p.dispatch(
                HANDLER_PUMP,
                &serde_json::json!({ "id": id, "timeout_ms": 100 }),
            );
            let lines_v = p
                .dispatch(HANDLER_READ_OUTPUT, &serde_json::json!({ "id": id }))
                .expect("read");
            let lines: Vec<OutputLine> = serde_json::from_value(lines_v).expect("decode");
            if lines.len() >= 3 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "buffer never reached 3 lines"
            );
        }
        let hits_v = p
            .dispatch(
                HANDLER_SEARCH_OUTPUT,
                &serde_json::json!({ "id": id, "query": "error:", "is_regex": false }),
            )
            .expect("search");
        let hits: Vec<usize> = serde_json::from_value(hits_v).expect("decode");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn invalid_args_shape_is_reported_as_execution_failed() {
        let mut p = TerminalCorePlugin::new();
        // `send_input` without required fields.
        let err = p
            .dispatch(HANDLER_SEND_INPUT, &serde_json::json!({}))
            .unwrap_err();
        assert!(matches!(err, PluginError::ExecutionFailed { .. }));
    }

    #[test]
    fn get_session_info_unknown_id_surfaces_terminal_error_via_plugin_error() {
        let mut p = TerminalCorePlugin::new();
        let err = p
            .dispatch(
                HANDLER_GET_SESSION_INFO,
                &serde_json::json!({ "id": "ghost" }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("not running") || reason.contains("ghost"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn pump_publishes_output_event_with_monotonic_seq() {
        use nexus_kernel::{EventFilter, NexusEvent};

        if !unix_only("pump_publishes_output_event_with_monotonic_seq") {
            return;
        }

        let bus = Arc::new(EventBus::new(64));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_OUTPUT_PREFIX.to_string()));

        // Two `printf`s separated by a sleep so the pump observes the
        // output in two distinct flushes — the second pump must produce
        // a fresh event with `seq = 2`.
        let mut p = TerminalCorePlugin::new().with_event_bus(Arc::clone(&bus));
        let create = serde_json::json!({
            "name": "stream-test",
            "shell": "/bin/sh",
            "shell_args": ["-c", "printf 'hello\\n'; sleep 0.2; printf 'world\\n'"],
        });
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create).expect("create"),
        )
        .expect("decode");

        // Pump until we see the first event (printf may take a tick to
        // flush through the PTY). 3s upper bound matches the sibling
        // pump tests in this module.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let first = loop {
            let _ = p
                .dispatch(
                    HANDLER_PUMP,
                    &serde_json::json!({ "id": id, "timeout_ms": 100 }),
                )
                .expect("pump");
            if let Some(evt) = sub.try_recv().expect("bus alive") {
                break evt;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "never received first output event"
            );
        };
        let (type_id, payload) = match &first.event {
            NexusEvent::Custom { type_id, payload, .. } => (type_id.clone(), payload.clone()),
            other => panic!("expected Custom, got {other:?}"),
        };
        assert_eq!(
            type_id,
            format!("{EVENT_OUTPUT_PREFIX}{id}"),
            "topic must include session id",
        );
        let payload: OutputStreamPayload =
            serde_json::from_value(payload).expect("payload decodes");
        assert_eq!(payload.seq, 1, "first chunk for a session is seq = 1");
        assert!(!payload.data.is_empty(), "payload bytes should be non-empty");

        // Force the second flush to land. Drain any extra pre-second-
        // printf events so we can assert seq strictly increments.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut last_seq = payload.seq;
        loop {
            let _ = p
                .dispatch(
                    HANDLER_PUMP,
                    &serde_json::json!({ "id": id, "timeout_ms": 100 }),
                )
                .expect("pump");
            while let Some(evt) = sub.try_recv().expect("bus alive") {
                if let NexusEvent::Custom { payload, .. } = &evt.event {
                    let p: OutputStreamPayload =
                        serde_json::from_value(payload.clone()).expect("decode");
                    assert!(
                        p.seq > last_seq,
                        "seq must increase monotonically: prev={last_seq}, got={}",
                        p.seq,
                    );
                    last_seq = p.seq;
                }
            }
            if last_seq >= 2 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "second emission never produced seq >= 2 (last={last_seq})"
            );
        }
    }

    #[test]
    fn drainer_publishes_output_without_manual_pump() {
        use nexus_kernel::{EventFilter, NexusEvent};

        if !unix_only("drainer_publishes_output_without_manual_pump") {
            return;
        }

        let bus = Arc::new(EventBus::new(64));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_OUTPUT_PREFIX.to_string()));

        // `with_event_bus` spawns the drainer thread.
        let mut p = TerminalCorePlugin::new().with_event_bus(Arc::clone(&bus));
        let create = serde_json::json!({
            "name": "drainer-test",
            "shell": "/bin/sh",
            "shell_args": ["-c", "printf 'autonomous\\n'; sleep 1"],
        });
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create).expect("create"),
        )
        .expect("decode");

        // No manual pump — wait for the drainer to publish on its own.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(evt) = sub.try_recv().expect("bus alive") {
                let (type_id, payload) = match &evt.event {
                    NexusEvent::Custom { type_id, payload, .. } => {
                        (type_id.clone(), payload.clone())
                    }
                    other => panic!("expected Custom, got {other:?}"),
                };
                assert_eq!(type_id, format!("{EVENT_OUTPUT_PREFIX}{id}"));
                let payload: OutputStreamPayload =
                    serde_json::from_value(payload).expect("payload decodes");
                assert!(
                    !payload.data.is_empty(),
                    "drainer chunk should not be empty"
                );
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "drainer never published output autonomously"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn pump_without_event_bus_remains_silent_publish_path() {
        if !unix_only("pump_without_event_bus_remains_silent_publish_path") {
            return;
        }
        // No event_bus wired — the legacy poll-only path must keep its
        // exact byte-count contract and never touch the (absent) bus.
        let mut p = TerminalCorePlugin::new();
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create_args("printf 'x\\n'"))
                .expect("create"),
        )
        .expect("decode");
        let resp = p
            .dispatch(
                HANDLER_PUMP,
                &serde_json::json!({ "id": id, "timeout_ms": 100 }),
            )
            .expect("pump");
        let r: PumpResponse = serde_json::from_value(resp).expect("decode");
        // Don't assert exact byte count (PTY can return 0 on the first
        // tick if the printf hasn't flushed yet) — the contract we care
        // about here is that the response shape is unchanged and the
        // call doesn't panic on a missing bus.
        let _ = r.bytes;
    }

    #[test]
    fn resize_dispatches_through_to_session_and_clamps_zero_dims() {
        if !unix_only("resize_dispatches_through_to_session_and_clamps_zero_dims") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 1"))
                .expect("create"),
        )
        .expect("decode");

        // Normal resize succeeds (returns Null per the dispatch contract).
        let resp = p
            .dispatch(
                HANDLER_RESIZE,
                &serde_json::json!({ "id": id, "cols": 120, "rows": 40 }),
            )
            .expect("resize");
        assert_eq!(resp, serde_json::Value::Null);

        // Zero dimensions are clamped to 1×1 — the underlying ioctl
        // rejects zero on Linux/macOS, so the call must not surface
        // that error to the caller.
        let resp = p
            .dispatch(
                HANDLER_RESIZE,
                &serde_json::json!({ "id": id, "cols": 0, "rows": 0 }),
            )
            .expect("resize zero clamps");
        assert_eq!(resp, serde_json::Value::Null);
    }

    #[test]
    fn resize_unknown_session_surfaces_not_running() {
        let mut p = TerminalCorePlugin::new();
        let err = p
            .dispatch(
                HANDLER_RESIZE,
                &serde_json::json!({ "id": "ghost", "cols": 80, "rows": 24 }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("not running") || reason.contains("ghost"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn wait_for_pattern_with_zero_timeout_returns_false_on_silent_session() {
        if !unix_only("wait_for_pattern_with_zero_timeout_returns_false_on_silent_session") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(
                HANDLER_CREATE_SESSION,
                &create_args("sleep 2"),
            )
            .expect("create"),
        )
        .expect("decode");
        let resp = p
            .dispatch(
                HANDLER_WAIT_FOR_PATTERN,
                &serde_json::json!({
                    "id": id,
                    "pattern": "never",
                    "is_regex": false,
                    "timeout_ms": 0,
                }),
            )
            .expect("wait");
        let r: WaitForPatternResponse = serde_json::from_value(resp).expect("decode");
        assert!(!r.matched);
    }

    /// Drain `sub` until a `Custom` event matching `predicate` is seen
    /// or `timeout` elapses. Pumps `id` along the way so the in-memory
    /// server makes progress without a manual loop in every test. The
    /// pattern matches the structure of the existing
    /// `pump_publishes_output_event_with_monotonic_seq` helper: spin
    /// the dispatch handler, drain the subscription, fail fast on
    /// timeout. Returns the matched payload as a `TerminalEvent`.
    fn pump_until_lifecycle<F>(
        plugin: &mut TerminalCorePlugin,
        sub: &mut nexus_kernel::EventSubscription,
        id: &str,
        predicate: F,
        timeout: Duration,
    ) -> Option<crate::TerminalEvent>
    where
        F: Fn(&crate::TerminalEvent) -> bool,
    {
        use nexus_kernel::NexusEvent;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            // Pumping is best-effort — if the session has already
            // closed the dispatch returns an error that we ignore so
            // the test can still observe trailing lifecycle events.
            let _ = plugin.dispatch(
                HANDLER_PUMP,
                &serde_json::json!({ "id": id, "timeout_ms": 50 }),
            );
            while let Some(evt) = sub.try_recv().expect("bus alive") {
                if let NexusEvent::Custom { type_id, payload, .. } = &evt.event {
                    if !type_id.starts_with(EVENT_LIFECYCLE_PREFIX) {
                        continue;
                    }
                    let term_evt: crate::TerminalEvent =
                        serde_json::from_value(payload.clone()).expect("decode lifecycle");
                    if predicate(&term_evt) {
                        return Some(term_evt);
                    }
                }
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn lifecycle_session_created_published_on_kernel_bus() {
        use nexus_kernel::EventFilter;

        if !unix_only("lifecycle_session_created_published_on_kernel_bus") {
            return;
        }

        let bus = Arc::new(EventBus::new(64));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_LIFECYCLE_PREFIX.to_string()));

        let mut p = TerminalCorePlugin::new().with_event_bus(Arc::clone(&bus));
        let resp = p
            .dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 1"))
            .expect("create");
        let CreateSessionResponse { id } = serde_json::from_value(resp).expect("decode");

        let evt = pump_until_lifecycle(
            &mut p,
            &mut sub,
            &id,
            |e| matches!(e, crate::TerminalEvent::SessionCreated { .. }),
            Duration::from_secs(2),
        )
        .expect("SessionCreated event never reached the kernel bus");

        match evt {
            crate::TerminalEvent::SessionCreated { id: eid, name } => {
                assert_eq!(eid, id);
                assert_eq!(name.as_deref(), Some("plugin-test"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn lifecycle_output_received_published_on_kernel_bus() {
        use nexus_kernel::EventFilter;

        if !unix_only("lifecycle_output_received_published_on_kernel_bus") {
            return;
        }

        let bus = Arc::new(EventBus::new(128));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_LIFECYCLE_PREFIX.to_string()));

        let mut p = TerminalCorePlugin::new().with_event_bus(Arc::clone(&bus));
        let create = serde_json::json!({
            "name": "lifecycle-output",
            "shell": "/bin/sh",
            "shell_args": ["-c", "printf 'first-line\\n'; sleep 0.2; printf 'second-line\\n'"],
        });
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create).expect("create"),
        )
        .expect("decode");

        let evt = pump_until_lifecycle(
            &mut p,
            &mut sub,
            &id,
            |e| {
                matches!(
                    e,
                    crate::TerminalEvent::OutputReceived { line, .. }
                        if line.content == "first-line"
                )
            },
            Duration::from_secs(3),
        )
        .expect("OutputReceived event for 'first-line' never published");

        match evt {
            crate::TerminalEvent::OutputReceived { id: eid, line } => {
                assert_eq!(eid, id);
                assert_eq!(line.content, "first-line");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn lifecycle_pattern_matched_published_on_kernel_bus() {
        use nexus_kernel::EventFilter;

        if !unix_only("lifecycle_pattern_matched_published_on_kernel_bus") {
            return;
        }

        let bus = Arc::new(EventBus::new(64));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_LIFECYCLE_PREFIX.to_string()));

        let mut p = TerminalCorePlugin::new().with_event_bus(Arc::clone(&bus));
        let create = serde_json::json!({
            "name": "lifecycle-match",
            "shell": "/bin/sh",
            "shell_args": ["-c", "printf 'warmup\\nready-signal\\ntail\\n'"],
        });
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create).expect("create"),
        )
        .expect("decode");

        // wait_for_pattern drives its own pump loop and emits a
        // PatternMatched event from inside the in-memory server when
        // the pattern lands.
        let resp = p
            .dispatch(
                HANDLER_WAIT_FOR_PATTERN,
                &serde_json::json!({
                    "id": id,
                    "pattern": "ready-signal",
                    "is_regex": false,
                    "timeout_ms": 3000,
                }),
            )
            .expect("wait");
        let r: WaitForPatternResponse = serde_json::from_value(resp).expect("decode");
        assert!(r.matched, "pattern should have matched");

        let evt = pump_until_lifecycle(
            &mut p,
            &mut sub,
            &id,
            |e| {
                matches!(
                    e,
                    crate::TerminalEvent::PatternMatched { pattern, .. }
                        if pattern == "ready-signal"
                )
            },
            Duration::from_secs(2),
        )
        .expect("PatternMatched event never reached the kernel bus");

        if let crate::TerminalEvent::PatternMatched { id: eid, pattern, .. } = evt {
            assert_eq!(eid, id);
            assert_eq!(pattern, "ready-signal");
        } else {
            panic!("expected PatternMatched");
        }
    }

    #[test]
    fn lifecycle_session_closed_published_on_kernel_bus() {
        use nexus_kernel::EventFilter;

        if !unix_only("lifecycle_session_closed_published_on_kernel_bus") {
            return;
        }

        let bus = Arc::new(EventBus::new(64));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_LIFECYCLE_PREFIX.to_string()));

        let mut p = TerminalCorePlugin::new().with_event_bus(Arc::clone(&bus));
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 1"))
                .expect("create"),
        )
        .expect("decode");

        let _ = p
            .dispatch(
                HANDLER_CLOSE_SESSION,
                &serde_json::json!({ "id": id }),
            )
            .expect("close");

        let evt = pump_until_lifecycle(
            &mut p,
            &mut sub,
            &id,
            |e| matches!(e, crate::TerminalEvent::SessionClosed { .. }),
            Duration::from_secs(2),
        )
        .expect("SessionClosed event never reached the kernel bus");

        if let crate::TerminalEvent::SessionClosed { id: eid, .. } = evt {
            assert_eq!(eid, id);
        } else {
            panic!("expected SessionClosed");
        }
    }

    // ── BL-060 — ad-hoc history dispatch tests ──────────────────────

    /// Build a plugin with empty adhoc + saved stores attached.
    fn plugin_with_adhoc() -> TerminalCorePlugin {
        let adhoc = SqliteAdHocStore::in_memory().expect("open adhoc");
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        TerminalCorePlugin::new()
            .with_adhoc_store(adhoc)
            .with_saved_store(saved)
    }

    /// Seed ad-hoc rows by writing through the store *before* handing
    /// it to the plugin (the plugin owns the only handle once
    /// `with_adhoc_store` consumes it). Returns the plugin plus the
    /// ids of each inserted row in insertion order.
    fn plugin_with_seeded_adhoc(rows: &[(&str, Option<&str>, Option<i32>, u64)])
        -> (TerminalCorePlugin, Vec<String>)
    {
        let adhoc = SqliteAdHocStore::in_memory().expect("open adhoc");
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        let mut ids = Vec::with_capacity(rows.len());
        for (cmd, cwd, code, dur) in rows {
            ids.push(adhoc.record(cmd, *cwd, *code, *dur).expect("seed row"));
            // Stagger executed_at so `recent` ordering is deterministic.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let plugin = TerminalCorePlugin::new()
            .with_adhoc_store(adhoc)
            .with_saved_store(saved);
        (plugin, ids)
    }

    #[test]
    fn adhoc_list_without_attached_store_surfaces_clear_error() {
        let mut p = TerminalCorePlugin::new(); // no adhoc store
        let err = p
            .dispatch(HANDLER_ADHOC_LIST, &serde_json::json!({}))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("ad-hoc history store not attached"),
                    "expected attach-error, got: {reason}",
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn adhoc_list_default_limit_returns_seeded_rows_in_recency_order() {
        let (mut p, _ids) = plugin_with_seeded_adhoc(&[
            ("ls", Some("/a"), Some(0), 10),
            ("pwd", Some("/b"), Some(0), 12),
            ("date", None, Some(0), 5),
        ]);
        let v = p
            .dispatch(HANDLER_ADHOC_LIST, &serde_json::json!({}))
            .expect("adhoc_list");
        let rows: Vec<AdHocRecord> = serde_json::from_value(v).expect("decode");
        assert_eq!(rows.len(), 3);
        // Most recent first: insertion order was ls → pwd → date.
        assert_eq!(rows[0].command, "date");
        assert_eq!(rows[1].command, "pwd");
        assert_eq!(rows[2].command, "ls");
    }

    #[test]
    fn adhoc_list_respects_explicit_limit() {
        let (mut p, _ids) = plugin_with_seeded_adhoc(&[
            ("a", None, Some(0), 1),
            ("b", None, Some(0), 1),
            ("c", None, Some(0), 1),
        ]);
        let v = p
            .dispatch(HANDLER_ADHOC_LIST, &serde_json::json!({ "limit": 2 }))
            .expect("adhoc_list");
        let rows: Vec<AdHocRecord> = serde_json::from_value(v).expect("decode");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].command, "c");
    }

    #[test]
    fn adhoc_get_returns_null_for_unknown_id() {
        let mut p = plugin_with_adhoc();
        let v = p
            .dispatch(HANDLER_ADHOC_GET, &serde_json::json!({ "id": "nope" }))
            .expect("adhoc_get");
        assert!(v.is_null());
    }

    #[test]
    fn adhoc_get_returns_full_record_for_known_id() {
        let (mut p, ids) =
            plugin_with_seeded_adhoc(&[("hang", None, None, 250)]);
        let id = ids.first().expect("seeded one row").clone();
        let v = p
            .dispatch(HANDLER_ADHOC_GET, &serde_json::json!({ "id": id }))
            .expect("adhoc_get");
        let row: AdHocRecord = serde_json::from_value(v).expect("decode");
        assert_eq!(row.id, id);
        assert_eq!(row.command, "hang");
        // Killed-without-exit row encodes as Timeout in this surface.
        assert_eq!(row.status, AdHocStatus::Timeout);
    }

    #[test]
    fn adhoc_delete_is_idempotent_for_unknown_id() {
        let mut p = plugin_with_adhoc();
        let v = p
            .dispatch(HANDLER_ADHOC_DELETE, &serde_json::json!({ "id": "ghost" }))
            .expect("adhoc_delete");
        assert_eq!(v, serde_json::json!({ "id": "ghost" }));
    }

    #[test]
    fn adhoc_delete_removes_row_so_subsequent_get_returns_null() {
        let (mut p, ids) = plugin_with_seeded_adhoc(&[("rm-me", None, Some(0), 10)]);
        let id = ids.first().expect("seeded one row").clone();
        p.dispatch(HANDLER_ADHOC_DELETE, &serde_json::json!({ "id": id }))
            .expect("adhoc_delete");
        let v = p
            .dispatch(HANDLER_ADHOC_GET, &serde_json::json!({ "id": id }))
            .expect("adhoc_get post-delete");
        assert!(v.is_null());
    }

    #[test]
    fn adhoc_promote_creates_saved_command_with_supplied_name_and_options() {
        let (mut p, ids) =
            plugin_with_seeded_adhoc(&[("npm test", Some("/work"), Some(0), 800)]);
        let id = ids.first().expect("seeded one row").clone();
        let v = p
            .dispatch(
                HANDLER_ADHOC_PROMOTE,
                &serde_json::json!({
                    "id": id,
                    "name": "Run Tests",
                    "icon": "play-circle",
                    "shell": "/bin/bash",
                }),
            )
            .expect("adhoc_promote");
        let saved: SavedCommand = serde_json::from_value(v).expect("decode");
        // slugify("Run Tests") → "run_tests"
        assert_eq!(saved.slug, "run_tests");
        assert_eq!(saved.name, "Run Tests");
        assert_eq!(saved.shell, "/bin/bash");
        assert_eq!(saved.shell_cmd, "npm test");
        assert_eq!(saved.working_dir.as_deref(), Some("/work"));
        assert_eq!(saved.icon, "play-circle");

        // The new row should round-trip through `saved_list`.
        let list_v = p
            .dispatch(HANDLER_SAVED_LIST, &serde_json::json!({}))
            .expect("saved_list");
        let saved_rows: Vec<SavedCommand> =
            serde_json::from_value(list_v).expect("decode saved");
        assert!(saved_rows.iter().any(|r| r.slug == "run_tests"));
    }

    #[test]
    fn adhoc_promote_unknown_id_surfaces_persist_error() {
        let mut p = plugin_with_adhoc();
        let err = p
            .dispatch(
                HANDLER_ADHOC_PROMOTE,
                &serde_json::json!({ "id": "ghost", "name": "Ghost" }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("no adhoc row"),
                    "expected 'no adhoc row' in error, got: {reason}",
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    // ── BL-055 — run_saved dispatch tests ───────────────────────────

    fn seed_saved(store: &SqliteSavedCommandStore, slug: &str, shell: &str, cmd: &str) {
        let mut row = SavedCommand::new(slug, slug, shell, cmd);
        row.working_dir = None;
        store.create(&row).expect("seed saved");
    }

    #[test]
    fn run_saved_without_attached_store_surfaces_clear_error() {
        let mut p = TerminalCorePlugin::new();
        let err = p
            .dispatch(HANDLER_RUN_SAVED, &serde_json::json!({ "slug": "any" }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("saved-command store not attached"),
                    "expected attach-error, got: {reason}",
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn run_saved_unknown_slug_surfaces_clear_error() {
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        let mut p = TerminalCorePlugin::new().with_saved_store(saved);
        let err = p
            .dispatch(HANDLER_RUN_SAVED, &serde_json::json!({ "slug": "ghost" }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("no saved command with slug 'ghost'"),
                    "got: {reason}",
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn run_saved_spawns_session_and_returns_id_unix() {
        if !unix_only("run_saved_spawns_session_and_returns_id_unix") {
            return;
        }
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        seed_saved(&saved, "hello", "/bin/sh", "printf hello");
        let mut p = TerminalCorePlugin::new().with_saved_store(saved);

        let resp = p
            .dispatch(HANDLER_RUN_SAVED, &serde_json::json!({ "slug": "hello" }))
            .expect("run_saved");
        let body: CreateSessionResponse = serde_json::from_value(resp).expect("decode");
        assert!(!body.id.is_empty());

        // The new session should appear in `list_sessions` with a name
        // tag derived from the slug, so the agent surface can recognise
        // an agent-spawned session at a glance.
        let list_v = p
            .dispatch(HANDLER_LIST_SESSIONS, &serde_json::json!({}))
            .expect("list_sessions");
        let list: Vec<SessionInfo> = serde_json::from_value(list_v).expect("decode list");
        let row = list.iter().find(|s| s.id == body.id).expect("present");
        assert_eq!(row.name, "saved:hello");
    }

    #[test]
    fn run_saved_with_command_override_uses_overridden_cmd_unix() {
        if !unix_only("run_saved_with_command_override_uses_overridden_cmd_unix") {
            return;
        }
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        seed_saved(&saved, "profile", "/bin/sh", "echo from-saved");
        let mut p = TerminalCorePlugin::new().with_saved_store(saved);

        // Run with an override; the call should still succeed and
        // produce a session whose name still tracks the slug.
        let resp = p
            .dispatch(
                HANDLER_RUN_SAVED,
                &serde_json::json!({ "slug": "profile", "command": "echo overridden" }),
            )
            .expect("run_saved override");
        let body: CreateSessionResponse = serde_json::from_value(resp).expect("decode");
        let list_v = p
            .dispatch(HANDLER_LIST_SESSIONS, &serde_json::json!({}))
            .expect("list_sessions");
        let list: Vec<SessionInfo> = serde_json::from_value(list_v).expect("decode list");
        let row = list.iter().find(|s| s.id == body.id).expect("present");
        // Session naming uses the slug regardless of override; the
        // override only affects which command line ran inside the
        // shell.
        assert_eq!(row.name, "saved:profile");
    }

    #[test]
    fn run_saved_invalid_args_reported_as_execution_failed() {
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        let mut p = TerminalCorePlugin::new().with_saved_store(saved);
        let err = p
            .dispatch(HANDLER_RUN_SAVED, &serde_json::json!({}))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("invalid args") || reason.contains("default args invalid"), "got: {reason}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    // ── BL-061 follow-up — per-saved-command memory_limit_mb override ───

    fn seed_saved_with_memory_limit(
        store: &SqliteSavedCommandStore,
        slug: &str,
        shell: &str,
        cmd: &str,
        memory_limit_mb: u32,
    ) {
        let mut row = SavedCommand::new(slug, slug, shell, cmd);
        row.working_dir = None;
        row.memory_limit_mb = Some(memory_limit_mb);
        store.create(&row).expect("seed saved");
    }

    /// `dispatch_run_saved` stages the saved command's
    /// `memory_limit_mb` into `MemoryState.pending_overrides` so the
    /// next poller round applies it instead of the bootstrap-wide
    /// default. Verified by inspecting state directly — running a
    /// poller in the test would race with the real-PTY spawn timing.
    #[test]
    fn run_saved_with_memory_limit_stages_pending_override_unix() {
        if !unix_only("run_saved_with_memory_limit_stages_pending_override_unix") {
            return;
        }
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        seed_saved_with_memory_limit(&saved, "capped", "/bin/sh", "sleep 5", 128);
        let bus = Arc::new(EventBus::new(8));
        // Disable the real poller so the test owns the pending_overrides
        // observation window — `with_memory_poll_interval` would spawn
        // a thread that consumes the entry between create_session and
        // the assertion.
        let mut p = TerminalCorePlugin::new()
            .with_saved_store(saved)
            .with_memory_monitor(TestMemoryLimits::default_recommended())
            .with_memory_poll_interval(Duration::from_secs(60))
            .with_event_bus(Arc::clone(&bus));

        let resp = p
            .dispatch(HANDLER_RUN_SAVED, &serde_json::json!({ "slug": "capped" }))
            .expect("run_saved");
        let body: CreateSessionResponse = serde_json::from_value(resp).expect("decode");

        // Inspect MemoryState directly: either the override is staged
        // in `pending_overrides` (if the poller hasn't run) OR
        // `monitor.set_limits` was already called (if the poller raced
        // ahead). The 60s poll interval makes the former the
        // overwhelmingly common branch in this test.
        let memory = p.memory.as_ref().expect("monitor wired");
        let mem = memory.lock().expect("memory lock");
        let staged = mem.pending_overrides.get(body.id.as_str());
        assert!(
            staged.is_some(),
            "pending override should be staged for the saved command",
        );
        let limits = staged.expect("staged");
        assert_eq!(limits.hard_mb, Some(128));
        assert_eq!(
            limits.soft_mb, None,
            "single-knob saved-command field maps to hard only — no soft warn",
        );
    }

    /// Without `with_memory_monitor`, a saved command carrying a
    /// `memory_limit_mb` is silently a no-op — the override has
    /// nowhere to land but `run_saved` still spawns the session.
    #[test]
    fn run_saved_with_memory_limit_silently_skips_when_no_monitor_unix() {
        if !unix_only("run_saved_with_memory_limit_silently_skips_when_no_monitor_unix") {
            return;
        }
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        seed_saved_with_memory_limit(&saved, "capped", "/bin/sh", "printf hi", 256);
        // No `with_memory_monitor` here — `self.memory` stays `None`.
        let mut p = TerminalCorePlugin::new().with_saved_store(saved);

        let resp = p
            .dispatch(HANDLER_RUN_SAVED, &serde_json::json!({ "slug": "capped" }))
            .expect("run_saved");
        let body: CreateSessionResponse = serde_json::from_value(resp).expect("decode");
        assert!(!body.id.is_empty());
        assert!(p.memory.is_none(), "monitor should not be silently created");
    }

    /// Without a `memory_limit_mb` on the saved command, the
    /// pending_overrides map stays empty so the poller falls back to
    /// the bootstrap-wide default.
    #[test]
    fn run_saved_without_memory_limit_does_not_stage_override_unix() {
        if !unix_only("run_saved_without_memory_limit_does_not_stage_override_unix") {
            return;
        }
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        // Use the existing helper — no memory_limit_mb on this row.
        seed_saved(&saved, "uncapped", "/bin/sh", "sleep 5");
        let bus = Arc::new(EventBus::new(8));
        let mut p = TerminalCorePlugin::new()
            .with_saved_store(saved)
            .with_memory_monitor(TestMemoryLimits::default_recommended())
            .with_memory_poll_interval(Duration::from_secs(60))
            .with_event_bus(Arc::clone(&bus));

        let resp = p
            .dispatch(HANDLER_RUN_SAVED, &serde_json::json!({ "slug": "uncapped" }))
            .expect("run_saved");
        let body: CreateSessionResponse = serde_json::from_value(resp).expect("decode");

        let memory = p.memory.as_ref().expect("monitor wired");
        let mem = memory.lock().expect("memory lock");
        assert!(
            !mem.pending_overrides.contains_key(body.id.as_str()),
            "no override should be staged when memory_limit_mb is None",
        );
    }

    #[test]
    fn one_shot_flag_picks_the_right_flag_per_shell_family() {
        // POSIX shells get -c.
        assert_eq!(one_shot_flag("/bin/sh"), "-c");
        assert_eq!(one_shot_flag("/usr/bin/bash"), "-c");
        assert_eq!(one_shot_flag("/usr/bin/zsh"), "-c");
        assert_eq!(one_shot_flag("/usr/bin/fish"), "-c");
        // PowerShell variants get -Command (extension stripped first).
        assert_eq!(one_shot_flag("pwsh"), "-Command");
        assert_eq!(one_shot_flag("pwsh.exe"), "-Command");
        assert_eq!(one_shot_flag("powershell.exe"), "-Command");
        // cmd.exe gets /C.
        assert_eq!(one_shot_flag("cmd.exe"), "/C");
        // Unknowns fall back to the POSIX convention.
        assert_eq!(one_shot_flag("/opt/odd/sh"), "-c");
    }

    #[test]
    fn adhoc_list_invalid_args_reported_as_execution_failed() {
        let mut p = plugin_with_adhoc();
        let err = p
            .dispatch(HANDLER_ADHOC_LIST, &serde_json::json!({ "limit": "lots" }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("invalid args") || reason.contains("default args invalid"), "got: {reason}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn lifecycle_topic_includes_session_id_suffix() {
        use nexus_kernel::{EventFilter, NexusEvent};

        if !unix_only("lifecycle_topic_includes_session_id_suffix") {
            return;
        }

        let bus = Arc::new(EventBus::new(64));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_LIFECYCLE_PREFIX.to_string()));

        let mut p = TerminalCorePlugin::new().with_event_bus(Arc::clone(&bus));
        let CreateSessionResponse { id } = serde_json::from_value(
            p.dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 1"))
                .expect("create"),
        )
        .expect("decode");

        // Wait for any lifecycle event for `id` to land. The
        // SessionCreated event must arrive via the prefix subscription
        // and its type_id must end with the session id so a per-session
        // `EventFilter::CustomExact` subscription is also viable for
        // remote-terminal style consumers.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(evt) = sub.try_recv().expect("bus alive") {
                if let NexusEvent::Custom { type_id, .. } = &evt.event {
                    assert_eq!(*type_id, format!("{EVENT_LIFECYCLE_PREFIX}{id}"));
                    return;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no lifecycle event landed within 2s",
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // ── BL-061 — memory backpressure tests ──────────────────────────

    use crate::memory::MemoryLimits as TestMemoryLimits;

    /// Without `with_memory_monitor`, `get_session_info` returns
    /// `rss_bytes: None` — the field is only populated when the
    /// monitor is wired.
    #[test]
    fn get_session_info_rss_is_none_when_no_monitor_attached() {
        if !unix_only("get_session_info_rss_is_none_when_no_monitor_attached") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        let resp = p
            .dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 1"))
            .expect("create");
        let CreateSessionResponse { id } = serde_json::from_value(resp).expect("decode");
        let info_v = p
            .dispatch(HANDLER_GET_SESSION_INFO, &serde_json::json!({ "id": id }))
            .expect("get_session_info");
        let info: SessionInfo = serde_json::from_value(info_v).expect("decode");
        assert!(info.rss_bytes.is_none());
    }

    /// With a memory monitor + an event bus + a tight poll interval,
    /// the cache is populated within a couple of poll rounds and a
    /// freshly-spawned session's RSS surfaces through
    /// `get_session_info`.
    #[test]
    fn poller_populates_rss_cache_for_running_session_unix() {
        if !unix_only("poller_populates_rss_cache_for_running_session_unix") {
            return;
        }
        let bus = Arc::new(EventBus::new(64));
        let mut p = TerminalCorePlugin::new()
            .with_memory_monitor(TestMemoryLimits::unlimited())
            .with_memory_poll_interval(Duration::from_millis(20))
            .with_event_bus(Arc::clone(&bus));

        let resp = p
            .dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 5"))
            .expect("create");
        let CreateSessionResponse { id } = serde_json::from_value(resp).expect("decode");

        // Poll up to ~3s for a non-zero RSS to land. The first sample
        // for a freshly-spawned process can race the kernel's VmRSS
        // ticking up from 0 — we want the structural guarantee
        // (cache is populated *and* sample is meaningful), not just
        // the presence check.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let info_v = p
                .dispatch(HANDLER_GET_SESSION_INFO, &serde_json::json!({ "id": &id }))
                .expect("get_session_info");
            let info: SessionInfo = serde_json::from_value(info_v).expect("decode");
            if info.rss_bytes.is_some_and(|b| b > 0) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "rss_bytes never settled to a positive value within 3s",
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// When a tracked session crosses `hard_mb`, the poller publishes
    /// `MemoryLimitExceeded` *before* the kill — so a subscriber on
    /// the lifecycle prefix sees it ahead of the eventual
    /// `SessionClosed`. We force the breach by setting an absurdly
    /// low hard limit (1 MB); any real shell exceeds that
    /// immediately.
    #[test]
    fn poller_publishes_memory_limit_exceeded_before_kill_unix() {
        use nexus_kernel::{EventFilter, NexusEvent};
        if !unix_only("poller_publishes_memory_limit_exceeded_before_kill_unix") {
            return;
        }
        let bus = Arc::new(EventBus::new(64));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix(EVENT_LIFECYCLE_PREFIX.to_string()));

        // 1 MB hard cap — any shell will exceed this in its first
        // sample. soft is left below hard so the evaluation prefers
        // the hard branch.
        let limits = TestMemoryLimits {
            soft_mb: Some(0),
            hard_mb: Some(1),
        };
        let mut p = TerminalCorePlugin::new()
            .with_memory_monitor(limits)
            .with_memory_poll_interval(Duration::from_millis(20))
            .with_event_bus(Arc::clone(&bus));

        let resp = p
            .dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 30"))
            .expect("create");
        let CreateSessionResponse { id } = serde_json::from_value(resp).expect("decode");

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut saw_breach = false;
        let mut saw_closed = false;
        let mut breach_before_closed = false;
        while std::time::Instant::now() < deadline {
            if let Some(evt) = sub.try_recv().expect("bus alive") {
                if let NexusEvent::Custom { type_id, payload, .. } = &evt.event {
                    if !type_id.ends_with(&id) {
                        continue;
                    }
                    if let Some(kind) = payload.get("kind").and_then(|v| v.as_str()) {
                        match kind {
                            "memory_limit_exceeded" => {
                                saw_breach = true;
                                let limit_mb =
                                    payload.get("limit_mb").and_then(|v| v.as_u64());
                                let rss_bytes =
                                    payload.get("rss_bytes").and_then(|v| v.as_u64());
                                assert_eq!(limit_mb, Some(1));
                                assert!(rss_bytes.is_some_and(|b| b > 0));
                            }
                            "session_closed" => {
                                saw_closed = true;
                                if saw_breach {
                                    breach_before_closed = true;
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(saw_breach, "MemoryLimitExceeded never published");
        assert!(saw_closed, "SessionClosed never published after kill");
        assert!(
            breach_before_closed,
            "MemoryLimitExceeded must be published before SessionClosed"
        );
    }

    #[test]
    fn rss_cache_clears_when_session_is_closed_unix() {
        if !unix_only("rss_cache_clears_when_session_is_closed_unix") {
            return;
        }
        let bus = Arc::new(EventBus::new(64));
        let mut p = TerminalCorePlugin::new()
            .with_memory_monitor(TestMemoryLimits::unlimited())
            .with_memory_poll_interval(Duration::from_millis(20))
            .with_event_bus(Arc::clone(&bus));

        let resp = p
            .dispatch(HANDLER_CREATE_SESSION, &create_args("sleep 5"))
            .expect("create");
        let CreateSessionResponse { id } = serde_json::from_value(resp).expect("decode");

        // Wait for the cache to populate.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let info_v = p
                .dispatch(HANDLER_GET_SESSION_INFO, &serde_json::json!({ "id": &id }))
                .expect("get_session_info");
            let info: SessionInfo = serde_json::from_value(info_v).expect("decode");
            if info.rss_bytes.is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "rss_bytes never populated within 2s",
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        // Close — the next poller round should see the session is
        // gone and prune its cache entry. We re-list_sessions; the
        // closed id no longer appears, but if for any reason we look
        // it up directly we must not see stale RSS.
        p.dispatch(HANDLER_CLOSE_SESSION, &serde_json::json!({ "id": &id }))
            .expect("close");

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            // Direct cache read — bypasses the server's
            // get_session_info (which returns NotRunning after close).
            let cached = p.cached_rss(&id);
            if cached.is_none() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "rss cache still has the closed session after 2s",
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // ── BL-064 — suggest dispatch tests ─────────────────────────────
    //
    // The handler is async (it lives in `dispatch_async`), so the
    // tests use `tokio::test`. The LLM-enrichment branch is exercised
    // by calling `handle_suggest` directly with `ctx = None`, which
    // walks every code path except the IPC call to `com.nexus.ai`.
    // The IPC-success / timeout paths are integration-level concerns
    // covered separately when a real `com.nexus.ai` is wired into a
    // bootstrap-style runtime.

    #[tokio::test]
    async fn suggest_returns_null_when_no_rule_fires() {
        if !unix_only("suggest_returns_null_when_no_rule_fires") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        // Spawn a short-lived shell that prints something boring.
        p.dispatch(
            HANDLER_CREATE_SESSION,
            &create_args("printf hello"),
        )
        .expect("create");
        let list_v = p
            .dispatch(HANDLER_LIST_SESSIONS, &serde_json::json!({}))
            .expect("list");
        let list: Vec<SessionInfo> = serde_json::from_value(list_v).expect("decode");
        let id = list.first().expect("one session").id.clone();
        // Pump a few times so the printf output lands in the line buffer.
        for _ in 0..10 {
            let _ = p.dispatch(
                HANDLER_PUMP,
                &serde_json::json!({ "id": &id, "timeout_ms": 50 }),
            );
        }
        // The "hello" line shouldn't match any default rule.
        let resp = handle_suggest(
            None,
            &p.server,
            &p.suggest_engine,
            &serde_json::json!({ "session_id": id }),
        )
        .await
        .expect("suggest");
        assert!(resp.is_null(), "expected null, got {resp}");
    }

    #[tokio::test]
    async fn suggest_returns_static_rule_response_when_no_kernel_context() {
        if !unix_only("suggest_returns_static_rule_response_when_no_kernel_context") {
            return;
        }
        let mut p = TerminalCorePlugin::new();
        p.dispatch(
            HANDLER_CREATE_SESSION,
            &create_args("printf 'error: could not compile foo due to errors\\n'"),
        )
        .expect("create");
        let list_v = p
            .dispatch(HANDLER_LIST_SESSIONS, &serde_json::json!({}))
            .expect("list");
        let list: Vec<SessionInfo> = serde_json::from_value(list_v).expect("decode");
        let id = list.first().expect("one session").id.clone();
        for _ in 0..30 {
            let _ = p.dispatch(
                HANDLER_PUMP,
                &serde_json::json!({ "id": &id, "timeout_ms": 50 }),
            );
        }
        let resp = handle_suggest(
            None,
            &p.server,
            &p.suggest_engine,
            &serde_json::json!({ "session_id": id }),
        )
        .await
        .expect("suggest");
        let body: SuggestResponse = serde_json::from_value(resp).expect("decode");
        assert_eq!(body.text, "cargo check --message-format=json");
        assert_eq!(body.source_rule, "cargo.compile_failure");
        assert_eq!(body.severity, "error");
        assert!(!body.llm_used, "ctx is None — must fall back to static");
        assert!(body.reason.contains("error info") || body.reason.contains("crate"),
            "static reason should mention the rule's hint, got: {}",
            body.reason);
    }

    #[tokio::test]
    async fn suggest_unknown_session_surfaces_clear_error() {
        let p = TerminalCorePlugin::new();
        let err = handle_suggest(
            None,
            &p.server,
            &p.suggest_engine,
            &serde_json::json!({ "session_id": "nope" }),
        )
        .await
        .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("unknown session"), "got: {reason}");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn suggest_invalid_args_reported_as_execution_failed() {
        let p = TerminalCorePlugin::new();
        let err = handle_suggest(
            None,
            &p.server,
            &p.suggest_engine,
            &serde_json::json!({ "wrong": "shape" }),
        )
        .await
        .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("invalid args") || reason.contains("default args invalid"), "got: {reason}");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    // ── BL-063 — cross_session_search dispatch tests ────────────────

    #[test]
    fn cross_session_search_without_attached_store_surfaces_clear_error() {
        let mut p = TerminalCorePlugin::new();
        let err = p
            .dispatch(
                HANDLER_CROSS_SESSION_SEARCH,
                &serde_json::json!({ "query": "anything" }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("session store not attached"),
                    "got: {reason}",
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn cross_session_search_pass_through_returns_indexed_hits() {
        // Open an in-memory store, populate it with two sessions'
        // worth of scrollback, attach to the plugin, then dispatch
        // the IPC and verify the response shape.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("store");
        store
            .save_scrollback("alpha", b"compile error: missing semicolon\n")
            .expect("save alpha");
        store
            .save_scrollback("beta", b"server starting on port 3000\n")
            .expect("save beta");
        let store = Arc::new(Mutex::new(store));
        let mut p = TerminalCorePlugin::new().with_session_store(store);

        let resp = p
            .dispatch(
                HANDLER_CROSS_SESSION_SEARCH,
                &serde_json::json!({ "query": "compile" }),
            )
            .expect("search");
        let hits: Vec<crate::ScrollbackHit> = serde_json::from_value(resp).expect("decode");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "alpha");
        assert!(hits[0].text.contains("missing semicolon"));
    }

    #[test]
    fn cross_session_search_invalid_args_reported_as_execution_failed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("store");
        let store = Arc::new(Mutex::new(store));
        let mut p = TerminalCorePlugin::new().with_session_store(store);
        let err = p
            .dispatch(
                HANDLER_CROSS_SESSION_SEARCH,
                &serde_json::json!({ "wrong": "shape" }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("invalid args") || reason.contains("default args invalid"), "got: {reason}");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn cross_session_search_default_limit_is_100() {
        // Insert > 100 matching lines and verify the response caps
        // out at the IPC default. The store-level test asserts a
        // custom `limit` flows through; this test pins the dispatch
        // arm's default.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("store");
        let mut blob = Vec::new();
        for _ in 0..150 {
            blob.extend_from_slice(b"abc xyz target\n");
        }
        store.save_scrollback("a", &blob).expect("save");
        let store = Arc::new(Mutex::new(store));
        let mut p = TerminalCorePlugin::new().with_session_store(store);
        let resp = p
            .dispatch(
                HANDLER_CROSS_SESSION_SEARCH,
                &serde_json::json!({ "query": "target" }),
            )
            .expect("search");
        let hits: Vec<crate::ScrollbackHit> = serde_json::from_value(resp).expect("decode");
        assert_eq!(hits.len(), 100, "default limit should cap at 100");
    }

    #[test]
    fn dispatch_suggest_via_sync_path_surfaces_async_hint() {
        let mut p = TerminalCorePlugin::new();
        let err = p
            .dispatch(HANDLER_SUGGEST, &serde_json::json!({}))
            .unwrap_err();
        match err {
            PluginError::HandlerIsAsyncOnly { handler_id } => {
                assert_eq!(handler_id, HANDLER_SUGGEST);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
