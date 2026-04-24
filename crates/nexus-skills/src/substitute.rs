//! Parameter substitution for skill bodies (PRD-13 §2.3 `parameters`).
//!
//! Skills declare typed parameters in frontmatter. [`render`] walks
//! the markdown body and replaces `{{ name }}` tokens with caller-
//! supplied values, falling back to per-parameter `default`s. Only
//! parameters *declared* by the skill are substituted; unknown
//! `{{ foo }}` tokens pass through untouched so prompt templates
//! that already use Jinja-like placeholders keep working.
//!
//! Validation is deliberately light — this layer enforces:
//!
//! - Required parameters (no default, no supplied value) error.
//! - Enum parameters reject values outside their `values:` list.
//!
//! Heavier typing (number coercion, list shape) is left to callers
//! who want it; the PRD treats parameters as a hint, not a schema.

use std::collections::HashMap;

use thiserror::Error;

use crate::Skill;

/// Errors from [`render`].
#[derive(Debug, Error, PartialEq)]
pub enum SubstitutionError {
    /// A declared parameter was not supplied and has no default.
    #[error("missing required parameter: {0}")]
    MissingParameter(String),
    /// Supplied value was not in the enum's allowed `values:` list.
    #[error("parameter '{name}' = {got:?} is not one of {allowed:?}")]
    EnumMismatch {
        /// Parameter name.
        name: String,
        /// Stringified supplied value.
        got: String,
        /// Stringified allowed values.
        allowed: Vec<String>,
    },
}

/// Render `skill.body`, substituting `{{ name }}` tokens for declared
/// parameters. Values win over defaults; undeclared tokens pass
/// through. Returns the rendered body.
///
/// # Errors
///
/// - [`SubstitutionError::MissingParameter`] — declared parameter
///   with no supplied value and no default.
/// - [`SubstitutionError::EnumMismatch`] — enum parameter supplied a
///   value outside its `values:` list.
pub fn render<S: std::hash::BuildHasher>(
    skill: &Skill,
    values: &HashMap<String, serde_yaml::Value, S>,
) -> Result<String, SubstitutionError> {
    let resolved = resolve(skill, values)?;
    Ok(replace_tokens(&skill.body, &resolved))
}

fn resolve<S: std::hash::BuildHasher>(
    skill: &Skill,
    values: &HashMap<String, serde_yaml::Value, S>,
) -> Result<HashMap<String, String>, SubstitutionError> {
    let mut out = HashMap::with_capacity(skill.meta.parameters.len());
    for param in &skill.meta.parameters {
        let supplied = values.get(&param.name);
        let Some(chosen) = supplied.or(param.default.as_ref()) else {
            return Err(SubstitutionError::MissingParameter(param.name.clone()));
        };
        if param.param_type == "enum" && !param.values.is_empty() {
            let matches = param
                .values
                .iter()
                .any(|allowed| yaml_eq(allowed, chosen));
            if !matches {
                return Err(SubstitutionError::EnumMismatch {
                    name: param.name.clone(),
                    got: stringify_yaml(chosen),
                    allowed: param.values.iter().map(stringify_yaml).collect(),
                });
            }
        }
        out.insert(param.name.clone(), stringify_yaml(chosen));
    }
    Ok(out)
}

fn replace_tokens(body: &str, resolved: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = find_close(body, i + 2) {
                let inner = body[i + 2..end].trim();
                if let Some(value) = resolved.get(inner) {
                    out.push_str(value);
                    i = end + 2;
                    continue;
                }
            }
        }
        out.push(body[i..].chars().next().unwrap());
        i += body[i..].chars().next().unwrap().len_utf8();
    }
    out
}

