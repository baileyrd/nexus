# What the Nexus Shell Could Borrow From Tolaria's UI

A companion to `nexus-borrowings-from-tolaria.md`, narrowed to the desktop shell. Tolaria has solved several real shell-level problems that Nexus hasn't tackled yet, plus a few patterns whose value transcends the plugin/non-plugin divide.

---

## High-value borrowings

### 1. Browser-mode dev with a `mockInvoke` shim

The single highest-leverage borrowing.

Tolaria's `isTauri()` branch + `src/utils/mock-tauri.ts` lets `pnpm dev` run the full UI in a browser at `localhost:5173` against in-memory fake data — no Rust rebuild, no Tauri window, no PTY, just Vite hot-reload.

For Nexus this is *more* valuable, not less:

- Plugin authors could iterate on UI contributions without spinning up the kernel.
- The empty-shell guarantee means the mock layer only needs to fake `api.kernel.invoke` (a single typed function), not the entire backend.
- A `MockKernel` that responds to a dictionary of `{ pluginId, commandId } → fixture` would unlock fast plugin-author iteration and component testing in Storybook-like fashion.

**Sketch:**

```ts
// In api.kernel implementation:
const isTauri = '__TAURI_INTERNALS__' in window
const invoke = isTauri ? tauriInvoke : mockKernelInvoke

// mockKernelInvoke reads from a registered dictionary, falls back to
// a "not implemented" warning. Each plugin registers its own fixtures.
```

### 2. Live-evaluated `.enabled` / `when` predicates on commands

Tolaria re-evaluates each command's enable predicate every render — "Pull from remote" disables itself when there's no remote, "Resolve conflicts" only appears mid-conflict, and so on.

Nexus has a more declarative `when`-clause story via context keys (better in principle), but the explorer's report didn't show evidence the command palette currently *evaluates* `when` clauses to filter results. Adding that — `commands.list({ context: contextKeyService.snapshot() })` — would dramatically improve palette signal-to-noise as the plugin ecosystem grows.

### 3. Wikilink stack depth

The single most polished UX pattern in Tolaria, and it does not depend on the non-plugin architecture.

Tolaria threads wikilinks through *every layer*: BlockNote schema, CM6 DOM handlers (`inlineWikilinkDom.ts`, `inlineWikilinkTokens.ts`), fuzzy-matching autocomplete, click-routes into Neighborhood mode.

For `nexus.editor`, an equivalent treatment would mean:

- A CM6 widget for inline wikilink rendering.
- A slash-command-like autocomplete on `[[`.
- A click handler that emits a kernel event other plugins can subscribe to (e.g. `nexus.editor.wikilinkClicked`).
- A "neighborhood" view contributed by `nexus.graph` that listens for that event.

Cross-plugin choreography that just falls out of the contribution model.

### 4. Neighborhood mode

Worth its own item. Tolaria's pattern: clicking an entity in the note list pivots the list into "this note's relationship neighborhood" — pinned source row, outgoing relationship groups first, inverse/backlink groups after, with a Cmd+[/]-style history stack.

Nexus has a knowledge graph (petgraph) and backlinks already; a neighborhood-style Leaf view would expose graph data through a relationship-first UI rather than a node-link diagram, and would slot into the Phase-6 Leaf workspace as a new view type. This is *richer* than a backlinks panel and lighter-weight than a graph view.

### 5. MCP-driven UI from outside (WS 9711-style)

Tolaria's WebSocket UI bridge lets MCP tools running in a separate process reach *into* the running UI: `open_note`, `highlight_editor`, `set_filter`. Nexus's MCP server is data-only right now.

Concrete tools to add: `nexus_focus_panel`, `nexus_open_in_leaf`, `nexus_pulse_block`, `nexus_set_filter`. The shell listens for UI broadcasts and dispatches commands by id; the plugin contribution registry already supports the receiving end. This makes the AI agent a peer of the user, not just a backend.

---

## Medium-value borrowings

### 6. Co-located component tests + Vitest

Nexus's testing is currently `node --test` at the unit level + WebdriverIO E2E (1/3 specs passing as of April 2026).

Tolaria's pattern — `Component.tsx` + `Component.test.tsx` siblings, with specialized variants (`*.keyboard.test.tsx`, `*.behavior.test.tsx`) — gives a lot of regression coverage at the component layer. For a plugin-first shell this is doubly valuable: every core plugin's view component should have a test that mounts it with a mock `PluginAPI` and verifies its slot contributions. Vitest has nicer ergonomics for component testing than `node --test`.

### 7. Linux window chrome (custom titlebar + AppImage WebKit fix)

`LinuxTitlebar.tsx` + `LinuxMenuButton.tsx` mount conditionally via `shouldUseLinuxWindowChrome()`, with `data-tauri-drag-region` for drag, 8 edge + 4 corner resize handles, and `WEBKIT_DISABLE_DMABUF_RENDERER=1` injection for AppImage builds on Fedora/Wayland.

