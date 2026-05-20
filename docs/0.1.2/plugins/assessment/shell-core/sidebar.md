# sidebar

- **Path:** `shell/src/plugins/core/sidebar/`
- **Tier:** Shell Core
- **Status:** Phase 7 legacy stub. Not loaded by `main.tsx`, absent from `shell/src/plugins/catalog.ts`. The active left sidebar is `nexus.sidebar` (catalog `DEFAULT_ON_PLUGINS`).

## Architecture
- Entry: `shell/src/plugins/core/sidebar/index.ts:11` — exports `sidebarPlugin` with manifest `id: 'core.sidebar'`, `activationEvents: []`, no contributions.
- `activate()` is a documented no-op.
- Persistence: none.
- Settings owned: none.
- External deps: none.

## Surface
- None. The original `SidebarView` that this plugin registered into `slot: 'sidebar'` was removed when the left sidedock became a workspace sidedock managed by `nexus.workspace` + `nexus.sidebar`.

## Necessity
- **Verdict:** Removable (this file). The user-facing concept "left sidebar" is **Essential** but is delivered by `nexus.sidebar`, not this stub.
- **Required for basic capabilities?** No — `activate()` does literally nothing.
- **Depended on by:** none — no `dependsOn: ['core.sidebar']` exists in `shell/src/plugins/`.
- **Depends on:** nothing.
- **What breaks if removed:** nothing. The header comment explicitly says "retained as a stub so any build that still imports it from the template compiles". A `git grep` confirms no live importer.

## Notes
- Safe to delete in a follow-up cleanup pass, together with `core.right-panel`, `core.terminal`, `core.title-bar`, and `core.status-bar` — all five are pre-Phase-7 stubs whose live equivalents now live under `shell/src/plugins/nexus/`.
- The block in `CLAUDE.md` notes "every visible UI element is a plugin contribution" — that contract holds because `nexus.sidebar` (not this file) is the contributing plugin.
