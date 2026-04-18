//! Workflow condition evaluator (PRD-16 §4 `[condition]`).
//!
//! A [`Condition`] is a typed predicate gating the rest of the
//! workflow run. The condition table is already round-tripped by the
//! parser; this module turns the `type` discriminator into a concrete
//! yes/no answer.
//!
//! # Supported condition types
//!
//! Terminal:
//! - `always`  — always true.
//! - `never`   — always false.
//! - `equals`  — `left == right`, both resolved via [`resolve_operand`].
//! - `regex_match` — `source` matches `pattern` (regex-lite flavour).
//! - `file_exists` — `path` (resolved) exists on the filesystem under
//!   `EvaluationContext::forge_root` when relative, or absolutely.
//!
//! Combinators:
//! - `and` / `or` — takes a `conditions = [...]` sub-array. `and`
//!   short-circuits on the first false; `or` short-circuits on the
//!   first true. Empty arrays evaluate to `true` / `false`
//!   respectively (the identity for each operator).
//! - `not` — takes a single `condition = { ... }` sub-table.
//!
//! # Operand resolution
//!
//! Every string-typed field (`source`, `left`, `right`, `path`,
//! `pattern`) flows through [`crate::interpolate::substitute_string`]
//! first, so `"${trigger.path}"` / `"${inputs.dir}"` all expand
//! against the same [`VariableMap`] the executor uses.
//!
//! # Error handling
//!
//! Unknown types, missing required fields, and invalid regex all
//! surface as [`ConditionError`]. The executor treats a
//! [`ConditionError`] as a run-level failure — the run never
//! dispatches any step.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::interpolate::{substitute_string, VariableMap};
use crate::Condition;

/// Everything a condition evaluator needs beyond the condition itself.
#[derive(Debug, Clone, Default)]
pub struct EvaluationContext {
    /// Forge root — relative `file_exists` paths resolve against it.
    /// Absolute paths bypass this and hit the filesystem directly.
    pub forge_root: Option<PathBuf>,
    /// Variable map for `${...}` expansion inside condition fields.
    pub variables: VariableMap,
}

/// Errors raised while evaluating a condition.
#[derive(Debug, Error)]
pub enum ConditionError {
    /// `type = "..."` is not one of the supported variants.
    #[error("unknown condition type: `{0}`")]
    UnknownType(String),
    /// A required field for the selected type is absent or wrong-shaped.
    #[error("condition `{condition_type}` missing or invalid field `{field}`")]
    MissingField {
        /// `condition_type` that failed (for error-rendering context).
        condition_type: String,
        /// The field name that was missing / wrong-typed.
        field: &'static str,
    },
    /// Regex compilation failed.
    #[error("condition `regex_match`: invalid pattern `{pattern}`: {error}")]
    InvalidRegex {
        /// The offending pattern (post-interpolation).
        pattern: String,
        /// The compiler's error message.
        error: String,
    },
}

/// Evaluate a [`Condition`] against the given context.
///
/// # Errors
///
/// Returns a [`ConditionError`] for unknown types, malformed fields,
/// or bad regexes. Callers typically surface this as a run-level
/// failure — if the gate can't be evaluated, the gate is closed.
pub fn evaluate_condition(
    cond: &Condition,
    ctx: &EvaluationContext,
) -> Result<bool, ConditionError> {
    match cond.condition_type.as_str() {
        "always" => Ok(true),
        "never" => Ok(false),
        "and" => eval_combinator(cond, ctx, true),
        "or" => eval_combinator(cond, ctx, false),
        "not" => eval_not(cond, ctx),
        "equals" => eval_equals(cond, ctx),
        "regex_match" => eval_regex_match(cond, ctx),
        "file_exists" => eval_file_exists(cond, ctx),
        other => Err(ConditionError::UnknownType(other.to_string())),
    }
}

fn eval_combinator(
    cond: &Condition,
    ctx: &EvaluationContext,
    is_and: bool,
) -> Result<bool, ConditionError> {
    let arr = cond
        .extra
        .get("conditions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConditionError::MissingField {
            condition_type: cond.condition_type.clone(),
            field: "conditions",
        })?;
    // `and` identity = true (empty); `or` identity = false (empty).
    for item in arr {
        let sub = parse_subcondition(item, &cond.condition_type, "conditions")?;
        let v = evaluate_condition(&sub, ctx)?;
        if is_and && !v {
            return Ok(false);
        }
        if !is_and && v {
            return Ok(true);
        }
    }
    Ok(is_and)
}

