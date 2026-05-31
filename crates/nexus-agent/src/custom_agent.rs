//! Custom agent manifest format — PRD-15 §9 (DG-36).
//!
//! Users describe a domain-tuned agent as a TOML manifest at
//! `<forge>/.forge/agents/<slug>/agent.toml`. The manifest carries:
//!
//! - `[agent]` — name, optional version + description, optional base
//!   `archetype` (one of [`crate::archetypes`]'s built-in ids).
//! - `[execution]` — best-effort limits on rounds / tokens / wall clock.
//! - `[tools]` — allow / deny lists scoped against
//!   [`crate::AgentToolRegistry`].
//! - `[memory]` — storage backend + retention policy (consumed by
//!   DG-33's memory layer; the parser captures the values verbatim).
//! - `[system_prompt]` — domain-tuned prompt that overrides (or
//!   layers on top of) the archetype's baseline.
//!
//! The parser is intentionally lenient about extra top-level tables
//! but strict about *typed fields it does know about*: an unknown key
//! inside `[execution]` fails the load (rather than silently ignoring
//! it) so a misspelled `max_steps` doesn't quietly fall through.
//!
//! Loading is a pure read — no kernel context required, no side
//! effects. Wired by the `com.nexus.agent::list_custom` IPC handler
//! and tested standalone in this module.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// File name the loader scans for inside each agent directory.
pub const MANIFEST_FILE_NAME: &str = "agent.toml";

/// Path fragment relative to `<forge>` where custom-agent manifests
/// live. Matches PRD-15 §9 ("`.forge/agents/<slug>/`").
pub const AGENTS_DIR: &str = ".forge/agents";

/// A parsed `agent.toml` manifest.
///
/// `slug` is the manifest's enclosing directory name (e.g. `code-quality`
/// for `.forge/agents/code-quality/agent.toml`) — the loader fills it in
/// so the agent has a stable id even if `[agent].name` collides with
/// another manifest. `name` (from the TOML) remains the display name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct CustomAgentManifest {
    /// Directory slug — `.forge/agents/<slug>/agent.toml`. Not present
    /// in the TOML; filled in by the loader.
    pub slug: String,
    /// Required `[agent]` block.
    pub agent: AgentSection,
    /// Optional `[execution]` block (defaults apply when absent).
    #[serde(default)]
    pub execution: ExecutionSection,
    /// Optional `[tools]` block (defaults to allow-all).
    #[serde(default)]
    pub tools: ToolsSection,
    /// Optional `[memory]` block (defaults to filesystem + 90 days).
    #[serde(default)]
    pub memory: MemorySection,
    /// Required `[system_prompt]` block.
    pub system_prompt: SystemPromptSection,
}

/// `[agent]` — identity + optional base archetype.
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
pub struct AgentSection {
    /// Display name.
    pub name: String,
    /// Optional semver-ish version.
    #[serde(default)]
    pub version: Option<String>,
    /// Optional one-line description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional base archetype id — one of the short names returned
    /// by `com.nexus.agent::list_archetypes` (`writer`, `coder`,
    /// `researcher`, …). When present, the custom agent's system
    /// prompt is layered on top of the archetype baseline.
    #[serde(default)]
    pub archetype: Option<String>,
}

/// `[execution]` — best-effort limits.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSection {
    /// Hard cap on planning rounds (default unset — session loop
    /// applies `MAX_AGENT_ROUNDS`).
    #[serde(default)]
    pub max_steps: Option<u32>,
    /// Soft token budget; surface only — enforcement is the planner's
    /// responsibility.
    #[serde(default)]
    pub token_budget: Option<u64>,
    /// Wall-clock cap in seconds.
    #[serde(default)]
    pub time_limit_secs: Option<u64>,
    /// Tool names the session should always pause on for explicit
    /// user approval. Overrides the registry's `requires_approval`
    /// flag for these tools (additive, never subtractive).
    #[serde(default)]
    pub requires_approval_for: Vec<String>,
}

