//! Skills subsystem scaffold (PRD-13).
//!
//! Skills are `.skill.md` files — YAML frontmatter + markdown body —
//! that encode reusable instruction templates the AI engine consumes
//! (inline AI, Chat panel, agents) to shape behavior for a domain.
//! This crate provides:
//!
//! - [`Skill`] / [`SkillParameter`] / [`SkillRestrictions`] — typed
//!   projection of the PRD-13 §2.3 frontmatter schema. Unknown
//!   fields round-trip through `extra` so future schema additions
//!   don't break older parsers.
//! - [`parse_skill_file`] — splits the `---` frontmatter block from
//!   the markdown body and decodes both.
//! - [`SkillRegistry`] — in-memory index built from a directory
//!   walk. Matches the `.forge/skills/` layout; sub-directories are
//!   recursed so `personal/` and `org/` share the same lookup
//!   surface.
//!
//! # What this is NOT (yet)
//!
//! - A core plugin. The `com.nexus.skills` dispatch surface will
//!   land once activation + composition land.
//! - Dependency resolution. `depends_on` is stored verbatim; the
//!   composer that stacks prompts ships with §5 in a follow-up.
//! - `REGISTRY.json` persistence (§3.1). The in-memory registry is
//!   authoritative today; a serialized index sits on top.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod core_plugin;
mod parse;
mod registry;
mod substitute;

pub use core_plugin::{
    SkillsCorePlugin, HANDLER_GET, HANDLER_LIST, HANDLER_LIST_BY_CONTEXT, HANDLER_RELOAD,
    HANDLER_RENDER, HANDLER_TRIGGERED_BY, PLUGIN_ID,
};
pub use parse::{parse_skill_file, parse_skill_text, SkillParseError};
pub use registry::{SkillRegistry, SkillRegistryError};
pub use substitute::{render, SubstitutionError};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A parsed `.skill.md` entry. Frontmatter is required; body is the
/// raw markdown after the closing `---` separator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    /// Frontmatter (typed subset).
    #[serde(flatten)]
    pub meta: SkillMeta,
    /// Everything after the frontmatter block, verbatim.
    #[serde(default)]
    pub body: String,
}

/// Typed projection of the PRD-13 §2.3 frontmatter schema. Unknown
/// keys go into [`SkillMeta::extra`] so the parser is forward-compat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillMeta {
    /// Human-readable skill name (`name`).
    pub name: String,
    /// Unique kebab-case identifier (`id`).
    pub id: String,
    /// One-to-two-sentence purpose (`description`).
    pub description: String,
    /// Semantic version string (`version`).
    pub version: String,
    /// Author or organization (`author`).
    pub author: String,
    /// ISO 8601 creation date (`created`).
    pub created: String,
    /// Category tags for discovery (`tags`).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Auto-activation contexts — `pull-request`, `terminal`,
    /// `editor`, `ai-chat`, `agent`.
    #[serde(default)]
    pub applicable_contexts: Vec<String>,
    /// Keyword or phrase triggers that auto-activate the skill.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// Typed input parameters the consumer can override.
    #[serde(default)]
    pub parameters: Vec<SkillParameter>,
    /// Other skill ids this skill layers on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Capability + tool restrictions.
    #[serde(default)]
    pub restrictions: Option<SkillRestrictions>,
    /// Output shape — `structured` / `markdown` / `natural` / `custom`.
    #[serde(default)]
    pub output_format: Option<String>,
    /// `public` (shareable) or `private` (default).
    #[serde(default)]
    pub visibility: Option<String>,
    /// Everything else in the frontmatter, preserved for forward
    /// compatibility. Consumers that need a not-yet-modeled field
    /// can reach into `extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

/// One entry under `parameters:` in the frontmatter. The PRD allows
/// enum / list / scalar parameters; this type carries each variant's
/// fields as optionals so a consumer can decide how strict it wants
/// to be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillParameter {
    /// Parameter name as used in prompt substitution.
    pub name: String,
    /// Rough type label: `"enum"`, `"list"`, `"string"`, `"number"`,
    /// `"boolean"`, or custom.
    #[serde(rename = "type")]
    pub param_type: String,
    /// Short sentence describing what the parameter controls.
    #[serde(default)]
    pub description: Option<String>,
    /// Allowed values when `param_type == "enum"`.
    #[serde(default)]
    pub values: Vec<serde_yaml::Value>,
    /// Element type when `param_type == "list"`.
    #[serde(default)]
    pub items: Option<String>,
    /// Default value (any YAML scalar or sequence).
    #[serde(default)]
    pub default: Option<serde_yaml::Value>,
}

/// Capability + tool restrictions (§2.2). Empty defaults mean
/// "unrestricted" — callers that want a safer posture pass
/// explicit `false` for each lever.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SkillRestrictions {
    /// Whether the skill may issue file-write tool calls.
    #[serde(default)]
    pub modify_files: Option<bool>,
    /// Whether the skill may delete content.
    #[serde(default)]
    pub delete_content: Option<bool>,
    /// Whether the skill may run arbitrary code.
    #[serde(default)]
    pub execute_code: Option<bool>,
    /// Allowlist of tool ids the skill may call. Empty means
    /// unconstrained.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}
