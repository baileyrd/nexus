# nexus-audio

> Kind: lib · IPC plugin id: com.nexus.audio · CorePlugin: yes · Has settings: yes (`[audio]` in `config.toml`) · As of: 2026-05-25

## Overview

`nexus-audio` is the BL-117 audio subsystem: it gives Nexus a speech-to-text (STT) and text-to-speech (TTS) capability behind two small provider traits and a `com.nexus.audio` core plugin with three IPC handlers (`transcribe`, `synthesize`, `status`). All audio blobs cross the kernel's JSON-only IPC boundary as base64 strings; the crate decodes incoming bytes and re-encodes outgoing bytes at that boundary.

The design centres on a `(SttProvider, TtsProvider)` pair selected by config. Three backend families exist: **`local`** (on-device Whisper STT via whisper-rs + OS-shell-out TTS, feature-gated behind `local-audio`), **`provider`** (direct HTTPS to OpenAI's `/v1/audio/transcriptions` and `/v1/audio/speech`), and **`platform`** (the shell-side Web Speech API contributed at runtime by the BL-118 `nexus.audio` shell plugin). The default build ships only stubs for `local` and `platform`; selecting an unsupported backend yields a clear `AudioError::BackendNotEnabled` on first dispatch rather than failing at boot, so a fresh forge always boots.

The crate fits the microkernel model by being a subsystem plugin: it depends on `nexus-kernel` and `nexus-plugins` (and `nexus-security` for the shared TLS-pinning HTTP client), never the reverse. The provider backend reaches credentials by issuing a `com.nexus.ai::resolve_credentials` IPC call through the wired kernel context, falling back to `OPENAI_API_KEY` env / `[audio]` TOML — so a forge already configured for chat works for audio with no extra setup.

Capability-wise, the plugin's own context is granted only `IpcCall` (to reach `com.nexus.ai`) and `NetHttp` (outbound HTTPS). The caller-facing gates `audio.record` and `audio.synthesize` are enforced by the kernel on the handler manifest before dispatch lands here.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-kernel` (KernelPluginContext, `Ipc` trait, event bus types), `nexus-plugins` (`CorePlugin`, `PluginError`, `define_dispatch_helpers!`), `nexus-security` (BL-102 `tls::build_pinned_client`).
- **Notable external deps:** `reqwest` (features `multipart` + `blocking`) for the provider backend's HTTP calls; `base64` for the IPC-boundary codec; `tokio` for the async-on-sync runtime bridge; `serde`/`serde_json`/`toml`/`thiserror`/`tracing`. Feature-gated: `whisper-rs` 0.16 (whisper.cpp bindings), `hound` 3.5 (WAV decode/encode), `tempfile` — all three pulled in only by the `local-audio` feature. `ts-rs` + `schemars` are gated behind `ts-export` for binding generation.
- **Crates depending on it:** `nexus-bootstrap` (registers the plugin in `src/plugins/audio.rs`, grants caps in `audio_capabilities()`). No other crate links it directly; CLI/TUI/MCP/shell reach it through `ipc_call`.

## Public API surface

**`lib.rs`** — re-exports the public surface; `#![deny(missing_docs)]`. Declares modules and gates `local_backend` behind `feature = "local-audio"`.

**`backend.rs`** — trait + type core:
- `AudioFormat` (enum: `Wav`/`Webm`/`Opus`/`Mp3`) — wire-level container labels; `as_str`, `extension` (alias of `as_str`, since OpenAI keys off the upload filename), `parse_or_default` (empty/absent → `Webm`).
- `TranscriptionInput { bytes, format, language }` — STT input (bytes already base64-decoded).
- `TranscriptionOutput { text, language }` — STT result (empty text is not an error).
- `SynthesisOutput { bytes, format }` — TTS result; format reflects what is actually in `bytes`.
- `SttProvider` trait — `name()`, `transcribe(&mut self, TranscriptionInput) -> Result<TranscriptionOutput, AudioError>` (synchronous; `&mut self` for stateful model contexts).
- `TtsProvider` trait — `name()`, `synthesize(&mut self, text, voice, format) -> Result<SynthesisOutput, AudioError>`.
- `AudioBackends` — owns the boxed `(stt, tts)` pair; `new`, `stt_mut`, `tts_mut`, `names()`.

**`config.rs`** — `AudioBackendName` (enum `Local`/`Provider`/`Platform`, `FromStr` + `as_str`, lowercase serde); `AudioConfig` (resolved config struct, see Settings); `AudioConfig::load(forge_root)` (reads `[audio]` table, applies env overrides); `AudioConfig::build_backends(SharedCtx) -> AudioBackends` (dispatches on backend name with `#[cfg]` gating for `local`); `DEFAULT_WHISPER_MODEL_URL_TEMPLATE` const.

**`core_plugin.rs`** — `AudioCorePlugin` (`new`, `with_backends`), `PLUGIN_ID = "com.nexus.audio"`, handler-id consts (`HANDLER_TRANSCRIBE=1`, `HANDLER_SYNTHESIZE=2`, `HANDLER_STATUS=3`), `MANIFEST_DEPS = ["com.nexus.ai"]`, `IPC_HANDLERS` slice (SD-06 single source of truth for command→id mapping consumed by bootstrap).

**`ipc.rs`** — wire types (all `#[serde(deny_unknown_fields)]`, optionally `TS`+`JsonSchema` under `ts-export`): `AudioTranscribeArgs`, `AudioTranscribeResult`, `AudioSynthesizeArgs`, `AudioSynthesizeResult`, `AudioStatusResult`.

**`error.rs`** — `AudioError` (see Internals).

**`provider_backend.rs`** (private mod) — `ProviderRoutedStt`/`ProviderRoutedTts`, `SharedCtx` type alias (`Arc<RwLock<Option<Arc<KernelPluginContext>>>>`), `DEFAULT_BASE_URL`, `DEFAULT_CREDS_LOOKUP_TIMEOUT` (2 s), `pub(crate)` codec helpers `encode_b64`/`decode_b64`.

**`stub_backend.rs`** (private mod) — `local_stt_stub`/`local_tts_stub`/`platform_stt_stub`/`platform_tts_stub`, all returning `BackendNotEnabled` on dispatch.

**`local_backend.rs`** (private, `#[cfg(feature = "local-audio")]`) — `local_stt`/`local_tts` factories, `LocalWhisperStt`, `LocalOsTts`, internal `decode_wav_to_mono16k`.

## IPC handlers

| command | args | returns | capability | description |
|---------|------|---------|------------|-------------|
| `transcribe` | `AudioTranscribeArgs { audio_b64, format?, language? }` | `AudioTranscribeResult { text, language?, backend }` | `audio.record` | Base64-decode audio, dispatch to the active STT backend, return recognised text + the backend name that handled it. Privacy-sensitive (captures room audio). |
| `synthesize` | `AudioSynthesizeArgs { text, voice?, format? }` | `AudioSynthesizeResult { audio_b64, format, backend }` | `audio.synthesize` | Synthesize speech via the active TTS backend; reply echoes the *actual* format (backend may downgrade, e.g. WebM→MP3 on OpenAI). |
| `status` | none (`{}`) | `AudioStatusResult { stt_backend, tts_backend }` | — | Read-only reflection of the active backend pair names. |

Notes: `format` defaults to `webm` (matches the shell's `MediaRecorder` capture); each reply includes a `backend` field for audit-log attribution. Capability gates are enforced by the kernel before dispatch; the handler code trusts that. `transcribe` returns an error on invalid base64 (`AudioError::Base64`).

## Capabilities

- **Plugin-context caps (granted in `nexus-bootstrap::audio_capabilities()`):** `IpcCall` (reach `com.nexus.ai::resolve_credentials`), `NetHttp` (outbound HTTPS to the provider audio endpoint). The plugin deliberately does **not** hold `AudioRecord`/`AudioSynthesize`.
- **Caller-facing handler gates (manifest, enforced by kernel):** `audio.record` for `transcribe`, `audio.synthesize` for `synthesize`. `status` is ungated.
- **TLS pinning:** BL-102 — `provider_backend::build_http_client` calls `nexus_security::tls::build_pinned_client(tls_pinning_enabled)`, sharing the same pin policy as `nexus-ai`. Sourced from `AudioConfig::tls_pinning_enabled` (bootstrapped from `KernelConfig::tls_pinning_enabled`); defaults to `false` (empty pin table). The local-audio model download uses a plain `reqwest::blocking::get` and is **not** pin-gated.

## Settings / Config

Loaded from `<forge>/.forge/config.toml` `[audio]` block by `AudioConfig::load`. A missing file or missing block yields defaults; malformed TOML or an unknown backend name yields `AudioError::InvalidConfig`. `OPENAI_API_KEY` / `OPENAI_BASE_URL` env vars override the file values when non-empty.

| Field (TOML key) | Type | Default | Purpose |
|---|---|---|---|
| `stt_backend` | `local`/`provider`/`platform` | `platform` | Backend handling `transcribe`. |
| `tts_backend` | `local`/`provider`/`platform` | `platform` | Backend handling `synthesize`. |
| `local_model_size` | string | `base.en` | Whisper model size (`tiny.en`/`base.en`/`small.en`); local backend only. |
| `local_model_dir` | path | `<forge>/.forge/.audio/models` (relative `.forge/.audio/models` in `Default`) | Dir for `ggml-*.bin` files; anchored to forge root by `load`. |
| `tls_pinning_enabled` | bool | `false` | Pin TLS to provider endpoints (BL-102). |
| `provider_api_key` | string | `None` | OpenAI key for `provider` backend; env-overridable. |
| `provider_base_url` | string | `None` → `https://api.openai.com` | Provider base URL; env-overridable. |
| `provider_stt_model` | string | `whisper-1` | STT model id. |
| `provider_tts_model` | string | `tts-1` | TTS model id (`tts-1`/`tts-1-hd`). |
| `provider_tts_voice` | string | `alloy` | Default TTS voice. |
| `whisper_model_url` (→ `whisper_model_url_template`) | string | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin` | P2-05 download template; must contain `{size}`. |
| `creds_lookup_timeout_secs` (→ `creds_lookup_timeout`) | u64 → Duration | 2 s | P2-06 deadline for the `resolve_credentials` IPC call. |

**Feature flags:** `local-audio` (= `dep:whisper-rs` + `dep:hound` + `dep:tempfile`) enables on-device backends; `local-whisper` is a legacy alias for `local-audio`; `ts-export` emits TS + JSON Schema bindings. Default build keeps all of these off.

## Events

None. The crate neither publishes nor subscribes to kernel event-bus topics — it is purely request/response over IPC. Note that despite the `NetHttp`/`IpcCall` caps, it does not hold `EventsPublish`.

## Internals & notable implementation details

- **Dispatch / ownership:** `AudioCorePlugin` holds `Mutex<Option<AudioBackends>>` (serialises dispatches so backends need not be `Sync`) plus a `SharedCtx` (`Arc<RwLock<Option<Arc<KernelPluginContext>>>>`). `on_init` loads config and builds the pair (skipped if `with_backends` pre-seeded it for tests); `wire_context` populates the shared slot *after* init so the provider backend, constructed during `on_init`, still observes a later-arriving context without rebuilding. `dispatch` routes on handler id; unknown ids → `ExecutionFailed`.
- **Provider backend (`provider_backend.rs`):** Talks to OpenAI directly rather than round-tripping through `nexus-ai` because the audio API uses multipart upload (STT) and a binary response (TTS), shapes the JSON-only IPC surface can't carry cleanly. `resolve_creds` prefers a live `com.nexus.ai::resolve_credentials` reply (non-empty `api_key`), else falls back to config/env, else returns `Misconfigured`. STT POSTs a `multipart::Form` with `model`, `response_format=json`, the audio `file` (filename extension drives container detection), and optional `language`; parses `{text, language?}` loosely. TTS POSTs JSON `{model, voice, input, response_format}` and reads the binary body; format mapping: Wav→wav, Opus→opus, Mp3/Webm→mp3 (WebM unsupported by OpenAI, downgraded and reflected in the reply).
- **Async-on-sync bridge:** `run_async` reuses a current tokio runtime via `Handle::try_current` + `block_in_place` + `block_on`; with no runtime it builds a current-thread runtime and drops it on a fresh OS thread to avoid the `Runtime::drop`-inside-async panic.
- **Base64:** standard (MIME) alphabet via `base64::engine::general_purpose::STANDARD`, shared by `encode_b64`/`decode_b64`.
- **Local Whisper (`local_backend.rs`, feature-gated):** `LocalWhisperStt` lazy-loads + caches a `WhisperContext` keyed on `local_model_size` (reloads on size change). `ensure_model` downloads `ggml-<size>.bin` from the URL template on first use (blocking reqwest). Only **WAV** input is accepted — `decode_wav_to_mono16k` (via hound) requires 16 kHz, rejects other rates (no resampler shipped), averages multi-channel to mono, normalises int/float samples to f32 in [-1,1]. Inference uses greedy sampling with all print flags off; segments concatenated and trimmed.
- **Local TTS (`local_backend.rs`):** OS shell-out to a temp WAV read back as bytes — Linux `espeak-ng -w … -s 160 -- <text>`, macOS `say -o … --data-format=LEF32@22050 -- <text>`, Windows PowerShell SAPI script; missing binary → `BackendNotEnabled` with install hints; other targets → `BackendNotEnabled`. Output is always reported as `AudioFormat::Wav`.
- **`AudioError` variants:** `BackendNotEnabled {backend, reason}`, `Backend {backend, reason}`, `InvalidAudio(String)`, `InvalidConfig(String)`, `Misconfigured {backend, reason}`, `Network(#[from] reqwest::Error)`, `Io(#[from] std::io::Error)`, `Base64(#[from] base64::DecodeError)`. Backends surface their own auth/HTTP failures through `Backend`; disabled/missing deps surface on first dispatch, not at boot.
- **Bootstrap registration:** `nexus-bootstrap/src/plugins/audio.rs` registers `com.nexus.audio` with `on_init: true` (no start/stop), feeds `IPC_HANDLERS` (with v1 aliases) and `MANIFEST_DEPS` into the manifest, and uses `or_lifecycle_skip` so a config-load failure degrades gracefully.

## Tests

No `tests/` integration directory. All coverage is in-crate `#[cfg(test)]` modules:
- `lib.rs` — `AudioBackendName` `FromStr` round-trip incl. rejection of bogus names.
- `config.rs` — defaults when no file, parsing the `[audio]` block, rejecting unknown backend, env override of `provider_api_key`.
- `core_plugin.rs` — `MockStt`/`MockTts` injection via `with_backends`: `transcribe` byte round-trip + text/language/backend fields, `synthesize` base64 round-trip, `status` backend pair, invalid-base64 rejection, unknown-handler error, `on_init` building default backends (asserts default `local` pair when no config file — note this differs from the documented `platform` default because the test does not write a config and `Default` is consumed before the `load`-time anchoring path; see surprises).
- `stub_backend.rs` — `local`/`platform` stubs return `BackendNotEnabled` with the expected hint text.
- `local_backend.rs` (only with `local-audio`) — rejects non-WAV input, rejects wrong sample rate, decodes 16 kHz mono WAV, collapses stereo to mono.

---

### Surprises / gaps

- The `on_init_builds_default_backends_when_no_config_file` test asserts the resulting `status` is `local`/`local`, yet `AudioConfig::default()` and the documented shipping default are both `platform`/`platform`. This looks like a test expecting stale defaults; worth verifying whether the test or the doc/default is authoritative. (Not changed — documentation task only.)
- The crate doc comment / stub still reference the old `local-whisper` feature name and BL-118 in user-facing error strings; the active feature is `local-audio` with `local-whisper` kept as an alias.
