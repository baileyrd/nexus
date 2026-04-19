# Tauri Shell → Nexus Forge Pixel-Match Migration Plan

Reference design: [docs/test/](test/) (Nexus Forge prototype)
Current shell: [src/](../src/)

## 1. Goals & Non-Goals

**Goal:** Reskin and restructure the current plugin-based Tauri shell so the rendered UI is a pixel-for-pixel match of the Nexus Forge prototype, while preserving the slot/plugin architecture.

**Non-goals:**
- Replacing Zustand, the slot/registry model, or the extension host.
- Replicating Nexus Forge's monolithic single-file structure — each workspace becomes a plugin.
- Implementing real backends for processes/AI/templates panes in this migration (visual parity only; data can stay mocked).

## 2. Architecture Decisions (lock these first)

| Decision | Choice | Rationale |
|---|---|---|
| Styling | **Vanilla CSS + token registry** (mirror Forge) | Forge's tokens are already CSS vars; Tailwind would duplicate work and fight OKLch. |
| Color space | **OKLch** with light/dark themes | Matches [Nexus Forge.html](test/Nexus%20Forge.html) exactly. |
| Density | Add `compact` / `cozy` / `spacious` to `layoutStore` | Forge switches row heights and font sizes per density. |
| Workspace switching | Extend `App.tsx` to select a **workspace layout** per activity item, not just a sidebar view | Forge fullscreens terminal/AI/templates panes. |
| Right panel | New core slot `rightPanel` (300px, toggleable) | Absent from current shell. |

## 3. Foundation Work (Milestone 0)

Before any component ports, land:

1. **Token stylesheet** — port the `:root` / `[data-theme]` / `[data-density]` blocks from [Nexus Forge.html](test/Nexus%20Forge.html) into [src/shell/shell.css](../src/shell/shell.css). Replace the current `--shell-bg`, `--accent`, etc. with the Forge token names so existing plugins pick up the theme automatically (with a thin compat layer where names differ).
2. **Theme service** — small store (`themeStore`) exposing `theme` (`dark`|`light`) and `density`; writes `data-theme` / `data-density` on `<html>`.
3. **Font loading** — Inter, IBM Plex Serif, JetBrains Mono via `@font-face` or CDN import in [index.html](../index.html).
4. **Grid recalibration** in [src/shell/App.tsx](../src/shell/App.tsx):
   - titlebar 36px, statusbar 24px (already ~correct)
   - activity rail **44px** (currently 48px)
   - default sidebar 260px, right panel 300px
   - add `rightPanel` slot to [SlotRegistry](../src/registry/) and `rightPanelVisible` to `layoutStore`.

## 4. Core Component Ports (Milestone 1)

Each item = a plugin or view in `src/plugins/core/`. Source files are in [docs/test/](test/).

| # | Target | Source reference | Notes |
|---|---|---|---|
| 1 | `TitleBar` restyle | [forge_app.jsx](test/forge_app.jsx) `<TopBar>` | Breadcrumb + sync dot + window controls; keep Tauri drag region. |
| 2 | `ActivityBar` restyle | [forge_app.jsx](test/forge_app.jsx) `<ActivityRail>` | 8 main items + bottom 3; 32px buttons, left accent bar on active, notification pip. |
| 3 | `TreeNode` + `LeftPanel` | [forge_panels.jsx](test/forge_panels.jsx) | Recursive tree, depth padding `6 + depth*14`, status dots, counts, caret, filter input, user footer. |
| 4 | `RightPanel` (new) | [forge_panels.jsx](test/forge_panels.jsx) | Tabs: Outline / Backlinks / Graph. Graph = inline SVG. Properties key/value card. |
| 5 | `TabBar` (new) | [forge_app.jsx](test/forge_app.jsx) `<Tabs>` | Dirty marker, icon, middle-click / ⌘W close. |
| 6 | `Doc` editor view | [forge_doc.jsx](test/forge_doc.jsx) | Markdown → semantic render, scroll-spy feeds RightPanel outline, 760px max-width serif. |
| 7 | `StatusBar` enrichment | [forge_app.jsx](test/forge_app.jsx) `<StatusBar>` | Left: sync/modules/branch/indexed/hot. Right: ln:col/encoding/words/backlinks. |
| 8 | `Icons` | [forge_icons.jsx](test/forge_icons.jsx) | Drop in as `src/shell/icons.tsx`; shared across all plugins. |

