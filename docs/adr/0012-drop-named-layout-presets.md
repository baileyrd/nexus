# ADR 0012: Drop Named Layout Presets in v1

**Status:** Rejected (the layout-presets feature is not shipped in v1)
**Date:** 2026-04-23
**Deciders:** Project lead

## Context

The legacy shell (`app/`) carried the concept of named "layout presets" —
canned arrangements such as **Obsidian**, **Vibe**, and **Dev** that the
user could pick from a switcher to reconfigure the tri-pane layout in one
gesture. That UX made sense when the layout was fixed and the presets
chose between a small set of pane mixes.

The Phase-2 plugin-first shell (`shell/`) replaces the hardcoded tri-pane
with a fully freeform Leaf tree (`docs/leaf-architecture.md`,
`leaf-migration-plan.md`). Users can split, drag, and re-nest panes
arbitrarily. The Phase-2 plan §5.5 (WI-15) asks the explicit question:
keep the named-preset concept on top of Leaf, or drop it?

## Decision

**Drop named layout presets from v1.** The new shell ships with no
`LayoutPreset` type in `@nexus/extension-api`, no preset registry in the
kernel, no preset picker in the settings panel, and no
`layout-presets/` directory in the user's config.

## Rationale

- **No demand signal.** We have no telemetry that ranks preset usage and
  no user-research evidence of demand. The legacy presets shipped because
  the legacy layout was rigid; the freeform Leaf tree removes the
  underlying constraint that made presets useful.
- **The Leaf tree is arbitrary.** Users can already arrive at any pane
  mix by direct manipulation. A named preset is a shortcut to a state the
  user can reach in seconds anyway.
- **API surface lasts forever.** Once `LayoutPreset` is in
  `@nexus/extension-api`, we own its serialization, migration, and
  cross-platform persistence indefinitely. Cheap to add later, expensive
  to remove.
- **Different problem, different shape.** If users later request "save
  my current layout and switch back to it later," the right answer is a
  `workspace.save_snapshot(name)` API, not a curated preset library.
  Snapshots are user-owned and don't require Nexus to ship a preset
  catalog.
- **Reversible.** This decision is easy to revisit in a later milestone
  if real demand appears.

## Consequences

- `@nexus/extension-api` does **not** export `LayoutPreset`,
  `registerLayoutPreset`, or related types.
- The kernel does not persist a `layout-presets/` directory under user
  config. Existing legacy preset files (if any survive in user data
  during migration) are ignored.
- The settings panel does not surface a "Choose layout preset" picker.
- Documentation (README, plugin-author guide, marketing copy) does not
  promise presets. Migration notes from the legacy shell explicitly call
  out their removal.
- If/when snapshot demand appears, it lands as a new ADR proposing
  `workspace.save_snapshot` — a different API with different semantics.

## Alternatives considered

- **Keep presets, rename them.** Rejected — the naming was not the
  problem; the abstraction is.
- **Ship one preset only ("Default").** Rejected — a preset list of one
  is dead UI; the default layout is just the default.
- **Introduce a `snapshot` API now in lieu of presets.** Rejected — no
  current demand. Speculative APIs land in extension-api and never leave.
  Wait for the user request, then design the API to fit it.

## References

- `docs/planning/PHASE-2-IMPLEMENTATION-PLAN.md` §5.5 (WI-15)
- `docs/leaf-architecture.md` — the freeform pane tree that obsoletes
  preset rigidity
- ADR 0011 — adoption of the plugin-first shell
