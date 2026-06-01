//! PII / secret egress filter (PRD-12 §15.1).
//!
//! A small, dependency-light redactor that scans untrusted text for
//! common high-confidence secret shapes (cloud keys, API tokens, PEM
//! private-key blocks) and replaces every match with a stable
//! `[REDACTED:<pattern-id>]` placeholder before the text is appended to
//! a model prompt.
//!
//! ## Wire point
//!
//! The redactor is wired into [`crate::rag::build_rag_prompt_budgeted`]
//! only — it scans retrieved RAG chunks before they are stitched into
//! the system prompt. It is **not** wired into `stream_chat`, the
//! provider request bodies, or any user-typed message: silently
//! mutating user input would be surprising and the user already chose
//! to send what they pasted. RAG injects retrieved content the user
//! did not paste this turn — that is the right boundary.
//!
//! ## Patterns
//!
//! Every pattern below is regex-lite-syntax compatible (no lookaround,
//! no `(?s)`-style inline flags except a leading `(?i)` which
//! `regex-lite` does accept):
//!
//! 1. `aws-access-key`  — `AKIA[0-9A-Z]{16}`
//! 2. `aws-secret`      — `(?i)aws(.{0,20})?(secret|access)?(.{0,20})?[=:\s]+[A-Za-z0-9/+=]{40}`
//! 3. `api-token`       — `sk_(live|test)_[A-Za-z0-9]{24,}`
//! 4. `github-pat-classic` — `ghp_[A-Za-z0-9]{36}`
//! 5. `github-pat-fine`    — `github_pat_[A-Za-z0-9_]{82}`
//! 6. `private-key`     — `-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----`
//!
//! All six patterns were verified to compile under `regex-lite` 0.1.x
//! before landing — including the leading `(?i)` and `[\s\S]*?` forms
//! — so no syntax adjustments were required.
//!
//! ## Policies
//!
//! [`PrivacyPolicy::Off`] disables redaction (the default). Existing
//! callers that don't pass a [`Redactor`] keep their byte-for-byte
//! legacy output. [`PrivacyPolicy::Redact`] replaces matches with a
//! placeholder. [`PrivacyPolicy::Strict`] is shipped today as a no-op
//! alias of [`PrivacyPolicy::Redact`] — the variant exists so callers
//! can opt into stricter semantics later (e.g. erroring on match)
//! without an enum-shape break. [`Redactor::is_strict`] returns the
//! current intent so future wiring can branch on it.

use regex_lite::Regex;
use serde::{Deserialize, Serialize};

/// Configured privacy policy for outbound context.
///
/// Default is [`PrivacyPolicy::Off`] — no redaction occurs unless a
/// caller explicitly opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrivacyPolicy {
    /// No redaction. Current default.
    #[default]
    Off,
    /// Replace every match with a `[REDACTED:<pattern-id>]` placeholder.
    Redact,
    /// Same observable behaviour as [`PrivacyPolicy::Redact`] today.
    /// Reserved for a future error-on-match path; callers can already
    /// branch on [`Redactor::is_strict`].
    Strict,
}

/// A single redaction event recorded by [`Redactor::redact`] /
/// [`Redactor::redact_in_place`].
///
/// `start` / `end` are byte offsets into the **original** input — they
/// are useful for diagnostics and will not necessarily line up with
/// offsets in the rewritten string (which has had matches replaced by
/// placeholders of a different length).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    /// Stable identifier for the pattern that fired (e.g. `"aws-access-key"`).
    pub pattern_id: &'static str,
    /// Byte offset of the match start in the original input.
    pub start: usize,
    /// Byte offset of the match end in the original input (exclusive).
    pub end: usize,
}

/// Compiled set of secret-detection regexes paired with replacement
/// placeholders.
///
/// Construct with [`Redactor::with_default_patterns`] for the
/// PRD-12 §15.1 default set.
pub struct Redactor {
    /// `(regex, placeholder, pattern_id)` triples evaluated in order.
    /// Ordering matters when patterns overlap: the more specific
    /// `aws-access-key` is listed before the looser `aws-secret`.
    patterns: Vec<(Regex, &'static str, &'static str)>,
    /// True for [`PrivacyPolicy::Strict`]; recorded so future wiring
    /// (error-on-match) can branch without re-plumbing the policy enum.
    strict: bool,
}

