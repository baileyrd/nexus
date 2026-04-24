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
//!
//! Ids are **append-only** — never reused after retirement — because
//! manifest registrations in loaded plugins bake them in.
//!
//! # Output streaming (Phase 2 WI-12)
//!
//! When the plugin is built via [`TerminalCorePlugin::with_event_bus`] the
//! `pump` and `read_raw_since` handlers additionally publish a
//! `com.nexus.terminal.output.<session_id>` `NexusEvent::Custom` whenever
//! they observe new bytes for a session. The payload is
//! `{ data: Vec<u8>, seq: u64, ts_ms: i64 }` — `data` matches the byte
//! shape of `read_raw_since` so a single TS subscriber can union the two
//! paths, and `seq` is a per-session monotonic counter so the subscriber
//! can detect drops/reorders. The legacy poll-style `pump` handler still
//! returns its byte count unchanged; subscribers that miss events can
//! always fall back to a `read_raw_since` snapshot.
//!
//! # What this is NOT (yet)
//!
//! - General [`crate::TerminalEvent`] broadcast (`SessionCreated`,
//!   `SessionClosed`, `PatternMatched`). Those still travel through the
//!   library's sync mpsc subscribers; only the high-volume output
//!   stream is bridged onto the kernel bus today.
//! - Saved-commands / ad-hoc CRUD. Those live in their own tables and
//!   will get a sibling `com.nexus.terminal.commands` plugin in a
//!   later slice — keeping the handler surface small here makes it
//!   easy to audit and version independently.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};

use crate::saved::{SavedCommand, SqliteSavedCommandStore};
use crate::server::{InMemoryTerminalServer, ServerSpawnConfig, TerminalServer};
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

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Arguments for `create_session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
pub struct CreateSessionResponse {
    /// Fresh session id.
    pub id: String,
}

/// Arguments for `close_session` / `pump` / `get_session_info` and
/// any other single-session-id handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIdArgs {
    /// Session id the handler targets.
    pub id: String,
}

/// Arguments for `send_input`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendInputArgs {
    /// Target session id.
    pub id: String,
    /// String input (newline appended automatically if absent).
    pub input: String,
}

/// Arguments for `send_raw_input`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRawInputArgs {
    /// Target session id.
    pub id: String,
    /// Raw bytes to write (no newline appended).
    pub data: Vec<u8>,
}

/// Arguments for `pump`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct PumpResponse {
    /// Byte count drained from the PTY in this pump.
    pub bytes: usize,
}

/// Arguments for `read_output`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Response from `read_raw_since`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct WaitForPatternResponse {
    /// `true` if a match landed before the deadline.
    pub matched: bool,
}

// ── The plugin ───────────────────────────────────────────────────────────────

/// Core plugin instance. Holds the server behind a [`Mutex`] so the
/// `CorePlugin: Send + Sync` bound is satisfied even though the
/// underlying PTY handles are `Send`-only. Kernel dispatches already
/// arrive single-threaded, so the lock is never contended in practice
/// — `Mutex` keeps the bound honest without adding latency.
pub struct TerminalCorePlugin {
    server: Mutex<InMemoryTerminalServer>,
    /// SQLite-backed saved-commands store. `None` when the plugin is
    /// instantiated without a forge path (tests, embedded runtimes) —
    /// the saved-command handlers return a clear error in that case.
    saved: Option<Mutex<SqliteSavedCommandStore>>,
    /// Optional kernel event bus. When `Some`, every `pump` /
    /// `read_raw_since` dispatch that observes new bytes publishes a
    /// `com.nexus.terminal.output.<session_id>` event (Phase 2 WI-12).
    /// `None` keeps the plugin a pure poll surface — used by the
    /// in-crate unit tests that drive the plugin without a kernel.
    event_bus: Option<Arc<EventBus>>,
    /// Per-session emitter state — tracks the internal byte cursor we
    /// last published from and the next sequence number to assign.
    /// Independent of the caller-supplied cursor in `read_raw_since`
    /// so a fast `read_raw_since` consumer doesn't starve the slower
    /// `pump` event subscriber (and vice-versa).
    emitters: Mutex<HashMap<SessionId, EmitterState>>,
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
            server: Mutex::new(InMemoryTerminalServer::new()),
            saved: None,
            event_bus: None,
            emitters: Mutex::new(HashMap::new()),
        }
    }

    /// Build around an existing server — used by tests that need to
    /// seed sessions before wiring up the plugin.
    #[must_use]
    pub fn with_server(server: InMemoryTerminalServer) -> Self {
        Self {
            server: Mutex::new(server),
            saved: None,
            event_bus: None,
            emitters: Mutex::new(HashMap::new()),
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

    /// Wire the plugin to a kernel event bus. After this call, `pump`
    /// and `read_raw_since` dispatches that observe new bytes publish
    /// a `com.nexus.terminal.output.<session_id>` custom event with
    /// payload [`OutputStreamPayload`] — the streaming half of WI-12.
    /// The legacy poll path keeps working unchanged.
    #[must_use]
    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
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
        if self.event_bus.is_none() {
            return None;
        }
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
    /// the freshly-drained bytes, assigning the next per-session
    /// sequence number under the emitters lock. `next_cursor` is the
    /// caller-side cursor that comes after this chunk and is
    /// informational only — it's already been recorded by
    /// [`Self::fetch_new_bytes`].
    fn publish_output(&self, id: &SessionId, _next_cursor: u64, data: Vec<u8>) {
        let Some(bus) = self.event_bus.as_ref() else {
            return;
        };
        let seq = match self.emitters.lock() {
            Ok(mut emitters) => {
                let entry = emitters.entry(id.clone()).or_default();
                entry.next_seq = entry.next_seq.saturating_add(1);
                if entry.next_seq == 0 {
                    // saturating_add into u64::MAX would have set it to MAX,
                    // not 0; the explicit check is defensive only.
                    entry.next_seq = 1;
                }
                entry.next_seq
            }
            Err(_) => {
                // Lock poisoned — skip publish rather than panic; the
                // pump return value is still correct for the caller.
                return;
            }
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
                    "failed to serialize output stream payload"
                );
                return;
            }
        };
        if let Err(err) = bus.publish_plugin(PLUGIN_ID, &type_id, payload_value) {
            tracing::warn!(
                plugin = PLUGIN_ID,
                %err,
                session = id.as_str(),
                "failed to publish terminal output event"
            );
        }
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
            if std::time::Instant::now() >= deadline {
                panic!("never saw both lines; last snapshot: {texts:?}");
            }
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
            if std::time::Instant::now() >= deadline {
                panic!("buffer never reached 3 lines");
            }
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
        if !unix_only("pump_publishes_output_event_with_monotonic_seq") {
            return;
        }
        use nexus_kernel::{EventFilter, NexusEvent};

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
            match sub.try_recv().expect("bus alive") {
                Some(evt) => break evt,
                None => {}
            }
            if std::time::Instant::now() >= deadline {
                panic!("never received first output event");
            }
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
            if std::time::Instant::now() >= deadline {
                panic!("second emission never produced seq >= 2 (last={last_seq})");
            }
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
}
