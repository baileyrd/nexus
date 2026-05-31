//! `com.nexus.audio` core plugin (BL-117).
//!
//! Three IPC handlers — `transcribe`, `synthesize`, `status` — routed
//! to the configured [`crate::AudioBackends`] pair. Capability gates
//! are enforced upstream by the kernel (manifest in
//! `nexus-bootstrap`); this module trusts that dispatch only lands
//! here when the caller has the matching capability.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use nexus_kernel::KernelPluginContext;
use nexus_plugins::{CorePlugin, PluginError};

use crate::backend::AudioBackends;
use crate::config::AudioConfig;
use crate::ipc::{
    AudioStatusResult, AudioSynthesizeArgs, AudioSynthesizeResult, AudioTranscribeArgs,
    AudioTranscribeResult,
};
use crate::provider_backend::{decode_b64, encode_b64, SharedCtx};
use crate::{AudioError, AudioFormat};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.audio";

/// Handler id for `transcribe`. Args: [`AudioTranscribeArgs`]; reply
/// [`AudioTranscribeResult`].
pub const HANDLER_TRANSCRIBE: u32 = 1;
/// Handler id for `synthesize`. Args: [`AudioSynthesizeArgs`]; reply
/// [`AudioSynthesizeResult`].
pub const HANDLER_SYNTHESIZE: u32 = 2;
/// Handler id for `status`. No args; reply [`AudioStatusResult`].
pub const HANDLER_STATUS: u32 = 3;