fn eval_not(cond: &Condition, ctx: &EvaluationContext) -> Result<bool, ConditionError> {
    let inner = cond
        .extra
        .get("condition")
        .ok_or_else(|| ConditionError::MissingField {
            condition_type: cond.condition_type.clone(),
            field: "condition",
        })?;
    let sub = parse_subcondition(inner, &cond.condition_type, "condition")?;
    Ok(!evaluate_condition(&sub, ctx)?)
}

fn eval_equals(cond: &Condition, ctx: &EvaluationContext) -> Result<bool, ConditionError> {
    let left = resolve_operand(cond, "left", ctx)?;
    let right = resolve_operand(cond, "right", ctx)?;
    Ok(left == right)
}

fn eval_regex_match(cond: &Condition, ctx: &EvaluationContext) -> Result<bool, ConditionError> {
    let source = resolve_operand(cond, "source", ctx)?;
    let pattern = resolve_operand(cond, "pattern", ctx)?;
    let re = regex_lite::Regex::new(&pattern).map_err(|e| ConditionError::InvalidRegex {
        pattern: pattern.clone(),
        error: e.to_string(),
    })?;
    Ok(re.is_match(&source))
}

fn eval_file_exists(cond: &Condition, ctx: &EvaluationContext) -> Result<bool, ConditionError> {
    let raw = resolve_operand(cond, "path", ctx)?;
    let path = Path::new(&raw);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match &ctx.forge_root {
            Some(root) => root.join(path),
            None => path.to_path_buf(),
        }
    };
    Ok(absolute.exists())
}

/// Pull a string field off the condition table, interpolating
/// variables.
fn resolve_operand(
    cond: &Condition,
    field: &'static str,
    ctx: &EvaluationContext,
) -> Result<String, ConditionError> {
    let raw = cond
        .extra
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ConditionError::MissingField {
            condition_type: cond.condition_type.clone(),
            field,
        })?;
    Ok(substitute_string(raw, &ctx.variables))
}

