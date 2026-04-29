//! BL-045 — auto-enrichment of markdown notes on save.
//!
//! Two-phase pipeline:
//!
//! 1. **`enrich_file(path)`** — read the file, parse frontmatter,
//!    compute a body-hash, run the AI provider for tags + summary, run
//!    `semantic_search` for related notes, return an
//!    [`EnrichmentProposal`] WITHOUT touching the file. This is the
//!    "propose" step.
//! 2. **`enrich_apply(path, proposal)`** — re-read the file (its body
//!    might have changed during the latency window), verify the
//!    body-hash still matches, merge the proposal's tags / summary /
//!    related into the YAML frontmatter, and write back. If the hash
//!    drifted, return `applied: false` so the shell can re-propose.
//!
//! All AI / vector calls happen in phase 1; phase 2 is pure I/O. The
//! shell shows the proposal in an accept-gate UI between the two
//! phases (see `shell/src/plugins/nexus/enrich/`).
//!
//! Idempotency: the body-hash is computed over the body **excluding**
//! the YAML frontmatter, so re-applying a proposal whose frontmatter
//! we just updated does **not** invalidate later proposals on the same
//! body.

use serde::{Deserialize, Serialize};

/// Proposed enrichment payload — the output of [`propose`] and the
/// input to [`apply`]. JSON-serialised across the IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EnrichmentProposal {
    /// Forge-relative path to the markdown file the proposal refers to.
    pub path: String,
    /// Hex-encoded SHA-256 of the body **excluding** any YAML
    /// frontmatter block. The shell should pass this back unmodified
    /// in [`apply`] so we can detect concurrent edits.
    pub body_hash: String,
    /// Up to 5 single-word lowercase tags suggested by the model.
    pub tags: Vec<String>,
    /// One-sentence summary suggested by the model (≤120 chars).
    pub summary: String,
    /// `[[basename]]` wikilinks to related notes, deduped, with the
    /// input file itself removed.
    pub related: Vec<String>,
}

/// Compute a SHA-256 hex digest of `body` after stripping any leading
/// YAML frontmatter block (`---\n...\n---\n`). Idempotent: applying
/// frontmatter mutations to a note does not change this hash.
#[must_use]
pub fn body_hash(content: &str) -> String {
    let body = strip_frontmatter(content).1;
    let digest = sha256_hex(body.as_bytes());
    digest
}

/// Split `content` into `(frontmatter_yaml, body)`. Returns
/// `(None, content)` if no frontmatter is present.
///
/// The recogniser is intentionally minimal and matches what the shell
/// editor + storage parser both accept: a `---` line on the very first
/// line, followed by YAML, followed by a closing `---` line. Anything
/// else is treated as no-frontmatter.
#[must_use]
pub fn strip_frontmatter(content: &str) -> (Option<&str>, &str) {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return (None, content);
    }
    // Skip the opening fence.
    let after_open = if let Some(rest) = content.strip_prefix("---\n") {
        rest
    } else {
        content.strip_prefix("---\r\n").unwrap_or(content)
    };
    // Find the closing fence — a line consisting solely of `---`.
    let mut idx = 0usize;
    for line in after_open.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            let fm_end = idx;
            let body_start = idx + line.len();
            let fm = &after_open[..fm_end];
            let body = &after_open[body_start..];
            return (Some(fm), body);
        }
        idx += line.len();
    }
    // Unterminated frontmatter — treat the whole file as body so we
    // never accidentally swallow real content.
    (None, content)
}

