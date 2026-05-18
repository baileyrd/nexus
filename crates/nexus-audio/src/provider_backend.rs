//! Provider-routed STT + TTS. Talks to OpenAI's
//! `/v1/audio/transcriptions` and `/v1/audio/speech` directly so
//! the audio path doesn't have to round-trip through `nexus-ai`'s
//! session-bound chat stack.
//!
//! Why direct HTTP and not an `IpcContext` round-trip into
//! `nexus-ai`? Two reasons:
//!
//! 1. `nexus-ai` exposes chat / embeddings / RAG — no audio handlers
//!    today. Adding three new handlers there just to forward bytes
//!    is more wiring than the value justifies for v1.
//! 2. The OpenAI audio API uses multipart upload (STT) and binary
//!    response (TTS), shapes `nexus-ai`'s JSON-only IPC surface
//!    can't carry cleanly without base64-everywhere overhead.
//!
//! The two crates still share keyring conventions — config.rs reads
//! `OPENAI_API_KEY` the same way `nexus-ai::config::detect_provider`
//! does, so a forge configured for chat works for audio out of the
//! box.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use base64::Engine;
use nexus_kernel::{KernelPluginContext, PluginContext};
use reqwest::multipart;
use tokio::runtime::Handle;

use crate::backend::{
    AudioFormat, SttProvider, SynthesisOutput, TranscriptionInput, TranscriptionOutput,
    TtsProvider,
};
use crate::config::AudioConfig;
use crate::AudioError;

/// Shared kernel-context slot. [`crate::AudioCorePlugin`] owns the
/// `RwLock`; the provider backend captures a clone at construction
/// time and reads it on every call so a `wire_context` that arrives
/// after `on_init` is still observed without rebuilding the backend.
pub type SharedCtx = Arc<RwLock<Option<Arc<KernelPluginContext>>>>;

const BACKEND_NAME: &str = "provider";
/// P2-05 — fallback OpenAI base URL used by the provider-routed
/// audio backend when neither `com.nexus.ai::resolve_credentials` nor
/// `[audio] provider_base_url` supplies one. Override per-forge via
/// `[audio] provider_base_url = "..."` in `config.toml` or by exporting
/// `OPENAI_BASE_URL`.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const CREDS_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

/// Credentials resolved at call time. Prefers `com.nexus.ai`'s live
/// chat-provider config when reachable through the shared kernel
/// context (so a runtime `set_config` push from the shell is
/// observed); falls back to `OPENAI_API_KEY` env / `[audio]
/// provider_*` TOML otherwise.
struct ResolvedCreds {
    api_key: String,
    base_url: String,
}

/// Look up the API key + base URL for the provider call.
///
/// Order of preference (BL-117 — "use the configured AI or fall
/// back to env"):
///   1. `com.nexus.ai::resolve_credentials` when the kernel
///      context is wired AND its reply has a non-empty api_key.
///   2. The `AudioConfig` snapshot (which already merged env
///      `OPENAI_API_KEY` + config.toml at load time).
///
/// Returns [`AudioError::Misconfigured`] when neither source has a
/// key, so the dispatch surfaces a clear error rather than POSTing
/// an empty Bearer token.
async fn resolve_creds(cfg: &AudioConfig, ctx: &SharedCtx) -> Result<ResolvedCreds, AudioError> {
    // Snapshot the context handle so we don't hold the read lock
    // across the await.
    let ctx_opt = ctx.read().ok().and_then(|g| g.clone());
    if let Some(kctx) = ctx_opt {
        let call = kctx
            .ipc_call(
                "com.nexus.ai",
                "resolve_credentials",
                serde_json::json!({}),
                CREDS_LOOKUP_TIMEOUT,
            )
            .await;
        if let Ok(value) = call {
            if let (Some(api_key), base_url) = (
                value.get("api_key").and_then(serde_json::Value::as_str),
                value.get("base_url").and_then(serde_json::Value::as_str),
            ) {
                if !api_key.is_empty() {
                    let base = base_url
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
                    return Ok(ResolvedCreds {
                        api_key: api_key.to_string(),
                        base_url: base,
                    });
                }
            }
        }
    }
    if let Some(key) = cfg.provider_api_key.clone().filter(|s| !s.is_empty()) {
        let base = cfg
            .provider_base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        return Ok(ResolvedCreds {
            api_key: key,
            base_url: base,
        });
    }
    Err(AudioError::Misconfigured {
        backend: BACKEND_NAME.to_string(),
        reason: "no API key found via `com.nexus.ai::resolve_credentials` or `OPENAI_API_KEY`; \
                 configure an AI provider in Settings → AI or export OPENAI_API_KEY"
            .to_string(),
    })
}

