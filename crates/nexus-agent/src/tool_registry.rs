//! Agent-facing tool registry (PRD-15 §4).
//!
//! Distinct from `nexus_ai::tools::ToolRegistry`:
//!
//! - The AI registry advertises tools *to the model* so it can produce
//!   tool-call proposals. It owns schemas and executors that drive
//!   provider function-calling.
//! - This registry is the *agent loop's* view of the same tools. Every
//!   entry carries capability requirements, an approval flag, and an
//!   estimated-duration hint so the session policy can gate calls
//!   before they reach the dispatcher.
//!
//! The two registries are deliberately not the same type. The AI side
//! is per-request (built from a kernel context plus optional MCP
//! bridging); the agent side is a process-global static catalogue
//! seeded at bootstrap and read through `com.nexus.agent::list_tools`
//! by the CLI / shell.
//!
//! ## Lookup
//!
//! `AgentToolRegistry::global()` returns the process-global registry.
//! Bootstrap calls [`seed_default_tools`] once at boot; tests should
//! avoid mutating the global and use [`AgentToolRegistry::new`] +
//! local [`AgentToolRegistry::register`] instead.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Capability domains an agent can hold. Matches PRD-15 §4's
/// `Capability` enum. The string form (returned by [`Capability::as_str`])
/// is what `nexus tool list` prints, what `.agent.toml` accepts,
/// and what the wire form serializes to — `Serialize` and
/// `Deserialize` go through the dotted-id form directly so a
/// `required_capabilities` array reads as `["fs.read", "search.forge"]`
/// rather than a Serde-tagged object.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/",
        type = "\"fs.read\" | \"fs.write\" | \"terminal.execute\" | \"search.forge\" | \"web.fetch\" | \"mcp.host\" | \"git.read\" | \"git.write\" | \"database.read\" | \"database.write\""
    )
)]
pub enum Capability {
    /// Forge file reads.
    FileSystemRead,
    /// Forge file writes (and deletes — keeps the cardinality low; if
    /// granular control is ever needed, split into `FileSystemDelete`).
    FileSystemWrite,
    /// Terminal session creation / send-input.
    TerminalExecute,
    /// Search the forge index (FTS or vector).
    SearchForge,
    /// Web fetch (HTTP `GET`).
    WebFetch,
    /// MCP-host tool calls.
    McpHost,
    /// Git plumbing (log, diff, blame, stage, commit).
    GitRead,
    /// Git mutations (commit, push, reset).
    GitWrite,
    /// Database query / mutation through `com.nexus.database`.
    DatabaseRead,
    /// Database writes.
    DatabaseWrite,
}

impl Capability {
    /// Stable lowercase identifier for serialization and display.
    /// Used by `.agent.toml` `capabilities = […]` entries.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FileSystemRead => "fs.read",
            Self::FileSystemWrite => "fs.write",
            Self::TerminalExecute => "terminal.execute",
            Self::SearchForge => "search.forge",
            Self::WebFetch => "web.fetch",
            Self::McpHost => "mcp.host",
            Self::GitRead => "git.read",
            Self::GitWrite => "git.write",
            Self::DatabaseRead => "database.read",
            Self::DatabaseWrite => "database.write",
        }
    }

    /// Parse a string id produced by [`Capability::as_str`].
    // Inherent `from_str` returns `Option`, not the `Result`-based
    // `std::str::FromStr`; the lenient Option API is intentional.
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "fs.read" => Some(Self::FileSystemRead),
            "fs.write" => Some(Self::FileSystemWrite),
            "terminal.execute" => Some(Self::TerminalExecute),
            "search.forge" => Some(Self::SearchForge),
            "web.fetch" => Some(Self::WebFetch),
            "mcp.host" => Some(Self::McpHost),
            "git.read" => Some(Self::GitRead),
            "git.write" => Some(Self::GitWrite),
            "database.read" => Some(Self::DatabaseRead),
            "database.write" => Some(Self::DatabaseWrite),
            _ => None,
        }
    }
}

impl Serialize for Capability {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s: String = String::deserialize(deserializer)?;
        Self::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown capability id '{s}'")))
    }
}