/// Merge `proposal` into the YAML frontmatter of `original`,
/// preserving everything the user already wrote and only adding tags
/// the user did not already have. Returns the new file contents.
///
/// Behaviour:
/// - `tags`: deduped union of existing-frontmatter `tags` (if a list)
///   and the proposal's tags. Order: existing first, then new.
/// - `summary`: overwritten with the proposal's summary if non-empty.
/// - `related`: overwritten with the proposal's related list if
///   non-empty (the model has fresh retrieval evidence each run).
///
/// If the original had no frontmatter, a new fenced block is prepended.
#[must_use]
pub fn merge_frontmatter(original: &str, proposal: &EnrichmentProposal) -> String {
    let (fm_opt, body) = strip_frontmatter(original);
    let existing = fm_opt.unwrap_or("");

    let mut existing_tags: Vec<String> = parse_tags_field(existing);
    for t in &proposal.tags {
        let lower = t.to_lowercase();
        if !existing_tags.iter().any(|e| e.to_lowercase() == lower) {
            existing_tags.push(t.clone());
        }
    }

    // Drop the lines we are about to rewrite (tags, summary, related)
    // from the existing frontmatter, keeping every other line intact.
    let mut kept: Vec<&str> = Vec::new();
    let mut skip_block = false;
    for line in existing.lines() {
        let trimmed = line.trim_start();
        // Skip continuation lines of a block we're dropping (list
        // entries under tags:).
        if skip_block {
            if line.starts_with(' ') || line.starts_with('\t') || trimmed.starts_with('-') {
                continue;
            }
            skip_block = false;
        }
        if let Some(key) = yaml_key(line) {
            if matches!(key.as_str(), "tags" | "summary" | "related") {
                // If the value is on the same line we just drop the
                // single line; if it's a block list, drop subsequent
                // indented lines too.
                let after_colon = line.splitn(2, ':').nth(1).unwrap_or("").trim();
                if after_colon.is_empty() {
                    skip_block = true;
                }
                continue;
            }
        }
        kept.push(line);
    }

    let mut out = String::new();
    out.push_str("---\n");
    for line in &kept {
        out.push_str(line);
        out.push('\n');
    }
    if !existing_tags.is_empty() {
        out.push_str("tags: [");
        for (i, t) in existing_tags.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(t);
        }
        out.push_str("]\n");
    }
    if !proposal.summary.is_empty() {
        out.push_str("summary: ");
        out.push_str(&yaml_escape_inline(&proposal.summary));
        out.push('\n');
    }
    if !proposal.related.is_empty() {
        out.push_str("related: [");
        for (i, r) in proposal.related.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            // `[[basename]]` → quote so YAML doesn't choke on `[`.
            out.push('"');
            out.push_str(&r.replace('"', "\\\""));
            out.push('"');
        }
        out.push_str("]\n");
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

