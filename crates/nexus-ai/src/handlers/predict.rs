//! BL-139 — `predict` IPC handler: per-keystroke edit prediction.
//!
//! Routes to the configured AI provider:
//!   - **Ollama** (default): uses `/api/generate` with the `suffix`
//!     field so a FIM-trained code model (qwen2.5-coder, codellama,
//!     deepseek-coder, starcoder2, …) fills the cursor split natively.
//!     Non-FIM models gracefully ignore `suffix` and emit a normal
//!     continuation of `prefix`, which is still useful as ghost text.
//!   - **OpenAI / Anthropic**: chat-shaped FIM prompt — both providers
//!     ship code-model variants without a dedicated FIM endpoint, so
//!     we describe the task in the system prompt and put the
//!     `<PREFIX>…</PREFIX><SUFFIX>…</SUFFIX>` markers in a user turn.
//!     The model is asked to emit only the missing middle.
//!
//! Output is post-processed by [`sanitize_completion`]: strip leading
//! whitespace at the seam, drop a leading copy of `suffix` (some
//! models echo the suffix back), and apply a single hard cap on
//! length so a runaway model doesn't pin the editor with a 10 KB
//! ghost.

use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::exec_err;
use crate::ipc::{AiPredictArgs, AiPredictReply};
use crate::ollama::OllamaProvider;
use crate::provider::{AiProvider, ChatMessage, Role};

/// Default token cap when the caller omits `max_tokens`.
pub(crate) const DEFAULT_MAX_TOKENS: u32 = 64;

/// Absolute character ceiling on the returned completion. Even with
/// `max_tokens = 64`, a tokeniser quirk or a runaway code model can
/// surface a long blob — capping in characters keeps the ghost-widget
/// render bounded.
pub(crate) const COMPLETION_CHAR_CAP: usize = 2048;

pub(crate) async fn handle_predict(
    ai_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiPredictArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("predict: args decode: {e}")))?;

    let ai_cfg =
        ai_cfg.ok_or_else(|| exec_err("predict: no AI chat provider configured"))?;

    let max_tokens = parsed.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

    let raw = match ai_cfg.provider.as_str() {
        "ollama" => {
            let provider = OllamaProvider::new(
                ai_cfg.base_url.clone(),
                ai_cfg.model.clone(),
                None,
            );
            provider
                .fim_generate(&parsed.prefix, &parsed.suffix, max_tokens)
                .await
                .map_err(|e| exec_err(format!("predict: ollama: {e}")))?
                .completion
        }
        "anthropic" | "openai" => {
            // Cloud providers don't expose a FIM endpoint — pose it as
            // a chat task. Keep the system prompt very short: tokens
            // here are paid on every keystroke.
            let ai = crate::handlers::shared::build_ai_provider(&ai_cfg).map_err(exec_err)?;
            chat_fim_fallback(ai.as_ref(), &parsed, max_tokens).await?
        }
        other => {
            return Err(exec_err(format!(
                "predict: provider '{other}' does not support code prediction"
            )));
        }
    };

    let completion = sanitize_completion(&raw, &parsed.suffix);
    let reply = AiPredictReply { completion };
    serde_json::to_value(&reply)
        .map_err(|e| exec_err(format!("predict: encode reply: {e}")))
}

async fn chat_fim_fallback(
    ai: &dyn AiProvider,
    args: &AiPredictArgs,
    _max_tokens: u32,
) -> Result<String, PluginError> {
    let system = format!(
        "You are a code completion engine. Given a code file with a cursor split into <PREFIX> and <SUFFIX>, emit ONLY the characters that should appear at the cursor — the missing middle. No prose, no markdown fences, no explanations. Language hint: {}.",
        args.language
    );
    let user = format!(
        "<PREFIX>{}</PREFIX>\n<SUFFIX>{}</SUFFIX>\n\nMissing middle:",
        args.prefix, args.suffix
    );
    let messages = [ChatMessage {
        role: Role::User,
        content: user,
    }];
    ai.chat(&messages, Some(&system))
        .await
        .map_err(|e| exec_err(format!("predict: chat fallback: {e}")))
}

/// Trim a model's raw output to something safe to splice at the
/// cursor. Pure function — exercised directly in unit tests.
pub(crate) fn sanitize_completion(raw: &str, suffix: &str) -> String {
    // 1. Strip leading whitespace — the model often pads with a
    //    space or newline at the seam, which clashes with the
    //    character the user just typed.
    let trimmed = raw.trim_start_matches([' ', '\t']);

    // 2. Some models echo the entire suffix back at the end of the
    //    completion. Drop a trailing duplicate of `suffix` so we
    //    don't end up doubling the post-cursor text.
    let trimmed = if !suffix.is_empty() && trimmed.ends_with(suffix) {
        &trimmed[..trimmed.len() - suffix.len()]
    } else {
        trimmed
    };

    // 3. Hard cap on length. Char-bounded so we don't split a
    //    multi-byte codepoint.
    let capped: String = trimmed.chars().take(COMPLETION_CHAR_CAP).collect();
    capped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_leading_whitespace() {
        assert_eq!(sanitize_completion("   foo()", ""), "foo()");
        assert_eq!(sanitize_completion("\t\tbar", ""), "bar");
    }

    #[test]
    fn sanitize_preserves_leading_newline() {
        // A leading newline is meaningful — it's the model placing the
        // completion on a new line. Only spaces/tabs are stripped.
        assert_eq!(sanitize_completion("\nfn main() {}", ""), "\nfn main() {}");
    }

    #[test]
    fn sanitize_drops_trailing_echo_of_suffix() {
        // Model continues `let x = ` with `42;\n}` and then echoes
        // the closing `}` from the suffix.
        let raw = "42;\n}";
        let suffix = "\n}";
        let out = sanitize_completion(raw, suffix);
        assert_eq!(out, "42;");
    }

    #[test]
    fn sanitize_keeps_completion_when_suffix_is_not_a_suffix_of_raw() {
        assert_eq!(sanitize_completion("foo()", "// comment"), "foo()");
    }

    #[test]
    fn sanitize_empty_suffix_is_noop() {
        assert_eq!(sanitize_completion("hello", ""), "hello");
    }

    #[test]
    fn sanitize_caps_runaway_output() {
        let raw: String = std::iter::repeat('x').take(COMPLETION_CHAR_CAP + 500).collect();
        let out = sanitize_completion(&raw, "");
        assert_eq!(out.chars().count(), COMPLETION_CHAR_CAP);
    }

    #[test]
    fn handle_predict_errors_without_provider_config() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let args = serde_json::json!({
            "prefix": "let x = ",
            "suffix": ";\n",
            "language": "rust",
            "file_path": "src/lib.rs",
        });
        let err = rt.block_on(handle_predict(None, &args)).expect_err("expected error");
        assert!(
            format!("{err}").contains("no AI chat provider configured"),
            "expected configured error, got: {err}",
        );
    }

    #[test]
    fn handle_predict_rejects_unsupported_provider() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let cfg = AiConfig {
            provider: "local".to_string(),
            model: None,
            base_url: None,
            api_key: None,
            max_tokens: 1024,
            tls_pinning_enabled: false,
            ..Default::default()
        };
        let args = serde_json::json!({
            "prefix": "x",
            "suffix": "",
            "language": "rust",
            "file_path": "src/lib.rs",
        });
        let err = rt
            .block_on(handle_predict(Some(cfg), &args))
            .expect_err("expected error");
        assert!(
            format!("{err}").contains("does not support code prediction"),
            "expected provider-unsupported error, got: {err}",
        );
    }
}
