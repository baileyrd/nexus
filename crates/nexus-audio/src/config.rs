//! `[audio]` config loaded from `<forge>/.forge/config.toml`. See
//! BL-117 for the field semantics.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::backend::{AudioBackends, SttProvider, TtsProvider};
use crate::provider_backend::{ProviderRoutedStt, ProviderRoutedTts, SharedCtx};
use crate::stub_backend::{local_stt_stub, local_tts_stub, platform_stt_stub, platform_tts_stub};
use crate::AudioError;

/// Named backend selector. Wire form is the same lowercase string
/// (`"local"` / `"provider"` / `"platform"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioBackendName {
    /// On-device Whisper / Piper. Shipped build stubs this out behind
    /// the `local-whisper` cargo feature — set the config to `local`
    /// only when running a build that opts into that feature.
    Local,
    /// Delegate to the configured AI provider (OpenAI today). Needs
    /// `OPENAI_API_KEY` (or `[audio] provider_api_key`).
    Provider,
    /// Shell-side Web Speech API (BL-118). Default — the Rust side
    /// ships a stub that the `nexus.audio` shell plugin replaces at
    /// runtime via the BL-113 contribution path. Works once the
    /// shell plugin is enabled; until then any dispatch returns
    /// `BackendNotEnabled` like the other backends. Chosen as the
    /// shipping default because it has the lightest setup ask of the
    /// three (no API key, no model download, no cargo-feature build).
    Platform,
}

impl FromStr for AudioBackendName {
    type Err = AudioError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "provider" => Ok(Self::Provider),
            "platform" => Ok(Self::Platform),
            other => Err(AudioError::InvalidConfig(format!(
                "unknown backend '{other}' (expected local | provider | platform)"
            ))),
        }
    }
}

impl AudioBackendName {
    /// Stable string label, mirroring [`FromStr`].
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Provider => "provider",
            Self::Platform => "platform",
        }
    }
}

/// Resolved audio config. Loaded from `<forge>/.forge/config.toml`'s
/// `[audio]` block; defaults are conservative (`platform` for both —
/// the Web Speech adapter contributed by the `nexus.audio` shell
/// plugin — and `base.en` whisper model size for when operators
/// flip to `local` on a `local-whisper`-built binary).
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Backend handling `transcribe` requests.
    pub stt_backend: AudioBackendName,
    /// Backend handling `synthesize` requests.
    pub tts_backend: AudioBackendName,
    /// Local Whisper model size (`tiny.en` / `base.en` / `small.en`).
    /// Honoured by the `local-whisper`-gated backend; ignored by the
    /// other two.
    pub local_model_size: String,
    /// Directory the local Whisper backend looks in for `ggml-*.bin`
    /// model files. Populated by [`Self::load`] from the forge root
    /// (`<forge>/.forge/.audio/models`); operators can override it
    /// via `[audio] local_model_dir = "/abs/path"` in `config.toml`
    /// — useful when several forges should share one downloaded
    /// model. [`Self::default`] uses a relative `".forge/.audio/models"`
    /// so unit tests that build a default config still resolve a
    /// predictable path.
    pub local_model_dir: PathBuf,
    /// BL-102 — pin TLS connections to the provider-backend HTTPS
    /// endpoints. Sourced from `KernelConfig::tls_pinning_enabled` at
    /// boot; matches the `nexus-ai` posture so audio + chat share
    /// one pin policy. Defaults to `false` while the shipped pin
    /// table is empty.
    pub tls_pinning_enabled: bool,
    /// OpenAI API key for the provider-routed backend. When `None`
    /// the backend reports [`AudioError::Misconfigured`] on first
    /// dispatch rather than failing at boot.
    pub provider_api_key: Option<String>,
    /// Override for the provider's base URL (`OPENAI_BASE_URL` style).
    /// Empty / `None` falls through to `https://api.openai.com`.
    pub provider_base_url: Option<String>,
    /// Default model for STT (`whisper-1`).
    pub provider_stt_model: String,
    /// Default model for TTS (`tts-1` / `tts-1-hd`).
    pub provider_tts_model: String,
    /// Default voice for TTS (`alloy` / `echo` / `fable` / `onyx` /
    /// `nova` / `shimmer`). The provider rejects unknown voices.
    pub provider_tts_voice: String,
    /// P2-05 — URL template the local Whisper backend downloads from
    /// when a configured `local_model_size` is missing on disk. Must
    /// contain `{size}`, which is substituted with the model-size
    /// label (`tiny.en` / `base.en` / …). Defaults to the canonical
    /// ggerganov/whisper.cpp HuggingFace mirror — see
    /// [`DEFAULT_WHISPER_MODEL_URL_TEMPLATE`]. Override via
    /// `[audio] whisper_model_url = "..."` in `config.toml`.
    pub whisper_model_url_template: String,
    /// P2-06 — deadline for `com.nexus.ai::resolve_credentials` IPC
    /// calls from the provider-routed backend. Override via
    /// `[audio] creds_lookup_timeout_secs = N`. Default
    /// [`crate::provider_backend::DEFAULT_CREDS_LOOKUP_TIMEOUT`] (2 s).
    pub creds_lookup_timeout: std::time::Duration,
}

