# settings

- **Path:** `shell/src/plugins/core/settings/`
- **Tier:** Shell Core
- **Catalog entry:** `shell/src/plugins/catalog.ts:106` (`DEFAULT_ON_PLUGINS`, `popoutCompatible: false`, `dependsOn: ['core.configuration-service', 'nexus.activityBar']`).

## Architecture
- Entry: `shell/src/plugins/core/settings/index.ts:10` — exports `settingsPlugin` with manifest `id: 'core.settings'`.
- View: `shell/src/plugins/core/settings/SettingsPanelView.tsx` — the entire Settings UI: General / Appearance / Hotkeys / Snippets / Plugins built-in tabs plus auto-generated sections from every `ConfigSection` registered via `core.configuration-service`, plus plugin-contributed tabs via `api.settings.registerTab` (read off `SettingsTabRegistry`). Plugins-page body is rendered inline via `PluginsMgmtInline` from `nexus.pluginsMgmt`.
- Activation: `onStartup`. `popoutCompatible: false` — does not boot in popout windows.
- Persistence: indirect — reads/writes `useConfigStore` (backed by `core.configuration-service`) and `useThemeStore`. Owns no `.forge/` file.
- Settings owned: none of its own; renders everyone else's.
- External deps: `@tauri-apps/api/core` (invoke), `@tauri-apps/plugin-dialog` for "Open folder" pickers.

## Surface
- Commands: `workbench.action.openSettings`, `workbench.action.openKeybindings`, `workbench.action.openHelp`.
- Keybindings: `ctrl+,` / `cmd+,` → `workbench.action.openSettings`.
- Views: registers `settingsPanel` into the `overlay` slot at priority 90 (`index.ts:59`).
- Activity bar: contributes two bottom-rail items — `core.help.activityBarItem` (priority 99) and `core.settings.activityBarItem` (priority 100), wired to the open commands.
- Context keys: owns `settingsPanelVisible`, `settingsActiveTab`.

## Necessity
- **Verdict:** Essential.
- **Required for basic capabilities?** Yes — this is the only path users have to change theme, keybindings, snippets, plugin enablement, and per-plugin config. Without it, the forge is openable and editable but immutable in configuration; per recent commits (`bf1a1341`, `dee79302`, `9c53cbca`, `77fd8b2e`), plugin management is now rendered inline here, making this the canonical settings + plugins surface.
- **Depended on by:** every plugin that registers a Settings tab via `api.settings.registerTab` (e.g. `nexus.notificationsSettings`, theming flows in `nexus.themePicker`, snippet management). No plugin declares `dependsOn: ['core.settings']`, but the Plugins page imports `PluginsMgmtInline` from `nexus.pluginsMgmt`.
- **Depends on:** `core.configuration-service` (config store + schema registry), `nexus.activityBar` (entry points), `nexus.pluginsMgmt` (Plugins page body), `core.theme-service` (Appearance tab data).
- **What breaks if removed:** no Settings UI, no Hotkeys editor, no inline Plugins page, no Appearance picker. Themeable + configurable shell drops to whatever defaults `core.configuration-service` returns; user has no editor for them.

## Notes
- The view component is closed over `api` via a `SettingsPanelHost` wrapper (`index.ts:56-58`) because the slot system does not propagate props — change with care if the slot API is ever revised.
- Help command opens `https://github.com/baileyrd/nexus` via `window.open`; works under Tauri because the webview routes external targets to the OS browser.
