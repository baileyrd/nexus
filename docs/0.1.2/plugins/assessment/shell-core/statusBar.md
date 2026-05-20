# statusBar

- **Path:** `shell/src/plugins/core/statusBar/`
- **Tier:** Shell Core
- **Status:** Legacy template — **not loaded** by `main.tsx`. Absent from `shell/src/plugins/catalog.ts`. The active status bar is `nexus.statusBar` (catalog `DEFAULT_ON_PLUGINS`, `dependsOn: ['nexus.workspace', 'nexus.editor']`).

## Architecture
- Entry: `shell/src/plugins/core/statusBar/index.tsx:4` — exports `statusBarPlugin` with manifest `id: 'core.status-bar'`.
- View: `shell/src/plugins/core/statusBar/StatusBarView.tsx` — `StatusBarLeft` and `StatusBarRight` render surfaces (not connected to any live store).
- `activate()` registers two views (`statusBarLeft`, `statusBarRight` slots) and seeds eight hardcoded placeholder items — `statusBar.sync`, `statusBar.branch`, `statusBar.index`, `statusBar.plugins`, `statusBar.position`, `statusBar.encoding`, `statusBar.count`, `statusBar.backlinks`. Values are static strings (e.g. `"main · 0000000"`, `"Tantivy · 0 docs"`).
- Persistence: none.
- Settings owned: none.
- External deps: none.

## Surface
- Views: `statusBarLeft`, `statusBarRight` (slots of the same names).
- Status bar items: 8 placeholder items split 4/4 across `slot: 'left'` / `slot: 'right'`.
- Commands / keybindings: none.

## Necessity
- **Verdict:** Useful (concept) / Removable (this file).
- **Required for basic capabilities?** No — the status bar is a footer; opening, browsing, editing, searching, and committing markdown all work without it. The "concept" rating from the brief refers to the **live** status bar (`nexus.statusBar` + `nexus.gitStatus`), not this template.
- **Depended on by:** none. No catalog entry, no `dependsOn: ['core.status-bar']`. The popout-compatibility test does not see this file.
- **Depends on:** nothing.
- **What breaks if removed:** nothing. Real cursor-position / encoding / git-branch / index-count items are produced by `nexus.statusBar`, `nexus.gitStatus`, and other feature plugins via `api.statusBar.createItem`.

## Notes
- This is the only file in the legacy `core/` cluster that's a `.tsx` rather than `.ts`, because the manifest items embed JSX literals. That JSX is unused; cleanup welcome.
- The hardcoded `"0000000"` SHA and `"0 plugins hot"` strings are visible in the template but never rendered at runtime, so they are not a UX issue — only confusing if a developer mistakes this for the live plugin.
