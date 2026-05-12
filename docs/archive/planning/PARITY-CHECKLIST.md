# Phase 2 Parity Checklist

> **Historical document** — Written before the `app/` → `shell/` migration (Phase 4 WI-37, 2026-04-24). Paths below reference the legacy `app/` and `crates/nexus-app/` tree that has since been deleted. For current locations see `docs/legacy-shell-retirement.md`.

**Created:** 2026-04-23
**Companion artifact:** [`Parity-Checklist.xlsx`](./Parity-Checklist.xlsx) — 23 work items, filterable/sortable with dropdowns for size/priority/phase/status.

This is the **ticket-ready backlog** for Phase 2 of the integration plan
(ADR 0011). Each row is a concrete migration unit with an acceptance
criterion, rather than a line in a comparison matrix. Use it directly as
your issue tracker seed or copy into whatever system you run.

---

## How it relates to other artifacts

- [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) — the audit that
  produced the 6-phase roadmap. **Phase 2 is implemented by this file.**
- [`adr/0011-adopt-plugin-first-shell.md`](../adr/0011-adopt-plugin-first-shell.md) —
  the decision this checklist executes on (Status: Accepted, 2026-04-23).
- [`SHELL-COMPARISON.md`](./SHELL-COMPARISON.md) + [`Shell-Capability-Comparison.xlsx`](./Shell-Capability-Comparison.xlsx) —
  the comparison matrix (115 rows). This checklist is the distilled
  ticket form of its `only-app` + `partial` + `architectural-diff` rows.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — the freeze policy that
  protects this backlog from drift.

---

## Rollup

| Metric | Value |
|---|---|
| Total work items | **23** |
| Total estimated weeks (one engineer) | **19.25** |
| P0 (ship-blocker) | 5 items · 5.75 weeks |
| P1 (high) | 7 items · 5.25 weeks |
| P2 (normal) | 8 items · 6.00 weeks |
| P3 (nice-to-have) | 3 items · 2.25 weeks |
| Phase 2a (leverage wave) | 9.50 weeks |
| Phase 2b (features) | 5.00 weeks |
| Phase 2c (polish + decisions) | 4.75 weeks |

The 19.25-week total lands in the 16–24-week estimate from
`INTEGRATION-REVIEW.md §5`. With parallel work, two engineers can
realistically cut this to ~10–12 calendar weeks.

---

## Phase structure

**Phase 2a — Leverage (do first).** The items that unblock everything
else. Port AI chat (the most user-visible gap), re-wire the theme
engine, finish the editor transaction model, wire `@nexus/extension-api`,
and add the structural guardrails that prevent regression.

**Phase 2b — Features.** Complete the remaining feature plugins that are
currently partial: agent, skills, workflow, bases (13 handlers),
canvas validation, terminal streaming upgrade.

**Phase 2c — Peripheral + decisions.** URI handlers, persistence
migration, and the three architectural-diff decisions (layout presets,
menu bar, ribbon vs. activity bar). These can slip without blocking v1.

---

## Work items by phase

### Phase 2a (9.5 weeks)

| ID | Title | Size | Priority |
|---|---|---|---|
| WI-01 | Port AI chat panel to shell (streaming + sessions) | L | **P0** |
| WI-02 | Re-wire theme engine (brand-neutral redesign) | L | P1 |
| WI-03 | Finish editor transaction wiring (Phases 0-8) | L | **P0** |
| WI-04 | Keybinding overrides UI (HotkeysTab) | S | P1 |
| WI-05 | Saved terminal commands sidebar | S | P2 |
| WI-06 | Generalize kernel_subscribe usage across streaming plugins | S | **P0** |
| WI-20 | @nexus/extension-api: regenerate types via ts-rs + wire | M | **P0** |
| WI-21 | File tree parity: legacy features (context menu, drag-drop) | S | P1 |
| WI-22 | Validation guardrail: prevent new `#[tauri::command]` in nexus-app | XS | **P0** |
| WI-23 | Validation guardrail: shell plugins don't import raw Tauri APIs | XS | P1 |

### Phase 2b (5.0 weeks)

| ID | Title | Size | Priority |
|---|---|---|---|
| WI-07 | Agent panel: approval loop + streaming progress | M | P1 |
| WI-08 | Skills browser: render with params | S | P2 |
| WI-09 | Workflow: list/get/reload/validate UI flow | S | P2 |
| WI-10 | Bases: validate granular IPC (13 handlers) | M | P2 |
| WI-11 | Canvas: live-data validation + kernel IPC pass | M | P2 |
| WI-12 | Terminal: upgrade from poll to streaming events | M | P2 |

### Phase 2c (4.75 weeks)

| ID | Title | Size | Priority |
|---|---|---|---|
| WI-13 | URI handler registry + dispatch_uri port | S | P2 |
| WI-14 | Persistence migration script (legacy → shell-state.json) | S | P1 |
| WI-15 | DECISION: Layout presets — keep as named Leaf snapshots? | M | P3 |
| WI-16 | DECISION: Menu bar — reintroduce or palette-only? | M | P3 |
| WI-17 | DECISION: Ribbon vs activity bar contribution API alignment | XS | P3 |
| WI-18 | Plugin capability listing surfaced in shell settings | S | P1 |
| WI-19 | Activation events (deferred plugin load) | M | P2 |

---

## How to use this checklist

**Daily work.** Open `Parity-Checklist.xlsx`, filter `Status = todo`,
sort by Priority ascending then Phase. Pick the top row you're qualified
to take; set `Owner` to your name and `Status` to `in-progress`.

**Sprint planning.** Pull items in ID order (which is already
priority-weighted). For a one-engineer week, aim for 1.0–1.5 in the
`Est wk` column.

**Progress reporting.** The `Rollup` sheet has a `Completion %` formula
that tracks done/total. Update status and it recomputes.

**New capability arriving?** Add a row. ID is `WI-<next>`. Fill the
service crate + kernel IPC columns; if no IPC exists, that's a P0
blocker task on the relevant service crate first. **Never** add a new
`#[tauri::command]` to `crates/nexus-app/` (see `CONTRIBUTING.md` and
WI-22 guardrail).

## Acceptance bar: when is Phase 2 done?

All 23 work items reach `Status = done` (or `P3 DECISIONs` are
explicitly closed via ADR). At that point:

- Every capability reachable in the legacy shell is reachable in
  `shell/`, verified by a side-by-side test run.
- `crates/nexus-app` has no `#[tauri::command]` additions beyond the
  pre-freeze baseline (95) — enforced by WI-22.
- Shell plugins import only from `@nexus/extension-api` — enforced by
  WI-23.

Then you're ready for Phase 3 (security hardening) per the integration
review roadmap.
