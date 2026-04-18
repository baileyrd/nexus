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

## Stage 1 — Topbar + structural cleanup ✓

Collapse the 80 px header stack into a single 36 px Forge topbar. **Shipped.**

- [x] **[app/src/App.tsx](../app/src/App.tsx)** — dropped the `<header class="app-header">` block; mounts `<ForgeTopBar />`. Brand (left) + breadcrumb pill (center) + icon cluster (right, includes `ModeToggle`).
- [x] **[app/src/components/layout/WorkspaceView.tsx](../app/src/components/layout/WorkspaceView.tsx)** — removed the `<header><h2>Workspace</h2><LayoutPresetPicker /></header>` render. Frame-only now.
- [x] **[app/src/components/layout/ForgeTopBar.tsx](../app/src/components/layout/ForgeTopBar.tsx)** (new) — composes breadcrumb from `forgeStore.info.name` + `openFileStore.file` + live word-count; falls back to "Workspace / no file open" when no file is open.
- [x] **MenuBar** — hidden under `[data-theme-id="nexus-forge"]` via CSS. Command palette covers the same surface.
- [~] **Preset picker** — no longer rendered in chrome but left in the codebase; command-palette entries for `Switch layout: <preset>` will land with Stage 5 (status bar feeds a new command group anyway).

---

## Stage 2 — Left side panel polish ✓ (`<pending-hash>`)

Match the "NEXUS_WORK" file-tree panel 1-for-1. **Shipped.**

- [x] **Panel header** — 36 px, 11 px letter-spaced uppercase label; panel-toolbar buttons restyled to 22×22 frameless icons.
- [x] **Filter pill** — 24 px button that opens the command palette (the real Nexus file-finder), with trailing `⌘P` kbd chip matching the design.
- [x] **Tree rows** — 26 px, ember active-bar for the open file, ember-soft background highlight.
- [x] **Footer** — forge-selector restyled as an avatar chip (gradient circle + name); compact icon actions on the right.

---

## Stage 3 — Center: tabstrip + welcome doc ✓

- [x] **[app/src/components/layout/TabStrip.tsx](../app/src/components/layout/TabStrip.tsx)** — TSX left as-is; Forge-scoped CSS gives it a 34 px bar, ember top-bar on the active tab, dirty dot, × on hover.
- [x] **Welcome doc** — `WelcomeSurface` component in `PaneView.tsx` replaces the `Empty pane / id ·` placeholder. Serif title + metaline chips + blockquote + sectioned ul, all classed as `.doc` so open files inherit the same rules.
- [x] **Editor surface styling** — serif body, ember-accent blockquote / wikilinks, code chips, uppercase section headings. All scoped to `[data-theme-id="nexus-forge"]` so other themes are unaffected.

---

## Stage 4 — Right Inspector panel ✓

- [x] **Header → "Inspector"** — added as a 36 px header row above the panel-selector, scoped to Forge. Non-Forge themes keep the per-panel title header.
- [x] **Tab switcher** — the existing `PanelSelector` gets a visible label span that's visually-hidden by default but shown under Forge. Net effect: icon-only on other themes, text-tabs with ember underline under Forge — same component, same contributions.
- [~] **Outline rows** — deferred; `Outline.tsx` renders through `GenericTreePanel` which already handles the data shape. Cosmetic row-tuning can ride with a future pass.
- [~] **Counts in tab labels** — deferred; requires each panel to report a `count` field, which is a contribution-API addition better packaged with other panel metadata (icon badge, loading state).

---

## Stage 5 — Status bar ✓

- [x] **Widen to 24 px** — full-width bar pinned at the bottom of the workspace frame, Inter font, muted text.
- [x] **Ember pip** — the sync/status item shows the green ok-pip with a soft glow, matching the design.
- [x] **Status-bar items stay contributions** — styling is applied to `.status-bar-item` / `.status-bar-text` / `.status-bar-icon`; the preset + plugins still decide *which* items appear. Microkernel model intact.

### Deferred (follow-up)

- [ ] **Left/right clustering** — requires adding a `position: "left" | "right"` field to the `StatusBarItem` Rust type (`ts-rs` re-export), then a flex grouping in `StatusBar.tsx`. Better bundled with the wider "status-bar schema v2" work.
- [ ] **Live data wiring** — replace hard-coded preset text (`"0 backlinks"`, `"7 properties"`, `"2,348 words"`) with live feeds from `activeEditor`, `forgeStore`, git IPC, Tantivy doc count, and plugin-hot count. Each datum should register as its own `contributions/builtins.ts` status-bar contribution.
- [ ] **Preset picker → command palette** — land the `Switch layout: <preset>` command group that replaces the retired preset picker row (carried over from Stage 1).

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
