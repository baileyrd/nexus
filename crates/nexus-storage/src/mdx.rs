//! MDX (Markdown + JSX) pre-processing pipeline.
//!
//! Extracts JSX component tags from MDX content, replaces them with
//! placeholder markers, and feeds the cleaned markdown to the standard
//! comrak parser. Returns both the parsed markdown and the extracted
//! JSX components.

use crate::parser::{parse_markdown, ParsedFile};
use crate::StorageError;

// ── Public types ──────────────────────────────────────────────────────────────

/// A JSX component extracted from an MDX file.
#[derive(Debug, Clone)]
pub struct ParsedJsxComponent {
    /// Component name (e.g. `"Chart"`, `"Alert"`).
    pub name: String,
    /// Raw props as a JSON string, or `None` if no props.
    pub props_json: Option<String>,
    /// 1-based line number where the component starts.
    pub line_number: u32,
    /// Whether the tag is self-closing (`<Component />`).
    pub self_closing: bool,
}

/// Result of parsing an MDX file.
#[derive(Debug, Clone)]
pub struct MdxParseResult {
    /// The markdown-parsed content (with JSX replaced by placeholders).
    pub parsed_file: ParsedFile,
    /// All JSX components extracted from the MDX source.
    pub components: Vec<ParsedJsxComponent>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse an MDX string, extracting JSX components and parsing the remaining
/// markdown.
///
/// 1. Strips `import` and `export` statements from the top of the file
/// 2. Scans for JSX tags (`<Component ...>...</Component>` and `<Component />`)
/// 3. Replaces JSX blocks with empty lines to preserve line numbering
/// 4. Feeds the cleaned markdown to [`parse_markdown`]
///
/// # Errors
///
/// Returns [`StorageError::ParseError`] if the underlying markdown parse fails.
pub fn parse_mdx(content: &str) -> Result<MdxParseResult, StorageError> {
    let lines: Vec<&str> = content.lines().collect();
    let mut cleaned_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut components: Vec<ParsedJsxComponent> = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Skip import/export statements.
        if trimmed.starts_with("import ") || trimmed.starts_with("export ") {
            cleaned_lines.push(String::new());
            i += 1;
            continue;
        }

        // Try to match a JSX tag.
        if let Some(result) = try_parse_jsx_tag(trimmed) {
            let line_number = u32::try_from(i + 1).unwrap_or(0);

            match result {
                JsxMatch::SelfClosing { name, props_json } => {
                    components.push(ParsedJsxComponent {
                        name,
                        props_json,
                        line_number,
                        self_closing: true,
                    });
                    cleaned_lines.push(String::new());
                    i += 1;
                }
                JsxMatch::Opening { name, props_json } => {
                    // Find the matching closing tag.
                    let close_tag = format!("</{name}>");
                    let start = i;
                    let mut depth: u32 = 1;
                    i += 1;

                    while i < lines.len() && depth > 0 {
                        let inner = lines[i].trim();
                        // Check for nested opening tags of the same component.
                        if inner.starts_with(&format!("<{name}"))
                            && !inner.ends_with("/>")
                        {
                            depth += 1;
                        }
                        if inner.contains(&close_tag) {
                            depth -= 1;
                        }
                        i += 1;
                    }

                    components.push(ParsedJsxComponent {
                        name,
                        props_json,
                        line_number,
                        self_closing: false,
                    });

                    // Replace all lines of the JSX block with empty lines.
                    for _ in start..i {
                        cleaned_lines.push(String::new());
                    }
                }
            }
        } else {
            cleaned_lines.push(lines[i].to_string());
            i += 1;
        }
    }

    // Ensure trailing newline for comrak.
    let cleaned = cleaned_lines.join("\n") + "\n";
    let parsed_file = parse_markdown(&cleaned)?;

    Ok(MdxParseResult {
        parsed_file,
        components,
    })
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Intermediate result from JSX tag detection.
enum JsxMatch {
    SelfClosing {
        name: String,
        props_json: Option<String>,
    },
    Opening {
        name: String,
        props_json: Option<String>,
    },
}

/// Try to parse a trimmed line as the start of a JSX component tag.
///
/// Returns `None` if the line doesn't start with `<` followed by an
/// uppercase letter (JSX convention). HTML tags (`<div>`, `<p>`) are
/// ignored.
fn try_parse_jsx_tag(line: &str) -> Option<JsxMatch> {
    // Must start with '<' followed by an uppercase ASCII letter.
    let rest = line.strip_prefix('<')?;
    let first_char = rest.chars().next()?;
    if !first_char.is_ascii_uppercase() {
        return None;
    }

    // Extract the component name (alphanumeric + dots for namespaced components).
    let name_end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '_')
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_string();
    let after_name = rest[name_end..].trim();