impl Redactor {
    /// Construct a redactor pre-loaded with the PRD-12 §15.1 default
    /// pattern set. Configured under [`PrivacyPolicy::Redact`]
    /// semantics — call [`Redactor::with_strict`] if you want
    /// `is_strict()` to return `true`.
    #[must_use]
    pub fn with_default_patterns() -> Self {
        Self::build(false)
    }

    /// Construct a redactor with the default pattern set and
    /// [`PrivacyPolicy::Strict`] semantics — observably identical to
    /// [`Self::with_default_patterns`] today, but [`Self::is_strict`]
    /// will return `true`.
    #[must_use]
    pub fn with_strict() -> Self {
        Self::build(true)
    }

    /// Build a redactor for the supplied [`PrivacyPolicy`].
    ///
    /// Returns `None` for [`PrivacyPolicy::Off`] (the convention
    /// callers use to thread `Option<&Redactor>` through the prompt
    /// builder).
    #[must_use]
    pub fn for_policy(policy: PrivacyPolicy) -> Option<Self> {
        match policy {
            PrivacyPolicy::Off => None,
            PrivacyPolicy::Redact => Some(Self::build(false)),
            PrivacyPolicy::Strict => Some(Self::build(true)),
        }
    }

    fn build(strict: bool) -> Self {
        // Order matters when patterns can overlap — list the more
        // specific shape first. `expect` is acceptable here because
        // every pattern is a compile-time constant verified at crate
        // build time by the unit tests below.
        let raw: &[(&str, &str, &str)] = &[
            (
                r"AKIA[0-9A-Z]{16}",
                "[REDACTED:aws-access-key]",
                "aws-access-key",
            ),
            (
                r"(?i)aws(.{0,20})?(secret|access)?(.{0,20})?[=:\s]+[A-Za-z0-9/+=]{40}",
                "[REDACTED:aws-secret]",
                "aws-secret",
            ),
            (
                r"sk_(live|test)_[A-Za-z0-9]{24,}",
                "[REDACTED:api-token]",
                "api-token",
            ),
            (
                r"ghp_[A-Za-z0-9]{36}",
                "[REDACTED:github-token]",
                "github-pat-classic",
            ),
            (
                r"github_pat_[A-Za-z0-9_]{82}",
                "[REDACTED:github-token]",
                "github-pat-fine",
            ),
            (
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
                "[REDACTED:private-key]",
                "private-key",
            ),
        ];
        let patterns = raw
            .iter()
            .map(|(pat, placeholder, id)| {
                (
                    Regex::new(pat).expect("default redactor pattern compiles"),
                    *placeholder,
                    *id,
                )
            })
            .collect();
        Self { patterns, strict }
    }

    /// Returns `true` for redactors built under
    /// [`PrivacyPolicy::Strict`]. Always `false` for `Off` (no
    /// redactor is constructed) and `Redact`.
    #[must_use]
    pub fn is_strict(&self) -> bool {
        self.strict
    }

    /// Scan `input` and return a redacted copy plus the list of
    /// matches. The original string is not mutated.
    ///
    /// Each pattern is evaluated in declaration order against the
    /// **current** (possibly already-redacted) string, so two patterns
    /// can't both fire on the same byte range — once the first wins,
    /// the placeholder is in place and the second sees only the
    /// placeholder.
    #[must_use]
    pub fn redact(&self, input: &str) -> (String, Vec<Redaction>) {
        let mut owned = input.to_string();
        let redactions = self.redact_in_place(&mut owned);
        (owned, redactions)
    }