/// `[tools]` — allow / deny lists. Empty `allowed` means *any* tool
/// the agent's capabilities satisfy is reachable; `denied` always
/// subtracts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ToolsSection {
    /// Tool names the agent may use. Empty = no allowlist filter.
    /// Each entry must match a tool registered in
    /// [`crate::AgentToolRegistry`].
    #[serde(default)]
    pub allowed: Vec<String>,
    /// Tool names the agent must not call. Applied after `allowed`.
    #[serde(default)]
    pub denied: Vec<String>,
}

/// DG-36 follow-up — runtime check derived from a manifest's
/// `[tools]` section. Built once per session and consulted before
/// each tool dispatch (see `ToolPolicyDispatcher` in
/// `crate::core_plugin`).
///
/// **Decision rules** (in order):
/// 1. If the tool name is in `denied` → reject.
/// 2. If `allowed` is non-empty and the tool name is NOT in `allowed` → reject.
/// 3. Otherwise accept.
///
/// Empty `allowed` means "no allow-list filter" (every tool is fair
/// game subject only to the deny list). This matches the spec
/// comment on [`ToolsSection::allowed`].
#[derive(Debug, Clone, Default)]
pub struct ManifestToolPolicy {
    allowed: std::collections::HashSet<String>,
    denied: std::collections::HashSet<String>,
    /// Slug of the contributing manifest — used to build a clear
    /// rejection message ("denied by `<slug>`'s [tools] manifest")
    /// without forcing the caller to thread the slug separately.
    slug: String,
}

impl ManifestToolPolicy {
    /// Build a policy from a parsed [`CustomAgentManifest`]. Cheap —
    /// just clones the two lists into `HashSet`s and copies the slug.
    #[must_use]
    pub fn from_manifest(manifest: &CustomAgentManifest) -> Self {
        Self {
            allowed: manifest.tools.allowed.iter().cloned().collect(),
            denied: manifest.tools.denied.iter().cloned().collect(),
            slug: manifest.slug.clone(),
        }
    }

    /// `Ok(())` when `tool_name` is permitted; `Err(reason)` otherwise.
    /// The error string is the message the session loop surfaces in
    /// the rejected `ToolCallRecord` so the model sees *why* its call
    /// failed and can adjust.
    ///
    /// # Errors
    /// Returns a human-readable rejection string keyed by the
    /// contributing slug so a transcript review can trace the policy
    /// origin back to the manifest.
    pub fn check(&self, tool_name: &str) -> Result<(), String> {
        if self.denied.contains(tool_name) {
            return Err(format!(
                "tool '{tool_name}' denied by '{slug}' [tools].denied",
                slug = self.slug,
            ));
        }
        if !self.allowed.is_empty() && !self.allowed.contains(tool_name) {
            return Err(format!(
                "tool '{tool_name}' not in '{slug}' [tools].allowed list",
                slug = self.slug,
            ));
        }
        Ok(())
    }

    /// `true` when both lists are empty — the policy is a no-op and
    /// the caller can skip wrapping the dispatcher altogether. Tiny
    /// optimisation; mainly here so the wrap site has a clear "skip
    /// the indirection" branch.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.allowed.is_empty() && self.denied.is_empty()
    }
}

/// DG-36 follow-up — `SessionPolicy` decorator that filters proposed
/// tool calls against a custom agent's manifest `[tools]` policy
/// *before* delegating to the inner policy (the existing
/// `AutoApproveAll` / `BusBridgePolicy` / etc.).
///
/// **Why decorate the policy rather than the dispatcher.** The
/// session loop already supports per-call denials through
/// `RoundDecision::Partial(Vec<RoundDecisionEntry>)`; a denied call
/// feeds back as an `is_error: true` tool-result turn so the model
/// can recover. Routing manifest denials through the same surface
/// keeps the failure semantics consistent and avoids growing a
/// parallel "denied at dispatch time" path.
///
/// **Merge rules** (when the manifest policy contributes denials):
/// - If the manifest denies every call, return
///   `RoundDecision::Partial(all-denied)`; we don't ask the inner
///   policy.
/// - Otherwise consult the inner policy on the full round:
///   - `ApproveAll` → emit `Partial` where every approved call rides
///     through and every manifest-denied call carries the manifest's
///     reason.
///   - `Partial(inner_entries)` → manifest denials override
///     inner approvals on the same `tool_use_id`; otherwise the
///     inner entry stands.
///   - `Abort` / `Timeout` → propagated unchanged (the round failed
///     for a reason orthogonal to manifest filtering).
///
/// **Manifest policy of [`ManifestToolPolicy::is_noop`] is a
/// pass-through** — the decorator delegates straight to the inner
/// policy. Construct only when at least one of `allowed` / `denied`
/// is non-empty.
pub struct ManifestPolicyGate<P> {
    inner: P,
    policy: ManifestToolPolicy,
}

