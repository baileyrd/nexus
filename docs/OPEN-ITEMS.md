# Open Items — Post-Migration Carryover Gaps

> Capabilities described in legacy `app/` documentation that were not carried over to `shell/` during the Phase 4 WI-37 retirement (2026-04-24). Surfaced by the capability-presence sweep on 2026-04-24.
>
> Listed here rather than in [PRDs/BACKLOG.md](PRDs/BACKLOG.md) because these are regressions from prior-shipped behavior, not new features. Linked from BACKLOG.md under "Post-migration carryover gaps."

---

## OI-01 — Settings modal / `registerSettingsTab` API

**Severity:** Should-fix (user-visible parity gap)
**Surfaced by:** `docs/references/obsidian-settings-modal.md`
**Status:** Not started

### Gap
The Obsidian-style tabbed settings modal with a plugin-extensible `registerSettingsTab` API is entirely absent in `shell/`.

- `shell/src/plugins/core/settings/SettingsPanelView.tsx` exists but is a single-panel view, not a tabbed modal with a plugin extension point.
- Zero hits for `SettingsModal`, `registerSettingsTab`, or `workspace.settings` across `shell/src/`.
- Legacy `app/src/contributions/builtins.ts` registered settings tabs declaratively; no equivalent contribution registry entry exists.

### Scope
- Design an extension-point contribution (`settings.tabs` or similar) aligned with the existing contribution-registry pattern (see `shell/src/registry/`).
- Build a tabbed modal view. Register a default set of tabs for bundled plugins (appearance, keybindings, file-system, AI providers, etc.).
- Expose `api.settings.registerTab(...)` on the plugin API surface.
- Persist last-open tab in shell state.

### Acceptance
- A plugin can declare `contributions.settings` in its manifest and have a tab appear in the settings modal.
- Default bundled tabs cover appearance, keybindings, and any plugin-level settings that existed in the legacy shell.
- `Ctrl/Cmd+,` opens the modal to the last-viewed tab.

---

## OI-02 — Split-size persistence

**Severity:** Should-fix (UX regression)
**Surfaced by:** `docs/superpowers/specs/2026-04-17-split-size-persistence-design.md`
**Status:** Not started — the design spec was written before `app/` retirement and never implemented in `shell/`.

### Gap
Pane-splitter positions are not persisted across reloads. Users drag a sidebar wider, reload, and the split reverts.

- `shell/src-tauri/src/persistence.rs` `get_shell_state`/`save_shell_state` has no `split_sizes` field.
- `shell/src/stores/workspaceStore` / `shell/src/workspace/` — zero hits for `splitSizes` or `split_sizes`.
- Frontend splitter components (sidebar, right panel, bottom panel, editor splits) do not report drag-end to a persistence layer.

### Scope
- Extend the shell-state schema with a `split_sizes: Record<string, number[]>` keyed by split ID.
- Emit a debounced save from splitter components on drag-end.
- Hydrate on boot, fall back to defaults if the key is missing.
- See the original design doc (now bannered as historical) for the proposed schema; reconfirm before implementing since naming conventions may have drifted.

### Acceptance
- Drag any pane splitter, reload — position restored.
- Clean uninstall (`~/.nexus-shell/` deleted) falls back to default layout without error.

---

## Relation to BACKLOG.md

These items are cross-listed in [PRDs/BACKLOG.md](PRDs/BACKLOG.md) under "Post-migration carryover gaps (2026-04-24)" with pointers back here. This file is the authoritative description; BACKLOG.md is the index.
