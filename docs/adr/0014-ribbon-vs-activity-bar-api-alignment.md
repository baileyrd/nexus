# ADR 0014: Ribbon vs Activity Bar — API Naming Alignment

**Status:** Accepted
**Date:** 2026-04-23
**Deciders:** Project lead

## Context

The legacy `app/` shell used the term **ribbon** for the vertical strip
of tool/view-launcher icons on the left edge of the window. The Phase-2
plugin-first shell (`shell/`) uses the term **activity bar** for the
same concept, matching the broader industry vocabulary (VS Code,
Obsidian) and matching the contribution point already shipped under
that name in `@nexus/extension-api` during Phase 1 and Phase 2a.

Both terms refer to the same surface: a narrow column that hosts plugin-
registered icon entries which toggle or focus a side panel / view. The
two coexist in older docs and code comments; Phase 2 plan §5.5 (WI-17)
flags the naming drift and asks for an alignment decision.

## Decision

**`activityBar` is the canonical name across the codebase, the public
API, and all documentation going forward.** "Ribbon" is a deprecated
synonym, retained only in legacy migration documents to help readers
trace concepts from the old shell.

No code change is required: `@nexus/extension-api` already exports the
contribution point as `activityBar`. This ADR exists to lock the naming
so future contributors do not re-introduce "ribbon" out of habit or by
copying legacy patterns.

## Rationale

- **Already shipped.** Phase 1 and Phase 2a work landed the
  contribution point as `activityBar`. Renaming it would break every
  plugin currently registering against it.
- **Industry alignment.** "Activity bar" is the term used by VS Code,
  the most-cloned plugin host in the category, and by Obsidian's plugin
  docs. New plugin authors arrive expecting that term.
- **"Ribbon" is overloaded.** In Microsoft Office and similar apps,
  "ribbon" denotes a horizontal multi-tab toolbar — a different shape
  from a vertical icon strip. The legacy term invited confusion.
- **Single canonical name reduces grep tax.** Contributors hunting for
  "where do I register my icon?" should find one answer in one place.

## Consequences

- All public documentation (`packages/nexus-extension-api/README.md`,
  plugin-author guide, marketing copy) refers exclusively to the
  **activity bar**.
- Code comments, plugin-internal identifiers, and TS types use
  `activityBar` / `ActivityBar*`. Any lingering `ribbon`/`Ribbon`
  identifiers in active code paths should be renamed opportunistically
  (no dedicated cleanup phase required).
- Migration documents (those describing the move from the legacy `app/`
  shell to the plugin-first `shell/`) may mention "ribbon" once, paired
  with "(now: activity bar)", to help readers carry knowledge forward.
- Pull-request reviewers should reject new uses of "ribbon" outside the
  migration-doc context.

## Alternatives considered

- **Keep both names as aliases in the public API.** Rejected — two names
  for one concept is exactly the drift this ADR exists to prevent.
- **Rename `activityBar` back to `ribbon`.** Rejected — would break
  shipped plugins and contradict the dominant industry term.
- **Leave the docs ambiguous and let usage settle organically.**
  Rejected — naming drift compounds; locking it now costs nothing and
  saves recurring review cycles.

## References

- `docs/PHASE-2-IMPLEMENTATION-PLAN.md` §5.5 (WI-17)
- `packages/nexus-extension-api/README.md` — canonical API surface
- ADR 0011 — plugin-first shell adoption