fn find_close(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
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

fn yaml_eq(a: &serde_yaml::Value, b: &serde_yaml::Value) -> bool {
    stringify_yaml(a) == stringify_yaml(b)
}

fn stringify_yaml(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::Null => String::new(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SkillMeta, SkillParameter};

    fn skill_with(body: &str, params: Vec<SkillParameter>) -> Skill {
        Skill {
            meta: SkillMeta {
                name: "t".into(),
                id: "t".into(),
                description: String::new(),
                version: "0.0.1".into(),
                author: String::new(),
                created: String::new(),
                tags: vec![],
                applicable_contexts: vec![],
                triggers: vec![],
                parameters: params,
                depends_on: vec![],
                restrictions: None,
                output_format: None,
                visibility: None,
                extra: std::collections::BTreeMap::default(),
            },
            body: body.into(),
        }
    }

    fn str_val(s: &str) -> serde_yaml::Value {
        serde_yaml::Value::String(s.into())
    }

    #[test]
    fn substitutes_with_supplied_value() {
        let skill = skill_with(
            "Hello {{ who }}!",
            vec![SkillParameter {
                name: "who".into(),
                param_type: "string".into(),
                description: None,
                values: vec![],
                items: None,
                default: None,
            }],
        );
        let mut values = HashMap::new();
        values.insert("who".into(), str_val("world"));
        assert_eq!(render(&skill, &values).unwrap(), "Hello world!");
    }

    #[test]
    fn falls_back_to_default() {
        let skill = skill_with(
            "mode={{tone}}",
            vec![SkillParameter {
                name: "tone".into(),
                param_type: "string".into(),
                description: None,
                values: vec![],
                items: None,
                default: Some(str_val("friendly")),
            }],
        );
        assert_eq!(render(&skill, &HashMap::new()).unwrap(), "mode=friendly");
    }

    #[test]
    fn missing_parameter_errors() {
        let skill = skill_with(
            "x",
            vec![SkillParameter {
                name: "who".into(),
                param_type: "string".into(),
                description: None,
                values: vec![],
                items: None,
                default: None,
            }],
        );
        let err = render(&skill, &HashMap::new()).unwrap_err();
        assert_eq!(err, SubstitutionError::MissingParameter("who".into()));
    }

    #[test]
    fn enum_rejects_out_of_range() {
        let skill = skill_with(
            "t={{ tone }}",
            vec![SkillParameter {
                name: "tone".into(),
                param_type: "enum".into(),
                description: None,
                values: vec![str_val("formal"), str_val("casual")],
                items: None,
                default: None,
            }],
        );
        let mut values = HashMap::new();
        values.insert("tone".into(), str_val("angry"));
        let err = render(&skill, &values).unwrap_err();
        match err {
            SubstitutionError::EnumMismatch { name, got, .. } => {
                assert_eq!(name, "tone");
                assert_eq!(got, "angry");
            }
            SubstitutionError::MissingParameter(_) => panic!("wrong error"),
        }
    }

    #[test]
    fn enum_accepts_valid_value() {
        let skill = skill_with(
            "t={{ tone }}",
            vec![SkillParameter {
                name: "tone".into(),
                param_type: "enum".into(),
                description: None,
                values: vec![str_val("formal"), str_val("casual")],
                items: None,
                default: None,
            }],
        );
        let mut values = HashMap::new();
        values.insert("tone".into(), str_val("casual"));
        assert_eq!(render(&skill, &values).unwrap(), "t=casual");
    }

    #[test]
    fn undeclared_tokens_pass_through() {
        let skill = skill_with("keep {{ unknown }} as-is", vec![]);
        assert_eq!(
            render(&skill, &HashMap::new()).unwrap(),
            "keep {{ unknown }} as-is"
        );
    }

    #[test]
    fn handles_tokens_without_whitespace() {
        let skill = skill_with(
            "{{x}}+{{ x }}",
            vec![SkillParameter {
                name: "x".into(),
                param_type: "string".into(),
                description: None,
                values: vec![],
                items: None,
                default: Some(str_val("1")),
            }],
        );
        assert_eq!(render(&skill, &HashMap::new()).unwrap(), "1+1");
    }

    #[test]
    fn does_not_cross_newline_in_token() {
        let skill = skill_with(
            "{{ broken\n}} {{ who }}",
            vec![SkillParameter {
                name: "who".into(),
                param_type: "string".into(),
                description: None,
                values: vec![],
                items: None,
                default: Some(str_val("me")),
            }],
        );
        assert_eq!(render(&skill, &HashMap::new()).unwrap(), "{{ broken\n}} me");
    }
}
