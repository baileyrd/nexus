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
//! `docs/ARCHITECTURE.md` §7 ("invokers must reach terminal features
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
//! - Saved-commands / ad-hoc CRUD. Those live in their own tables and
//!   will get a sibling `com.nexus.terminal.commands` plugin in a
//!   later slice — keeping the handler surface small here makes it
//!   easy to audit and version independently.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::saved::{SavedCommand, SqliteSavedCommandStore};
use crate::server::{InMemoryTerminalServer, ServerSpawnConfig, TerminalEvent, TerminalServer};
use crate::session::SessionId;
use crate::shell::ShellSpec;

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

// ── The plugin ───────────────────────────────────────────────────────────────

/// Core plugin instance. Holds the server behind an [`Arc<Mutex<_>>`]
/// so the `CorePlugin: Send + Sync` bound is satisfied (PTY handles are
/// `Send`-only) **and** so the autonomous drainer thread can hold its
/// own clone of the lock. IPC dispatches arrive single-threaded; the
/// drainer competes for the same lock with short-blocking pumps. See
/// [`spawn_drainer`] for the cadence + contention model.
pub struct TerminalCorePlugin {
    server: Arc<Mutex<InMemoryTerminalServer>>,
    /// SQLite-backed saved-commands store. `None` when the plugin is
    /// instantiated without a forge path (tests, embedded runtimes) —
    /// the saved-command handlers return a clear error in that case.
    saved: Option<Mutex<SqliteSavedCommandStore>>,
    /// Optional kernel event bus. When `Some`, the constructor spawns
    /// the autonomous drainer (see [`Self::drainer`]) and `pump` /
    /// `read_raw_since` dispatches additionally publish on demand for
    /// any bytes the drainer hasn't already shipped.
    event_bus: Option<Arc<EventBus>>,
    /// Per-session emitter state — tracks the internal byte cursor we
    /// last published from and the next sequence number to assign.
    /// Shared between the dispatch path and the drainer thread; both
    /// take the same lock, so the cursor advance + seq assignment is
    /// atomic per chunk and neither path can publish the same bytes
    /// twice. Independent of the caller-supplied cursor in
    /// `read_raw_since`.
    emitters: Arc<Mutex<HashMap<SessionId, EmitterState>>>,
    /// Background drainer that pumps every active session and publishes
    /// new bytes onto the event bus without waiting for IPC clients to
    /// poll. Spawned by [`Self::with_event_bus`] and stopped on drop —
    /// `DrainerHandle::drop` joins the thread so by the time the rest
    /// of the plugin's `Arc`s release their data, the drainer has
    /// already let go of its clones. `None` when no event bus is wired
    /// (tests / embedded runtimes that don't need streaming).
    drainer: Option<DrainerHandle>,
    /// Background forwarder that pulls [`TerminalEvent`]s from the
    /// in-memory server's local mpsc and republishes them on the
    /// kernel bus as `com.nexus.terminal.events.<session_id>` (BL-013).
    /// `None` when no event bus is wired — the legacy mpsc subscriber
    /// path stays the only consumer in that case.
    lifecycle_forwarder: Option<LifecycleForwarderHandle>,
}

/// Owns the drainer thread + the stop flag. The flag is checked at the
/// top of every drainer cycle and after each pumped session so a Drop
/// signal during a long round still exits within one pump timeout. The
/// `Option<JoinHandle>` lets `Drop` move the handle out for the join.
struct DrainerHandle {
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
struct LifecycleForwarderHandle {
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
struct EmitterState {
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
            event_bus: None,
            emitters: Arc::new(Mutex::new(HashMap::new())),
            drainer: None,
            lifecycle_forwarder: None,
        }
    }

