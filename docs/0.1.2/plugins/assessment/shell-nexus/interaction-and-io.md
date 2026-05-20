# Interaction and I/O

This category covers shell-side helpers for user interaction and I/O
that don't fit elsewhere: the shared confirm modal, the MCP host
inspector, the Web Speech audio runtime, and the terminal multiplexer
frontend. They sit between feature plugins and either the user
(confirm), an external protocol (MCP), or the browser / host platform
(audio, terminal).

### nexus.confirm

- **Path:** `shell/src/plugins/nexus/confirm/`
- **Surface:** Registers `nexus.confirm.modal` into the `overlay` slot
  at priority 90 (above MCP's tool-call modal at 30 and the plugins
  modal at 20 so a confirm raised from inside another modal lands on
  top). The modal itself reads `useConfirmStore`; `api.input.confirm`
  is wired in `host/PluginAPI.ts` to push a request onto that store
  and return a Promise that resolves on click. Plugins call
  `api.input.confirm(...)` without knowing this plugin exists.
- **Depends on:** Nothing structural — `useConfirmStore` is plain
  zustand; the `api.input` wiring lives in the host, not in another
  plugin. Dependents: every plugin that calls `api.input.confirm`.
- **Verdict:** Useful
- **Rationale:** Provides the only React surface that renders confirm
  prompts; without it `api.input.confirm` resolves to a never-mounted
  Promise and destructive actions hang silently. Not on the basic
  browse / edit / search / git path itself, but several Essential
  flows (file rename, delete) call through `api.input.confirm`, so
  removing it would visibly break those.

### nexus.mcp

- **Path:** `shell/src/plugins/nexus/mcp/`
- **Surface:** Registers the `mcp` view type, a `ToolCallModal` in the
  `overlay` slot (priority 30), an activity-bar entry (`plug` icon,
  priority 50), and commands `nexus.mcp.refresh`, `nexus.mcp.show`.
  Talks to `com.nexus.mcp.host` via IPC — `list_servers`, `connect`,
  `disconnect`, `list_tools`, `list_resources`, `list_prompts`,
  `call_tool`. Refreshes on `workspace:opened`, resets on `closed`.
  Connect timeouts use `SERVICE_CONNECT_TIMEOUT_MS` so cold-start
  subprocess spawn doesn't hit the default 30 s.
  `dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar']`.
- **Depends on:** `com.nexus.mcp.host` core plugin (the rmcp client +
  per-forge `mcp.toml` config); activity bar; workspace lifecycle.
- **Verdict:** Optional
- **Rationale:** UI for the MCP host subsystem. Useful for users
  wiring external tool servers (filesystem, web search, etc.) into the
  agent, but not part of basic browse / edit / search / git. The
  backend itself is gated by config and stays dormant without a
  populated `mcp.toml`.

### nexus.audio

- **Path:** `shell/src/plugins/nexus/audio/`
- **Surface:** Commands `nexus.audio.transcribe`, `synthesize`,
  `status`. Settings (registered via `api.configuration.register`):
  `nexus.audio.useWebSpeech` (default true), `defaultLanguage`,
  `defaultVoice`, `defaultRate`. The runtime in `runtime.ts`
  dispatches transcribe / synthesize to either the in-browser Web
  Speech API (`webkitSpeechRecognition` / `SpeechSynthesisUtterance`)
  or — when `useWebSpeech` is off — IPC into the `com.nexus.audio`
  core plugin (Whisper / Piper / OpenAI). Transcribe copies the
  result to the system clipboard. Re-exports `transcribe`,
  `synthesize`, `startContinuous` so other plugins (quick-capture,
  agent voice mode) can call the runtime directly without going
  through the command bus.
- **Depends on:** Browser Web Speech API (when enabled); `com.nexus.audio`
  core plugin (when disabled); clipboard via `navigator.clipboard`.
- **Verdict:** Optional
- **Rationale:** STT/TTS layer. Genuinely useful for hands-free
  capture and accessibility, but not on the basic markdown-editing
  path. Removing it disables voice-mode features in other plugins
  cleanly — the runtime exports are tree-shake-safe imports.

### nexus.terminal

- **Path:** `shell/src/plugins/nexus/terminal/`
- **Surface:** Registers four view types — `terminal` (the live
  session pane), `SAVED_COMMANDS_VIEW_TYPE`, `HISTORY_VIEW_TYPE`,
  `CROSS_SEARCH_VIEW_TYPE` — plus an activity-bar item with a custom
  terminal-glyph icon. Commands `nexus.terminal.toggle` / `focus` /
  `savedCommands.show` / `history.show` / `crossSearch.show`. Talks
  to `com.nexus.terminal` via IPC handlers `create_session`,
  `close_session`, `read_raw_since`; subscribes to the
  `com.nexus.terminal.output.<sessionId>` event stream with a single
  forwarder that fans out across sessions in the store. Persists the
  saved-commands list and history rings.
  `dependsOn: ['nexus.workspace', 'nexus.activityBar']`.
- **Core-vs-nexus:** `core.terminal` (`shell/src/plugins/core/terminal/`)
  is the Phase 7 legacy template — its `terminal.toggle` is an
  explicit no-op, and the file's own comment says it's "NOT loaded
  from `main.tsx`". Its only live work is registering a font-size /
  font-family configuration schema under `core.terminal`. The
  active terminal surface is entirely owned by `nexus.terminal`,
  backed by the `com.nexus.terminal` core Rust plugin in
  `crates/nexus-terminal`. So the three layers are: Rust backend
  (`com.nexus.terminal`) → shell frontend (`nexus.terminal`) →
  legacy config-schema stub (`core.terminal`, do-not-load).
- **Depends on:** `com.nexus.terminal` core plugin (PTY sessions,
  scrollback buffer); activity bar; workspace lifecycle.
- **Verdict:** Optional
- **Rationale:** Embedded terminal. A polished feature with cross-
  session search, history, and saved commands, but not on the basic
  browse / edit / search / git path — a user can run git in their
  external shell. Most Nexus users will want it, but absence does not
  break note editing.

## Category verdict

| Plugin            | Verdict  | Required for basic workflow |
|-------------------|----------|-----------------------------|
| `nexus.confirm`   | Useful   | No directly — but several Essential flows call `api.input.confirm` |
| `nexus.mcp`       | Optional | No — frontend for MCP host subsystem |
| `nexus.audio`     | Optional | No — STT/TTS UI |
| `nexus.terminal`  | Optional | No — embedded terminal, external shell suffices |

The core-vs-nexus duplication in this category is one-sided:
`core.terminal` is a Phase 7 legacy stub (not loaded; toggle is a
no-op; retained only for its config schema), and the active terminal
implementation is `nexus.terminal` paired with the
`com.nexus.terminal` Rust backend. The other three plugins
(`nexus.confirm`, `nexus.mcp`, `nexus.audio`) have no `core.*`
counterpart.
