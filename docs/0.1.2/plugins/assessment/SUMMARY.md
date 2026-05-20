# Plugin Assessment — Summary

Aggregation of the 51 per-plugin assessments under this directory. Scope and verdict criteria are defined in [README.md](README.md).

## Verdict roll-up

### Rust core plugins (23)

| Verdict | Plugins |
|---------|---------|
| Essential | `security`, `storage`, `editor` |
| Essential-as-library | `formats` (compile-time dep of storage & editor; IPC surface itself is Optional) |
| Useful | `notifications`, `theme` |
| Optional | `database`, `terminal`, `git`*, `ai`, `ai-runtime`, `agent`, `skills`, `templates`, `workflow`, `comments`, `linkpreview`, `mcp`, `lsp`, `dap`, `acp`, `audio`, `collab` |

`*` — Note `git` lands as Optional at the Rust crate level because the basic capability "commit/push" is delivered through the shell-side `nexus.gitPanel`, which calls these handlers. The crate is Essential for the basic flow, but it surfaces purely as IPC handlers; nothing in the basic flow degrades if the crate is built but unused.

### Shell core plugins (17)

| Verdict | Plugins |
|---------|---------|
| Essential | `configurationService`, `fileSystemService`, `settings` |
| Useful | `capabilityPrompt`, `notificationService`, `themeService` |
| Optional | `zoom` |
| Removable (dead stubs not in `catalog.ts`) | `activityBar`, `commandPalette`, `editorArea`, `fileExplorer`, `panelArea`, `rightPanel`, `sidebar`, `statusBar`, `terminal`, `titleBar` |

**Only 7 of the 17 shell-core plugin directories are actually loaded at runtime.** The remaining 10 are Phase 7 template stubs left in the tree after the active implementations migrated to `shell/src/plugins/nexus/*`.

### Shell-nexus feature plugins (~65, by category)

| Category | Essential | Useful | Optional / Removable |
|----------|-----------|--------|----------------------|
| search-and-navigation | `search`, `searchPanel` | `launcher`, `commandPalette`, `pick` | `semanticSearch`, `recall`, `outline`, `graph`, `backlinks`, `outgoingLinks`, `tags` |
| ai-and-knowledge | — | `prompt` | `ai`, `agent`, `memory`, `enrich`, `dreamCycle`, `skills`, `linkSuggest` |
| files-and-properties | `files` | — | `fileProperties`, `allProperties`, `bookmarks` |
| editor-views | `editor` | `multibufferSync`, `paneMode` | `viewBuilder`, `canvas`, `notion` |
| collaboration | — | — | `collab`, `comments`, `crdtConflict` |
| git | `gitPanel` | `gitStatus` | — |
| diagnostics-and-observability | — | `diagnostics` | `debugger`, `healthPanel`, `observability`, `processes`, `osArchitecture`, `status` (not a plugin) |
| notifications | — | `notifications`, `notificationsInbox` | `notificationsSettings` |
| workspace-chrome | `workspace` | `sidebar` (stub), `rightPanel`, `statusBar`, `themePicker` | `activityTimeline` |
| extension-system | — | `pluginsMgmt`, `templates` | `workflow`, `bases` |
| interaction-and-io | — | `confirm` | `mcp`, `audio`, `terminal` |

## What "basic capabilities" actually needs

To deliver the basic-capability scope — open a forge, browse + edit markdown, search, commit via git — the following are load-bearing:

**Rust:** `nexus-types`, `nexus-plugin-api`, `nexus-kernel`, `nexus-plugins`, `nexus-bootstrap`, plus core plugins `security`, `storage`, `formats`, `editor`, and `git` (handlers consumed by the shell git plugin). Theme + notifications are not strictly required but the UX degrades.

**Shell core:** `configurationService`, `fileSystemService`, `settings`, `capabilityPrompt`, `notificationService`, `themeService`. (Plus the kernel bridge in `shell/src-tauri/`.)

**Shell nexus:** `workspace` (forge lifecycle), `files` (tree), `editor` (CM6 surface), `search` + `searchPanel`, `gitPanel`. Everything else can be lazy-loaded or omitted.

Everything outside that list is feature surface area.

## Notable findings

### Dead code

- **10 of 17 shell-core plugins are dead stubs** that no longer appear in `shell/src/plugins/catalog.ts`. They survive as filesystem clutter from the Phase 7 microkernel migration. Safe to delete after extracting two stragglers:
  - `core/editorArea/MarkdownDoc.tsx` exports a `Heading` type still imported by `shell/src/stores/docStore.ts:6`.
  - `core/panelArea/panelAreaStore.ts` exports `usePanelAreaStore` still imported by `core/terminal` (itself dead).
- **`nexus.sidebar` is itself a stub** kept alive only so ~10 other plugins' `dependsOn: ['nexus.sidebar']` declarations resolve. Both halves of the sidebar pair are effectively dead.
- **`shell/src/plugins/nexus/status/` is not a plugin.** It has no manifest and is not in `catalog.ts`. It's a frontmatter-status utility (`statusStore.ts`, `useFileStatus.ts`, `StatusPill.tsx`) consumed by `nexus.files` and `core/editorArea`; belongs in a shared lib directory, not `plugins/nexus/`.
- **`nexus.linkSuggest`** is a configuration-only shim — its actual logic lives in `editor/cm/linkSuggest.ts`. Borderline Removable as a standalone plugin.
- **`nexus.graph`** ships a full-forge `GraphGlobalView` that is dead code — the manifest only registers the per-file `GraphView`.
- **`nexus.multibufferSync`** has no consumers outside `nexus.editor` and could fold back into it.
- **`com.nexus.acp` has zero in-tree shell consumers.** Only exercised by tests and the `nexus acp serve` CLI subcommand.

