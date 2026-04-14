//! CSS variable registry, variable maps, and `var(...)` substitution.
//!
//! The [`DEFAULT_VARIABLES`] constant encodes the light-mode base palette
//! described in PRD §1.2. Plugins extend this set by declaring additional
//! `--nx-<plugin>-*` variables in their manifest; the resolver then merges
//! them into the final [`VariableMap`].

use std::collections::BTreeMap;

use crate::{Result, ThemeError};

/// Required prefix for every Nexus CSS variable.
pub const VARIABLE_PREFIX: &str = "--nx-";

/// Maximum depth of nested `var(...)` substitution before we declare a cycle.
const MAX_SUBSTITUTION_DEPTH: usize = 16;

/// Ordered mapping of CSS variable name → value.
///
/// Uses a [`BTreeMap`] so that serialized output (and thus diffs and snapshot
/// tests) is deterministic regardless of insertion order.
pub type VariableMap = BTreeMap<String, String>;

/// Built-in light-mode variable defaults — the baseline cascade layer.
///
/// Derived verbatim from PRD §1.2. Values that reference other variables use
/// standard CSS `var(...)` syntax; the resolver substitutes them lazily.
pub const DEFAULT_VARIABLES: &[(&str, &str)] = &[
    // --- Base palette --------------------------------------------------
    ("--nx-color-primary", "#4A90E2"),
    ("--nx-color-primary-light", "#6BA3FF"),
    ("--nx-color-primary-dark", "#2E5CB8"),
    ("--nx-color-secondary", "#9B59B6"),
    ("--nx-color-success", "#27AE60"),
    ("--nx-color-warning", "#F39C12"),
    ("--nx-color-error", "#E74C3C"),
    ("--nx-color-info", "#3498DB"),
    ("--nx-color-neutral-50", "#FAFAFA"),
    ("--nx-color-neutral-100", "#F5F5F5"),
    ("--nx-color-neutral-200", "#E8E8E8"),
    ("--nx-color-neutral-300", "#D4D4D4"),
    ("--nx-color-neutral-400", "#A0A0A0"),
    ("--nx-color-neutral-500", "#737373"),
    ("--nx-color-neutral-600", "#525252"),
    ("--nx-color-neutral-700", "#3F3F3F"),
    ("--nx-color-neutral-800", "#262626"),
    ("--nx-color-neutral-900", "#0F0F0F"),
    // --- Surfaces ------------------------------------------------------
    ("--nx-bg-primary", "#FFFFFF"),
    ("--nx-bg-secondary", "#F8F9FA"),
    ("--nx-bg-tertiary", "#E8EAEF"),
    ("--nx-bg-overlay", "rgba(0, 0, 0, 0.5)"),
    ("--nx-bg-elevated", "#FFFFFF"),
    // --- Text ----------------------------------------------------------
    ("--nx-text-primary", "#1A1A1A"),
    ("--nx-text-secondary", "#4A4A4A"),
    ("--nx-text-tertiary", "#7A7A7A"),
    ("--nx-text-muted", "#A0A0A0"),
    ("--nx-text-inverted", "#FFFFFF"),
    // --- Interactive states -------------------------------------------
    ("--nx-interactive-hover", "rgba(74, 144, 226, 0.08)"),
    ("--nx-interactive-active", "rgba(74, 144, 226, 0.16)"),
    ("--nx-interactive-focus-ring", "2px solid var(--nx-color-primary)"),
    ("--nx-interactive-disabled", "rgba(0, 0, 0, 0.38)"),
    // --- Typography ---------------------------------------------------
    (
        "--nx-type-sans",
        "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, \"Helvetica Neue\", sans-serif",
    ),
    ("--nx-type-mono", "\"Monaco\", \"Courier New\", monospace"),
    ("--nx-type-serif", "\"Georgia\", \"Times New Roman\", serif"),
    ("--nx-type-h1-size", "32px"),
    ("--nx-type-h1-weight", "700"),
    ("--nx-type-h1-line-height", "1.2"),
    ("--nx-type-body-size", "14px"),
    ("--nx-type-body-weight", "400"),
    ("--nx-type-body-line-height", "1.5"),
    ("--nx-type-code-size", "12px"),
    ("--nx-type-code-weight", "400"),
    ("--nx-type-code-line-height", "1.4"),
    // --- Editor & syntax ----------------------------------------------
    ("--nx-editor-bg", "var(--nx-bg-primary)"),
    ("--nx-editor-gutter-bg", "var(--nx-bg-secondary)"),
    ("--nx-editor-line-number", "var(--nx-text-tertiary)"),
    ("--nx-editor-line-highlight", "rgba(74, 144, 226, 0.1)"),
    ("--nx-editor-cursor", "var(--nx-text-primary)"),
    ("--nx-syntax-keyword", "#E74C3C"),
    ("--nx-syntax-string", "#27AE60"),
    ("--nx-syntax-comment", "#95A5A6"),
    ("--nx-syntax-number", "#F39C12"),
    ("--nx-syntax-function", "#3498DB"),
    ("--nx-syntax-variable", "var(--nx-text-primary)"),
    // --- Spacing ------------------------------------------------------
    ("--nx-space-xs", "4px"),
    ("--nx-space-sm", "8px"),
    ("--nx-space-md", "16px"),
    ("--nx-space-lg", "32px"),
    ("--nx-space-xl", "64px"),
    // --- Effects ------------------------------------------------------
    ("--nx-shadow-sm", "0 1px 2px rgba(0, 0, 0, 0.05)"),
    ("--nx-shadow-md", "0 4px 6px rgba(0, 0, 0, 0.1)"),
    ("--nx-shadow-lg", "0 10px 15px rgba(0, 0, 0, 0.1)"),
    ("--nx-blur-sm", "blur(4px)"),
    ("--nx-blur-md", "blur(8px)"),
    // --- Graph & canvas -----------------------------------------------
    ("--nx-graph-node-bg", "var(--nx-bg-elevated)"),
    ("--nx-graph-node-border", "var(--nx-color-primary)"),
    ("--nx-graph-edge-stroke", "var(--nx-text-tertiary)"),
    ("--nx-graph-grid", "rgba(0, 0, 0, 0.05)"),
    ("--nx-graph-selection", "rgba(74, 144, 226, 0.2)"),
];

