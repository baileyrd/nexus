//! On-device Whisper STT + OS-TTS synthesis (BL-117). Compiled in
//! only with the `local-audio` cargo feature.
//!
//! ## STT
//!
//! Wraps `whisper-rs` against a GGML model file kept under
//! `<forge>/.forge/.audio/models/ggml-<size>.bin`. The model is
//! downloaded from the ggerganov HuggingFace repo on first use; the
//! download is gated by the storage subsystem's network capability
//! (audio plugin already declares `NetHttp` in
//! `audio_capabilities`). The default size is `base.en` (~140 MB)
//! per BL-117; the user can pick `tiny.en` (~75 MB) or `small.en`
//! (~466 MB) via `[audio] local_model_size = "tiny.en" | "base.en"
//! | "small.en"` in `config.toml`.
//!
//! Whisper expects 16 kHz mono f32 samples. The wire-level
//! `transcribe` payload only accepts WAV here — WebM / Opus / MP3
//! decode requires an audio framework we don't ship. Use the
//! `provider` backend for non-WAV input. A WAV at a different rate
//! or bit depth than expected gets a clear
//! [`AudioError::InvalidAudio`] error, not silent garbage.
//!
//! ## TTS
//!
//! Cross-platform shell-out:
//!
//! - Linux: `espeak-ng -w <out.wav> -s 160 -- <text>`
//! - macOS: `say -o <out.aiff>` (returned as AIFF wrapped in WAV
//!   via hound — TODO: bare AIFF passthrough)
//! - Windows: PowerShell SAPI script
//!
//! Each path returns WAV bytes. If the platform binary isn't on
//! PATH the backend reports [`AudioError::BackendNotEnabled`] with
//! a hint at install commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::backend::{
    AudioFormat, SttProvider, SynthesisOutput, TranscriptionInput, TranscriptionOutput,
    TtsProvider,
};
use crate::config::AudioConfig;
use crate::AudioError;

const STT_NAME: &str = "local";
const TTS_NAME: &str = "local";

/// Build the local Whisper STT backend.
#[must_use]
pub fn local_stt(cfg: AudioConfig) -> Box<dyn SttProvider> {
    Box::new(LocalWhisperStt {
        cfg,
        ctx: None,
        loaded_size: None,
    })
}

/// Build the local OS-TTS backend.
#[must_use]
pub fn local_tts(cfg: AudioConfig) -> Box<dyn TtsProvider> {
    Box::new(LocalOsTts { cfg })
}

// ─── STT ──────────────────────────────────────────────────────────────────────

struct LocalWhisperStt {
    cfg: AudioConfig,
    /// Lazy-loaded context. Whisper model load is ~1 s for `base.en`
    /// so we cache across calls; the size is remembered so a
    /// runtime config change (e.g. operator-edited TOML) reloads on
    /// next dispatch.
    ctx: Option<WhisperContext>,
    loaded_size: Option<String>,
}

impl LocalWhisperStt {
    fn model_dir(&self) -> PathBuf {
        // The audio plugin's runtime forge_root isn't carried in
        // AudioConfig — we store the model relative to the current
        // working directory's `.forge` so the test + the CLI both
        // resolve the same path. AudioCorePlugin's on_init writes
        // the model dir into AudioConfig::local_model_dir at boot
        // (TODO: thread forge_root through cleanly; for now we
        // accept that local-audio runs require the kernel to chdir
        // into the forge root first).
        PathBuf::from(".forge/.audio/models")
    }

    fn model_filename(size: &str) -> String {
        format!("ggml-{size}.bin")
    }

    fn ensure_model(&self) -> Result<PathBuf, AudioError> {
        let size = self.cfg.local_model_size.as_str();
        let path = self.model_dir().join(Self::model_filename(size));
        if path.exists() {
            return Ok(path);
        }
        let parent = self.model_dir();
        std::fs::create_dir_all(&parent).map_err(AudioError::Io)?;
        let url = format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin"
        );
        tracing::info!(
            %url,
            target = %path.display(),
            "BL-117 local-audio: downloading Whisper model (first launch)"
        );
        let bytes = reqwest::blocking::get(&url)
            .map_err(AudioError::Network)?
            .error_for_status()
            .map_err(AudioError::Network)?
            .bytes()
            .map_err(AudioError::Network)?;
        std::fs::write(&path, &bytes).map_err(AudioError::Io)?;
        Ok(path)
    }

    fn load_ctx(&mut self) -> Result<&mut WhisperContext, AudioError> {
        let needs_reload = match (&self.ctx, &self.loaded_size) {
            (Some(_), Some(size)) if size == &self.cfg.local_model_size => false,
            _ => true,
        };
        if needs_reload {
            let path = self.ensure_model()?;
            let params = WhisperContextParameters::default();
            let ctx = WhisperContext::new_with_params(
                path.to_str().ok_or_else(|| AudioError::Backend {
                    backend: STT_NAME.to_string(),
                    reason: format!("model path is not valid UTF-8: {}", path.display()),
                })?,
                params,
            )
            .map_err(|e| AudioError::Backend {
                backend: STT_NAME.to_string(),
                reason: format!("whisper load: {e}"),
            })?;
            self.ctx = Some(ctx);
            self.loaded_size = Some(self.cfg.local_model_size.clone());
        }
        Ok(self.ctx.as_mut().expect("ctx populated above"))
    }
}

