//! BL-052 — universal activity timeline types.
//!
//! The activity timeline is the single audit surface in Nexus: every
//! observable side effect a user *or* an agent *or* a plugin triggers
//! lands here. AI calls (BL-037) were the first emitter and these
//! types were originally introduced in `nexus-ai`. BL-052 lifted them
//! to this leaf crate so other subsystems (terminal, git, storage,
//! workflow, capability) can publish without depending on `nexus-ai`.
//!
//! ## Topics
//!
//! - [`ACTIVITY_APPENDED_TOPIC`] — kernel-owned, every emitter publishes
//!   here. Subscribers iterate by `origin` to slice across surfaces.
//! - [`AI_ACTIVITY_APPENDED_TOPIC`] — back-compat alias kept alive by
//!   `nexus-ai`'s recorder so existing AI-only subscribers don't break.
//!
//! ## Origin
//!
//! [`ActivityOrigin`] is a single string-on-the-wire discriminator with
//! a structured constructor surface. Wire forms — read by the shell as
//! a plain string and pattern-matched by prefix:
//!
//! - `ai`
//! - `user`
//! - `plugin:<plugin_id>`
//! - `workflow:<run_id>`
//! - `agent:<session_id>`
//! - `terminal:<session_id>`
//! - `git`
//! - `storage`
//! - `capability`
//!
//! The format matches the BL-052 / BL-057 DoD literally so the shell's
//! `origin` filter chip can render labels by splitting on the first `:`.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Universal kernel-owned bus topic. Every emitter publishes here.
pub const ACTIVITY_APPENDED_TOPIC: &str = "com.nexus.activity.appended";

/// Legacy AI-only topic. Preserved so pre-BL-052 subscribers don't break.
/// `nexus-ai`'s recorder publishes to BOTH this and [`ACTIVITY_APPENDED_TOPIC`].
pub const AI_ACTIVITY_APPENDED_TOPIC: &str = "com.nexus.ai.activity_appended";

/// Hard cap on prompt / message text stored in an entry. Truncation
/// happens at emit time. Keeps the shell store and on-disk JSONL log
/// bounded even when a caller pastes a huge string.
pub const ACTIVITY_PROMPT_MAX_CHARS: usize = 256;

/// Surface that originated the entry. Lossy on the wire — unknown
/// values land in [`ActivitySurface::Other`] so a future emitter
/// (community plugin) doesn't crash deserialisation. Producers that
/// don't fit any AI surface should use [`ActivitySurface::Other`] and
/// rely on `origin` for typing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum ActivitySurface {
    /// Chat panel (`stream_chat`, mode=chat).
    Chat,
    /// RAG retrieve + chat (`stream_ask`).
    Ask,
    /// Cmd+I command-anywhere overlay (BL-032).
    CmdI,
    /// Inline ghost completion (BL-034, mode=complete).
    Ghost,
    /// Headless single-shot completion (`complete` CLI / mode=complete).
    Complete,
    /// Auto-enrichment on save (BL-045).
    Enrich,
    /// File-system event surface — write / delete / rename.
    File,
    /// Process / terminal session lifecycle (BL-057).
    Process,
    /// Git command boundary — commit / push / pull / merge.
    Git,
    /// Workflow run boundary — start / end.
    Workflow,
    /// Capability grant / revoke (security audit).
    Capability,
    /// Catch-all when the surface tag is missing or unknown.
    Other,
}

impl ActivitySurface {
    /// Parse a wire string into a surface tag, falling back to
    /// [`ActivitySurface::Other`] for unknown values. Tolerant on
    /// purpose so a future surface added by a community plugin
    /// doesn't crash deserialisation.
    #[must_use]
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "chat" => Self::Chat,
            "ask" => Self::Ask,
            "cmdi" | "cmd-i" | "cmd_i" => Self::CmdI,
            "ghost" => Self::Ghost,
            "complete" => Self::Complete,
            "enrich" => Self::Enrich,
            "file" => Self::File,
            "process" => Self::Process,
            "git" => Self::Git,
            "workflow" => Self::Workflow,
            "capability" => Self::Capability,
            _ => Self::Other,
        }
    }
}

