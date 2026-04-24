# Split-size persistence — design

> **Historical document** — Written before the `app/` → `shell/` migration (Phase 4 WI-37, 2026-04-24). See `docs/legacy-shell-retirement.md`. The schema touchpoints below (`crates/nexus-app/src/persistence.rs`, `app/src/ipc/persistence.ts`, `app/src/stores/layout.ts`) no longer exist.
>
> ⚠ **Gap (2026-04-24):** the feature this spec describes is not yet implemented in the new shell. `shell/src-tauri/src/persistence.rs` persists `get_shell_state` / `save_shell_state` / `read_last_forge_path` / `write_last_forge_path` / `forget_forge_path`, but has no `split_sizes` field; `shell/src/workspace/persistence.ts` likewise has no split-size channel. Porting this design into the plugin-first shell remains open work — needs user confirmation on whether to keep this spec as the target design or redesign against the new `WorkspaceRenderer` / `workspaceStore` tree.

**Context:** PRD-07 "Layout persistence" gap. Tab list persistence landed in a prior wave, but split-pane proportional sizes are held in-memory only. `app/src/stores/layout.ts` has the comment: *"cross-session persistence of split sizes is a separate binding-schema change."* This spec closes that gap.

## Goal

After dragging a split divider, quitting, and reopening the app, splits come back at the user's chosen ratios.

## Scope

Per-preset storage of split proportional sizes, keyed by split pane id. Tree structure is still owned by the preset; only numeric sizes are persisted.

**Out of scope:** drag-to-reorder tabs, drag-to-split, pinned tabs UI, responsive breakpoints, platform chrome — separate PRD-07 items.

## Schema

### Rust (`crates/nexus-app/src/persistence.rs`)

Add one field to `PersistedLayoutState`:

```rust
#[serde(default)]
pub split_sizes: BTreeMap<String, Vec<f32>>,
```

- **Key:** split pane id (e.g. `"pane-main-split"`) as declared in the preset.
- **Value:** proportional sizes array whose length matches the split node's `children.length`.
- `#[serde(default)]` keeps existing `layout-state.json` files loading unchanged — **no `version` bump**.

### TypeScript (`app/src/ipc/persistence.ts`)

Mirror the field on `PersistedLayoutState`:

```ts
splitSizes?: Record<string, number[]>;
```

Optional so a freshly-loaded persistence blob without the field is valid.

## Save path

Touch points in `app/src/stores/layout.ts`:

1. **`extractState(layout)`** — extend to walk the pane tree and collect `(splitId → sizes)` for every `Split` node. Today it only captures side-panel fields.
2. **`setSplitSizes(paneId, sizes)`** — currently only mutates in-memory `layout.root`. Add:
   ```ts
   const persistence = updatePersistence(state.persistence, layout);
   scheduleSave(persistence);
   ```
   `scheduleSave` is the existing 500ms debounce used by `toggleSidePanelCollapsed` and `activatePanel`, so burst dragging collapses to one IPC call.

## Restore path

`mergePersistedState(layout, state)` walks the preset-derived tree. For each `Split` node:

- If `state.splitSizes[node.id]` exists **and** `splitSizes[node.id].length === node.children.length`, replace `node.sizes`.
- Otherwise (missing entry, arity mismatch from a preset edit, or absent field on legacy files): leave the preset default.

Matches the forgiveness model already used for unknown pane ids in tab restore.

## Tests

**Rust (`persistence.rs`):**
- Round-trip: set `split_sizes`, save, load, assert equal.
- Legacy load: JSON without `splitSizes` deserializes cleanly (add alongside the existing `legacy_file_without_forge_path_loads` pattern).

**Frontend (`layout.ts` unit test):**
- Merger applies matching-arity sizes.
- Merger drops entries whose arity doesn't match the preset tree (fall back to preset default).

## Non-goals / YAGNI

- No migration code — `serde(default)` handles old files.
- No min-size clamp in persistence — `SplitPane` enforces 80px at interaction time.
- No debounce tuning — existing 500ms is proven by side-panel saves.
- No preset-id collision handling — `PersistedLayoutState` is already keyed per preset in `LayoutPersistence.layouts`.

## Files touched

- `crates/nexus-app/src/persistence.rs` — schema field + tests
- `app/src/ipc/persistence.ts` — TS type mirror
- `app/src/stores/layout.ts` — `extractState`, `mergePersistedState`, `setSplitSizes`

## Risks

- **Stale ids after preset edits.** Mitigated by the length-match guard; the worst case is reverting to preset defaults.
- **Size drift from repeated float round-trips.** Sizes are proportional (not pixel), so rounding below `f32` precision is imperceptible; no special handling needed.
