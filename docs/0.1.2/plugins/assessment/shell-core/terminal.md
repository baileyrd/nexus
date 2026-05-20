# terminal

- **Path:** `shell/src/plugins/core/terminal/`
- **Tier:** Shell Core
- **Status:** Legacy template — **not loaded** by `main.tsx`. Absent from `shell/src/plugins/catalog.ts`. The active terminal is `nexus.terminal` (catalog `DEFAULT_OFF_PLUGINS`, `shell/src/plugins/catalog.ts:427`).

## Architecture
- Entry: `shell/src/plugins/core/terminal/index.ts:10` — exports `terminalPlugin` with manifest `id: 'core.terminal'`, `dependsOn: ['core.panel-area', 'core.configuration-service']`.
- View: `shell/src/plugins/core/terminal/TerminalView.tsx` — a Zustand-backed dummy terminal log (no PTY, no kernel call).
- Store: `shell/src/plugins/core/terminal/terminalStore.ts` — in-memory `lines[]` log; `useTerminalStore` exposes `addLine` / `setInput` / `clear`.
- The activate path: `terminal.toggle` is registered as a documented no-op; `terminal.clear` calls `useTerminalStore.getState().clear()`; the plugin registers its `configuration` schema with `core.configuration-service`. The `core.panel-area` dep refers to a retired Phase 7 concept; follow-up task #11 (per the source comment) tracks a bottom-dock replacement.
- Persistence: none on disk; ephemeral Zustand store.
- Settings owned: `terminal.fontSize` (number, default 13), `terminal.fontFamily` (string, default `'Cascadia Code', 'Consolas', monospace'`) — see plugin `configuration.schema` block. Not currently documented in `docs/0.1.2/settings/`.
- External deps: `zustand`.

## Surface
- Commands: `terminal.toggle` (no-op), `terminal.new`, `terminal.clear`.
- Keybindings: ``ctrl+` `` → `terminal.toggle`.
- Configuration section: `core.terminal` titled "Terminal", category `system`, order 30.

## Necessity
- **Verdict:** Optional → effectively Removable.
- **Required for basic capabilities?** No — running a PTY inside the shell is unrelated to opening, browsing, editing, searching, and committing markdown. The active terminal plugin `nexus.terminal` ships **default-off**, confirming the project treats embedded shell as opt-in.
- **Depended on by:** none. No catalog entry; no other plugin declares `dependsOn: ['core.terminal']`.
- **Depends on:** the (also legacy) `core.panel-area` and the live `core.configuration-service`. Since the plugin is never loaded, the `core.panel-area` dependency is moot.
- **What breaks if removed:** nothing. The ``ctrl+` `` chord would free up if this file were deleted — `nexus.terminal` re-registers its own toggle when enabled.

## Notes
- The leading source comment is unusually explicit: "This file's toggle command no-ops because the panel-area concept was retired by Phase 7". Together with the `core.panel-area` dep, this is one of the clearer deletion candidates among the legacy core-shell plugins.
- The two configuration keys (`terminal.fontSize`, `terminal.fontFamily`) are dead settings because the plugin that reads them never activates; if `nexus.terminal` ever inherits them, document under `docs/0.1.2/settings/`.
