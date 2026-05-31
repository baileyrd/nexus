//! `{{var}}` substitution. Intentionally tiny — no conditionals, no loops,
//! no escapes. Templates that need real templating should use a community
//! plugin.
//!
//! - `{{name}}` — substitute the value of `name` from the value map
//! - Unknown vars produce a [`SubstitutionError::UnknownVariable`]
//! - Whitespace inside braces is allowed: `{{ name }}` works
//! - Literal `{{` can be emitted by writing `{{!}}` (escape)

use std::collections::BTreeMap;

/// Substitution failure.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SubstitutionError {
    /// A `{{var}}` referenced a name not in the value map.
    #[error("unknown template variable: '{name}'")]
    UnknownVariable {
        /// The variable name.
        name: String,
    },
    /// A `{{` was opened but never closed on the same line, or two `{{`
    /// were nested without a `}}` between them.
    #[error("malformed template tag at line {line}")]
    MalformedTag {
        /// 1-based line number where the parse failed.
        line: usize,
    },
}

/// Render a template string by substituting `{{var}}` placeholders with
/// values from `values`.
///
/// # Errors
/// Returns [`SubstitutionError::UnknownVariable`] when a placeholder is not
/// present in `values`, and [`SubstitutionError::MalformedTag`] when braces
/// are unbalanced.
pub fn render(input: &str, values: &BTreeMap<String, String>) -> Result<String, SubstitutionError> {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut line = 1;

    while i < input.len() {
        if i + 1 < input.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Escape: {{!}} → literal `{{`.
            if i + 4 <= input.len() && &input[i..i + 5] == "{{!}}" {
                out.push_str("{{");
                i += 5;
                continue;
            }
            // Find closing `}}`.
            let close = find_close(input, i + 2).ok_or(SubstitutionError::MalformedTag { line })?;
            let inner = input[i + 2..close].trim();
            if inner.is_empty() || inner.contains('{') {
                return Err(SubstitutionError::MalformedTag { line });
            }
            let value = values
                .get(inner)
                .ok_or_else(|| SubstitutionError::UnknownVariable {
                    name: inner.to_string(),
                })?;
            out.push_str(value);
            i = close + 2;
            continue;
        }
        if bytes[i] == b'\n' {
            line += 1;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    Ok(out)
}

fn find_close(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i + 1 < s.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        if bytes[i] == b'\n' {
            return None;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn substitutes_simple_variable() {
        let v = vals(&[("name", "World")]);
        assert_eq!(render("Hello, {{name}}!", &v).unwrap(), "Hello, World!");
    }

    #[test]
    fn allows_whitespace_inside_braces() {
        let v = vals(&[("name", "World")]);
        assert_eq!(render("Hi {{ name }}.", &v).unwrap(), "Hi World.");
    }

    #[test]
    fn unknown_variable_is_an_error() {
        let v = vals(&[]);
        assert_eq!(
            render("{{missing}}", &v).unwrap_err(),
            SubstitutionError::UnknownVariable {
                name: "missing".to_string(),
            }
        );
    }

    #[test]
    fn unclosed_tag_is_malformed() {
        let v = vals(&[]);
        assert!(matches!(
            render("Hi {{name", &v).unwrap_err(),
            SubstitutionError::MalformedTag { line: 1 }
        ));
    }

    #[test]
    fn nested_open_braces_is_malformed() {
        let v = vals(&[("a", "1")]);
        assert!(matches!(
            render("{{ a {{ b }}}}", &v).unwrap_err(),
            SubstitutionError::MalformedTag { .. }
        ));
    }

    #[test]
    fn escape_emits_literal_braces() {
        let v = vals(&[]);
        assert_eq!(render("{{!}}name{{!}}", &v).unwrap(), "{{name{{");
    }

    #[test]
    fn line_count_advances_through_newlines() {
        let v = vals(&[]);
        let err = render("line 1\nline 2\n{{x", &v).unwrap_err();
        assert_eq!(err, SubstitutionError::MalformedTag { line: 3 });
    }

    #[test]
    fn multiple_substitutions_in_one_string() {
        let v = vals(&[("a", "1"), ("b", "2"), ("c", "3")]);
        assert_eq!(render("{{a}}-{{b}}-{{c}}", &v).unwrap(), "1-2-3");
    }
}