impl<P> ManifestPolicyGate<P> {
    /// Wrap `inner` with `policy`. The wrapper is itself a
    /// [`crate::SessionPolicy`].
    pub fn new(inner: P, policy: ManifestToolPolicy) -> Self {
        Self { inner, policy }
    }
}

#[async_trait::async_trait]
impl<P> crate::SessionPolicy for ManifestPolicyGate<P>
where
    P: crate::SessionPolicy,
{
    async fn allow_round(&self, round: &crate::ProposedRound) -> crate::RoundDecision {
        // Manifest denials, indexed by tool_use_id.
        let manifest_denials: Vec<crate::RoundDecisionEntry> = round
            .tool_calls
            .iter()
            .filter_map(|p| match self.policy.check(&p.name) {
                Ok(()) => None,
                Err(reason) => Some(crate::RoundDecisionEntry {
                    tool_use_id: p.id.clone(),
                    approve: false,
                    reason,
                }),
            })
            .collect();
        if manifest_denials.is_empty() {
            return self.inner.allow_round(round).await;
        }
        if manifest_denials.len() == round.tool_calls.len() {
            // Every call rejected by the manifest. Skip the inner
            // policy — there's nothing left for it to approve.
            return crate::RoundDecision::Partial(manifest_denials);
        }
        // Mixed: consult the inner policy, then merge.
        let denial_ids: std::collections::HashSet<String> = manifest_denials
            .iter()
            .map(|e| e.tool_use_id.clone())
            .collect();
        match self.inner.allow_round(round).await {
            crate::RoundDecision::ApproveAll => {
                let mut entries = manifest_denials;
                for p in &round.tool_calls {
                    if !denial_ids.contains(&p.id) {
                        entries.push(crate::RoundDecisionEntry {
                            tool_use_id: p.id.clone(),
                            approve: true,
                            reason: String::new(),
                        });
                    }
                }
                crate::RoundDecision::Partial(entries)
            }
            crate::RoundDecision::Partial(inner_entries) => {
                let mut entries = manifest_denials;
                for entry in inner_entries {
                    if !denial_ids.contains(&entry.tool_use_id) {
                        entries.push(entry);
                    }
                }
                crate::RoundDecision::Partial(entries)
            }
            other => other,
        }
    }
}

/// `[memory]` — storage policy. Consumed by DG-33's memory layer;
/// the parser captures the values without acting on them.
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
pub struct MemorySection {
    /// `"filesystem"` (default) or `"database"`. Validated at parse
    /// time so a typo doesn't silently default to filesystem.
    #[serde(default = "default_storage")]
    pub storage: String,
    /// Days to retain conversation history. PRD-15 §5 spec'd 90.
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    /// Cap on stored memory entries before pruning kicks in.
    #[serde(default = "default_max_entries")]
    pub max_entries: u32,
}

impl Default for MemorySection {
    fn default() -> Self {
        Self {
            storage: default_storage(),
            retention_days: default_retention_days(),
            max_entries: default_max_entries(),
        }
    }
}

fn default_storage() -> String {
    "filesystem".to_string()
}

const fn default_retention_days() -> u32 {
    90
}

const fn default_max_entries() -> u32 {
    1000
}

/// `[system_prompt]` — the prompt body. PRD-15 §9 shows it as a
/// triple-quoted `text` field; the parser accepts either inline or
/// a `path` pointing at a sibling file (for prompts long enough to
/// deserve their own file).
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
pub struct SystemPromptSection {
    /// Inline prompt text. Mutually exclusive with `path`.
    #[serde(default)]
    pub text: Option<String>,
    /// Filesystem path relative to the manifest's enclosing
    /// directory. Used for long prompts. Mutually exclusive with
    /// `text`.
    #[serde(default)]
    pub path: Option<String>,
}