/// Parse a `tags:` field from a raw YAML frontmatter string. Supports
/// inline `tags: [a, b, c]` and block-list `tags:\n  - a\n  - b`. Any
/// other shape returns an empty vec — we'd rather lose existing tags
/// than corrupt the frontmatter.
fn parse_tags_field(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in yaml.lines() {
        if in_block {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix('-') {
                out.push(rest.trim().trim_matches('"').to_string());
                continue;
            }
            // End of block.
            in_block = false;
        }
        if let Some(key) = yaml_key(line) {
            if key == "tags" {
                let after = line.splitn(2, ':').nth(1).unwrap_or("").trim();
                if after.is_empty() {
                    in_block = true;
                } else if let Some(stripped) = after.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    for piece in stripped.split(',') {
                        let p = piece.trim().trim_matches('"').trim_matches('\'');
                        if !p.is_empty() {
                            out.push(p.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

fn yaml_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.is_empty() {
        return None;
    }
    // Top-level keys only — must not be indented.
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let colon = trimmed.find(':')?;
    let key = trimmed[..colon].trim();
    if key.is_empty() {
        return None;
    }
    Some(key.to_string())
}

fn yaml_escape_inline(s: &str) -> String {
    if s.contains(':') || s.contains('#') || s.contains('"') || s.starts_with('\'') {
        let escaped = s.replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

/// SHA-256 hex digest. Inlined here to avoid pulling another dep into
/// nexus-ai — the storage crate has its own copy via nexus-formats.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_hash_is_idempotent_under_frontmatter_changes() {
        let a = "# Hello\n\nbody text\n";
        let with_fm = "---\ntags: [foo]\n---\n# Hello\n\nbody text\n";
        let with_fm2 = "---\ntags: [foo, bar]\nsummary: hello world\n---\n# Hello\n\nbody text\n";
        assert_eq!(body_hash(a), body_hash(with_fm));
        assert_eq!(body_hash(a), body_hash(with_fm2));
    }

    #[test]
    fn body_hash_changes_when_body_changes() {
        let a = body_hash("---\ntags: [x]\n---\nhello\n");
        let b = body_hash("---\ntags: [x]\n---\nhello!\n");
        assert_ne!(a, b);
    }

    #[test]
    fn strip_frontmatter_handles_no_fm() {
        let (fm, body) = strip_frontmatter("just a body\n");
        assert!(fm.is_none());
        assert_eq!(body, "just a body\n");
    }

    #[test]
    fn strip_frontmatter_handles_unterminated() {
        let s = "---\ntags: [x]\nno closing fence\n";
        let (fm, body) = strip_frontmatter(s);
        assert!(fm.is_none(), "unterminated fm should be treated as body");
        assert_eq!(body, s);
    }

    #[test]
    fn strip_frontmatter_extracts_block() {
        let s = "---\ntags: [x]\nsummary: y\n---\nthe body\n";
        let (fm, body) = strip_frontmatter(s);
        assert_eq!(fm, Some("tags: [x]\nsummary: y\n"));
        assert_eq!(body, "the body\n");
    }

    #[test]
    fn merge_into_empty_file_creates_frontmatter() {
        let proposal = EnrichmentProposal {
            path: "n.md".into(),
            body_hash: "h".into(),
            tags: vec!["alpha".into(), "beta".into()],
            summary: "a note".into(),
            related: vec!["[[other]]".into()],
        };
        let out = merge_frontmatter("# Title\nbody\n", &proposal);
        assert!(out.starts_with("---\n"));
        assert!(out.contains("tags: [alpha, beta]"));
        assert!(out.contains("summary: a note"));
        assert!(out.contains("related: [\"[[other]]\"]"));
        assert!(out.ends_with("# Title\nbody\n"));
    }

    #[test]
    fn merge_preserves_unrelated_keys_and_dedupes_tags() {
        let original = "---\ntitle: My Note\ntags: [keep, beta]\nauthor: me\n---\nbody\n";
        let proposal = EnrichmentProposal {
            path: "n.md".into(),
            body_hash: "h".into(),
            tags: vec!["beta".into(), "new".into()],
            summary: String::new(),
            related: vec![],
        };
        let out = merge_frontmatter(original, &proposal);
        assert!(out.contains("title: My Note"));
        assert!(out.contains("author: me"));
        // Tags preserved + deduped (beta only once) + new added.
        assert!(out.contains("tags: [keep, beta, new]"), "got: {out}");
        // No empty summary / related lines emitted.
        assert!(!out.contains("summary:"));
        assert!(!out.contains("related:"));
        // Body preserved.
        assert!(out.ends_with("body\n"));
    }

    #[test]
    fn merge_drops_block_list_tags_and_replaces_inline() {
        let original = "---\ntags:\n  - old1\n  - old2\nfoo: bar\n---\nbody\n";
        let proposal = EnrichmentProposal {
            path: "n.md".into(),
            body_hash: "h".into(),
            tags: vec!["new".into()],
            summary: String::new(),
            related: vec![],
        };
        let out = merge_frontmatter(original, &proposal);
        // The block-list `tags:` should have been replaced by an
        // inline list including the merged old + new tags.
        assert!(out.contains("foo: bar"));
        assert!(out.contains("tags: [old1, old2, new]"), "got: {out}");
        assert!(!out.contains("- old1"));
    }

    #[test]
    fn merge_summary_with_colon_is_quoted() {
        let proposal = EnrichmentProposal {
            path: "n.md".into(),
            body_hash: "h".into(),
            tags: vec![],
            summary: "Title: subtitle".into(),
            related: vec![],
        };
        let out = merge_frontmatter("body\n", &proposal);
        assert!(out.contains("summary: \"Title: subtitle\""), "got: {out}");
    }
}
