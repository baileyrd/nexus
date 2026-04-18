# Nexus Forge — UI Implementation Plan

**Goal.** Make the running front end match the `Nexus Forge.html` design bundle
(`.design-bundle/project/Nexus Forge.html`) while preserving the editor-shell
microkernel: activity rail + side panels + tabstrip + status bar +
contributions registry remain the thin core; every visible surface ships as a
theme / panel / content-type contribution.

**Status key.** ☐ not started · ◐ in progress · ☑ done · ✕ dropped

---

## Stage 0 — Theme + chrome scaffold (shipped)

- [x] `nexus-forge` builtin theme registered in [`crates/nexus-theme`](../crates/nexus-theme). Ember palette, Inter / IBM Plex Serif / JetBrains Mono. Commit `bd788b3`.
- [x] `data-theme-id` stamped on `<html>` so theme-scoped CSS can light up without leaking into light/dark.
- [x] First-mount default = `nexus-forge`.
- [x] Windows build unblocked — `nexus-storage` made cross-platform via `fs4` + `sync_all`. Commit `bac47fc`.

Result: palette + typography correct, but structural chrome still renders the
old "Workspace / preset-picker / MenuBar" layout. Closing that gap is the rest
of this plan.

---

## Stage 1 — Topbar + structural cleanup

Collapse the 80 px header stack into a single 36 px Forge topbar.

- [ ] **[app/src/App.tsx](../app/src/App.tsx)** — drop the `<header class="app-header">` block; mount `<ForgeTopBar />` instead. Brand (left) + breadcrumb pill (center) + icon cluster (right). Keep `ModeToggle` as one of the cluster buttons.
- [ ] **[app/src/components/layout/WorkspaceView.tsx](../app/src/components/layout/WorkspaceView.tsx)** — remove the `<header><h2>Workspace</h2><LayoutPresetPicker /></header>` render. `WorkspaceView` becomes frame-only.
- [ ] **Preset picker** — retire from the chrome; re-expose via command palette entries (`Switch layout: Coding`, etc.). Keeps the contribution alive without stealing 40 px.
- [ ] **MenuBar** — don't render under `[data-theme-id="nexus-forge"]`. The File/View/Help vertical column disappears; command palette covers the same commands.
- [ ] **Breadcrumb data source** — compose from `forgeStore.info.name` + `activeEditor` (file path) + doc stats; falls back to "Workspace" when no file is open.

Deliverable: topbar collapses to 36 px. Center column gets back vertical real estate for tabs + doc.

---

## Stage 2 — Left side panel polish

Match the "NEXUS_WORK" file-tree panel 1-for-1.

- [ ] **Panel header** — 36 px, 11 px letter-spaced uppercase label, tiny icon buttons (`+` new · folder · collapse). Style via `.panel-head` scoped to Forge.
- [ ] **Filter pill** — 24 px inline input `Filter files...` with a trailing `⌘P` kbd chip, matching `.filter` in the design HTML.
- [ ] **Tree rows** — 26 px, caret + icon + name + optional count / activity dot. Already have `FileTree.tsx`; restyle, don't rewrite.
- [ ] **Footer** — avatar chip + forge name (`LW  lap-working`) + help + settings icons. Reuse `SidePanelFooter.tsx`.

---

## Stage 3 — Center: tabstrip + welcome doc

- [ ] **[app/src/components/layout/TabStrip.tsx](../app/src/components/layout/TabStrip.tsx)** — restyle: 34 px bar, ember top-bar on active tab, dirty dot, × on hover, overflow-x scroll, trailing `+` new-tab button.
- [ ] **Welcome doc** — new content type `nexus.welcome` registered in [`app/src/contributions/builtins.ts`](../app/src/contributions/builtins.ts). Renders a serif "Welcome to Forge" document (matches the HTML prototype's hero card) when no file is open. Replaces today's `Empty pane / id · pane-obsidian-main` placeholder.
- [ ] **Editor surface styling** — scoped to `[data-theme-id="nexus-forge"]`: serif body, tables with rounded borders, wikilinks ember + dashed underline, `<blockquote>` with ember left-rail, `<code>` chips, callout blocks.

---

## Stage 4 — Right Inspector panel

- [ ] **Header → "INSPECTOR"** — star (pin) + close-panel icons.
- [ ] **Tab switcher** — Outline / Backlinks / Graph as a segmented tab row. Each tab drives the existing panel contributions (`Outline.tsx`, backlinks panel, graph panel). No new panels; just swap `PanelSelector` for a segmented tab inside the panel shell.
- [ ] **Outline rows** — 2-digit index + title + word-count chip in the right margin. Restyle [`app/src/components/panels/Outline.tsx`](../app/src/components/panels/Outline.tsx).
- [ ] **Counts in tab labels** — `Outline 14 · Backlinks 5 · Graph`, driven by panel state.

---

## Stage 5 — Status bar

- [ ] **Widen** — 24 px, flex row, left + right clusters.
- [ ] **Left cluster** — `● Forge synced · main · <git-sha> · Tantivy · N docs · ● N plugins hot`. Needs: git HEAD sha (already surfaced via `forgeStore`/IPC? — verify or add), Tantivy doc count (`nexus-storage` exposes one), plugin-hot count (plugins store).
- [ ] **Right cluster** — `ln X, col Y · MD · UTF-8 · N words · N chars · N backlinks missing`. Drive from `activeEditor` + doc metadata.
- [ ] Each datum registered as a status-bar item contribution in `contributions/builtins.ts`, not hard-coded — keeps the microkernel model.

---

## Stage 6 — Verification

- [ ] Vitest: `cd app && npm test` — existing panel tests must still pass.
- [ ] Cargo: `cargo test -p nexus-theme --lib` + `cargo test -p nexus-storage --lib`.
- [ ] Visual diff: launch Tauri dev (`npm run tauri:dev`), take a screenshot, compare side-by-side to `.design-bundle/project/Nexus Forge.html` rendered. Update this plan's checkboxes.

---

## Out of scope (explicit)

- Process-manager workspace (`forge_processes.jsx`) — separate pass; ties into PRD-09.
- AI Orchestrate workspace (`forge_orchestrate.jsx`) — separate pass; ties into PRD-12 / PRD-15.
- Templates gallery (`forge_templates.jsx`) — separate pass.
- Light-mode Forge variant — tokens exist in `nexus-light` already; only the `[data-theme-id="nexus-forge"]` rules need a `[data-theme-id="nexus-forge-light"]` companion if we add one later.

---

## Ground rules

- **No architectural changes.** Every visible surface stays a contribution (theme / panel / content-type / status-bar item / command). No hard-coding.
- **Scoped CSS.** All visual tweaks live under `[data-theme-id="nexus-forge"]` so `nexus-light` / `nexus-dark` are unaffected.
- **Small commits per stage.** One commit per stage so regressions bisect cleanly. Reference this plan's section in each commit body.
- **Update this file** as stages land — flip `[ ]` to `[x]`, add commit hash, note any deviations.
