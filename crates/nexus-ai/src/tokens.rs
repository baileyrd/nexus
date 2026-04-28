//! Token counting and budget management for AI context assembly.
//!
//! PRD-12 §12. The budget tracks how many tokens have been allocated to
//! each context source kind (RAG chunks, system prompt, user prompt, …),
//! reserves headroom for the model's response, and reports utilisation so
//! callers can warn the user before the prompt blows past the model's
//! context window.
//!
//! No tokenizer crate is pulled in — counting uses a local approximation
//! of `(text.len() + 3) / 4` (≈ 4 chars per token) which is good enough
//! for budget arithmetic across English-language chunks. Provider request
//! bodies still cap generation length via [`crate::AiConfig::max_tokens`].

use std::collections::HashMap;

/// Strategy for counting tokens in a string of text.
///
/// Implementations are deliberately cheap; counters used by
/// [`TokenBudget`] should never block or perform I/O.
pub trait TokenCounter: Send + Sync {
    /// Return the (approximate) token count of `text`.
    fn count_tokens(&self, text: &str) -> usize;
}

/// Approximation that returns `(text.len() + 3) / 4`.
///
/// Matches the rough rule-of-thumb used by Anthropic's docs ("≈4 chars per
/// token" for English) and avoids pulling a tokenizer dependency. Good
/// enough for budget arithmetic; do **not** use it for billing.
#[derive(Debug, Default, Clone, Copy)]
pub struct ApproxTokenCounter;

impl TokenCounter for ApproxTokenCounter {
    fn count_tokens(&self, text: &str) -> usize {
        // PRD-12 §12.1: ~4 chars per token. Equivalent to `len.div_ceil(4)`
        // but spelled the way the PRD spells it.
        #[allow(clippy::manual_div_ceil)]
        {
            (text.len() + 3) / 4
        }
    }
}

/// Identifies which slice of the assembled context a budget allocation
/// belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextSourceKind {
    /// A retrieved RAG chunk attached to the system prompt.
    RagChunk,
    /// The base system prompt (instructions, persona, …).
    SystemPrompt,
    /// The user's prompt / latest message.
    UserPrompt,
    /// Anything else (tool output, scratchpad, …).
    Other,
}

/// Tracks token allocations against a fixed context window, leaving room
/// for the model's response.
///
/// `total` is the model's full context window. `reserved_for_response`
/// is carved out of `total` up-front so generation still has room. The
/// remaining headroom is divvied out via [`TokenBudget::allocate`]; an
/// allocation that would exceed the headroom is rejected and leaves the
/// budget unchanged.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    total: usize,
    reserved_for_response: usize,
    allocated: HashMap<ContextSourceKind, usize>,
}

impl TokenBudget {
    /// Create a new budget for a model with `total`-token context window,
    /// reserving `reserved` tokens for the response.
    #[must_use]
    pub fn new(total: usize, reserved: usize) -> Self {
        Self {
            total,
            reserved_for_response: reserved,
            allocated: HashMap::new(),
        }
    }

    /// Sum of every prior [`allocate`](Self::allocate) accepted by this
    /// budget.
    #[must_use]
    pub fn used(&self) -> usize {
        self.allocated.values().sum()
    }

    /// Tokens still available for context. Equal to
    /// `total - reserved_for_response - used`, saturating at 0.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.total
            .saturating_sub(self.reserved_for_response)
            .saturating_sub(self.used())
    }

    /// Charge `tokens` against `kind`. Returns `true` if the allocation
    /// fit and was recorded; returns `false` and leaves the budget
    /// untouched if it would have overflowed the remaining headroom.
    ///
    /// Allocating the same `kind` twice is additive — the new charge is
    /// summed onto any existing allocation for that kind.
    pub fn allocate(&mut self, kind: ContextSourceKind, tokens: usize) -> bool {
        if tokens > self.remaining() {
            return false;
        }
        *self.allocated.entry(kind).or_insert(0) += tokens;
        true
    }

    /// Fraction of the *available* (post-reservation) budget that has
    /// been allocated. `0.0` when nothing is allocated, `1.0` when full.
    /// Returns `0.0` when the available budget is zero (nothing to
    /// utilise).
    #[must_use]
    pub fn utilization(&self) -> f32 {
        let available = self.total.saturating_sub(self.reserved_for_response);
        if available == 0 {
            return 0.0;
        }
        // `as` casts on usize -> f32 are intentional and bounded by the
        // budget sizes callers pass in.
        #[allow(clippy::cast_precision_loss)]
        let used = self.used() as f32;
        #[allow(clippy::cast_precision_loss)]
        let available = available as f32;
        used / available
    }
}

/// Diagnostic emitted while assembling a budgeted prompt.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetWarning {
    /// Total post-assembly utilisation crossed the 80 % threshold.
    NearLimit {
        /// Realised utilisation in `[0.0, 1.0]`.
        utilization: f32,
    },
    /// A source was dropped because it didn't fit the remaining budget.
    SourceDropped {
        /// Which kind of source was dropped.
        kind: ContextSourceKind,
        /// How many tokens the dropped source would have cost.
        tokens: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approx_counter_returns_chars_div_4_rounded_up() {
        let counter = ApproxTokenCounter;
        // "hello world" = 11 chars -> ceil(11/4) = 3.
        assert_eq!(counter.count_tokens("hello world"), 3);
        // empty -> 0.
        assert_eq!(counter.count_tokens(""), 0);
        // single char rounds up to 1.
        assert_eq!(counter.count_tokens("a"), 1);
        // exact multiple of 4.
        assert_eq!(counter.count_tokens("abcd"), 1);
        assert_eq!(counter.count_tokens("abcde"), 2);
    }

    #[test]
    fn budget_remaining_subtracts_reserved_and_used() {
        let mut budget = TokenBudget::new(10_000, 2_000);
        assert_eq!(budget.remaining(), 8_000);
        assert!(budget.allocate(ContextSourceKind::RagChunk, 3_000));
        assert_eq!(budget.remaining(), 5_000);
        assert_eq!(budget.used(), 3_000);
    }

    #[test]
    fn budget_allocate_returns_false_when_overflow() {
        let mut budget = TokenBudget::new(1_000, 200);
        // Available is 800; first allocation fits.
        assert!(budget.allocate(ContextSourceKind::SystemPrompt, 500));
        let snapshot_used = budget.used();
        let snapshot_remaining = budget.remaining();
        // 400 more would overflow (only 300 left).
        assert!(!budget.allocate(ContextSourceKind::RagChunk, 400));
        // State unchanged.
        assert_eq!(budget.used(), snapshot_used);
        assert_eq!(budget.remaining(), snapshot_remaining);
    }

    #[test]
    fn budget_utilization_reports_used_over_available() {
        let mut budget = TokenBudget::new(10_000, 2_000);
        assert!(budget.allocate(ContextSourceKind::RagChunk, 4_000));
        // available = 8_000, used = 4_000 -> utilisation ≈ 0.5.
        let u = budget.utilization();
        assert!((u - 0.5).abs() < f32::EPSILON, "expected ~0.5, got {u}");
    }

    #[test]
    fn budget_utilization_zero_when_available_zero() {
        let budget = TokenBudget::new(100, 100);
        assert!((budget.utilization() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn budget_allocate_same_kind_is_additive() {
        let mut budget = TokenBudget::new(1_000, 0);
        assert!(budget.allocate(ContextSourceKind::RagChunk, 200));
        assert!(budget.allocate(ContextSourceKind::RagChunk, 300));
        assert_eq!(budget.used(), 500);
    }
}
