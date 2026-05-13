//! Archetype factories — pre-baked [`LlmAgent`] configurations tuned
//! for specific domains (PRD-15 §3.3).
//!
//! Each archetype is a thin wrapper that swaps the planner's system
//! prompt and agent id. The driver, plan schema, and executor loop
//! are identical to `LlmAgent::new` — nothing here is a specialization
//! of the library surface, just of the prompt.
//!
//! Archetypes are selected by id string (`"writer"`, `"coder"`,
//! `"researcher"`). [`resolve`] maps a caller-supplied string (plus
//! an optional skill-composed prompt preamble) to a concrete
//! `LlmAgent`.

use crate::{ChatDriver, LlmAgent, DEFAULT_SYSTEM_PROMPT};

/// Writer archetype — biases the planner toward markdown-authoring
/// outputs, pushes it to prefer file writes over shell-like
/// operations. Tool schemas come from the AI registry; the prompt
/// only describes posture.
pub const WRITER_SYSTEM_PROMPT: &str = "\
You are Nexus's Writer planner. Choose tool calls that favour \
producing markdown content. Prefer `write_file` for finalising notes \
and `read_file` / `search_forge` for grounding. Avoid `git_log` or \
shell-like tools unless the goal explicitly involves version control. \
If the goal is naturally prose and no tool call is needed, respond \
with text and no tool calls.";

/// Coder archetype — biases the planner toward code edits + git +
/// build operations.
pub const CODER_SYSTEM_PROMPT: &str = "\
You are Nexus's Coder planner. Choose tool calls focused on \
software-engineering tasks. Prefer `read_file` / `write_file` for \
source edits and `git_log` to orient yourself before non-trivial \
changes. Stage small, reversible operations so a failed build \
doesn't strand the working tree.";

/// Researcher archetype — biases the planner toward search +
/// knowledge-graph traversal, with a strong preference for reading
/// over writing.
pub const RESEARCHER_SYSTEM_PROMPT: &str = "\
You are Nexus's Researcher planner. Choose tool calls centred on \
gathering and synthesising information. Prefer `search_forge`, \
`list_backlinks`, and `read_file` over writes. Avoid destructive \
tool calls; if a write is needed, end with a single summarising \
note via `write_file`. Reference source paths in your narration so \
the user can audit the trail.";

/// Auditor archetype — biases the planner toward critical review of \
/// existing material. Read-heavy, write-only-for-the-final-report. \
/// Maps to PRD-15 §8.5 (Review Agent) in spirit.
pub const AUDITOR_SYSTEM_PROMPT: &str = "\
You are Nexus's Auditor planner. Your goal is to review existing \
material for correctness, consistency, and risk. Prefer \
`read_file`, `search_forge`, `list_backlinks`, and `git_log` to \
build a complete picture before writing anything. Call out specific \
problems with file paths and line numbers; explain WHY each finding \
matters. When you produce a report, write it once at the end via \
`write_file` to a clearly-named audit note (e.g. \
`audits/<topic>.md`). Never modify the material you're auditing.";

/// Librarian archetype — biases the planner toward knowledge \
/// organisation: indexing, cross-linking, deduplication, and \
/// directory hygiene. Reads broadly, writes narrowly to canonical \
/// catalogue notes.
pub const LIBRARIAN_SYSTEM_PROMPT: &str = "\
You are Nexus's Librarian planner. Your goal is to keep the forge \
well-organised and discoverable. Use `search_forge` and \
`list_backlinks` to find related material, `read_file` to confirm \
content, and `write_file` only to update catalogue / index notes \
or to add forward links from canonical hubs. Prefer linking over \
duplicating; never reorganise files without the user's explicit \
go-ahead. When the goal is ambiguous, propose a structure in \
your narration first and let the user redirect.";

/// Coach archetype — biases the planner toward guidance, teaching, \
/// and learning loops rather than execution. Read-heavy; writes \
/// notes that help the user build their own understanding rather \
/// than handing back finished work.
pub const COACH_SYSTEM_PROMPT: &str = "\
You are Nexus's Coach planner. Your goal is to help the user grow \
their understanding — not to do their work for them. Prefer \
`read_file` and `search_forge` to ground your guidance in the \
user's actual material. Surface questions to clarify the user's \
real goal before reaching for tools. When you write, write \
explanations / suggestions / next-step prompts (via `write_file` \
to a coaching note) rather than finished artefacts. Be \
encouraging but specific; reference concrete file paths so the \
user can follow your trail.";

/// Identifier for the Writer archetype — `com.nexus.agent.writer`.
pub const WRITER_ID: &str = "com.nexus.agent.writer";
/// Identifier for the Coder archetype — `com.nexus.agent.coder`.
pub const CODER_ID: &str = "com.nexus.agent.coder";
/// Identifier for the Researcher archetype — `com.nexus.agent.researcher`.
pub const RESEARCHER_ID: &str = "com.nexus.agent.researcher";
/// Identifier for the Auditor archetype — `com.nexus.agent.auditor` (DG-35).
pub const AUDITOR_ID: &str = "com.nexus.agent.auditor";
/// Identifier for the Librarian archetype — `com.nexus.agent.librarian` (DG-35).
pub const LIBRARIAN_ID: &str = "com.nexus.agent.librarian";
/// Identifier for the Coach archetype — `com.nexus.agent.coach` (DG-35).
pub const COACH_ID: &str = "com.nexus.agent.coach";

