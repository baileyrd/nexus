//! BL-028f — built-in workflow templates library.
//!
//! Five starter templates ship embedded in the binary via
//! `include_str!`. Authors discover them through
//! `com.nexus.workflow::templates_list` and instantiate one into
//! their forge via `templates_init`, which writes the template body
//! to `<forge>/.workflows/<filename>.workflow.toml`.
//!
//! The catalog is a `&'static [Template]` so lookup is O(n) over a
//! tiny list — no allocation, no I/O. Each template is parsed at
//! load time only by the unit tests below to guarantee shipped
//! templates are valid TOML and pass the workflow validator.
//!
//! # Adding a template
//!
//! 1. Drop `<name>.workflow.toml` into `crates/nexus-workflow/templates/`.
//! 2. Add an entry to [`CATALOG`] with a slug, description, and the
//!    `include_str!` body.
//! 3. The `templates_each_entry_parses_and_validates` test will
//!    enforce shape on next `cargo test`.

use serde::Serialize;

use crate::Workflow;

/// One built-in template.
#[derive(Debug, Clone, Serialize)]
pub struct Template {
    /// Slug used by `templates_get` / `templates_init` (kebab-case).
    pub slug: &'static str,
    /// Short description sourced from the embedded TOML's
    /// `[workflow].description` — kept in sync at compile time.
    pub description: &'static str,
    /// Tags surfaced for filtering in UIs.
    pub tags: &'static [&'static str],
    /// Recommended on-disk filename when the template is instantiated.
    /// Always `<slug>.workflow.toml`.
    pub filename: &'static str,
    /// Embedded TOML body.
    pub body: &'static str,
}

/// Catalog of built-in templates. Order is preserved when listing.
pub const CATALOG: &[Template] = &[
    Template {
        slug: "daily-journal",
        description: "Create today's journal note from a cron schedule.",
        tags: &["journal", "cron", "starter"],
        filename: "daily-journal.workflow.toml",
        body: include_str!("../templates/daily-journal.workflow.toml"),
    },
    Template {
        slug: "commit-summary",
        description: "On every git commit, ask the AI to summarise the change and append it to a running log.",
        tags: &["git", "ai", "log"],
        filename: "commit-summary.workflow.toml",
        body: include_str!("../templates/commit-summary.workflow.toml"),
    },
    Template {
        slug: "note-classifier",
        description: "When a markdown note is saved, ask the AI to bucket it as journal, reference, or task.",
        tags: &["ai", "tagging", "file_event"],
        filename: "note-classifier.workflow.toml",
        body: include_str!("../templates/note-classifier.workflow.toml"),
    },
    Template {
        slug: "parallel-fetch",
        description: "Run two storage scans in parallel, then write a combined summary.",
        tags: &["parallel", "ipc", "starter"],
        filename: "parallel-fetch.workflow.toml",
        body: include_str!("../templates/parallel-fetch.workflow.toml"),
    },
    Template {
        slug: "research-prompt",
        description: "Manual workflow that asks the AI a question with RAG context and writes the answer to a note.",
        tags: &["ai", "manual", "rag"],
        filename: "research-prompt.workflow.toml",
        body: include_str!("../templates/research-prompt.workflow.toml"),
    },
];

/// Find a template by slug.
#[must_use]
pub fn find(slug: &str) -> Option<&'static Template> {
    CATALOG.iter().find(|t| t.slug == slug)
}

/// Parse the embedded body of `template` into a [`Workflow`]. Useful
/// for callers (and tests) that want the structured form rather than
/// the raw TOML.
///
/// # Errors
/// Returns the underlying parse error string when the embedded body
/// fails to parse — this should only happen in development, since
/// `templates_each_entry_parses_and_validates` enforces shape at test
/// time.
pub fn parse(template: &Template) -> Result<Workflow, String> {
    crate::parse_workflow_text(template.body).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_at_least_five_templates() {
        assert!(
            CATALOG.len() >= 5,
            "PRD-16 §11 requires at least 5 built-in templates; have {}",
            CATALOG.len()
        );
    }

    #[test]
    fn slugs_are_unique_and_kebab_case() {
        let mut seen = std::collections::HashSet::new();
        for t in CATALOG {
            assert!(
                t.slug.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "slug `{}` must be kebab-case ASCII",
                t.slug
            );
            assert!(seen.insert(t.slug), "duplicate slug `{}`", t.slug);
        }
    }

    #[test]
    fn filenames_match_slug_and_extension() {
        for t in CATALOG {
            assert_eq!(t.filename, format!("{}.workflow.toml", t.slug));
        }
    }

    #[test]
    fn each_entry_parses_and_validates() {
        // Every embedded body must round-trip through the workflow
        // parser without panicking — this is the shipped-template
        // shape contract.
        for t in CATALOG {
            let wf =
                parse(t).unwrap_or_else(|e| panic!("template `{}` failed to parse: {e}", t.slug));
            assert!(!wf.workflow.name.trim().is_empty());
            assert!(!wf.trigger.trigger_type.trim().is_empty());
        }
    }

    #[test]
    fn find_round_trips() {
        for t in CATALOG {
            let found = find(t.slug).unwrap();
            assert_eq!(found.slug, t.slug);
        }
        assert!(find("does-not-exist").is_none());
    }

    #[test]
    fn description_field_is_consistent_with_body() {
        // The Template.description must mirror what's inside the TOML
        // so the listing UX matches what the user reads after init.
        for t in CATALOG {
            let wf = parse(t).unwrap();
            assert_eq!(
                wf.workflow.description, t.description,
                "Template.description out of sync for `{}`",
                t.slug
            );
        }
    }
}
