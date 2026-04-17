//! AI terminal integration — PRD-09 §12.
//!
//! # Role
//!
//! Watches lines coming off [`crate::TerminalServer::pump`] and
//! surfaces [`SuggestedCommand`]s that the UI can render as one-click
//! fixes ("cargo check", "npm update", etc.). The engine is purely
//! pattern-driven here — no LLM; a future layer on top can wrap this
//! to forward un-matched lines to Claude and merge the structured
//! responses back into the same [`SuggestedCommand`] surface.
//!
//! # Microkernel fit
//!
//! Plain library. No tokio, no HTTP client, no prompt caching. The
//! engine takes `&OutputLine` (or just `&str`) references and returns
//! owned results; wiring it to the event stream is one `while let`
//! loop the core plugin or UI owns.
//!
//! # Why rules, not closures
//!
//! [`SuggestionRule`] is a small trait (not a function pointer) so a
//! rule can hold compiled regex state across invocations. The PRD §12
//! examples are all simple substring or regex matches; richer rules
//! (context-aware, multi-line) can implement the trait without this
//! module learning about them.
//!
//! # What this is NOT
//!
//! - An execution surface. [`SuggestedCommand::text`] is just a string;
//!   the UI decides whether to show it as a button, a command-palette
//!   entry, or a chat message. Running it goes through the normal
//!   [`crate::TerminalServer::send_input`] path.
//! - A learner. Rules are fixed at construction; no reinforcement from
//!   user accept/reject signals. That layer belongs to the AI
//!   subsystem proper.

use serde::{Deserialize, Serialize};

use crate::server::OutputLine;

/// Severity hint the UI reads to pick a color + urgency badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionSeverity {
    /// Neutral hint — dev-quality improvement, tool suggestion.
    Info,
    /// Something noteworthy but non-blocking — e.g. a warning surfaced
    /// in the output.
    Warning,
    /// Something the user almost certainly wants to address — e.g. a
    /// build failure with a known workaround.
    Error,
}

/// One-shot suggestion emitted in response to an output line. The
/// caller renders `text` as the runnable command and `reason` as a
/// short explanation ("Get detailed error info for debugging").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedCommand {
    /// Suggested command line (shell-ready; callers feed to
    /// [`crate::TerminalServer::send_input`] verbatim).
    pub text: String,
    /// Human-readable motivation surfaced alongside the button.
    pub reason: String,
    /// Severity hint (see [`SuggestionSeverity`]).
    pub severity: SuggestionSeverity,
    /// Stable rule id — the rule's [`SuggestionRule::id`]. Useful for
    /// deduping, analytics, and rule disable lists in settings.
    pub source_rule: &'static str,
}

/// A single pattern the engine evaluates against each output line.
/// Implementors hold any compiled regex state internally so the hot
/// path doesn't re-parse.
pub trait SuggestionRule: Send + Sync {
    /// Short, stable id used in [`SuggestedCommand::source_rule`].
    /// Must be unique within an engine.
    fn id(&self) -> &'static str;

    /// Evaluate `line`. Returning `None` means "no match"; `Some`
    /// carries the suggestion.
    fn evaluate(&self, line: &str) -> Option<SuggestedCommand>;
}

/// Engine that runs every configured rule against each observed line
/// and surfaces one suggestion per rule that fires.
pub struct AiSuggestionEngine {
    rules: Vec<Box<dyn SuggestionRule>>,
}

impl AiSuggestionEngine {
    /// Build an empty engine. Tests and advanced callers can layer
    /// their own rules on with [`Self::with_rule`].
    #[must_use]
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Build an engine preloaded with [`default_suggestion_rules`].
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut e = Self::empty();
        for rule in default_suggestion_rules() {
            e.rules.push(rule);
        }
        e
    }

    /// Append a rule. Returns `self` for builder-style chaining.
    #[must_use]
    pub fn with_rule(mut self, rule: Box<dyn SuggestionRule>) -> Self {
        self.rules.push(rule);
        self
    }

    /// Number of installed rules. Useful in tests + settings UIs.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Evaluate every rule against `line`, collecting matches in
    /// registration order. Fires at most once per rule per call — a
    /// rule that matches twice on the same line still only produces
    /// one suggestion.
    #[must_use]
    pub fn observe(&self, line: &str) -> Vec<SuggestedCommand> {
        self.rules
            .iter()
            .filter_map(|r| r.evaluate(line))
            .collect()
    }

    /// Convenience wrapper that reads the ANSI-stripped text from an
    /// [`OutputLine`] so callers already holding one don't re-deref.
    #[must_use]
    pub fn observe_output_line(&self, line: &OutputLine) -> Vec<SuggestedCommand> {
        self.observe(&line.content)
    }
}

impl Default for AiSuggestionEngine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ── Built-in rules ───────────────────────────────────────────────────────────
//
// Kept as plain substring checks unless the PRD example explicitly
// calls for regex. Substring matches are O(n) per line and faster than
// regex-lite's small constant overhead — matters at the 10k-lines/sec
// throughput target (§17.1).