    /// Build around an existing server — used by tests that need to
    /// seed sessions before wiring up the plugin.
    #[must_use]
    pub fn with_server(server: InMemoryTerminalServer) -> Self {
        Self {
            server: Arc::new(Mutex::new(server)),
            saved: None,
            event_bus: None,
            emitters: Arc::new(Mutex::new(HashMap::new())),
            drainer: None,
            lifecycle_forwarder: None,
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
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

// ── Dispatch helpers ─────────────────────────────────────────────────────────

impl TerminalCorePlugin {
    fn dispatch_create_session(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: CreateSessionArgs = parse_args(args, "create_session")?;
        let shell = a.shell.map(|p| ShellSpec {
            program: PathBuf::from(p),
            args: a.shell_args,
        });
        let cfg = ServerSpawnConfig {
            name: a.name,
            shell,
            working_dir: a.working_dir.map(PathBuf::from),
            env: a.env,
        };
        let id = self
            .server
            .lock()
            .map_err(poisoned)?
            .create_session(cfg)
            .map_err(crate_err)?;
        to_value(
            &CreateSessionResponse {
                id: id.as_str().to_string(),
            },
            "create_session",
        )
    }

    fn dispatch_close_session(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "close_session")?;
        let id = SessionId::from_string(a.id);
        self.server
            .lock()
            .map_err(poisoned)?
            .close_session(&id)
            .map_err(crate_err)?;
        // Drop the per-session emitter state so the map doesn't grow
        // unboundedly across long-running plugin instances. The drainer's
        // next round won't see this id from `list_sessions`, so the
        // entry is unreachable anyway.
        if let Ok(mut em) = self.emitters.lock() {
            em.remove(&id);
        }
        Ok(serde_json::Value::Null)
    }

    fn dispatch_send_input(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SendInputArgs = parse_args(args, "send_input")?;
        let id = SessionId::from_string(a.id);
        self.server
            .lock()
            .map_err(poisoned)?
            .send_input(&id, &a.input)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    fn dispatch_send_raw_input(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SendRawInputArgs = parse_args(args, "send_raw_input")?;
        let id = SessionId::from_string(a.id);
        self.server
            .lock()
            .map_err(poisoned)?
            .send_raw_input(&id, &a.data)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    fn dispatch_pump(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: PumpArgs = parse_args(args, "pump")?;
        let id = SessionId::from_string(a.id);
        let timeout = Duration::from_millis(a.timeout_ms.unwrap_or(100));
        // Hold the server lock just long enough to drain + read the new
        // bytes since our internal cursor; release before publishing so
        // a slow subscriber can't back-pressure the next IPC dispatch.
        let (bytes, new_chunk) = {
            let mut server = self.server.lock().map_err(poisoned)?;
            let bytes = server.pump(&id, timeout).map_err(crate_err)?;
            let new_chunk = self.fetch_new_bytes(&server, &id);
            (bytes, new_chunk)
        };
        if let Some((next_cursor, data)) = new_chunk {
            self.publish_output(&id, next_cursor, data);
        }
        to_value(&PumpResponse { bytes }, "pump")
    }

    fn dispatch_read_output(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ReadOutputArgs = parse_args(args, "read_output")?;
        let id = SessionId::from_string(a.id);
        let lines = self
            .server
            .lock()
            .map_err(poisoned)?
            .read_output(&id, a.start, a.count)
            .map_err(crate_err)?;
        to_value(&lines, "read_output")
    }

    fn dispatch_read_raw_since(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ReadRawSinceArgs = parse_args(args, "read_raw_since")?;
        let id = SessionId::from_string(a.id);
        let timeout = Duration::from_millis(a.timeout_ms.unwrap_or(30));
        // Same lock-discipline as `dispatch_pump`: drain inside the
        // server lock, derive the event bytes from the plugin's
        // independent cursor, then drop the lock before publishing.
        let (cursor, data, new_chunk) = {
            let mut server = self.server.lock().map_err(poisoned)?;
            let (cursor, data) = server
                .read_raw_since(&id, a.cursor, timeout)
                .map_err(crate_err)?;
            let new_chunk = self.fetch_new_bytes(&server, &id);
            (cursor, data, new_chunk)
        };
        if let Some((next_cursor, bytes)) = new_chunk {
            self.publish_output(&id, next_cursor, bytes);
        }
        to_value(&ReadRawSinceResponse { cursor, data }, "read_raw_since")
    }

    fn dispatch_resize(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ResizeArgs = parse_args(args, "resize")?;
        let id = SessionId::from_string(a.id);
        // Clamp zero dimensions — most tty ioctls reject them and the
        // resulting error would be opaque to the caller. xterm's fit
        // addon can occasionally propose zero before layout settles.
        let cols = a.cols.max(1);
        let rows = a.rows.max(1);
        self.server
            .lock()
            .map_err(poisoned)?
            .resize(&id, cols, rows)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    fn dispatch_search_output(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SearchOutputArgs = parse_args(args, "search_output")?;
        let id = SessionId::from_string(a.id);
        let hits = self
            .server
            .lock()
            .map_err(poisoned)?
            .search_output(&id, &a.query, a.is_regex)
            .map_err(crate_err)?;
        to_value(&hits, "search_output")
    }

    fn dispatch_wait_for_pattern(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: WaitForPatternArgs = parse_args(args, "wait_for_pattern")?;
        let id = SessionId::from_string(a.id);
        let timeout = Duration::from_millis(a.timeout_ms);
        let matched = self
            .server
            .lock()
            .map_err(poisoned)?
            .wait_for_pattern(&id, &a.pattern, a.is_regex, timeout)
            .map_err(crate_err)?;
        to_value(&WaitForPatternResponse { matched }, "wait_for_pattern")
    }

    fn dispatch_get_session_info(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "get_session_info")?;
        let id = SessionId::from_string(a.id);
        let info = self
            .server
            .lock()
            .map_err(poisoned)?
            .get_session_info(&id)
            .map_err(crate_err)?;
        to_value(&info, "get_session_info")
    }

    fn dispatch_list_sessions(
        &self,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let list = self.server.lock().map_err(poisoned)?.list_sessions();
        to_value(&list, "list_sessions")
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
    fn fetch_new_bytes(
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
    fn publish_output(&self, id: &SessionId, _next_cursor: u64, data: Vec<u8>) {
        let Some(bus) = self.event_bus.as_ref() else {
            return;
        };
        publish_chunk(bus, &self.emitters, id, data);
    }

    fn saved_store(&self) -> Result<&Mutex<SqliteSavedCommandStore>, PluginError> {
        self.saved.as_ref().ok_or_else(|| {
            exec_err("saved-command store not attached (runtime built without a forge path)".into())
        })
    }

    fn dispatch_saved_list(&self) -> Result<serde_json::Value, PluginError> {
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        let rows = store.list().map_err(crate_err)?;
        to_value(&rows, "saved_list")
    }

    fn dispatch_saved_create(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let cmd: SavedCommand = parse_args(args, "saved_create")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.create(&cmd).map_err(crate_err)?;
        to_value(&cmd, "saved_create")
    }

    fn dispatch_saved_update(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let cmd: SavedCommand = parse_args(args, "saved_update")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.update(&cmd).map_err(crate_err)?;
        let fresh = store
            .get(&cmd.slug)
            .map_err(crate_err)?
            .ok_or_else(|| exec_err(format!("saved_update: slug '{}' vanished", cmd.slug)))?;
        to_value(&fresh, "saved_update")
    }

    fn dispatch_saved_delete(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(serde::Deserialize)]
        struct DeleteArgs {
            slug: String,
        }
        let a: DeleteArgs = parse_args(args, "saved_delete")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.delete(&a.slug).map_err(crate_err)?;
        Ok(serde_json::json!({ "slug": a.slug }))
    }

    fn dispatch_saved_reorder(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(serde::Deserialize)]
        struct ReorderArgs {
            slug: String,
            #[serde(default)]
            sidebar_order: Option<i32>,
        }
        let a: ReorderArgs = parse_args(args, "saved_reorder")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.reorder(&a.slug, a.sidebar_order).map_err(crate_err)?;
        Ok(serde_json::json!({ "slug": a.slug, "sidebar_order": a.sidebar_order }))
    }

    /// BL-059 — open the saved command's `working_dir` in the user's
    /// preferred external terminal emulator. Optional `priority` arg
    /// overrides [`crate::DEFAULT_PRIORITY`] with a `snake_case` list.
    fn dispatch_open_in_terminal(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        use crate::external_terminal::{
            launch_spec, parse_kind, pick_first_available, spawn_detached, which_in_path,
            DEFAULT_PRIORITY,
        };

        #[derive(serde::Deserialize)]
        struct OpenInTerminalArgs {
            slug: String,
            #[serde(default)]
            priority: Option<Vec<String>>,
        }
        let a: OpenInTerminalArgs = parse_args(args, "open_in_terminal")?;

        let saved = {
            let store = self.saved_store()?.lock().map_err(poisoned)?;
            store
                .get(&a.slug)
                .map_err(crate_err)?
                .ok_or_else(|| {
                    exec_err(format!(
                        "open_in_terminal: no saved command with slug '{}'",
                        a.slug
                    ))
                })?
        };

        let working_dir_str = saved
            .working_dir
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                exec_err(format!(
                    "open_in_terminal: saved command '{}' has no working_dir",
                    saved.slug
                ))
            })?;
        let working_dir = std::path::PathBuf::from(&working_dir_str);

        // Translate the (optional) caller-supplied priority into typed
        // kinds, falling back to the built-in default. Unknown tags are
        // silently dropped — the priority list shouldn't be a place
        // where a typo blocks the whole launch.
        let priority: Vec<crate::external_terminal::TerminalKind> = match a.priority {
            Some(names) => names.iter().filter_map(|n| parse_kind(n)).collect(),
            None => DEFAULT_PRIORITY.to_vec(),
        };

        let (kind, spec) = pick_first_available(
            &priority,
            launch_spec,
            which_in_path,
            &working_dir,
        )
        .ok_or_else(|| {
            exec_err(
                "open_in_terminal: no supported terminal emulator found on PATH \
                 (tried the configured priority list)"
                    .to_string(),
            )
        })?;

        spawn_detached(&spec).map_err(|e| {
            exec_err(format!(
                "open_in_terminal: spawning {program} failed: {e}",
                program = spec.program,
            ))
        })?;

        Ok(serde_json::json!({
            "kind": kind,
            "program": spec.program,
            "args": spec.args,
            "working_dir": working_dir_str,
        }))
    }
}

// ── Error plumbing ───────────────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn poisoned<T>(_e: std::sync::PoisonError<T>) -> PluginError {
    exec_err("server mutex poisoned — prior handler panicked".into())
}

// Used as a function pointer by `.map_err(crate_err)`; wrapping in a
// closure would re-trip `redundant_closure`.
#[allow(clippy::needless_pass_by_value)]
fn crate_err(e: crate::TerminalError) -> PluginError {
    exec_err(e.to_string())
}

fn parse_args<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(value.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