### Architectural debt

- **Two command palettes**: `nexus.commandPalette` and `core.command-palette` both register `Ctrl/Cmd+Shift+P` and an overlay view. The dead `core/commandPalette/index.ts:7` still imports `DEFAULT_MAX_PALETTE_RESULTS` from `nexus/commandPalette/match` — an inverted core→nexus dependency.
- **Conflicting keybinds**: `nexus.search` and `nexus.searchPanel` both bind `Ctrl/Cmd+Shift+F` — last-registered wins.
- **`nexus.fileProperties` and `nexus.allProperties` are near-duplicates.** Both call `read_frontmatter`, differ only in chrome. Strong merge candidate.
- **Overlapping knowledge-graph plugins**: `outgoingLinks`, `tags`, `backlinks`, and `graph` all read `com.nexus.storage::backlinks` / `outgoing_links` for the active file. Could be one "Links" panel.
- **Layering leaks**: `outgoingLinks` and `tags` import `getKernel` from `../files/kernelClient` instead of going through `@nexus/extension-api`.
- **`nexus.crdtConflict` has no `dependsOn: ['nexus.collab']`** despite being dead weight without it.
- **`nexus.notion` is misnamed** — only wraps `com.nexus.formats::import_notion`/`export_notion`, no block-style editing.
- **Hidden coupling in shell-core**:
  - `core.settings` imports `PluginsMgmtInline` from `nexus.pluginsMgmt` without declaring `dependsOn`.
  - `core.zoom` reads from `api.configuration` without declaring `dependsOn` on configuration-service.
  - `core.notificationService` reaches directly into the `configStore` singleton instead of going through `api.configuration`.

### Documentation drift uncovered during the assessment

The following are bugs in **existing** docs/comments that we hit while researching:

- `docs/0.1.2/plugins/core.md:32` lists registration order as security → storage → formats → database → editor → terminal → git → ai → ai-runtime → … — the actual bootstrap order is **security → storage → database → editor → theme → ai-runtime → ai → skills → templates → formats → workflow → linkpreview → notifications → audio → comments → agent → mcp → lsp → dap → acp → git → terminal → collab.** `formats` is #10, not #3.
- The same doc claims "every core plugin embeds its `plugin.toml` as a string constant" (step 3 of "Authoring"). **No `plugin.toml` strings exist for core plugins.** Manifests are built inline by `core_manifest_with_ipc(...)` reading each crate's `IPC_HANDLERS: &[(&str, u32)]` slice.
- `crates/nexus-terminal/src/lib.rs:18` doc comment claims terminal "is **not** a core plugin yet — mirroring the positioning of `nexus-git`". Both are now registered core plugins.
- `crates/nexus-mcp/src/lib.rs` doc comment says "no IPC surface, no core plugin wrapper" — `McpHostPlugin` exists and exposes 12 handlers.
- `[digests]` and `[webhooks]` blocks in `<forge>/.forge/config.toml` (loaded by `nexus_bootstrap::load_digest_config` / `load_webhook_config`) are not documented in `docs/0.1.2/settings/forge-config.md`.

### Build-time gotchas

- `com.nexus.audio` defaults to the `local` backend, but the shipped build stubs it — first dispatch returns `BackendNotEnabled` unless built with the `local-audio` feature or operators flip to `provider`.
- `com.nexus.collab` is registered with `LifecycleFlags::NONE` — entirely request-driven, no background tasks.
- `LayoutManager` ships in `nexus-theme` but is not exposed through any `IPC_HANDLERS` entry; layout persistence still lives in the shell.

## Recommendations (for follow-up, not auto-applied)

1. Delete the 10 dead shell-core stubs after relocating the two leaked exports (`Heading` type, `usePanelAreaStore`). Move `shell/src/plugins/nexus/status/` to `shell/src/lib/status/` (or fold into `nexus.files`).
2. Pick a single command palette (`nexus.commandPalette` is the live one) and remove `core.command-palette`. Resolve the `Ctrl/Cmd+Shift+F` collision between `nexus.search` and `nexus.searchPanel`.
3. Merge `nexus.fileProperties` + `nexus.allProperties`. Consider consolidating `outgoingLinks` / `tags` / `backlinks` / per-file `graph` into a single Links panel; keep `graph` for the global view it already implements.
4. Fix the four doc/comment drifts called out above (bootstrap order, `plugin.toml` claim, `nexus-terminal` lib comment, `nexus-mcp` lib comment, missing `[digests]`/`[webhooks]` settings docs).
5. Add explicit `dependsOn` declarations for the hidden couplings (`core.settings → nexus.pluginsMgmt`, `core.zoom → core.configuration-service`, `nexus.crdtConflict → nexus.collab`).
6. Decide whether `com.nexus.acp` should ship in 0.1.2 — no in-tree consumer today.
