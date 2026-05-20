# Plugin Dependencies

Companion to the per-plugin assessments in this directory. Source data is in `_extract-rust-deps.md`, `_extract-shell-deps-A.md`, and `_extract-shell-deps-B.md`, derived by reading manifest builders, `ipc_call(...)` sites, `api.kernel.invoke(...)` sites, event subscriptions, cross-plugin imports, and per-crate `Cargo.toml` files directly.

## How Nexus expresses plugin dependencies (or doesn't)

| Tier | Declared deps mechanism | Cross-tier deps | Result |
|------|--------------------------|-----------------|--------|
| Rust core | **None.** `PluginManifest` (`crates/nexus-plugins/src/manifest.rs`) has no `dependsOn` field. `core_manifest_with_ipc(...)` (`crates/nexus-bootstrap/src/plugins/mod.rs:112`) emits only `[plugin]`, `[lifecycle]`, `[[registrations.ipc_command]]`. Boot ordering is hand-curated in `register_all` (`crates/nexus-bootstrap/src/plugins/mod.rs:44`). | N/A. | Every Rust→Rust dependency is implicit (via `ipc_call`, subscriptions, or Cargo path-deps). Zero machine-readable graph exists. |
| Shell core / shell nexus | `dependsOn: [...]` in the `registerExtension({...})` manifest call inside each plugin's `index.ts`. The shell-side `package.json` and `plugin.toml` may or may not duplicate it. `shell/src/plugins/catalog.ts` is the activation index. | `dependsOn` only accepts other **shell** plugin ids. There is no syntax for "needs Rust crate `com.nexus.ai`". | Every shell→Rust dependency is implicit (via `api.kernel.invoke(target_plugin_id, ...)` or `api.kernel.on(topic)`). Many shell→shell dependencies are also implicit (via direct TS imports across plugin directories). |

The practical consequence is that "what does this plugin require to function" cannot be answered from any single file. The tables below collate it from the four channels that *do* express dependencies (declared `dependsOn`, runtime IPC calls, event subscriptions, cross-plugin TS imports).

## Section 1 — Rust core plugins

23 plugins. Declared `dependsOn` is uniformly **not supported by manifest schema** so the column is omitted. "Runtime IPC targets" is the set of plugins this plugin invokes at runtime; absence does **not** mean nothing depends on it — see the inverse index in §4.

