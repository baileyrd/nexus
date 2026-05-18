# Shell + Packages

> **As of:** 2026-05-17. Tauri 2 desktop shell (`shell/`) + Rust bridge (`shell/src-tauri/`) + TypeScript SDK (`packages/nexus-extension-api/`). The shell is the single active desktop target per the v0.4.0 legacy retirement.

## Tree

```
shell/
├── src/                          # React frontend (TypeScript)
│   ├── shell/                    # chrome: layout, panes, popouts
│   ├── workspace/                # leaf/pane content host
│   ├── host/                     # ExtensionHost + community plugin loader + sandbox
│   ├── registry/                 # SlotRegistry, command palette, theme registry
│   ├── stores/                   # zustand stores
│   ├── plugins/
│   │   ├── core/                 # 17 chrome plugins (provided by host)
│   │   ├── nexus/                # 65 first-party feature plugins
│   │   └── community/            # 3rd-party plugin landing (loaded at runtime)
│   └── types/                    # boundary types
├── src-tauri/                    # Rust ↔ WebView bridge (Tauri commands)
├── e2e/                          # Playwright E2E suite
└── tests/                        # node:test unit tests

packages/
└── nexus-extension-api/          # @nexus/extension-api — TypeScript SDK
```

## ExtensionHost

Single entry point: `shell/src/host/ExtensionHost.ts`. Loads every plugin contribution at startup (no built-in chrome shipped statically). The shell starts **empty** and is rebuilt from plugin contributions.

Plugin tiers visible to the host:
1. **Core** (`shell/src/plugins/core/`) — provided by the host, ships in the binary. Plugin id begins with `core.*`.
2. **Nexus first-party** (`shell/src/plugins/nexus/`) — also ships in the binary; plugin id begins with `nexus.*`.
3. **Community** — discovered at runtime from `~/.nexus-shell/plugins/<name>/`. Loaded into an iframe sandbox (ADR 0015).

## Core chrome plugins (17)

| Plugin id | Folder | Provides |
|-----------|--------|----------|
| `core.activityBar` | `core/activityBar/` | left-edge ribbon, slot for icons |
| `core.capabilityPrompt` | `core/capabilityPrompt/` | per-call cap consent banner |
| `core.commandPalette` | `core/commandPalette/` | `Ctrl+P` / `Ctrl+Shift+P` palette |
| `core.configurationService` | `core/configurationService/` | settings registry + persistence |
| `core.editorArea` | `core/editorArea/` | the editor pane host |
| `core.fileExplorer` | `core/fileExplorer/` | left-dock file tree |
| `core.fileSystemService` | `core/fileSystemService/` | high-level fs ops over `com.nexus.storage` |
| `core.notificationService` | `core/notificationService/` | toast container, dismiss queue |
| `core.panelArea` | `core/panelArea/` | bottom-dock leaf host |
| `core.rightPanel` | `core/rightPanel/` | right-dock leaf host |
| `core.settings` | `core/settings/` | settings modal |
| `core.sidebar` | `core/sidebar/` | left-dock leaf host |
| `core.statusBar` | `core/statusBar/` | bottom-strip status bar |
| `core.terminal` | `core/terminal/` | bottom-dock terminal pane |
| `core.themeService` | `core/themeService/` | CSS variable resolver, theme switch |
| `core.titleBar` | `core/titleBar/` | window-chrome title bar |
| `core.zoom` | `core/zoom/` | `Ctrl+=` / `Ctrl+-` zoom level |

## Nexus first-party plugins (65)

Domain groupings (folder names under `shell/src/plugins/nexus/`):

- **Editing & content:** `editor`, `outline`, `tags`, `templates`, `comments`, `crdtConflict`, `multibufferSync`, `linkSuggest`, `enrich`
- **AI surfaces:** `ai`, `agent`, `recall`, `semanticSearch`, `viewBuilder`, `dreamCycle`, `memory`
- **Navigation:** `backlinks`, `outgoingLinks`, `files`, `search`, `searchPanel`, `bookmarks`, `launcher`, `paneMode`, `workspace`, `pick`
- **Visualisation:** `canvas`, `graph`, `bases`, `osArchitecture`, `activityTimeline`
- **Tools / processes:** `processes`, `terminal`, `git`/`gitPanel`/`gitStatus`, `mcp`, `skills`, `workflow`, `debugger`
- **System / chrome supplements:** `activityBar`, `sidebar`, `rightPanel`, `statusBar`, `status`, `commandPalette`
- **Notifications & observability:** `notifications`, `notificationsInbox`, `notificationsSettings`, `healthPanel`, `observability`, `diagnostics`
- **Files & metadata:** `fileProperties`, `allProperties`
- **Extension / catalog:** `pluginsMgmt`, `extensionsTab`, `themePicker`
- **Collaboration:** `collab`
- **Audio:** `audio`
- **Notion import/export:** `notion`
- **UX primitives:** `confirm`, `prompt`
- **Constants:** `constants.ts` (shared cap/threshold table)