/// Plugin ids this plugin invokes at handler-dispatch time. `ai` is
/// the credentials backstop the `provider` backend uses for OPENAI_API_KEY
/// resolution (see `provider_backend::AudioProviderBackend`).
pub const MANIFEST_DEPS: &[&str] = &["com.nexus.ai"];

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::audio::register`.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("transcribe", HANDLER_TRANSCRIBE),
    ("synthesize", HANDLER_SYNTHESIZE),
    ("status", HANDLER_STATUS),
];

/// Core plugin. Owns the configured backend pair behind a mutex so
/// IPC dispatches can serialise into the backend without forcing
/// every backend to be `Sync`. Holds a shared `Arc<RwLock<_>>`
/// kernel-context slot that the provider-routed backend reads at
/// call time to ask `com.nexus.ai::resolve_credentials` for the
/// active chat provider's api_key (BL-117 — "use the configured AI
/// or fall back to env").
pub struct AudioCorePlugin {
    forge_root: PathBuf,
    backends: Mutex<Option<AudioBackends>>,
    /// Shared so the provider backend constructed in `on_init` can
    /// observe the context that lands later through `wire_context`.
    context: SharedCtx,
}

impl AudioCorePlugin {
    /// Construct an unstarted plugin. `on_init` loads the config and
    /// builds the backend pair.
    #[must_use]
    pub fn new(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            backends: Mutex::new(None),
            context: Arc::new(RwLock::new(None)),
        }
    }

    /// Construct with pre-built backends (test injection + the BL-118
    /// shell-side contribution path). Skips the config load so callers
    /// can wire mocks without writing a TOML file.
    #[must_use]
    pub fn with_backends(forge_root: PathBuf, backends: AudioBackends) -> Self {
        Self {
            forge_root,
            backends: Mutex::new(Some(backends)),
            context: Arc::new(RwLock::new(None)),
        }
    }
}

impl CorePlugin for AudioCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        // Skip re-init when `with_backends` was used.
        if self
            .backends
            .lock()
            .expect("audio backends mutex")
            .is_some()
        {
            return Ok(());
        }
        let cfg = AudioConfig::load(&self.forge_root).map_err(|e| PluginError::LifecycleError {
            plugin_id: PLUGIN_ID.to_string(),
            hook: "on_init".to_string(),
            reason: format!("load config: {e}"),
        })?;
        let pair = cfg.build_backends(Arc::clone(&self.context));
        *self.backends.lock().expect("audio backends mutex") = Some(pair);
        Ok(())
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        if let Ok(mut g) = self.context.write() {
            *g = Some(ctx);
        }
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_TRANSCRIBE => self.dispatch_transcribe(args),
            HANDLER_SYNTHESIZE => self.dispatch_synthesize(args),
            HANDLER_STATUS => self.dispatch_status(),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl AudioCorePlugin {
    fn dispatch_transcribe(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: AudioTranscribeArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("transcribe: invalid args: {e}")))?;
        let bytes = decode_b64(&a.audio_b64).map_err(audio_err)?;
        let format = AudioFormat::parse_or_default(a.format.as_deref());
        let mut guard = self.backends.lock().expect("audio backends mutex");
        let backends = guard.as_mut().ok_or_else(|| {
            exec_err("audio backends not initialised (on_init did not run)".to_string())
        })?;
        let stt = backends.stt_mut();
        let backend_name = stt.name().to_string();
        let out = stt
            .transcribe(crate::backend::TranscriptionInput {
                bytes,
                format,
                language: a.language,
            })
            .map_err(audio_err)?;
        serde_json::to_value(&AudioTranscribeResult {
            text: out.text,
            language: out.language,
            backend: backend_name,
        })
        .map_err(|e| exec_err(format!("transcribe: serialize: {e}")))
    }

    fn dispatch_synthesize(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: AudioSynthesizeArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("synthesize: invalid args: {e}")))?;
        let format = AudioFormat::parse_or_default(a.format.as_deref());
        let mut guard = self.backends.lock().expect("audio backends mutex");
        let backends = guard.as_mut().ok_or_else(|| {
            exec_err("audio backends not initialised (on_init did not run)".to_string())
        })?;
        let tts = backends.tts_mut();
        let backend_name = tts.name().to_string();
        let out = tts
            .synthesize(&a.text, a.voice.as_deref(), format)
            .map_err(audio_err)?;
        serde_json::to_value(&AudioSynthesizeResult {
            audio_b64: encode_b64(&out.bytes),
            format: out.format.as_str().to_string(),
            backend: backend_name,
        })
        .map_err(|e| exec_err(format!("synthesize: serialize: {e}")))
    }

    fn dispatch_status(&self) -> Result<serde_json::Value, PluginError> {
        let guard = self.backends.lock().expect("audio backends mutex");
        let backends = guard.as_ref().ok_or_else(|| {
            exec_err("audio backends not initialised (on_init did not run)".to_string())
        })?;
        let (stt, tts) = backends.names();
        serde_json::to_value(&AudioStatusResult {
            stt_backend: stt.to_string(),
            tts_backend: tts.to_string(),
        })
        .map_err(|e| exec_err(format!("status: serialize: {e}")))
    }
}

nexus_plugins::define_dispatch_helpers!();

// Pass-by-value matches `Result::map_err`'s contract; clippy's
// needless_pass_by_value would force a closure at every call site.
#[allow(clippy::needless_pass_by_value)]
fn audio_err(e: AudioError) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{
        SttProvider, SynthesisOutput, TranscriptionInput, TranscriptionOutput, TtsProvider,
    };
    use tempfile::tempdir;

    struct MockStt {
        calls: std::sync::Arc<std::sync::Mutex<Vec<TranscriptionInput>>>,
    }
    impl SttProvider for MockStt {
        fn name(&self) -> &'static str {
            "mock-stt"
        }
        fn transcribe(
            &mut self,
            input: TranscriptionInput,
        ) -> Result<TranscriptionOutput, AudioError> {
            self.calls.lock().unwrap().push(input);
            Ok(TranscriptionOutput {
                text: "hello world".to_string(),
                language: Some("en".to_string()),
            })
        }
    }
    struct MockTts;
    impl TtsProvider for MockTts {
        fn name(&self) -> &'static str {
            "mock-tts"
        }
        fn synthesize(
            &mut self,
            text: &str,
            _voice: Option<&str>,
            format: AudioFormat,
        ) -> Result<SynthesisOutput, AudioError> {
            Ok(SynthesisOutput {
                bytes: text.as_bytes().to_vec(),
                format,
            })
        }
    }

    fn plugin_with_mocks() -> (
        AudioCorePlugin,
        std::sync::Arc<std::sync::Mutex<Vec<TranscriptionInput>>>,
    ) {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let dir = tempdir().unwrap();
        let backends = AudioBackends::new(
            Box::new(MockStt {
                calls: calls.clone(),
            }),
            Box::new(MockTts),
        );
        let plugin = AudioCorePlugin::with_backends(dir.path().to_path_buf(), backends);
        (plugin, calls)
    }

    #[test]
    fn transcribe_round_trips_bytes_and_returns_text() {
        let (mut plugin, calls) = plugin_with_mocks();
        let audio_b64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"raw-bytes");
        let v = plugin
            .dispatch(
                HANDLER_TRANSCRIBE,
                &serde_json::json!({
                    "audio_b64": audio_b64,
                    "format": "wav",
                    "language": "en",
                }),
            )
            .unwrap();
        assert_eq!(v["text"], "hello world");
        assert_eq!(v["language"], "en");
        assert_eq!(v["backend"], "mock-stt");
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].bytes, b"raw-bytes");
        assert_eq!(calls[0].format, AudioFormat::Wav);
        assert_eq!(calls[0].language.as_deref(), Some("en"));
    }

    #[test]
    fn synthesize_returns_base64_audio() {
        let (mut plugin, _) = plugin_with_mocks();
        let v = plugin
            .dispatch(
                HANDLER_SYNTHESIZE,
                &serde_json::json!({
                    "text": "hi",
                    "format": "wav",
                }),
            )
            .unwrap();
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            v["audio_b64"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(decoded, b"hi");
        assert_eq!(v["format"], "wav");
        assert_eq!(v["backend"], "mock-tts");
    }

    #[test]
    fn status_returns_backend_pair() {
        let (mut plugin, _) = plugin_with_mocks();
        let v = plugin
            .dispatch(HANDLER_STATUS, &serde_json::json!({}))
            .unwrap();
        assert_eq!(v["stt_backend"], "mock-stt");
        assert_eq!(v["tts_backend"], "mock-tts");
    }

    #[test]
    fn transcribe_rejects_invalid_base64() {
        let (mut plugin, _) = plugin_with_mocks();
        let err = plugin
            .dispatch(
                HANDLER_TRANSCRIBE,
                &serde_json::json!({ "audio_b64": "***not-base64***" }),
            )
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("base64") || msg.contains("Invalid"), "{msg}");
    }

    #[test]
    fn dispatch_unknown_handler_fails() {
        let (mut plugin, _) = plugin_with_mocks();
        let err = plugin.dispatch(999, &serde_json::json!({})).unwrap_err();
        assert!(err.to_string().contains("unknown handler id 999"));
    }

    #[test]
    fn on_init_builds_default_backends_when_no_config_file() {
        let dir = tempdir().unwrap();
        let mut plugin = AudioCorePlugin::new(dir.path().to_path_buf());
        plugin.on_init().unwrap();
        let v = plugin
            .dispatch(HANDLER_STATUS, &serde_json::json!({}))
            .unwrap();
        // Default backend is `Platform` (config.rs::AudioConfig::default)
        // — the Web Speech API contributed by the `nexus.audio` shell
        // plugin. This test was previously stale on `"local"`; surfaced
        // by the #185 drift-script run, would have been caught by the
        // new #184 CI gate.
        assert_eq!(v["stt_backend"], "platform");
        assert_eq!(v["tts_backend"], "platform");
    }
}
