//! YAML frontmatter extraction and parsing.
//!
//! Files may optionally begin with a `---\n` … `\n---` YAML block.
//! Reserved keys are mapped to typed fields; all other keys go into `custom`.

use std::collections::HashMap;

use crate::error::MarkdownError;

/// Maximum byte size accepted for a YAML frontmatter block. Legitimate
/// frontmatter is typically a few dozen lines (a few KiB at the high
/// end); the cap exists to short-circuit the YAML parser before it can
/// expand pathological alias/anchor structures (the "billion-laughs"
/// shape) on attacker-controlled input. See issue #78.
pub const MAX_FRONTMATTER_BYTES: usize = 256 * 1024;

// ── Public types ──────────────────────────────────────────────────────────────

/// Parsed YAML frontmatter with reserved fields and a custom-key escape hatch.
#[derive(Debug, Clone, Default)]
pub struct Frontmatter {
    /// Document title.
    pub title: Option<String>,
    /// Alternative names for wikilink resolution.
    pub aliases: Vec<String>,
    /// Content tags (same as inline `#tag`).
    pub tags: Vec<String>,
    /// Document type (guide, reference, spec, …).
    pub doc_type: Option<String>,
    /// Workflow state (draft, published, archived, …).
    pub status: Option<String>,
    /// Custom CSS class applied to the rendered page.
    pub cssclass: Option<String>,
    /// Publication / reference date (`YYYY-MM-DD`).
    pub date: Option<String>,
    /// File creation date (`YYYY-MM-DD`).
    pub created: Option<String>,
    /// Last modification date (`YYYY-MM-DD`).
    pub modified: Option<String>,
    /// All unrecognised keys, preserved as raw JSON values.
    pub custom: HashMap<String, serde_json::Value>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Extract YAML frontmatter from raw markdown text.
///
/// Returns `(frontmatter, body_without_delimiter)`. When no frontmatter is
/// present (or it is unterminated), returns a default `Frontmatter` and the
/// full input as the body.
///
/// # Errors
///
/// Returns [`MarkdownError::FrontmatterParse`] if the YAML block is present
/// but malformed.
pub fn extract(content: &str) -> Result<(Frontmatter, &str), MarkdownError> {
    if !content.starts_with("---\n") {
        return Ok((Frontmatter::default(), content));
    }

    let after_open = &content[4..]; // skip "---\n"
    let close_pattern = "\n---";

    let Some(close_pos) = after_open.find(close_pattern) else {
        // Unterminated frontmatter — treat as no frontmatter.
        return Ok((Frontmatter::default(), content));
    };

    let yaml_src = &after_open[..close_pos];
    let after_close = &after_open[close_pos + close_pattern.len()..];
    let body = after_close.strip_prefix('\n').unwrap_or(after_close);

    if yaml_src.len() > MAX_FRONTMATTER_BYTES {
        return Err(MarkdownError::FrontmatterParse {
            file: "<frontmatter>".to_string(),
            reason: format!(
                "frontmatter is {} bytes; max is {MAX_FRONTMATTER_BYTES} bytes",
                yaml_src.len()
            ),
        });
    }

    let yaml: serde_yml::Value =
        serde_yml::from_str(yaml_src).map_err(|e| MarkdownError::FrontmatterParse {
            file: "<frontmatter>".to_string(),
            reason: e.to_string(),
        })?;

    let fm = parse_yaml_frontmatter(&yaml);
    Ok((fm, body))
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn parse_yaml_frontmatter(yaml: &serde_yml::Value) -> Frontmatter {
    let mut fm = Frontmatter::default();

    let serde_yml::Value::Mapping(map) = yaml else {
        return fm;
    };

    for (k, v) in map {
        let key = yaml_key_str(k);
        match key.as_str() {
            "title"    => fm.title    = yaml_as_string(v),
            "type"     => fm.doc_type = yaml_as_string(v),
            "status"   => fm.status   = yaml_as_string(v),
            "cssclass" => fm.cssclass = yaml_as_string(v),
            "date"     => fm.date     = yaml_as_string(v),
            "created"  => fm.created  = yaml_as_string(v),
            "modified" => fm.modified = yaml_as_string(v),
            "aliases"  => fm.aliases  = yaml_as_string_list(v),
            "tags"     => fm.tags     = yaml_as_string_list(v),
            _          => {
                fm.custom.insert(key, yaml_to_json(v));
            }
        }
    }

    fm
}

fn yaml_key_str(k: &serde_yml::Value) -> String {
    match k {
        serde_yml::Value::String(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn yaml_as_string(v: &serde_yml::Value) -> Option<String> {
    match v {
        serde_yml::Value::String(s) => Some(s.clone()),
        serde_yml::Value::Bool(b)   => Some(b.to_string()),
        serde_yml::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn yaml_as_string_list(v: &serde_yml::Value) -> Vec<String> {
    match v {
        serde_yml::Value::Sequence(seq) => seq
            .iter()
            .filter_map(|item| match item {
                serde_yml::Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        serde_yml::Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn yaml_to_json(v: &serde_yml::Value) -> serde_json::Value {
    match v {
        serde_yml::Value::Null         => serde_json::Value::Null,
        serde_yml::Value::Bool(b)      => serde_json::Value::Bool(*b),
        serde_yml::Value::Number(n)    => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yml::Value::String(s)    => serde_json::Value::String(s.clone()),
        serde_yml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(yaml_to_json).collect())
        }
        serde_yml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, val)| (yaml_key_str(k), yaml_to_json(val)))
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yml::Value::Tagged(t) => yaml_to_json(&t.value),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frontmatter_returns_full_body() {
        let (fm, body) = extract("# Hello\n").unwrap();
        assert!(fm.title.is_none());
        assert_eq!(body, "# Hello\n");
    }

    #[test]
    fn basic_reserved_keys() {
        let md = "---\ntitle: My Note\nstatus: draft\n---\n# Body\n";
        let (fm, body) = extract(md).unwrap();
        assert_eq!(fm.title.as_deref(), Some("My Note"));
        assert_eq!(fm.status.as_deref(), Some("draft"));
        assert_eq!(body, "# Body\n");
    }

    #[test]
    fn tags_extracted_as_list() {
        let md = "---\ntags:\n  - rust\n  - programming\n---\nHello\n";
        let (fm, _) = extract(md).unwrap();
        assert_eq!(fm.tags, ["rust", "programming"]);
    }

    #[test]
    fn aliases_extracted() {
        let md = "---\naliases:\n  - Alt Name\n  - Another\n---\n";
        let (fm, _) = extract(md).unwrap();
        assert_eq!(fm.aliases, ["Alt Name", "Another"]);
    }

    #[test]
    fn custom_keys_go_to_custom_map() {
        let md = "---\ncustomField: hello\nnested:\n  a: 1\n---\n";
        let (fm, _) = extract(md).unwrap();
        assert!(fm.custom.contains_key("customField"));
        assert!(fm.custom.contains_key("nested"));
    }

    #[test]
    fn unterminated_frontmatter_treated_as_none() {
        let md = "---\ntitle: Oops\n# No closing delimiter\n";
        let (fm, body) = extract(md).unwrap();
        assert!(fm.title.is_none());
        assert_eq!(body, md);
    }

    #[test]
    fn malformed_yaml_returns_error() {
        let md = "---\ntitle: [unclosed\n---\n";
        let result = extract(md);
        assert!(result.is_err());
    }

    #[test]
    fn empty_input_returns_defaults() {
        let (fm, body) = extract("").unwrap();
        assert!(fm.title.is_none());
        assert_eq!(body, "");
    }

    #[test]
    fn type_key_maps_to_doc_type() {
        let md = "---\ntype: guide\n---\n";
        let (fm, _) = extract(md).unwrap();
        assert_eq!(fm.doc_type.as_deref(), Some("guide"));
    }

    #[test]
    fn date_fields_preserved_as_strings() {
        let md = "---\ndate: 2026-04-13\ncreated: 2026-01-01\nmodified: 2026-04-12\n---\n";
        let (fm, _) = extract(md).unwrap();
        assert_eq!(fm.date.as_deref(), Some("2026-04-13"));
        assert_eq!(fm.created.as_deref(), Some("2026-01-01"));
        assert_eq!(fm.modified.as_deref(), Some("2026-04-12"));
    }
}
