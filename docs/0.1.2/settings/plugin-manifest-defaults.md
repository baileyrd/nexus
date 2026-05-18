# Plugin Manifest Defaults

> **As of:** 2026-05-17. Completes the hardcoded-settings audit by walking values **baked into plugin manifests** (backend `MANIFEST_TOML` strings + scaffold templates + shell `definePlugin({...})` calls). Companion to [`hardcoded-rust.md`](hardcoded-rust.md) (code constants) and [`hardcoded-shell.md`](hardcoded-shell.md) (shell component constants).

## What counts as "hardcoded" in a manifest

Three tiers of baked-in defaults, in increasing order of "hardcoded badness":

| Tier | Pattern | Verdict |
|------|---------|---------|
| **Documented** | `configuration.schema.<key>.default = X` | ✅ Already a user setting; the default is declared. No remediation. |
| **Overrideable** | `keybindings: [{ key: 'Ctrl+P', ... }]` — overrideable via the binding registry (`core.commandPalette`'s `bindStorage/setOverride`) | ⚠️ User can change at runtime, but the default ships in source. Promoting to `keybindings.<command>` settings schema key is the right next step. Many entries below already have a schema key — the gap is for the ones that don't. |
| **Hardcoded** | `priority: 95`, `position: 'left'`, fixed file-extension registrations, scaffold-baked WASM caps | ❌ No override path. To change you must edit the plugin source and rebuild. |

The hardcoded tier is what most needs remediation. The schema-default tier is included here only as a reference — search for what *can* be tuned today.

---

## Backend (Rust)

### `WasmConfig` defaults — `crates/nexus-plugins/src/manifest.rs:690-698`

Apply to every community WASM plugin that doesn't override them in its own `plugin.toml`.

| Field | Default | Helper | Verdict |
|-------|---------|--------|---------|
| `memory_mb` | `16` | `fn default_memory_mb` line 691 | per-plugin overridable via manifest; no system-wide override |
| `fuel` | `10_000_000` | `fn default_fuel` line 694 | per-plugin overridable |
| `max_execution_ms` | `5_000` | `fn default_max_execution_ms` line 697 | per-plugin overridable |

**Gap:** there's no system-wide knob to bound these — a malicious community plugin could set `fuel = 999_999_999_999` in its own manifest. Suggested remediation: add a `KernelConfig.wasm_caps: { max_memory_mb, max_fuel, max_execution_ms }` ceiling that the loader clamps against.

### `LifecycleConfig` default — `crates/nexus-plugins/src/manifest.rs:566-585`

`#[derive(Default)]` → every hook flag defaults to **false**.

| Field | Default |
|-------|---------|
| `on_load` / `on_init` / `on_start` / `on_stop` / `on_unload` / `on_enable` / `on_disable` / `on_settings_changed` | `false` |

Plugins that need a hook must opt-in in their manifest. OK as-is.

### `ActivationConfig` default — `crates/nexus-plugins/src/manifest.rs:545-552`

All three `Vec<String>` fields default to empty.

| Field | Default |
|-------|---------|
| `on_command` | `[]` |
| `on_content_type` | `[]` |
| `on_uri_scheme` | `[]` |

When all empty, plugin activates **eagerly at shell start** (`is_eager()` returns true at line 557-558). OK as-is, but the eager-by-default policy is itself worth flagging — a community plugin author who forgets to set activation triggers ships an always-on plugin.

### Plugin scaffold templates — `crates/nexus-plugins/src/scaffold.rs`

Whenever someone runs `nexus plugin scaffold ...`, these values ship in the new plugin's manifest.

**`MANIFEST_TOML_CORE` (lines 76-95):**

| Field | Default | Note |
|-------|---------|------|
| `[plugin].trust_level` | `"core"` | scaffold target marker |
| `[plugin].api_version` | `"1"` | sets minimum kernel API |
| `[wasm].module` | `"plugin.wasm"` | artifact filename |
| `[wasm].memory_mb` | `16` | matches `WasmConfig::default()` |
| `[wasm].fuel` | `0` | core plugins skip fuel metering |
| `[lifecycle].on_load / on_init / on_start / on_stop / on_unload` | all `true` | scaffold is more permissive than the struct default (all false) |

**`MANIFEST_TOML_COMMUNITY` (lines 97-119):**

| Field | Default | Note |
|-------|---------|------|
| `[plugin].trust_level` | `"community"` | scaffold target marker |
| `[plugin].api_version` | `"1"` | sets minimum kernel API |
| `[capabilities].required` | `["kv.read", "kv.write"]` | baked starter set — every new plugin gets KV by default |
| `[wasm].module` | `"plugin.wasm"` | artifact filename |
| `[wasm].memory_mb` | `16` | matches `WasmConfig::default()` |
| `[wasm].fuel` | `10_000_000` | matches `WasmConfig::default()` |
| `[lifecycle].on_load / on_init / on_start / on_stop / on_unload` | all `true` | same scaffold convention |

**`crates/nexus-plugins/templates/script/plugin.json`:**

| Field | Default | Note |
|-------|---------|------|
| `version` | `"0.1.0"` (line 3) | starter version |
| `main` | `"index.js"` (line 5) | entry point |
| `enabled` | `true` (line 6) | ships enabled |
| `apiVersion` | `1` (line 9) | shell-side API floor |
| `sandboxed` | `true` (line 10) | iframe-sandboxed by default |
| `capabilities` | `[]` (line 11) | empty by default |

**`crates/nexus-plugins/src/scaffold.rs:57`:**

```rust
const EXTENSION_API_VERSION: &str = "^1.0.0";
```

Pinned scaffold-wide; not configurable. Bump when releasing a new major.

**Cargo.toml scaffold (lines 61-74) for WASM plugins:**

| Field | Default |
|-------|---------|
| `edition` | `"2021"` |
| `crate-type` | `["cdylib"]` |
| `[profile.release].opt-level` | `"s"` |
| `[profile.release].lto` | `true` |

**`package.json` scaffold for script plugins:**

| Field | Default |
|-------|---------|
| `version` | `"0.1.0"` |
| `type` | `"module"` |
| `main` | `"index.js"` |
| `private` | `true` |
| `esbuild` | `"^0.24.0"` (devDep) |
| `typescript` | `"^5.5.0"` (devDep) |

**`tsconfig.json` scaffold:**

| Field | Default |
|-------|---------|
| `target` | `"ES2020"` |
| `module` | `"esnext"` |
| `lib` | `["ES2020", "DOM"]` |
| `strict` | `true` |
| `noEmit` | `true` |

---

## Shell — `definePlugin({...})` baked-in defaults

### Keybindings — overrideable via binding registry, but defaults are baked

`core.commandPalette` provides a binding-override UI; defaults below ship in each plugin's manifest. Promoting these to `keybindings.<command>` schema keys would close the gap fully.

| Plugin | Command | Default key | Already in schema? |
|--------|---------|-------------|---------------------|
| `core.commandPalette` | `workbench.action.showCommandPalette` | `Ctrl+Shift+P` / `Cmd+Shift+P` | no |
| `core.editorArea` | `editor.closeTab` | `Ctrl+W` / `Cmd+W` | no |
| `core.editorArea` | `editor.nextTab` | `Ctrl+Tab` | no |
| `core.editorArea` | `editor.previousTab` | `Ctrl+Shift+Tab` | no |
| `core.fileExplorer` | `fileExplorer.openFolder` | `Ctrl+K Ctrl+O` / `Cmd+K Cmd+O` | no |
| `core.panelArea` | `panel.toggle` | `Ctrl+J` / `Cmd+J` | no |
| `core.rightPanel` | `rightPanel.toggle` | `Ctrl+Alt+B` / `Cmd+Alt+B` | no |
| `core.settings` | `workbench.action.openSettings` | `Ctrl+,` / `Cmd+,` | no |
| `core.terminal` | `terminal.toggle` | `Ctrl+\`` | no |
| `core.zoom` | `core.zoom.in` | `Ctrl+=` / `Cmd+=` (and `Ctrl+Shift+=` / `Cmd+Shift+=`) | no |
| `core.zoom` | `core.zoom.out` | `Ctrl+-` / `Cmd+-` | no |
| `core.zoom` | `core.zoom.reset` | `Ctrl+0` / `Cmd+0` | no |
| `nexus.ai` | `nexus.ai.focus` | `Ctrl+Alt+A` / `Cmd+Alt+A` | no |
| `nexus.ai` | `nexus.ai.cmdI.open` | `Ctrl+I` / `Cmd+I` | no |
| `nexus.bases` | `nexus.bases.undo` / `.redo` / `.cut` / `.copy` / `.paste` | standard combos with `bases.focused` context | no |
| `nexus.canvas` | `canvas.undo` / `.redo` / `.delete` / `.fit` / `.fitSelection` / `.toggleHelp` / `.closeHelp` | Mod+Z, Mod+Shift+Z / Y, Del/BS, F, Shift+F, Shift+/, Esc | no |
| `nexus.commandPalette` | `nexus.commandPalette.open` | `Ctrl+Shift+P` / `Cmd+Shift+P` (also `Ctrl+P` / `Cmd+P`) | no |
| `nexus.files` | `nexus.files.delete` / `.rename` | `Delete` / `F2` | no |
| `nexus.pluginsMgmt` | `nexus.pluginsMgmt.open` | `Ctrl+Shift+X` / `Cmd+Shift+X` | no |
| `nexus.processes` | `nexus.processes.show` | `Ctrl+Shift+Y` / `Cmd+Shift+Y` | no |
| `nexus.recall` | `nexus.recall.open` | `Mod-Shift-R` (default — `recall.hotkey` schema key is configurable) | **yes** (`recall.hotkey`) |
| `nexus.rightPanel` | `nexus.rightPanel.toggle` | `Ctrl+Alt+R` / `Cmd+Alt+R` | no |
| `nexus.search` | `nexus.search.focus` | `Ctrl+Shift+F` / `Cmd+Shift+F` | no |
| `nexus.terminal` | `nexus.terminal.toggle` | `Ctrl+\`` / `Cmd+\`` (also `terminal.send` `Ctrl+Shift+G`) | no |
| `nexus.themePicker` | `nexus.themePicker.open` | `Ctrl+Shift+T` / `Cmd+Shift+T` | no |
| `nexus.workspace` | `nexus.workspace.open` | `Ctrl+O` (when `nexus.workspace.hasRoot`) | no |

Plus every overlay's `Escape`-to-close binding (~10 plugins).

**Suggested remediation:** introduce a `keybindings.*` settings cascade. Plugins declare `defaultKey` in the manifest, the override resolves at `core.commandPalette` binding registry, persistence in `<forge>/.forge/app.toml`. Pattern: `keybindings.workbench.action.showCommandPalette: "ctrl+shift+p"`.

### Priorities (panel + activity-bar ordering) — fully hardcoded

No override mechanism — to reorder you must edit the plugin source.

**Activity-bar priorities** (plugin → priority, lower = higher in the rail):

| Plugin | Activity-bar `priority` |
|--------|------------------------:|
| `core.activityBar` (seeds) | `20`–`80` (search → ai) |
| `nexus.gitPanel` | `25` |
| `nexus.memory` | `25` |
| `nexus.collab` | `27` |
| `nexus.skills` | `40` |
| `nexus.osArchitecture` | `45` |
| `nexus.templates` | `45` |
| `nexus.ai` | `50` |
| `nexus.activity` (timeline) | `55` |
| `nexus.diagnostics` | `56` |
| `nexus.notificationsInbox` | `57` |
| `nexus.dreamCycle` | `58` |
| `nexus.processes` | `60` |
| `nexus.viewBuilder` | `60` |
| `nexus.agent` | `70` |
| `core.settings` (Help) | `99` |
| `core.settings` (Settings) | `100` (bottom) |

**View / overlay priorities:**

| Plugin | View | priority |
|--------|------|---------:|
| `core.commandPalette` | CommandPaletteView | `100` |
| `core.capabilityPrompt` | CapabilityModalView | `95` |
| `nexus.themePicker` | ThemePickerModal | `95` |
| `nexus.crdtConflict` | ConflictModal | `90` |
| `nexus.pick` | PickModal | `90` |
| `nexus.prompt` | PromptOverlay | `90` |
| `core.settings` | SettingsPanelView | `90` |
| `nexus.bases` | NewBaseDialog | `70` |
| `nexus.graph` | GraphView | `30` (right-panel tab) |
| `nexus.ai` | CmdIOverlay | `20` |
| `nexus.recall` | RecallOverlay | `26` |
| `core.capabilityPrompt` | CapabilityBannerView | `10` |
| `nexus.backlinks` / `nexus.outline` | right-panel tabs | `20` / `10` |

**Suggested remediation:** allow per-forge ordering overrides via a new schema key `activityBar.order: { "<plugin-id>": <int> }`. Today the cascade is plugin-source-only.

### Schema defaults (declared in `configuration.schema`)

These ARE user settings and are tunable via the settings UI. Listed here so the inventory is complete.

**Core plugins:**

| Plugin | Schema key | Default | Type |
|--------|------------|---------|------|
| `core.commandPalette` | `commandPalette.maxResultsLimit` | `50` | number |
| `core.fileExplorer` | `fileExplorer.showHidden` | `false` | boolean |
| `core.fileExplorer` | `fileExplorer.sortOrder` | `"name"` | enum (name\|modified\|type) |
| `core.fileExplorer` | `ui.fileCreationNotificationMs` | `2000` | number |
| `core.notificationService` | `ui.notificationDurationMs` | `4000` | number |
| `core.terminal` | `terminal.fontSize` | `13` | number |
| `core.terminal` | `terminal.fontFamily` | `"'Cascadia Code', 'Consolas', monospace"` | string |
| `core.zoom` | `ui.zoom` / `zoomStep` / `zoomMin` / `zoomMax` / `zoomDefault` | `1.0 / 0.1 / 0.5 / 3.0 / 1.0` | number |

**Nexus plugins:**

| Plugin | Schema key | Default | Type |
|--------|------------|---------|------|
| `nexus.ai` | `ai.provider` / `model` / `apiKey` / `baseUrl` | `""` | string / password |
| `nexus.ai` | `ai.embedProvider` / `embedModel` / `embedApiKey` / `embedBaseUrl` | `""` | same |
| `nexus.ai` | `ui.copiedNotificationMs` | `1200` | number |
| `nexus.ai` | `ai.ghost.enabled` / `debounceMs` / `minChars` / `contextChars` / `maxTokens` | `true / 350 / 8 / 2000 / 64` | mixed |
| `nexus.ai` | `ai.marginSuggest.enabled` / `idleMs` / `minDocChars` / `maxDocChars` | `false / 5000 / 200 / 8000` | mixed |
| `nexus.audio` | `nexus.audio.useWebSpeech` / `defaultLanguage` / `defaultVoice` / `defaultRate` | `true / "" / "" / 1.0` | mixed |
| `nexus.canvas` | `canvas.exportMarginUnits` / `exportMarginPx` / `maxExportEdge` | `48 / 48 / 8192` | number |
| `nexus.canvas` | `canvas.colorSwatches` | `['#ef4444', '#f59e0b', '#eab308', '#22c55e', '#3b82f6', '#8b5cf6', '#ec4899']` | string[] |
| `nexus.commandPalette` | `commandPalette.maxResultsLimit` | `50` | number |
| `nexus.linkSuggest` | `ai.linkSuggest.enabled` / `debounceMs` / `minChars` / `maxChars` / `scoreGate` | `true / 600 / 4 / 80 / 0.55` | mixed |
| `nexus.memory` | `recall.hotkey` / `recall.inboxPath` | varies (constants) | string |
| `nexus.recall` | `recall.hotkey` | `"Mod-Shift-r"` | string |
| `nexus.search` | `search.maxResultsLimit` | `50` | number |
| `nexus.terminal` | `ui.commandSaveNotificationMs` / `commandCopiedNotificationMs` / `autoRestartDelayMs` / `externalPriority` | `3000 / 1800 / 2000 / ""` | mixed |

**Verdict:** ✅ These resolve the corresponding rows in [`hardcoded-shell.md`](hardcoded-shell.md) — they're declared, defaulted, and tunable. Several entries in the old shell audit are now redundant because the schema landed; those were already marked "✓ done" in the existing audit.

### File / view-type registrations — hardcoded

| Plugin | Type | Pattern | Note |
|--------|------|---------|------|
| `nexus.bases` | file extension | `.bases`, `.base` | hardcoded list — to add a new bases-shape extension, edit the plugin |
| `nexus.canvas` | file extension | `.canvas` | hardcoded |
| `nexus.editor` | viewType | `markdown`, empty, `diff` | hardcoded |

**Suggested remediation:** allow per-plugin extension lists via `<plugin>.fileExtensions: string[]` schema.

### Categories — labels only, not settings

Core: `View`, `Editor`, `File`, `Preferences`, `Help`. Nexus: `AI`, `Agent`, `Bases`, `Canvas`, `Collaboration`, `Databases`, `Diagnostics`, `Dream Cycle`, `Files`, `Git`, `Memory`, `MCP`, `Notifications`, `Search`, `Terminal`, `Workflows`, `Workspace`.

These group commands in the palette + settings panel. Acceptable as labels.

---

## Summary

| Category | Items | Verdict |
|----------|------:|---------|
| **WasmConfig defaults** (manifest.rs) | 3 | per-plugin overridable; system-wide ceiling missing |
| **Scaffold-baked values** (scaffold.rs + templates) | ~22 | ship with every new plugin; mostly fine but `[capabilities].required = ["kv.read","kv.write"]` is a notable opinionated default |
| **Shell keybindings** (manifest-baked) | ~46 | overrideable at runtime via binding registry; defaults are hardcoded |
| **Shell priorities** (activity-bar + overlays) | ~38 | **fully hardcoded** — no override path |
| **Shell schema defaults** | ~50 | ✅ already settings; included as reference |
| **File / view-type registrations** | 4 | hardcoded; minor remediation candidate |

## What's still uncovered

The audit now spans:
- ✅ Rust code constants ([`hardcoded-rust.md`](hardcoded-rust.md))
- ✅ Shell component constants ([`hardcoded-shell.md`](hardcoded-shell.md))
- ✅ Backend plugin manifest defaults (this file, §Backend)
- ✅ Scaffold template defaults (this file, §Backend)
- ✅ Shell plugin manifest defaults (this file, §Shell)

Things outside the audit's scope:
- **CSS variable defaults** in the theme registry — these are visual defaults, ~547 tokens; documented at the per-theme level via the `nexus-manuscript` etc. theme files. Treat as visual design, not settings.
- **Bundled skill / template content** (`skills/built-in/*.skill.md`) — content, not settings.
- **Translation / i18n strings** — there's no i18n layer at v0.1.2; English-only.
- **Help text / tooltip copy** — copy, not settings.

If any of these become a settings concern (e.g. user-chosen language), they'd warrant their own audit pass.
