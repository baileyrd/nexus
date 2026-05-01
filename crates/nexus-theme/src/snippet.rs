//! CSS snippet format and discovery (PRD §4).
//!
//! A snippet is a user-authored `.css` file with a CSS block comment at the
//! top whose lines carry metadata (Name, Description, Mode, Scope). The body
//! may contain arbitrary CSS; for resolution purposes we extract any
//! `:root { --nx-*: value }` declarations so they can participate in the
//! variable cascade.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;

use crate::variables::VariableMap;
use crate::{Result, ThemeError};

/// The mode(s) in which a snippet applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum SnippetMode {
    /// Applies in both light and dark modes.
    #[default]
    All,
    /// Applies only in light mode.
    Light,
    /// Applies only in dark mode.
    Dark,
}

/// Where a snippet's CSS rules apply.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "kebab-case")]
pub enum SnippetScope {
    /// Applied to `<html>` for all surfaces.
    #[default]
    Global,
    /// Applied only to panes matching the given selector (e.g. `.editor-pane`).
    PerSurface(String),
}

/// Parsed CSS snippet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CssSnippet {
    /// Identifier — the filename stem (e.g. `neon-accents.css` → `neon-accents`).
    pub id: String,

    /// Source path on disk (empty for in-memory snippets).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<PathBuf>,

    /// `Name:` field from the header (required).
    pub name: String,

    /// `Description:` field from the header (required).
    pub description: String,

    /// `Author:` field from the header (optional).
    #[serde(default)]
    pub author: Option<String>,

    /// `Version:` field from the header (optional).
    #[serde(default)]
    pub version: Option<String>,

    /// `Mode:` field — defaults to [`SnippetMode::All`].
    #[serde(default)]
    pub mode: SnippetMode,

    /// `Scope:` field — defaults to [`SnippetScope::Global`].
    #[serde(default)]
    pub scope: SnippetScope,

    /// CSS variable assignments extracted from the top-level `:root { ... }`
    /// block. These feed the cascade.
    #[serde(default)]
    pub variables: VariableMap,

    /// Full CSS body (after the header comment). The frontend injects this
    /// verbatim so arbitrary selectors still work.
    pub body: String,
}

impl CssSnippet {
    /// Parse a snippet from source text + an identifier.
    ///
    /// # Errors
    /// Returns [`ThemeError::SnippetHeader`] when the header comment block is
    /// missing, malformed, or missing required fields.
    pub fn parse(id: impl Into<String>, source: &str) -> Result<Self> {
        let id = id.into();
        let (header_fields, body) = split_header(source).ok_or_else(|| {
            ThemeError::SnippetHeader {
                path: PathBuf::from(&id),
                reason: "missing `/* ... */` header block".into(),
            }
        })?;

        let mut name = None;
        let mut description = None;
        let mut author = None;
        let mut version = None;
        let mut mode = SnippetMode::All;
        let mut scope = SnippetScope::Global;

        for (key, value) in header_fields {
            match key.as_str() {
                "name" => name = Some(value),
                "description" => description = Some(value),
                "author" => author = Some(value),
                "version" => version = Some(value),
                "mode" => {
                    mode = match value.to_ascii_lowercase().as_str() {
                        "light" => SnippetMode::Light,
                        "dark" => SnippetMode::Dark,
                        "all" => SnippetMode::All,
                        other => {
                            return Err(ThemeError::SnippetHeader {
                                path: PathBuf::from(&id),
                                reason: format!("invalid Mode: {other}"),
                            });
                        }
                    };
                }
                "scope" => {
                    scope = if value.eq_ignore_ascii_case("global") {
                        SnippetScope::Global
                    } else {
                        SnippetScope::PerSurface(value)
                    };
                }
                _ => {}
            }
        }

        let name = name.ok_or_else(|| ThemeError::SnippetHeader {
            path: PathBuf::from(&id),
            reason: "missing required `Name:` field".into(),
        })?;
        let description = description.ok_or_else(|| ThemeError::SnippetHeader {
            path: PathBuf::from(&id),
            reason: "missing required `Description:` field".into(),
        })?;

        let variables = extract_root_variables(body);

        Ok(Self {
            id,
            path: None,
            name,
            description,
            author,
            version,
            mode,
            scope,
            variables,
            body: body.to_string(),
        })
    }

    /// Load and parse a snippet from a `.css` file. The file stem becomes
    /// the snippet's [`id`](Self::id).
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] on read failure or [`ThemeError::SnippetHeader`]
    /// on parse failure.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|e| ThemeError::io(path, e))?;
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map_or_else(|| path.display().to_string(), str::to_owned);
        let mut snippet = Self::parse(id, &source)?;
        snippet.path = Some(path.to_path_buf());
        Ok(snippet)
    }

    /// Scan `dir` for `*.css` files and parse each one.
    ///
    /// Missing directories return `Ok(vec![])` — the caller decides whether
    /// the absence is fatal. Files that fail to parse are logged via
    /// `tracing::warn!` and skipped so one bad snippet never hides the rest.
    ///
    /// # Errors
    /// Only returns an error if the directory read itself fails with
    /// something other than "not found".
    pub fn discover(dir: impl AsRef<Path>) -> Result<Vec<Self>> {
        let dir = dir.as_ref();
        let entries = match fs::read_dir(dir) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(ThemeError::io(dir, e)),
        };

        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("css") {
                continue;
            }
            match Self::load(&path) {
                Ok(s) => out.push(s),
                Err(e) => tracing::warn!(?path, %e, "skipping malformed snippet"),
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Returns `true` if this snippet applies to the given mode.
    #[must_use]
    pub fn applies_to(&self, mode: SnippetMode) -> bool {
        matches!(self.mode, SnippetMode::All) || self.mode == mode
    }
}

