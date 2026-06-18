# Environment Variables

> Every env var the runtime reads. Sourced from `grep env::var` across `crates/` and `shell/`. If you set one and nothing happens, it's not in this list — please file it.

## Nexus-owned

| Variable | Read at | Default | Effect |
|----------|---------|---------|--------|
| `NEXUS_FORGE_PATH` | `nexus-cli/src/main.rs:1747`, `nexus-tui/src/lib.rs:93` | _(unset)_ | Override discovered forge path. `--forge-path` CLI flag wins over this. |
| `NEXUS_LOCAL_EMBEDDINGS` | `nexus-ai/src/config.rs:92` | `0` | `1`/`true`/`yes`/`on` enables local fastembed embedding provider. |
| `NEXUS_LOCAL_EMBEDDING_MODEL` | `nexus-ai/src/config.rs:96` | `bge-small-en-v1.5-int8` | Override fastembed model identifier. |
| `NEXUS_TLS_PINNING` | `nexus-ai/src/core_plugin.rs:1657`, `handlers/shared.rs:190` | _(off)_ | Enable TLS cert pinning for AI provider endpoints. |
| `NEXUS_NO_KEYRING` | `nexus-plugins/src/grants_crypto.rs:49` | _(off)_ | Disable OS-keyring sealing of granted_caps.json (development only). |
| `NEXUS_SHELL_BIN` | `nexus-cli/src/commands/desktop.rs:52` | searched on PATH | Path to `nexus-shell` executable; consulted by `nexus desktop`. |
| `NEXUS_TUI_LOG` | `nexus-tui/src/lib.rs:107` | `$TEMP/nexus-tui.log` | TUI log file path. |
| `NEXUS_SUBAGENT_BIN` | `nexus-agent/src/subagent.rs` (`SubagentRunner::resolve`) | _(current_exe)_ | Path to the `nexus` CLI used to spawn isolated subagents (RFC 0007, `delegate isolation="worktree"`). The binary location is install-specific, so it's an env var, not a forge setting. Frontends whose own `current_exe()` isn't the CLI (Tauri shell, MCP) set this. |
| `NEXUS_SUBAGENT_MAX_CONCURRENT` | `nexus-agent/src/subagent.rs` (`subagent_semaphore`) | `4` | Cap on isolated subagents (each a full child `nexus` process) running concurrently in one process. Non-positive / unparseable values fall back to the default. |
| `NEXUS_EMBEDDED_SHELL` | `nexus-rush/src/main.rs` (`set_embedded`) | _(unset)_ | Set by `nexus-terminal` (any value) when launching the bundled `nexus-rush` shell inside a Nexus-owned PTY (RFC 0002). Tells rush to disable its job-control terminal hand-off (`tcsetpgrp`/`setpgid`) so it doesn't fight portable-pty's session leader for the controlling terminal. Not meant to be set by operators. |

## AI provider detection (read directly by `nexus-ai`)

| Variable | Read at | Effect |
|----------|---------|--------|
| `ANTHROPIC_API_KEY` | `nexus-ai/src/config.rs:114` | Triggers Anthropic provider detection at boot. |
| `OPENAI_API_KEY` | `nexus-ai/src/config.rs:121,151` | Triggers OpenAI provider detection at boot. |
| `OLLAMA_BASE_URL` | `nexus-ai/src/config.rs:128,158` | Triggers Ollama provider detection at boot. Defaults to `http://localhost:11434`. |

## Toolchain / Rust stdlib

| Variable | Read at | Effect |
|----------|---------|--------|
| `RUST_LOG` | tracing-subscriber init in every binary | Filter directives for the global `tracing` subscriber. |
| `NO_COLOR` | `nexus-cli/src/output.rs:46` | Disable ANSI colour output (text mode). |
| `HOME` / `USERPROFILE` | `nexus-cli/src/main.rs:1751`, `nexus-cli/src/commands/plugin.rs:588-589` | User home directory resolution. |
| `PATH` | `nexus-cli/src/commands/desktop.rs:97` | Searched to locate shell binary. |
| `VISUAL` / `EDITOR` | `nexus-tui/src/input.rs:422-423` | External editor for input. |
| `SHELL` | `nexus-terminal/src/shell.rs:64` | Default shell for new terminal sessions. |
| `TEMP` | `nexus-tui` log path resolution | TUI log fallback dir on Windows. |

## Tauri / WebKit (`shell/`)

These are **baked into `shell/`'s `tauri:dev` invocation** for WSLg compatibility — see the comment in `shell/package.json` and the `feedback-wslg-webkit-env` memory.

| Variable | Default in dev script | Why |
|----------|----------------------|-----|
| `WEBKIT_DISABLE_COMPOSITING_MODE` | `1` | Avoids GTK compositing crash on WSLg. |
| `WEBKIT_DISABLE_DMABUF_RENDERER` | `1` | Forces software renderer; DMABUF fails on WSLg. |
| `GDK_BACKEND` | `x11` | Forces X11 over Wayland on WSLg (Wayland surfaces unstable). |

## Compile-time

| Variable | Read at | Effect |
|----------|---------|--------|
| `CARGO_PKG_VERSION` | `nexus-bootstrap/src/lib.rs:291` (via `env!`) | Embedded version string for `--version` output. |

## Reserved / mentioned but not actively read

- `XDG_*` — used by `dirs` crate internally for OS-specific dirs (config / cache / data). Not read directly by Nexus code.
- `TAURI_*` — set by Tauri runtime on the WebView; consumed by Tauri internals.
