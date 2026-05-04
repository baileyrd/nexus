//! Template parsing, rendering, and application.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::substitute::{render, SubstitutionError};

/// A parsed `.template.md` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    /// Frontmatter (typed projection).
    #[serde(flatten)]
    pub meta: TemplateMeta,
    /// Body — everything after the closing `---`. Substitution is run on
    /// this when [`Self::apply`] is called.
    #[serde(default)]
    pub body: String,
}

/// Frontmatter fields. Unknown fields are accepted via [`Self::extra`] so
/// the schema can grow without breaking older parsers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateMeta {
    /// Unique short name used to identify the template.
    pub name: String,
    /// Optional human description for the picker.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional path pattern for the destination file. Substitution runs on
    /// this just like the body. Defaults to `{{name}}.md` if omitted.
    #[serde(default)]
    pub target_path: Option<String>,
    /// Optional list of declared parameters.
    #[serde(default)]
    pub parameters: Vec<TemplateParameter>,
    /// Future fields are tolerated.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yml::Value>,
}

/// One parameter declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateParameter {
    /// Parameter name. Used both as the substitution key and the CLI arg.
    pub name: String,
    /// Type hint. Used by future UIs to render the right input control.
    #[serde(default = "default_param_type")]
    pub r#type: ParameterType,
    /// Default value (substituted into the body if the caller didn't supply
    /// one). Built-ins like `{{today}}` are resolved at apply time.
    #[serde(default)]
    pub default: Option<String>,
    /// If `true`, the caller must supply a value (no fallback).
    #[serde(default)]
    pub required: bool,
    /// Description shown in the picker.
    #[serde(default)]
    pub description: Option<String>,
}

fn default_param_type() -> ParameterType {
    ParameterType::String
}

/// Parameter type hint. The runtime stores everything as strings; this is
/// a UI hint for how to prompt for the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    /// Free-form string.
    String,
    /// Numeric.
    Number,
    /// Boolean (true/false).
    Boolean,
    /// ISO date.
    Date,
}

// ── Errors ──────────────────────────────────────────────────────────────────