| Plugin | Runtime IPC targets | Event subscriptions | Compile-time `nexus-*` deps | Hard-required for it to function |
|--------|--------------------|--------------------|---------------------------|---------------------------------|
| com.nexus.security | none observed | none | kernel, plugins, types | self-contained |
| com.nexus.storage | none observed | `com.nexus.git.commit` (loose; no-op if git absent) | kernel, plugins, types, **database**, **formats** | none at runtime; **compile** needs database + formats |
| com.nexus.formats | none observed | none | plugins | self-contained |
| com.nexus.database | none observed | none | plugins, types | self-contained |
| com.nexus.editor | **storage**::{read_file, write_file, write_vault_file, delete_file, base_load}, **database**::apply_view | (publishes `com.nexus.editor.changed.*`, `com.nexus.editor.ops.*`) | formats, kernel, plugins, types | **storage**, **database** |
| com.nexus.terminal | **ai**::stream_chat (graceful degrade if absent) | (publishes `com.nexus.terminal.*`) | kernel, plugins, types | none (ai optional) |
| com.nexus.git | none observed | `com.nexus.storage.file_modified` (prefix, for dirty-poll wake-up) | kernel, plugins, **security**, types | security at compile time |
| com.nexus.ai | **storage** (12 handlers — read_file, write_file, search, backlinks, query_files/blocks/symbol, entity_get/search/upsert, vector_*), **git**::log, **terminal**::run_saved/get_session_info/send_raw_input, **mcp.host**::call_tool/list_servers/list_tools | `com.nexus.storage.file_*` (prefix, indexing daemon) | kernel, plugins, types, security, **ai-runtime** | **storage**; soft: git, terminal, mcp.host |
| com.nexus.ai.runtime | **agent**::session_run; dynamic dispatch (any plugin) | `com.nexus.ai.stream_chunk`, `com.nexus.agent.round_proposed` | kernel, plugin-api, plugins, types | **agent**, **ai** (cycle — see §6) |
| com.nexus.agent | **storage**::write_vault_file/list_dir, **ai.runtime**::submit/wait_for, **ai**::entity_recall/propose_tool_calls, **storage**::entity_search, **skills**::triggered_by/compose/render, **mcp.host**::list_servers/list_tools, **notifications**::send; **dynamic dispatch** (any plugin via tool calls) | `com.nexus.agent.session_completed` (own) | kernel, plugins | **storage**, **ai**, **ai-runtime**, **skills**, **notifications** |
| com.nexus.skills | **agent**::session_run | none | kernel, plugins | **agent** (cycle — see §6) |
| com.nexus.templates | none observed | none | plugins | self-contained |
| com.nexus.workflow | **storage** (list_dir/read/write/create_dir), **terminal** (saved_list/run_saved/list_sessions/close_session), **ai**::ask, **ai.runtime**::submit, **notifications**::send, **self**::run/run_digest, **dynamic dispatch** (any plugin via step args) | `com.nexus.storage.file_*` (prefix), `com.nexus.git.*` (prefix), `com.nexus.mcp.*` (prefix) — all gated by per-workflow trigger spec | kernel, plugins, types | **storage** (mandatory); rest by workflow content |
| com.nexus.comments | none observed | none | plugins | self-contained |
| com.nexus.linkpreview | none observed | none | plugins | self-contained |
| com.nexus.notifications | none observed | `com.nexus.ai.runtime.*` (prefix, delivery transports), `com.nexus.notifications.*` (own inbox fanout) | kernel, plugin-api, plugins, types | self-contained (ai-runtime sub is graceful) |
| com.nexus.theme | none observed | `com.nexus.theme.*` (own fanout) | kernel, plugins | self-contained |
| com.nexus.mcp.host | none observed in core plugin | none | kernel, plugins, types | self-contained at plugin level (heavy fan-out lives in the MCP **server binary** frontend) |
| com.nexus.lsp | none observed | (publishes `com.nexus.lsp.*`) | kernel, plugins | self-contained |
| com.nexus.dap | none observed | (publishes `com.nexus.dap.*`) | kernel, plugins | self-contained |
| com.nexus.acp | dynamic dispatch only (target id supplied by JSON-RPC request; typical: agent) | (publishes `com.nexus.acp.*`) | kernel, plugins | none at compile; runtime depends on what callers ask |
| com.nexus.audio | **ai**::resolve_credentials (only when `provider` backend in use) | none | kernel, plugins, security | none required; `provider` backend needs ai for creds |
| com.nexus.collab | none observed | `com.nexus.editor.ops.*` (outbound relay prefix), `com.nexus.collab.*` (inbound republish), `com.nexus.collab.presence` | kernel, plugins | none required at compile or boot; needs **editor** publisher to be useful |

Boot order rationale (from `register_all` comments at `crates/nexus-bootstrap/src/plugins/mod.rs:53-82`): **security first** (the vault/audit log), **storage** second (other plugins do not call into the network bus during `on_init`, but file-as-truth must exist before anyone reads), **ai-runtime before ai** (shared tokio worker pool handle is created in runtime and reused by ai), **collab last** (relay bridge subscribes to topics published by every other plugin, so loading it after them gives a complete snapshot).

## Section 2 — Shell core plugins (live only)

The 10 stub plugins (`activityBar`, `commandPalette`, `editorArea`, `fileExplorer`, `panelArea`, `rightPanel`, `sidebar`, `statusBar`, `terminal`, `titleBar`) are not in `catalog.ts` and have no dependencies in either direction.

