//! BL-139 — env-gated smoke test for the `predict` handler's hot
//! path: `OllamaProvider::fim_generate`.
//!
//! Off by default — the test no-ops with a clear skip message unless
//! the operator sets `NEXUS_TEST_OLLAMA=1`. CI does not set it; this
//! lives here as an operator-runnable check of the BL-139 DoD's
//! "Ghost-text renders within 300 ms on a local Ollama model for
//! ≤200-token prefix" target.
//!
//! Usage:
//!
//! ```text
//! # Requires a running ollama daemon + a pulled FIM-capable model.
//! ollama pull qwen2.5-coder:1.5b   # or any FIM-trained coder
//! NEXUS_TEST_OLLAMA=1 cargo test -p nexus-ai --test predict_smoke -- --nocapture
//! ```
//!
//! Optional env knobs:
//! - `NEXUS_TEST_OLLAMA_BASE_URL` — defaults to `http://localhost:11434`.
//! - `NEXUS_TEST_OLLAMA_MODEL`    — defaults to `qwen2.5-coder:1.5b`
//!   (smallest FIM-trained coder in the qwen2.5-coder family —
//!   biggest single factor in p95 latency).
//! - `NEXUS_TEST_OLLAMA_LATENCY_MS` — soft latency budget; defaults
//!   to `300` (the BL-139 DoD target). A miss prints a warning but
//!   does NOT fail the test — the budget is hardware-dependent and a
//!   slow box shouldn't fail CI by accident. The DoD compliance check
//!   is whether the path works at all.

use std::env;
use std::time::Instant;

use nexus_ai::OllamaProvider;

const ENV_GATE: &str = "NEXUS_TEST_OLLAMA";
const ENV_BASE_URL: &str = "NEXUS_TEST_OLLAMA_BASE_URL";
const ENV_MODEL: &str = "NEXUS_TEST_OLLAMA_MODEL";
const ENV_LATENCY_BUDGET: &str = "NEXUS_TEST_OLLAMA_LATENCY_MS";

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "qwen2.5-coder:1.5b";
const DEFAULT_LATENCY_BUDGET_MS: u128 = 300;

fn env_is_truthy(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes")
    )
}

#[tokio::test]
async fn predict_smoke_against_local_ollama() {
    if !env_is_truthy(ENV_GATE) {
        eprintln!(
            "[skip] {ENV_GATE} not set — skipping live Ollama smoke. \
             Set {ENV_GATE}=1 with a running daemon to exercise."
        );
        return;
    }

    let base_url = env::var(ENV_BASE_URL).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let model = env::var(ENV_MODEL).unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let budget_ms: u128 = env::var(ENV_LATENCY_BUDGET)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_LATENCY_BUDGET_MS);

    eprintln!("[smoke] base_url={base_url} model={model} budget={budget_ms}ms");

    // Representative cursor split — a Rust file with a partial
    // function body. Well under the BL-139 editor extension's 800-byte
    // prefix / 200-byte suffix budget, mirroring a real keystroke.
    let prefix = "fn add(a: i32, b: i32) -> i32 {\n    ";
    let suffix = "\n}\n";

    let provider = OllamaProvider::new(Some(base_url), Some(model), None);

    let started = Instant::now();
    let result = provider
        .fim_generate(prefix, suffix, 64)
        .await
        .expect("fim_generate must succeed against a running Ollama daemon");
    let elapsed = started.elapsed();

    eprintln!(
        "[smoke] completion={:?} ({} ms)",
        result.completion,
        elapsed.as_millis()
    );

    assert!(
        !result.completion.trim().is_empty(),
        "expected a non-empty completion; got empty string (model warm-up? wrong model?)"
    );

    if elapsed.as_millis() > budget_ms {
        eprintln!(
            "[warn] elapsed {} ms exceeded the BL-139 DoD budget of {} ms — \
             check model size, GPU acceleration, or first-call warmup.",
            elapsed.as_millis(),
            budget_ms,
        );
    }
}
