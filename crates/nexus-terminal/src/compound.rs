//! Compound-command splitter (PRD-09 §9).
//!
//! Takes a shell command string like
//!
//! ```text
//! cargo build && cargo test; echo done || echo failed
//! ```
//!
//! and returns an ordered list of [`CommandStep`]s, each tagged with the
//! [`Operator`] that decides whether it runs given the previous step's
//! exit code.
//!
//! # Why we parse this ourselves
//!
//! PRD §9.1 sketches a regex-split approach, but a naive regex split
//! would break any command that contains `&&`, `||`, or `;` inside a
//! quoted string — `echo "hello && world"` would wrongly become two
//! steps. The hand-rolled scanner here treats single- and double-quoted
//! regions as opaque (the PRD's own §9.2 UI example assumes correct
//! step extraction). Backslash escaping outside quotes is intentionally
//! NOT handled — nearly every real input either quotes everything or
//! avoids `\&&` entirely, and escape handling is a separate feature to
//! ship when a user hits it.
//!
//! # What this is not
//!
//! - **A shell parser.** We do not expand globs, evaluate `${VAR}`, or
//!   understand pipes (`|`), redirects (`>`, `<`), subshells (`$(...)`,
//!   `(...)`), or backticks. Each step's command string is handed to
//!   the shell verbatim via `sh -c <step>` when executed; the shell
//!   does all of that. We only need to split at the **control
//!   operators** that affect whether a subsequent step runs.
//! - **An executor.** [`CommandStep::should_run`] tells you whether to
//!   run the step given the previous exit code; actually running the
//!   step is the caller's job (typically [`crate::Session::spawn`]
//!   with `sh -c <step.command>`).

use std::fmt;

/// Control operator that links two adjacent commands in a chain.
///
/// The operator attached to a [`CommandStep`] governs whether the step
/// runs given the previous step's exit code — see
/// [`CommandStep::should_run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// `&&` — only run the next step if the previous one exited 0.
    And,
    /// `||` — only run the next step if the previous one exited non-zero.
    Or,
    /// `;` — always run the next step. Also the synthetic operator used
    /// for the very first step in a chain, which has no predecessor.
    Seq,
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operator::And => f.write_str("&&"),
            Operator::Or => f.write_str("||"),
            Operator::Seq => f.write_str(";"),
        }
    }
}

/// One step in a compound command chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandStep {
    /// Operator that gates this step against the previous one. The
    /// first step in a chain carries `Operator::Seq` because there is
    /// no previous exit code to branch on.
    pub operator: Operator,
    /// Command text as extracted from the source string, with
    /// surrounding whitespace trimmed. Internal whitespace and
    /// quoting are preserved verbatim.
    pub command: String,
}

impl CommandStep {
    /// Does this step run given `previous_exit_code`?
    ///
    /// - `Operator::Seq` ⇒ always `true`.
    /// - `Operator::And` ⇒ `true` iff `previous_exit_code == 0`.
    /// - `Operator::Or`  ⇒ `true` iff `previous_exit_code != 0`.
    ///
    /// For the first step in a chain (which carries `Seq` by
    /// convention), pass `0` as the previous exit code; the predicate
    /// is trivially `true` so the value is irrelevant.
    #[must_use]
    pub fn should_run(&self, previous_exit_code: i32) -> bool {
        match self.operator {
            Operator::Seq => true,
            Operator::And => previous_exit_code == 0,
            Operator::Or => previous_exit_code != 0,
        }
    }
}

/// Split `input` into a chain of [`CommandStep`]s at the top-level
/// `&&` / `||` / `;` operators.
///
/// Quoted regions (`"..."` and `'...'`) are treated as opaque: control
/// operators inside them do not split the chain. Empty steps (produced
/// by leading / trailing / doubled operators like `a && ; b`) are
/// dropped — the parser is tolerant of the kinds of typos users make.
///
/// If `input` is empty or whitespace-only, returns an empty `Vec`.
#[must_use]
pub fn parse_command_chain(input: &str) -> Vec<CommandStep> {
    let bytes = input.as_bytes();
    let mut steps: Vec<CommandStep> = Vec::new();
    let mut buf = String::new();
    let mut next_operator = Operator::Seq;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];

        if in_single {
            buf.push(c as char);
            if c == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            buf.push(c as char);
            if c == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }

        match c {
            b'\'' => {
                buf.push(c as char);
                in_single = true;
                i += 1;
            }
            b'"' => {
                buf.push(c as char);
                in_double = true;
                i += 1;
            }
            b'&' if bytes.get(i + 1) == Some(&b'&') => {
                push_step(&mut steps, &mut buf, next_operator);
                next_operator = Operator::And;
                i += 2;
            }
            b'|' if bytes.get(i + 1) == Some(&b'|') => {
                push_step(&mut steps, &mut buf, next_operator);
                next_operator = Operator::Or;
                i += 2;
            }
            b';' => {
                push_step(&mut steps, &mut buf, next_operator);
                next_operator = Operator::Seq;
                i += 1;
            }
            _ => {
                buf.push(c as char);
                i += 1;
            }
        }
    }

    push_step(&mut steps, &mut buf, next_operator);
    steps
}

/// Trim `buf`, push it as a new step if non-empty, then clear it.
/// Drops empty steps so a pathological input like `a && ;; b` still
/// produces `[a, b]` instead of littering the chain with blanks.
fn push_step(steps: &mut Vec<CommandStep>, buf: &mut String, operator: Operator) {
    let trimmed = buf.trim();
    if !trimmed.is_empty() {
        steps.push(CommandStep {
            operator,
            command: trimmed.to_string(),
        });
    }
    buf.clear();
}

