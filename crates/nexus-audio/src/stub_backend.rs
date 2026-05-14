//! Stub backends for `local` (default builds without the
//! `local-whisper` feature) and `platform` (until a shell plugin
//! registers its concrete impl via BL-118).
//!
//! Both report [`AudioError::BackendNotEnabled`] on dispatch so an
//! operator who selects them in `config.toml` without the supporting
//! infrastructure sees a clear, actionable error instead of a panic
//! or silent fallback.

use crate::backend::{
    AudioFormat, SttProvider, SynthesisOutput, TranscriptionInput, TranscriptionOutput,
    TtsProvider,
};
use crate::AudioError;

const LOCAL_STT_NAME: &str = "local";
const LOCAL_TTS_NAME: &str = "local";
const PLATFORM_STT_NAME: &str = "platform";
const PLATFORM_TTS_NAME: &str = "platform";

const LOCAL_REASON: &str =
    "build nexus-audio with `--features local-whisper` to enable on-device Whisper / Piper";
const PLATFORM_REASON: &str =
    "platform backend requires the BL-118 shell plugin to register against the kernel";

struct DisabledStt {
    name: &'static str,
    reason: &'static str,
}
struct DisabledTts {
    name: &'static str,
    reason: &'static str,
}

impl SttProvider for DisabledStt {
    fn name(&self) -> &'static str {
        self.name
    }
    fn transcribe(&mut self, _input: TranscriptionInput) -> Result<TranscriptionOutput, AudioError> {
        Err(AudioError::BackendNotEnabled {
            backend: self.name.to_string(),
            reason: self.reason.to_string(),
        })
    }
}

impl TtsProvider for DisabledTts {
    fn name(&self) -> &'static str {
        self.name
    }
    fn synthesize(
        &mut self,
        _text: &str,
        _voice: Option<&str>,
        _format: AudioFormat,
    ) -> Result<SynthesisOutput, AudioError> {
        Err(AudioError::BackendNotEnabled {
            backend: self.name.to_string(),
            reason: self.reason.to_string(),
        })
    }
}

/// Local-Whisper STT stub. Real implementation lands when the
/// `local-whisper` feature ships.
#[must_use]
pub fn local_stt_stub() -> Box<dyn SttProvider> {
    Box::new(DisabledStt {
        name: LOCAL_STT_NAME,
        reason: LOCAL_REASON,
    })
}

/// Local-TTS (Piper) stub.
#[must_use]
pub fn local_tts_stub() -> Box<dyn TtsProvider> {
    Box::new(DisabledTts {
        name: LOCAL_TTS_NAME,
        reason: LOCAL_REASON,
    })
}

/// Platform STT stub. The BL-118 shell plugin replaces this through
/// the kernel's contribution path.
#[must_use]
pub fn platform_stt_stub() -> Box<dyn SttProvider> {
    Box::new(DisabledStt {
        name: PLATFORM_STT_NAME,
        reason: PLATFORM_REASON,
    })
}

/// Platform TTS stub.
#[must_use]
pub fn platform_tts_stub() -> Box<dyn TtsProvider> {
    Box::new(DisabledTts {
        name: PLATFORM_TTS_NAME,
        reason: PLATFORM_REASON,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_stt_stub_returns_backend_not_enabled() {
        let mut p = local_stt_stub();
        let err = p
            .transcribe(TranscriptionInput {
                bytes: vec![],
                format: AudioFormat::Webm,
                language: None,
            })
            .unwrap_err();
        match err {
            AudioError::BackendNotEnabled { backend, reason } => {
                assert_eq!(backend, "local");
                assert!(reason.contains("local-whisper"));
            }
            other => panic!("expected BackendNotEnabled, got {other:?}"),
        }
    }

    #[test]
    fn platform_tts_stub_names_bl_118() {
        let mut p = platform_tts_stub();
        let err = p.synthesize("hi", None, AudioFormat::Wav).unwrap_err();
        match err {
            AudioError::BackendNotEnabled { backend, reason } => {
                assert_eq!(backend, "platform");
                assert!(reason.contains("BL-118"));
            }
            other => panic!("expected BackendNotEnabled, got {other:?}"),
        }
    }
}
