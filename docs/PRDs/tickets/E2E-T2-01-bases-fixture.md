# E2E-T2-01 — Seed `.bases/` fixture for Tier-2 database specs

**Status**: open
**Opened**: 2026-04-23
**Context**: follow-up from Tier-2 testability pass (shell/ commit `8a4ce44`, PR #1).
**Unblocks**: 3 `it.skip` blocks in [shell/e2e/specs/tier2/database.spec.ts](../../../shell/e2e/specs/tier2/database.spec.ts) — column sort, row count, cell edit commit/cancel.

## Scope

- Add `shell/e2e/fixtures/vault/.bases/tasks.base.toml` (or equivalent) with a known schema. Schema must include at least:
  - one sortable text field (e.g. `title`)
  - one number field (e.g. `priority`)
  - one checkbox field (e.g. `done`)
- Seed ≥3 records with deterministic IDs so `data-testid="record-row-{id}"` lookups are stable.
- Un-skip the three scenarios in `database.spec.ts`. Replace `// no-op` bodies with assertions against the existing selectors:
  - Column sort → click `[aria-label="Sort by title"]`; assert `aria-sort` cycles `none → ascending → descending → none`.
  - Row count → count elements matching `[data-testid^="record-row-"]`; assert it equals the seeded count.
  - Cell commit → double-click a cell, type into `[aria-label="Commit cell"]`, press Enter, assert the new value renders.
  - Cell cancel → double-click a cell, press Escape, assert the `[aria-label="Commit cell"]` element is gone and the prior value still renders.

## Non-goals

- Fake kernel adapter — the real `com.nexus.storage::base_*` surface is on by default in the e2e harness.

## Selectors (already landed)

| Element | Selector |
| --- | --- |
| Column header | `th[aria-label="Sort by {field}"]` + `aria-sort` |
| Row | `tr[role="row"][data-testid="record-row-{id}"]` |
| Inline editor | `[aria-label="Commit cell"]` |
