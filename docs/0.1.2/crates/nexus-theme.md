# nexus-theme

> Kind: lib · IPC plugin id: com.nexus.theme · CorePlugin: yes · Has settings: no dedicated `.forge/*.toml` (theme selection persisted client-side via `ThemeConfig`) · As of: 2026-05-25

## Overview

`nexus-theme` is the pure-Rust theming engine for Nexus (PRD-07). Its core deliverable is a CSS-variable cascade: a baseline of ~430 built-in `--nx-*` variable defaults (light-mode palette) is merged, in strict precedence order, with a selected theme's `[variables]`, that theme's platform-specific overrides, user CSS snippets, and last-wins plugin overrides, producing a final flat `VariableMap`. The shell injects that map into the document root so every visible surface (chrome, editor, prose, graph, syntax) is styled from one variable namespace. The crate is deliberately transport-agnostic — it has no `tauri` or `tokio` dependency; frontends reach it only through kernel IPC.

Theme packages and snippets are file-as-truth. A theme is a directory containing a `NEXUS.toml` manifest (metadata + `[variables]` + `[platforms.*]` + `[typography]` + `[tags]`); a snippet is a `.css` file whose leading block comment carries `Name`/`Description`/`Mode`/`Scope` header fields and whose `:root { --nx-*: … }` declarations feed the cascade. Eleven themes are bundled into the binary via `include_str!` (light, dark, forge, solarized dark/light, nord, dracula, tomorrow-night, ember dark/light, manuscript) and are always present; user-installed themes/snippets are discovered by scanning configured directories. A `notify`-backed debounced file watcher (`ThemeWatcher`) emits reload events when a manifest or snippet changes on disk, so the shell can re-resolve live.

The crate is registered by `nexus-bootstrap` as the core plugin `com.nexus.theme` (`crates/nexus-bootstrap/src/plugins/theme.rs`), wrapping a `Mutex<ThemeEngine>` behind 11 IPC handlers and publishing a `com.nexus.theme.changed` event on every state mutation. This was created specifically to satisfy the microkernel invariant: before it existed, the Tauri shell instantiated `ThemeEngine` directly and locked it from `#[tauri::command]` bodies, so other plugins had no way to react to theme changes. Now any plugin can `ipc_call("com.nexus.theme", …)` and subscribe with `EventFilter::CustomPrefix("com.nexus.theme.")`.

