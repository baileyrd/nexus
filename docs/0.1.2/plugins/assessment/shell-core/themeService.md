# themeService

- **Path:** `shell/src/plugins/core/themeService/`
- **Tier:** Shell Core
- **Catalog entry:** `shell/src/plugins/catalog.ts:121` (`DEFAULT_ON_PLUGINS`, `core: true`, `activationEvents: ['onStartup']`).

## Architecture
- Entry: `shell/src/plugins/core/themeService/index.ts:24` — exports `themeServicePlugin` with manifest `id: 'core.theme-service'`, version `2.0.0`. Activation handler is `async`.
- Pure lifecycle wrapper. State + DOM application live in `shell/src/stores/themeStore.ts` (Zustand). This plugin only:
  1. Calls `useThemeStore.getState().hydrate(api)` once at startup if `api.kernel.available()` (and re-runs on `workspace:opened` for the cold-start path where the kernel is not yet up).
  2. Subscribes to the `com.nexus.theme.changed` kernel event (`THEME_CHANGED_EVENT`) via `api.kernel.on` and re-hydrates on every payload.
- Replaces the prior in-process `ThemeService` (which carried brand-named built-in palettes and OS-pref auto-flip). All theme data now flows from `crates/nexus-theme` (the `com.nexus.theme` core Rust plugin).
- Persistence: none — `crates/nexus-theme` owns the on-disk state under `.forge/`.
- Settings owned: none directly; the kernel theme plugin owns `theme_id`, `mode`, `enabled_snippets`.
- External deps: `clientLogger`; consumes `api.kernel.available()`, `api.kernel.on`, and indirectly any `ipc_call` the store uses to hydrate.

## Surface
- Commands: none.
- Keybindings: none.
- Views: none.
- Configuration: none.
- Event subscriptions: `workspace:opened` (shell event bus), `com.nexus.theme.changed` (kernel event bus).

## Necessity
- **Verdict:** Useful.
- **Required for basic capabilities?** No — the shell renders with `shell.css` defaults if hydration fails (see the `try/catch` around `hydrate`, `index.ts:43-49`). Markdown open / edit / search / git commit do not depend on theme state.
- **Depended on by:** `nexus.themePicker` (`shell/src/plugins/catalog.ts:307` declares `dependsOn: ['core.theme-service', 'nexus.activityBar']`) and indirectly the Appearance tab in `core.settings` (it reads `useThemeStore` directly). No `dependsOn` from `core.settings` despite the data-coupling.
- **Depends on:** the `com.nexus.theme` kernel plugin (Rust core) for the event payload and hydration data; `shell/src/stores/themeStore.ts` for state + DOM cascade.
- **What breaks if removed:** themes stop tracking kernel-side changes — the shell shows `shell.css` defaults forever, `nexus.themePicker` cannot apply a theme, and the Appearance settings tab still renders but its selections never reach the DOM. None of that blocks basic editing.

## Notes
- Subscribe failure is logged at WARN and swallowed (`index.ts:73-78`); the plugin still finishes activating. Robust by design for boot before kernel readiness.
- The comment at `index.ts:60-65` notes the payload does not carry resolved variables, so each `themeChanged` event triggers an extra hydration round-trip — acceptable because theme mutations are rare.