impl SttProvider for LocalWhisperStt {
    fn name(&self) -> &'static str {
        STT_NAME
    }

    fn transcribe(
        &mut self,
        input: TranscriptionInput,
    ) -> Result<TranscriptionOutput, AudioError> {
        if input.format != AudioFormat::Wav {
            return Err(AudioError::InvalidAudio(format!(
                "local Whisper backend only accepts WAV input; got {}. \
                 Re-record as 16 kHz mono WAV or use the `provider` backend.",
                input.format.as_str()
            )));
        }
        let samples = decode_wav_to_mono16k(&input.bytes)?;
        let language = input.language.clone();
        let ctx = self.load_ctx()?;
        let mut state = ctx.create_state().map_err(|e| AudioError::Backend {
            backend: STT_NAME.to_string(),
            reason: format!("whisper state: {e}"),
        })?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(lang) = language.as_deref() {
            params.set_language(Some(lang));
        }
        state.full(params, &samples).map_err(|e| AudioError::Backend {
            backend: STT_NAME.to_string(),
            reason: format!("whisper inference: {e}"),
        })?;
        let n = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n {
            let Some(seg) = state.get_segment(i) else {
                continue;
            };
            let seg_text = seg.to_str().map_err(|e| AudioError::Backend {
                backend: STT_NAME.to_string(),
                reason: format!("whisper segment {i} text: {e}"),
            })?;
            text.push_str(seg_text);
        }
        Ok(TranscriptionOutput {
            text: text.trim().to_string(),
            language,
        })
    }
}

/// Decode WAV bytes to 16 kHz mono f32 samples (Whisper's expected
/// input shape). Stereo collapses to mono via simple average; sample
/// rates other than 16 kHz are rejected because we don't ship a
/// resampler. Tell the user to re-record rather than silently giving
/// them garbage transcripts.
fn decode_wav_to_mono16k(bytes: &[u8]) -> Result<Vec<f32>, AudioError> {
    let mut reader =
        hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|e| {
            AudioError::InvalidAudio(format!("wav decode header: {e}"))
        })?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(AudioError::InvalidAudio(format!(
            "local Whisper expects 16 kHz audio; got {} Hz. Re-record at 16 kHz mono.",
            spec.sample_rate
        )));
    }
    let channels = spec.channels;
    if channels == 0 {
        return Err(AudioError::InvalidAudio("wav has zero channels".to_string()));
    }
    // Convert to f32 in [-1, 1].
    let mut samples_f32: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()
                .map_err(|e| AudioError::InvalidAudio(format!("wav decode: {e}")))?
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| AudioError::InvalidAudio(format!("wav decode: {e}")))?,
    };
    if channels > 1 {
        // Average channels into mono.
        let c = channels as usize;
        samples_f32 = samples_f32
            .chunks_exact(c)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect();
    }
    Ok(samples_f32)
}

// ─── TTS ──────────────────────────────────────────────────────────────────────

struct LocalOsTts {
    cfg: AudioConfig,
}

impl TtsProvider for LocalOsTts {
    fn name(&self) -> &'static str {
        TTS_NAME
    }

    fn synthesize(
        &mut self,
        text: &str,
        _voice: Option<&str>,
        _format: AudioFormat,
    ) -> Result<SynthesisOutput, AudioError> {
        let _ = &self.cfg; // reserved for future per-voice config
        let tmp = tempfile::Builder::new()
            .prefix("nexus-tts-")
            .suffix(".wav")
            .tempfile()
            .map_err(AudioError::Io)?;
        let out_path = tmp.path().to_path_buf();
        run_platform_tts(text, &out_path)?;
        let bytes = std::fs::read(&out_path).map_err(AudioError::Io)?;
        // tempfile cleans up on drop, but we've already read the
        // bytes so the file can go.
        drop(tmp);
        Ok(SynthesisOutput {
            bytes,
            format: AudioFormat::Wav,
        })
    }
}

