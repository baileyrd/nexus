//! Error type for `nexus-audio`.

use thiserror::Error;

/// All public failure modes from the audio subsystem.
#[derive(Debug, Error)]
pub enum AudioError {
    /// The selected backend exists but is gated behind a cargo feature
    /// or runtime contribution that hasn't been enabled. Default
    /// builds report this for `local` (rebuild with
    /// `--features local-whisper`) and for `platform` (BL-118 shell
    /// plugin not loaded). The message names the backend so logs are
    /// triagable from a single line.
    #[error("audio backend '{backend}' is not enabled in this build: {reason}")]
    BackendNotEnabled {
        /// Backend identifier (`local` / `platform`).
        backend: String,
        /// Human-readable hint for how to enable it.
        reason: String,
    },

    /// The backend ran but reported a failure of its own (provider
    /// HTTP error, model load failed, audio decode failed, etc.).
    /// Backend-specific surface so the cause is preserved without
    /// proliferating variants per provider.
    #[error("audio backend '{backend}' failed: {reason}")]
    Backend {
        /// Backend identifier.
        backend: String,
        /// Underlying error string.
        reason: String,
    },

    /// Caller passed bytes that aren't valid for the named codec, or
    /// asked for a format the backend doesn't support.
    #[error("invalid audio payload: {0}")]
    InvalidAudio(String),

    /// Config parse error (bad TOML, unknown backend identifier).
    #[error("invalid audio config: {0}")]
    InvalidConfig(String),

    /// Configuration is missing for the selected backend (no API key
    /// for `provider`, no model file for `local`, …).
    #[error("audio backend '{backend}' is missing required configuration: {reason}")]
    Misconfigured {
        /// Backend identifier.
        backend: String,
        /// Human-readable hint for what's missing.
        reason: String,
    },

    /// Outbound network call to a provider endpoint failed before the
    /// backend could read the response.
    #[error("audio network error: {0}")]
    Network(#[from] reqwest::Error),

    /// I/O failure (model file read, temp file write, …).
    #[error("audio io error: {0}")]
    Io(#[from] std::io::Error),

    /// base64 decode failure on an incoming audio blob.
    #[error("invalid base64 audio payload: {0}")]
    Base64(#[from] base64::DecodeError),
}
