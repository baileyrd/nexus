# activityBar (core)

- **Path:** `shell/src/plugins/core/activityBar/`
- **Tier:** Shell Core
- **Plugin id:** `core.activity-bar`

## Architecture
- Entry point: `shell/src/plugins/core/activityBar/index.ts:8`
- Activation: `onStartup`
- Modules:
  - `index.ts` — plugin manifest + activate hook
  - `activityBarStore.ts` — Zustand store holding `ActivityBarItem[]`, deduped by id
  - `ActivityBarView.tsx` — the rendered icon-rail React component
- Persistence: in-memory only (rail composition is rebuilt per boot)
- Settings owned: none
- External deps: none beyond `@nexus/extension-api` types

## Surface
- **Commands:** `activityBar.toggle`
- **Views:** registers `activityBar` into the `activityBar` slot at priority 0
- **Events consumed:** `activityBar:itemAdded`, `activityBar:itemRemoved`
- **Seeded items:** seven placeholder rail entries (search, graph, tasks, git, db, templates, ai) at priorities 20–80
- **Consumes from `@nexus/extension-api`:** `Plugin`, `PluginAPI` types only

## Necessity
- **Verdict:** Essential (role); this specific implementation is superseded
- **Required for basic capabilities?** Yes — the left rail is the only navigation surface that lets a user switch sidebar views (files, search, git). Without an activity bar the shell collapses to a single hard-coded panel.
- **Depended on by (the role, via `nexus.activityBar`):** `nexus.files`, `nexus.search`, `nexus.ai`, `nexus.gitPanel`, `nexus.collab`, `nexus.terminal`, `nexus.diagnostics`, `nexus.notificationsInbox`, `nexus.dreamCycle`, `nexus.osArchitecture`, `nexus.observability`, `nexus.skills`, `nexus.templates`, `nexus.workflow`, `nexus.mcp`, `nexus.viewBuilder`, `nexus.activityTimeline`, `nexus.themePicker`, `nexus.agent`, `nexus.processes`, `nexus.osArchitecture`, `core.settings`, etc. — every plugin that contributes a sidebar view registers a rail icon via `api.activityBar.addItem`.
- **Depends on:** nothing (pure UI registry)
- **What breaks if removed:** Every sidebar feature loses its entry point; users can't reach files/search/git/AI from the shell chrome.

## Notes
- **This crate is dead code in 0.1.2.** The catalog at `shell/src/plugins/catalog.ts:166` registers `nexus.activityBar` (under `shell/src/plugins/nexus/activityBar/`) as the default-on rail; `core.activity-bar` is never imported by `catalog.ts` or `main.tsx`. The two implementations duplicate the store, view, and seed list.
- Grep cross-refs (`useActivityBarStore`, `activityBar:itemAdded`) all resolve to the `nexus.activityBar` copy; nothing imports from `core/activityBar`.
- Cleanup candidate: delete this directory once a release-notes entry confirms no consumer expects the `core.activity-bar` id (no `plugins.enabled` rows reference it — it's default-on in neither catalog).
- Necessity verdict above is for the activity-bar feature; the literal `core.activity-bar` plugin is Removable on its own merits.
