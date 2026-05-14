//! BL-120 — context compression for the agent session loop.
//!
//! When the per-round prompt is about to exceed
//! [`SessionConfig::max_context_tokens`](crate::SessionConfig::max_context_tokens),
//! the session loop hands the oldest rounds to a [`Compressor`] and
//! replaces them with a single summary block. Newer rounds (the
//! "working set") stay verbatim so the model still has full detail
//! on the in-flight work — only the historic context is rolled up.
//!
//! ## Strategy plug-points
//!
//! [`Compressor`] is a trait so callers can swap the default
//! LLM-backed summariser for cheaper alternatives without touching
//! the loop:
//!
//! - [`LlmCompressor`] — feeds the rounds back through the active
//!   [`crate::ChatDriver`] with a "summarise" system prompt. The
//!   shipped default; the same provider that's driving the session
//!   does the rollup so no second auth surface is needed.
//! - [`KeepDecisionsCompressor`] — extracts every approved tool
//!   call as a one-line bullet list. Cheap, deterministic, no
//!   provider round-trip. Useful in tests + as a fallback when the
//!   provider is unreachable.
//! - [`NoopCompressor`] — emits a fixed `"[N rounds elided]"`
//!   string. Lower-bound baseline that's still spec-compliant and
//!   keeps the loop budget-safe; the summary carries no information
//!   so callers should treat it as a degradation.
//!
//! ## Token estimation
//!
//! [`estimate_tokens`] is a cheap chars-per-4 heuristic. Real
//! tokeniser counts would mean shipping `tiktoken-rs` per provider
//! plus eating the per-round overhead of running it; v1 keeps the
//! arithmetic simple and conservative — the heuristic always
//! over-estimates compared to a real tokeniser, so the loop
//! triggers compression slightly earlier than strictly necessary.
//! That's the right side of the safety boundary.

use async_trait::async_trait;

use crate::session::RoundRecord;
use crate::ChatDriver;

/// One BL-120 compaction event captured against [`crate::AgentSession`].
/// Persisted inside the session JSON + mirrored as
/// [`crate::memory::MemoryEntry::CompactedTurns`] when the caller
/// bridges session→memory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS, schemars::JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct CompactionEvent {
    /// Round index of the first round that was rolled into the
    /// summary (1-based, matches `RoundRecord::round`).
    pub first_round: u32,
    /// Round index of the last round in the rollup.
    pub last_round: u32,
    /// The summary text the compressor produced.
    pub summary: String,
    /// Unix epoch milliseconds when the compaction ran.
    pub timestamp_ms: u64,
}

/// Estimate token count from character length. Approximates 4
/// characters per token — the rule of thumb the OpenAI cookbook
/// quotes for English prose. Over-estimates slightly for code +
/// punctuation-heavy text, which is the safe side: the loop
/// triggers compression a hair earlier than strictly necessary.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Strategy interface for compressing a slice of older rounds into
/// a single summary string. Implementations should be cheap +
/// idempotent — the session loop calls them at most once per
/// compaction event but may retry on transient failures.
#[async_trait]
pub trait Compressor: Send + Sync {
    /// Summarise `rounds` in the context of the session's original
    /// `goal`. Return the summary text or a one-line error string;
    /// the loop falls back to a "compaction failed" placeholder so
    /// a single transient failure doesn't blow up the entire
    /// session.
    async fn compress(&self, rounds: &[RoundRecord], goal: &str) -> Result<String, String>;
}

/// LLM-backed compressor that round-trips through whatever
/// [`ChatDriver`] is driving the session. Cheaper to set up than a
/// dedicated summariser provider (no second auth) at the cost of
/// the latency of one round-trip.
pub struct LlmCompressor<'a, D: ChatDriver + ?Sized> {
    driver: &'a D,
}

impl<'a, D: ChatDriver + ?Sized> LlmCompressor<'a, D> {
    /// Construct a compressor that summarises through `driver`.
    #[must_use]
    pub fn new(driver: &'a D) -> Self {
        Self { driver }
    }
}

const COMPRESSION_SYSTEM_PROMPT: &str =
    "You are summarising an agent's prior tool-use rounds for a downstream planner. \
     Produce a short paragraph (under 300 words) that records: the work that succeeded, \
     decisions the agent committed to, files / artifacts produced, and any failures \
     that affect what should happen next. Skip routine 'tool X succeeded' noise. \
     Reply with prose only — no headings, no markdown.";

#[async_trait]
impl<D: ChatDriver + ?Sized> Compressor for LlmCompressor<'_, D> {
    async fn compress(&self, rounds: &[RoundRecord], goal: &str) -> Result<String, String> {
        if rounds.is_empty() {
            return Ok(String::new());
        }
        let mut user_msg = String::new();
        user_msg.push_str("Original goal: ");
        user_msg.push_str(goal);
        user_msg.push_str("\n\nRounds to summarise:\n");
        for r in rounds {
            user_msg.push_str(&format!(
                "Round {}{}\n",
                r.round,
                if r.text.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", r.text)
                }
            ));
            for tc in &r.tool_calls {
                let verdict = if !tc.approved {
                    "denied"
                } else if !tc.error.is_empty() {
                    "failed"
                } else {
                    "ok"
                };
                user_msg.push_str(&format!("  - {} {}\n", tc.name, verdict));
                if !tc.error.is_empty() {
                    user_msg.push_str(&format!("    error: {}\n", tc.error));
                }
            }
        }
        let proposal = self
            .driver
            .propose(COMPRESSION_SYSTEM_PROMPT, &user_msg)
            .await
            .map_err(|e| format!("compressor driver error: {e}"))?;
        let trimmed = proposal.text.trim().to_string();
        if trimmed.is_empty() {
            // Some providers return only tool calls + no text on
            // the summary turn; treat that as "no useful summary"
            // and let the loop fall back to a placeholder.
            return Err("provider returned no summary text".to_string());
        }
        Ok(trimmed)
    }
}

