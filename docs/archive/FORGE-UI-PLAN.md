> **Archived 2026-04-26** — Pre-shell-migration UI plan. The legacy `app/` frontend it targets was retired 2026-04-24 (see [`docs/legacy-shell-retirement.md`](../legacy-shell-retirement.md)). All stages shipped against the now-deleted `app/`; equivalent surfaces in the current shell live under `shell/src/shell/`, `shell/src/workspace/`, and `shell/src/plugins/core/titleBar/`.

# Nexus Forge — UI Implementation Plan

> **Historical document** — Written before the `app/` → `shell/` migration (Phase 4 WI-37, 2026-04-24). See `docs/legacy-shell-retirement.md`. All stages in this plan shipped against the retired `app/` frontend; the `app/src/…` paths below are preserved verbatim as a record of state at time of commit. Equivalent surfaces in the current plugin-first shell live under `shell/src/shell/`, `shell/src/workspace/`, and `shell/src/plugins/core/titleBar/`.

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
- [x] **Outline rows** — `Outline.tsx` now emits a 2-digit index (h1/h2 only) + title + word-count chip per heading; word counts derived by slicing file content between heading lines. Styled under `[data-theme-id="nexus-forge"]`.
- [~] **Counts in tab labels** — still deferred; requires each panel to report a `count` field via a contribution-API addition. Outline count is derivable from `openFileStore` content but wiring it into the Inspector's panel-selector text-tabs is a separate schema change.

---

## Stage 5 — Status bar ✓

- [x] **Widen to 24 px** — full-width bar pinned at the bottom of the workspace frame, Inter font, muted text.
- [x] **Ember pip** — the sync/status item shows the green ok-pip with a soft glow, matching the design.
- [x] **Status-bar items stay contributions** — styling is applied to `.status-bar-item` / `.status-bar-text` / `.status-bar-icon`; the preset + plugins still decide *which* items appear. Microkernel model intact.

### Deferred (follow-up)

- [x] **Left/right clustering** — shipped via a sentinel `id = "status.spacer"` item instead of a schema change. `StatusBar.tsx` renders the sentinel as a flex:1 span; preset authors place it anywhere they want the left/right boundary. Zero Rust-type churn. Obsidian preset reordered: sync on the left; word/char/mode/backlinks/properties on the right.
- [~] **Live data wiring** — word-count, character-count, and outgoing-link count now driven by `openFileStore`. Git branch + SHA + dirty-marker driven by subscribing to the `com.nexus.git.*` plugin events — no new IPC command, no nexus-app → nexus-git dep added (the existing plugin event bus forwards state through `plugin:event` Tauri events). Left-cluster items (forge.synced, git.branch, git.sha, index.docs, plugins.hot) now rendered; index.docs + plugins.hot remain static pending a Tantivy count IPC + plugin registry snapshot.
- [x] **Preset picker → command palette** — six `Switch layout: <preset>` palette entries (Obsidian, Writing, Reviewing, Coding, Vibe coding, Dev) registered in `builtins.ts`; each dispatches `useLayoutStore.loadPreset()`. Plugin-contributed presets can self-register via manifest.

---

## Stage 6 — Verification ✓

- [x] Vitest: `cd app && npm test` — 18 tests pass.
- [x] Cargo: `cargo test -p nexus-theme --lib` — 114 pass; `cargo test -p nexus-storage --lib` — 261 pass; `cargo test -p nexus-bootstrap --test theme_ipc` — 4 pass.
- [x] Vite build clean, no CSS warnings.
- [~] Visual diff: pending user relaunch of `npm run tauri:dev` on Windows to compare against `.design-bundle/project/Nexus Forge.html`. Remaining gaps are the three deferred items in Stage 5 (left/right status clustering, live status data, switch-layout palette commands).

## Shipped commits

| Stage | Commit   | Summary |
| ----- | -------- | ------- |
| 0a    | `bd788b3` | Forge theme registered as a builtin; default first-mount theme. |
| 0b    | `bac47fc` | Cross-platform `nexus-storage` (unblocks the Windows Tauri build). |
| plan  | `1afe8ab` | This document. |
| 1     | `d78eb7d` | 36 px Forge topbar + chrome cleanup (MenuBar / workspace heading / preset picker row retired). |
| 2     | `5fd586b` | Left panel polish — compact header, ⌘P filter pill, 26 px tree rows, avatar-chip footer. |
| 3     | `2392abc` | Tabstrip + Welcome doc + `.doc` typography (IBM Plex Serif body, ember wikilinks/blockquote). |
| 4     | `a5cc7f3` | Right Inspector panel — "Inspector" header + text-tab selector with ember underline. |
| 5     | `89bf52d` | 24 px Forge status bar (structural clustering + live data deferred). |

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
