# ADR 0013: Menu Bar Strategy — Palette-First, macOS Minimal Exception

**Status:** Accepted
**Date:** 2026-04-23
**Deciders:** Project lead

## Context

Modern desktop apps split between two command-discovery models: a native
top-of-window **menu bar** (File / Edit / View / …) and a keyboard-driven
**command palette**. Nexus's plugin-first shell ships with a palette
plus an activity bar; the legacy `app/` shell had neither a menu bar nor
a palette of comparable scope. Phase 2 plan §5.5 (WI-16) asks whether
to introduce a menu bar (as a plugin) or commit to palette-first.

A native menu bar in Tauri is platform-specific. macOS users in
particular expect at minimum a system menu bar with **File**, **Edit**,
**View**, and **Window** — the OS itself reserves screen real estate
for it, and its absence reads as broken.

## Decision

**Commit to palette-first as the v1 command-discovery surface on all
platforms.** `@nexus/extension-api` does **not** expose
`api.menuBar.register()` in v1. No general menu-bar contribution point
ships.

**Exception:** macOS gets a minimum platform-conformance menu bar
(File / Edit / View / Window) provided by a small platform-specific
plugin landing in **Phase 4**. This plugin uses an internal API local to
itself; menu items are not user-extensible in v1.

## Rationale

- **The palette already covers discovery.** Every command registered via
  `api.commands.register()` is searchable from the palette. Activity bar
  + palette + per-view context menus reach every entry point a menu bar
  would.
- **Two discovery surfaces drift.** A native menu bar is a parallel
  registry that must stay synchronized with the palette. Every plugin
  contribution would need to choose, every refactor would need to keep
  both in sync, and missing a menu entry would become a recurring bug
  class.
- **Tauri menu bars are awkward cross-platform.** The menu bar API is
  platform-conditional, web/embedded shells have no equivalent, and
  styling is OS-controlled. Building a portable abstraction is more work
  than the surface justifies.
- **The trend is away from menu bars.** Modern knowledge-work apps
  (Notion, Linear, Raycast, Obsidian's mobile UX) lean palette-first.
  Users now expect ⌘K / Ctrl-K as the primary discovery gesture.
- **macOS is genuinely different.** macOS HIG and user expectation make
  a system menu bar table-stakes. A four-menu minimum (File / Edit /
  View / Window) is the smallest acceptable footprint and small enough
  to maintain as a platform plugin without contribution-point sprawl.

## Consequences

- `@nexus/extension-api` v1 has **no** `menuBar` namespace. Plugin
  authors should not design around one.
- The palette is the canonical command surface. All commands must be
  reachable from the palette; this becomes a Phase-2 acceptance criterion.
- A small platform plugin (working name: `nexus.macos-menu`) lands in
  Phase 4. It owns its menu items internally — no public contribution
  point, no third-party menu items in v1.
- Other platforms (Windows, Linux, web) get nothing menu-bar-shaped in
  v1. The window-level title-bar plugin remains the only top-of-window
  contribution surface.
- A future ADR may revisit and promote a `menuBar` contribution point if
  demand materializes; nothing here precludes that.

## Alternatives considered

- **Ship a full cross-platform menu bar plugin in v1.** Rejected — adds
  a parallel registry, drift surface, and Tauri portability cost for a
  feature the palette already covers.
- **No macOS menu bar at all (palette everywhere, period).** Rejected —
  violates macOS HIG and reads as a broken port to Mac users.
- **Defer the macOS exception to post-v1.** Rejected — Phase-4 timing
  aligns with the Mac packaging/notarization work; doing it then is
  cheaper than doing it twice.

## References

- `docs/archive/planning/PHASE-2-IMPLEMENTATION-PLAN.md` §5.5 (WI-16)
- ADR 0011 — plugin-first shell
- ADR 0008 — tech-stack defaults (Tauri)
