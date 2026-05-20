# zoom

- **Path:** `shell/src/plugins/core/zoom/`
- **Tier:** Shell Core
- **Catalog entry:** `shell/src/plugins/catalog.ts:127` (`DEFAULT_ON_PLUGINS`, `core: false`, `activationEvents: ['onStartup']`).

## Architecture
- Entry: `shell/src/plugins/core/zoom/index.ts:34` — exports `zoomPlugin` with manifest `id: 'core.zoom'`. Activation handler is `async`.
- Applies `document.documentElement.style.zoom = level` (CSS `zoom`, non-standard but supported by every webview Tauri ships against — WebView2 / WebKit / WebKitGTK). Scales chrome, terminal, modals, and overlays uniformly.
- Reads/writes `ui.zoom` via `api.configuration.getValue` / `setValue` with `clamp(min, max)` bounds rounded to one decimal. Subscribes via `api.configuration.onChange('ui.zoom', …)` so writes from the Settings panel (which bypass the in-plugin `write()` wrapper) re-apply and re-clamp.
- Persistence: through `core.configuration-service` only (key `ui.zoom`).
- Settings owned (`configuration.schema` in `index.ts:56-100`, category `appearance`, order 5):
  - `ui.zoom` — number, default 1.0.
  - `ui.zoomStep` — number, default 0.1.
  - `ui.zoomMin` — number, default 0.5.
  - `ui.zoomMax` — number, default 3.0.
  - `ui.zoomDefault` — number, default 1.0 (reset target).
  - Not yet documented in `docs/0.1.2/settings/`.
- External deps: only `document.documentElement.style.zoom` (DOM API).

## Surface
- Commands: `core.zoom.in`, `core.zoom.out`, `core.zoom.reset`.
- Keybindings: `ctrl+=` / `cmd+=`, `ctrl+shift+=` / `cmd+shift+=` (US keyboard `Ctrl++`), `ctrl+-` / `cmd+-`, `ctrl+0` / `cmd+0`.
- Views: none.
- Configuration section: `core.zoom` titled "Zoom", category `appearance`.

## Necessity
- **Verdict:** Optional.
- **Required for basic capabilities?** No — zoom is convenience-grade. Open / browse / edit / search / commit all work at the default `zoom: 1`.
- **Depended on by:** none. No `dependsOn: ['core.zoom']` in any other plugin or catalog entry.
- **Depends on:** `core.configuration-service` (implicitly, via `api.configuration`). Not declared as a `dependsOn` in the manifest, which is a minor oversight — the plugin is in `DEFAULT_ON_PLUGINS` after the configuration service in the catalog order, so it works in practice.
- **What breaks if removed:** the four zoom chords and the Appearance → Zoom settings disappear; UI is locked at the browser's intrinsic scaling. Nothing else regresses.

## Notes
- The plugin is marked `core: false` in the catalog despite living under `shell/src/plugins/core/` — consistent with "shell-core directory" being a structural label, not a privilege grade.
- The `clamp` helper rounds to `Math.round(n * 10) / 10` to avoid floating-point drift across many incremental Ctrl+= presses — sensible.