    /// In-place variant of [`Self::redact`]. The string is rewritten
    /// in place; the returned vector holds one [`Redaction`] per match
    /// in the order matches were applied.
    pub fn redact_in_place(&self, input: &mut String) -> Vec<Redaction> {
        let mut events: Vec<Redaction> = Vec::new();
        for (regex, placeholder, pattern_id) in &self.patterns {
            // Walk left-to-right, replacing each match. Recompute the
            // search position after each replacement because the
            // placeholder length differs from the match length.
            let mut search_start = 0usize;
            while let Some(m) = regex.find_at(input, search_start) {
                let start = m.start();
                let end = m.end();
                events.push(Redaction {
                    pattern_id,
                    start,
                    end,
                });
                input.replace_range(start..end, placeholder);
                search_start = start + placeholder.len();
                if search_start >= input.len() {
                    break;
                }
            }
        }
        events
    }
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Redactor")
            .field("pattern_count", &self.patterns.len())
            .field("strict", &self.strict)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redactor_replaces_aws_access_key() {
        let r = Redactor::with_default_patterns();
        let input = "before AKIAIOSFODNN7EXAMPLE after";
        let (out, events) = r.redact(input);
        assert_eq!(out, "before [REDACTED:aws-access-key] after");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].pattern_id, "aws-access-key");
        assert_eq!(events[0].start, 7);
        assert_eq!(events[0].end, 7 + 20);
    }

    #[test]
    fn redactor_replaces_private_key_block_multiline() {
        let r = Redactor::with_default_patterns();
        let pem = "-----BEGIN RSA PRIVATE KEY-----\n\
                   MIIEpAIBAAKCAQEA0Z3VS5uKYsAB\n\
                   abcdefABCDEF1234567890+/=\n\
                   -----END RSA PRIVATE KEY-----";
        let input = format!("noise above\n{pem}\nnoise below");
        let (out, events) = r.redact(&input);
        assert!(out.contains("[REDACTED:private-key]"), "got: {out}");
        assert!(!out.contains("BEGIN RSA"));
        assert!(!out.contains("END RSA"));
        assert!(out.starts_with("noise above\n"));
        assert!(out.ends_with("\nnoise below"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].pattern_id, "private-key");
    }

    #[test]
    fn redactor_replaces_multiple_distinct_secrets_in_one_pass() {
        let r = Redactor::with_default_patterns();
        // Stripe-style key + GitHub classic PAT in the same input.
        let stripe = "sk_live_abcdefghijklmnopqrstuvwxyz0123";
        let ghp = format!("ghp_{}", "a".repeat(36));
        let input = format!("token1={stripe} token2={ghp}");
        let (out, events) = r.redact(&input);
        assert!(out.contains("[REDACTED:api-token]"), "got: {out}");
        assert!(out.contains("[REDACTED:github-token]"), "got: {out}");
        assert!(!out.contains(stripe));
        assert!(!out.contains(&ghp));
        // Two events, one per pattern, in declaration order
        // (api-token before github-pat-classic).
        assert_eq!(events.len(), 2, "events: {events:?}");
        assert_eq!(events[0].pattern_id, "api-token");
        assert_eq!(events[1].pattern_id, "github-pat-classic");
    }

    #[test]
    fn redactor_no_match_returns_input_unchanged() {
        let r = Redactor::with_default_patterns();
        let input = "Just some prose with no secrets here. Numbers like 1234 are fine.";
        let (out, events) = r.redact(input);
        assert_eq!(out, input);
        assert!(events.is_empty(), "unexpected events: {events:?}");
    }

    #[test]
    fn for_policy_returns_none_for_off() {
        assert!(Redactor::for_policy(PrivacyPolicy::Off).is_none());
        assert!(Redactor::for_policy(PrivacyPolicy::Redact).is_some());
        assert!(Redactor::for_policy(PrivacyPolicy::Strict).is_some());
    }

    #[test]
    fn is_strict_reflects_constructor() {
        assert!(!Redactor::with_default_patterns().is_strict());
        assert!(Redactor::with_strict().is_strict());
        assert!(!Redactor::for_policy(PrivacyPolicy::Redact)
            .unwrap()
            .is_strict());
        assert!(Redactor::for_policy(PrivacyPolicy::Strict)
            .unwrap()
            .is_strict());
    }

    #[test]
    fn privacy_policy_default_is_off() {
        assert_eq!(PrivacyPolicy::default(), PrivacyPolicy::Off);
    }
}
