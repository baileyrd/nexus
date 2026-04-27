# Nexus vs. Tolaria — UI Architecture Deep-Dive

A side-by-side architectural look at the two desktop UIs. Both are Tauri 2 + Vite + React + TypeScript apps, but the *shape* of those UIs is fundamentally different: Nexus is a near-empty plugin host that materializes its UI from contributions at runtime; Tolaria is a hand-wired four-panel application with deep prop chains and no plugin boundary. Each design has distinct strengths.

## TL;DR

| | **Nexus shell** | **Tolaria** |
|---|---|---|
| Stance | **Empty shell** — every visible element is a plugin contribution | **Composed app** — App.tsx orchestrates four named panels |
| Top-level component | `App.tsx` reads slot stores; renders whatever is registered | `App.tsx` (~2,800 lines) wires Sidebar/NoteList/Editor/Inspector explicitly |
| Layout | Slot system + Leaf-based workspace renderer (Phase 6) with persisted pane tree | Fixed four-panel layout with `ResizeHandle` between fixed regions |
| State management | **Zustand** stores per concern (slots, theme, tabs, terminal) + `ContextKeyService` ambient KV | **Hook-local state**, closure-captured refs, no global store, no context (ADR 0026) |
| Extensibility | Plugin contribution registry: commands, views, keybindings, status-bar items, settings tabs, URI handlers, activity-bar items, context keys | None — extend by editing the React app |
| Editor | `nexus.editor` plugin wraps **CodeMirror 6** via `CodeMirrorHost` with composable extensions; transactions stream to kernel for undo | **BlockNote** rich + **CodeMirror 6** raw + **diff view**, three exclusive modes; debounced save round-trips through wikilink pre/post-processors |
| Multi-window | Architecturally supported; Phase-6 Leaf system + community-plugin iframes (sandbox) | Note windows via `WebviewWindow`; `isNoteWindow()` flag at boot routes between `App` and `NoteWindow` |
| AI surface | `nexus.ai` plugin renders chat panel; `api.kernel.invoke` to `com.nexus.ai` | `AiPanel` toggles with Inspector in right pane; streams over WS 9710, applies UI actions from WS 9711 |
| Theming | `useThemeStore` (Zustand) hydrated from kernel; CSS vars applied to `:root` | Internal light/dark runtime (ADR 0081); CSS vars + shadcn `.dark` class |
| Localization | Not yet | Lara CLI + JSON catalogs, app-owned i18n runtime (ADR 0087) |
| Tests | `node --test` + tsx loader; WebdriverIO + tauri-driver E2E (1/3 specs passing as of Apr 2026) | Vitest co-located `*.test.tsx` next to virtually every component (60+ files); Playwright smoke + integration |
| Lines of TS in UI | ~15k (the agent estimate from the previous comparison) | Substantially more (one component file alone is ~2,800 lines; ~150 components in `src/components/`) |

The cleanest one-liner: **Nexus's UI is a runtime — Tolaria's UI is a program.** Nexus says "the shell knows nothing; plugins draw the app." Tolaria says "the app is the app; conventions make it work for everyone."

---

## 1. Top-level shell layout

### Nexus

The shell lives in `nexus/shell/` as a hybrid pnpm workspace:

```
shell/
├── package.json                  # Vite + React + Zustand
├── src-tauri/                    # Rust bridge (Tauri commands, PTY, IPC routing)
│   └── src/bridge.rs             # invoke() dispatch into kernel plugins
├── src/
│   ├── main.tsx                  # creates PluginRegistry + ExtensionHost
│   ├── App.tsx                   # reads slot stores, renders contributions
│   ├── host/
│   │   ├── ExtensionHost.ts      # plugin lifecycle orchestrator
│   │   └── PluginRegistry.ts     # ownership-tracked registries
│   ├── plugins/
│   │   ├── core/                 # core.activity-bar, core.command-palette, core.terminal, …
│   │   ├── nexus/                # nexus.editor, nexus.files, nexus.bases, nexus.graph, nexus.ai, nexus.backlinks
│   │   └── community/            # WASM/iframe-loaded third-party plugins
│   └── stores/                   # Zustand: slots, theme, tabs, terminal
└── packages/
    └── nexus-extension-api/      # public TS contract for plugin authors (workspace:*)
```