/// Failure parsing a `.template.md` file.
#[derive(Debug, thiserror::Error)]
pub enum TemplateParseError {
    /// Frontmatter block was missing or malformed.
    #[error("missing or malformed frontmatter in '{file}': {reason}")]
    MissingFrontmatter {
        /// Source path (best-effort).
        file: String,
        /// Reason.
        reason: String,
    },
    /// Frontmatter parsed as YAML but didn't match the [`TemplateMeta`] shape.
    #[error("frontmatter schema error in '{file}': {reason}")]
    SchemaError {
        /// Source path.
        file: String,
        /// Serde error message.
        reason: String,
    },
    /// I/O failure reading the file.
    #[error("I/O error reading '{file}': {source}")]
    Io {
        /// Source path.
        file: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

/// Failure applying (rendering + writing) a template.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// A required parameter wasn't supplied and has no default.
    #[error("missing required parameter '{name}'")]
    MissingParameter {
        /// Parameter name.
        name: String,
    },
    /// Substitution in the body or target path failed.
    #[error("substitution failed: {0}")]
    Substitution(#[from] SubstitutionError),
    /// The resolved target path tried to escape the destination root.
    #[error("target path '{path}' escapes destination root")]
    PathEscape {
        /// Resolved path.
        path: String,
    },
    /// I/O failure writing the rendered output.
    #[error("I/O error writing '{path}': {source}")]
    Io {
        /// Output path.
        path: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The destination file already exists and the caller asked not to overwrite.
    #[error("destination file already exists: '{path}'")]
    AlreadyExists {
        /// Output path.
        path: String,
    },
}

// ── Parsing ─────────────────────────────────────────────────────────────────

/// Parse a template from a file. Convenience wrapper over [`parse_template_text`].
///
/// # Errors
/// See [`TemplateParseError`].
pub fn parse_template_file(path: &Path) -> Result<Template, TemplateParseError> {
    let body = std::fs::read_to_string(path).map_err(|source| TemplateParseError::Io {
        file: path.display().to_string(),
        source,
    })?;
    parse_template_text(&body, &path.display().to_string())
}

/// Parse a template from a string. `file` is used only for error messages.
///
/// # Errors
/// See [`TemplateParseError`].
pub fn parse_template_text(input: &str, file: &str) -> Result<Template, TemplateParseError> {
    let stripped = input.strip_prefix("---\n").ok_or_else(|| {
        TemplateParseError::MissingFrontmatter {
            file: file.to_string(),
            reason: "file does not start with `---`".to_string(),
        }
    })?;

    let end = stripped.find("\n---\n").or_else(|| {
        // Tolerate `---` at end of file with no body.
        stripped.find("\n---").filter(|i| stripped[*i + 4..].is_empty())
    });
    let end = end.ok_or_else(|| TemplateParseError::MissingFrontmatter {
        file: file.to_string(),
        reason: "missing closing `---` separator".to_string(),
    })?;

    let yaml = &stripped[..end];
    let body = if end + 5 <= stripped.len() {
        stripped[end + 5..].to_string()
    } else {
        String::new()
    };

    let meta: TemplateMeta =
        serde_yml::from_str(yaml).map_err(|e| TemplateParseError::SchemaError {
            file: file.to_string(),
            reason: e.to_string(),
        })?;

    Ok(Template { meta, body })
}

// ── Application ─────────────────────────────────────────────────────────────

impl Template {
    /// Resolve final values for every parameter, applying defaults and
    /// built-ins. Returns the value map that [`Self::render`] will consume.
    ///
    /// # Errors
    /// [`ApplyError::MissingParameter`] if a required parameter is missing.
    /// [`ApplyError::Substitution`] if a default contains an unresolved
    /// `{{var}}`.
    pub fn resolve_values(
        &self,
        user_args: &BTreeMap<String, String>,
        forge_root: &Path,
    ) -> Result<BTreeMap<String, String>, ApplyError> {
        let builtins = builtin_values(forge_root);

        let mut values: BTreeMap<String, String> = builtins.clone();
        // Layer user args on top so users can override built-ins like `today`.
        for (k, v) in user_args {
            values.insert(k.clone(), v.clone());
        }

        // Apply defaults for every declared parameter that's still missing.
        for p in &self.meta.parameters {
            if values.contains_key(&p.name) {
                continue;
            }
            if let Some(default) = &p.default {
                let rendered = render(default, &values)?;
                values.insert(p.name.clone(), rendered);
            } else if p.required {
                return Err(ApplyError::MissingParameter {
                    name: p.name.clone(),
                });
            } else {
                values.insert(p.name.clone(), String::new());
            }
        }

        Ok(values)
    }

    /// Render the body and target path against `values`. Returns the
    /// rendered (body, target_path).
    ///
    /// # Errors
    /// [`ApplyError::Substitution`] if either render fails.
    pub fn render(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<(String, String), ApplyError> {
        let body = render(&self.body, values)?;
        let target = match &self.meta.target_path {
            Some(t) => render(t, values)?,
            None => format!("{}.md", self.meta.name),
        };
        // If the user typed a parameter that already ends in `.md` (e.g.
        // `title = "Notes.md"`), the rendered target_path can come out
        // looking like `Notes.md.md`. Collapse the duplicate.
        let target = if let Some(stripped) = target.strip_suffix(".md.md") {
            format!("{stripped}.md")
        } else {
            target
        };
        Ok((body, target))
    }

    /// Apply the template: resolve, render, then write to disk under `dest_root`.
    /// `dest_root` is typically the forge root.
    ///
    /// `overwrite = false` makes the call fail if the target exists.
    /// Returns the absolute path that was written.
    ///
    /// # Errors
    /// See [`ApplyError`].
    pub fn apply(
        &self,
        user_args: &BTreeMap<String, String>,
        dest_root: &Path,
        overwrite: bool,
    ) -> Result<PathBuf, ApplyError> {
        let values = self.resolve_values(user_args, dest_root)?;
        let (body, target) = self.render(&values)?;

        let target_path = PathBuf::from(&target);
        if target_path.is_absolute() || target.contains("..") {
            return Err(ApplyError::PathEscape { path: target });
        }
        let abs = dest_root.join(&target_path);
        if !overwrite && abs.exists() {
            return Err(ApplyError::AlreadyExists {
                path: abs.display().to_string(),
            });
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ApplyError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        std::fs::write(&abs, body).map_err(|source| ApplyError::Io {
            path: abs.display().to_string(),
            source,
        })?;
        Ok(abs)
    }
}

/// Built-in template variables provided to every apply call.
///
/// Currently:
/// - `today` — `YYYY-MM-DD` (UTC)
/// - `now`   — RFC-3339 timestamp (UTC)
/// - `forge_path` — absolute path of the forge root
fn builtin_values(forge_root: &Path) -> BTreeMap<String, String> {
    let now = chrono::Utc::now();
    let mut m = BTreeMap::new();
    m.insert("today".to_string(), now.format("%Y-%m-%d").to_string());
    m.insert("now".to_string(), now.to_rfc3339());
    m.insert("forge_path".to_string(), forge_root.display().to_string());
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn parse(text: &str) -> Template {
        parse_template_text(text, "test").expect("parse ok")
    }

    #[test]
    fn parses_minimal_template() {
        let t = parse("---\nname: greet\n---\nHello.\n");
        assert_eq!(t.meta.name, "greet");
        assert_eq!(t.body, "Hello.\n");
    }

    #[test]
    fn parses_with_parameters() {
        let t = parse(
            "---\nname: greet\nparameters:\n  - name: title\n    required: true\n  - name: tone\n    default: friendly\n---\n# {{title}}\n",
        );
        assert_eq!(t.meta.parameters.len(), 2);
        assert_eq!(t.meta.parameters[0].name, "title");
        assert!(t.meta.parameters[0].required);
        assert_eq!(t.meta.parameters[1].default.as_deref(), Some("friendly"));
    }

    #[test]
    fn missing_frontmatter_errors() {
        let err = parse_template_text("no frontmatter here", "test").unwrap_err();
        assert!(matches!(err, TemplateParseError::MissingFrontmatter { .. }));
    }

    #[test]
    fn missing_closing_separator_errors() {
        let err = parse_template_text("---\nname: x\nbody no closing", "test").unwrap_err();
        assert!(matches!(err, TemplateParseError::MissingFrontmatter { .. }));
    }

    #[test]
    fn missing_required_parameter_errors() {
        let t = parse("---\nname: x\nparameters:\n  - name: title\n    required: true\n---\n# {{title}}\n");
        let dir = tempdir().unwrap();
        let err = t
            .apply(&BTreeMap::new(), dir.path(), false)
            .unwrap_err();
        assert!(matches!(err, ApplyError::MissingParameter { name } if name == "title"));
    }

    #[test]
    fn resolves_default_with_substitution() {
        let t = parse("---\nname: x\nparameters:\n  - name: stamp\n    default: \"{{today}}\"\n---\nDate: {{stamp}}\n");
        let dir = tempdir().unwrap();
        let out = t.apply(&BTreeMap::new(), dir.path(), false).unwrap();
        let body = std::fs::read_to_string(&out).unwrap();
        // The date format is YYYY-MM-DD — just check it has a 4-digit year.
        assert!(
            body.contains("Date: 20") || body.contains("Date: 21"),
            "{body}"
        );
    }

    #[test]
    fn user_args_override_builtins() {
        let t = parse("---\nname: x\n---\n{{today}}\n");
        let dir = tempdir().unwrap();
        let mut args = BTreeMap::new();
        args.insert("today".to_string(), "1999-01-01".to_string());
        let out = t.apply(&args, dir.path(), false).unwrap();
        let body = std::fs::read_to_string(&out).unwrap();
        assert!(body.contains("1999-01-01"), "{body}");
    }

    #[test]
    fn renders_target_path_pattern() {
        let t = parse(
            "---\nname: daily\ntarget_path: daily/{{today}}.md\n---\nDay log.\n",
        );
        let dir = tempdir().unwrap();
        let out = t.apply(&BTreeMap::new(), dir.path(), false).unwrap();
        assert!(out.starts_with(dir.path().join("daily")));
        assert!(out.extension().is_some_and(|e| e == "md"));
    }

    #[test]
    fn collapses_double_md_extension() {
        let t = parse(
            "---\nname: x\ntarget_path: \"{{title}}.md\"\nparameters:\n  - name: title\n    required: true\n---\nbody\n",
        );
        let dir = tempdir().unwrap();
        let mut args = BTreeMap::new();
        args.insert("title".to_string(), "Notes.md".to_string());
        let out = t.apply(&args, dir.path(), false).unwrap();
        assert_eq!(out.file_name().unwrap(), "Notes.md");
    }

    #[test]
    fn refuses_overwrite_by_default() {
        let t = parse("---\nname: x\ntarget_path: x.md\n---\nv1\n");
        let dir = tempdir().unwrap();
        t.apply(&BTreeMap::new(), dir.path(), false).unwrap();
        let err = t.apply(&BTreeMap::new(), dir.path(), false).unwrap_err();
        assert!(matches!(err, ApplyError::AlreadyExists { .. }));
    }

    #[test]
    fn allows_overwrite_when_requested() {
        let t = parse("---\nname: x\ntarget_path: x.md\n---\nv1\n");
        let dir = tempdir().unwrap();
        t.apply(&BTreeMap::new(), dir.path(), false).unwrap();
        let t2 = parse("---\nname: x\ntarget_path: x.md\n---\nv2\n");
        t2.apply(&BTreeMap::new(), dir.path(), true).unwrap();
        let body = std::fs::read_to_string(dir.path().join("x.md")).unwrap();
        assert_eq!(body, "v2\n");
    }

    #[test]
    fn rejects_path_escape() {
        let t = parse("---\nname: x\ntarget_path: ../escape.md\n---\nbad\n");
        let dir = tempdir().unwrap();
        let err = t.apply(&BTreeMap::new(), dir.path(), false).unwrap_err();
        assert!(matches!(err, ApplyError::PathEscape { .. }));
    }

    #[test]
    fn unknown_param_falls_back_to_default() {
        let t = parse("---\nname: x\nparameters:\n  - name: optional\n    default: \"\"\n---\nVal: '{{optional}}'\n");
        let dir = tempdir().unwrap();
        let out = t.apply(&BTreeMap::new(), dir.path(), false).unwrap();
        let body = std::fs::read_to_string(&out).unwrap();
        assert!(body.contains("Val: ''"), "{body}");
    }
}