#[cfg(target_os = "linux")]
fn run_platform_tts(text: &str, out: &Path) -> Result<(), AudioError> {
    let status = Command::new("espeak-ng")
        .args(["-w"])
        .arg(out)
        .args(["-s", "160", "--"])
        .arg(text)
        .status()
        .map_err(|e| AudioError::BackendNotEnabled {
            backend: TTS_NAME.to_string(),
            reason: format!(
                "espeak-ng not found on PATH ({e}). Install with `apt install espeak-ng` \
                 / `dnf install espeak-ng` / `pacman -S espeak-ng`."
            ),
        })?;
    if !status.success() {
        return Err(AudioError::Backend {
            backend: TTS_NAME.to_string(),
            reason: format!("espeak-ng exited with {status}"),
        });
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn run_platform_tts(text: &str, out: &Path) -> Result<(), AudioError> {
    let status = Command::new("say")
        .args(["-o"])
        .arg(out)
        .args(["--data-format=LEF32@22050"])
        .args(["--"])
        .arg(text)
        .status()
        .map_err(|e| AudioError::BackendNotEnabled {
            backend: TTS_NAME.to_string(),
            reason: format!("`say` not found on PATH ({e})"),
        })?;
    if !status.success() {
        return Err(AudioError::Backend {
            backend: TTS_NAME.to_string(),
            reason: format!("say exited with {status}"),
        });
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_platform_tts(text: &str, out: &Path) -> Result<(), AudioError> {
    let escaped = text.replace('"', "`\"");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.SetOutputToWaveFile(\"{}\"); \
         $s.Speak(\"{}\");",
        out.display(),
        escaped
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .map_err(|e| AudioError::BackendNotEnabled {
            backend: TTS_NAME.to_string(),
            reason: format!("powershell not found on PATH ({e})"),
        })?;
    if !status.success() {
        return Err(AudioError::Backend {
            backend: TTS_NAME.to_string(),
            reason: format!("powershell exited with {status}"),
        });
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn run_platform_tts(_text: &str, _out: &Path) -> Result<(), AudioError> {
    Err(AudioError::BackendNotEnabled {
        backend: TTS_NAME.to_string(),
        reason: "local TTS shell-out not implemented for this platform".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_wav_input() {
        let mut stt = LocalWhisperStt {
            cfg: AudioConfig::default(),
            ctx: None,
            loaded_size: None,
        };
        let err = stt
            .transcribe(TranscriptionInput {
                bytes: vec![1, 2, 3],
                format: AudioFormat::Webm,
                language: None,
            })
            .unwrap_err();
        match err {
            AudioError::InvalidAudio(msg) => assert!(msg.contains("WAV")),
            other => panic!("expected InvalidAudio, got {other:?}"),
        }
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        // Build a tiny 48 kHz WAV header and feed it in.
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut buf = Vec::new();
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut buf), spec).unwrap();
            for _ in 0..100 {
                w.write_sample(0_i16).unwrap();
            }
            w.finalize().unwrap();
        }
        let err = decode_wav_to_mono16k(&buf).unwrap_err();
        match err {
            AudioError::InvalidAudio(msg) => assert!(msg.contains("16 kHz")),
            other => panic!("expected InvalidAudio, got {other:?}"),
        }
    }

    #[test]
    fn decodes_16khz_mono_wav() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut buf = Vec::new();
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut buf), spec).unwrap();
            for i in 0..1600 {
                let v = ((i as f32) / 100.0).sin();
                w.write_sample((v * i16::MAX as f32) as i16).unwrap();
            }
            w.finalize().unwrap();
        }
        let samples = decode_wav_to_mono16k(&buf).unwrap();
        assert_eq!(samples.len(), 1600);
    }

    #[test]
    fn collapses_stereo_to_mono() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut buf = Vec::new();
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut buf), spec).unwrap();
            for _ in 0..800 {
                w.write_sample(1000_i16).unwrap(); // L
                w.write_sample(-500_i16).unwrap(); // R → averages to 250
            }
            w.finalize().unwrap();
        }
        let samples = decode_wav_to_mono16k(&buf).unwrap();
        assert_eq!(samples.len(), 800);
        // Both channels are constant, so the average is constant.
        // 1000 + (-500) = 500, /2 = 250; /max ~= 250 / 32768 ~ 0.00763
        let expected = (1000.0_f32 + -500.0_f32) / 2.0 / 32768.0;
        assert!((samples[0] - expected).abs() < 1e-4);
    }
}