The build is a deliberate two-phase step: `pnpm --filter @nexus/extension-api build && vite` — the API package compiles first so plugins can type-check against the latest contract.

### Tolaria

Tolaria is a single React project with a Tauri sub-project for the backend:

```
tolaria/
├── package.json                  # one project: React 19 + Vite 7 + Tailwind v4
├── src-tauri/                    # Tauri Rust backend
├── src/
│   ├── main.tsx                  # branches App ↔ NoteWindow on isNoteWindow()
│   ├── App.tsx                   # ~2,800 lines of orchestration
│   ├── NoteWindow.tsx            # minimal editor-only shell
│   ├── components/               # ~150 .tsx, each with sibling *.test.tsx
│   ├── hooks/                    # useVaultLoader, useEditorSave, useCliAiAgent, …
│   ├── lib/                      # i18n catalogs, locales/
│   ├── utils/                    # frontmatter, wikilinks, openNoteWindow, …
│   └── theme.json                # BlockNote theme tokens
└── tests/smoke/                  # Playwright specs
```

There is no plugin host, no contribution registry, no public API package. New features land as new components + new hooks + new Tauri commands.

---

## 2. State management

### Nexus — Zustand stores + ambient context keys

State is scoped per concern, with a strong preference for *non-shared* stores:

- `useSlotStore` — slot-name → `[{ component, priority, ownerId }]`. Mutating writes here trigger React re-renders inside `<SlotSurface>`.
- `useThemeStore` — `activeThemeId`, `resolvedVariables`. Hydrated from the kernel; `applyResolvedVariables()` writes to `document.documentElement.style`.
- `useTabsStore`, `useTerminalStore`, etc. — per subsystem.
- **`ContextKeyService`** — a singleton key-value store used as ambient state. Plugin A doesn't import plugin B's store; it reads `editorFocus`, `terminalVisible`, `shellReady` etc. via `api.context.get()`.
- **`EventBus`** — typed pub/sub for decoupled plugin-to-plugin coordination.

Two patterns matter here. First, **plugins never share JS objects** — they communicate by serialized events or by reading context keys. Second, **all plugin registrations are owner-tagged**; on unload, `registry.unregisterAll(pluginId)` sweeps every sub-registry (commands, views, keybindings, config, status-bar items, …) without per-plugin cleanup code.

### Tolaria — strict hook-locality (ADR 0026)

ADR 0026 is enforced: no Redux, no Zustand, no React Context. Verified by the explorer.

- The vault list (`VaultEntry[]`) lives in `useVaultLoader` and is passed *down* through props to consumers. There is no normalized store — children get the full entry.
- Filtered views are computed on every render via `useMemo` selectors.
- Mutations are routed through hooks that own a slice of behavior:
  - `useEntryActions` for archive/favorite/organize.
  - `useNoteActions` for create/delete/rename/type-change with conflict detection.
  - `useEditorSave` for 500ms-debounced writes.
  - `useAutoSync` for vault polling and external-edit detection.
  - `useCliAiAgent` for the WS bridge to the MCP server.
- Cross-component state (e.g., neighborhood drill-down stack) lives in refs at the App level and is passed down.

The trade-off: `App.tsx` becomes very large (the explorer measured ~2,800 lines), but each child has a small, predictable surface and re-render trees stay shallow because closure-captured callbacks don't change identity gratuitously.

---

## 3. The plugin host (Nexus only)

This is Nexus's central UI mechanism, so it's worth detailing.

`ExtensionHost.ts` runs a **two-pass loader**:

1. **Manifest registration.** Every plugin's manifest is read and pre-classified as *eager* (`activationEvents: []`, `'onStartup'`, or `'*'`) or *lazy*. Manifest-declared contributions (commands, keybindings) are pre-registered for *lazy* plugins so the command palette can show them before the plugin code runs. The activation triggers (`onCommand:X`, `onView:Y`) are recorded in an `activationTriggers` singleton.
2. **Activation.** Plugins are topologically sorted by `dependsOn`; core plugins sort before community plugins. `activate(api)` is awaited per plugin. Failures put the plugin into `'error'` state without retry, but don't poison the host.

A typical plugin (paraphrased from `core/activityBar/index.ts`):

```ts
export const activityBarPlugin: Plugin = {
  manifest: {
    id: 'core.activity-bar',
    core: true,
    activationEvents: ['onStartup'],
    contributes: { commands: [{ id: 'activityBar.toggle', title: 'Toggle Activity Bar' }] },
  },
  activate(api: PluginAPI) {
    api.views.register('activityBar', {
      slot: 'activityBar',
      component: ActivityBarView,
      priority: 0,
    })
  },
}
```

The shell exposes a fixed set of **slots** (`overlay`, `titleBar`, `activityBar`, `statusBarLeft`, `statusBarRight`, `paneMode`). `App.tsx` renders each slot via:

```tsx
<SlotSurface entries={slots.overlay} />
<SlotSurface entries={slots.activityBar} />
```

If no plugin registers into a slot, the slot renders empty — no errors. The README's "every visible element is a plugin contribution" claim holds: commenting out plugin imports yields a blank window.

Phase 6 of the shell is shifting layout from slot-based chrome to a **Leaf-based workspace renderer** — view containers (Leaves) arranged in a resizable pane tree persisted to `workspace.json`. Plugins register *view creators*; opening a Leaf instantiates the appropriate view by id. This is the same model VS Code uses for editor groups.

### Public extension API

Plugins import from `@nexus/extension-api`. The exported `PluginAPI` includes:

```ts
interface PluginAPI {
  commands:   { register, execute, all }
  views:      { register }
  workspace:  /* leaf-based facade */
  context:    { set, get, evaluate }   // context keys
  events:     { on, emit }              // typed pub/sub
  storage:    /* per-plugin localStorage */
  statusBar:  { createItem }
  settings:   { registerTab }
  configuration: { register, getValue, setValue, onChange }
  notifications: { show }
  fs:         { read, write, list, watch, exists, mkdir, delete, rename }
  kernel:     { invoke<T>, on<T>, available }
  platform:   { fs, dialog, window, shell }   // Tauri adapters
  activityBar:{ addItem, removeItem }
  uri:        { register }
  input:      { prompt, confirm }
  internal?:  { /* core-only escape hatch */ }
}
```

Core plugins additionally get `api.internal`; community plugins do not. That's the trust boundary.

---

## 4. Layout & composition

### Nexus — slot surfaces + Leaf workspace

`App.tsx` is small. It:

1. Reads `slots` from `useSlotStore`.
2. Waits for the `shellReady` context key (flipped to `true` after core plugins activate).
3. Hydrates the workspace layout from persisted JSON.
4. Installs a global keydown dispatcher that matches keybindings against context-key `when` clauses and executes commands.
5. Renders the activity bar, the workspace, and the overlay slot.

That's roughly the entire top-level. The visible UI emerges from registered plugins.

### Tolaria — explicit four-panel orchestration

`App.tsx` wires:

- **Sidebar** — fixed width (resizable), renders type sections, favorites, views, folder tree, inbox count. Drag-and-drop ordering via `@dnd-kit`. Selection (`SidebarSelection` discriminated union) drives NoteList content.
- **NoteList / PulseView** — width-resizable. NoteList shows filtered notes with `useMultiSelectKeyboard` for Shift+arrow / Ctrl+A / etc.; PulseView shows git-commit history grouped by day.
- **Editor** — flex region. Single note open at a time (ADR 0003 — no tabs; navigation history Cmd+[/]). BlockNote + CodeMirror 6 + diff view.
- **Inspector / AiPanel** — width-resizable, toggleable. Sparkle icon flips between modes.