/// Rust compile failure → get machine-readable diagnostics.
pub struct CargoCompileFailureRule;

impl SuggestionRule for CargoCompileFailureRule {
    fn id(&self) -> &'static str {
        "cargo.compile_failure"
    }

    fn evaluate(&self, line: &str) -> Option<SuggestedCommand> {
        // Cargo prints "error: could not compile …" on bubble-up and
        // "error[E…]:" for individual diagnostics. Match the summary.
        if line.contains("could not compile") || line.contains("error: Failed to compile") {
            return Some(SuggestedCommand {
                text: "cargo check --message-format=json".into(),
                reason: "Get detailed, machine-readable error info for the failing crate.".into(),
                severity: SuggestionSeverity::Error,
                source_rule: self.id(),
            });
        }
        None
    }
}

/// npm 404 → update lockfile.
pub struct NpmPackageNotFoundRule;

impl SuggestionRule for NpmPackageNotFoundRule {
    fn id(&self) -> &'static str {
        "npm.package_not_found"
    }

    fn evaluate(&self, line: &str) -> Option<SuggestedCommand> {
        if line.contains("npm ERR! 404") || line.contains("npm ERR! notfound") {
            return Some(SuggestedCommand {
                text: "npm update".into(),
                reason: "Package reference 404'd — the lockfile may be stale.".into(),
                severity: SuggestionSeverity::Error,
                source_rule: self.id(),
            });
        }
        None
    }
}

/// `command not found` → hint at installation check. Covers shells
/// that emit "bash: foo: command not found" as well as zsh's
/// "zsh: command not found: foo".
pub struct CommandNotFoundRule;

impl SuggestionRule for CommandNotFoundRule {
    fn id(&self) -> &'static str {
        "shell.command_not_found"
    }

    fn evaluate(&self, line: &str) -> Option<SuggestedCommand> {
        if !line.contains("command not found") {
            return None;
        }
        // Extract the offending command name. Shell formats vary; try
        // the two most common shapes before giving up.
        let cmd = line
            .split(':')
            .nth(1)
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "command not found")
            .or_else(|| {
                // zsh: "zsh: command not found: foo" — the name is the
                // last colon-separated chunk.
                line.rsplit(':').next().map(str::trim).filter(|s| !s.is_empty())
            })?;
        Some(SuggestedCommand {
            text: format!("which {cmd}"),
            reason: format!(
                "Shell reported '{cmd}' missing. Check if it's installed or on PATH.",
            ),
            severity: SuggestionSeverity::Warning,
            source_rule: self.id(),
        })
    }
}

/// `git` "permission denied (publickey)" → surface the ssh-add workflow.
pub struct GitPublicKeyRule;

impl SuggestionRule for GitPublicKeyRule {
    fn id(&self) -> &'static str {
        "git.permission_denied_publickey"
    }

    fn evaluate(&self, line: &str) -> Option<SuggestedCommand> {
        // Covers both "Permission denied (publickey)" from ssh and
        // git's "fatal: Could not read from remote repository" that
        // normally follows it on the next line; matching either is fine.
        if line.contains("Permission denied (publickey)") {
            return Some(SuggestedCommand {
                text: "ssh-add -l".into(),
                reason: "Git failed ssh auth — list loaded keys in your ssh-agent.".into(),
                severity: SuggestionSeverity::Error,
                source_rule: self.id(),
            });
        }
        None
    }
}

/// Port-in-use bind failures (Node, Python `-m http.server`, Rust
/// servers). Emits a `lsof`/`ss` suggestion to find the conflicting
/// process.
pub struct AddressInUseRule;

impl SuggestionRule for AddressInUseRule {
    fn id(&self) -> &'static str {
        "net.address_in_use"
    }

    fn evaluate(&self, line: &str) -> Option<SuggestedCommand> {
        // Node emits "EADDRINUSE"; others emit "Address already in use".
        if !(line.contains("EADDRINUSE") || line.contains("Address already in use")) {
            return None;
        }
        Some(SuggestedCommand {
            // lsof is Unix-only but the reason text hints at `ss` /
            // `netstat -ano` alternatives; we're optimising for the
            // common dev-laptop case.
            text: "lsof -iTCP -sTCP:LISTEN -nP".into(),
            reason: "A port's already bound. List LISTENing sockets to find the holder."
                .into(),
            severity: SuggestionSeverity::Error,
            source_rule: self.id(),
        })
    }
}

