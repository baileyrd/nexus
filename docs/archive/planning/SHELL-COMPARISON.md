# App vs. Shell: Capability Comparison

> **Historical document** — Written before the `app/` → `shell/` migration (Phase 4 WI-37, 2026-04-24). Paths below reference the legacy `app/` and `crates/nexus-app/` tree that has since been deleted. For current locations see `docs/legacy-shell-retirement.md`.

**Date:** 2026-04-23
**Scope:** Detailed per-command / per-plugin comparison between the two coexisting Tauri desktop shells in the Nexus repo — legacy `app/` + `crates/nexus-app` vs. new plugin-first `shell/` + `shell/src-tauri`.
**Companion artifact:** [`Shell-Capability-Comparison.xlsx`](./Shell-Capability-Comparison.xlsx) — 115-row matrix, sortable/filterable by category and parity status.

---

## Headline numbers

| Metric | Value |
|---|---|
| Legacy `#[tauri::command]` handlers | **95** (in `crates/nexus-app/src/lib.rs`'s `generate_handler!` block) |
| Legacy event forwarders (kernel bus → Tauri event) | **~11** (forge, theme, AI×3, agent×4, plugin×2, URI) |
| New shell bridge commands total | **15** (7 narrow bridge + 8 shell-side convenience) |
| New shell feature plugins registered | **32** `nexus.*` plugins (shell/src/main.tsx) |
| New shell service plugins loaded | **4** `core.*` (configurationService, notificationService, fileSystemService, settings) |
| New shell UI plugins **not** loaded | **11** `core.*` plugins explicitly disabled (template branding) |
| Matrix rows (per-command + shell-only + shell-ahead) | **115** |

## Parity at a glance

Rolled up from the 115-row matrix, distributing each row into one verdict:

| Verdict | Rows | % | What it means |
|---|---|---|---|
| **parity** | 39 | 34% | Both shells deliver equivalent capability (verification only) |
| **partial** | 26 | 23% | Both have it but one is weaker (finish the weaker side) |
| **only-app** | 30 | 26% | Legacy has it, new shell doesn't — port before retiring |
| **only-shell** | 20 | 17% | New shell has it, legacy doesn't — net-new (no migration cost) |
| **Total** | **115** | 100% | |

(Five additional rows carry an `architectural-diff` note in the Notes column — these are counted inside `only-app` or `partial` above, depending on which side currently works. They represent concept-level decisions, not missing implementations.)

The exact cross-tabulation by category is on the `Summary by Category` sheet of the xlsx.

---

## The shape of the gap

Looking at the categories that have the most only-app rows (i.e., what needs porting before retiring `app/`), the picture is this:

### Tier 1 — biggest gaps (port first)

**AI chat + RAG (7 handlers, all `only-app`).** The `nexus.ai` plugin in the new shell is a skeleton. The legacy shell has live streaming chat, RAG `ask` with vector store, and multi-session persistence — all wired to kernel event forwarders (`ai:stream_start/chunk/done`). The kernel IPC surface (`com.nexus.ai::*`) already exists; only the plugin UI needs writing. This is the single biggest user-visible gap and should be Phase-2 priority one.

**Theme engine (8 handlers, all `only-app`).** The new shell deliberately excludes `core.themeService` because the template carried hardcoded "Forge Ember" / "Forge Paper" branding. Dark is currently enforced via `shell.css` at `:root`. To reach parity, redesign the theme plugin (brand-neutral) and wire it back in — including snippet cascade and light/dark/system mode switching. Medium effort, high visibility.

**Keybinding overrides UI (3 handlers, all `only-app`).** The shell has a `KeybindingRegistry` but no override persistence UI. The legacy `HotkeysTab.tsx` must be ported. Small effort.

**URI handler registry (1 handler + 1 extension point, `only-app`).** Any `nexus://` deep-link support lives only in legacy. If anyone uses URI handlers today, port before v1.

**Saved terminal commands sidebar (5 handlers, all `only-app`).** Legacy has `SavedCommandsPanel.tsx` and five IPC handlers for saved command CRUD + reorder. The new shell's `nexus.terminal` plugin doesn't expose this. Port as a sub-view.

**Menu bar + activation events + plugin capability listing (3 handlers, `only-app`).** Architecturally, the new shell chose palette-first over a MenuBar, so menu items may never port. Activation events and capability listing are security-adjacent items (tracked as marketplace gates in the integration review).

### Tier 2 — partial parity (finish the weaker side)

**Editor transaction wiring (2 handlers, `partial`).** The kernel has full `apply_transaction` / `undo` / `redo` IPC; the new shell's `nexus.editor` uses CM6 and the transaction bridge per `editor-transaction-wiring-plan.md`, but that plan's Phases 0–8 are incomplete. Legacy likely uses simpler direct-writes against kernel IPC. Validate which path is active before declaring parity.

**Agent panel (7 handlers, all `partial`).** Agent plan/run/step/history UX renders in both shells, but the approval-loop and streaming UI in the new shell need validation against the legacy.

**Skills / Workflow browsers (8 handlers, `partial`).** Both render, but render-with-params (skills) and validate step (workflow) may not be fully ported.

**Plugin script loading (1 handler, `partial`).** Shell loads via dynamic `import()` with no sandbox; legacy loads via `read_plugin_script` + evaluator. Both are insecure (UI F-8.1.1). Gate on sandbox work in Phase 3.

**Bases granular IPC (1 handler `partial`).** Shell uses finer-grained IPC (`base_load`, `base_view_*`, `base_record_*`) instead of legacy's single `db_apply_view`. Architecturally cleaner but a higher surface area to validate.

### Tier 3 — only-shell wins (net-new capability)

**Canvas (Phases 1–6 complete, `only-shell`).** 500+ nodes at 60fps, drag/drop, minimap, auto-layout, PNG/SVG/PDF export, rich node embeds. Legacy has no canvas view.

**Bases (Phases 1–6 complete, `only-shell`).** Virtualized 50k-row table, Kanban, List, Calendar, Gallery, Timeline views, formula evaluator. Legacy has `BaseFileView.tsx` but nothing like the shell's database model.

**Notion block UX (Phases 1–6 complete, `only-shell`).** Slash menu, block selection, drag handles, input rules, inline toolbar. Shell's editor is meaningfully better than legacy's.

**Graph (local + global, `only-shell`).** Force-graph visualization with zoom/pan/drag. Legacy doesn't have a graph panel.

**MCP host management (`only-shell`).** Servers, tools, resources, prompts browser + tool call modal. Legacy has kernel MCP server (`nexus mcp`) but no UI to manage external MCP host connections.

**Pane mode / launcher / confirm / processes plugins (`only-shell`).** UX features that didn't exist in legacy — fullscreen-single-pane, Obsidian-style workspace launcher, shared confirm modal, unified processes view.

**Generic `kernel_invoke` / `kernel_subscribe` bridge (`only-shell`).** The shell replaces 95 bespoke Tauri commands with one generic `kernel_invoke(plugin_id, cmd, args)` plus `kernel_subscribe(topic_prefix)`. This is a meaningful architectural win — new services and handlers don't require any Tauri-host changes at all.

### Tier 4 — architectural-diff (decision required)

**Layout presets (`architectural-diff`).** Legacy has named `get_default_layout` / `list_layout_presets` / `get_layout_preset` for Obsidian / Vibe / Dev styles. Shell uses Leaf tree with no preset concept. If any users rely on presets, reintroduce them as named Leaf snapshots.

**Ribbon vs. activity bar (`architectural-diff`).** Legacy's vertical ribbon (left of workspace) maps to the shell's activity bar, but the contribution API differs. Minor.

**Menu bar vs. palette-first (`architectural-diff`).** Legacy has plugin-contributed `MenuBar` items; shell has no menu bar. Probably fine to drop, but confirm.

---

## What this does NOT change

The earlier recommendation (adopt `shell/`, retire `app/`) stands. The comparison here sharpens the cost of the migration, not the decision:

- The new shell is **ahead** on the highest-leverage features (Canvas, Bases, Notion blocks, Graph, MCP management) — features that legacy will never have without a rewrite.
- The new shell's architecture (generic `kernel_invoke`, one plugin per capability, `ExtensionHost`-mediated contributions) is strictly more extensible and more secure than legacy.
- The gaps in Tier 1 (AI, theme, keybindings, URI, saved-commands) are all feature ports where the backing kernel IPC already exists — so Phase 2 is frontend work, not kernel work.

## Revised Phase 2 migration plan (concrete)

Based on the detailed comparison, the parity-migration Phase 2 from the earlier roadmap becomes this ordered list of 12 work-items:

1. **AI chat + RAG plugin** (`nexus.ai`) — port ChatPanel from legacy; wire existing event forwarders. *~2 weeks.*
2. **Theme engine plugin** — redesign brand-neutral; re-wire `com.nexus.theme` IPC. *~1–2 weeks.*
3. **Editor transaction wiring validation** — complete `editor-transaction-wiring-plan` Phases 0–8; confirm undo/redo paths. *~1 week.*
4. **Agent panel completion** — approval-loop UI, streaming event subscriptions. *~1 week.*
5. **Skills / Workflow render-with-params UI** — finish partial flows. *~0.5 week.*
6. **Keybindings override UI** — port `HotkeysTab`. *~0.5 week.*
7. **Saved terminal commands** — sub-view of `nexus.terminal`. *~0.5 week.*
8. **URI handler registry** — port `dispatch_uri` + `list_plugin_uri_handlers`. *~0.5 week.*
9. **Layout preset reintroduction (if needed)** — named Leaf snapshots. *~0.5 week.*
10. **Bases granular IPC validation** — smoke-test all 13 base_* handlers. *~1 week.*
11. **Canvas / Graph live-data validation** — confirm Phases 1–6 work against live kernel. *~1 week.*
12. **Persistence migration script** — read legacy `layout-persistence.json` and write equivalent `shell-state.json`. *~0.5 week.*

**Total:** ~10–12 weeks for one engineer, consistent with the earlier 6–10 week estimate for Phase 2 (the slightly wider range accounts for AI + theme being bigger than previously scoped).

## How to consume this

- **Daily work:** open [`Shell-Capability-Comparison.xlsx`](./Shell-Capability-Comparison.xlsx), filter `Parity = only-app`, sort by category. That's your Phase 2 ticket backlog.
- **Pre-v1 decision points:** the 5 `architectural-diff` rows need explicit calls (layout presets, ribbon vs. activity bar, menu bar vs. palette-first, two minor others).
- **Security gates:** partial rows involving plugin loading / capability listing block any public marketplace; tracked in Phase 3 (security hardening).

---

## Cited sources

- `crates/nexus-app/src/lib.rs` — `generate_handler![...]` block (95 commands)
- `crates/nexus-app/src/{commands, persistence, forge, plugins, keybindings, editor, ai, agent, skills, workflow, database, terminal, uri}.rs`
- `app/src/{stores, components, plugins, ipc, contributions, editor, bindings}`
- `shell/src-tauri/src/{lib, bridge, persistence}.rs` (15 Tauri commands)
- `shell/src/main.tsx` — plugin load sequence (32 `nexus.*` + 4 `core.*`)
- `shell/src/plugins/{core, nexus, community}` — plugin directories
- `shell/src/{host, workspace, registry, stores, shell}` — shell substrate
- `docs/{canvas-shell-plan, bases-shell-plan, notion-block-ux-plan, editor-transaction-wiring-plan, leaf-architecture, shell-kernel-bridge-plan}.md` — phase-progress source of truth