/// What the agent registry knows about a tool. Bigger than
/// `nexus_ai::tools::ToolSchema` — adds capability + approval +
/// duration hints so the session loop can apply policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct AgentToolSpec {
    /// Stable name the model uses to call this tool.
    pub name: String,
    /// One-paragraph description. Mirrors the AI registry's wording.
    pub description: String,
    /// JSON Schema for the tool's input. Top-level type is `object`.
    /// PRD-15 §4 calls for validation against this; the
    /// [`AgentToolRegistry::validate_params`] check is structural
    /// (required fields + additional-property rejection) — strict
    /// schema enforcement is the executor's job.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub input_schema: serde_json::Value,
    /// Whether the user must approve before the agent calls this.
    /// Read by the session policy (ADR 0024 / DG-34); the registry
    /// itself does not gate dispatch.
    pub requires_approval: bool,
    /// Best-guess duration. Surfaced in `nexus tool list` so users
    /// can plan around long-running tools without diving into source.
    /// Not enforced.
    pub estimated_duration_ms: u64,
    /// Capabilities this tool requires. The agent must hold every
    /// capability in this list for [`AgentToolRegistry::call_tool`]
    /// to dispatch.
    pub required_capabilities: Vec<Capability>,
    /// Kernel IPC target — `(target_plugin_id, command_id)` —
    /// `ToolDispatcher` will route the call through this pair. Kept
    /// in the spec so external surfaces (e.g. `nexus tool list`) can
    /// show users where calls actually land.
    pub target_plugin_id: String,
    /// Kernel command id within the target plugin.
    pub command_id: String,
}

/// Errors specific to the agent tool registry. Distinct from
/// `nexus_ai::ToolError` so a `nexus-agent` caller doesn't need to
/// depend on `nexus-ai`.
#[derive(Debug, Error)]
pub enum AgentToolError {
    /// Tool name doesn't resolve.
    #[error("agent tool not found: {0}")]
    NotFound(String),
    /// Caller doesn't hold the capability the tool requires.
    #[error("tool {tool} requires capability {capability}, which the agent does not hold")]
    CapabilityDenied {
        /// Tool whose capability check failed.
        tool: String,
        /// Missing capability id (string form of [`Capability`]).
        capability: &'static str,
    },
    /// Args failed structural validation against the registered schema.
    #[error("invalid params for tool {tool}: {reason}")]
    InvalidParams {
        /// Tool whose params failed validation.
        tool: String,
        /// Human-readable reason — surfaced back to the model so it
        /// can self-correct.
        reason: String,
    },
    /// Underlying dispatcher returned an error.
    #[error("tool {tool} dispatch failed: {reason}")]
    DispatchFailed {
        /// Tool whose dispatch errored.
        tool: String,
        /// Verbatim error from the dispatcher.
        reason: String,
    },
}

/// Audit record appended every time the registry routes a call.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct AgentToolAccessRecord {
    /// Unix epoch milliseconds when the call completed.
    pub completed_at_ms: u64,
    /// Agent id (e.g. `com.nexus.agent.coder`) that issued the call.
    pub agent_id: String,
    /// Tool name as registered.
    pub tool: String,
    /// Whether the dispatcher returned `Ok`.
    pub success: bool,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Process-global agent tool registry. Read-mostly: bootstrap seeds
/// it once via [`seed_default_tools`], then read through
/// [`AgentToolRegistry::list_for_agent`] and
/// [`AgentToolRegistry::lookup`] from request handlers.
#[derive(Default)]
pub struct AgentToolRegistry {
    tools: Mutex<HashMap<String, AgentToolSpec>>,
    access_log: Mutex<Vec<AgentToolAccessRecord>>,
}

static GLOBAL: OnceLock<Arc<AgentToolRegistry>> = OnceLock::new();

impl AgentToolRegistry {
    /// Construct an empty registry. Used by tests; production code
    /// reads from [`AgentToolRegistry::global`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the process-global registry, creating it on first use.
    #[must_use]
    pub fn global() -> Arc<Self> {
        Arc::clone(GLOBAL.get_or_init(|| Arc::new(Self::default())))
    }

