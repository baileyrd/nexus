//! `.skill.md` parsing — split frontmatter + body, decode YAML.

use std::path::Path;

use thiserror::Error;

use crate::{Skill, SkillMeta};

/// Errors surfaced from [`parse_skill_text`] / [`parse_skill_file`].
#[derive(Debug, Error)]
pub enum SkillParseError {
    /// File did not begin with a `---` frontmatter block.
    #[error("missing frontmatter opening delimiter (expected '---' on first line)")]
    MissingOpenDelimiter,
    /// Frontmatter block was never closed with a `---` line.
    #[error("frontmatter opening delimiter was never closed with '---'")]
    MissingCloseDelimiter,
    /// Frontmatter YAML failed to decode.
    #[error("frontmatter YAML decode failed: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    /// File could not be read from disk.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Parse a skill from an in-memory string. The input must start with
/// a `---` line, hold valid YAML frontmatter, close with another
/// `---` line, and optionally carry a markdown body.
///
/// # Errors
/// Returns [`SkillParseError`] when delimiters are missing or YAML
/// decode fails.
pub fn parse_skill_text(source: &str) -> Result<Skill, SkillParseError> {
    let (frontmatter, body) = split_frontmatter(source)?;
    let meta: SkillMeta = serde_yaml::from_str(frontmatter)?;
    Ok(Skill {
        meta,
        body: body.to_string(),
    })
}

/// Parse a skill from disk. Thin wrapper over [`parse_skill_text`]
/// that reads the file first and surfaces IO errors with the same
/// error type.
///
/// # Errors
/// Returns [`SkillParseError::Io`] when the read fails, or any
/// variant [`parse_skill_text`] would return.
pub fn parse_skill_file(path: &Path) -> Result<Skill, SkillParseError> {
    let raw = std::fs::read_to_string(path)?;
    parse_skill_text(&raw)
}

/// Split a `.skill.md` source into `(frontmatter, body)` slices.
/// Accepts CRLF line endings and preserves the body verbatim
/// (including a leading blank line when the frontmatter closes with
/// one). Rejects inputs whose first non-empty line isn't `---`.
fn split_frontmatter(source: &str) -> Result<(&str, &str), SkillParseError> {
    let normalized = source.strip_prefix('\u{feff}').unwrap_or(source);
    let first_non_empty_line_start = normalized
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map_or(0, |(i, _)| i);
    let rest = &normalized[first_non_empty_line_start..];
    let rest = rest
        .strip_prefix("---\r\n")
        .or_else(|| rest.strip_prefix("---\n"))
        .or_else(|| rest.strip_prefix("---"))
        .ok_or(SkillParseError::MissingOpenDelimiter)?;

    // Find the next line that is exactly `---` (optionally with
    // trailing whitespace).
    let mut cursor = 0usize;
    while cursor < rest.len() {
        let next_nl = rest[cursor..]
            .find('\n')
            .map(|n| cursor + n + 1)
            .unwrap_or(rest.len());
        let line = rest[cursor..next_nl].trim_end_matches(['\r', '\n']);
        if line.trim() == "---" {
            let frontmatter = &rest[..cursor];
            let body = &rest[next_nl..];
            return Ok((frontmatter, body));
        }
        cursor = next_nl;
    }
    Err(SkillParseError::MissingCloseDelimiter)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN_VALID: &str = r#"---
name: Code Review
id: code-review
description: Structured code reviews.
version: 1.0.0
author: Team
created: 2026-04-01
tags:
  - engineering
output_format: structured
---
# Body
Review with care.
"#;

    #[test]
    fn parses_minimal_valid_skill() {
        let skill = parse_skill_text(MIN_VALID).unwrap();
        assert_eq!(skill.meta.id, "code-review");
        assert_eq!(skill.meta.name, "Code Review");
        assert_eq!(skill.meta.tags, vec!["engineering"]);
        assert_eq!(skill.meta.output_format.as_deref(), Some("structured"));
        assert!(skill.body.starts_with("# Body"));
    }

    #[test]
    fn preserves_unknown_frontmatter_keys_in_extra() {
        let src = r#"---
name: X
id: x
description: y
version: 1.0.0
author: me
created: 2026-04-01
futurefield: [a, b]
---
body
"#;
        let skill = parse_skill_text(src).unwrap();
        assert!(skill.meta.extra.contains_key("futurefield"));
    }

    #[test]
    fn rejects_missing_open_delimiter() {
        let err = parse_skill_text("no delimiter here").unwrap_err();
        assert!(matches!(err, SkillParseError::MissingOpenDelimiter));
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        let err = parse_skill_text("---\nname: X\nid: x\n").unwrap_err();
        assert!(matches!(err, SkillParseError::MissingCloseDelimiter));
    }

    #[test]
    fn parses_parameters_list() {
        let src = r#"---
name: X
id: x
description: y
version: 1.0.0
author: me
created: 2026-04-01
parameters:
  - name: strictness
    type: enum
    values: [low, medium, high]
    default: medium
    description: Depth of review rigor
  - name: focus_areas
    type: list
    items: string
    default: [security, performance]
---
body
"#;
        let skill = parse_skill_text(src).unwrap();
        assert_eq!(skill.meta.parameters.len(), 2);
        assert_eq!(skill.meta.parameters[0].name, "strictness");
        assert_eq!(skill.meta.parameters[0].param_type, "enum");
        assert_eq!(skill.meta.parameters[1].items.as_deref(), Some("string"));
    }

    #[test]
    fn handles_crlf_line_endings() {
        let src = "---\r\nname: A\r\nid: a\r\ndescription: b\r\nversion: 1\r\nauthor: me\r\ncreated: 2026-04-01\r\n---\r\nbody line\r\n";
        let skill = parse_skill_text(src).unwrap();
        assert_eq!(skill.meta.id, "a");
        assert!(skill.body.contains("body line"));
    }
}