/// Split `source` into `(header_fields, body)`.
///
/// Returns `None` when the source does not begin with a `/* ... */` block.
fn split_header(source: &str) -> Option<(Vec<(String, String)>, &str)> {
    let trimmed = source.trim_start();
    let start = source.len() - trimmed.len();
    let rest = trimmed.strip_prefix("/*")?;
    let end_rel = rest.find("*/")?;
    let header_body = &rest[..end_rel];
    let body_start = start + 2 + end_rel + 2;
    let body = &source[body_start..];

    let mut fields = Vec::new();
    for line in header_body.lines() {
        let line = line
            .trim()
            .trim_start_matches('*')
            .trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if !key.is_empty() {
                fields.push((key, value));
            }
        }
    }
    Some((fields, body))
}

/// Extract `--nx-foo: value;` pairs from the first `:root { ... }` block in
/// the snippet body. Nested `:root` blocks are merged; other selectors are
/// ignored (they still pass through via [`CssSnippet::body`]).
fn extract_root_variables(body: &str) -> VariableMap {
    let mut out = VariableMap::new();
    let mut remaining = body;

    while let Some(idx) = remaining.find(":root") {
        let after = &remaining[idx + 5..];
        let Some(brace_start) = after.find('{') else {
            break;
        };
        let inside = &after[brace_start + 1..];
        let Some(brace_end) = inside.find('}') else {
            break;
        };
        let block = &inside[..brace_end];

        for decl in block.split(';') {
            let decl = decl.trim();
            if decl.is_empty() {
                continue;
            }
            if let Some((name, value)) = decl.split_once(':') {
                let name = name.trim();
                if name.starts_with("--nx-") {
                    out.insert(name.to_string(), value.trim().to_string());
                }
            }
        }

        remaining = &inside[brace_end + 1..];
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r"/*
 * Nexus CSS Snippet
 * Name: Neon Accents
 * Description: Bright, high-contrast accent colors
 * Author: user@example.com
 * Version: 1.0
 * Mode: dark
 * Scope: global
 */

:root {
  --nx-color-primary: #00FF00;
  --nx-color-error: #FF00FF;
}

.nx-button:hover {
  text-shadow: 0 0 8px var(--nx-color-primary);
}
";

    #[test]
    fn parses_sample_snippet() {
        let s = CssSnippet::parse("neon", SAMPLE).unwrap();
        assert_eq!(s.name, "Neon Accents");
        assert_eq!(s.description, "Bright, high-contrast accent colors");
        assert_eq!(s.author.as_deref(), Some("user@example.com"));
        assert_eq!(s.mode, SnippetMode::Dark);
        assert_eq!(s.scope, SnippetScope::Global);
        assert_eq!(s.variables["--nx-color-primary"], "#00FF00");
        assert_eq!(s.variables["--nx-color-error"], "#FF00FF");
        assert!(s.body.contains(".nx-button:hover"));
    }

    #[test]
    fn missing_required_fields_errors() {
        let src = "/* Description: no name here */ :root { }";
        let err = CssSnippet::parse("x", src).unwrap_err();
        assert!(matches!(err, ThemeError::SnippetHeader { .. }));
    }

    #[test]
    fn missing_header_errors() {
        let err = CssSnippet::parse("x", ":root { --nx-x: 1; }").unwrap_err();
        assert!(matches!(err, ThemeError::SnippetHeader { .. }));
    }

    #[test]
    fn per_surface_scope_is_preserved() {
        let src = "/* Name: N\nDescription: D\nScope: .editor-pane */";
        let s = CssSnippet::parse("x", src).unwrap();
        assert_eq!(s.scope, SnippetScope::PerSurface(".editor-pane".into()));
    }

    #[test]
    fn applies_to_respects_mode() {
        let s = CssSnippet::parse("x", SAMPLE).unwrap();
        assert!(s.applies_to(SnippetMode::Dark));
        assert!(!s.applies_to(SnippetMode::Light));
    }

    #[test]
    fn discovery_returns_empty_for_missing_dir() {
        let out = CssSnippet::discover("/nonexistent/path/definitely").unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn discovery_reads_css_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.css"),
            "/* Name: A\nDescription: a */\n:root { --nx-a: 1; }",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.css"),
            "/* Name: B\nDescription: b */\n:root { --nx-b: 2; }",
        )
        .unwrap();
        std::fs::write(dir.path().join("ignore.txt"), "not a snippet").unwrap();

        let out = CssSnippet::discover(dir.path()).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[1].id, "b");
    }
}