    /// Register a tool spec. Re-registering an existing name
    /// overwrites the previous entry — same posture as the AI
    /// registry, so a `seed_default_tools` re-run is idempotent.
    pub fn register(&self, spec: AgentToolSpec) {
        let mut tools = self.tools.lock().expect("agent tool registry mutex");
        tools.insert(spec.name.clone(), spec);
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.lock().expect("agent tool registry mutex").len()
    }

    /// Whether the registry has no tools.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools
            .lock()
            .expect("agent tool registry mutex")
            .is_empty()
    }

    /// Snapshot every spec. Order is unspecified; callers that need
    /// stable rendering should sort by `name`.
    #[must_use]
    pub fn list_all(&self) -> Vec<AgentToolSpec> {
        self.tools
            .lock()
            .expect("agent tool registry mutex")
            .values()
            .cloned()
            .collect()
    }

    /// Subset of [`AgentToolRegistry::list_all`] filtered to tools the
    /// given capability set satisfies. Used by `list_tools_for_agent`
    /// in PRD-15 §4 and by `nexus tool list --agent <id>`.
    #[must_use]
    pub fn list_for_agent(&self, capabilities: &[Capability]) -> Vec<AgentToolSpec> {
        let owned: std::collections::HashSet<&Capability> = capabilities.iter().collect();
        self.list_all()
            .into_iter()
            .filter(|spec| {
                spec.required_capabilities
                    .iter()
                    .all(|cap| owned.contains(cap))
            })
            .collect()
    }

    /// Look up a tool by name. `None` if not registered.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<AgentToolSpec> {
        self.tools
            .lock()
            .expect("agent tool registry mutex")
            .get(name)
            .cloned()
    }

    /// Cheap structural validation of params against the spec's
    /// `input_schema`. Checks `required` fields and rejects
    /// `additionalProperties: false` violations. Not a full JSON
    /// Schema validator — strict shape checks belong in the
    /// executor.
    ///
    /// # Errors
    /// Returns [`AgentToolError::InvalidParams`] when a required key
    /// is missing or an unknown key is present (when the schema
    /// declares `additionalProperties: false`).
    pub fn validate_params(
        spec: &AgentToolSpec,
        params: &serde_json::Value,
    ) -> Result<(), AgentToolError> {
        let Some(obj) = params.as_object() else {
            return Err(AgentToolError::InvalidParams {
                tool: spec.name.clone(),
                reason: "params must be a JSON object".to_string(),
            });
        };

        let schema = &spec.input_schema;

        if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
            for field in required {
                let Some(name) = field.as_str() else { continue };
                if !obj.contains_key(name) {
                    return Err(AgentToolError::InvalidParams {
                        tool: spec.name.clone(),
                        reason: format!("missing required field `{name}`"),
                    });
                }
            }
        }

        let additional_allowed = schema
            .get("additionalProperties")
            .is_none_or(|v| v.as_bool().unwrap_or(true));
        if !additional_allowed {
            let properties = schema
                .get("properties")
                .and_then(|v| v.as_object())
                .map(|m| m.keys().cloned().collect::<std::collections::HashSet<_>>())
                .unwrap_or_default();
            for key in obj.keys() {
                if !properties.contains(key) {
                    return Err(AgentToolError::InvalidParams {
                        tool: spec.name.clone(),
                        reason: format!("unknown field `{key}`"),
                    });
                }
            }
        }

        Ok(())
    }

    /// Check the agent holds every capability the spec requires.
    ///
    /// # Errors
    /// Returns [`AgentToolError::CapabilityDenied`] on the first
    /// missing capability.
    pub fn check_capabilities(
        spec: &AgentToolSpec,
        held: &[Capability],
    ) -> Result<(), AgentToolError> {
        let owned: std::collections::HashSet<&Capability> = held.iter().collect();
        for cap in &spec.required_capabilities {
            if !owned.contains(cap) {
                return Err(AgentToolError::CapabilityDenied {
                    tool: spec.name.clone(),
                    capability: cap.as_str(),
                });
            }
        }
        Ok(())
    }

    /// Record a dispatch outcome in the in-memory access log.
    /// `nexus-bootstrap` will eventually want to fan these into the
    /// universal-activity bus (BL-052) but the in-memory log alone
    /// satisfies the PRD-15 §4 "log tool access for audit/debugging"
    /// requirement.
    pub fn record_access(
        &self,
        agent_id: &str,
        tool: &str,
        success: bool,
        duration: Duration,
        completed_at: std::time::SystemTime,
    ) {
        let completed_at_ms = completed_at
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let mut log = self.access_log.lock().expect("agent tool access log mutex");
        log.push(AgentToolAccessRecord {
            completed_at_ms,
            agent_id: agent_id.to_string(),
            tool: tool.to_string(),
            success,
            duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        });
        // Bound the log so a long-running session can't OOM the process.
        // 1024 entries is enough to debug a recent run; deeper history
        // lives in the per-session transcript on disk.
        const MAX_LOG_ENTRIES: usize = 1024;
        if log.len() > MAX_LOG_ENTRIES {
            let drop = log.len() - MAX_LOG_ENTRIES;
            log.drain(..drop);
        }
    }

    /// Snapshot of the access log, newest-last (insertion order).
    #[must_use]
    pub fn access_log(&self) -> Vec<AgentToolAccessRecord> {
        self.access_log
            .lock()
            .expect("agent tool access log mutex")
            .clone()
    }
}