Each is a folder with an `index.ts` exporting a `definePlugin({...})` call. Contributions ride through `ExtensionHost` and end up registered in the relevant `SlotRegistry`.

## Tauri bridge (`shell/src-tauri/`)

The bridge is intentionally thin per ADR 0011: any real capability flows through `kernel_invoke` → `ipc_call`. Bridge code only owns shell-intrinsic concerns (kernel lifecycle, plugin management, persistence, popouts).

### 29 registered Tauri commands

(Up from 25 noted in earlier drafts — 4 commands were added across the intervening BLs. Confirmed via `shell/src-tauri/src/lib.rs:712-742`.)

Grouped by purpose:

**Kernel + bridge (10):**
- `init_forge` — write `.forge/` skeleton, prepare paths
- `boot_kernel` — start local kernel + plugin loader
- `boot_remote` — attach kernel to a remote forge over SSH (BL-140 Phase 3)
- `shutdown_kernel`
- `revoke_plugin_capability` — runtime cap revocation (paired with `set_plugin_granted_capabilities`)
- `kernel_invoke` — single chokepoint for `ipc_call`
- `kernel_subscribe` / `kernel_unsubscribe` — kernel-bus topic subscription → WebView events
- `kernel_is_booted`
- `kernel_connection_state` — observability for the remote-reconnect wrapper (BL-146)

**Plugin management (5):**
- `scan_plugin_directory` (default location)
- `scan_plugin_directory_at` (custom path)
- `set_plugin_enabled`
- `get_plugin_granted_capabilities`
- `set_plugin_granted_capabilities`

**Shell persistence (6):**
- `get_shell_state` / `save_shell_state`
- `write_last_forge_path` / `forget_forge_path`
- `write_remote_recent` / `forget_remote_recent` — extend recents to `ssh://` URIs

**Utility (3) — host-platform primitives:**
- `path_exists` — wraps Tauri's filesystem allowlist
- `append_shell_log` — wraps the renderer panic log path
- `notify_desktop` — wraps `tauri_plugin_notification`; the *only* path to OS-level toasts. Called by the shell-side `nexus.notifications` plugin in response to backend `com.nexus.notifications::send` events.

**Popout windows (5) — ADR 0020:**
- `popout_window`
- `close_popout_window`
- `list_popout_windows`
- `get_popout_window_bounds`
- `set_popout_window_bounds`

### Host-platform primitive pattern

The Utility + Popout commands all wrap a Tauri-only capability that requires a `tauri::AppHandle` and is unreachable from a backend service plugin (notification API, WebView window API, FS allowlist). These are deliberate exceptions to the thin-bridge rule and the *only* legitimate venue for a feature-shaped bridge command.

Adding a Tauri command that *isn't* a host-platform primitive is a smell — extend a service plugin's IPC instead.

## `packages/nexus-extension-api/`

The `@nexus/extension-api` TypeScript package. Two surfaces:

1. **Hand-authored** — `src/index.ts` re-exports types (`PluginManifest`, `PluginContext`, `Capability`, `Slot`, etc.) and the `definePlugin` helper.
2. **Generated** — `src/generated/ipc/*.ts` emitted by `ts-rs` from every pilot IPC type. Regenerated by `scripts/check_ipc_drift.sh`; the drift check fails CI if Rust source changes without committing the regenerated TS.

Sandbox surface for community/script plugins:
- `src/sandbox/context.ts` — the `PluginAPI` handed to a script plugin at boot. Carries the orchestrator-assigned `pluginId` (per F-8.1.2 boundary-binding).
- `src/sandbox/runtime.ts` — the entry-point shim for the iframe runtime.
- `src/sandbox/host.ts` — host-side counterpart that owns the iframe and proxies calls.

Capability strings used by the SDK match the kernel's `Capability` enum strings — see [`capabilities.md`](capabilities.md).

## Shell-side state vs forge-side state

| State | Where it lives | Why |
|-------|----------------|-----|
| Editor open files, layout, sidebar collapse, theme | `<forge>/.forge/workspace.json` | follows the forge |
| Active forge path, recent forges | `<app_config_dir>/shell-state.json` | survives forge switch |
| Plugin grant decisions | `<plugin_dir>/granted_caps.json` (encrypted) | per-plugin install state |
| Plugin enabled/disabled | `<plugin_dir>/plugin.json::enabled` | per-plugin install state |
| Theme tokens, snippet cascade | `<forge>/.forge/app.toml` via `com.nexus.theme` | per-forge personalisation |

## How a shell plugin adds a new pane

1. Author `shell/src/plugins/nexus/<feature>/index.ts` with a `definePlugin({...})` call.
2. Contribute a `UiPanel` registration pointing at a React component.
3. Component receives a `PluginContext` exposing `ctx.ipc(plugin_id, command, args)` and `ctx.events.subscribe(topic, handler)`.
4. Capabilities required by the plugin go in `manifest.capabilities.required`; the user grants at install.
5. If state needs persistence, write through `com.nexus.storage::settings_write` or a plugin-side `kv.write` — **never** a new Tauri command.