In Nexus this naturally becomes a `core.linux-titlebar` plugin that registers into the `titleBar` slot when the platform matches. Not glamorous, but Linux daily-use without it is rough.

### 8. Localization runtime

`src/lib/i18n.ts` is small (just `translate(locale, key, values)`) but the discipline around it — every string flows through it, browser language detection on first launch, Lara CLI keeping locales in sync — is easier to add early than retrofit.

For Nexus, the natural shape is `api.i18n.t(key, values)` exposed through the `PluginAPI` so plugins inherit the same machinery. A community plugin written in Spanish should feel native.

### 9. Window-state persistence with monitor-reattach handling

Tolaria stores window position/size in *logical* points, migrates older physical-pixel state on read (Retina vs non-Retina), and clamps to currently-available monitor work areas on restore. Standard polish, but easy to miss until users complain about "Nexus opens off-screen after I undocked."

### 10. Drag-and-drop reordering with `@dnd-kit`

Tolaria uses `@dnd-kit/core` + `@dnd-kit/sortable` for sidebar type reordering, favorites reordering, etc. Nexus's activity-bar items are priority-sorted at registration time; user-customizable order via dnd-kit would feel more native. Same applies to the workspace pane tree and kanban columns in Bases.

### 11. Onboarding hooks

Tolaria has a careful first-launch UX: detect missing CLI agents (Claude Code, Codex), show install links, dismiss locally so the prompt doesn't repeat. Nexus's first-run is currently terse (`nexus forge init`).

For the desktop shell specifically, a `core.onboarding` plugin that detects missing keyring entries, prompts to set up MCP integration, and walks the user through their first plugin install would lower the bar significantly.

---

## Low-value or already-handled

### 12. Resizable panel handles

Tolaria's `ResizeHandle` is fine, but Nexus's Phase-6 Leaf workspace already handles this — Nexus's pane-tree model is more flexible. Not a borrowing.

### 13. Three-mode editor (BlockNote / CM6 raw / diff)

Tempting to copy, but BlockNote's commercial license model and the round-trip complexity (`editorRawModeSync`) are real costs. Nexus's pure-CM6 approach with an extension-composable editor is cleaner for an extensibility platform — a plugin could provide a BlockNote-style rich view as an *alternative* view, but it shouldn't be the default.

### 14. Strict hook-locality / no global state

Tolaria's discipline (ADR 0026) is right for Tolaria and wrong for Nexus. Nexus *needs* shared stores (slot registry, theme, context keys) because plugins need to coordinate. Tolaria can avoid them because the app is monolithic. Do not import this philosophy.

### 15. System `git` CLI + Pulse view

Already covered in the broader borrowings doc; the Pulse panel is a natural fit as a `core.pulse` plugin or a `nexus.git` view contribution.

---

## Patterns to borrow philosophically, not literally

### 16. "Build the dev experience first"

Tolaria's `pnpm dev` browser mode, Husky pre-commit hooks, CodeScene gates, codecov, dual Vitest/Playwright suites, mock-tauri shim — all of this is in service of making the UI loop fast and safe. Nexus has the harder architectural problem (plugin host) but Tolaria has the more disciplined dev-loop. The plugin-first model amplifies the value of fast iteration: every plugin author benefits from `pnpm dev` working without Rust.

### 17. "Conventions reduce config"

The pattern that makes Tolaria's frontmatter automatic — standard field names trigger UI behavior — translates directly to Nexus's plugin contribution model.

A documented spec covering:

- standard slot ids,
- standard event topic prefixes,
- standard context-key namespaces,
- standard activity-bar conventions,

…means plugins compose without negotiation. The `PluginAPI` has the *mechanism* but not always the *vocabulary*.

### 18. "Polish is platform-specific"

Linux titlebar, monitor reattach, AppImage WebKit, dual-arch macOS — Tolaria's polish is a series of platform-aware fixes, none of them visible in the architecture diagram but all visible in the daily-use experience. The plugin-first shell can absorb these as platform-specific core plugins (`core.linux-chrome`, `core.macos-window-state`), which is actually a *cleaner* place for them than scattered conditionals.

---

## If I had to pick three for the shell

If I were shipping the next Nexus shell milestone and could only pull three things from Tolaria:

1. **Browser-mode dev with `mockInvoke`.** Biggest force multiplier on plugin-author velocity, costs almost nothing.
2. **Wikilink stack depth (CM6 widget + autocomplete + click-route + neighborhood view).** Biggest UX win for the core "knowledge graph" claim.
3. **Co-located component tests with Vitest + a `MockPluginAPI`.** Biggest leverage on shell stability before the plugin ecosystem grows.

The MCP-driven UI tools (`nexus_focus_panel`, `nexus_open_in_leaf`, `nexus_pulse_block`) are a close fourth, especially since Nexus is positioning itself as an MCP integration point.