/// STT backend that POSTs to `/v1/audio/transcriptions` using the
/// configured API key.
pub struct ProviderRoutedStt {
    cfg: AudioConfig,
    ctx: SharedCtx,
}

impl ProviderRoutedStt {
    /// Construct from a resolved [`AudioConfig`] and the shared
    /// kernel-context slot. The context is read at call time; a
    /// `None` slot falls back to config / env credentials.
    #[must_use]
    pub fn new(cfg: AudioConfig, ctx: SharedCtx) -> Self {
        Self { cfg, ctx }
    }
}

impl SttProvider for ProviderRoutedStt {
    fn name(&self) -> &'static str {
        BACKEND_NAME
    }

    fn transcribe(
        &mut self,
        input: TranscriptionInput,
    ) -> Result<TranscriptionOutput, AudioError> {
        let cfg = self.cfg.clone();
        let ctx = Arc::clone(&self.ctx);
        let model = self.cfg.provider_stt_model.clone();
        let format = input.format;
        let language = input.language.clone();
        let bytes = input.bytes;

        run_async(async move {
            let ResolvedCreds { api_key, base_url } = resolve_creds(&cfg, &ctx).await?;
            let client = build_http_client(cfg.tls_pinning_enabled);
            let url = format!("{}/v1/audio/transcriptions", base_url.trim_end_matches('/'));
            let filename = format!("audio.{}", format.extension());
            let mut form = multipart::Form::new()
                .text("model", model)
                .text("response_format", "json")
                .part(
                    "file",
                    multipart::Part::bytes(bytes).file_name(filename),
                );
            if let Some(lang) = language {
                form = form.text("language", lang);
            }
            let resp = client
                .post(&url)
                .bearer_auth(&api_key)
                .multipart(form)
                .send()
                .await?;
            let status = resp.status();
            let body = resp.text().await?;
            if !status.is_success() {
                return Err(AudioError::Backend {
                    backend: BACKEND_NAME.to_string(),
                    reason: format!("transcribe HTTP {status}: {body}"),
                });
            }
            // OpenAI's `response_format: json` returns
            // `{ "text": "…" }` plus optional `language`. We parse
            // loosely so a backend that adds fields doesn't break us.
            let parsed: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| AudioError::Backend {
                    backend: BACKEND_NAME.to_string(),
                    reason: format!("transcribe response parse: {e}: {body}"),
                })?;
            let text = parsed
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let language = parsed
                .get("language")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            Ok(TranscriptionOutput { text, language })
        })
    }
}

/// TTS backend that POSTs to `/v1/audio/speech` and decodes the
/// binary response.
pub struct ProviderRoutedTts {
    cfg: AudioConfig,
    ctx: SharedCtx,
}

impl ProviderRoutedTts {
    /// Construct from a resolved [`AudioConfig`] and the shared
    /// kernel-context slot.
    #[must_use]
    pub fn new(cfg: AudioConfig, ctx: SharedCtx) -> Self {
        Self { cfg, ctx }
    }
}

