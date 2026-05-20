# com.nexus.audio

- **Path:** `crates/nexus-audio/`
- **Tier:** Core Rust
- **Bootstrap order:** 22

## Architecture

- Entry point `crates/nexus-audio/src/lib.rs` re-exports `AudioCorePlugin`, `AudioConfig`, `AudioBackendName`, the backend traits (`SttProvider`, `TtsProvider`, `AudioBackends`), the format / IO structs, and `AudioError`. Registered by `crates/nexus-bootstrap/src/plugins/audio.rs` with only `on_init` enabled (no `on_start` / `on_stop`).
- Key modules: `core_plugin` (3 IPC handlers), `config` (`[audio]` block loader from `<forge>/.forge/config.toml`), `backend` (provider traits + `AudioBackends` pair), `provider_backend` (reqwest-based OpenAI `/v1/audio/{transcriptions,speech}` adapter — uses `nexus-security`'s TLS-pin builder), `stub_backend` (returns `AudioError::BackendNotEnabled`), optional `local_backend` (feature-gated `local-audio`, pulls in `whisper-rs` + `hound` + `tempfile`).
- Three backends per direction: `local` (whisper.cpp via `whisper-rs`, Piper / espeak-ng / `say` / SAPI for TTS), `provider` (routes through configured AI provider — OpenAI today), `platform` (shell-side Web Speech API; the Rust crate stubs it out — populated via `AudioCorePlugin::with_backends` from the shell contribution path).
- `on_init` loads `<forge>/.forge/config.toml::[audio]` and builds the backend pair (`AudioConfig::build_backends`). `with_backends` constructor exists for test injection + the BL-118 shell-side platform-backend handoff. `wire_context` plumbs in `KernelPluginContext` so the provider backend can call `com.nexus.ai::resolve_credentials` at dispatch time. No `on_start` / `on_stop` (stateless beyond the held backend pair).
- Persistence: `<forge>/.forge/config.toml` `[audio]` block (documented at `docs/0.1.2/settings/forge-config.md:157`). Local Whisper models cached at `<forge>/.forge/.audio/models/ggml-*.bin` (override via `local_model_dir`). No SQLite.
- Settings owned: `[audio]` block (`crates/nexus-audio/src/config.rs:64`) — `stt_backend`, `tts_backend`, `local_model_size`, `local_model_dir`, `tls_pinning_enabled` (BL-102, read from `KernelConfig`), `provider_api_key`, `provider_base_url`, `provider_stt_model`, `provider_tts_model`, plus voice default and more in the file's lower portion.
- External dependencies: native — `whisper-rs` (feature-gated; ~3-5 min build, ~75 MB model download on first run), `hound` (WAV decode), `reqwest` with `multipart` + `blocking` (calls `https://api.openai.com` by default), `tempfile` (TTS shell-out). System binaries for local TTS: `espeak-ng` (Linux), `say` (macOS), PowerShell SAPI (Windows). The `provider` backend opens outbound HTTPS; capability `network` is gated upstream by the kernel.

## Surface

- IPC handlers (from `IPC_HANDLERS` in `core_plugin.rs:38`):
  - `transcribe` (1) — STT: `{audio_b64, format?, language?}` → `{text, language?, backend}`. Capability `audio.record`.
  - `synthesize` (2) — TTS: `{text, voice?, format?}` → `{audio_b64, format, backend}`. Capability `audio.synthesize`.
  - `status` (3) — `{stt_backend, tts_backend}`.
- No bus events. No UI contributions from the Rust side — the shell-side `nexus.audio` plugin (`shell/src/plugins/nexus/audio/`) provides the Web Speech API platform backend through a contribution path.

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No. Markdown edit / search / git does not invoke STT or TTS.
- **Depended on by:** shell `nexus.audio` plugin (`shell/src/plugins/nexus/audio/index.ts`, `runtime.ts`, `speechApi.ts`) — registers the platform backend and exposes a thin in-shell API for other plugins; consumed via the TS bindings generated under `packages/nexus-extension-api/src/generated/ipc/Audio*.ts`. No CLI/TUI verbs.
- **Depends on:** `nexus-kernel`, `nexus-plugins`, `nexus-security` (for the BL-102 TLS-pinned HTTP client builder shared with `nexus-ai`).
- **What breaks if removed:** dictation, read-aloud, and any future agent voice-IO mode go offline. Markdown / search / git unaffected.

## Notes

- The shipped default build stubs `local` (no `local-audio` feature) and `platform` (no shell), so a fresh forge with the default `local` backend hits `AudioError::BackendNotEnabled` on first dispatch. Operators flip to `provider` (with `OPENAI_API_KEY`) or build with `local-audio` to opt in.
- BL-118 platform backend handoff: the shell registers itself as the `platform` backend through `AudioCorePlugin::with_backends`; the Rust crate alone cannot drive Web Speech.
- Provider backend additionally consumes the AI plugin's credential resolution via `wire_context` — there is a soft dependency on `com.nexus.ai` being registered when `provider` is selected.
- Not listed in `docs/0.1.2/settings/hardcoded-rust.md` audit — the `[audio]` block is fully promoted to settings.
