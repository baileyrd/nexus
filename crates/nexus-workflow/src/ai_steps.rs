//! BL-028d — pure helpers for the `ai_prompt` / `ai_decision` step
//! types.
//!
//! The step types route through `com.nexus.ai::ask` (the same RAG
//! handler the digest pipeline uses). Everything in this module is
//! kernel-free so it can be unit-tested without an `ipc_call` stub —
//! the kernel-aware glue lives in
//! [`crate::core_plugin::KernelActionDispatcher`].
//!
//! # `ai_prompt`
//!
//! Send a free-form prompt to the AI provider, return its full
//! `RagResponse` JSON as the step response. Workflow shape:
//!
//! ```toml
//! [[steps]]
//! type = "ai_prompt"
//! prompt = "Summarise the latest commit message: ${trigger.head}"
//! limit = 5  # optional; passed through to ask's RAG context
//! ```
//!
//! # `ai_decision`
//!
//! Ask the AI to pick one label from a fixed `choices` list. The
//! step composes a tightly-instructed prompt, sends it through `ask`
//! with `limit = 0` (no RAG context — this is a classifier call,
//! not a research question), and parses the answer. On no match the
//! step returns `Err` so retry/backoff and `on_error = "continue"`
//! both work as documented for any other failing step.
//!
//! ```toml
//! [[steps]]
//! type = "ai_decision"
//! prompt = "Is this commit a bug fix, feature, or chore?"
//! choices = ["bug", "feature", "chore"]
//! ```

use crate::Step;

/// Parsed args for `type = "ai_prompt"`.
#[derive(Debug)]
pub struct AiPromptArgs {
    /// Prompt text after `${…}` interpolation.
    pub prompt: String,
    /// Optional RAG context `limit`. Forwarded to `com.nexus.ai::ask`
    /// when present; otherwise the handler default applies.
    pub limit: Option<u64>,
}

impl AiPromptArgs {
    /// Pull `prompt` (required) and `limit` (optional) off `step.extra`.
    ///
    /// # Errors
    /// Returns a human-readable string when `prompt` is missing or
    /// not a string.
    pub fn from_step(step: &Step) -> Result<Self, String> {
        let prompt = step
            .extra
            .get("prompt")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "ai_prompt step missing `prompt` string".to_string())?
            .to_string();
        let limit = step
            .extra
            .get("limit")
            .and_then(toml::Value::as_integer)
            .and_then(|n| u64::try_from(n).ok());
        Ok(Self { prompt, limit })
    }
}

/// Parsed args for `type = "ai_decision"`.
#[derive(Debug)]
pub struct AiDecisionArgs {
    /// The decision question.
    pub prompt: String,
    /// Non-empty list of choice labels. Order is preserved.
    pub choices: Vec<String>,
}

impl AiDecisionArgs {
    /// Pull `prompt` and `choices` off `step.extra`. Validates that
    /// `choices` is a non-empty array of non-empty strings.
    ///
    /// # Errors
    /// Returns a human-readable string when validation fails.
    pub fn from_step(step: &Step) -> Result<Self, String> {
        let prompt = step
            .extra
            .get("prompt")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "ai_decision step missing `prompt` string".to_string())?
            .to_string();
        let raw = step
            .extra
            .get("choices")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| "ai_decision step missing `choices` array".to_string())?;
        if raw.is_empty() {
            return Err("ai_decision: `choices` must be non-empty".into());
        }
        let mut choices = Vec::with_capacity(raw.len());
        for item in raw {
            let s = item
                .as_str()
                .ok_or_else(|| "ai_decision: `choices` must be strings".to_string())?;
            if s.trim().is_empty() {
                return Err("ai_decision: choice labels cannot be empty".into());
            }
            choices.push(s.to_string());
        }
        Ok(Self { prompt, choices })
    }
}

/// Compose the prompt sent to the AI for an `ai_decision` step.
///
/// The format is intentionally terse: question, then a numbered list,
/// then a one-line instruction asking for the label by itself. Models
/// reliably echo a single bare label when asked this way, which makes
/// [`pick_choice`] robust without elaborate parsing.
#[must_use]
pub fn build_decision_prompt(prompt: &str, choices: &[String]) -> String {
    let mut s =
        String::with_capacity(prompt.len() + choices.iter().map(String::len).sum::<usize>() + 64);
    s.push_str(prompt.trim());
    s.push_str("\n\nChoose exactly one of these options. Reply with only the chosen label, no other text:\n");
    for choice in choices {
        s.push_str("- ");
        s.push_str(choice);
        s.push('\n');
    }
    s
}