/// Deserialize a sub-condition out of a [`toml::Value`] (as nested
/// inside `and` / `or` / `not`). Sub-conditions have the same shape
/// as the top-level `[condition]` table.
fn parse_subcondition(
    value: &toml::Value,
    parent_type: &str,
    parent_field: &'static str,
) -> Result<Condition, ConditionError> {
    let table = value
        .as_table()
        .ok_or_else(|| ConditionError::MissingField {
            condition_type: parent_type.to_string(),
            field: parent_field,
        })?;
    let condition_type = table
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ConditionError::MissingField {
            condition_type: parent_type.to_string(),
            field: "type",
        })?
        .to_string();
    let mut extra = std::collections::BTreeMap::new();
    for (k, v) in table {
        if k != "type" {
            extra.insert(k.clone(), v.clone());
        }
    }
    Ok(Condition {
        condition_type,
        extra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cond(tz: &str, extras: &[(&str, toml::Value)]) -> Condition {
        let mut extra = std::collections::BTreeMap::new();
        for (k, v) in extras {
            extra.insert((*k).to_string(), v.clone());
        }
        Condition {
            condition_type: tz.to_string(),
            extra,
        }
    }

    fn empty_ctx() -> EvaluationContext {
        EvaluationContext::default()
    }

    #[test]
    fn always_is_true_never_is_false() {
        assert!(evaluate_condition(&cond("always", &[]), &empty_ctx()).unwrap());
        assert!(!evaluate_condition(&cond("never", &[]), &empty_ctx()).unwrap());
    }

    #[test]
    fn unknown_type_errors() {
        let err = evaluate_condition(&cond("bogus", &[]), &empty_ctx()).unwrap_err();
        assert!(matches!(err, ConditionError::UnknownType(s) if s == "bogus"));
    }

    #[test]
    fn equals_compares_after_interpolation() {
        let c = cond(
            "equals",
            &[
                ("left", toml::Value::String("${trigger.path}".into())),
                ("right", toml::Value::String("notes/a.md".into())),
            ],
        );
        let mut ctx = empty_ctx();
        ctx.variables.insert(
            "trigger.path".into(),
            toml::Value::String("notes/a.md".into()),
        );
        assert!(evaluate_condition(&c, &ctx).unwrap());

        ctx.variables.insert(
            "trigger.path".into(),
            toml::Value::String("other.md".into()),
        );
        assert!(!evaluate_condition(&c, &ctx).unwrap());
    }

    #[test]
    fn equals_missing_field_errors() {
        let c = cond(
            "equals",
            &[("left", toml::Value::String("x".into()))],
        );
        let err = evaluate_condition(&c, &empty_ctx()).unwrap_err();
        assert!(matches!(
            err,
            ConditionError::MissingField { field: "right", .. }
        ));
    }

    #[test]
    fn regex_match_applies_pattern_to_source() {
        let c = cond(
            "regex_match",
            &[
                ("source", toml::Value::String("${trigger.path}".into())),
                ("pattern", toml::Value::String(r"^notes/.*\.md$".into())),
            ],
        );
        let mut ctx = empty_ctx();
        ctx.variables.insert(
            "trigger.path".into(),
            toml::Value::String("notes/a.md".into()),
        );
        assert!(evaluate_condition(&c, &ctx).unwrap());

        ctx.variables.insert(
            "trigger.path".into(),
            toml::Value::String("other/a.txt".into()),
        );
        assert!(!evaluate_condition(&c, &ctx).unwrap());
    }

    #[test]
    fn regex_match_invalid_pattern_errors() {
        let c = cond(
            "regex_match",
            &[
                ("source", toml::Value::String("x".into())),
                ("pattern", toml::Value::String("[unterminated".into())),
            ],
        );
        let err = evaluate_condition(&c, &empty_ctx()).unwrap_err();
        assert!(matches!(err, ConditionError::InvalidRegex { .. }));
    }

    #[test]
    fn file_exists_resolves_relative_to_forge_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.md"), "x").unwrap();
        let c = cond(
            "file_exists",
            &[("path", toml::Value::String("a.md".into()))],
        );
        let ctx = EvaluationContext {
            forge_root: Some(tmp.path().to_path_buf()),
            variables: VariableMap::new(),
        };
        assert!(evaluate_condition(&c, &ctx).unwrap());

        let c_missing = cond(
            "file_exists",
            &[("path", toml::Value::String("ghost.md".into()))],
        );
        assert!(!evaluate_condition(&c_missing, &ctx).unwrap());
    }

    #[test]
    fn and_short_circuits_on_first_false() {
        let c = cond(
            "and",
            &[(
                "conditions",
                toml::Value::Array(vec![
                    toml::Value::Table({
                        let mut t = toml::value::Table::new();
                        t.insert("type".into(), toml::Value::String("always".into()));
                        t
                    }),
                    toml::Value::Table({
                        let mut t = toml::value::Table::new();
                        t.insert("type".into(), toml::Value::String("never".into()));
                        t
                    }),
                ]),
            )],
        );
        assert!(!evaluate_condition(&c, &empty_ctx()).unwrap());
    }

    #[test]
    fn or_short_circuits_on_first_true() {
        let c = cond(
            "or",
            &[(
                "conditions",
                toml::Value::Array(vec![
                    toml::Value::Table({
                        let mut t = toml::value::Table::new();
                        t.insert("type".into(), toml::Value::String("never".into()));
                        t
                    }),
                    toml::Value::Table({
                        let mut t = toml::value::Table::new();
                        t.insert("type".into(), toml::Value::String("always".into()));
                        t
                    }),
                ]),
            )],
        );
        assert!(evaluate_condition(&c, &empty_ctx()).unwrap());
    }

    #[test]
    fn empty_and_is_true_empty_or_is_false() {
        let c_and = cond("and", &[("conditions", toml::Value::Array(vec![]))]);
        let c_or = cond("or", &[("conditions", toml::Value::Array(vec![]))]);
        assert!(evaluate_condition(&c_and, &empty_ctx()).unwrap());
        assert!(!evaluate_condition(&c_or, &empty_ctx()).unwrap());
    }

    #[test]
    fn not_inverts_the_inner_condition() {
        let mut inner = toml::value::Table::new();
        inner.insert("type".into(), toml::Value::String("always".into()));
        let c = cond("not", &[("condition", toml::Value::Table(inner))]);
        assert!(!evaluate_condition(&c, &empty_ctx()).unwrap());
    }
}
