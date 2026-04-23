# E2E-T2-03 — Seed `.forge/skills/` fixture for Tier-2 skills spec

**Status**: open
**Opened**: 2026-04-23
**Context**: follow-up from Tier-2 testability pass (shell/ commit `8a4ce44`, PR #1).
**Unblocks**: 1 `it.skip` in [shell/e2e/specs/tier2/skills.spec.ts](../../../shell/e2e/specs/tier2/skills.spec.ts) — "expanding a skill row reveals a body preview".

## Scope

- Add `shell/e2e/fixtures/vault/.forge/skills/sample.skill.md` containing:
  - YAML frontmatter with `name`, `description`, at least one `tag`, and a `version`.
  - A multi-line body (≥5 lines) so the truncate-to-40 preview path renders.
- Un-skip the spec. Assertion flow:
  1. `SkillsPage.openPanel()`; `SkillsPage.refresh()`.
  2. Wait for `skillCount() === 1`.
  3. Click the `role="button"[aria-expanded="false"]` row header.
  4. Wait for the header to flip to `aria-expanded="true"`.
  5. Assert `[aria-label="Skill body preview"]` (or `[data-testid="skill-body-preview"]`) is visible and its text includes a line from the fixture body.

## Non-goals

- Additional test coverage for collapse-on-re-click or per-row keyboard navigation — separate follow-up.

## Selectors (already landed)

| Element | Selector |
| --- | --- |
| Body preview `<pre>` | `[aria-label="Skill body preview"]`, `[data-testid="skill-body-preview"]` |
| Row header | `[role="button"][aria-expanded]` |
