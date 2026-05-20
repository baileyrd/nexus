# panelArea

- **Path:** `shell/src/plugins/core/panelArea/`
- **Tier:** Shell Core
- **Plugin id:** `core.panel-area`

## Architecture
- Entry point: `shell/src/plugins/core/panelArea/index.ts:14`
- Activation: `onStartup`
- Modules:
  - `index.ts` — manifest only; `activate()` is a no-op (Phase 7 retired the panel-area concept; see header comment)
  - `panelAreaStore.ts` — Zustand store with `PanelTab[]`; the file is preserved but unused at runtime
- Persistence: none
- Settings owned: none
- External deps: none

## Surface
- **Commands:** `panel.toggle` (category `View`) — declared but no handler is registered
- **Keybindings:** `ctrl+j` / `cmd+j` → `panel.toggle`
- **Views:** none
- **Consumes from `@nexus/extension-api`:** `Plugin` type only
- **In-file note:** "No-op: the workspace owns bottom-dock state (task #11 pending)."

## Necessity
- **Verdict:** Useful (role); this implementation is **Removable**
- **Required for basic capabilities?** No — a minimum-viable Nexus (open forge, browse, edit, search, commit) does not require a bottom panel. Diagnostics, terminal, and debugger live there, but those are themselves Optional for the core workflow.
- **Depended on by:** `core.terminal` (`shell/src/plugins/core/terminal/index.ts`) still imports `usePanelAreaStore` — that's the only live consumer. Nothing in the catalog loads `core.panel-area` itself.
- **Depends on:** nothing
- **What breaks if removed:** Nothing functional. `core.terminal`'s import of `usePanelAreaStore` would need to be retargeted at whatever bottom-dock state replaces this (workspace node per task #11).

## Notes
- **Dead plugin.** The header at `index.ts:1` explicitly states "Legacy template plugin — retained on disk but NOT loaded from main.tsx." The catalog confirms it. Both the `panel.toggle` command and its keybinding are declared but never wired (`activate()` does nothing).
- Future direction: per the file comment, the bottom dock should be re-implemented inside the workspace node (`nexus.workspace`), not as a sibling plugin. The legacy task tracker reference is "follow-up task #11".
- Cleanup candidate after `core.terminal` is migrated off `usePanelAreaStore`.
