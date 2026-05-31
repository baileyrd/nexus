//! Nexus audio subsystem (BL-117) — STT + TTS provider traits with
//! three pluggable backends:
//!
//! - **`local`** — on-device Whisper / Piper. Feature-gated behind
//!   `local-whisper`; default build ships a stub that returns
//!   [`AudioError::BackendNotEnabled`] so a fresh forge has a clear
//!   error message rather than a silent dependency on an operator
//!   step.
//! - **`provider`** — routes to whatever AI provider `nexus-ai` is
//!   configured for. Today that's OpenAI's Whisper (STT) + TTS APIs;
//!   when Anthropic ships audio the existing keyring entry just
//!   works.
//! - **`platform`** — shell-side Web Speech API (BL-118). The
//!   shell plugin registers itself via the kernel; the core crate
//!   stubs it out for runtimes without a webview.
//!
//! See `docs/PRDs/BACKLOG.md` (BL-117) for the full spec.
//!
//! # IPC surface
//!
//! | Handler | Args                                | Returns                                  |
//! |---------|-------------------------------------|------------------------------------------|
//! | `transcribe` | `{ audio_b64, format?, language? }` | `{ text, language? }`            |
//! | `synthesize` | `{ text, voice?, format? }`         | `{ audio_b64, format }`          |
//!
//! Audio blobs travel over JSON as base64. Wav / webm / opus / mp3 are
//! the format strings the OpenAI side accepts; for `synthesize` the
//! caller asks for one of those and the provider returns whatever it
//! actually emits (echoed in the reply).
//!
//! # Capability gates
//!
//! - `audio.record` — required to invoke `transcribe`.
//! - `audio.synthesize` — required to invoke `synthesize`.
//! - The `provider` backend additionally requires `network` for
//!   outbound HTTPS to the provider endpoint.
//!
//! Capability checks are enforced by the kernel before dispatch lands
//! here — we surface them through the [`AudioError::Backend`] variant
//! when a backend reports its own auth failure.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
// Provider names (OpenAI, WebM, MediaRecorder, JSDoc, …) appear in
// many doc comments without backticks because they're proper nouns,
// not code identifiers. Matches the rest of the workspace.
#![allow(clippy::doc_markdown)]

pub mod backend;
pub mod config;
pub mod core_plugin;
mod error;
pub mod ipc;
/// BL-117 local backends. Compiled in only with the `local-audio`
/// feature; default builds fall through to the
/// [`stub_backend`] family which returns
/// [`AudioError::BackendNotEnabled`] from the first dispatch.
#[cfg(feature = "local-audio")]
mod local_backend;
mod provider_backend;
mod stub_backend;

pub use backend::{
    AudioBackends, AudioFormat, SttProvider, SynthesisOutput, TranscriptionInput,
    TranscriptionOutput, TtsProvider,
};
pub use config::{AudioBackendName, AudioConfig};
pub use core_plugin::{AudioCorePlugin, PLUGIN_ID};
pub use error::AudioError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_backend_name_round_trips() {
        assert_eq!(
            "local".parse::<AudioBackendName>().unwrap(),
            AudioBackendName::Local
        );
        assert_eq!(
            "provider".parse::<AudioBackendName>().unwrap(),
            AudioBackendName::Provider
        );
        assert_eq!(
            "platform".parse::<AudioBackendName>().unwrap(),
            AudioBackendName::Platform
        );
        assert!("bogus".parse::<AudioBackendName>().is_err());
    }
}