/// Outcome of the activity. Captures success vs failure separately
/// from the (possibly error) free-form `error` string so the UI can
/// flash an error glyph without parsing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum ActivityOutcome {
    /// Operation completed successfully.
    Ok,
    /// Operation errored. `error` carries the message when set.
    Error,
    /// User cancelled mid-operation.
    Cancelled,
}

/// Origin of an activity entry — who or what triggered it. Wire form
/// is a single string with a `kind` or `kind:detail` shape so subscribers
/// can split on the first `:` to extract the structured detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityOrigin {
    /// AI surface (chat / ask / cmdi / ghost / complete / enrich).
    Ai,
    /// Direct user action via the shell (file open, manual file write,
    /// etc.) — distinct from `ai` so the timeline can render "you did
    /// X" vs "the model did X".
    User,
    /// Any plugin id (`com.example.foo`). Storage / git / capability
    /// can fall back to this when no more specific tag fits.
    Plugin(String),
    /// Workflow run id (matches `com.nexus.workflow.run_started`).
    Workflow(String),
    /// Agent session id (matches `com.nexus.agent.session_started`).
    Agent(String),
    /// Terminal session id (matches `com.nexus.terminal.events.<id>`).
    Terminal(String),
    /// Git operation — origin doesn't carry an id since git ops are
    /// best identified by the metadata payload (commit sha, branch).
    Git,
    /// Storage event — see metadata for the event subtype.
    Storage,
    /// Capability grant / revoke (security audit).
    Capability,
}

impl ActivityOrigin {
    /// Render as the wire string used in [`ActivityEntry::origin`].
    #[must_use]
    pub fn to_wire(&self) -> String {
        match self {
            Self::Ai => "ai".into(),
            Self::User => "user".into(),
            Self::Plugin(id) => format!("plugin:{id}"),
            Self::Workflow(id) => format!("workflow:{id}"),
            Self::Agent(id) => format!("agent:{id}"),
            Self::Terminal(id) => format!("terminal:{id}"),
            Self::Git => "git".into(),
            Self::Storage => "storage".into(),
            Self::Capability => "capability".into(),
        }
    }

    /// Parse a wire string back into an origin. Tolerant — unknown
    /// kinds fall back to `Plugin(<full_string>)` so a future emitter
    /// surfaces in the timeline without crashing.
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        match s.split_once(':') {
            Some(("plugin", rest)) => Self::Plugin(rest.into()),
            Some(("workflow", rest)) => Self::Workflow(rest.into()),
            Some(("agent", rest)) => Self::Agent(rest.into()),
            Some(("terminal", rest)) => Self::Terminal(rest.into()),
            None => match s {
                "ai" => Self::Ai,
                "user" => Self::User,
                "git" => Self::Git,
                "storage" => Self::Storage,
                "capability" => Self::Capability,
                other => Self::Plugin(other.into()),
            },
            // Unknown prefix — preserve verbatim under the catch-all
            // so the round-trip is lossless.
            Some(_) => Self::Plugin(s.into()),
        }
    }

    /// Return the prefix kind (the part before `:`, or the whole
    /// string for kinds without a detail). Used by the shell's
    /// `origin` filter chip to bucket entries.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Ai => "ai",
            Self::User => "user",
            Self::Plugin(_) => "plugin",
            Self::Workflow(_) => "workflow",
            Self::Agent(_) => "agent",
            Self::Terminal(_) => "terminal",
            Self::Git => "git",
            Self::Storage => "storage",
            Self::Capability => "capability",
        }
    }
}

/// One tool call attempted during a chat round (AI emitter only) — or,
/// generalised, any sub-step that an emitter wants to surface inline
/// inside a single entry. Captures a name + ok/error split; the full
/// input/output is intentionally not persisted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ActivityToolCall {
    /// Registered name of the tool (e.g. `read_file`, `write_file`).
    pub name: String,
    /// `false` if the executor reported an error.
    pub ok: bool,
}