After this milestone the **Files workspace** is pixel-matched.

## 5. Workspace Plugins (Milestone 2)

Each is a separate plugin that registers an activity item + a full-viewport workspace layout (collapses left/right panels as Forge does).

| Plugin | Source | Layout |
|---|---|---|
| `processes` | [forge_processes.jsx](test/forge_processes.jsx) + [screenshots/](test/screenshots/) | 280px sidebar (process groups, status dots, memory) + detail pane (tabs: Logs/Env/Routes/History/Metrics, filter bar, sparkline footer). |
| `orchestrate` | [forge_orchestrate.jsx](test/forge_orchestrate.jsx) | Three sub-layouts: agents list / DAG / chat. Role-colored avatars, run trace with timestamps. |
| `templates` | [forge_templates.jsx](test/forge_templates.jsx) | Category rail + filter bar + card grid with cover previews. |
| `data` | [forge_data.jsx](test/forge_data.jsx) | Table/dataset browser (scope: visual parity only). |

Workspace switching mechanism: add `activeWorkspace` to `layoutStore`; `App.tsx` branches on it to choose which slots render. The current static 3-column render becomes the `files` workspace.

## 6. Plugin Model Mapping

| Forge concept | Current shell mechanism |
|---|---|
| Activity rail item | `activityBar` slot entry + registered command to switch workspace |
| Left panel content | `sidebar` slot, one view per workspace |
| Right panel tabs | `rightPanel` slot, multiple entries (plugin contributes a tab) |
| Tab bar | New `editorTabs` slot above `editorArea` |
| Status metadata | `statusBarLeft` / `statusBarRight` slot entries, plugins contribute cells |

This keeps extensibility: third-party plugins can contribute activity items, right-panel tabs, and status cells without forking the shell.

## 7. Sequencing & Estimates

1. **M0 Foundation** (tokens, fonts, grid, rightPanel slot) — ~1 day.
2. **M1 Files workspace pixel match** (items 1–8 above) — ~3–4 days. Ship-gated on visual diff against [Nexus Forge.html](test/Nexus%20Forge.html).
3. **M2a Processes workspace** — ~2 days (screenshots exist in [docs/test/screenshots/](test/screenshots/) for diffing).
4. **M2b Orchestrate workspace** — ~2 days.
5. **M2c Templates + Data workspaces** — ~2 days.
6. **Polish pass** — density toggle, light theme QA, keyboard shortcuts, scrollbar styling — ~1 day.

## 8. Verification

- Side-by-side visual diff: run the prototype ([docs/test/Nexus Forge.html](test/Nexus%20Forge.html)) in a browser next to `pnpm tauri:dev`; compare each workspace.
- Compare [docs/test/screenshots/processes*.jpg](test/screenshots/) against the live Processes workspace at 100% zoom.
- Theme parity: toggle light/dark and all three densities; confirm no hard-coded colors remain in plugin CSS.
- Keep existing keyboard/command registry tests green through all milestones.

## 9. Risks

- **Token name collisions** between current `--shell-*` vars and Forge's `--bg`/`--accent`/etc. Mitigation: compat shim in shell.css for one milestone, then delete.
- **Workspace switching** touches every core plugin's mount logic. Mitigation: land as a single PR with all core plugins updated.
- **Markdown renderer choice** for Doc — Forge uses a bespoke renderer. Recommend `react-markdown` + `remark-gfm` for speed; match Forge's class names so CSS carries over.