The whole thing is glued together by panel-resize handles and `useMainWindowSizeConstraints`, which recomputes the OS-level minimum window size whenever the panel composition changes (collapsing the sidebar shrinks the floor; opening the inspector grows it back out).

---

## 5. The editor

### Nexus

`nexus.editor` is a plugin. Its `MarkdownView.tsx` wraps `CodeMirrorHost`, a generic CM6 wrapper. Extensions are composed in `cm/extensions.ts`:

- Markdown language (`@codemirror/lang-markdown`)
- Block selection
- Block handles
- Slash commands
- Inline toolbar
- A `transactionBridge` that serializes CM transactions to the kernel — **the kernel owns the undo tree** across saves, not the editor. This is unusual and powerful: undo persists across reloads, and other plugins can observe edit transactions through the event bus.

Inline AI completion would be implemented as a command + keybinding registered by `nexus.ai`, hooked into the editor by the kernel-streamed transaction model.

### Tolaria

The editor is the heart of the app, and it juggles three exclusive modes:

- **Block mode (default):** BlockNote rich-text editor with a custom `wikilink` block type.
- **Raw mode:** CodeMirror 6 view of the underlying markdown. Toggled via `useRawModeWithFlush`, which carefully syncs the latest CM doc back into BlockNote on exit.
- **Diff mode:** Side-by-side commit diff via `DiffView.tsx`, driven by `useDiffMode`.

`useEditorModeExclusion` enforces mutual exclusivity (entering raw exits diff, etc.). Saving:

1. User edits → `EditorContent` → `onContentChange`.
2. App-level `useEditorSave` debounces 500ms and calls Tauri `save_note_content`.
3. `useEditorSaveWithLinks` extracts wikilinks and updates the entry's `outgoingLinks` so backlinks stay accurate.

Wikilinks are threaded through the *whole* stack: BlockNote schema, CM6 DOM handlers (`inlineWikilinkDom.ts`, `inlineWikilinkTokens.ts`), fuzzy-matching autocomplete, click handlers that drill into Neighborhood mode. This depth is one of the things that makes Tolaria feel "native" rather than markdown-with-an-editor-bolted-on.

---

## 6. Sidebar / list / right panel

### Nexus

The activity bar (a thin icon strip) is rendered by `core.activity-bar`. Each item is a contributed activity-bar entry (`{ icon, title, viewId }`) registered by other plugins. Clicking switches the primary sidebar view. In the Phase-6 Leaf model, the sidebar is just another Leaf type and any plugin can supply a view creator.

There is no "note list" as a built-in concept — `nexus.files` provides one, and other plugins can ship alternative views (kanban from Bases, graph from `nexus.graph`, etc.).

### Tolaria

Each panel is a hand-written component with deep behavior:

- **Sidebar**: Inbox, Favorites, Types, Folders, Views (all draggable).
- **NoteList**: thin wrapper over `useNoteListModel`; supports multi-select keyboard, natural-language search via `useNoteListSearchState`, and **Neighborhood mode** — when a user Cmd+Clicks an entity, the list pivots into "this note's relationship neighborhood" with pinned source row, outgoing relationship groups first, inverse/backlink groups after. A history stack lets the user back out (Cmd+[).
- **PulseView**: parses `git log --name-status` into a daily commit feed, grouped by day with file-status icons.
- **Inspector**: frontmatter (editable), relationships, instances, backlinks, git history. Mutations route through Tauri commands like `update_frontmatter`, `add_property`, `delete_property`.
- **AiPanel**: chat with the selected CLI agent, action cards rendered as the agent calls tools, onboarding for missing agents.

Toggling Inspector ↔ AiPanel is a single boolean (`inspectorCollapsed`) at the App level.

---

## 7. Command palette

### Nexus

`core.command-palette` registers a view in the `overlay` slot. Commands come from every plugin's contribution; the palette is just a fuzzy filter over `api.commands.all()`. Since manifest-declared commands are *pre-registered* before activation, lazy plugins are discoverable from the palette and activate on invocation. This is exactly VS Code's pattern.

### Tolaria

`CommandPalette.tsx` (~400 lines) is opened by Cmd+K. Commands are registered through `useCommandRegistry` and grouped by domain in `src/hooks/commands/` (navigation, note, git, settings, AI). Each command exposes an `.enabled` getter that's re-evaluated *every render* against current state — so "Pull from remote" disables itself when there's no remote, "Resolve conflicts" appears only mid-conflict, and so on. Pressing `>` toggles the palette into an AI-prompt mode (vs. command-search mode).

There's no shared manifest file for shortcut routing in the way VS Code does it; ADR 0051 documents a deterministic shortcut routing matrix that's QA-tested via tests in `src/utils/shortcuts*` rather than declaratively registered.

---

## 8. AI surface

### Nexus

`nexus.ai` is a plugin that registers a chat panel view. Calls into the AI subsystem go through the kernel:

```ts
const stream = await api.kernel.invoke('com.nexus.ai', 'stream_chat', { messages, model })
```

The same path is used by community plugins (subject to capabilities). Inline-completion features hook in by registering keybindings + commands and writing transactions through the editor's transaction bridge.

### Tolaria

`AiPanel` shares the right pane with `Inspector`. Two WebSocket bridges back the AI flow:

- **9710 (tools)** — `useCliAiAgent` opens this when the panel mounts; the bridge proxies `search_notes`, `get_note`, `vault_context`, etc. into the bundled MCP server.
- **9711 (UI)** — `useAiActivity` listens here for *MCP-originated UI commands*. When the agent calls an `open_note` or `highlight_editor` tool, the broadcast comes back to the renderer, which applies it (open the note, pulse the highlight for 800ms, set the sidebar filter, …).

Streaming events are normalized into `TextDelta`, `ThinkingDelta`, `ToolStart`, `ToolDone`, `Done`, `Error` (verified in `ai_agents.rs`) so the panel can render reasoning blocks, tool action cards, and final responses uniformly across Claude Code and Codex adapters.

---

## 9. Tauri IPC

### Nexus

All plugin → kernel calls go through:

```ts
await api.kernel.invoke<T>(pluginId, commandId, args, timeoutMs?)
```

This funnels into a single Tauri `invoke()` that lands in `src-tauri/src/bridge.rs`, which routes to the appropriate plugin handler. Errors are wrapped in a typed `KernelIpcError`. Subscriptions use `api.kernel.on(topicPrefix, handler)`, with the registry tracking unsubscribes for automatic sweep on unload.

This uniformity is the load-bearing claim from the broader architecture: the CLI, TUI, and shell *all* use the same IPC path.

### Tolaria

Direct, ad-hoc Tauri invocations are made from hooks and components:

```ts
return isTauri()
  ? invoke<{ ok: boolean }>('save_note_content', request)
  : mockInvoke<{ ok: boolean }>('save_note_content', request)
```

There is no client SDK or codegen layer; each call is hand-written. The `mock-tauri.ts` shim makes the same code paths work in browser dev mode (`pnpm dev` at `localhost:5173`), which is unusually good for a Tauri app — it allows pre-Rust feature work without a full rebuild loop.

---

## 10. Multi-window

### Nexus

Multi-window is architecturally supported but not heavily used yet. Two pieces are notable:

- The Phase-6 Leaf model can host webview-panel views, allowing plugins to register webview content that runs in its own iframe.
- Community plugins run in **sandboxed iframes** (`SandboxOrchestrator`) — each iframe loads a minimal bootstrap runtime and the plugin bundle. Communication is `postMessage`-based; the router enforces capability checks at the API boundary; no shared memory.

That iframe isolation is strong — a misbehaving community plugin cannot crash the shell or read other plugins' state.

### Tolaria

Note windows are first-class. `openNoteInNewWindow(notePath, vaultPath, noteTitle)` creates a `WebviewWindow` with URL params (`?window=note&path=…&vault=…&title=…`). On boot, `main.tsx` checks `isNoteWindow()` and routes between `App` and `NoteWindow`:

- `App` is the four-panel main window.
- `NoteWindow` is a minimal shell that loads vault entries, fetches the note, applies the theme, and renders a single `Editor` instance.
- Tauri `capabilities/default.json` grants the same permissions to the `main` and `note-*` window labels, so backend commands work identically.

Each note window has its own auto-save loop and frontmatter-derived `VaultEntry` state, so the Inspector and note list react immediately without a full reload.

---

## 11. Theming & localization

### Nexus

`useThemeStore` (Zustand):

```ts
hydrate: async (api) => {
  const config = await api.kernel.invoke(THEME_PLUGIN_ID, 'get_theme_config', {})
  const variables = await api.kernel.invoke(THEME_PLUGIN_ID, 'compute_variables',
    { theme_id: config.theme_id, enabled_snippets: config.enabled_snippets })
  set({ activeThemeId: config.theme_id, resolvedVariables: variables, loaded: true })
  applyResolvedVariables()
}

applyResolvedVariables: () => {
  const root = document.documentElement
  Object.entries(get().resolvedVariables)
    .forEach(([name, value]) => root.style.setProperty(name, value))
}
```

The kernel owns theme state; the shell mirrors it. Community themes are loaded as theme packages and compiled to token cascades by the Rust `nexus-theme` crate. The shell listens for `com.nexus.theme.changed` events and re-hydrates.

No localization runtime yet.

### Tolaria

ADR 0081 defines an internal light/dark runtime: CSS variables in `src/index.css` (semantic colors, surfaces, borders), bridged to Tailwind v4 via `@theme inline`. A separate BlockNote theme (`src/theme.json`) is flattened to CSS vars by `useEditorTheme`. The runtime applies `data-theme` and the shadcn-compatible `.dark` class *before* React renders, with a localStorage mirror so dark mode doesn't flash on startup.

ADR 0087 defines the i18n runtime: `src/lib/i18n.ts` exports `translate(locale, key, values)`; locale catalogs live in `src/lib/locales/*.json`; Lara CLI keeps non-English locales in sync with `en.json`. Browser language is detected via `getBrowserLanguagePreferences()`. The user can override from Settings (the `ui_language` setting).

---

## 12. Linux window chrome

### Nexus

Not a focus area in the explorer's findings — the architecture would handle this via a `core.titlebar` plugin if needed.

### Tolaria

ADR 0079 ships custom React-rendered window chrome on Linux. `LinuxTitlebar.tsx` and `LinuxMenuButton.tsx` mount when `shouldUseLinuxWindowChrome()` returns true:

- `data-tauri-drag-region` on the title bar area enables dragging.
- Window controls (minimize, maximize, close) call Tauri window APIs.
- 8 edge + 4 corner resize handles call `startResizeDragging(direction)`.
- Height: `LINUX_TITLEBAR_HEIGHT = 32`; the App layout adds top padding accordingly.

For AppImage builds, `WEBKIT_DISABLE_DMABUF_RENDERER=1` is injected unless already set, working around Fedora/Wayland DMA-BUF crashes.

---

## 13. Tests

### Nexus

`node --import tsx --test tests/*.test.ts` for unit tests with the native Node test runner and tsx loader. End-to-end is WebdriverIO + tauri-driver (Windows only): the harness boots the app under WebDriver, navigates to the Tauri asset protocol URL, and attaches `window.__nexusShellApi` (gated on `VITE_E2E=true`) to drive CodeMirror, execute commands, and verify save/reload roundtrips. As of April 2026, 1 of 3 specs passes; the other two fail on product-level semantics (undo across save boundary, tab-close state).

### Tolaria

Vitest is the unit-test workhorse. Tests are co-located: most components have a sibling `*.test.tsx`, with specialized variants like `*.keyboard.test.tsx`, `*.behavior.test.tsx`, `*.coverage.test.tsx` for focused scenarios. The explorer counted ~63 test files in `src/components/` alone. `mock-tauri.ts` provides in-memory fake data so tests can exercise the same components that ship in production. Playwright drives smoke tests (`pnpm playwright:smoke`) and a separate integration config (`playwright.integration.config.ts`).

Plus husky pre-commit/pre-push hooks, codecov reporting, and CodeScene gates with ratcheted thresholds (ADR 0064).

---

## What stood out

A few patterns are genuinely distinctive enough to be worth naming.

**Nexus**

1. **Empty-shell guarantee.** Comment out plugins, get a blank window with no errors. The shell never assumes a plugin is loaded; every slot renders whatever is registered, including nothing. Inverse of typical Electron/Tauri apps that ship hardcoded chrome.
2. **Two-pass loader with manifest pre-registration.** Lazy plugins are discoverable from the command palette before they activate, because their manifest contributions are registered up front. The plugin only runs when invoked.
3. **Context keys as ambient KV state.** Plugin A reads `editorFocus` or `terminalVisible` without importing plugin B's store. Drives `when` clauses on commands, conditional rendering, and cross-plugin queries. Decoupling done right.
4. **Kernel-owned undo via transaction bridge.** CodeMirror transactions stream to the kernel, which owns the undo tree across reloads. Other plugins can observe edit events through the bus.
5. **Owner-tagged registrations + automatic sweep.** Every contribution carries its plugin ID; `unregisterAll(pluginId)` cleans every sub-registry on unload. Plugins almost never need a `deactivate()` hand-written cleanup.

**Tolaria**

1. **Three-mode editor with seamless round-trips.** BlockNote rich, CodeMirror raw, and diff view are mutually exclusive but flippable mid-session, and `editorRawModeSync` carefully preserves cursor and content across the transitions. Most apps pick one editor; Tolaria switches between three.
2. **Strict hook-locality with closure-captured refs.** ADR 0026 (no global state) is held to in practice. The cost is that `App.tsx` is large; the benefit is small re-render trees, easy forking for note windows, and zero "stale store" bugs.
3. **Wikilinks threaded through the entire stack.** Schema-level in BlockNote, DOM-level in CodeMirror, fuzzy-matched in autocomplete, and click-routed into Neighborhood mode. Internal linking feels native, not bolted on.
4. **Live-evaluated command enable predicates.** Every render re-evaluates `.enabled` for ~100+ commands. The palette never shows a stale capability. The cost is per-render work; the benefit is correctness.
5. **MCP-driven UI from outside.** A WS bridge (port 9711) lets MCP tools (running in Claude Code, Cursor, or another MCP client elsewhere on the machine) reach back *into* the running Tolaria UI to highlight elements, open notes, and set filters. The agent is a peer, not just a backend.

---

## When each approach wins

Pick **Nexus's UI architecture** when:

- You want a third-party plugin ecosystem.
- Different user personas need different UI surfaces over the same data (editor, kanban, graph, terminal — each as a plugin).
- You're willing to pay for a contribution registry, slot system, capability boundary, and IPC discipline up front.
- Cross-process or cross-language plugins (WASM, iframes) are on the roadmap.

Pick **Tolaria's UI architecture** when:

- One opinionated UX is the product, not a substrate.
- Iteration speed on the *built-in* features matters more than third-party extensibility.
- A small team needs to keep the whole UI in their head — strict hook-locality and props-down keep the mental model uniform.
- Polished platform-specific behaviors (Linux titlebar, AppImage workarounds, monitor-reattach window restore) need direct access to the React tree without going through a plugin abstraction.

The convergence is real: both use Tauri 2 + Vite + React + TypeScript + CSS-variable theming + IPC to a Rust backend. The divergence is where the *application* lives. In Nexus, the application is a contract plus a dozen plugins. In Tolaria, the application is `App.tsx`.
