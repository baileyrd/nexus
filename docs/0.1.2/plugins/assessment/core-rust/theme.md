# com.nexus.theme

- **Path:** `crates/nexus-theme/`
- **Tier:** Core Rust
- **Bootstrap order:** 17

## Architecture
- Entry point: `crates/nexus-theme/src/core_plugin.rs` (`ThemeCorePlugin`). Library modules: `variables`, `manifest`, `theme`, `snippet`, `resolver`, `api` (`ThemeEngine`), `layout`, `layout_manager`, `preset`, `watcher`.
- PRD-07. Pure-Rust theming engine: ~100 built-in CSS variable defaults, theme-package TOML manifests, CSS snippets, and a resolution cascade `defaults → theme → platform overrides → snippets → plugin overrides → final VariableMap`.
- Engine instance held behind `Mutex<ThemeEngine>` inside the plugin. Bootstrap registers via `ThemeCorePlugin::with_builtins(event_bus)` (`crates/nexus-bootstrap/src/plugins/theme.rs:32`) — only built-in light/dark themes; no forge-dir scan at registration. External themes/snippets are loaded by callers providing `themes_dir` / `snippets_dir` (`ThemeEngine::with_dirs`) — currently exercised by tests, not by bootstrap.
- Every mutating handler publishes `com.nexus.theme.changed` with the new `ThemeConfig` snapshot. Filter prefix: `com.nexus.theme.`.
- `LayoutManager` (`layout_manager.rs`) persists named layouts as `{layouts_dir}/{id}.json` — caller-owned directory.

## Persistence
- None at the plugin layer. `ThemeConfig` lives in-memory; the shell layer persists user selection via `shell-state.json` / `workspace.json` and replays it through `apply_config` on boot.
- Optional layout files (`{id}.json` under a caller-supplied dir) — not wired from bootstrap.

## Settings owned
- None at the `.forge/` level. Theme selection is part of `app.toml` (`AppConfig.theme`, see `docs/0.1.2/settings/forge-config.md` line 48) but that field is consumed by the shell, not by this plugin.
- Built-in CSS variable defaults are baked into the binary at `variables.rs`.

## External dependencies of note
- `notify`, `notify-debouncer-mini` for theme-file watching (`watcher.rs`).
- `serde`, `toml`, `ts-rs`. No native libs.

## Surface
Handlers (`IPC_HANDLERS`, `src/core_plugin.rs:88`):

| Id | Command | Notes |
|---:|---------|-------|
| 1 | `get_available_themes` | Built-in + discovered |
| 2 | `apply_theme` | Switch active theme; emits `changed` |
| 3 | `compute_variables` | Stateless cascade |
| 4 | `get_available_snippets` | List with enabled flag |
| 5 | `toggle_snippet` | Emits `changed` |
| 6 | `reorder_snippets` | Replace enabled order; emits `changed` |
| 7 | `get_theme_config` | Current snapshot |
| 8 | `set_mode` | Light/dark/system; emits `changed` |
| 9 | `apply_config` | Restore from persisted `ThemeConfig` |
| 10 | `set_plugin_overrides` | Merge plugin variable overrides |
| 11 | `reload` | Rescan themes/snippets dirs |

Publishes: `com.nexus.theme.changed` (`EVENT_CHANGED`).

## Necessity
- **Verdict:** Useful
- **Required for basic capabilities?** No — the shell will render with browser defaults if `compute_variables` never returns. But the experience is unrecognisable.
- **Depended on by:** shell `themeService` core plugin and `shell-nexus` `themePicker`; any plugin subscribing to `com.nexus.theme.changed` to sync its own palette.
- **Depends on:** kernel event bus only.
- **What breaks if removed:** active theme application, dark/light mode toggling, CSS variable cascade, snippet system, the entire shell's visual styling pipeline. Basic open/browse/edit/search/git logic would still run headless, but the desktop shell becomes effectively unusable without manual CSS.

## Notes
- Bootstrap registers with built-ins only — the `themes_dir` / `snippets_dir` constructor (`api.rs:146`) is unused on the live path; tests are the only callers. Worth tracking as a gap.
- `layout_manager` ships in this crate but is not exercised via any IPC handler in `IPC_HANDLERS` — layout persistence lives in the shell side today.
- `category` not set on the bootstrap manifest.