/// One entry in the activity timeline.
///
/// AI surfaces (BL-037) persist these as JSONL in `.forge/ai-activity.log`.
/// Non-AI emitters publish the entry to [`ACTIVITY_APPENDED_TOPIC`] only
/// — there's no on-disk audit log for them in v1; the bus is the audit
/// surface and the shell timeline mirrors what passed through it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
// Pre-BL-052 entries on disk lack `origin`; `#[serde(default)]` on
// the field handles missing-on-read (deny_unknown_fields rejects
// extra fields, not missing ones), so legacy log lines still parse
// cleanly. `deny_unknown_fields` is kept on for the audit-2026-05-01
// P0-2 schema invariant (every object schema must set
// additionalProperties:false).
#[serde(deny_unknown_fields)]
pub struct ActivityEntry {
    /// UUID v4 — stable across reads, useful for de-duping.
    pub id: String,
    /// RFC3339 wall-clock timestamp (UTC).
    pub timestamp: String,
    /// Originating session id (matches `com.nexus.ai.stream_*` events,
    /// or the per-emitter session id for non-AI surfaces).
    pub session_id: String,
    /// Surface that triggered the call.
    pub surface: ActivitySurface,
    /// BL-052 origin discriminator. Defaults to `"ai"` so legacy
    /// JSONL entries (written before BL-052) still parse.
    #[serde(default = "default_origin_ai")]
    pub origin: String,
    /// Provider name (`anthropic` / `openai` / `ollama`) when the
    /// emitter is AI; ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Concrete model id, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Truncated prompt / message text. AI emitters fill from the last
    /// user message; non-AI emitters fill from a short summary line
    /// (e.g. "git commit a1b2c3", "saved Cargo.toml").
    #[serde(default)]
    pub prompt: String,
    /// Files referenced — RAG sources for AI, the affected paths for
    /// storage, the staged paths for git.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Tool calls / sub-steps. AI emitters fill this; other emitters
    /// usually leave it empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ActivityToolCall>,
    /// Final outcome.
    pub outcome: ActivityOutcome,
    /// Error message when `outcome=error`. Free-form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Wall-clock duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

fn default_origin_ai() -> String {
    "ai".into()
}

impl ActivityEntry {
    /// Construct a minimally-populated entry tagged with `origin` and
    /// `surface`. Fields the recorder will fill (`id`, `timestamp`)
    /// get sensible defaults; everything else is up to the caller.
    #[must_use]
    pub fn now(session_id: String, surface: ActivitySurface, origin: ActivityOrigin) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id,
            surface,
            origin: origin.to_wire(),
            provider: None,
            model: None,
            prompt: String::new(),
            files: Vec::new(),
            tool_calls: Vec::new(),
            outcome: ActivityOutcome::Ok,
            error: None,
            duration_ms: None,
        }
    }

    /// Back-compat constructor — defaults the origin to `Ai`. Existing
    /// AI call sites that pre-date BL-052 use this so they don't have
    /// to thread the origin through. Prefer [`ActivityEntry::now`] for
    /// new code.
    #[must_use]
    pub fn now_ai(session_id: String, surface: ActivitySurface) -> Self {
        Self::now(session_id, surface, ActivityOrigin::Ai)
    }
}