/// P2-05 — default URL template the local-audio backend uses to fetch
/// the `ggml-{size}.bin` Whisper weights when the configured size is
/// missing from `local_model_dir`. `{size}` is substituted at download
/// time. Override via `[audio] whisper_model_url = "..."`.
pub const DEFAULT_WHISPER_MODEL_URL_TEMPLATE: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin";

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            stt_backend: AudioBackendName::Platform,
            tts_backend: AudioBackendName::Platform,
            local_model_size: "base.en".to_string(),
            local_model_dir: PathBuf::from(".forge/.audio/models"),
            tls_pinning_enabled: false,
            provider_api_key: None,
            provider_base_url: None,
            provider_stt_model: "whisper-1".to_string(),
            provider_tts_model: "tts-1".to_string(),
            provider_tts_voice: "alloy".to_string(),
            whisper_model_url_template: DEFAULT_WHISPER_MODEL_URL_TEMPLATE.to_string(),
            creds_lookup_timeout: crate::provider_backend::DEFAULT_CREDS_LOOKUP_TIMEOUT,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    audio: Option<RawAudio>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAudio {
    stt_backend: Option<String>,
    tts_backend: Option<String>,
    local_model_size: Option<String>,
    local_model_dir: Option<PathBuf>,
    tls_pinning_enabled: Option<bool>,
    provider_api_key: Option<String>,
    provider_base_url: Option<String>,
    provider_stt_model: Option<String>,
    provider_tts_model: Option<String>,
    provider_tts_voice: Option<String>,
    whisper_model_url: Option<String>,
    creds_lookup_timeout_secs: Option<u64>,
}

impl AudioConfig {
    /// Load the `[audio]` table from `<forge>/.forge/config.toml`. A
    /// missing file or a missing `[audio]` block produces the default.
    /// Malformed TOML produces [`AudioError::InvalidConfig`] — the
    /// caller decides whether to fall back to the default or surface
    /// the error.
    ///
    /// `OPENAI_API_KEY` / `OPENAI_BASE_URL` env vars override the
    /// config-file values when present so a fresh forge running with
    /// `OPENAI_API_KEY=…` works without editing TOML — matching the
    /// `nexus-ai` provider-detection convention.
    ///
    /// # Errors
    /// Returns [`AudioError::InvalidConfig`] for parse errors or
    /// unrecognised backend names.
    pub fn load(forge_root: &Path) -> Result<Self, AudioError> {
        let path = forge_root.join(".forge").join("config.toml");
        let raw = if path.exists() {
            let text = std::fs::read_to_string(&path).map_err(AudioError::Io)?;
            toml::from_str::<RawConfig>(&text)
                .map_err(|e| AudioError::InvalidConfig(e.to_string()))?
        } else {
            RawConfig::default()
        };
        let raw = raw.audio.unwrap_or_default();
        let mut out = Self::default();
        // Anchor the default model dir to the forge root so local-audio
        // works regardless of cwd. TOML can still override (next block).
        out.local_model_dir = forge_root.join(".forge").join(".audio").join("models");
        if let Some(name) = raw.stt_backend {
            out.stt_backend = name.parse()?;
        }
        if let Some(name) = raw.tts_backend {
            out.tts_backend = name.parse()?;
        }
        if let Some(v) = raw.local_model_size {
            out.local_model_size = v;
        }
        if let Some(v) = raw.local_model_dir {
            out.local_model_dir = v;
        }
        if let Some(v) = raw.tls_pinning_enabled {
            out.tls_pinning_enabled = v;
        }
        if let Some(v) = raw.provider_api_key {
            if !v.is_empty() {
                out.provider_api_key = Some(v);
            }
        }
        if let Some(v) = raw.provider_base_url {
            if !v.is_empty() {
                out.provider_base_url = Some(v);
            }
        }
        if let Some(v) = raw.provider_stt_model {
            out.provider_stt_model = v;
        }
        if let Some(v) = raw.provider_tts_model {
            out.provider_tts_model = v;
        }
        if let Some(v) = raw.provider_tts_voice {
            out.provider_tts_voice = v;
        }
        if let Some(v) = raw.whisper_model_url {
            if !v.is_empty() {
                out.whisper_model_url_template = v;
            }
        }
        if let Some(secs) = raw.creds_lookup_timeout_secs {
            out.creds_lookup_timeout = std::time::Duration::from_secs(secs);
        }
        // Env overrides — matches nexus-ai's detect_provider convention.
        if let Ok(env_key) = std::env::var("OPENAI_API_KEY") {
            if !env_key.is_empty() {
                out.provider_api_key = Some(env_key);
            }
        }
        if let Ok(env_url) = std::env::var("OPENAI_BASE_URL") {
            if !env_url.is_empty() {
                out.provider_base_url = Some(env_url);
            }
        }
        Ok(out)
    }

