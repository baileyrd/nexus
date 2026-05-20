# configurationService

- **Path:** `shell/src/plugins/core/configurationService/`
- **Tier:** Shell Core
- **Plugin id:** `core.configuration-service`

## Architecture
- Entry point: `shell/src/plugins/core/configurationService/index.ts:19`
- Activation: `onStartup`
- Modules:
  - `index.ts` — the entire plugin; thin shim that constructs the registry and store, wires hydrate/reset to workspace lifecycle
- Backing implementations live outside this directory:
  - `shell/src/registry/ConfigurationRegistry.ts` — schema registry for plugin-contributed `configuration` blocks
  - `shell/src/stores/configStore.ts` — Zustand-style store holding the live values; exports `hydrateFromForge` and `resetForWorkspaceClose`
- Persistence: values are read from / written to the forge's `app.toml` `[settings]` table over IPC (`hydrateFromForge` runs on `workspace:opened` and on cold start once the kernel is available)
- Settings owned: none directly — owns the *mechanism* every other plugin uses
- External deps: `clientLogger`; the kernel IPC bridge via `api.kernel.available()`

## Surface
- **Commands / views / keybindings:** none
- **Internal services registered (via `api.internal.registerInternalService`):**
  - `configurationRegistry` — instance of `ConfigurationRegistry`
  - `configStore` — the live config store
- **Events consumed:** `workspace:opened` (triggers hydrate), `workspace:closed` (resets to defaults)
- **Consumes from `@nexus/extension-api`:** the `Plugin`, `PluginAPI` types, and the `api.internal` + `api.kernel` + `api.events` surfaces

## Necessity
- **Verdict:** Essential
- **Required for basic capabilities?** Yes — `api.configuration.register`, `api.configuration.getValue`, `api.configuration.setValue` are unusable until this plugin activates. Every plugin that exposes a setting (file explorer sort order, AI provider, theme, notification duration, palette limit, …) reads through it.
- **Depended on by:** declared `dependsOn` from `core.settings` (`catalog.ts:109`) and `nexus.themePicker` (`catalog.ts:307`); transitively required by essentially every other plugin since they all call `api.configuration.register`. Direct importers of `configStore` include `core.notification-service`, `core.terminal`, `core.zoom`, `nexus.editor`, `nexus.terminal`, `nexus.search`, `nexus.bookmarks`, `nexus.canvas`, `nexus.ai`, `nexus.linkSuggest`, and others.
- **Depends on:** the Tauri host's settings IPC handler (used by `hydrateFromForge`); the kernel readiness signal (`api.kernel.available`)
- **What breaks if removed:** Every plugin's settings round-trip to disk breaks; per-forge config bleeds across workspaces; the Settings UI has nothing to render.

## Notes
- Hydrate gate mirrors `core.theme-service`: kernel_invoke fails before `nexus.workspace` opens a forge, so the plugin probes `api.kernel.available()` and also subscribes to `workspace:opened` for the warm path.
- Resetting on `workspace:closed` is the critical guarantee that one forge's settings don't leak into the next.