impl TtsProvider for ProviderRoutedTts {
    fn name(&self) -> &'static str {
        BACKEND_NAME
    }

    fn synthesize(
        &mut self,
        text: &str,
        voice: Option<&str>,
        format: AudioFormat,
    ) -> Result<SynthesisOutput, AudioError> {
        let cfg = self.cfg.clone();
        let ctx = Arc::clone(&self.ctx);
        let model = self.cfg.provider_tts_model.clone();
        let voice = voice
            .map_or_else(|| self.cfg.provider_tts_voice.clone(), str::to_string);
        let text = text.to_string();

        // OpenAI accepts `mp3` / `opus` / `aac` / `flac` / `wav` /
        // `pcm` for the `response_format` field. We map our enum to
        // the closest supported value and remember what we asked for
        // so the reply field matches the bytes. WebM isn't a
        // supported container — fall back to MP3 and let the reply
        // reflect what actually came back.
        let (response_format, returned_format) = match format {
            AudioFormat::Wav => ("wav", AudioFormat::Wav),
            AudioFormat::Opus => ("opus", AudioFormat::Opus),
            AudioFormat::Mp3 | AudioFormat::Webm => ("mp3", AudioFormat::Mp3),
        };

        run_async(async move {
            let ResolvedCreds { api_key, base_url } = resolve_creds(&cfg, &ctx).await?;
            let client = build_http_client(cfg.tls_pinning_enabled);
            let url = format!("{}/v1/audio/speech", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": model,
                "voice": voice,
                "input": text,
                "response_format": response_format,
            });
            let resp = client
                .post(&url)
                .bearer_auth(&api_key)
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            if !status.is_success() {
                let err_body = resp.text().await.unwrap_or_default();
                return Err(AudioError::Backend {
                    backend: BACKEND_NAME.to_string(),
                    reason: format!("synthesize HTTP {status}: {err_body}"),
                });
            }
            let bytes = resp.bytes().await?.to_vec();
            Ok(SynthesisOutput {
                bytes,
                format: returned_format,
            })
        })
    }
}

/// Run an async future to completion from a sync context. Reuses the
/// caller's tokio runtime when one is already in scope (the kernel's
/// dispatch path), otherwise spins up a single-threaded runtime for
/// this call only. Same shape as the WI-09 / BL-061 monitor threads
/// pattern in nexus-terminal — keeps async-on-sync clear at every
/// call site.
fn run_async<F, T>(fut: F) -> Result<T, AudioError>
where
    F: std::future::Future<Output = Result<T, AudioError>>,
{
    if let Ok(handle) = Handle::try_current() {
        // Inside a tokio runtime — block on the future without
        // re-entering. `block_on` from inside a runtime panics on a
        // multi-thread runtime; switch to `block_in_place` so the
        // kernel's worker pool absorbs the wait.
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        // No runtime — build a single-threaded one for this call.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(AudioError::Io)?;
        let result = rt.block_on(fut);
        // Drop the runtime on a fresh OS thread. `Handle::try_current()`
        // returning `Err` does not guarantee the calling thread is fully
        // outside tokio (e.g. a kernel `spawn_blocking` site whose
        // task-local handle was cleared, or a re-entered `block_on`),
        // and the blocking-pool shutdown in `Runtime::drop` panics if it
        // runs inside an async context. Moving the drop off-thread
        // sidesteps that check.
        std::thread::spawn(move || drop(rt));
        result
    }
}

fn build_http_client(tls_pinning_enabled: bool) -> reqwest::Client {
    // BL-102: same TLS-pinning gate `nexus-ai` uses, sourced from
    // `AudioConfig::tls_pinning_enabled` (bootstrapped from
    // `KernelConfig::tls_pinning_enabled`). With pinning off this is a
    // stock client — no behaviour change vs. pre-BL-102 audio calls.
    nexus_security::tls::build_pinned_client(tls_pinning_enabled)
}

/// Base64 codec used by the IPC boundary. Exported so the core
/// plugin can re-use the same engine selection (URL-safe vs.
/// standard); MIME-style standard alphabet is what we ship for
/// audio blobs since the shell already encodes captures that way.
pub(crate) fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Convenience: encode bytes to base64 for an IPC reply.
pub(crate) fn encode_b64(bytes: &[u8]) -> String {
    b64().encode(bytes)
}

/// Convenience: decode base64 from an IPC arg.
pub(crate) fn decode_b64(s: &str) -> Result<Vec<u8>, AudioError> {
    b64().decode(s).map_err(AudioError::Base64)
}