/// Returns the default variables as a fresh owned [`VariableMap`].
#[must_use]
pub fn default_variables() -> VariableMap {
    DEFAULT_VARIABLES
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

/// Validate that `name` starts with the required `--nx-` prefix.
///
/// # Errors
/// Returns [`ThemeError::InvalidVariableName`] if the name is not prefixed.
pub fn validate_variable_name(name: &str) -> Result<()> {
    if name.starts_with(VARIABLE_PREFIX) {
        Ok(())
    } else {
        Err(ThemeError::InvalidVariableName(name.to_string()))
    }
}

/// Substitute every `var(--nx-foo)` reference in `value` with the concrete
/// value from `vars`.
///
/// Unknown variables are left as-is so downstream CSS still sees a valid
/// `var(...)` fallback. Detects cycles up to [`MAX_SUBSTITUTION_DEPTH`].
///
/// # Errors
/// Returns [`ThemeError::CircularReference`] if the same variable is
/// substituted recursively more than [`MAX_SUBSTITUTION_DEPTH`] times.
pub fn substitute(value: &str, vars: &VariableMap) -> Result<String> {
    substitute_inner(value, vars, 0)
}

fn substitute_inner(value: &str, vars: &VariableMap, depth: usize) -> Result<String> {
    if depth > MAX_SUBSTITUTION_DEPTH {
        return Err(ThemeError::CircularReference(value.to_string()));
    }

    let mut out = String::with_capacity(value.len());
    let mut remaining = value;

    while let Some(idx) = remaining.find("var(") {
        out.push_str(&remaining[..idx]);
        let after = &remaining[idx + 4..];

        let Some(end_rel) = find_matching_paren(after) else {
            out.push_str("var(");
            out.push_str(after);
            return Ok(out);
        };

        let inside = &after[..end_rel];
        let name_end = inside.find(',').unwrap_or(inside.len());
        let name = inside[..name_end].trim();

        if let Some(raw) = vars.get(name) {
            let substituted = substitute_inner(raw, vars, depth + 1)?;
            out.push_str(&substituted);
        } else {
            // Unknown variable — preserve the original `var(...)` call so the
            // browser can still honour any CSS fallback.
            out.push_str("var(");
            out.push_str(inside);
            out.push(')');
        }

        remaining = &after[end_rel + 1..];
    }

    out.push_str(remaining);
    Ok(out)
}

/// Scans `s` starting just after a `var(` and returns the offset of the
/// matching closing paren (relative to the start of `s`).
fn find_matching_paren(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1_usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_well_formed() {
        let vars = default_variables();
        assert!(vars.contains_key("--nx-color-primary"));
        for name in vars.keys() {
            validate_variable_name(name).unwrap();
        }
        // Spot-check: the PRD lists these explicitly.
        assert_eq!(vars["--nx-space-md"], "16px");
        assert_eq!(vars["--nx-color-success"], "#27AE60");
    }

    #[test]
    fn substitute_resolves_known_references() {
        let vars = default_variables();
        let resolved = substitute("var(--nx-bg-primary)", &vars).unwrap();
        assert_eq!(resolved, "#FFFFFF");
    }

    #[test]
    fn substitute_resolves_nested_references() {
        let mut vars = default_variables();
        vars.insert(
            "--nx-editor-bg".into(),
            "var(--nx-bg-primary)".into(),
        );
        let resolved = substitute("var(--nx-editor-bg)", &vars).unwrap();
        assert_eq!(resolved, "#FFFFFF");
    }

    #[test]
    fn substitute_leaves_unknown_variables_intact() {
        let vars = default_variables();
        let out = substitute("var(--nx-unknown-thing)", &vars).unwrap();
        assert_eq!(out, "var(--nx-unknown-thing)");
    }

    #[test]
    fn substitute_detects_cycles() {
        let mut vars = VariableMap::new();
        vars.insert("--nx-a".into(), "var(--nx-b)".into());
        vars.insert("--nx-b".into(), "var(--nx-a)".into());
        let err = substitute("var(--nx-a)", &vars).unwrap_err();
        assert!(matches!(err, ThemeError::CircularReference(_)));
    }

    #[test]
    fn substitute_handles_plain_text() {
        let vars = default_variables();
        let out = substitute("#FF00FF", &vars).unwrap();
        assert_eq!(out, "#FF00FF");
    }

    #[test]
    fn validate_rejects_bad_prefix() {
        assert!(validate_variable_name("--other-thing").is_err());
        assert!(validate_variable_name("--nx-color-primary").is_ok());
    }
}