/// Should this chain be executed in a single long-lived shell session,
/// or is spawning a fresh subshell per step acceptable? (PRD-09 §9.3)
///
/// A chain that contains `cd …` or `pushd …` changes the shell's
/// working directory, and spawning per-step would lose that change for
/// every subsequent step. Return `true` in that case so the caller
/// knows to pipe every step's text into one live shell via stdin
/// rather than invoke `sh -c <step>` per step.
#[must_use]
pub fn requires_single_shell(steps: &[CommandStep]) -> bool {
    steps
        .iter()
        .any(|s| starts_with_word(&s.command, "cd") || starts_with_word(&s.command, "pushd"))
}

/// Does `command` begin with `word` followed by whitespace, end-of-input,
/// or a shell metacharacter that ends the token? We avoid matching
/// `cdk-app` as `cd` by requiring the next byte to be a token terminator.
fn starts_with_word(command: &str, word: &str) -> bool {
    let cmd = command.trim_start();
    if !cmd.starts_with(word) {
        return false;
    }
    match cmd.as_bytes().get(word.len()) {
        None => true,
        Some(b) => b.is_ascii_whitespace() || *b == b';' || *b == b'&' || *b == b'|',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_command_chain ────────────────────────────────────────────

    #[test]
    fn empty_input_produces_no_steps() {
        assert!(parse_command_chain("").is_empty());
        assert!(parse_command_chain("   \t  ").is_empty());
    }

    #[test]
    fn single_command_produces_one_seq_step() {
        let steps = parse_command_chain("ls -la");
        assert_eq!(
            steps,
            vec![CommandStep {
                operator: Operator::Seq,
                command: "ls -la".into()
            }]
        );
    }

    #[test]
    fn two_commands_with_and() {
        let steps = parse_command_chain("cargo build && cargo test");
        assert_eq!(
            steps,
            vec![
                CommandStep {
                    operator: Operator::Seq,
                    command: "cargo build".into()
                },
                CommandStep {
                    operator: Operator::And,
                    command: "cargo test".into()
                },
            ]
        );
    }

    #[test]
    fn mixed_operators_in_declaration_order() {
        let steps = parse_command_chain("a && b || c ; d");
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].operator, Operator::Seq);
        assert_eq!(steps[1].operator, Operator::And);
        assert_eq!(steps[2].operator, Operator::Or);
        assert_eq!(steps[3].operator, Operator::Seq);
        assert_eq!(steps[0].command, "a");
        assert_eq!(steps[1].command, "b");
        assert_eq!(steps[2].command, "c");
        assert_eq!(steps[3].command, "d");
    }

    #[test]
    fn operators_inside_double_quotes_are_not_split() {
        let steps = parse_command_chain(r#"echo "hello && world; still one""#);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].command, r#"echo "hello && world; still one""#);
    }

    #[test]
    fn operators_inside_single_quotes_are_not_split() {
        let steps = parse_command_chain(r"echo 'a || b || c'");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].command, r"echo 'a || b || c'");
    }

    #[test]
    fn quoted_then_unquoted_operator_splits_correctly() {
        let steps = parse_command_chain(r#"echo "a && b" && echo done"#);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].command, r#"echo "a && b""#);
        assert_eq!(steps[1].command, "echo done");
        assert_eq!(steps[1].operator, Operator::And);
    }

    #[test]
    fn doubled_and_empty_operators_drop_empty_steps() {
        // Pathological input — real users won't write this, but we
        // shouldn't blow up either. The stray `;` between `a` and `b`
        // produces an empty middle step that we drop.
        let steps = parse_command_chain("a ; ; b");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].command, "a");
        assert_eq!(steps[1].command, "b");
    }

    #[test]
    fn leading_and_trailing_whitespace_are_trimmed_per_step() {
        let steps = parse_command_chain("   a   &&   b   ");
        assert_eq!(steps[0].command, "a");
        assert_eq!(steps[1].command, "b");
    }

    #[test]
    fn chain_with_cd_detected() {
        let steps = parse_command_chain("cd project && cargo build");
        assert!(requires_single_shell(&steps));
    }

    #[test]
    fn chain_with_pushd_detected() {
        let steps = parse_command_chain("pushd /tmp && ls");
        assert!(requires_single_shell(&steps));
    }

    #[test]
    fn chain_without_cd_does_not_require_single_shell() {
        let steps = parse_command_chain("cargo build && cargo test");
        assert!(!requires_single_shell(&steps));
    }

    #[test]
    fn cd_as_prefix_of_longer_word_does_not_trigger() {
        // `cdk-app` is not `cd` — word-boundary check should reject it.
        let steps = parse_command_chain("cdk-app deploy");
        assert!(!requires_single_shell(&steps));
    }

    // ── should_run ─────────────────────────────────────────────────────

    #[test]
    fn seq_always_runs() {
        let step = CommandStep {
            operator: Operator::Seq,
            command: "x".into(),
        };
        assert!(step.should_run(0));
        assert!(step.should_run(1));
        assert!(step.should_run(127));
    }

    #[test]
    fn and_runs_only_when_previous_was_success() {
        let step = CommandStep {
            operator: Operator::And,
            command: "x".into(),
        };
        assert!(step.should_run(0));
        assert!(!step.should_run(1));
    }

    #[test]
    fn or_runs_only_when_previous_was_failure() {
        let step = CommandStep {
            operator: Operator::Or,
            command: "x".into(),
        };
        assert!(!step.should_run(0));
        assert!(step.should_run(1));
        assert!(step.should_run(127));
    }

    // ── Operator Display ──────────────────────────────────────────────

    #[test]
    fn operator_display_matches_source_symbol() {
        assert_eq!(format!("{}", Operator::And), "&&");
        assert_eq!(format!("{}", Operator::Or), "||");
        assert_eq!(format!("{}", Operator::Seq), ";");
    }
}
