//! Wire-level types for the `com.nexus.audio` IPC surface.
//!
//! Audio blobs travel as base64 strings (`audio_b64`) because the
//! kernel's IPC layer carries JSON only. The format label is a
//! lowercase string (`"wav"` / `"webm"` / `"opus"` / `"mp3"`); see
//! [`crate::AudioFormat`] for the typed enum.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Args for `com.nexus.audio::transcribe` (handler id `1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AudioTranscribeArgs {
    /// Base64-encoded raw audio. The backend decodes this once at
    /// the IPC boundary.
    pub audio_b64: String,
    /// Container format label (`"wav"` / `"webm"` / `"opus"` /
    /// `"mp3"`). Empty / absent defaults to `"webm"` to match the
    /// shell's default capture from `MediaRecorder`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Optional BCP-47 language hint (e.g. `"en"`). When `None` the
    /// backend auto-detects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// Reply from `com.nexus.audio::transcribe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AudioTranscribeResult {
    /// Recognised text. Empty when nothing was recognised — that is
    /// not an error.
    pub text: String,
    /// Detected language code, when the backend reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Backend identifier (`"local"` / `"provider"` / `"platform"`)
    /// that handled this call. Useful for audit-log attribution.
    pub backend: String,
}

/// Args for `com.nexus.audio::synthesize` (handler id `2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AudioSynthesizeArgs {
    /// Text to speak. UTF-8; the provider may impose its own length
    /// cap (OpenAI: 4096 chars).
    pub text: String,
    /// Voice identifier. Provider-specific (OpenAI: `alloy` /
    /// `echo` / `fable` / `onyx` / `nova` / `shimmer`). `None`
    /// falls back to the configured default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Requested container format. The reply echoes the actual
    /// format in `format` since the backend may downgrade (e.g.
    /// OpenAI doesn't ship WebM — WebM requests come back as MP3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// Reply from `com.nexus.audio::synthesize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AudioSynthesizeResult {
    /// Base64-encoded synthesized audio. The shell decodes once for
    /// `<audio>`-element playback.
    pub audio_b64: String,
    /// Container format of the returned bytes (one of `"wav"` /
    /// `"mp3"` / `"opus"`).
    pub format: String,
    /// Backend identifier that handled this call.
    pub backend: String,
}

/// Reply from `com.nexus.audio::status` (handler id `3`). Read-only
/// reflection of the active backend pair — surfaces the bare names so
/// a future settings UI can render "STT: provider · TTS: platform"
/// without knowing the backend taxonomy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AudioStatusResult {
    /// Backend handling `transcribe`.
    pub stt_backend: String,
    /// Backend handling `synthesize`.
    pub tts_backend: String,
}
