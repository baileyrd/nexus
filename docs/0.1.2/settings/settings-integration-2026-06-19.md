# Settings Integration Audit — 2026-06-19

> **As of:** 2026-06-19. Answers a specific question: are each subsystem's
> settings *integrated* into one nexus Settings surface (terminal used as the
> worked example)? Verdicts grounded in direct source reads. Companion to the
> [`README.md`](README.md) config-surface index and the trackers
> [`hardcoded-rust.md`](hardcoded-rust.md) / [`hardcoded-shell.md`](hardcoded-shell.md).

## Headline

There is **no single unified settings model**. There is a unified settings
**panel** that renders only what each plugin *opts into*, sitting on top of
**three parallel settings worlds**. Coverage is therefore **partial and uneven** —
some subsystems are well surfaced, many are TOML-only, and some values are
hardcoded with no override. The terminal is a representative case: peripheral
knobs were in the UI while the two settings users most expect (font size,
scrollback) were hardcoded — now fixed (see [§4](#4-terminal-worked-example)).

## 1. The three settings worlds

| World | Where it lives | Persistence | Surfaced in the Settings panel? |
|-------|----------------|-------------|--------------------------------|
| **(A) Shell config schemas** | A plugin declares `contributes.configuration.schema` in its shell manifest; collected by `shell/src/registry/ConfigurationRegistry.ts` and rendered generically by `shell/src/plugins/core/settings/SettingsPanelView.tsx` (`SettingsSection`/`SettingsField`) | flat `[settings]` table in `<forge>/.forge/app.toml` via `com.nexus.storage::settings_write` (`crates/nexus-storage/src/handlers/config.rs`); shell mirror in `shell/src/stores/configStore.ts` | **Yes** — but only if the plugin opts in |
| **(B) Backend service TOML** | per-service `Config` structs → `ai.toml`, `mcp.toml`, `lsp.toml`, `dap.toml`, `sandbox.toml`, `notifications.toml`, `config.toml`, kernel `.nexus/config.toml` | the owning crate reads the file at startup; a few are live-updatable via IPC (e.g. `com.nexus.ai::set_config`) | **No** — the panel has no mechanism to discover or enumerate these |
| **(C) Per-plugin `settings.json`** | a plugin's JSON Schema registered via `SettingsManager::register_schema` (`crates/nexus-plugins/src/settings.rs:42`) | `<plugin_dir>/settings.json`, validated on write | **No** — only the owning plugin reads it |

The "unified" experience is World (A): a **plugin opt-in registry**, not
auto-discovery of backend configs. Worlds (B) and (C) are parallel and largely
invisible to the panel. The panel also carries hardcoded built-in pages
(General/Editor/Appearance/Hotkeys/…) and ~10 "coming soon" Obsidian-parity stub
pages (`SettingsStubPages.tsx`).

## 2. Integration matrix (by subsystem)

Legend: **UI** = a setting reaches the Settings panel via a schema (World A);
**TOML** = configured by a backend `Config` file (World B); **Hard** = hardcoded
constants with no override.

| Subsystem | UI | TOML | Coverage / notes |
|-----------|:--:|:----:|------------------|
| **AI** | ✓ | ✓ (`ai.toml`) | Best-integrated: shell knobs (ghost/margin) via schema **and** provider/model/key pushed to `ai.toml` via `com.nexus.ai::set_config` (the `aiSettings` plugin). |
| **Terminal** | ◑ | ✗ (no `Config` struct) | font size + scrollback now schema-backed (this change); notifications/auto-restart/external-emulator in UI; backend drainer/memory timings still `Hard`. |
| **Audio / Search / Memory / Recall / CommandPalette / LinkSuggest / Enrich** | ✓ | — | a handful of schema fields each; remainder hardcoded constants. |
| **Notifications** | ◑ | ✓ (`notifications.toml`) | channel routing partly in UI; transports/routing largely `notifications.toml`. |
| **MCP** | ✗ | ✓ (`mcp.toml`) | server registry is TOML-only; not surfaced. |
| **LSP / DAP** | ✗ | ✓ (`lsp.toml` / `dap.toml`) | server/adapter specs TOML-only. |
| **Sandbox / Security** | ✗ | ✓ (`sandbox.toml`) | policy/downloads/bundled-shell TOML-only; shell sandbox-orchestrator timeouts are `Hard`. |
| **Kernel** | ✗ | ✓ (`.nexus/config.toml`) | event-bus capacity, plugin caps, TLS, signatures — TOML-only. |
| **Editor / Storage / Git / Collab / Workflow / Database** | ✗/◑ | ◑ (`app.toml`/`config.toml`) | mostly internal config + constants; few user-facing UI knobs. |
| **LinkPreview** | ✗ | ✗ | no config surface at all (see [`hardcoded-audit-2026-06-19.md`](hardcoded-audit-2026-06-19.md) §A.1). |
| Activity-bar / panel priorities, most keybinding defaults | ✗ | ✗ | `Hard` — no override path (keybindings overridable via registry, defaults baked). |

`◑` = partially surfaced.

## 3. Why coverage is uneven

1. **Opt-in, not automatic.** A subsystem appears in the panel only if a shell
   plugin hand-writes a `configuration.schema`. Nothing enumerates backend
   `Config` structs, so a setting can exist in `ai.toml`/`mcp.toml` yet never be
   discoverable in the UI.
2. **Split-brain backends.** Some subsystems are configured in *two* places with
   no single surface — e.g. AI provider config in `ai.toml` (backend) vs. AI shell
   knobs in the `[settings]` bag (UI). The panel shows one; the TOML owns the other.
3. **Schema ≠ wiring.** A schema field can exist while the consumer ignores it.
   The terminal previously *looked* configured (the tracker even claimed "schema
   present") but the xterm instance hardcoded `fontSize: 13` — schema and code had
   drifted apart.

## 4. Terminal — worked example (gap closed in this change)

**Before:** the `nexus.terminal` plugin (`shell/src/plugins/nexus/terminal/index.ts`)
declared 4 schema fields — two toast durations, auto-restart delay, and
external-emulator priority — none of which is what most users mean by "terminal
settings." Font size and scrollback were hardcoded in the xterm constructor
(`shell/src/plugins/nexus/terminal/TerminalInstance.tsx:246,250` → `fontSize: 13`,
`scrollback: 5000`). The backend `nexus-terminal` crate has **no `Config` struct**
at all (drainer/memory timings are bare consts). The `hardcoded-shell.md` tracker
pointed at a stale `core/terminal/index.ts` path (that plugin no longer exists)
and wrongly claimed font had a schema.

**After (this change):**
- Added `terminal.fontSize` (default 13) and `terminal.scrollback` (default 5000)
  to the plugin's `configuration.schema` — they now render in the Settings panel
  and persist to the forge `[settings]` bag like every other World-A setting.
- `TerminalInstance.tsx` reads them at mount via `configStore.get<number>(…)`
  (the same non-reactive accessor `savedCommandsStore.ts:153` uses), with a
  numeric guard so a bad value can't break the xterm canvas. Applies to newly
  opened terminals; font *family* remains theme-driven (`--font-monospace`).
- Corrected the stale `hardcoded-shell.md` row.

Still `Hard` (out of scope here, tracked in `hardcoded-rust.md`): backend
`DRAINER_PUMP_TIMEOUT_MS`, `DRAINER_SLEEP_MS`, `DEFAULT_HISTORY_SAMPLES`.

## 5. Recommendations

1. **Make backend configs discoverable.** Give the panel a way to surface
   World-B `Config` structs (e.g. each service exposes a `config_schema` IPC
   handler the panel renders), so MCP/LSP/DAP/sandbox/kernel settings aren't
   TOML-only. *Largest structural win.*
2. **Add a schema↔consumer lint.** A schema field whose key is never read by its
   plugin (or a hardcoded literal that shadows a declared key) should fail review —
   this is exactly the terminal font drift.
3. **Pick one home per subsystem.** For split-brain cases (AI), document which
   settings live in the `[settings]` bag vs. the service TOML so users aren't
   hunting across two surfaces.
4. **Promote the obvious remaining UI knobs** — terminal backend timings,
   `LinkPreviewConfig` (no surface at all), sandbox orchestrator timeouts — per the
   [How to add a setting](README.md#how-to-add-a-setting) convention.