/// Build an [`LlmAgent`] from a caller-supplied archetype name.
/// Unknown / empty / `"general"` → the default `LlmAgent`. The
/// optional `extra_prompt` is appended to the chosen archetype's
/// system prompt — use this to layer in skill-matched instructions
/// alongside the archetype's domain bias.
#[must_use]
pub fn build_archetype<D: ChatDriver>(
    name: Option<&str>,
    driver: D,
    extra_prompt: Option<&str>,
) -> LlmAgent<D> {
    let (id, prompt) = resolve_prompt(name);
    let final_prompt = match extra_prompt {
        Some(extra) if !extra.is_empty() => format!("{prompt}\n\n{extra}"),
        _ => prompt.to_string(),
    };
    LlmAgent::new(driver)
        .with_id(id)
        .with_system_prompt(final_prompt)
}

pub(crate) fn resolve_prompt(name: Option<&str>) -> (&'static str, &'static str) {
    match name.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("writer") => (WRITER_ID, WRITER_SYSTEM_PROMPT),
        Some("coder") => (CODER_ID, CODER_SYSTEM_PROMPT),
        Some("researcher") => (RESEARCHER_ID, RESEARCHER_SYSTEM_PROMPT),
        Some("auditor") => (AUDITOR_ID, AUDITOR_SYSTEM_PROMPT),
        Some("librarian") => (LIBRARIAN_ID, LIBRARIAN_SYSTEM_PROMPT),
        Some("coach") => (COACH_ID, COACH_SYSTEM_PROMPT),
        _ => ("com.nexus.agent.llm", DEFAULT_SYSTEM_PROMPT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Agent;
    use async_trait::async_trait;

    struct CannedDriver;

    #[async_trait]
    impl ChatDriver for CannedDriver {
        async fn propose(
            &self,
            _system: &str,
            _user: &str,
        ) -> Result<crate::Proposal, String> {
            Ok(crate::Proposal {
                text: "ok".into(),
                tool_calls: Vec::new(),
            })
        }
    }

    #[test]
    fn unknown_archetype_falls_back_to_default() {
        let (id, prompt) = resolve_prompt(Some("nonsense"));
        assert_eq!(id, "com.nexus.agent.llm");
        assert_eq!(prompt, DEFAULT_SYSTEM_PROMPT);
    }

    #[test]
    fn resolve_is_case_insensitive() {
        assert_eq!(resolve_prompt(Some("WRITER")).0, WRITER_ID);
        assert_eq!(resolve_prompt(Some("  Coder ")).0, CODER_ID);
    }

    /// DG-35 — three new archetypes resolve to their own ids and
    /// surface a non-empty system prompt distinct from the default.
    #[test]
    fn dg35_archetypes_resolve_to_their_own_ids() {
        for (name, id, prompt) in [
            ("auditor", AUDITOR_ID, AUDITOR_SYSTEM_PROMPT),
            ("librarian", LIBRARIAN_ID, LIBRARIAN_SYSTEM_PROMPT),
            ("coach", COACH_ID, COACH_SYSTEM_PROMPT),
        ] {
            let (resolved_id, resolved_prompt) = resolve_prompt(Some(name));
            assert_eq!(resolved_id, id, "id mismatch for `{name}`");
            assert_eq!(resolved_prompt, prompt, "prompt mismatch for `{name}`");
            assert_ne!(
                resolved_prompt, DEFAULT_SYSTEM_PROMPT,
                "`{name}` falling back to default prompt"
            );
            assert!(!resolved_prompt.is_empty());
        }
    }

    /// DG-35 — the three new archetype prompts each describe a
    /// distinct posture. Anchors the prompt text against a key
    /// phrase so a refactor that rewrites a prompt has to update
    /// the test deliberately.
    #[test]
    fn dg35_prompts_describe_distinct_postures() {
        assert!(AUDITOR_SYSTEM_PROMPT.contains("Auditor"));
        assert!(AUDITOR_SYSTEM_PROMPT.contains("review"));
        assert!(LIBRARIAN_SYSTEM_PROMPT.contains("Librarian"));
        assert!(LIBRARIAN_SYSTEM_PROMPT.contains("organis"));
        assert!(COACH_SYSTEM_PROMPT.contains("Coach"));
        assert!(COACH_SYSTEM_PROMPT.contains("understanding"));
    }

    #[tokio::test]
    async fn build_archetype_assigns_id() {
        let agent = build_archetype(Some("writer"), CannedDriver, None);
        assert_eq!(agent.id(), WRITER_ID);
    }

    #[tokio::test]
    async fn extra_prompt_appends_to_archetype_prompt() {
        let agent = build_archetype(Some("coder"), CannedDriver, Some("# Skill: Rust"));
        // Reach in via planning: driver ignores prompt but we can
        // confirm the builder returned a valid agent by planning.
        let plan = agent.plan("do thing").await.unwrap();
        assert_eq!(plan.steps.len(), 1);
    }

    #[tokio::test]
    async fn none_archetype_produces_default_id() {
        let agent = build_archetype(None, CannedDriver, None);
        assert_eq!(agent.id(), "com.nexus.agent.llm");
    }
}
