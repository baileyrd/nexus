//! Backend traits + the registry that owns the three concrete
//! implementations. Each [`AudioCorePlugin`](crate::AudioCorePlugin)
//! instance carries one [`AudioBackends`] containing the selected
//! STT / TTS pair.

use crate::AudioError;

/// Audio container format used by the wire-level `transcribe` /
/// `synthesize` IPC handlers. The string label is what travels in
/// JSON; the enum is just for typed dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// Waveform Audio File Format. 16-bit PCM, RIFF container.
    Wav,
    /// WebM container (typically Opus inside). Default for browser
    /// `MediaRecorder` captures.
    Webm,
    /// Bare Opus.
    Opus,
    /// MP3.
    Mp3,
}

impl AudioFormat {
    /// Stable on-wire string label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Webm => "webm",
            Self::Opus => "opus",
            Self::Mp3 => "mp3",
        }
    }

    /// File extension for temp blobs handed to a multipart upload.
    /// OpenAI's API keys off the *filename's* extension to detect
    /// container, so this matches [`Self::as_str`].
    #[must_use]
    pub fn extension(self) -> &'static str {
        self.as_str()
    }

    /// Parse a wire-level string label into a typed format. Falls
    /// back to [`AudioFormat::Webm`] for empty input — that's what
    /// the BL-043 quick-capture shell flow records.
    #[must_use]
    pub fn parse_or_default(label: Option<&str>) -> Self {
        let s = label.unwrap_or("").to_ascii_lowercase();
        match s.as_str() {
            "wav" => Self::Wav,
            "opus" => Self::Opus,
            "mp3" => Self::Mp3,
            _ => Self::Webm,
        }
    }
}

/// Payload passed into [`SttProvider::transcribe`].
#[derive(Debug, Clone)]
pub struct TranscriptionInput {
    /// Raw audio bytes (already base64-decoded at the IPC boundary).
    pub bytes: Vec<u8>,
    /// Container format of the supplied bytes.
    pub format: AudioFormat,
    /// Optional BCP-47 language hint (e.g. `"en"`). When `None` the
    /// backend auto-detects.
    pub language: Option<String>,
}

/// Result from [`SttProvider::transcribe`].
#[derive(Debug, Clone)]
pub struct TranscriptionOutput {
    /// The recognised text. Empty when nothing was recognised — that
    /// is not an error.
    pub text: String,
    /// Detected language code, when the backend exposes it.
    pub language: Option<String>,
}

/// Result from [`TtsProvider::synthesize`].
#[derive(Debug, Clone)]
pub struct SynthesisOutput {
    /// Raw audio bytes (caller re-encodes to base64 at the IPC
    /// boundary).
    pub bytes: Vec<u8>,
    /// Container format of the returned bytes. Backends are free to
    /// return a different format than the request — the field
    /// reflects what's actually in `bytes`.
    pub format: AudioFormat,
}

/// Speech-to-text trait. Backends implementing this take raw audio
/// bytes and return text. `&mut self` because some backends (notably
/// the future Whisper one) hold a stateful model context across
/// calls; provider-routed backends ignore the mutability.
pub trait SttProvider: Send {
    /// Stable identifier for logs / metrics / errors.
    fn name(&self) -> &'static str;

    /// Synchronous transcribe. The wire handler dispatches this from
    /// the kernel's worker pool — callers must not assume a tokio
    /// runtime, but a tokio-based provider backend can spin up its
    /// own runtime via `tokio::runtime::Handle::try_current` /
    /// `Runtime::new`.
    ///
    /// # Errors
    /// Implementations return [`AudioError`] for any failure mode.
    fn transcribe(&mut self, input: TranscriptionInput)
        -> Result<TranscriptionOutput, AudioError>;
}

/// Text-to-speech trait. Mirrors [`SttProvider`] in shape.
pub trait TtsProvider: Send {
    /// Stable identifier for logs / metrics / errors.
    fn name(&self) -> &'static str;

    /// Synchronous synthesize. `voice` is provider-specific (OpenAI's
    /// `alloy`/`echo`/…); the backend picks a sensible default when
    /// `None`. `format` is the requested container; the backend may
    /// return a different one (reflected in the output).
    ///
    /// # Errors
    /// Implementations return [`AudioError`] for any failure mode.
    fn synthesize(
        &mut self,
        text: &str,
        voice: Option<&str>,
        format: AudioFormat,
    ) -> Result<SynthesisOutput, AudioError>;
}

/// Pair of backends owned by an [`AudioCorePlugin`](crate::AudioCorePlugin).
/// Constructed from [`crate::AudioConfig`] at boot via
/// [`AudioCorePlugin::with_config`](crate::AudioCorePlugin::with_config);
/// tests can inject mocks via [`AudioBackends::new`].
pub struct AudioBackends {
    stt: Box<dyn SttProvider>,
    tts: Box<dyn TtsProvider>,
}

impl AudioBackends {
    /// Construct directly from two backends. Used by tests + by the
    /// shell-side BL-118 contribution path; production wires through
    /// [`crate::config::AudioConfig::build_backends`].
    #[must_use]
    pub fn new(stt: Box<dyn SttProvider>, tts: Box<dyn TtsProvider>) -> Self {
        Self { stt, tts }
    }

    /// Mutable accessor for the STT provider.
    pub fn stt_mut(&mut self) -> &mut dyn SttProvider {
        self.stt.as_mut()
    }

    /// Mutable accessor for the TTS provider.
    pub fn tts_mut(&mut self) -> &mut dyn TtsProvider {
        self.tts.as_mut()
    }

    /// Read-only view of the active backend pair, for IPC reflection
    /// (`com.nexus.audio::status` once that's added).
    #[must_use]
    pub fn names(&self) -> (&'static str, &'static str) {
        (self.stt.name(), self.tts.name())
    }
}