/// Errors surfaced by the manifest parser / loader.
#[derive(Debug, Error)]
pub enum CustomAgentError {
    /// `agent.toml` is malformed TOML.
    #[error("invalid TOML in {path}: {source}")]
    Toml {
        /// Path to the manifest.
        path: PathBuf,
        /// Underlying parse error.
        #[source]
        source: toml::de::Error,
    },
    /// File I/O failure (missing file, permissions, …).
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// `[system_prompt].text` and `[system_prompt].path` are both
    /// missing, or both set.
    #[error("invalid system_prompt in {path}: {reason}")]
    SystemPrompt {
        /// Manifest path.
        path: PathBuf,
        /// Specific reason.
        reason: String,
    },
    /// `[memory].storage` is something other than `"filesystem"` /
    /// `"database"`.
    #[error("invalid memory.storage in {path}: '{value}' (expected 'filesystem' or 'database')")]
    InvalidMemoryStorage {
        /// Manifest path.
        path: PathBuf,
        /// The invalid value supplied by the user.
        value: String,
    },
}

/// Parse a single `agent.toml` from a path.
///
/// `slug` is the manifest's enclosing directory name (used to
/// populate [`CustomAgentManifest::slug`] and to resolve a
/// `[system_prompt].path` relative to the manifest).
///
/// # Errors
/// See [`CustomAgentError`].
pub fn load_from_path(path: &Path) -> Result<CustomAgentManifest, CustomAgentError> {
    let slug = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let body = std::fs::read_to_string(path).map_err(|source| CustomAgentError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_str(&body, &slug, path)
}

/// Parse from an in-memory string. Useful for tests and IPC payloads
/// that don't go through disk.
///
/// `slug` populates the returned manifest's `slug` field; `path` is
/// only used to surface paths in error messages.
///
/// # Errors
/// See [`CustomAgentError`].
pub fn parse_str(
    body: &str,
    slug: &str,
    path: &Path,
) -> Result<CustomAgentManifest, CustomAgentError> {
    // `toml::from_str` returns a value with `slug` stripped (the
    // field isn't present in the TOML); we patch it in below.
    #[derive(Deserialize)]
    struct Raw {
        agent: AgentSection,
        #[serde(default)]
        execution: ExecutionSection,
        #[serde(default)]
        tools: ToolsSection,
        #[serde(default)]
        memory: MemorySection,
        system_prompt: SystemPromptSection,
    }
    let raw: Raw = toml::from_str(body).map_err(|source| CustomAgentError::Toml {
        path: path.to_path_buf(),
        source,
    })?;

    // Validate system_prompt exclusivity / presence.
    match (
        raw.system_prompt.text.as_deref().map(str::trim),
        raw.system_prompt.path.as_deref().map(str::trim),
    ) {
        (None, None) | (Some(""), None) | (None, Some("")) | (Some(""), Some("")) => {
            return Err(CustomAgentError::SystemPrompt {
                path: path.to_path_buf(),
                reason: "exactly one of `text` or `path` is required".to_string(),
            });
        }
        (Some(t), Some(p)) if !t.is_empty() && !p.is_empty() => {
            return Err(CustomAgentError::SystemPrompt {
                path: path.to_path_buf(),
                reason: "`text` and `path` are mutually exclusive".to_string(),
            });
        }
        _ => {}
    }

    // Validate memory storage value.
    if raw.memory.storage != "filesystem" && raw.memory.storage != "database" {
        return Err(CustomAgentError::InvalidMemoryStorage {
            path: path.to_path_buf(),
            value: raw.memory.storage,
        });
    }

    Ok(CustomAgentManifest {
        slug: slug.to_string(),
        agent: raw.agent,
        execution: raw.execution,
        tools: raw.tools,
        memory: raw.memory,
        system_prompt: raw.system_prompt,
    })
}

/// Resolve the system prompt body, reading
/// [`SystemPromptSection::path`] from disk when needed. The path is
/// resolved relative to the manifest's enclosing directory.
///
/// # Errors
/// Returns [`CustomAgentError::Io`] when the referenced file can't
/// be read.
pub fn resolve_system_prompt(
    manifest: &CustomAgentManifest,
    manifest_dir: &Path,
) -> Result<String, CustomAgentError> {
    if let Some(text) = &manifest.system_prompt.text {
        return Ok(text.clone());
    }
    if let Some(rel) = &manifest.system_prompt.path {
        let full = manifest_dir.join(rel);
        return std::fs::read_to_string(&full)
            .map_err(|source| CustomAgentError::Io { path: full, source });
    }
    // Should be unreachable thanks to the parse-time check, but keep
    // a clear error rather than panicking.
    Err(CustomAgentError::SystemPrompt {
        path: manifest_dir.join(MANIFEST_FILE_NAME),
        reason: "neither `text` nor `path` set after parse".to_string(),
    })
}

/// Scan `<forge_root>/.forge/agents/*/agent.toml` and return every
/// manifest that parses cleanly. Per-manifest errors are returned in
/// the second element of the tuple so a single broken manifest
/// doesn't poison the rest of the scan — callers can render the
/// errors next to the manifests that loaded.
///
/// Symlinks are followed once (matching `std::fs::read_dir`'s default);
/// nested subdirectories under `<slug>/` are ignored — only
/// `<slug>/agent.toml` counts.
pub fn scan_forge(
    forge_root: &Path,
) -> (Vec<CustomAgentManifest>, Vec<(PathBuf, CustomAgentError)>) {
    let agents_dir = forge_root.join(AGENTS_DIR);
    let mut manifests = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(e) => e,
        // Directory missing is a silent miss — most forges won't
        // have a custom-agents directory until a user creates one.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (manifests, errors),
        Err(source) => {
            errors.push((
                agents_dir.clone(),
                CustomAgentError::Io {
                    path: agents_dir,
                    source,
                },
            ));
            return (manifests, errors);
        }
    };

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest_path = dir.join(MANIFEST_FILE_NAME);
        if !manifest_path.exists() {
            continue;
        }
        match load_from_path(&manifest_path) {
            Ok(m) => manifests.push(m),
            Err(e) => errors.push((manifest_path, e)),
        }
    }

    // Deterministic ordering for downstream callers (CLI render,
    // IPC reply diffability).
    manifests.sort_by(|a, b| a.slug.cmp(&b.slug));
    errors.sort_by(|a, b| a.0.cmp(&b.0));

    (manifests, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_path() -> PathBuf {
        PathBuf::from("/forge/.forge/agents/code-quality/agent.toml")
    }

    const MIN_VALID: &str = r#"
[agent]
name = "Code Quality"

[system_prompt]
text = "You are a reviewer."
"#;

    #[test]
    fn parses_minimal_manifest() {
        let m = parse_str(MIN_VALID, "code-quality", &fake_path()).expect("parse");
        assert_eq!(m.slug, "code-quality");
        assert_eq!(m.agent.name, "Code Quality");
        assert_eq!(m.system_prompt.text.as_deref(), Some("You are a reviewer."));
        // Defaults populated.
        assert_eq!(m.memory.storage, "filesystem");
        assert_eq!(m.memory.retention_days, 90);
        assert_eq!(m.memory.max_entries, 1000);
        assert!(m.tools.allowed.is_empty());
    }

    #[test]
    fn parses_full_manifest_from_prd_example() {
        let body = r#"
[agent]
name = "MyCustomAgent"
version = "1.0.0"
description = "Analyze code quality and suggest refactorings."
archetype = "coder"

[execution]
max_steps = 50
token_budget = 10000
time_limit_secs = 300
requires_approval_for = ["write_file", "terminal_run_saved"]

[tools]
allowed = ["read_file", "search_forge", "git_log"]
denied = ["write_file"]

[memory]
storage = "database"
retention_days = 30
max_entries = 500

[system_prompt]
text = """
You are a code quality analyst.
"""
"#;
        let m = parse_str(body, "my-custom-agent", &fake_path()).expect("parse");
        assert_eq!(m.agent.version.as_deref(), Some("1.0.0"));
        assert_eq!(m.agent.archetype.as_deref(), Some("coder"));
        assert_eq!(m.execution.max_steps, Some(50));
        assert_eq!(m.execution.token_budget, Some(10_000));
        assert_eq!(m.execution.time_limit_secs, Some(300));
        assert_eq!(m.execution.requires_approval_for.len(), 2);
        assert_eq!(m.tools.allowed.len(), 3);
        assert_eq!(m.tools.denied, vec!["write_file"]);
        assert_eq!(m.memory.storage, "database");
        assert_eq!(m.memory.retention_days, 30);
        assert!(m
            .system_prompt
            .text
            .as_deref()
            .unwrap()
            .contains("code quality analyst"));
    }

    #[test]
    fn rejects_missing_system_prompt() {
        let body = r#"
[agent]
name = "Foo"

[system_prompt]
"#;
        let err = parse_str(body, "foo", &fake_path()).expect_err("should reject");
        match err {
            CustomAgentError::SystemPrompt { reason, .. } => {
                assert!(reason.contains("required"));
            }
            other => panic!("expected SystemPrompt error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_both_text_and_path_in_system_prompt() {
        let body = r#"
[agent]
name = "Foo"

[system_prompt]
text = "inline"
path = "prompt.md"
"#;
        let err = parse_str(body, "foo", &fake_path()).expect_err("should reject");
        match err {
            CustomAgentError::SystemPrompt { reason, .. } => {
                assert!(reason.contains("mutually exclusive"));
            }
            other => panic!("expected SystemPrompt error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_memory_storage() {
        let body = r#"
[agent]
name = "Foo"

[system_prompt]
text = "ok"

[memory]
storage = "redis"
"#;
        let err = parse_str(body, "foo", &fake_path()).expect_err("should reject");
        match err {
            CustomAgentError::InvalidMemoryStorage { value, .. } => {
                assert_eq!(value, "redis");
            }
            other => panic!("expected InvalidMemoryStorage, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_field_in_execution() {
        let body = r#"
[agent]
name = "Foo"

[execution]
max_stepz = 10

[system_prompt]
text = "ok"
"#;
        let err = parse_str(body, "foo", &fake_path()).expect_err("should reject");
        match err {
            CustomAgentError::Toml { .. } => {}
            other => panic!("expected Toml error for unknown field, got {other:?}"),
        }
    }

    #[test]
    fn scan_forge_handles_missing_directory_silently() {
        let tmp =
            std::env::temp_dir().join(format!("nexus-agent-scan-missing-{}", std::process::id()));
        let (manifests, errors) = scan_forge(&tmp);
        assert!(manifests.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn scan_forge_loads_multiple_and_sorts() {
        let tmp = std::env::temp_dir().join(format!(
            "nexus-agent-scan-multi-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(tmp.join(".forge/agents/alpha")).unwrap();
        std::fs::create_dir_all(tmp.join(".forge/agents/beta")).unwrap();
        std::fs::write(tmp.join(".forge/agents/alpha/agent.toml"), MIN_VALID).unwrap();
        std::fs::write(
            tmp.join(".forge/agents/beta/agent.toml"),
            r#"
[agent]
name = "Beta"

[system_prompt]
text = "second"
"#,
        )
        .unwrap();
        let (manifests, errors) = scan_forge(&tmp);
        assert_eq!(errors.len(), 0);
        assert_eq!(manifests.len(), 2);
        // Sorted by slug.
        assert_eq!(manifests[0].slug, "alpha");
        assert_eq!(manifests[1].slug, "beta");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_forge_isolates_broken_manifests_into_error_list() {
        let tmp = std::env::temp_dir().join(format!(
            "nexus-agent-scan-broken-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(tmp.join(".forge/agents/good")).unwrap();
        std::fs::create_dir_all(tmp.join(".forge/agents/bad")).unwrap();
        std::fs::write(tmp.join(".forge/agents/good/agent.toml"), MIN_VALID).unwrap();
        // Missing [system_prompt] block — should error.
        std::fs::write(
            tmp.join(".forge/agents/bad/agent.toml"),
            r#"
[agent]
name = "Bad"
"#,
        )
        .unwrap();
        let (manifests, errors) = scan_forge(&tmp);
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].slug, "good");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].0.ends_with("bad/agent.toml"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_system_prompt_reads_inline_text() {
        let m = parse_str(MIN_VALID, "code-quality", &fake_path()).unwrap();
        let prompt = resolve_system_prompt(&m, Path::new("/anywhere")).unwrap();
        assert_eq!(prompt, "You are a reviewer.");
    }

    #[test]
    fn resolve_system_prompt_reads_file_when_path_set() {
        let tmp = std::env::temp_dir().join(format!(
            "nexus-agent-prompt-file-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("prompt.md"), "loaded from file").unwrap();
        let body = r#"
[agent]
name = "Foo"

[system_prompt]
path = "prompt.md"
"#;
        let m = parse_str(body, "foo", &tmp.join("agent.toml")).unwrap();
        let prompt = resolve_system_prompt(&m, &tmp).unwrap();
        assert_eq!(prompt, "loaded from file");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── DG-36 follow-up — ManifestToolPolicy + ManifestPolicyGate ──────────────

    fn manifest_with_tools(allowed: &[&str], denied: &[&str]) -> CustomAgentManifest {
        let body = format!(
            r#"
[agent]
name = "Foo"

[tools]
allowed = [{allowed}]
denied = [{denied}]

[system_prompt]
text = "p"
"#,
            allowed = allowed
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", "),
            denied = denied
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", "),
        );
        parse_str(&body, "foo", Path::new("/anywhere/agent.toml")).unwrap()
    }

    #[test]
    fn policy_empty_lists_is_noop() {
        let m = manifest_with_tools(&[], &[]);
        let p = ManifestToolPolicy::from_manifest(&m);
        assert!(p.is_noop());
        assert!(p.check("read_file").is_ok());
    }

    #[test]
    fn policy_denied_takes_precedence_over_allowed() {
        let m = manifest_with_tools(&["read_file", "write_file"], &["write_file"]);
        let p = ManifestToolPolicy::from_manifest(&m);
        assert!(!p.is_noop());
        assert!(p.check("read_file").is_ok());
        let err = p.check("write_file").unwrap_err();
        assert!(err.contains("denied"));
        assert!(err.contains("foo"));
        assert!(err.contains("write_file"));
    }

    #[test]
    fn policy_allow_list_narrows_when_non_empty() {
        let m = manifest_with_tools(&["read_file"], &[]);
        let p = ManifestToolPolicy::from_manifest(&m);
        assert!(p.check("read_file").is_ok());
        let err = p.check("write_file").unwrap_err();
        assert!(err.contains("not in"));
        assert!(err.contains("[tools].allowed"));
    }

    #[test]
    fn policy_empty_allow_means_no_filter() {
        let m = manifest_with_tools(&[], &["write_file"]);
        let p = ManifestToolPolicy::from_manifest(&m);
        // Read passes (no allow filter); write denied.
        assert!(p.check("read_file").is_ok());
        assert!(p.check("write_file").is_err());
    }

    // ── ManifestPolicyGate decision matrix ─────────────────────────────────────

    use crate::session::{ProposedRound, RoundDecision, RoundDecisionEntry};
    use crate::SessionPolicy;
    use crate::ToolCall;
    use async_trait::async_trait;

    fn proposed(id: &str, name: &str) -> crate::llm::ProposedToolCall {
        crate::llm::ProposedToolCall {
            id: id.to_string(),
            name: name.to_string(),
            tool_call: ToolCall {
                target_plugin_id: "com.nexus.storage".to_string(),
                command_id: name.to_string(),
                args: serde_json::json!({}),
            },
        }
    }

    fn round_of(calls: Vec<(&str, &str)>) -> ProposedRound {
        ProposedRound {
            round: 1,
            text: String::new(),
            tool_calls: calls.into_iter().map(|(id, n)| proposed(id, n)).collect(),
        }
    }

    struct ApproveAllInner;
    #[async_trait]
    impl SessionPolicy for ApproveAllInner {
        async fn allow_round(&self, _round: &ProposedRound) -> RoundDecision {
            RoundDecision::ApproveAll
        }
    }

    struct AbortInner;
    #[async_trait]
    impl SessionPolicy for AbortInner {
        async fn allow_round(&self, _round: &ProposedRound) -> RoundDecision {
            RoundDecision::Abort("inner abort".into())
        }
    }

    struct PartialDenyInner {
        deny_id: String,
    }
    #[async_trait]
    impl SessionPolicy for PartialDenyInner {
        async fn allow_round(&self, round: &ProposedRound) -> RoundDecision {
            RoundDecision::Partial(
                round
                    .tool_calls
                    .iter()
                    .map(|p| RoundDecisionEntry {
                        tool_use_id: p.id.clone(),
                        approve: p.id != self.deny_id,
                        reason: if p.id == self.deny_id {
                            "inner reason".to_string()
                        } else {
                            String::new()
                        },
                    })
                    .collect(),
            )
        }
    }

    #[tokio::test]
    async fn gate_passes_through_when_no_manifest_denials() {
        let m = manifest_with_tools(&[], &["forbidden"]);
        let policy = ManifestToolPolicy::from_manifest(&m);
        let gate = ManifestPolicyGate::new(ApproveAllInner, policy);
        let r = round_of(vec![("a", "read_file"), ("b", "write_file")]);
        match gate.allow_round(&r).await {
            RoundDecision::ApproveAll => {}
            other => panic!("expected ApproveAll passthrough, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn gate_returns_full_partial_when_every_call_denied() {
        let m = manifest_with_tools(&[], &["read_file", "write_file"]);
        let policy = ManifestToolPolicy::from_manifest(&m);
        let gate = ManifestPolicyGate::new(ApproveAllInner, policy);
        let r = round_of(vec![("a", "read_file"), ("b", "write_file")]);
        match gate.allow_round(&r).await {
            RoundDecision::Partial(entries) => {
                assert_eq!(entries.len(), 2);
                assert!(entries.iter().all(|e| !e.approve));
                assert!(entries.iter().all(|e| e.reason.contains("denied")));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn gate_layers_manifest_denials_on_top_of_inner_approve_all() {
        let m = manifest_with_tools(&[], &["write_file"]);
        let policy = ManifestToolPolicy::from_manifest(&m);
        let gate = ManifestPolicyGate::new(ApproveAllInner, policy);
        let r = round_of(vec![
            ("a", "read_file"),
            ("b", "write_file"),
            ("c", "list_backlinks"),
        ]);
        match gate.allow_round(&r).await {
            RoundDecision::Partial(entries) => {
                let by_id: std::collections::HashMap<_, _> =
                    entries.iter().map(|e| (e.tool_use_id.clone(), e)).collect();
                assert!(!by_id["b"].approve, "write_file should be denied");
                assert!(by_id["a"].approve, "read_file should pass");
                assert!(by_id["c"].approve, "list_backlinks should pass");
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn gate_manifest_overrides_inner_partial_approval() {
        // Inner approves everything but call "b"; manifest denies
        // call "a". Expected: a denied by manifest reason; b denied
        // by inner reason; c approved.
        let m = manifest_with_tools(&[], &["read_file"]);
        let policy = ManifestToolPolicy::from_manifest(&m);
        let inner = PartialDenyInner {
            deny_id: "b".into(),
        };
        let gate = ManifestPolicyGate::new(inner, policy);
        let r = round_of(vec![
            ("a", "read_file"),
            ("b", "write_file"),
            ("c", "list_backlinks"),
        ]);
        match gate.allow_round(&r).await {
            RoundDecision::Partial(entries) => {
                let by_id: std::collections::HashMap<_, _> =
                    entries.iter().map(|e| (e.tool_use_id.clone(), e)).collect();
                assert!(!by_id["a"].approve);
                assert!(by_id["a"].reason.contains("denied"));
                assert!(!by_id["b"].approve);
                assert_eq!(by_id["b"].reason, "inner reason");
                assert!(by_id["c"].approve);
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn gate_propagates_inner_abort_when_no_manifest_denial() {
        // No manifest denials → delegate fully to inner, which aborts.
        let m = manifest_with_tools(&[], &["nothing-matches"]);
        let policy = ManifestToolPolicy::from_manifest(&m);
        let gate = ManifestPolicyGate::new(AbortInner, policy);
        let r = round_of(vec![("a", "read_file")]);
        match gate.allow_round(&r).await {
            RoundDecision::Abort(reason) => assert_eq!(reason, "inner abort"),
            other => panic!("expected Abort, got {other:?}"),
        }
    }
}
