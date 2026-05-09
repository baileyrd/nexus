# Shell Docs Archive

> Historical shell-specific documentation. Created 2026-04-26 during
> the docs-cleanup pass. Each file has an `> **Archived <date>**` line
> at the top explaining why it's no longer in the active set.

For repository-wide archived documentation see
[`docs/archive/`](../../../docs/archive/).

## Inventory

| File | Status |
|---|---|
| `MIGRATION_PLAN.md` | Tauri Shell → Nexus Forge pixel-match migration. Migration completed. |
| `predefined-layouts.md` | Named-layout-presets feature. **Rejected** in v1 per [ADR 0012](../../../docs/adr/0012-drop-named-layout-presets.md). Never built. |
| `plans/keybinding-storage-refactor.md` | Refactor plan for keybinding override persistence. Refactor shipped. |
| `plans/keybindings-ui-fixes.md` | Three-bug fix plan for the keybindings settings tab. Fixes shipped. |

## Still current

`shell/docs/` (the parent directory) holds shell architecture
documentation that *is* current:

- `architecture.md`, `extension-host.md`, `plugin-system.md`,
  `plugin-api.md`, `slot-system.md`, `registry-system.md`,
  `event-bus.md`, `core-plugins.md`, `context-keys.md`,
  `workspace-layout.md`, `writing-a-plugin.md`.
- `obsidian/` — Obsidian behavior reference notes used while
  porting Workspace/Leaf semantics.
- The various `*.svg` / `*.json` diagrams describing boot, layout,
  and event-flow.