    /// Build the backend pair selected by this config.
    ///
    /// `shared_ctx` is the kernel-context slot owned by
    /// [`crate::AudioCorePlugin`]; the provider-routed backend
    /// reads it at call time to issue an `ipc_call` for the active
    /// AI provider's credentials. Pass `None` (or a never-populated
    /// handle) and the provider backend falls back to env / TOML
    /// only.
    ///
    /// Backend construction itself is infallible — disabled /
    /// missing dependencies surface as
    /// [`AudioError::BackendNotEnabled`](crate::AudioError::BackendNotEnabled)
    /// or [`AudioError::Misconfigured`](crate::AudioError::Misconfigured)
    /// from the first dispatch instead of at boot, so a forge with
    /// a missing API key is still bootable (the user can still edit
    /// text + see "audio backend not configured" from the UI).
    #[must_use]
    pub fn build_backends(&self, shared_ctx: SharedCtx) -> AudioBackends {
        AudioBackends::new(
            self.build_stt(Arc::clone(&shared_ctx)),
            self.build_tts(shared_ctx),
        )
    }

    fn build_stt(&self, ctx: SharedCtx) -> Box<dyn SttProvider> {
        match self.stt_backend {
            #[cfg(feature = "local-audio")]
            AudioBackendName::Local => crate::local_backend::local_stt(self.clone()),
            #[cfg(not(feature = "local-audio"))]
            AudioBackendName::Local => local_stt_stub(),
            AudioBackendName::Platform => platform_stt_stub(),
            AudioBackendName::Provider => Box::new(ProviderRoutedStt::new(self.clone(), ctx)),
        }
    }

    fn build_tts(&self, ctx: SharedCtx) -> Box<dyn TtsProvider> {
        match self.tts_backend {
            #[cfg(feature = "local-audio")]
            AudioBackendName::Local => crate::local_backend::local_tts(self.clone()),
            #[cfg(not(feature = "local-audio"))]
            AudioBackendName::Local => local_tts_stub(),
            AudioBackendName::Platform => platform_tts_stub(),
            AudioBackendName::Provider => Box::new(ProviderRoutedTts::new(self.clone(), ctx)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_defaults_when_no_config_file() {
        let dir = tempdir().unwrap();
        let cfg = AudioConfig::load(dir.path()).unwrap();
        assert_eq!(cfg.stt_backend, AudioBackendName::Platform);
        assert_eq!(cfg.tts_backend, AudioBackendName::Platform);
        assert_eq!(cfg.local_model_size, "base.en");
    }

    #[test]
    fn load_parses_audio_block() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(
            dir.path().join(".forge/config.toml"),
            "[audio]\nstt_backend = \"provider\"\ntts_backend = \"platform\"\nlocal_model_size = \"tiny.en\"\n",
        )
        .unwrap();
        let cfg = AudioConfig::load(dir.path()).unwrap();
        assert_eq!(cfg.stt_backend, AudioBackendName::Provider);
        assert_eq!(cfg.tts_backend, AudioBackendName::Platform);
        assert_eq!(cfg.local_model_size, "tiny.en");
    }

    #[test]
    fn load_rejects_unknown_backend() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(
            dir.path().join(".forge/config.toml"),
            "[audio]\nstt_backend = \"bogus\"\n",
        )
        .unwrap();
        let err = AudioConfig::load(dir.path()).unwrap_err();
        assert!(matches!(err, AudioError::InvalidConfig(_)));
    }

    #[test]
    fn env_overrides_provider_api_key() {
        // SAFETY: tests touch process env; serial guard not strictly
        // needed because cargo runs different tests in different
        // processes by default, but use a uniquely-named key just
        // in case a future test threads through the same env name.
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test-117") };
        let dir = tempdir().unwrap();
        let cfg = AudioConfig::load(dir.path()).unwrap();
        assert_eq!(cfg.provider_api_key.as_deref(), Some("sk-test-117"));
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }
}
