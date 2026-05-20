# Search and Navigation

Plugins that help the user find their way around a forge — full-text search, palette navigation, document outline, and link-graph inspectors. The category mixes two distinct concerns: global query surfaces (`search`, `searchPanel`, `semanticSearch`, `commandPalette`, `launcher`, `pick`, `recall`) and per-document graph views (`outline`, `graph`, `backlinks`, `outgoingLinks`, `tags`). Most graph plugins overlap heavily — they all read from `com.nexus.storage::backlinks` / `outgoing_links` and decorate one file at a time. Only `search` and `searchPanel` are load-bearing for the basic-capability scope.

### search

- **Path:** `shell/src/plugins/nexus/search/`
- **Surface:** sidedock view `search` (basename query against `com.nexus.storage::search`), command `nexus.search.focus`, keybinding `Ctrl/Cmd+Shift+F`, configuration `search.maxResultsLimit`. Hits emit `files:open` for the editor.
- **Depends on:** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`; reads `com.nexus.storage::search`.
- **Verdict:** Essential
- **Rationale:** Global text search is in the basic-capability scope. This plugin is the only producer of an in-sidebar full-text search surface.

### searchPanel

- **Path:** `shell/src/plugins/nexus/searchPanel/`
- **Surface:** sidedock view `search-panel` (multi-file find/replace), command `nexus.searchPanel.focus` (also bound to `Ctrl/Cmd+Shift+F` — collides with `nexus.search`; keymap resolution decides which wins per build). Wraps `com.nexus.storage::find_in_files` / `replace_in_files`.
- **Depends on:** `com.nexus.storage` IPC only.
- **Verdict:** Essential
- **Rationale:** Find-in-files / replace-in-files is the workspace-search complement to the sidebar search; together they cover the basic-capability "search across the forge" requirement. Note the keybinding collision with `nexus.search` — both register `Ctrl/Cmd+Shift+F`.

### semanticSearch

- **Path:** `shell/src/plugins/nexus/semanticSearch/`
- **Surface:** one palette command `nexus.semanticSearch.run` ("Search by Meaning"). Prompts for a query, fans out to `com.nexus.storage::search` and `com.nexus.ai::semantic_search` in parallel, merges per `merge.ts`, opens the top hit via `files:open`.
- **Depends on:** `com.nexus.storage`, `com.nexus.ai` (requires an embedding provider configured).
- **Verdict:** Optional
- **Rationale:** AI/embedding-backed feature; needs configured providers. Keyword search alone satisfies the basic scope.

### launcher

- **Path:** `shell/src/plugins/nexus/launcher/`
- **Surface:** overlay views `nexus.launcher.view` (recents picker) and `nexus.launcher.remoteDialog` (SSH connect modal). Drives `nexus.workspace.open` / `openWithTemplate` / `openRemote` / `setRoot`. No commands of its own.
- **Depends on:** `nexus.workspace`.
- **Verdict:** Useful
- **Rationale:** Not strictly required — users can open a forge via the underlying `nexus.workspace.open` command — but it is the only first-run UI for picking a forge before any window content exists. Without it a fresh install drops the user into an empty shell with no obvious entry point.

### commandPalette

- **Path:** `shell/src/plugins/nexus/commandPalette/`
- **Surface:** overlay view `nexus.commandPalette.overlay`, commands `nexus.commandPalette.open` / `close`, keybindings `Ctrl/Cmd+Shift+P` and `Ctrl/Cmd+P`, context key `nexus.commandPalette.visible`, configuration `commandPalette.maxResultsLimit`.
- **Depends on:** core command registry only.
- **Verdict:** Useful
- **Rationale:** A second command-palette implementation alongside `core.command-palette` (`shell/src/plugins/core/commandPalette/`). Both bind `Ctrl/Cmd+Shift+P` and both register an overlay view. Whichever activates last wins the binding; the duplication is technical debt that should resolve to one plugin. For the basic scope, *one* palette is Essential — but which of the two is kept is a follow-up decision, not a removal.

### pick

- **Path:** `shell/src/plugins/nexus/pick/`
- **Surface:** overlay view `nexus.pick.modal`. Backs `api.input.pick(...)` — a generic list-picker used by other plugins (terminal cross-search, workflows, etc.). No commands.
- **Depends on:** none directly; consumed via `PluginAPI.input.pick`.
- **Verdict:** Useful
- **Rationale:** Pure infrastructure modal — other plugins lazy-import `requestPick`. Without it, any plugin that calls `api.input.pick(...)` falls back to nothing. Not user-facing on its own; whether it is needed depends on which feature plugins remain.

### recall

- **Path:** `shell/src/plugins/nexus/recall/`
- **Surface:** overlay view `nexus.recall.overlay`, commands `nexus.recall.open` / `close`, keybinding `Ctrl/Cmd+Shift+R`, context key `nexus.recall.visible`, configuration `recall.hotkey`. Semantic-searches capture-note inbox via `com.nexus.ai::semantic_search`; inserts a quote snippet at the editor caret or copies it.
- **Depends on:** `nexus.ai`; implicitly `nexus.memory` for the inbox scope filter.
- **Verdict:** Optional
- **Rationale:** AI-backed convenience layered on top of the `nexus.memory` quick-capture flow. Not in the basic scope.

### outline

- **Path:** `shell/src/plugins/nexus/outline/`
- **Surface:** right-panel tab "Outline" (view type `outline`), command `nexus.outline.focus`. Subscribes to `editor:activeHeadingChanged` and the editor runtime's `sessionManager.onChanged`. Derives headings from `kernelClient.getTree(relpath)` (canonical BlockTree), with a `parseHeadings(tab.content)` fallback.
- **Depends on:** `nexus.rightPanel`; reads from `nexus.editor` runtime.
- **Verdict:** Optional
- **Rationale:** A typing aid, not required to browse / edit markdown. The first registered right-panel tab if present, which makes it discoverable, but the basic workflow doesn't break without it.

### graph

- **Path:** `shell/src/plugins/nexus/graph/`
- **Surface:** right-panel tab "Graph" (view type `graph`). Calls `com.nexus.storage::outgoing_links` + `backlinks` for the active file, merges into a neighbour list, renders a force-directed local graph. Also contains a separate `GraphGlobalView` for whole-forge graph (unwired from manifest — only the per-file graph is registered).
- **Depends on:** `nexus.rightPanel`; reads from `com.nexus.storage`.
- **Verdict:** Optional
- **Rationale:** Knowledge-graph visualisation. Not in the basic scope. Overlaps heavily with `backlinks` and `outgoingLinks` — same kernel handlers, different presentation.

### backlinks

- **Path:** `shell/src/plugins/nexus/backlinks/`
- **Surface:** right-panel tab "Backlinks" (view type `backlink`), command `nexus.backlinks.focus`. Reads `com.nexus.storage::backlinks` (and `backlinks_to_block` when a block filter is active) for the active file; supports block-anchored filtering via `BL-049` fragments.
- **Depends on:** `nexus.rightPanel`; reads from `com.nexus.storage`; reads `nexus.editor` runtime for change events.
- **Verdict:** Optional
- **Rationale:** Knowledge-graph inspector; an Obsidian-style feature. Not required to edit or save markdown. Shares its data source with `graph` and (partially) `outgoingLinks`.

### outgoingLinks

- **Path:** `shell/src/plugins/nexus/outgoingLinks/`
- **Surface:** right-panel view type `outgoing-links`, command `nexus.outgoingLinks.focus`. Reads `com.nexus.storage::outgoing_links` for the active file; renders a flat clickable list. Resolved links emit `files:open`; unresolved appear muted.
- **Depends on:** `com.nexus.storage`; uses `nexus.files`' `kernelClient` directly (slight layering leak — the `getKernel()` import reaches across plugin boundaries).
- **Verdict:** Optional
- **Rationale:** Companion to `backlinks`. Same not-in-basic-scope rationale. Overlaps with `graph` (same `outgoing_links` source) and could plausibly be merged into a single "Links" tab.

### tags

- **Path:** `shell/src/plugins/nexus/tags/`
- **Surface:** right-panel view type `tags`, command `nexus.tags.focus`. Reads frontmatter via `com.nexus.storage::read_frontmatter`, then `query_tags` per tag to find co-occurring files. Renders expandable per-tag lists.
- **Depends on:** `com.nexus.storage`; uses `nexus.files`' `kernelClient` (same layering leak as `outgoingLinks`).
- **Verdict:** Optional
- **Rationale:** Tag-navigation feature. Not in the basic scope. Overlaps conceptually with `backlinks` / `graph` (all are per-file graph inspectors).

## Category verdict

| Plugin           | Verdict   | Notes                                                       |
|------------------|-----------|-------------------------------------------------------------|
| search           | Essential | Sidebar full-text search.                                   |
| searchPanel      | Essential | Find/replace in files; shares `Ctrl/Cmd+Shift+F`.           |
| semanticSearch   | Optional  | Needs AI + embeddings.                                      |
| launcher         | Useful    | Only first-run forge-picker UI.                             |
| commandPalette   | Useful    | Duplicates `core.command-palette`; collision to resolve.    |
| pick             | Useful    | Infrastructure modal for `api.input.pick(...)`.             |
| recall           | Optional  | AI semantic recall over capture-notes.                      |
| outline          | Optional  | Document headings panel.                                    |
| graph            | Optional  | Per-file local link graph.                                  |
| backlinks        | Optional  | Per-file backlinks list.                                    |
| outgoingLinks    | Optional  | Per-file outgoing-links list; overlaps with `graph`.        |
| tags             | Optional  | Per-file tag co-occurrence list.                            |