| Plugin | Declared `dependsOn` | Runtime kernel IPC | Cross-plugin imports | Hidden couplings (used but not declared) |
|--------|---------------------|---------------------|----------------------|------------------------------------------|
| core.capabilityPrompt | none | `com.kernel::set_plugin_granted_capabilities`, `revoke_plugin_capability` | `../../nexus/pluginsMgmt/capabilityInfo` | `nexus.pluginsMgmt` (TS import), kernel handlers |
| core.configurationService | none | indirect (configStore reads forge `app.toml`) | none (uses shell-internal `stores/configStore`) | every plugin consuming `api.configuration` |
| core.fileSystemService | none | none (delegates to `api.platform.fs`); direct `@tauri-apps/plugin-fs::watch` | none | hard dep on `@tauri-apps/plugin-fs` (allowlisted per WI-23) |
| core.notificationService | none | none | none | reads `configStore` singleton instead of `api.configuration` |
| core.settings | `core.configuration-service`, `nexus.activityBar` | none | `../../nexus/pluginsMgmt/PluginsMgmtView` | **`nexus.pluginsMgmt`** (TS import, used for inline plugin manager) |
| core.themeService | none | `com.nexus.theme::*` (via `useThemeStore.hydrate`) | none | **`com.nexus.theme`** (kernel-side) — undeclared because schema cannot express cross-tier |
| core.zoom | none | none | none | consumes `api.configuration` (ambient) without declaring `core.configuration-service` |

## Section 3 — Shell nexus plugins (live)

`shell/src/plugins/nexus/status/` is NOT a plugin (no manifest, not in `catalog.ts`); it's a misfiled shared utility consumed by `nexus.files` and `core/editorArea`. Excluded from the table.

Column legend: **D** = declared `dependsOn` (shell plugin ids only). **K** = kernel/Rust IPC targets (`com.nexus.*`). **X** = cross-plugin TS imports (structural couplings). **H** = hidden couplings (D missing one of K/X).