/// Heuristic compressor: collect approved tool-call names + every
/// failure verbatim into a short bullet summary. Doesn't need a
/// provider so it works in tests + as the fallback when the LLM
/// path errors out. Deterministic — exact same input → exact same
/// summary.
pub struct KeepDecisionsCompressor;

#[async_trait]
impl Compressor for KeepDecisionsCompressor {
    async fn compress(&self, rounds: &[RoundRecord], _goal: &str) -> Result<String, String> {
        let mut out = String::new();
        if rounds.is_empty() {
            return Ok(out);
        }
        out.push_str("Summary of prior rounds:\n");
        for r in rounds {
            for tc in &r.tool_calls {
                if tc.approved && tc.error.is_empty() {
                    out.push_str(&format!("- round {}: {} ok\n", r.round, tc.name));
                } else if !tc.error.is_empty() {
                    out.push_str(&format!(
                        "- round {}: {} failed ({})\n",
                        r.round, tc.name, tc.error
                    ));
                }
            }
        }
        Ok(out)
    }
}

/// Minimal compressor used by the run-loop when no provider is
/// available + the deterministic fallback is undesired (e.g. the
/// caller has set `max_context_tokens` for budget tracking only).
/// Emits a "`[N rounds elided]`" line — keeps the loop budget-safe
/// but carries no detail.
pub struct NoopCompressor;

#[async_trait]
impl Compressor for NoopCompressor {
    async fn compress(&self, rounds: &[RoundRecord], _goal: &str) -> Result<String, String> {
        Ok(format!("[{} earlier rounds elided]", rounds.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{RoundRecord, ToolCallRecord};
    use crate::Proposal;

    fn round(idx: u32, tool_name: &str, ok: bool) -> RoundRecord {
        RoundRecord {
            round: idx,
            text: String::new(),
            tool_calls: vec![ToolCallRecord {
                id: format!("u{idx}"),
                name: tool_name.into(),
                tool_call: crate::ToolCall {
                    target_plugin_id: "com.nexus.storage".into(),
                    command_id: "read_file".into(),
                    args: serde_json::json!({}),
                },
                approved: true,
                reason: String::new(),
                response: ok.then(|| serde_json::json!({"ok": true})),
                error: if ok { String::new() } else { "boom".into() },
                duration_ms: 0,
            }],
        }
    }

    #[test]
    fn estimate_tokens_is_chars_div4_ceiled() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abc"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"x".repeat(400)), 100);
    }

    #[tokio::test]
    async fn keep_decisions_compressor_records_ok_and_failure_rounds() {
        let rounds = vec![
            round(1, "read_file", true),
            round(2, "write_file", false),
            round(3, "grep", true),
        ];
        let summary = KeepDecisionsCompressor
            .compress(&rounds, "investigate flaky test")
            .await
            .unwrap();
        assert!(summary.contains("round 1: read_file ok"));
        assert!(summary.contains("round 2: write_file failed"));
        assert!(summary.contains("round 3: grep ok"));
    }

    #[tokio::test]
    async fn keep_decisions_compressor_empty_round_set_returns_empty() {
        let summary = KeepDecisionsCompressor.compress(&[], "g").await.unwrap();
        assert!(summary.is_empty());
    }

    #[tokio::test]
    async fn noop_compressor_records_count() {
        let rounds = vec![round(1, "x", true), round(2, "y", true)];
        let s = NoopCompressor.compress(&rounds, "g").await.unwrap();
        assert!(s.contains("2"));
        assert!(s.contains("elided"));
    }

    /// Mock driver used to exercise [`LlmCompressor`] without a
    /// real provider. Captures the (system, user) pair so the
    /// tests can assert the summariser prompt is well-formed.
    struct MockDriver {
        reply: String,
        captured: std::sync::Mutex<Option<(String, String)>>,
    }
    #[async_trait]
    impl crate::ChatDriver for MockDriver {
        async fn propose(
            &self,
            system: &str,
            user_message: &str,
        ) -> Result<Proposal, String> {
            *self.captured.lock().unwrap() =
                Some((system.to_string(), user_message.to_string()));
            Ok(Proposal {
                text: self.reply.clone(),
                tool_calls: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn llm_compressor_packs_rounds_into_prompt_and_returns_provider_text() {
        let driver = MockDriver {
            reply: "Investigated flaky test; root cause is timing-sensitive setup.".into(),
            captured: std::sync::Mutex::new(None),
        };
        let compressor = LlmCompressor::new(&driver);
        let rounds = vec![round(1, "read_file", true), round(2, "write_file", false)];
        let summary = compressor
            .compress(&rounds, "investigate flaky test")
            .await
            .unwrap();
        assert!(summary.contains("timing-sensitive setup"));
        let (system, user) = driver.captured.lock().unwrap().clone().unwrap();
        assert!(system.contains("agent"));
        assert!(user.contains("Original goal: investigate flaky test"));
        assert!(user.contains("read_file ok"));
        assert!(user.contains("write_file failed"));
    }

    #[tokio::test]
    async fn llm_compressor_rejects_empty_reply() {
        let driver = MockDriver {
            reply: String::new(),
            captured: std::sync::Mutex::new(None),
        };
        let compressor = LlmCompressor::new(&driver);
        let err = compressor
            .compress(&[round(1, "read_file", true)], "g")
            .await
            .unwrap_err();
        assert!(err.contains("no summary"));
    }
}