    // Self-closing: ends with "/>"
    if after_name.ends_with("/>") {
        let props_raw = after_name.strip_suffix("/>").unwrap_or("").trim();
        let props_json = parse_props(props_raw);
        return Some(JsxMatch::SelfClosing { name, props_json });
    }

    // Opening tag: ends with ">"
    if after_name.ends_with('>') {
        let props_raw = after_name.strip_suffix('>').unwrap_or("").trim();
        let props_json = parse_props(props_raw);
        return Some(JsxMatch::Opening { name, props_json });
    }

    // Opening tag without closing ">" on this line (multi-line props) —
    // treat as opening with whatever props we can extract.
    if !after_name.is_empty() {
        let props_json = parse_props(after_name);
        return Some(JsxMatch::Opening { name, props_json });
    }

    None
}

/// Parse raw JSX props into a JSON object string.
///
/// Handles `key="string"`, `key={expression}`, and bare `key` (boolean true).
/// Returns `None` for empty props.
#[allow(clippy::too_many_lines)]
fn parse_props(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let mut props = serde_json::Map::new();
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace.
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        // Extract key.
        let key_start = i;
        while i < len && chars[i] != '=' && !chars[i].is_whitespace() {
            i += 1;
        }
        let key: String = chars[key_start..i].iter().collect();
        if key.is_empty() {
            break;
        }

        // Skip whitespace around '='.
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }

        if i >= len || chars[i] != '=' {
            // Boolean prop (no value).
            props.insert(key, serde_json::Value::Bool(true));
            continue;
        }

        i += 1; // skip '='

        while i < len && chars[i].is_whitespace() {
            i += 1;
        }

        if i >= len {
            props.insert(key, serde_json::Value::Null);
            break;
        }

        // Parse value.
        match chars[i] {
            '"' => {
                // String value: "..."
                i += 1;
                let val_start = i;
                while i < len && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1; // skip escaped char
                    }
                    i += 1;
                }
                let val: String = chars[val_start..i].iter().collect();
                props.insert(key, serde_json::Value::String(val));
                if i < len {
                    i += 1; // skip closing '"'
                }
            }
            '\'' => {
                // String value: '...'
                i += 1;
                let val_start = i;
                while i < len && chars[i] != '\'' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let val: String = chars[val_start..i].iter().collect();
                props.insert(key, serde_json::Value::String(val));
                if i < len {
                    i += 1;
                }
            }
            '{' => {
                // Expression value: {...}
                i += 1;
                let val_start = i;
                let mut brace_depth: u32 = 1;
                while i < len && brace_depth > 0 {
                    match chars[i] {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                    if brace_depth > 0 {
                        i += 1;
                    }
                }
                let expr: String = chars[val_start..i].iter().collect();
                // Try to parse as JSON value; fall back to string.
                let val = serde_json::from_str::<serde_json::Value>(expr.trim())
                    .unwrap_or_else(|_| serde_json::Value::String(expr.trim().to_string()));
                props.insert(key, val);
                if i < len {
                    i += 1; // skip closing '}'
                }
            }
            _ => {
                // Bare value (number or identifier).
                let val_start = i;
                while i < len && !chars[i].is_whitespace() {
                    i += 1;
                }
                let val: String = chars[val_start..i].iter().collect();
                let json_val = serde_json::from_str::<serde_json::Value>(&val)
                    .unwrap_or(serde_json::Value::String(val));
                props.insert(key, json_val);
            }
        }
    }

    if props.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&serde_json::Value::Object(props)).unwrap_or_default())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── self-closing component ────────────────────────────────────────────

    #[test]
    fn parse_self_closing_component() {
        let mdx = "# Title\n\n<Chart data={items} />\n\nSome text.\n";
        let result = parse_mdx(mdx).unwrap();
        assert_eq!(result.components.len(), 1);
        let c = &result.components[0];
        assert_eq!(c.name, "Chart");
        assert!(c.self_closing);
        assert_eq!(c.line_number, 3);
        assert!(c.props_json.is_some());
        let props = c.props_json.as_ref().unwrap();
        assert!(props.contains("data"));
    }

    // ── opening + closing component ──────────────────────────────────────

    #[test]
    fn parse_block_component() {
        let mdx = "# Title\n\n<Alert type=\"warning\">\nThis is dangerous\n</Alert>\n\nMore text.\n";
        let result = parse_mdx(mdx).unwrap();
        assert_eq!(result.components.len(), 1);
        let c = &result.components[0];
        assert_eq!(c.name, "Alert");
        assert!(!c.self_closing);
        assert_eq!(c.line_number, 3);
        let props = c.props_json.as_ref().unwrap();
        assert!(props.contains("warning"));
    }

    // ── multiple components ──────────────────────────────────────────────

    #[test]
    fn parse_multiple_components() {
        let mdx = "<Header />\n\n# Title\n\n<Chart data={[1,2,3]} />\n\n<Footer>\nCopyright\n</Footer>\n";
        let result = parse_mdx(mdx).unwrap();
        assert_eq!(result.components.len(), 3);
        assert_eq!(result.components[0].name, "Header");
        assert!(result.components[0].self_closing);
        assert_eq!(result.components[1].name, "Chart");
        assert!(result.components[1].self_closing);
        assert_eq!(result.components[2].name, "Footer");
        assert!(!result.components[2].self_closing);
    }

    // ── import/export stripping ──────────────────────────────────────────

    #[test]
    fn strip_import_export_statements() {
        let mdx = "import { data } from \"./data.json\"\nexport const meta = {}\n\n# Title\n";
        let result = parse_mdx(mdx).unwrap();
        assert_eq!(result.components.len(), 0);
        assert!(result.parsed_file.blocks.iter().any(|b| b.content == "Title"));
    }

    // ── html tags ignored ────────────────────────────────────────────────

    #[test]
    fn html_tags_not_treated_as_components() {
        let mdx = "# Title\n\n<div>hello</div>\n\n<p>world</p>\n";
        let result = parse_mdx(mdx).unwrap();
        assert_eq!(result.components.len(), 0);
    }

    // ── component with no props ──────────────────────────────────────────

    #[test]
    fn parse_component_no_props() {
        let mdx = "<Divider />\n";
        let result = parse_mdx(mdx).unwrap();
        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].name, "Divider");
        assert!(result.components[0].props_json.is_none());
    }

    // ── props parsing ────────────────────────────────────────────────────

    #[test]
    fn parse_string_prop() {
        let mdx = "<Alert type=\"info\" />\n";
        let result = parse_mdx(mdx).unwrap();
        let props = result.components[0].props_json.as_ref().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(props).unwrap();
        assert_eq!(parsed["type"], "info");
    }

    #[test]
    fn parse_numeric_expression_prop() {
        let mdx = "<Widget count={42} />\n";
        let result = parse_mdx(mdx).unwrap();
        let props = result.components[0].props_json.as_ref().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(props).unwrap();
        assert_eq!(parsed["count"], 42);
    }

    #[test]
    fn parse_boolean_prop() {
        let mdx = "<Toggle disabled />\n";
        let result = parse_mdx(mdx).unwrap();
        let props = result.components[0].props_json.as_ref().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(props).unwrap();
        assert_eq!(parsed["disabled"], true);
    }

    // ── markdown content preserved ───────────────────────────────────────

    #[test]
    fn markdown_content_preserved_around_components() {
        let mdx = "# Heading\n\nParagraph before.\n\n<Chart />\n\nParagraph after.\n";
        let result = parse_mdx(mdx).unwrap();
        let blocks = &result.parsed_file.blocks;
        assert!(blocks.iter().any(|b| b.content == "Heading"));
        assert!(blocks.iter().any(|b| b.content == "Paragraph before."));
        assert!(blocks.iter().any(|b| b.content == "Paragraph after."));
    }

    // ── wikilinks and tags still extracted ────────────────────────────────

    #[test]
    fn wikilinks_and_tags_extracted_from_mdx() {
        let mdx = "See [[other-note]] and #rust\n\n<Widget />\n";
        let result = parse_mdx(mdx).unwrap();
        assert!(result.parsed_file.links.iter().any(|l| l.link_text == "other-note"));
        assert!(result.parsed_file.tags.iter().any(|t| t.name == "rust"));
    }

    // ── nested components ────────────────────────────────────────────────

    #[test]
    fn parse_nested_same_component() {
        let mdx = "<Tabs>\n<Tabs>\nInner\n</Tabs>\nOuter\n</Tabs>\n";
        let result = parse_mdx(mdx).unwrap();
        // The outer component is extracted; inner is part of its body.
        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].name, "Tabs");
    }

    // ── empty mdx ────────────────────────────────────────────────────────

    #[test]
    fn parse_empty_mdx() {
        let result = parse_mdx("").unwrap();
        assert!(result.components.is_empty());
        assert!(result.parsed_file.blocks.is_empty());
    }
}