| Plugin | D — declared `dependsOn` | K — kernel IPC targets | X — cross-plugin imports | H — hidden? |
|--------|---|---|---|---|
| nexus.activity (activityTimeline) | not investigated | not investigated | not investigated | not investigated |
| nexus.activityBar | none | none | none | many other plugins call `api.activityBar.addItem` without declaring it as a dep |
| nexus.allProperties | not investigated | not investigated | not investigated | not investigated |
| nexus.backlinks | not investigated | not investigated | not investigated | not investigated |
| nexus.bases | `nexus.workspace` | `com.nexus.storage::base_*` (12 handlers), `com.nexus.database` | none | kernel storage+database deps (schema can't express) |
| nexus.bookmarks | not investigated | not investigated | not investigated | not investigated |
| nexus.canvas | `nexus.workspace` | `com.nexus.storage::*`, `com.nexus.linkpreview::*`, `com.nexus.terminal::*` | `../editor/blockRefDrag`, `../editor/markdownRender` | **`nexus.editor` (X), `com.nexus.linkpreview`, `com.nexus.terminal` (K)** |
| nexus.collab | `nexus.workspace`, `nexus.activityBar` | `com.nexus.collab::relay_status` | none | kernel collab dep (schema can't express) |
| nexus.commandPalette | none | none | none | consumes every plugin's command contributions — implicit |
| nexus.comments | not investigated | not investigated | not investigated | not investigated |
| nexus.confirm | none | none | none | `api.input.confirm` host-side lazy-imports this plugin's store |
| nexus.crdtConflict | none | `com.nexus.editor::apply_transaction` (in resolver modal) | none | **does not declare `nexus.collab`** despite being dead weight without it |
| nexus.diagnostics | `nexus.paneMode`, `nexus.activityBar` | `com.nexus.editor::open_excerpts`; subs `com.nexus.lsp.textDocument.publishDiagnostics` | `../workspace/workspaceStore`, `../editor/kernelClient`, `../editor/cm/lspIpc`, `../editor/cm/lspToExcerpts` | **`nexus.editor`, `nexus.workspace` (X); `com.nexus.editor`, `com.nexus.lsp` (K)** |
| nexus.dreamCycle | `nexus.paneMode`, `nexus.activityBar` | `com.nexus.storage::list_draft_relations/entity_get/entity_upsert` | none structural | kernel storage dep |
| nexus.editor | **none declared** | `com.nexus.storage::*` (6), `com.nexus.git::*` (3), `com.nexus.editor::*`, `com.nexus.ai::predict` | `../comments/commentsApi`, `../workspace/workspaceStore`, `../files/filesStore` | **`nexus.comments`, `nexus.workspace`, `nexus.files` (X) — all undeclared.** Plus 5 kernel-plugin deps. |
| nexus.enrich | not investigated | not investigated | not investigated | not investigated |
| nexus.fileProperties | not investigated | not investigated | not investigated | not investigated |
| nexus.files | `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar` | `com.nexus.storage::*` (5); subs `com.nexus.storage.file_*` | `../workspace/workspaceStore`, `../status/statusStore`, `../editor/editorStore`, `../status/StatusPill`, `../status/useFileStatus` | **`nexus.editor` (X) — imports `editorStore` without declaring**; `nexus.status` is not a plugin so the imports leak to a non-plugin location |
| nexus.gitPanel | `nexus.workspace`, `nexus.activityBar`, `nexus.gitStatus` | `com.nexus.git::*` (5) | `../gitStatus/gitStatusStore` (declared) | none structural (`com.nexus.git` is a kernel dep schema can't express) |
| nexus.gitStatus | `nexus.workspace` | `com.nexus.git::status` | none | registers `statusBarLeft` view without declaring `nexus.statusBar` |
| nexus.graph | not investigated | not investigated | not investigated | not investigated |
| nexus.healthPanel | not investigated | not investigated | not investigated | not investigated |
| nexus.launcher | `nexus.workspace` | none | none | dispatches `nexus.workspace.{open,...}` commands (declared) |
| nexus.linkSuggest | not investigated | not investigated | not investigated | not investigated |
| nexus.mcp | not investigated | not investigated | not investigated | not investigated |
| nexus.memory | none | `com.nexus.storage::note_append` | none | direct `@tauri-apps/plugin-global-shortcut` import bypasses `api.platform`; kernel storage dep |
| nexus.multibufferSync | `nexus.editor` | `com.nexus.editor::*` (2) | `../editor/types`, `../editor/kernelClient` (declared) | none structural |
| nexus.notifications | none | Tauri `notify_desktop` bridge | none | consumes `com.nexus.notifications.delivered` topic (Rust producer) without dep |
| nexus.notificationsInbox | `nexus.paneMode`, `nexus.activityBar` | `com.nexus.notifications::inbox_*` (3) | `../../../stores/paneModeStore` | kernel notifications dep |
| nexus.notificationsSettings | none | `com.nexus.security::list_secret_names/delete_secret/set_secret`, `com.nexus.notifications::send` | none | uses module-scope `_api` singleton (`notificationsSettingsRuntime.ts`) |
| nexus.notion | none | `com.nexus.formats::import_notion/export_notion` | none | direct `@tauri-apps/plugin-dialog` import |
| nexus.observability | `nexus.workspace` | `com.nexus.ai::activity_list`, `com.nexus.workflow::*` (4) | `../activityTimeline/activityTimelineStore` (type import), `../../../workspace` | **type import from `nexus.activity` (X) — undeclared**; kernel ai+workflow deps |
| nexus.osArchitecture | `nexus.workspace` | `com.nexus.storage::read_file`, `com.nexus.skills::list`, `com.nexus.workflow::list` | `../../../workspace` | kernel storage/skills/workflow deps; skills+workflow are soft (degrades gracefully) |
| nexus.outgoingLinks | none | `com.nexus.storage::outgoing_links` | `../editor/editorStore`, `../files/kernelClient` | **`nexus.editor`, `nexus.files` (X) — undeclared. Reuses `nexus.files` kernelClient singleton.** |
| nexus.outline | `nexus.rightPanel` | none directly (uses `nexus.editor` runtime) | `../editor/editorStore`, `../editor/runtime`, `../editor/types` | **`nexus.editor` (X) — heavy coupling, undeclared** |
| nexus.paneMode | none | none | `../../../stores/paneModeStore` (own store) | the store lives outside the plugin dir so other plugins bypass the plugin entirely |
| nexus.pick | none | none | none | `api.input.pick` host-side lazy-imports this plugin's store |
| nexus.pluginsMgmt | none | Tauri `get_plugin_granted_capabilities`, `set_plugin_enabled` bridge | `../../catalog`, `../../core/capabilityPrompt`, `../../../host/pluginActivation`, `../../../host/communityPluginLoader`, `../../../host/shellRegistry`, `../../../stores/pluginsStatusStore` | legitimate fan-out for a plugin manager; deep host coupling |
| nexus.processes | `nexus.paneMode`, `nexus.activityBar` | `com.nexus.terminal::list_sessions`, `com.nexus.mcp.host::list_servers`; subs 9 kernel prefix filters | `host/communityPluginLoader` type import, `stores/paneModeStore` | depends on shell boot sequence registering `pluginList`/`communityPluginManifests` internal services |
| nexus.prompt | none | none | none | `api.input.prompt` host-side lazy-imports this plugin's store |
| nexus.recall | `nexus.ai` | `com.nexus.ai::semantic_search` | none (uses `recallApi` module singleton) | reads `memory.inboxPath` config — implicit soft-dep on `nexus.memory` schema |
| nexus.rightPanel | none | none | `../../../workspace` | uses workspace host directly |
| nexus.search | `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar` | `com.nexus.storage::search` | `../../../workspace`, `stores/configStore` | searchRuntime caches kernel handle in module-scope singleton |
| nexus.searchPanel | none | `com.nexus.storage::find_in_files`, `replace_in_files` | `../../../workspace` | does not declare any deps despite calling storage |
| nexus.semanticSearch | none | `com.nexus.storage::search`, `com.nexus.ai::semantic_search` | none | **functionally requires `com.nexus.ai` — undeclared and not even soft-checked** |
| nexus.sidebar | none | none | none | **kept alive only so ~10 other plugins' `dependsOn` resolve. Functionally dead.** |
| nexus.skills | `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar` | `com.nexus.skills::list` | `../../../workspace` | kernel skills dep |
| nexus.statusBar | `nexus.workspace`, `nexus.editor` | `com.nexus.ai::index_trigger/index_status` (polled every 2s) | (likely `nexus.backlinks` store import, per code comment) | **deliberately** omits `nexus.backlinks` from `dependsOn` (per index.tsx:16-18) — soft dep so status bar survives if backlinks default-off; kernel ai dep |
| nexus.tags | none | `com.nexus.storage::read_frontmatter/query_tags` | `../editor/editorStore`, `../files/kernelClient` | **`nexus.editor`, `nexus.files` (X) — same anti-pattern as outgoingLinks** |
| nexus.templates | `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar` | `com.nexus.templates::list/apply` | `../../../workspace` | executes `nexus.files.openByPath` command — undeclared command dep |
| nexus.terminal | `nexus.workspace`, `nexus.activityBar` | `com.nexus.terminal::create_session/close_session/read_raw_since`; subs `com.nexus.terminal.output.` prefix | `../workspace/workspaceStore` | reaches into nexus.workspace's own store rather than going through host workspace global |
| nexus.themePicker | `nexus.activityBar` (catalog ALSO lists `core.theme-service` — drift) | `com.nexus.theme::compute_variables/apply_theme/set_plugin_overrides/reload` | `../../../stores/themeStore` | `getPickerApi()` module singleton in `ThemeBuilder.tsx`; **catalog drift**: `core.theme-service` declared in `catalog.ts:307` but not in the plugin's own `index.ts:22` manifest |
| nexus.viewBuilder | `nexus.workspace` | `com.nexus.storage::list_dir/read_file/create_dir/write_file/delete_file` | `../../../workspace` | writes layouts under `.forge/layouts/` and `.forge/plugins/` via raw storage IPC, bypassing any layout/plugin-scaffold contract |
| nexus.workflow | `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar` | `com.nexus.workflow::list/run/validate` | `../../../workspace`, `../constants` | none beyond `com.nexus.workflow` kernel dep |
| nexus.workspace | none | Tauri bridge commands (init_forge, boot_kernel, shutdown_kernel, etc.) | none | the cornerstone — emits `workspace:opened`/`workspace:closed` subscribed by ~30 other plugins |

> The "not investigated" rows are plugins whose dirs exist but were outside the alphabetical split given to the extractor agents. They were assessed in §3 of the per-category shell-nexus assessments; see those files for declared deps.

## Section 4 — Inverse index ("who depends on me?")

For every plugin that has at least one dependent, who needs it active. Cross-tier (shell→Rust) coupled here as well — those are the dependencies that no schema currently expresses.

### Rust-tier most-depended-on plugins

| Plugin | Required by (Rust) | Required by (shell, via `api.kernel.invoke`) |
|--------|-------------------|----------------------------------------------|
| **com.nexus.storage** | editor, ai, agent, workflow (mandatory); storage compile-deps: database, formats | bases, canvas, dreamCycle, editor, files, memory, viewBuilder, semanticSearch, search, searchPanel, tags, outgoingLinks, osArchitecture, notion (via formats) — **15+ shell plugins** |
| com.nexus.editor | (none — pure publisher to collab) | crdtConflict, diagnostics, editor, multibufferSync — 4 shell plugins |
| com.nexus.git | ai (log) | editor, gitPanel, gitStatus — 3 shell plugins |
| com.nexus.ai | terminal (stream_chat soft), agent, workflow, audio (resolve_credentials), ai-runtime (cycle) | osObservability, recall, semanticSearch, statusBar (index polling) — 4 shell plugins |
| com.nexus.ai.runtime | ai (compile + worker pool), agent (submit/wait_for), workflow (submit) | — |
| com.nexus.agent | ai-runtime, skills (cycle) | — |
| com.nexus.skills | agent (cycle) | osArchitecture, skills, mcp.host (frontend) |
| com.nexus.terminal | ai (run_saved, get_session_info), workflow, ai (in tool calls) | canvas, processes, terminal |
| com.nexus.notifications | agent (send), workflow (send) | notifications, notificationsInbox, notificationsSettings |
| com.nexus.mcp.host | ai (call_tool), agent | processes |
| com.nexus.workflow | (none — pure handler provider) | observability, osArchitecture, workflow |
| com.nexus.theme | (none — pure handler provider) | themeService (core.theme-service), themePicker |
| com.nexus.formats | storage (compile), editor (compile) | notion |
| com.nexus.database | storage (compile), editor (apply_view) | bases |
| com.nexus.security | git (compile), audio (compile), ai (compile) | notificationsSettings |
| com.nexus.linkpreview | — | canvas |
| com.nexus.collab | — | collab (UI), crdtConflict (transitively) |
| com.nexus.templates | — | templates |
| com.nexus.comments | — | (consumed by `nexus.editor` via comments API which goes through IPC) |
| com.nexus.lsp | — | diagnostics |
| com.nexus.dap | — | (no in-tree consumer — see assessment) |
| com.nexus.acp | — | (no in-tree consumer — see assessment) |
| com.nexus.audio | — | (no in-tree consumer) |

### Shell-tier most-depended-on plugins

| Plugin | Required by (declared via `dependsOn`) |
|--------|----------------------------------------|
| **nexus.workspace** | bases, canvas, collab, files, gitPanel, gitStatus, launcher, observability, osArchitecture, search, skills, statusBar, templates, terminal, viewBuilder, workflow — **16+ plugins** |
| **nexus.activityBar** | collab, diagnostics, dreamCycle, files, gitPanel, notificationsInbox, processes, search, skills, templates, terminal, themePicker, workflow, observability, osArchitecture — **15+ plugins** |
| nexus.sidebar | files, search, skills, templates, workflow (~10 declarations) — **functionally dead but kept alive only to satisfy these dependsOn** |
| nexus.paneMode | diagnostics, dreamCycle, notificationsInbox, processes |
| nexus.editor | multibufferSync, statusBar; **undeclared by** canvas, diagnostics, files, outgoingLinks, outline, tags |
| nexus.files | **undeclared by** outgoingLinks, tags (both reuse `files/kernelClient`); templates (executes `nexus.files.openByPath`) |
| nexus.rightPanel | outline |
| nexus.gitStatus | gitPanel |
| core.configuration-service | settings; **undeclared ambient consumer:** every plugin using `api.configuration` |
| core.theme-service | (catalog only) themePicker — manifest drift |
| nexus.ai | recall |

## Section 5 — Required set for "basic capabilities"

Basic capabilities are defined in `README.md` as: open a forge, browse + edit markdown in the desktop shell, run global search, commit via git.

Computing the transitive closure of `dependsOn` + observed `api.kernel.invoke` + `api.events` subscriptions:

| Tier | Required plugins (boot order where ordering matters) |
|------|-----------------------------------------------------|
| Rust core | security → storage → formats (compile dep of storage) → database (compile dep of storage) → editor → git. (Theme is needed for non-default theming; notifications is needed for the toast/inbox infra most plugins use but the basic flow degrades cleanly without it.) |
| Shell core | core.configurationService, core.fileSystemService, core.settings (provides Settings entry to switch theme; not strictly editing), core.themeService (otherwise no theming), core.capabilityPrompt (mediates community plugin consent), core.notificationService (no toasts otherwise) |
| Shell nexus | nexus.workspace (boot the kernel), nexus.activityBar (left rail), nexus.sidebar (still needed for ~10 other plugins' dependsOn even though stub), nexus.files (file tree), nexus.editor (the markdown surface), nexus.search + nexus.searchPanel (global search), nexus.gitPanel + nexus.gitStatus (commit/push) |

That's **17 plugins** total (6 Rust + 6 shell-core + 5 shell-nexus + 6 more nexus listed above = 17 unique once dedup'd) versus the ~93 plugins that exist. Everything else is feature surface.

## Section 6 — Cycles, reverse couplings, layering leaks

- **Runtime cycle: `com.nexus.agent` ↔ `com.nexus.skills`.** `agent` calls `skills::triggered_by/compose/render`; `skills` calls back into `agent::session_run`. Boot order alone does not resolve this — both must be present for either to fully function. The cycle is functional, not a deadlock (calls are async, no shared lock).
- **Runtime cycle: `com.nexus.ai` ↔ `com.nexus.ai.runtime`.** `ai` compile-depends on `ai-runtime` (shared tokio pool handle); `ai-runtime` subscribes to `com.nexus.ai.stream_chunk` and republishes. Bootstrap registers `ai-runtime` first to break the boot-order half.
- **Reverse coupling: `com.nexus.storage` subscribes to `com.nexus.git.commit`.** A subsystem that comes earlier in registration listens to a topic published by a later plugin (BL-114 indexing wake-up). Loose coupling — the subscriber path no-ops if git is absent.
- **Compile-time leak: `nexus-storage` Cargo deps on `nexus-database` + `nexus-formats`.** Both are reachable via IPC, but storage links them in-process for query helpers. This bypasses the microkernel IPC seam (`nexus-storage` ought to depend only on `nexus-types` + `nexus-plugin-api` if the seam were strict).
- **Dynamic dispatch makes `agent`, `workflow`, and `acp` runtime deps unbounded.** Their effective dep set is "any plugin a user wires into a workflow step, agent tool call, or ACP JSON-RPC method."
- **Shell `dependsOn` cannot express kernel-plugin deps.** Every shell→Rust dep is hidden by design of the schema, not by author oversight. Examples: `core.themeService → com.nexus.theme`, `nexus.bases → com.nexus.storage + com.nexus.database`, `nexus.editor → com.nexus.storage + com.nexus.git + com.nexus.editor + com.nexus.ai`, ~30 more.

## Section 7 — Hidden couplings (declared deps don't cover actual usage)

Author-introduced (would be fixed by adding the missing `dependsOn` entry):

| Plugin | Missing declared dep | Why it's hidden |
|--------|---------------------|-----------------|
| nexus.editor | nexus.comments, nexus.workspace, nexus.files | TS imports across plugin dirs |
| nexus.canvas | nexus.editor | imports `../editor/blockRefDrag`, `markdownRender` |
| nexus.diagnostics | nexus.editor, nexus.workspace | imports kernelClient/workspaceStore directly |
| nexus.files | nexus.editor | imports `editorStore` |
| nexus.outgoingLinks | nexus.editor, nexus.files | imports editorStore + files' kernelClient |
| nexus.outline | nexus.editor | imports editorRuntime + editorStore + types |
| nexus.tags | nexus.editor, nexus.files | imports editorStore + files' kernelClient |
| nexus.observability | nexus.activity (activityTimeline) | type import |
| nexus.templates | nexus.files | dispatches `nexus.files.openByPath` |
| nexus.gitStatus | nexus.statusBar | registers `statusBarLeft` view in a slot owned by statusBar |
| nexus.crdtConflict | nexus.collab | calls `com.nexus.editor::apply_transaction` in a flow that only fires when collab is publishing CRDT conflicts |
| nexus.statusBar | nexus.backlinks (deliberately omitted; documented in index.tsx:16-18) | soft-dep policy decision |
| core.settings | nexus.pluginsMgmt | imports `PluginsMgmtView` for inline plugins page |
| core.capabilityPrompt | nexus.pluginsMgmt | imports `capabilityInfo` |
| core.zoom | core.configurationService | uses `api.configuration` ambiently |
| core.notificationService | core.configurationService | reaches into `configStore` singleton instead of going through `api.configuration` |
| nexus.themePicker | (catalog drift) | `core.theme-service` listed in `catalog.ts:307` but not in plugin's own `index.ts:22` manifest |

Schema-introduced (no fix possible without extending the manifest):

- Every shell plugin that calls `api.kernel.invoke('com.nexus.<x>', ...)` — there is no way to declare a Rust-tier dep from a shell manifest. The activate-time `api.kernel.available('com.nexus.<x>')` check is the only graceful-degrade hook.
- Every Rust plugin that calls `ipc_call('com.nexus.<x>', ...)` — same story: the manifest has no `dependsOn` field at all.

Anti-pattern: **module-scope singletons that cache kernel handles.** `recallApi`, `pickerRuntime`, `searchRuntime`, `notificationsSettingsRuntime`, and the `themePicker` `getPickerApi()` all hide kernel coupling inside the plugin's own module-level state, bypassing PluginAPI prop drilling. This works but makes the dep invisible to any static-analysis pass.

## Section 8 — Recommendations

In rough priority order. None of these are applied — they fall out of the assessment.

1. **Add `dependsOn` to `PluginManifest` (Rust).** Even if boot order stays hand-curated, a declarative field makes the dep graph machine-readable. Today the only place the order is documented is comments in `register_all`.
2. **Allow cross-tier deps in shell `dependsOn`.** Either extend the schema with a `kernelDependsOn: ['com.nexus.ai']` field, or have the host normalise `com.nexus.*` ids inside the existing `dependsOn` array. Today every shell plugin that calls a Rust handler is silently coupled to that handler's plugin being active.
3. **Audit and declare the ~17 hidden shell couplings** listed in §7 — these are local fixes (one `dependsOn:` entry each).
4. **Resolve `nexus.themePicker` catalog drift** (`catalog.ts:307` says one thing, `index.ts:22` says another).
5. **Decide what to do with `nexus.sidebar`** — keeping a dead stub alive solely to satisfy `dependsOn` is sustainable only because the dep is declared, not because the stub does anything.
6. **Either declare or remove the runtime cycle between `com.nexus.agent` and `com.nexus.skills`.** If the cycle is intentional, a comment in both `core_plugin.rs` files would save the next reader.
7. **Replace module-scope singletons** (`recallApi`, `pickerRuntime`, `searchRuntime`, etc.) with PluginAPI-injected handles. The singleton pattern bypasses every declared-dep mechanism.
8. **Move `nexus-storage` compile-time deps on `nexus-database` and `nexus-formats`** behind feature flags or extract the shared types to `nexus-types`. The current arrangement leaks across the IPC seam that the rest of the architecture relies on.
