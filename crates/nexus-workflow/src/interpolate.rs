//! Variable interpolation for workflow steps (PRD-16 §6).
//!
//! Expands `${PATH}` placeholders in string values using a flat
//! variables map keyed by dotted paths (e.g. `trigger.file.path`,
//! `inputs.dir`, `steps.Foo.output`). The runtime builds the map
//! from the trigger payload + input values and passes it into
//! [`crate::run_workflow_with_variables`], which calls
//! [`interpolate_step`] on each step before dispatch.
//!
//! # Syntax
//!
//! - `${path.to.value}` — replaced by the stringified variable.
//! - `$${literal}` — escape; emitted as `${literal}` with no lookup.
//! - Missing variables pass through unchanged (`${unset}` stays
//!   literal) so a misconfigured trigger surfaces in the executed
//!   command rather than via a silent empty string.
//!
//! Substitution only happens inside string leaves; numbers, bools,
//! and table keys are untouched. Nested tables / arrays are walked
//! recursively.

use std::collections::BTreeMap;

use crate::Step;

/// Map of dotted variable paths → values. Values are TOML so callers
/// can stash structured data (e.g. `trigger.payload`) even though
/// only scalars meaningfully interpolate into strings.
pub type VariableMap = BTreeMap<String, toml::Value>;

/// Walk a [`Step`]'s `extra` fields and substitute `${path}`
/// placeholders in any string leaf.
pub fn interpolate_step(step: &mut Step, vars: &VariableMap) {
    for value in step.extra.values_mut() {
        *value = substitute(value, vars);
    }
}

/// Recursively substitute placeholders in a TOML value.
#[must_use]
pub fn substitute(value: &toml::Value, vars: &VariableMap) -> toml::Value {
    match value {
        toml::Value::String(s) => toml::Value::String(substitute_string(s, vars)),
        toml::Value::Array(items) => {
            toml::Value::Array(items.iter().map(|v| substitute(v, vars)).collect())
        }
        toml::Value::Table(t) => {
            let mut out = toml::value::Table::new();
            for (k, v) in t {
                out.insert(k.clone(), substitute(v, vars));
            }
            toml::Value::Table(out)
        }
        other => other.clone(),
    }
}

/// Substitute every well-formed `${path}` in a string.
///
/// Parser rules:
///
/// - `$${...}` emits a literal `${...}` (escape).
/// - `${NAME}` — `NAME` may contain ASCII letters, digits, `.`, `_`,
///   and `-`. On unknown variable, the entire `${NAME}` is preserved
///   verbatim. On unterminated `${` (no closing `}`), the `${` is
///   preserved verbatim and parsing continues after it.
#[must_use]
pub fn substitute_string(input: &str, vars: &VariableMap) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        // $$ → literal $
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
            out.push('$');
            i += 2;
            continue;
        }
        // ${...}
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = find_closing_brace(&bytes[i + 2..]) {
                let name = &input[i + 2..i + 2 + end];
                if is_valid_var_name(name) {
                    match vars.get(name) {
                        Some(v) => {
                            out.push_str(&toml_value_to_string(v));
                        }
                        None => {
                            // Unknown var — preserve verbatim so the
                            // caller can see what wasn't interpolated.
                            out.push_str(&input[i..i + 3 + end]);
                        }
                    }
                    i += 3 + end;
                    continue;
                }
                // Invalid name — fall through to literal $ handling.
            }
            // Unterminated / invalid — emit the $ verbatim and keep scanning.
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_closing_brace(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|&b| b == b'}')
}

fn is_valid_var_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn toml_value_to_string(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        // For arrays / tables, fall back to the TOML debug
        // representation — interpolating composite values is an
        // anti-pattern but we don't silently lose data.
        toml::Value::Array(_) | toml::Value::Table(_) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, toml::Value)]) -> VariableMap {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn substitutes_single_string_var() {
        let v = vars(&[("trigger.path", toml::Value::String("notes/a.md".into()))]);
        assert_eq!(
            substitute_string("file=${trigger.path}", &v),
            "file=notes/a.md"
        );
    }

    #[test]
    fn substitutes_multiple_vars() {
        let v = vars(&[
            ("trigger.path", toml::Value::String("a.md".into())),
            ("inputs.dir", toml::Value::String("notes/".into())),
        ]);
        assert_eq!(
            substitute_string("${inputs.dir}${trigger.path}", &v),
            "notes/a.md"
        );
    }

    #[test]
    fn preserves_unknown_vars_verbatim() {
        let v = VariableMap::new();
        assert_eq!(
            substitute_string("hello ${trigger.name} world", &v),
            "hello ${trigger.name} world"
        );
    }

    #[test]
    fn escaped_double_dollar_emits_literal_dollar() {
        let v = vars(&[("x", toml::Value::String("Y".into()))]);
        assert_eq!(substitute_string("cost: $${x}", &v), "cost: ${x}");
    }

    #[test]
    fn stringifies_numeric_and_bool_vars() {
        let v = vars(&[
            ("count", toml::Value::Integer(42)),
            ("ratio", toml::Value::Float(3.5)),
            ("enabled", toml::Value::Boolean(true)),
        ]);
        assert_eq!(
            substitute_string("${count}/${ratio}/${enabled}", &v),
            "42/3.5/true"
        );
    }

    #[test]
    fn unterminated_placeholder_kept_verbatim() {
        let v = VariableMap::new();
        assert_eq!(
            substitute_string("oops ${unterminated", &v),
            "oops ${unterminated"
        );
    }

    #[test]
    fn invalid_name_chars_skip_substitution() {
        let v = vars(&[("trigger path", toml::Value::String("X".into()))]);
        // Space inside name → not a valid var → passed through.
        assert_eq!(substitute_string("${trigger path}", &v), "${trigger path}");
    }

    #[test]
    fn recurses_into_arrays_and_tables() {
        let v = vars(&[("trigger.path", toml::Value::String("X".into()))]);
        let mut table = toml::value::Table::new();
        table.insert(
            "path".into(),
            toml::Value::String("p=${trigger.path}".into()),
        );
        let input = toml::Value::Array(vec![
            toml::Value::String("${trigger.path}".into()),
            toml::Value::Table(table),
            toml::Value::Integer(3),
        ]);
        let out = substitute(&input, &v);
        let arr = out.as_array().unwrap();
        assert_eq!(arr[0].as_str(), Some("X"));
        assert_eq!(
            arr[1]
                .as_table()
                .unwrap()
                .get("path")
                .and_then(|v| v.as_str()),
            Some("p=X")
        );
        assert_eq!(arr[2].as_integer(), Some(3));
    }

    #[test]
    fn interpolate_step_rewrites_extras() {
        use crate::Step;
        let mut step = Step {
            name: Some("S".into()),
            step_type: "ipc".into(),
            parallel: false,
            async_submit: false,
            on_error: None,
            max_retries: None,
            retry_backoff: None,
            retry_initial_delay_ms: None,
            retry_max_delay_ms: None,
            retry_jitter: None,
            extra: BTreeMap::new(),
        };
        step.extra.insert(
            "target".into(),
            toml::Value::String("com.nexus.storage".into()),
        );
        step.extra
            .insert("command".into(), toml::Value::String("read_file".into()));
        let mut args = toml::value::Table::new();
        args.insert("path".into(), toml::Value::String("${trigger.path}".into()));
        step.extra.insert("args".into(), toml::Value::Table(args));

        let v = vars(&[("trigger.path", toml::Value::String("notes/x.md".into()))]);
        interpolate_step(&mut step, &v);

        let path = step
            .extra
            .get("args")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("path"))
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(path, "notes/x.md");
    }
}