In addition to theming, the crate hosts a substantial **workspace-layout subsystem** (PRD-05): a recursive split/leaf pane tree (`WorkspaceLayout`), JSON persistence, a filesystem-backed `LayoutManager` of named layouts, and a `PresetRegistry` of six embedded TOML layout presets (writing/reviewing/coding/obsidian/vibe/dev). Note this entire layout half is a public library API only — as of this writing nothing in `nexus-bootstrap`, the Tauri shell, or any other crate consumes `WorkspaceLayout`, `LayoutManager`, or `PresetRegistry`, and the `ThemeCorePlugin` exposes no layout/preset IPC handlers. It is built and fully unit-tested but unwired. See "Internals" for the gap.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-kernel` (only for `EventBus` / `EventFilter` in the core plugin), `nexus-plugins` (`CorePlugin` trait, `PluginError`, `define_dispatch_helpers!`). Both leaf-ward of subsystems, so the microkernel layering holds — the kernel does not depend on this crate.
- **Notable external deps:** `notify` + `notify-debouncer-mini` (file watcher), `toml` (manifest/preset parsing), `serde`/`serde_json` (manifest + layout JSON), `ts-rs` (TypeScript binding generation, unconditional on most types), `schemars` (optional, behind the `ts-export` feature, for JSON-Schema IPC bindings), `uuid` (pane/tab/workspace ids, v7), `chrono` (layout timestamps), `thiserror`, `tracing`. Dev-only: `tempfile`.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin). No other crate imports it. The Tauri shell reaches the engine only through IPC, never via a direct dep.

## Public API surface

| Module | Item(s) | Purpose |
|--------|---------|---------|
| `lib` | `Platform` (`Macos`/`Windows`/`Linux`, `current()`, `as_key()`), `Result<T>` alias | Build-target platform tag used for platform overrides. |
| `variables` | `VariableMap` (= `BTreeMap<String,String>`), `DEFAULT_VARIABLES`, `default_variables()`, `VARIABLE_PREFIX` (`--nx-`), `validate_variable_name()`, `substitute()` | CSS-variable registry, the ~430-entry default palette, `--nx-` prefix validation, and recursive `var(...)` substitution with cycle detection. |
| `manifest` | `ThemeManifest`, `ThemeHeader`, `ThemeCategory` (light/dark/sepia/high-contrast/custom), `TypographyBlock`, `PlatformOverrides` (`for_platform()`), `TagBlock` | TOML schema for a theme package's `NEXUS.toml`. |
| `theme` | `Theme` (`load`/`discover`/`builtins`/`metadata`), `ThemeMode` (light/dark/system), `ThemeMetadata`, `BUILTIN_THEMES` table + per-theme `*_ID`/`*_TOML` constants, `MANIFEST_FILENAME` (`NEXUS.toml`) | Theme loader, directory scan, and the 11 bundled built-in themes. |
| `snippet` | `CssSnippet` (`parse`/`load`/`discover`/`applies_to`), `SnippetMode` (all/light/dark), `SnippetScope` (global / per-surface selector) | CSS-snippet header parser + `:root` variable extraction + directory scan. |
| `resolver` | `resolve()`, `ResolverInput`, `ResolvedTheme` | The 5-stage cascade producing a final `VariableMap` plus provenance (theme id, mode, platform, applied snippet ids). |
| `api` | `ThemeEngine` (`new`/`with_dirs`/`reload`/`apply_theme`/`set_mode`/`toggle_snippet`/`reorder_snippets`/`compute_variables`/`config`/`apply_config`/`set_plugin_overrides`/`compute`), `AppliedTheme`, `SnippetMetadata`, `ThemeConfig`, `default_theme_id()`, `dark_theme_id()` | Stateful, transport-agnostic engine owning discovered themes/snippets + current selection; the command-shaped functions the core plugin wraps. |
| `core_plugin` | `ThemeCorePlugin` (`new`/`with_builtins`), `PLUGIN_ID`, `EVENT_CHANGED`, `IPC_HANDLERS`, `HANDLER_*` ids, arg/reply DTOs (`ApplyThemeArgs`, `ComputeVariablesArgs`, `ToggleSnippetArgs`, `ReorderSnippetsArgs`, `SetModeArgs`, `ApplyConfigArgs`, `SetPluginOverridesArgs`, `Ack`) | `CorePlugin` impl: dispatches the 11 handlers over a `Mutex<ThemeEngine>` and publishes `changed` events. |
| `watcher` | `ThemeWatcher` (`start`/`try_recv`/`recv_timeout`/`drain`), `ThemeReloadEvent` (Theme/Snippet), `DEFAULT_DEBOUNCE_MS` (500) | Debounced `notify` watcher for theme + snippet directories. |
| `layout` | `WorkspaceLayout`, `LayoutNode` (Split/Leaf), `PaneId`, `TabId`, `Tab`, `Direction`, `Surface`, `SidePanel`/`SidePanelSide`/`SidePanelFooter`, `BottomPanel`, `Panel`/`PanelToolbarItem`, `RibbonItem`/`RibbonAction`, `StatusBarItem`, `FooterAction`, `LayoutMetadata`, `PaneNode` alias | Recursive pane-tree data model + mutation API (split/close/resize/add-tab/focus) + JSON persistence. **Not wired to any IPC handler.** |
| `layout_manager` | `LayoutManager` (`new`/`save`/`save_as`/`load`/`delete`/`list`/`dir`), `SavedLayoutInfo` | Filesystem store of named `WorkspaceLayout`s as `{id}.json`. **Unwired.** |
| `preset` | `LayoutPreset` (`instantiate`), `PresetRegistry` (`with_core_presets`/`scan_user_dir`/`register_plugin_preset`/`get`/`list`), `PresetInfo`, `PresetSourceKind` (embedded/user/plugin), `parse_preset()` | Six embedded `*.layout.toml` presets + user/plugin preset discovery. **Unwired.** |
| `error` | `ThemeError` | Single error enum for the whole crate (theming + layout + preset). |

## IPC handlers

All registered under plugin id `com.nexus.theme`. Handler ids are stable and append-only; the canonical `(name, id)` table is `core_plugin::IPC_HANDLERS`. None declare or check a capability (see Capabilities).

| Command | id | Args | Returns | Description |
|---------|----|------|---------|-------------|
| `get_available_themes` | 1 | `{}` | `Vec<ThemeMetadata>` | List every built-in + discovered theme (alphabetical). |
| `apply_theme` | 2 | `ApplyThemeArgs { id }` | `AppliedTheme { id, name, variables }` | Switch active theme, recompute cascade; emits `changed`. Errors if id unknown. |
| `compute_variables` | 3 | `ComputeVariablesArgs { theme_id, enabled_snippets }` | `VariableMap` | Stateless cascade compute — does not mutate engine state. Errors if any id unknown. |
| `get_available_snippets` | 4 | `{}` | `Vec<SnippetMetadata>` | List snippets with their per-snippet `enabled` flag. |
| `toggle_snippet` | 5 | `ToggleSnippetArgs { id }` | `Vec<String>` (new ordered enabled ids) | Toggle a snippet on/off; emits `changed`. Errors if id unknown. |
| `reorder_snippets` | 6 | `ReorderSnippetsArgs { ids }` | `{}` (empty object) | Replace the ordered enabled-snippet list; emits `changed`. Errors if any id unknown. |
| `get_theme_config` | 7 | `{}` | `ThemeConfig { theme_id, mode, enabled_snippets }` | Current selection snapshot. |
| `set_mode` | 8 | `SetModeArgs { mode }` | `AppliedTheme` | Set light/dark/system and recompute; emits `changed`. |
| `apply_config` | 9 | `ApplyConfigArgs { config }` | `Ack { ok: true }` | Restore from a persisted `ThemeConfig`; unknown ids silently dropped; emits `changed`. |
| `set_plugin_overrides` | 10 | `SetPluginOverridesArgs { overrides }` | `Ack { ok: true }` | Merge a `VariableMap` on top of the cascade (last-wins); emits `changed`. |
| `reload` | 11 | `{}` | `Ack { ok: true }` | Rescan the configured theme/snippet directories (built-ins preserved); emits `changed`. |

Cross-check: `docs/0.1.2/ipc-handlers.md` lists `com.nexus.theme` with 11 handlers grouped as read/pure (`get_available_themes`, `get_available_snippets`, `get_theme_config`, `compute_variables`), settings-mutation (`apply_theme`, `apply_config`, `set_mode`, `toggle_snippet`, `reorder_snippets`, `set_plugin_overrides`), and `reload`. This matches the source exactly.

## Capabilities

**None.** No handler declares or checks a capability, and the crate does not reference the kernel capability system. The only kernel touchpoint is the `EventBus` used to publish `changed` events. (The `core_manifest_with_ipc(... LifecycleFlags::NONE ...)` registration in bootstrap grants no special capabilities either.)

## Settings / Config

There is **no dedicated `.forge/theme.toml`** owned by this crate — it is absent from `docs/0.1.2/settings/README.md`. Theme selection is carried as a `ThemeConfig` value (`theme_id`, `mode`, `enabled_snippets`) round-tripped through `get_theme_config` / `apply_config`; persistence is the caller's responsibility (the engine doc comment references a client-side `theme-config.json`, but the crate itself never writes it). `ThemeEngine::with_dirs(themes_dir, snippets_dir)` takes the discovery directories, but the registered core plugin uses `with_builtins(...)` — i.e. **built-ins only, no on-disk discovery directory is configured at bootstrap.** Watching and discovery are available APIs but not yet wired into the registered plugin.

**Theme manifest config (`ThemeManifest`):** `[theme]` (name, version, author, description, plus defaulted `license`=`MIT`, `nexus_min_version`=`0.1.0`, `nexus_max_version`=`*`, `display_name`, `icon`, `category`=`light`, `supports`, `platform_specific`); `[variables]` (`--nx-*` → value); optional `[typography]` (sans/mono/serif font stacks + `font_imports`); `[platforms.{macos,windows,linux}]` override maps; `[dependencies]`; `[tags]` (keywords, color_temperature, contrast_level, use_case); `[version_history]`.

**The ~430 built-in CSS variable defaults** (`variables::DEFAULT_VARIABLES`) — light-mode baseline, all `--nx-`-prefixed, organized into categories (a representative, not exhaustive, list):

- Base palette + nine-step tonal ramps (primary/secondary/success/warning/error/info, neutral 25–950)
- Surfaces / backgrounds, borders (+ widths), text (+ on-color, link, code, status), icons
- Typography: families, size scale (xxs–6xl), weight scale (thin–black), line-heights, letter-spacing, h1–h6 + subtitle/caption/overline
- Spacing scale (3xs–5xl), radii (2xs–full/pill), z-index ladder (base–max), motion (durations + easings)
- Effects: shadows (xs–2xl + semantic: focus/elevated/dialog/dropdown/toast), blurs
- Component groups: buttons (primary/secondary/danger/ghost/link), inputs, modals, tooltips, toasts, context menu, status bar, tab bar, ribbon, sidebar, scrollbar, selection, links, code blocks, inspector, forge-meta, file tree
- Editor + extended editor (selection, active-line, find-match, gutter, indent guides, brackets) and syntax highlighting (keyword/string/comment/number/function + extended type/class/operator/tag/attr/regex/markdown styles)
- Graph & canvas (nodes/edges/grid/minimap), prose (rendered markdown), callouts, form validation
- Density presets (cozy/compact/spacious row heights, UI/body sizes, chrome dimensions) and editor-canvas variables (`--nx-editor-font-family`, content max-width/padding)

A unit test (`defaults_cover_prd_minimum`) asserts the floor is ≥ 400 entries; another guards against duplicate keys.

**Where files persist on disk:** themes are directories with `NEXUS.toml`; snippets are `.css` files; named layouts are `{id}.json` (via `LayoutManager`); user layout presets are `*.layout.toml`. The doc comment in `preset.rs` references `<forge>/.nexus/layouts/` for user presets, but no concrete `.forge/` path is wired by bootstrap for any of these (the registered plugin is built-ins-only).

## Events

- **Published:** `com.nexus.theme.changed` (`EVENT_CHANGED`) — emitted by `ThemeCorePlugin` on every mutating handler (`apply_theme`, `toggle_snippet`, `reorder_snippets`, `set_mode`, `apply_config`, `set_plugin_overrides`, `reload`). Payload is the post-mutation `ThemeConfig` snapshot (`theme_id`, `mode`, `enabled_snippets`). Published via `EventBus::publish_plugin`; dropped silently if the plugin was constructed without an `EventBus`.
- **Subscribed:** none. (Consumers subscribe to `changed` externally via `EventFilter::CustomPrefix("com.nexus.theme.")`.)
- `ThemeWatcher::ThemeReloadEvent` (Theme / Snippet) is a separate `mpsc`-channel signal, **not** an `EventBus` event, and the watcher is not started by the registered plugin.

## Internals & notable implementation details

**Cascade precedence** (`resolver::resolve`, lower → higher): 1. base defaults → 2. theme `[variables]` → 3. theme `[platforms.<current>]` → 4. enabled snippets (in caller-supplied order, filtered by mode) → 5. plugin overrides. `var(--nx-foo)` references are **left intact** in the resolved map so the browser's CSS engine resolves them at render time; callers wanting a flat map run each value through `variables::substitute`.

**Variable substitution** is a hand-rolled scanner over `var(...)` with balanced-paren matching. Unknown variables are preserved verbatim (so any CSS fallback survives); recursion is bounded at `MAX_SUBSTITUTION_DEPTH = 16`, beyond which it returns `ThemeError::CircularReference`.

**Snippet application** is ordered by the user-supplied enabled list and filtered by mode: `ThemeMode::Dark` selects `SnippetMode::Dark`; `Light` and `System` both map to `SnippetMode::Light` (the resolver does not itself resolve `System` to a concrete mode — the caller must). `SnippetMode::All` snippets always apply. Snippet variables are extracted only from top-level `:root { … }` blocks (`--nx-` declarations); the full body is preserved separately for verbatim injection so arbitrary selectors still work.

**File watcher:** `ThemeWatcher::start` builds a `notify-debouncer-mini` debouncer (default 500 ms), watches the themes dir recursively and the snippets dir non-recursively, and spawns a thread that classifies each debounced path — theme dir + filename `NEXUS.toml` → `Theme` event (id = parent dir name); snippets dir + `.css` extension → `Snippet` event (id = file stem); everything else ignored. Missing directories are tolerated (watcher starts, emits nothing). Dropping the `ThemeWatcher` stops the OS watcher via its retained `_debouncer` field.

**schemars / ts-rs:** Most public types derive `TS` unconditionally and emit TypeScript bindings into `packages/nexus-extension-api/src/generated/ipc/`. The IPC arg/reply types are additionally gated behind the `ts-export` feature (`dep:schemars`), which derives `JsonSchema` and triggers ts-rs's `export_to` write side-effect — off by default so normal builds don't pull schemars or write files. Run with `cargo test -p nexus-bootstrap --test ipc_schema_emit --features ts-export`.

**Built-in theme set:** 11 themes, all loaded from `BUILTIN_THEMES`. Default startup theme is `nexus-light` (`BUILTIN_LIGHT_ID`); `nexus-dark` is the dark counterpart. `nexus-forge` is categorized Dark. A `builtins()` call panics if a bundled TOML fails to parse (caught by the `builtins_parse` test).

**Layout subsystem invariants** (`layout.rs`, currently unused at runtime): splits always keep ≥ 2 children (a 1-child split collapses to its child on removal), `children.len() == sizes.len()`, sizes renormalize to ~1.0 after removal, the tree never goes empty (closing the last leaf yields a fresh empty leaf), and `focused_pane_id`/`active_tab_id` always reference live nodes. Serialization is camelCase JSON with a `{"type": "split"|"leaf"}` discriminator. Pane/tab/workspace ids are UUID v7. Mutations stamp `metadata.last_modified` via `touch()`.

**Known gap / wiring status:** the layout + preset + layout-manager modules (≈ half the crate by line count, fully unit-tested) have **no external consumers** anywhere in the workspace and **no IPC handlers** in `ThemeCorePlugin` — they are a built-but-unwired public API. The theming half (variables/manifest/theme/snippet/resolver/api/core_plugin) is the part actually reachable at runtime.

**On-disk discovery + hot reload (C87 — fixed):** bootstrap now registers via `ThemeCorePlugin::with_dirs(~/.nexus/themes, ~/.nexus/snippets, event_bus)` with `on_start`/`on_stop` enabled (`crates/nexus-bootstrap/src/plugins/theme.rs`), so user-installed theme packages and CSS snippets under those two directories are discovered at boot and a `ThemeWatcher` background thread live-reloads on change, publishing `com.nexus.theme.changed`. Falls back to built-ins-only (logged, not a boot failure — `or_lifecycle_skip`) if the initial scan errors or the watcher fails to start.

## Tests

No `tests/` directory — all coverage is inline `#[cfg(test)]` modules. Summary by module:

- `variables`: defaults well-formed + all `--nx-`-prefixed; ≥ 400 defaults floor; no duplicate keys; defaults resolve without cycles; `substitute` for known/nested/unknown refs, cycle detection, plain text; name-prefix validation.
- `manifest`: full + minimal manifest parse (defaults applied), platform-override lookup, serialization round-trip.
- `theme`: all built-ins parse with expected ids/categories; directory discovery; missing-dir tolerated; missing-manifest error; metadata round-trip.
- `snippet`: sample parse (header fields + `:root` vars + body), missing required fields/header errors, per-surface scope, `applies_to` mode filtering, discovery (skips non-`.css`, empty for missing dir).
- `resolver`: theme-overrides-defaults, platform-overrides-win, snippets-override-platform/theme, plugin-overrides-win-last, snippet-mode-filtering, provenance recorded.
- `api` (`ThemeEngine`): built-ins present, apply switches current + unknown errors, snippet toggle/compute (stateless) , config restore + unknown-id drop, reorder validation, plugin-override wins.
- `core_plugin`: dispatch lists built-ins, `apply_theme`/`set_mode` emit `changed` with correct payload, unknown handler id errors, config reflects state after apply, reorder unknown-id errors.
- `watcher`: starts on missing dirs, drain-empty, `classify` for theme-manifest / non-manifest / snippet / unrelated; live snippet + theme-manifest change detection (soft assertions — FS watchers are flaky under WSL2/network mounts, so a missed event downgrades to a printed note rather than a failure).
- `layout`: default single-focused-leaf, JSON round-trip + camelCase/type discriminator, split/close/collapse/nested-remove, tab add/close/focus, split-size validation, focus-pane/tab, panel mutations, file save/load, size renormalization, recursive find helpers.
- `layout_manager`: creates missing dir, save/load round-trip, list summaries, delete (+ missing-is-ok), skips non-JSON/malformed, `save_as` assigns fresh id+name.
- `preset`: all six core presets parse + instantiate, list sorted, unknown errors, fresh-id per instantiation, user-dir scan picks up `*.layout.toml`, missing-dir-is-ok.
