# BL-067 + BL-068 — Shell View Builder & Theme Builder
_Captured: 2026-05-06_

Two authoring tools that close the gap between what Nexus can render and what
a non-code user can create. Both are shell plugins that produce artifacts the
rest of the system already knows how to consume.

---

## BL-067 — Shell View Builder

### The idea

The Nexus shell is built entirely from plugin contributions — every panel,
sidebar, and pane is a registered extension loaded by `ExtensionHost`. Today,
arranging those contributions requires editing TypeScript. The View Builder
exposes that composition layer as a visual, drag-and-drop tool inside the
shell itself.

The output is a **layout definition file** (`.forge/layouts/<name>.layout.toml`
or a shell plugin's `manifest.toml` contribution block) that describes which
plugin panels occupy which positions at what split ratios. The shell already
reads this structure at boot; the builder is purely an authoring surface over
it.

### What the user can do

- **See the live layout** rendered as an editable canvas alongside the actual
  shell — move panels by dragging, resize splits by dragging dividers
- **Add plugin contributions** from a searchable palette of registered panels
  (all `contributes.views` entries from loaded plugins)
- **Configure panel options** — default width/height, minimum size, whether
  it floats or is docked, which side of a split it occupies
- **Name and save layouts** — "Focus", "Research", "Dev" — switch between them
  from the command palette
- **Export as a plugin contribution block** — the saved layout becomes a
  redistributable shell plugin anyone can install

### How it fits the architecture

- The builder is a shell plugin under `shell/src/plugins/nexus/viewBuilder/`
  — it has no backend IPC handlers of its own
- It reads the current layout state from the `ExtensionHost` via an existing
  JS API and writes changes back via the same path
- Saved layouts are stored as forge files (`.forge/layouts/`) so they're
  version-controlled and forge-portable
- No new kernel-side work needed; the layout engine already supports dynamic
  reconfiguration

### What makes this hard

- The current layout engine may not expose a full introspection API (what's
  currently rendered where); that needs to be added first
- Drag-and-drop panel placement in a live shell requires careful z-index and
  pointer-event management to avoid conflicts with the panels themselves
- "Export as plugin" requires a code-generation step that writes a valid
  `manifest.toml` and `index.ts` contribution block — doable but needs a
  template

### Relationship to existing work

- BL-053 (forge visual target) — the visual target defines *what* renders;
  this defines *where* it renders
- BL-054 (Nexus OS Mode) — the OS architecture panel (Phase 2) would be one
  of the panels a user could place via the view builder
- ADR 0011 (plugin-first shell) — this is the authoring layer for that system;
  it proves the invariant

---

## BL-068 — Theme Builder

### The idea

Nexus themes are TOML files that override a 400+ variable CSS token system
(`--nx-{category}-{property}-{variant}`). Authoring one by hand means editing
hundreds of variables with no visual feedback until you reload. The Theme
Builder is a visual editor inside the shell that renders live previews as you
adjust tokens, enforces contrast ratios, and exports a valid `.theme.toml`.

The theme system already has live reload (file-watcher in `nexus-theme`
triggers a hot CSS push). The builder just closes the loop: instead of editing
a file externally and watching it reload, you edit in the builder and the
preview updates on every change.

### What the user can do

- **Token palette** — grouped by category (Surface, Text, Accent, Border,
  Editor/Syntax, Shadow) with color pickers, sliders for opacity/size, and
  font inputs for typography tokens
- **Live preview** — a split view showing the actual shell rendering against
  the token edits in real time; preview pane shows a representative forge
  document (headings, code blocks, tables, callouts, wikilinks)
- **Base theme selector** — start from any installed theme and override;
  only changed tokens are written to the output file (delta model, not a full
  copy)
- **Contrast checker** — per-token WCAG AA/AAA pass/fail against its likely
  background; surface pairs auto-detected from the token naming convention
- **Light/dark synchronization** — if the theme supports both modes, show them
  side by side and propagate hue/saturation adjustments proportionally
- **Export** — writes a valid `.theme.toml` to `.forge/themes/<name>/` and
  immediately activates it; the file can be shared or published as a plugin

### How it fits the architecture

- Shell plugin under `shell/src/plugins/nexus/themeBuilder/`
- Reads installed themes via `com.nexus.theme` IPC (handler: `list_themes`,
  `get_theme`)
- Writes token edits via a new `com.nexus.theme::preview_override` handler —
  an in-memory token overlay that takes effect without touching any files,
  cleared on cancel
- On "Save", dispatches to `com.nexus.storage::write_file` to persist the
  TOML — same path the file-watcher picks up for live reload
- `scripts/check_ipc_drift.sh` required for the new handler

### What makes this hard

- **Token count** — 400+ variables is a lot to make navigable. Good grouping,
  search, and a "changed only" filter are essential; without them the UI is
  unusable
- **Preview fidelity** — the preview needs representative content across every
  visual surface (editor, sidebar, modal, code block, table, callout). A
  static preview document seeded at builder open is the simplest path
- **Contrast pairing** — auto-detecting which token pairs need contrast
  checking requires knowledge of the CSS layout (which text token sits on
  which surface token). A static pairing manifest (authored once) is more
  reliable than dynamic inference
- **`preview_override` handler** — the theme system needs to support an
  ephemeral in-memory override layer that takes priority over the active
  theme without mutating any files. This is a new concept in the theme engine

### Relationship to existing work

- PRD-07 (theming) — the builder is the authoring tool for the system PRD-07
  specifies
- BL-053 (forge visual target) — the target look defined there is exactly what
  a user would use the theme builder to replicate or extend
- `nexus-ember-dark` / `nexus-ember-light` — the bundled themes become the
  natural starting points for the builder's base theme selector

---

## Sequencing

Neither builder is blocked on unshipped infrastructure — both can start from
what exists today. The one prerequisite that isn't obvious:

- **Theme Builder needs `preview_override`** — a new in-memory token overlay
  handler in `nexus-theme`. This is a 1-day addition before the UI work can
  start.
- **View Builder needs layout introspection API** — the `ExtensionHost` needs
  to expose a read-only snapshot of the current contribution layout as a
  JS-accessible structure. Currently implicit; needs to be made explicit.

Relative effort:
- Theme Builder: 1 week (0.5d backend `preview_override`, 4d UI, 0.5d export)
- View Builder: 1.5–2 weeks (1d introspection API, 5–7d drag-drop UI, 1d
  export-as-plugin template)

Both are UI-heavy, non-blocking, and independently shippable. Theme Builder
first is the natural order — it's lower risk and the visual feedback it
provides would itself be useful while building the View Builder.

---

## BL-067 Phase 0 — Layout Introspection API (shipped 2026-05-14)

The 1-day prerequisite called out in §Sequencing has shipped. The View Builder
plugin already in tree (`shell/src/plugins/nexus/viewBuilder/`) previously
walked `workspace.layoutSnapshot()` directly to render its canvas; that
covered the live workspace tree but left the chrome-slot inventory and the
view-type catalog unexposed. Phase 0 closes those two gaps.

### What shipped

- **`shell/src/host/layoutSnapshot.ts`** — `getLayoutSnapshot(pluginRegistry?)`
  returns a JSON-safe `LayoutSnapshot { slots, viewTypes, extensions, layout,
  takenAtMs }`. `globalSnapshot()` is a convenience accessor that uses the
  registry singleton bound at boot via `bindPluginRegistry(reg)` (wired in
  `shell/src/main.tsx`).
- **`SlotRegistry.snapshot()`** — emits one `SlotEntrySnapshot { id,
  pluginId, priority }` per registered chrome contribution. The React
  `component` reference is intentionally dropped (not serialisable, builder
  doesn't invoke creators directly).
- **`viewRegistry.registeredTypes()` / `registeredExtensions()`** — read-only
  inventory of every view-type creator and every `ext → viewType` binding,
  surfaced through `PluginAPI` so plugin code reaches them the same way it
  reaches `register()` / `update()`.
- **`countLeavesInLayout(json)`** — utility that walks the workspace tree
  (splits, tabs, floating windows) so the builder's status line can render
  "N leaves" without re-implementing the walk.
- Tests at `shell/tests/layoutSnapshot.test.ts` (re-exporting
  `src/host/layoutSnapshot.test.ts`).

### Acceptance criteria (met)

- ✅ Snapshot is JSON-stringify-able end-to-end (no React refs, no Maps).
- ✅ Every registered slot key is present in the snapshot even when empty.
- ✅ View-type ownership resolves through `PluginRegistry.ownerOfViewType`
  when a registry is bound; shell built-ins (`empty`) report `pluginId:
  null`.
- ✅ Newly-registered view-types and slot entries surface on the next
  snapshot call (point-in-time projection, no caching).
- ✅ Typecheck + node:test green.

### Deferred (Phase 1+)

Phases 1+ — drag-drop palette UI, export-as-plugin codegen template,
`.forge/layouts/<name>.layout.toml` round-trip — remain on the backlog. The
existing `nexus.viewBuilder` plugin renders the live canvas today but does
not yet consume the new slot/viewType inventories or write a layout file.