/// Truncate `s` in place to at most `max_chars` chars (Unicode-safe),
/// appending an ellipsis when truncated. Shared by every emitter so
/// the bound is uniform.
pub fn truncate_prompt(s: &mut String, max_chars: usize) {
    if s.chars().count() <= max_chars {
        return;
    }
    let take = max_chars.saturating_sub(1);
    let mut new = String::with_capacity(take + 1);
    for c in s.chars().take(take) {
        new.push(c);
    }
    new.push('…');
    *s = new;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_wire_round_trip_known_kinds() {
        let cases = [
            ActivityOrigin::Ai,
            ActivityOrigin::User,
            ActivityOrigin::Git,
            ActivityOrigin::Storage,
            ActivityOrigin::Capability,
            ActivityOrigin::Plugin("com.example.foo".into()),
            ActivityOrigin::Workflow("run-123".into()),
            ActivityOrigin::Agent("sess-abc".into()),
            ActivityOrigin::Terminal("tty-1".into()),
        ];
        for c in cases {
            let wire = c.to_wire();
            let back = ActivityOrigin::from_wire(&wire);
            assert_eq!(back, c, "{wire}");
        }
    }

    #[test]
    fn origin_unknown_kind_falls_back_to_plugin() {
        let parsed = ActivityOrigin::from_wire("custom:thing");
        assert_eq!(parsed, ActivityOrigin::Plugin("custom:thing".into()));
        let parsed_no_colon = ActivityOrigin::from_wire("future");
        assert_eq!(parsed_no_colon, ActivityOrigin::Plugin("future".into()));
    }

    #[test]
    fn origin_kind_strips_detail() {
        assert_eq!(
            ActivityOrigin::Plugin("anything".into()).kind(),
            "plugin",
        );
        assert_eq!(
            ActivityOrigin::Terminal("tty-7".into()).kind(),
            "terminal",
        );
        assert_eq!(ActivityOrigin::Ai.kind(), "ai");
    }

    #[test]
    fn entry_legacy_log_line_parses_with_default_origin() {
        // A JSONL line written before BL-052 lacked the `origin` field.
        // Confirm it still parses with origin defaulted to "ai".
        let legacy = r#"{
            "id":"id-1",
            "timestamp":"2026-04-29T00:00:00Z",
            "session_id":"sess-1",
            "surface":"chat",
            "prompt":"hi",
            "outcome":"ok"
        }"#;
        let entry: ActivityEntry = serde_json::from_str(legacy).unwrap();
        assert_eq!(entry.origin, "ai");
        assert_eq!(entry.surface, ActivitySurface::Chat);
    }

    #[test]
    fn entry_round_trips_with_origin_field() {
        let entry = ActivityEntry::now(
            "sess-1".into(),
            ActivitySurface::Process,
            ActivityOrigin::Terminal("tty-1".into()),
        );
        let line = serde_json::to_string(&entry).unwrap();
        assert!(line.contains("\"origin\":\"terminal:tty-1\""));
        assert!(line.contains("\"surface\":\"process\""));
        let back: ActivityEntry = serde_json::from_str(&line).unwrap();
        assert_eq!(back.origin, "terminal:tty-1");
        assert_eq!(back.surface, ActivitySurface::Process);
    }

    #[test]
    fn surface_from_str_lossy_normalizes_known_aliases() {
        assert_eq!(ActivitySurface::from_str_lossy("chat"), ActivitySurface::Chat);
        assert_eq!(ActivitySurface::from_str_lossy("cmdi"), ActivitySurface::CmdI);
        assert_eq!(ActivitySurface::from_str_lossy("cmd-i"), ActivitySurface::CmdI);
        assert_eq!(ActivitySurface::from_str_lossy("cmd_i"), ActivitySurface::CmdI);
        assert_eq!(ActivitySurface::from_str_lossy("process"), ActivitySurface::Process);
        assert_eq!(
            ActivitySurface::from_str_lossy("not-a-surface"),
            ActivitySurface::Other,
        );
    }

    #[test]
    fn truncate_prompt_unicode_safe_under_limit_is_noop() {
        let mut s = "hello".to_string();
        truncate_prompt(&mut s, 10);
        assert_eq!(s, "hello");
    }

    #[test]
    fn truncate_prompt_preserves_grapheme_boundaries_with_ellipsis() {
        let mut s = "🦀🦀🦀🦀🦀".to_string();
        truncate_prompt(&mut s, 3);
        assert_eq!(s.chars().count(), 3);
        assert!(s.ends_with('…'));
    }
}
