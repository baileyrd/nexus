# titleBar

- **Path:** `shell/src/plugins/core/titleBar/`
- **Tier:** Shell Core
- **Status:** Legacy template — **not loaded** by `main.tsx`. Absent from `shell/src/plugins/catalog.ts`. The shell renders its top chrome via other slot contributions (workspace + activity bar) and the Tauri window decorations.

## Architecture
- Entry: `shell/src/plugins/core/titleBar/index.ts:5` — exports `titleBarPlugin` with manifest `id: 'core.title-bar'`.
- View: `shell/src/plugins/core/titleBar/TitleBarView.tsx` — three-column grid (cluster | breadcrumb | win-controls) using `@tauri-apps/api/window`'s `getCurrentWindow()` for min/max/close and a drag-region for window movement. Imports `getRegistry` from `host/shellRegistry` to dispatch `nexus.rightPanel.toggle`.
- Activation: `onStartup` (manifest), unused because the plugin is not enrolled in the catalog.
- Persistence: none.
- Settings owned: none.
- External deps: `@tauri-apps/api/window` (window controls), `host/shellRegistry` for command execution.

## Surface
- Commands: `window.minimize`, `window.maximize`, `window.close` — all defer to `api.platform.window.*` (the Tauri-only host-platform primitive bridge).
- Keybindings: none.
- Views: registers `titleBar` into the `titleBar` slot.

## Necessity
- **Verdict:** Useful (concept) / Removable (this file).
- **Required for basic capabilities?** No — markdown open / edit / search / git commit don't need a custom title bar. Most desktop shells run with native window decorations; Tauri provides them out of the box.
- **Depended on by:** none. No catalog entry; no `dependsOn: ['core.title-bar']` exists. The popout-compatibility contract test does not enumerate this file.
- **Depends on:** `nexus.rightPanel` (the right-panel toggle button in the view body fires `nexus.rightPanel.toggle`) — but only if the view is ever rendered, which it isn't.
- **What breaks if removed:** nothing at runtime. The `titleBar` slot would have no contributor unless another plugin claims it; the current shell already runs without this plugin loaded.

## Notes
- The view leaks `getCurrentWindow()` calls directly rather than going through `api.platform.window`, which is what the registered commands do. If this file is ever revived, that inconsistency should be reconciled.
- `useContextKey('nexus.rightPanel.visible')` in `TitleBarView.tsx:28-30` shows the chrome design was meant to mirror right-panel state — but that pressed-state UI is currently delivered through `nexus.workspace` chrome, not here.