/// Match an AI response against the decision choices.
///
/// Strategy, in order:
/// 1. **Exact match (case-insensitive)** after stripping leading /
///    trailing whitespace and surrounding punctuation (`. , " '`).
/// 2. **Substring match** — first choice whose label appears
///    case-insensitively anywhere in the cleaned answer. When two
///    choices both match by substring, the *longer* label wins —
///    avoids `"chore"` swallowing `"chore-major"` if both are choices.
///
/// Returns `None` if no choice matches; the dispatcher surfaces that
/// as a step failure.
#[must_use]
pub fn pick_choice(raw: &str, choices: &[String]) -> Option<String> {
    let cleaned = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '.' | ',' | '!' | '?'))
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    for c in choices {
        if c.eq_ignore_ascii_case(cleaned) {
            return Some(c.clone());
        }
    }
    let lower = cleaned.to_ascii_lowercase();
    let mut best: Option<&String> = None;
    for c in choices {
        if lower.contains(&c.to_ascii_lowercase()) {
            match best {
                None => best = Some(c),
                Some(prev) if c.len() > prev.len() => best = Some(c),
                _ => {}
            }
        }
    }
    best.cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_workflow_text;

    fn step_from(toml_src: &str) -> Step {
        parse_workflow_text(toml_src)
            .unwrap()
            .steps
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn ai_prompt_args_round_trip_required_and_optional_fields() {
        let step = step_from(
            r#"
[workflow]
name = "P"

[trigger]
type = "manual"

[[steps]]
type = "ai_prompt"
prompt = "hello"
limit = 7
"#,
        );
        let args = AiPromptArgs::from_step(&step).unwrap();
        assert_eq!(args.prompt, "hello");
        assert_eq!(args.limit, Some(7));
    }

    #[test]
    fn ai_prompt_args_omits_limit_when_absent() {
        let step = step_from(
            r#"
[workflow]
name = "P"

[trigger]
type = "manual"

[[steps]]
type = "ai_prompt"
prompt = "hi"
"#,
        );
        let args = AiPromptArgs::from_step(&step).unwrap();
        assert!(args.limit.is_none());
    }

    #[test]
    fn ai_prompt_args_rejects_missing_prompt() {
        let step = step_from(
            r#"
[workflow]
name = "P"

[trigger]
type = "manual"

[[steps]]
type = "ai_prompt"
"#,
        );
        let err = AiPromptArgs::from_step(&step).unwrap_err();
        assert!(err.contains("prompt"));
    }

    #[test]
    fn ai_decision_args_parses_choices() {
        let step = step_from(
            r#"
[workflow]
name = "D"

[trigger]
type = "manual"

[[steps]]
type = "ai_decision"
prompt = "Pick"
choices = ["one", "two", "three"]
"#,
        );
        let args = AiDecisionArgs::from_step(&step).unwrap();
        assert_eq!(args.prompt, "Pick");
        assert_eq!(args.choices, vec!["one", "two", "three"]);
    }

    #[test]
    fn ai_decision_args_rejects_empty_choices() {
        let step = step_from(
            r#"
[workflow]
name = "D"

[trigger]
type = "manual"

[[steps]]
type = "ai_decision"
prompt = "Pick"
choices = []
"#,
        );
        let err = AiDecisionArgs::from_step(&step).unwrap_err();
        assert!(err.contains("non-empty"));
    }

    #[test]
    fn ai_decision_args_rejects_blank_choice() {
        let step = step_from(
            r#"
[workflow]
name = "D"

[trigger]
type = "manual"

[[steps]]
type = "ai_decision"
prompt = "Pick"
choices = ["one", "  "]
"#,
        );
        let err = AiDecisionArgs::from_step(&step).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn ai_decision_args_rejects_non_string_choice() {
        let step = step_from(
            r#"
[workflow]
name = "D"

[trigger]
type = "manual"

[[steps]]
type = "ai_decision"
prompt = "Pick"
choices = ["one", 42]
"#,
        );
        let err = AiDecisionArgs::from_step(&step).unwrap_err();
        assert!(err.contains("strings"));
    }

    #[test]
    fn build_decision_prompt_lists_choices_with_instruction() {
        let p = build_decision_prompt("Which?", &["yes".into(), "no".into()]);
        assert!(p.starts_with("Which?"));
        assert!(p.contains("- yes\n"));
        assert!(p.contains("- no\n"));
        assert!(p.contains("Reply with only the chosen label"));
    }

    #[test]
    fn pick_choice_handles_exact_case_insensitive_match() {
        let cs: Vec<String> = vec!["yes".into(), "no".into()];
        assert_eq!(pick_choice("YES", &cs).as_deref(), Some("yes"));
        assert_eq!(pick_choice("No", &cs).as_deref(), Some("no"));
    }

    #[test]
    fn pick_choice_strips_quotes_and_punctuation() {
        let cs: Vec<String> = vec!["accept".into(), "reject".into()];
        assert_eq!(pick_choice("\"accept\".", &cs).as_deref(), Some("accept"));
        assert_eq!(pick_choice("'Reject'!", &cs).as_deref(), Some("reject"));
    }

    #[test]
    fn pick_choice_substring_picks_longest_match() {
        // A naive contains() would let "chore" match "chore-major" since
        // "chore" is a prefix; we want the longer label.
        let cs: Vec<String> = vec!["chore".into(), "chore-major".into()];
        let answer = "I think this is a chore-major change.";
        assert_eq!(pick_choice(answer, &cs).as_deref(), Some("chore-major"));
    }

    #[test]
    fn pick_choice_returns_none_when_no_choice_matches() {
        let cs: Vec<String> = vec!["yes".into(), "no".into()];
        assert!(pick_choice("maybe", &cs).is_none());
        assert!(pick_choice("", &cs).is_none());
        assert!(pick_choice("   ", &cs).is_none());
    }
}
