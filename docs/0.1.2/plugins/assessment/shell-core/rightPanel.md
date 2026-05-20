# rightPanel

- **Path:** `shell/src/plugins/core/rightPanel/`
- **Tier:** Shell Core
- **Status:** Legacy template — retained on disk, **not loaded** by `main.tsx`. Absent from `shell/src/plugins/catalog.ts`. Superseded by `nexus.rightPanel`.

## Architecture
- Entry: `shell/src/plugins/core/rightPanel/index.ts:13` — exports `rightPanelPlugin` with manifest `id: 'core.right-panel'`.
- View: `shell/src/plugins/core/rightPanel/RightPanelView.tsx` — Outline / Backlinks / Graph tab component, read by no live plugin (the active right-panel plugin `nexus.rightPanel` has its own panes).
- Activation: `onStartup` (in manifest, unused — never enrolled in the catalog so `ExtensionHost` never sees it).
- Persistence: none.
- Settings owned: none.
- External deps: imports `workspace` from `../../../workspace` to flip `rightSplit.collapsed`.

## Surface
- Commands: `rightPanel.toggle`.
- Keybindings: `ctrl+alt+b` / `cmd+alt+b` → `rightPanel.toggle`.
- Views: none (`RightPanelView` is exported but not registered into a slot from this plugin).

## Necessity
- **Verdict:** Useful (concept) / Removable (this file).
- **Required for basic capabilities?** No — the right panel hosts outline / backlinks / properties, which are not needed to open, read, edit, search, or commit markdown. The shipping right-panel plugin is `nexus.rightPanel`; this stub is dead weight.
- **Depended on by:** none. No catalog entry, no `dependsOn: ['core.right-panel']` anywhere in `shell/src/plugins/`.
- **Depends on:** the in-process `workspace` singleton only.
- **What breaks if removed:** nothing user-visible. Source-level only: `shell/src/plugins/popoutCompatible.test.ts` walks `ALL_PLUGINS`; this file is not in that list, so the test is unaffected.

## Notes
- The leading file comment in `index.ts` explicitly states this is "retained on disk but NOT loaded from main.tsx". Safe to delete in a follow-up cleanup; `nexus.rightPanel` owns the live right sidedock.
- The bound chord `ctrl+alt+b` would collide with `nexus.rightPanel`'s toggle if this plugin were ever re-enabled; another reason to delete rather than re-wire.