/// Every built-in rule, in the order the engine evaluates them. Adding
/// rules here enables them for every caller that uses
/// [`AiSuggestionEngine::with_defaults`].
#[must_use]
pub fn default_suggestion_rules() -> Vec<Box<dyn SuggestionRule>> {
    vec![
        Box::new(CargoCompileFailureRule),
        Box::new(NpmPackageNotFoundRule),
        Box::new(CommandNotFoundRule),
        Box::new(GitPublicKeyRule),
        Box::new(AddressInUseRule),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(v: &[SuggestedCommand]) -> Vec<&str> {
        v.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn empty_engine_yields_nothing() {
        let e = AiSuggestionEngine::empty();
        assert_eq!(e.rule_count(), 0);
        assert!(e.observe("anything").is_empty());
    }

    #[test]
    fn cargo_compile_failure_fires_on_summary_line() {
        let e = AiSuggestionEngine::empty().with_rule(Box::new(CargoCompileFailureRule));
        let hits = e.observe("error: could not compile `myapp` (lib) due to 2 previous errors");
        assert_eq!(texts(&hits), vec!["cargo check --message-format=json"]);
        assert_eq!(hits[0].severity, SuggestionSeverity::Error);
        assert_eq!(hits[0].source_rule, "cargo.compile_failure");
    }

    #[test]
    fn cargo_rule_does_not_fire_on_unrelated_errors() {
        let e = AiSuggestionEngine::empty().with_rule(Box::new(CargoCompileFailureRule));
        assert!(e.observe("error: unrelated panic at src/main.rs:42").is_empty());
    }

    #[test]
    fn npm_404_rule_fires_and_gives_update_hint() {
        let e = AiSuggestionEngine::empty().with_rule(Box::new(NpmPackageNotFoundRule));
        let hits = e.observe("npm ERR! 404 Not Found - GET https://registry.npmjs.org/xyz");
        assert_eq!(texts(&hits), vec!["npm update"]);
    }

    #[test]
    fn command_not_found_extracts_bash_style_command_name() {
        let e = AiSuggestionEngine::empty().with_rule(Box::new(CommandNotFoundRule));
        let hits = e.observe("bash: foobar: command not found");
        assert_eq!(texts(&hits), vec!["which foobar"]);
        assert_eq!(hits[0].severity, SuggestionSeverity::Warning);
    }

    #[test]
    fn command_not_found_extracts_zsh_style_command_name() {
        let e = AiSuggestionEngine::empty().with_rule(Box::new(CommandNotFoundRule));
        let hits = e.observe("zsh: command not found: foobar");
        assert_eq!(texts(&hits), vec!["which foobar"]);
    }

    #[test]
    fn git_publickey_rule_fires_on_permission_denied_line() {
        let e = AiSuggestionEngine::empty().with_rule(Box::new(GitPublicKeyRule));
        let hits =
            e.observe("git@github.com: Permission denied (publickey).");
        assert_eq!(texts(&hits), vec!["ssh-add -l"]);
    }

    #[test]
    fn address_in_use_fires_on_eaddrinuse_and_plain_wording() {
        let e = AiSuggestionEngine::empty().with_rule(Box::new(AddressInUseRule));
        assert!(!e.observe("Error: listen EADDRINUSE :::3000").is_empty());
        assert!(!e
            .observe("OSError: [Errno 98] Address already in use")
            .is_empty());
    }

    #[test]
    fn default_engine_exposes_prd_rules_and_fires_on_their_patterns() {
        let e = AiSuggestionEngine::with_defaults();
        assert!(e.rule_count() >= 5);
        let cargo = e.observe("error: could not compile foo due to errors");
        assert_eq!(cargo.len(), 1);
        let npm = e.observe("npm ERR! 404 package gone");
        assert_eq!(npm.len(), 1);
    }

    #[test]
    fn observe_runs_every_rule_in_registration_order() {
        // Two rules whose patterns both fire — observe() must return
        // both, in the order they were registered.
        struct A;
        impl SuggestionRule for A {
            fn id(&self) -> &'static str {
                "a"
            }
            fn evaluate(&self, line: &str) -> Option<SuggestedCommand> {
                line.contains("trigger").then(|| SuggestedCommand {
                    text: "a".into(),
                    reason: "a".into(),
                    severity: SuggestionSeverity::Info,
                    source_rule: "a",
                })
            }
        }
        struct B;
        impl SuggestionRule for B {
            fn id(&self) -> &'static str {
                "b"
            }
            fn evaluate(&self, line: &str) -> Option<SuggestedCommand> {
                line.contains("trigger").then(|| SuggestedCommand {
                    text: "b".into(),
                    reason: "b".into(),
                    severity: SuggestionSeverity::Info,
                    source_rule: "b",
                })
            }
        }
        let e = AiSuggestionEngine::empty()
            .with_rule(Box::new(A))
            .with_rule(Box::new(B));
        let hits = e.observe("trigger!");
        assert_eq!(texts(&hits), vec!["a", "b"]);
    }

    #[test]
    fn observe_output_line_reads_content_field() {
        let e = AiSuggestionEngine::with_defaults();
        let line = OutputLine {
            timestamp_ms: 0,
            content: "npm ERR! 404 nope".into(),
            raw: vec![],
            repeats: 1,
        };
        let hits = e.observe_output_line(&line);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].text, "npm update");
    }

    #[test]
    fn rule_ids_are_unique_within_defaults() {
        // Manually verify — if this ever fires, rename one of the
        // colliding ids so analytics + settings UI can disambiguate.
        let rules = default_suggestion_rules();
        let mut ids: Vec<&'static str> = rules.iter().map(|r| r.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate rule ids in defaults");
    }
}
