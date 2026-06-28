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
3. **Community** — discovered at runtime from `~/.nexus-shell/plugins/<name>/` (user-installed, all modes), plus the in-repo `shell/src/plugins/community/` in dev mode. Loaded into an iframe sandbox (ADR 0015). See `shell/src/host/communityPluginLoader.ts`.

## Core plugins (7)

Provided by the host from `shell/src/plugins/core/`; plugin id begins with `core.*`. The authoritative list is curated in `shell/src/plugins/catalog.ts`.

| Plugin id | Folder | Provides |
|-----------|--------|----------|
| `core.capabilityPrompt` | `core/capabilityPrompt/` | modal that grants/denies high-risk plugin capabilities |
| `core.configuration-service` | `core/configurationService/` | shell config store; backs `api.configuration` for every plugin |
| `core.filesystem-service` | `core/fileSystemService/` | forge-relative file IO with capability checks |
| `core.notification-service` | `core/notificationService/` | toast + status notifications via `api.notifications` |
| `core.settings` | `core/settings/` | settings panel — config sections, themes, keybindings, plugin management |
| `core.theme-service` | `core/themeService/` | loads/switches/persists themes; exposes CSS variables to the shell |
| `core.zoom` | `core/zoom/` | app-wide UI zoom with persisted level (`Ctrl+=`, `Ctrl+-`, `Ctrl+0`) |

> Earlier drafts listed 17 `core.*` chrome plugins (`core.activityBar`, `core.editorArea`, `core.fileExplorer`, `core.panelArea`, `core.sidebar`, `core.statusBar`, `core.rightPanel`, `core.terminal`, `core.titleBar`, `core.commandPalette`). Those were refactored into `nexus.*` first-party plugins (e.g. `nexus.activityBar`, `nexus.rightPanel`, `nexus.statusBar`, `nexus.terminal`, `nexus.commandPalette`) or folded into built-in chrome; they are no longer core plugins.

## Nexus first-party plugins (58)

Folder names under `shell/src/plugins/nexus/` (each is one `nexus.*` plugin id; a few folders register more than one catalog entry — e.g. `graph` → `nexus.graph` + `nexus.graph.global`). The enable/disable split is curated in `shell/src/plugins/catalog.ts`: a default-on set loaded at boot plus a default-off set the user opts into from **Settings → Plugins** (backend-gated features such as `collab`, `crdtConflict`, and `dreamCycle` ship default-off). Domain groupings:

- **Editing & content:** `editor`, `outline`, `paneMode`, `comments`, `crdtConflict`, `templates`, `linkSuggest`
- **AI surfaces:** `ai`, `aiSettings`, `agent`, `sessions` (session-tree navigator — resume/branch/rewind/checkpoint, RFC 0008), `recall`, `semanticSearch`, `enrich`, `dreamCycle`, `skills`, `memory`, `memoryDashboard`
- **Navigation & context:** `files`, `search`, `searchPanel`, `bookmarks`, `launcher`, `workspace`, `noteContext` (backlinks / outgoing links / tags / per-file graph)
- **Visualisation:** `canvas`, `graph`, `bases`, `osArchitecture`, `activityTimeline`, `viewBuilder`
- **Tools / processes:** `processes`, `terminal`, `gitPanel`, `gitStatus`, `mcp`, `workflow`, `debugger`, `sandboxPanel`
- **System / chrome supplements:** `activityBar`, `rightPanel`, `statusBar`, `commandPalette`
- **Notifications & observability:** `notifications`, `notificationsInbox`, `notificationsSettings`, `healthPanel`, `observability`, `diagnostics`
- **Files & metadata:** `fileProperties`
- **Extension / catalog:** `pluginsMgmt`, `themePicker`
- **Collaboration:** `collab`
- **Audio:** `audio`
- **Notion import/export:** `notion`
- **UX primitives:** `confirm`, `prompt`, `pick`

Each is a folder with an `index.ts` exporting a `Plugin` object — `export const <name>Plugin: Plugin = { manifest, activate, deactivate? }`. Contributions ride through `ExtensionHost` and end up registered in the relevant `SlotRegistry`. (`_lib/` is a shared-utility namespace, not a plugin; `constants.ts` holds the shared cap/threshold table.)

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

1. **Hand-authored** — `src/index.ts` re-exports the plugin-contract types (`NexusPluginContext`, `ScriptPlugin`, `Capability`, etc.) plus shared SDK enums and helpers. There is no `definePlugin` factory — first-party shell plugins export a `Plugin` object directly, and community/script plugins default-export a `ScriptPlugin`.
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

1. Author `shell/src/plugins/nexus/<feature>/index.ts` exporting a `Plugin` object — `export const <feature>Plugin: Plugin = { manifest, activate }` — and register it in `shell/src/plugins/catalog.ts`.
2. In `activate(api)`, register the pane through the Leaf/View pipeline: `api.viewRegistry.register('<view-type>', creator)` (where `creator` returns the React view), then give it an entry point — an activity-bar icon via `api.activityBar.addItem({...})` and/or a focus command (`api.commands.register`) that calls `api.workspace.ensureLeafOfType('<view-type>', 'left' | 'right')` + `api.workspace.revealLeaf(leaf)`. (Fixed chrome widgets — status bar, overlay — use `api.views.register(viewId, { slot, component, priority })` instead.)
3. The component reads and writes through the same `api` captured in `activate` (or a shared store it updates): `api.kernel.invoke(pluginId, command, args)` for IPC and `api.events.on(topic, handler)` for bus events. There is no per-component `PluginContext`/`ctx.ipc`.
4. First-party plugins run trusted (granted `Capability::ALL`), so no capability declaration is needed. Capability-gating is the **community** tier: those ship a manifest declaring `capabilities` (PascalCase strings matching the kernel `Capability` enum) that the user grants at install (WI-31).
5. If state needs persistence, write through `com.nexus.storage::settings_write` or a plugin-side `kv.write` — **never** a new Tauri command.
