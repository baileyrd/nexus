//! Wire-mirror IPC types for `com.nexus.terminal`.
//!
//! R8 / #191 — lifted out of `core_plugin.rs` (which exceeded 3500 lines) so
//! the dispatcher module stays focused on lifecycle + dispatch and the
//! wire shapes live in a single auditable place. Re-exported from the
//! crate root (`lib.rs`) so external consumers and the `ipc_schema_emit`
//! drift test keep their existing import paths (`use nexus_terminal::{…}`).
//!
//! Every type below is on the IPC wire — args or replies for one of the
//! handlers listed in `core_plugin.rs`'s top-of-file table. Each derives
//! `#[serde(deny_unknown_fields)]` so a typo on the caller side
//! (`{ idd: "x" }`) surfaces as `PluginCrashedDuringCall` rather than a
//! silent default. Under the `ts-export` feature each type emits a
//! TypeScript binding into
//! `packages/nexus-extension-api/src/generated/ipc/` and a JSON Schema
//! into `crates/nexus-bootstrap/schemas/ipc/`.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;


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

/// Arguments for `rename_session`.
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
pub struct RenameSessionArgs {
    /// Session id whose label is being changed.
    pub id: String,
    /// New human-readable label.
    pub name: String,
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