/// Convenience wrapper that records the duration around a closure
/// and appends to the access log on completion. Used by the session
/// loop once it's wired through `KernelToolBridge`.
pub fn measure_dispatch<F, T, E>(
    registry: &AgentToolRegistry,
    agent_id: &str,
    tool: &str,
    f: F,
) -> Result<T, E>
where
    F: FnOnce() -> Result<T, E>,
{
    let started = Instant::now();
    let out = f();
    registry.record_access(
        agent_id,
        tool,
        out.is_ok(),
        started.elapsed(),
        std::time::SystemTime::now(),
    );
    out
}

/// Populate [`AgentToolRegistry::global`] with the in-tree catalogue:
/// the same set of tools the AI registry seeds on every chat request,
/// but tagged with the agent-side metadata (capabilities, approval,
/// duration hint).
///
/// Idempotent — re-registration overwrites. Bootstrap calls this once
/// after the core plugins register; tests should construct a fresh
/// [`AgentToolRegistry::new`] instead of mutating the global.
pub fn seed_default_tools() {
    let registry = AgentToolRegistry::global();
    for spec in default_tool_catalog() {
        registry.register(spec);
    }
}

/// Static catalogue of agent tools. Each entry's
/// `(target_plugin_id, command_id)` mirrors the IPC route the AI
/// executor uses for the same tool. Kept here rather than wired
/// dynamically so `nexus tool list` works without a kernel context
/// (CLI surface needs the catalogue at parse time).
#[must_use]
pub fn default_tool_catalog() -> Vec<AgentToolSpec> {
    vec![
        AgentToolSpec {
            name: "read_file".to_string(),
            description: "Read the UTF-8 contents of a forge-relative file. The reply \
                includes the file's hashline `tag` — pass it to `edit` to make a precise \
                in-place change."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 50,
            required_capabilities: vec![Capability::FileSystemRead],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "read_file".to_string(),
        },
        AgentToolSpec {
            name: "read_lines".to_string(),
            description: "Read a 1-based, inclusive line range of a forge file — use this \
                instead of read_file for large files to keep context small. Returns the \
                slice plus `total_lines` and the file's hashline `tag` (for `edit`). \
                Defaults to the first 200 lines when `start`/`end` are omitted."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "start": { "type": "integer", "minimum": 1 },
                    "end": { "type": "integer", "minimum": 1 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 50,
            required_capabilities: vec![Capability::FileSystemRead],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "read_lines".to_string(),
        },
        AgentToolSpec {
            name: "write_file".to_string(),
            description: "Write or overwrite a whole forge-relative file. To change part \
                of an existing file, prefer `edit`."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            requires_approval: true,
            estimated_duration_ms: 100,
            required_capabilities: vec![Capability::FileSystemWrite],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "write_file".to_string(),
        },
        AgentToolSpec {
            name: "edit".to_string(),
            description: "Apply a hashline patch — the precise way to change part of a \
                file (prefer over write_file for edits). A patch is one or more sections, \
                each headed by `[PATH#TAG]` where TAG is the 4-hex `tag` from read_file. \
                Operations use 1-based, inclusive line numbers: `SWAP a.=b:` followed by \
                `+`-prefixed replacement lines; `DEL a.=b`; `INS.PRE n:` / `INS.POST n:` \
                / `INS.HEAD:` / `INS.TAIL:` followed by `+` lines. A bare `+` is a blank \
                line. Multiple `[PATH#TAG]` sections edit several files atomically. If the \
                file changed since you read it, a 3-way merge is attempted; unresolved \
                sections come back as `conflicts` and nothing is written, so re-read and \
                retry."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "patch": { "type": "string" } },
                "required": ["patch"],
                "additionalProperties": false
            }),
            requires_approval: true,
            estimated_duration_ms: 100,
            required_capabilities: vec![Capability::FileSystemWrite],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "edit".to_string(),
        },
        AgentToolSpec {
            name: "search_forge".to_string(),
            description: "FTS over forge markdown.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 150,
            required_capabilities: vec![Capability::SearchForge],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "search".to_string(),
        },
        AgentToolSpec {
            name: "list_backlinks".to_string(),
            description: "List notes that link to the target.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "target": { "type": "string" } },
                "required": ["target"],
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 100,
            required_capabilities: vec![Capability::SearchForge],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "backlinks".to_string(),
        },
        AgentToolSpec {
            name: "list_dir".to_string(),
            description: "List the entries (files and subdirectories) of a forge-relative \
                directory. Omit `relpath` or pass an empty string for the forge root. Use \
                this to explore the forge before reading or editing."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "relpath": { "type": "string" } },
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 50,
            required_capabilities: vec![Capability::FileSystemRead],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "list_dir".to_string(),
        },
        AgentToolSpec {
            name: "grep".to_string(),
            description: "Search file contents across the forge for a substring or regex \
                (ripgrep-style), returning matching lines with their paths and line \
                numbers. Set `is_regex` for a pattern. Distinct from `search_forge`, which \
                is a ranked full-text search over indexed notes."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "is_regex": { "type": "boolean" },
                    "case_sensitive": { "type": "boolean" },
                    "whole_word": { "type": "boolean" },
                    "max_files": { "type": "integer", "minimum": 1 },
                    "max_results": { "type": "integer", "minimum": 1 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 300,
            required_capabilities: vec![Capability::FileSystemRead],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "find_in_files".to_string(),
        },
        AgentToolSpec {
            name: "find_symbol".to_string(),
            description: "Look up code symbols (functions, types, …) by exact name in the \
                forge's code index, optionally scoped to one file. Returns each symbol's \
                path and line — useful for navigating code before editing."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "path": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 100,
            required_capabilities: vec![Capability::SearchForge],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "query_symbol".to_string(),
        },
        AgentToolSpec {
            name: "git_log".to_string(),
            description: "Most-recent N commits on the current branch.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "limit": { "type": "integer", "minimum": 1 } },
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 200,
            required_capabilities: vec![Capability::GitRead],
            target_plugin_id: "com.nexus.git".to_string(),
            command_id: "log".to_string(),
        },
        AgentToolSpec {
            name: "terminal_run_saved".to_string(),
            description: "Run a saved terminal command by slug.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "command": { "type": "string" }
                },
                "required": ["slug"],
                "additionalProperties": false
            }),
            requires_approval: true,
            estimated_duration_ms: 2_000,
            required_capabilities: vec![Capability::TerminalExecute],
            target_plugin_id: "com.nexus.terminal".to_string(),
            command_id: "run_saved".to_string(),
        },
        AgentToolSpec {
            name: "terminal_get_status".to_string(),
            description: "Read a terminal session's status.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 50,
            required_capabilities: vec![Capability::TerminalExecute],
            target_plugin_id: "com.nexus.terminal".to_string(),
            command_id: "get_session_info".to_string(),
        },
        AgentToolSpec {
            name: "terminal_send_signal".to_string(),
            description: "Send SIGINT / SIGQUIT / SIGTSTP / EOF to a session.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "signal": { "type": "string" }
                },
                "required": ["id", "signal"],
                "additionalProperties": false
            }),
            requires_approval: true,
            estimated_duration_ms: 50,
            required_capabilities: vec![Capability::TerminalExecute],
            target_plugin_id: "com.nexus.terminal".to_string(),
            command_id: "send_raw_input".to_string(),
        },
        // DG-37 — agent-to-agent delegation. The "tool" is itself a
        // sub-session run. `auto_approve` defaults to true on the
        // server side so the user isn't prompted twice for nested
        // approvals; the parent's approval gate is sufficient.
        AgentToolSpec {
            name: "delegate_to_agent".to_string(),
            description: "Hand a sub-goal to another agent archetype. Returns the child session's \
                full transcript (rounds + tool calls + outcome). Use when the work \
                naturally splits along an archetype boundary (e.g. ask the auditor to \
                review what the writer just produced)."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "archetype": {
                        "type": "string",
                        "description": "Target archetype id (writer / coder / researcher / auditor / librarian / coach)."
                    },
                    "goal": {
                        "type": "string",
                        "description": "Natural-language sub-goal for the child agent."
                    }
                },
                "required": ["archetype", "goal"],
                "additionalProperties": false
            }),
            // Delegation is itself a write-class action — the child
            // can call write tools. Flag for approval so the parent's
            // session policy can decide whether the user wants the
            // sub-run to proceed.
            requires_approval: true,
            estimated_duration_ms: 5_000,
            required_capabilities: vec![Capability::FileSystemRead],
            target_plugin_id: "com.nexus.agent".to_string(),
            command_id: "delegate".to_string(),
        },
        // ── BL-132 follow-up: destructive operations ────────────
        // BL-132 + DG-34 set up the approval gate (BusBridgePolicy
        // auto-approves rounds whose tool calls are all
        // `requires_approval = false`, prompts otherwise). The
        // following three handlers correspond to the BL-132 DoD's
        // "approval_required flag on at minimum: nexus-storage::
        // delete_file, nexus-git::push (force flag path),
        // nexus-storage::replace_in_files (> N files)" list. Adding
        // them here gives agents the capability + flags them
        // destructive so the prompt fires under `--interactive` /
        // strict_approval. `execute_command` (the fourth item on the
        // DoD list) is already covered by `terminal_run_saved` above.
        AgentToolSpec {
            name: "delete_file".to_string(),
            description: "Delete a forge-relative file. Destructive — \
                no built-in undo; the user should confirm before \
                approving."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            requires_approval: true,
            estimated_duration_ms: 50,
            required_capabilities: vec![Capability::FileSystemWrite],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "delete_file".to_string(),
        },
        AgentToolSpec {
            name: "replace_in_files".to_string(),
            description: "Find-and-replace across forge files \
                (literal or regex). Destructive — touches multiple \
                files; review the affected file count before approving."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "replace": { "type": "string" },
                    "regex": { "type": "boolean" },
                    "case_sensitive": { "type": "boolean" },
                    "whole_word": { "type": "boolean" },
                    "max_files": { "type": "integer", "minimum": 1 }
                },
                "required": ["query", "replace"],
                "additionalProperties": false
            }),
            requires_approval: true,
            estimated_duration_ms: 500,
            required_capabilities: vec![Capability::FileSystemWrite],
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "replace_in_files".to_string(),
        },
        AgentToolSpec {
            name: "git_push".to_string(),
            description: "Push the current branch to its remote. \
                Destructive — visible to collaborators; force pushes \
                rewrite history."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "remote": { "type": "string" },
                    "branch": { "type": "string" },
                    "force": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
            requires_approval: true,
            estimated_duration_ms: 3_000,
            required_capabilities: vec![Capability::GitWrite],
            target_plugin_id: "com.nexus.git".to_string(),
            command_id: "push".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_spec(name: &str, caps: Vec<Capability>) -> AgentToolSpec {
        AgentToolSpec {
            name: name.to_string(),
            description: format!("test {name}"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "x": { "type": "string" } },
                "required": ["x"],
                "additionalProperties": false
            }),
            requires_approval: false,
            estimated_duration_ms: 1,
            required_capabilities: caps,
            target_plugin_id: "com.test".to_string(),
            command_id: "noop".to_string(),
        }
    }

    /// BL-132 follow-up: the agent tool registry now exposes the four
    /// destructive operations the BL-132 DoD called for (the fourth —
    /// `execute_command` — is covered by `terminal_run_saved`). Every
    /// one must carry `requires_approval = true` so the BL-132
    /// `--interactive` prompt fires before dispatch.
    #[test]
    fn bl132_destructive_tools_registered_with_requires_approval() {
        let catalog = default_tool_catalog();
        for name in ["delete_file", "replace_in_files", "git_push"] {
            let spec = catalog
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("missing destructive tool: {name}"));
            assert!(
                spec.requires_approval,
                "tool {name} must be flagged requires_approval = true",
            );
            assert!(
                !spec.required_capabilities.is_empty(),
                "tool {name} must declare a write capability",
            );
        }
    }

    /// Cross-check that the new tools point at handler ids that
    /// actually exist on the target plugin. Static pin only — the
    /// IPC dispatcher is what enforces this at runtime, but the
    /// command_id strings have to match the manifest registrations
    /// or the call returns a "no handler" error before approval even
    /// runs.
    #[test]
    fn bl132_destructive_tools_target_known_handlers() {
        let catalog = default_tool_catalog();
        let by_name: std::collections::HashMap<_, _> =
            catalog.iter().map(|s| (s.name.as_str(), s)).collect();

        let delete = by_name["delete_file"];
        assert_eq!(delete.target_plugin_id, "com.nexus.storage");
        assert_eq!(delete.command_id, "delete_file");

        let replace = by_name["replace_in_files"];
        assert_eq!(replace.target_plugin_id, "com.nexus.storage");
        assert_eq!(replace.command_id, "replace_in_files");

        let push = by_name["git_push"];
        assert_eq!(push.target_plugin_id, "com.nexus.git");
        assert_eq!(push.command_id, "push");
    }

    #[test]
    fn capability_round_trips_via_str() {
        for cap in [
            Capability::FileSystemRead,
            Capability::FileSystemWrite,
            Capability::TerminalExecute,
            Capability::SearchForge,
            Capability::WebFetch,
            Capability::McpHost,
            Capability::GitRead,
            Capability::GitWrite,
            Capability::DatabaseRead,
            Capability::DatabaseWrite,
        ] {
            let s = cap.as_str();
            assert_eq!(Capability::from_str(s), Some(cap.clone()));
        }
    }

    #[test]
    fn capability_from_str_unknown_returns_none() {
        assert!(Capability::from_str("nope").is_none());
    }

    #[test]
    fn register_and_lookup() {
        let reg = AgentToolRegistry::new();
        reg.register(fake_spec("a", vec![]));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        let spec = reg.lookup("a").expect("lookup");
        assert_eq!(spec.name, "a");
    }

    #[test]
    fn register_overwrites_existing_name() {
        let reg = AgentToolRegistry::new();
        reg.register(fake_spec("a", vec![Capability::FileSystemRead]));
        let mut updated = fake_spec("a", vec![Capability::FileSystemWrite]);
        updated.description = "updated".to_string();
        reg.register(updated);
        assert_eq!(reg.len(), 1);
        let spec = reg.lookup("a").expect("lookup");
        assert_eq!(spec.description, "updated");
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let reg = AgentToolRegistry::new();
        assert!(reg.lookup("missing").is_none());
    }

    #[test]
    fn list_for_agent_filters_by_capability() {
        let reg = AgentToolRegistry::new();
        reg.register(fake_spec("read", vec![Capability::FileSystemRead]));
        reg.register(fake_spec("write", vec![Capability::FileSystemWrite]));
        reg.register(fake_spec(
            "both",
            vec![Capability::FileSystemRead, Capability::FileSystemWrite],
        ));
        let visible = reg.list_for_agent(&[Capability::FileSystemRead]);
        let names: std::collections::HashSet<_> = visible.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("read"));
        assert!(!names.contains("write"));
        assert!(!names.contains("both"));
    }

    #[test]
    fn check_capabilities_passes_when_all_held() {
        let spec = fake_spec("t", vec![Capability::FileSystemRead]);
        AgentToolRegistry::check_capabilities(&spec, &[Capability::FileSystemRead]).unwrap();
    }

    #[test]
    fn check_capabilities_denies_missing() {
        let spec = fake_spec("t", vec![Capability::FileSystemWrite]);
        let err = AgentToolRegistry::check_capabilities(&spec, &[Capability::FileSystemRead])
            .expect_err("should deny");
        match err {
            AgentToolError::CapabilityDenied { tool, capability } => {
                assert_eq!(tool, "t");
                assert_eq!(capability, "fs.write");
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }
    }

    #[test]
    fn validate_params_accepts_well_formed() {
        let spec = fake_spec("t", vec![]);
        AgentToolRegistry::validate_params(&spec, &serde_json::json!({ "x": "hi" })).unwrap();
    }

    #[test]
    fn validate_params_rejects_non_object() {
        let spec = fake_spec("t", vec![]);
        let err = AgentToolRegistry::validate_params(&spec, &serde_json::json!("nope"))
            .expect_err("should reject");
        match err {
            AgentToolError::InvalidParams { tool, reason } => {
                assert_eq!(tool, "t");
                assert!(reason.contains("JSON object"));
            }
            other => panic!("expected InvalidParams, got {other:?}"),
        }
    }

    #[test]
    fn validate_params_rejects_missing_required() {
        let spec = fake_spec("t", vec![]);
        let err = AgentToolRegistry::validate_params(&spec, &serde_json::json!({}))
            .expect_err("should reject");
        match err {
            AgentToolError::InvalidParams { reason, .. } => {
                assert!(reason.contains("missing required field"));
                assert!(reason.contains('x'));
            }
            other => panic!("expected InvalidParams, got {other:?}"),
        }
    }

    #[test]
    fn validate_params_rejects_unknown_field() {
        let spec = fake_spec("t", vec![]);
        let err = AgentToolRegistry::validate_params(
            &spec,
            &serde_json::json!({ "x": "ok", "extra": 1 }),
        )
        .expect_err("should reject");
        match err {
            AgentToolError::InvalidParams { reason, .. } => {
                assert!(reason.contains("unknown field"));
                assert!(reason.contains("extra"));
            }
            other => panic!("expected InvalidParams, got {other:?}"),
        }
    }

    #[test]
    fn access_log_records_and_caps_length() {
        let reg = AgentToolRegistry::new();
        for i in 0..2000 {
            reg.record_access(
                "com.nexus.agent.test",
                "noop",
                i % 2 == 0,
                Duration::from_millis(1),
                std::time::SystemTime::now(),
            );
        }
        let log = reg.access_log();
        assert_eq!(log.len(), 1024);
    }

    #[test]
    fn default_catalog_covers_known_builtins() {
        let names: std::collections::HashSet<_> =
            default_tool_catalog().into_iter().map(|s| s.name).collect();
        for expected in [
            "read_file",
            "write_file",
            "search_forge",
            "list_backlinks",
            "git_log",
            "terminal_run_saved",
            "terminal_get_status",
            "terminal_send_signal",
            "delegate_to_agent",
        ] {
            assert!(names.contains(expected), "missing tool: {expected}");
        }
    }

    #[test]
    fn global_returns_same_arc() {
        let a = AgentToolRegistry::global();
        let b = AgentToolRegistry::global();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn write_file_is_marked_for_approval() {
        let spec = default_tool_catalog()
            .into_iter()
            .find(|s| s.name == "write_file")
            .expect("write_file in catalog");
        assert!(spec.requires_approval);
    }

    #[test]
    fn read_only_navigation_tools_are_present() {
        let catalog = default_tool_catalog();
        for (name, command) in [
            ("list_dir", "list_dir"),
            ("grep", "find_in_files"),
            ("find_symbol", "query_symbol"),
            ("edit", "edit"),
        ] {
            let spec = catalog
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("{name} in catalog"));
            assert_eq!(spec.target_plugin_id, "com.nexus.storage");
            assert_eq!(spec.command_id, command);
        }
        // The three navigation tools are read-only (no approval gate); edit is not.
        for name in ["list_dir", "grep", "find_symbol"] {
            let spec = catalog.iter().find(|s| s.name == name).unwrap();
            assert!(!spec.requires_approval, "{name} must be read-only");
        }
        assert!(
            catalog.iter().find(|s| s.name == "edit").unwrap().requires_approval,
            "edit must require approval",
        );
    }
}
