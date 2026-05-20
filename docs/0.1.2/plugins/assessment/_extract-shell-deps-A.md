# Shell deps extraction — batch A

Source-of-truth: `shell/src/plugins/catalog.ts`. Covers all 7 live `core/*` plugins
plus the first half (alphabetical, A–M, 16 of 30) of live `nexus/*` plugins.

## Live core plugins

### core.capabilityPrompt
- **dependsOn (manifest):** none (TOML absent; TS manifest has none)
- **IPC calls:** com.kernel::set_plugin_granted_capabilities, com.kernel::revoke_plugin_capability (via `invoker.invoke(...)` from `applyCapabilityChange.ts:92,103` + `requestConsent.ts:62`)
- **API surfaces:** api.views
- **Cross-plugin imports:** `../../nexus/pluginsMgmt/capabilityInfo` (consentLogic.ts:19, CapabilityBannerView.tsx:11, CapabilityModalView.tsx:14)
- **Events subscribed:** none
- **Hidden couplings:** depends on `nexus.pluginsMgmt` exports (capabilityInfo) without declaring it; calls kernel handlers `set_plugin_granted_capabilities` / `revoke_plugin_capability` without declaring a kernel-plugin dep
- **Sources:** shell/src/plugins/core/capabilityPrompt/index.ts:22-31; applyCapabilityChange.ts:92-108; CapabilityModalView.tsx:14

### core.configuration-service
- **dependsOn (manifest):** none
- **IPC calls:** indirect — `hydrateFromForge` reads forge `app.toml` (via configStore IPC)
- **API surfaces:** api.internal, api.configuration, api.kernel, api.events
- **Cross-plugin imports:** none (only shell internals: `registry/ConfigurationRegistry`, `stores/configStore`)
- **Events subscribed:** `workspace:opened`, `workspace:closed`
- **Hidden couplings:** owns `configurationRegistry` + `configStore` services consumed by virtually every other plugin (not surfaced as a hard manifest dep on consumers)
- **Sources:** shell/src/plugins/core/configurationService/index.ts:19-58

### core.filesystem-service
- **dependsOn (manifest):** none
- **IPC calls:** none (delegates to `api.platform.fs`); imports `@tauri-apps/plugin-fs::watch` directly
- **API surfaces:** api.platform, api.internal
- **Cross-plugin imports:** none
- **Events subscribed:** none
- **Hidden couplings:** registers `fsService` internal service; takes a hard runtime dep on `@tauri-apps/plugin-fs` for watch() — orchestrator allowlisted per WI-23
- **Sources:** shell/src/plugins/core/fileSystemService/index.ts:1-84

### core.notification-service
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** api.internal, api.configuration
- **Cross-plugin imports:** none (uses `stores/configStore` shell-internal)
- **Events subscribed:** none
- **Hidden couplings:** registers `notificationQueue` internal service consumed by `api.notifications` host wiring; reads `ui.notificationDurationMs` config but config registration happens at activate (fine)
- **Sources:** shell/src/plugins/core/notificationService/index.ts:61-92

### core.settings
- **dependsOn (manifest):** core.configuration-service, nexus.activityBar
- **IPC calls:** none (the panel itself; SettingsPanelView imports many shell registries)
- **API surfaces:** api.views, api.commands, api.context, api.activityBar
- **Cross-plugin imports:** `../../nexus/pluginsMgmt/PluginsMgmtView` (SettingsPanelView.tsx:19) — undeclared
- **Events subscribed:** none (panel uses shell `eventBus` direct)
- **Hidden couplings:** SettingsPanelView reaches into `nexus.pluginsMgmt` for the inline plugin manager; `nexus.themePicker` / `core.theme-service` state is wired via direct `stores/themeStore` import — none declared
- **Sources:** shell/src/plugins/core/settings/index.ts:18; SettingsPanelView.tsx:10-27

### core.theme-service
- **dependsOn (manifest):** none
- **IPC calls:** `com.nexus.theme::*` via `useThemeStore.getState().hydrate(api)`; subscribes to `com.nexus.theme.changed` (kernel topic)
- **API surfaces:** api.kernel, api.events
- **Cross-plugin imports:** none (uses `stores/themeStore` shell-internal)
- **Events subscribed:** `workspace:opened`; kernel bus `com.nexus.theme.changed`
- **Hidden couplings:** kernel-side `com.nexus.theme` plugin is undeclared (no manifest entry can express kernel-plugin deps)
- **Sources:** shell/src/plugins/core/themeService/index.ts:24-82

### core.zoom
- **dependsOn (manifest):** none (`core: false` in catalog; toml absent)
- **IPC calls:** none
- **API surfaces:** api.configuration, api.commands
- **Cross-plugin imports:** none
- **Events subscribed:** none (uses api.configuration.onChange instead)
- **Hidden couplings:** consumes `api.configuration` but doesn't declare `core.configuration-service`; same omission as most plugins (treated as ambient service)
- **Sources:** shell/src/plugins/core/zoom/index.ts:34-145

## Live nexus plugins (first half, A–M)

### nexus.activityBar
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** api.events, api.context, api.commands, api.views
- **Cross-plugin imports:** none
- **Events subscribed:** `activityBar:itemAdded`, `activityBar:itemRemoved` (shell internal bus)
- **Hidden couplings:** contributes the `nexus.activityBar.activeView` context key consumed forge-wide; many other plugins call `api.activityBar.addItem` without declaring it as a manifest dep (settings, themePicker, gitPanel, collab, diagnostics, dreamCycle, files do — the rest don't)
- **Sources:** shell/src/plugins/nexus/activityBar/index.ts:11-78

### nexus.bases
- **dependsOn (manifest):** nexus.workspace
- **IPC calls:** com.nexus.storage::{base_load, base_create, base_record_create/update/delete/soft_delete/restore, base_property_create/update/rename/delete, base_view_create/update/delete}; com.nexus.database (kernel client target)
- **API surfaces:** api.kernel, api.viewRegistry, api.configuration, api.commands, api.events, api.views, api.notifications
- **Cross-plugin imports:** none (lives entirely under nexus/bases/)
- **Events subscribed:** none (emits `files:open`)
- **Hidden couplings:** depends on the `bases.focused` / `bases.editing` context keys it owns; no dep on `com.nexus.storage` kernel plugin declared (cannot be expressed)
- **Sources:** shell/src/plugins/nexus/bases/index.ts:40-146; kernelClient.ts:8-273

### nexus.canvas
- **dependsOn (manifest):** nexus.workspace
- **IPC calls:** com.nexus.storage::{list_dir, canvas_write, canvas_read, canvas_patch, read_file, base_load}; com.nexus.linkpreview::*; com.nexus.terminal::*
- **API surfaces:** api.kernel, api.viewRegistry, api.commands, api.configuration, api.events, api.notifications
- **Cross-plugin imports:** `../editor/blockRefDrag` (blockRefDrop.ts:20), `../editor/markdownRender` (CanvasOverlay.tsx:13)
- **Events subscribed:** none (emits `files:open`)
- **Hidden couplings:** structural import from `nexus.editor` without declaring `nexus.editor` in dependsOn; uses `com.nexus.linkpreview` + `com.nexus.terminal` kernel plugins not declared
- **Sources:** shell/src/plugins/nexus/canvas/index.ts:42-211; kernelClient.ts:9-173

### nexus.collab
- **dependsOn (manifest):** nexus.workspace, nexus.activityBar
- **IPC calls:** com.nexus.collab::relay_status; bus prefix sub `com.nexus.collab.`
- **API surfaces:** api.kernel, api.viewRegistry, api.events, api.commands, api.activityBar
- **Cross-plugin imports:** none (uses shell-level `workspace` singleton)
- **Events subscribed:** `workspace:closed`; kernel topics `com.nexus.collab.peers.joined/left/presence/connection/relay.started/relay.stopped`
- **Hidden couplings:** declares activityBar dep; OK
- **Sources:** shell/src/plugins/nexus/collab/index.ts:42-131

### nexus.commandPalette
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** api.commands, api.context, api.views, api.configuration
- **Cross-plugin imports:** none
- **Events subscribed:** none (palette reads command registry directly via paletteRuntime)
- **Hidden couplings:** consumes every plugin's `api.commands.register` contributions — no manifest expression of this dependency
- **Sources:** shell/src/plugins/nexus/commandPalette/index.ts:13-105

### nexus.confirm
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** api.views
- **Cross-plugin imports:** none
- **Events subscribed:** none (modal subscribes to confirmStore directly)
- **Hidden couplings:** `api.input.confirm` host wiring depends on this plugin being loaded; consumers don't declare it
- **Sources:** shell/src/plugins/nexus/confirm/index.ts:17-38

### nexus.crdtConflict
- **dependsOn (manifest):** none
- **IPC calls:** none in index.ts (modal uses `api.kernel.invoke` for `apply_transaction` per inline doc); bus prefix sub `com.nexus.editor.crdt.conflict.`
- **API surfaces:** api.views, api.kernel, api.events
- **Cross-plugin imports:** none
- **Events subscribed:** `workspace:opened`, `workspace:closed`; kernel `com.nexus.editor.crdt.conflict.<relpath>`
- **Hidden couplings:** the resolver modal emits `files:open` and invokes `com.nexus.editor::apply_transaction` (per docstring) — kernel-side `com.nexus.editor` plugin dep undeclared
- **Sources:** shell/src/plugins/nexus/crdtConflict/index.ts:29-97

### nexus.diagnostics
- **dependsOn (manifest):** nexus.paneMode, nexus.activityBar
- **IPC calls:** com.nexus.editor::open_excerpts (via EditorKernelClient); bus sub `com.nexus.lsp.textDocument.publishDiagnostics`
- **API surfaces:** api.kernel, api.views, api.activityBar, api.events, api.commands, api.notifications
- **Cross-plugin imports:** `../workspace/workspaceStore` (index.ts:15), `../editor/kernelClient` (18), `../editor/cm/lspIpc` (23), `../editor/cm/lspToExcerpts` (24)
- **Events subscribed:** `activityBar:activeChanged`, `workspace:opened`, `workspace:closed`; kernel `com.nexus.lsp.textDocument.publishDiagnostics`
- **Hidden couplings:** structural imports from `nexus.editor` + `nexus.workspace` without declaring them in dependsOn (only paneMode + activityBar declared)
- **Sources:** shell/src/plugins/nexus/diagnostics/index.ts:14-26, 72-94

### nexus.dreamCycle
- **dependsOn (manifest):** nexus.paneMode, nexus.activityBar
- **IPC calls:** com.nexus.storage::{list_draft_relations, entity_get, entity_upsert}; bus sub `com.nexus.dream_cycle.proposals`
- **API surfaces:** api.kernel, api.views, api.activityBar, api.events, api.commands, api.notifications
- **Cross-plugin imports:** none (only shell `stores/paneModeStore`)
- **Events subscribed:** `activityBar:activeChanged`, `workspace:opened`, `workspace:closed`; kernel `com.nexus.dream_cycle.proposals`
- **Hidden couplings:** none structural; `com.nexus.storage` kernel dep undeclared (can't be)
- **Sources:** shell/src/plugins/nexus/dreamCycle/index.ts:179-308

### nexus.editor
- **dependsOn (manifest):** none declared in TS manifest (catalog also lists none)
- **IPC calls:** com.nexus.storage::{read_file, write_file, rename_entry, write_frontmatter, delete_file, backlinks}; com.nexus.git::{conflict_files, file_log, diff_file}; com.nexus.editor::{open_excerpts, refresh_excerpts, …}; com.nexus.ai::predict; bus topic `com.nexus.editor.changed.<relpath>`, `com.nexus.terminal.output.<sessionId>`
- **API surfaces:** api.kernel, api.commands, api.events, api.input, api.platform, api.notifications, api.views, api.viewRegistry, api.configuration
- **Cross-plugin imports:** `../comments/commentsApi` (index.ts:42), `../workspace/workspaceStore` (43), `../files/filesStore` (44)
- **Events subscribed:** `files:open`, `workspace:closed`, `nexus.editor:reveal-block`, `nexus.editor:reveal-line` (cap 4 of many)
- **Hidden couplings:** declares no dependsOn yet structurally imports from `nexus.comments`, `nexus.workspace`, `nexus.files`; calls com.nexus.git + com.nexus.ai + com.nexus.editor + com.nexus.storage + com.nexus.terminal IPC handlers
- **Sources:** shell/src/plugins/nexus/editor/index.ts:42-49, 1482-1597, 1670-1780

### nexus.files
- **dependsOn (manifest):** nexus.workspace, nexus.activityBar, nexus.sidebar
- **IPC calls:** com.nexus.storage::{list_dir, create_file, create_dir, rename_entry, delete_file} (via `./kernelClient`); bus subs `com.nexus.storage.file_{created,modified,deleted,renamed}`
- **API surfaces:** api.kernel, api.commands, api.context, api.events, api.input, api.notifications, api.platform, api.viewRegistry
- **Cross-plugin imports:** `../workspace/workspaceStore` (index.ts:17), `../status/statusStore` (18); FilesTree.tsx pulls `../editor/editorStore`, `../status/StatusPill`, `../status/useFileStatus`
- **Events subscribed:** `workspace:opened`, `workspace:closed`; kernel storage events listed above
- **Hidden couplings:** imports `../status/*` and `../editor/editorStore` though `nexus.status` is per the brief NOT a plugin, and `nexus.editor` is not declared in dependsOn
- **Sources:** shell/src/plugins/nexus/files/index.ts:17-18, 77-110, 419-444

### nexus.gitPanel
- **dependsOn (manifest):** nexus.workspace, nexus.activityBar, nexus.gitStatus
- **IPC calls:** com.nexus.git::{status, file_statuses, branches, log, stash_list}; bus prefix sub `com.nexus.git.`
- **API surfaces:** api.kernel, api.viewRegistry, api.events, api.commands, api.activityBar
- **Cross-plugin imports:** `../gitStatus/gitStatusStore` (declared dep — OK)
- **Events subscribed:** `workspace:opened`, `workspace:closed`; kernel `com.nexus.git.*`
- **Hidden couplings:** none — all cross-plugin imports match declared deps
- **Sources:** shell/src/plugins/nexus/gitPanel/index.ts:1-108

### nexus.gitStatus
- **dependsOn (manifest):** nexus.workspace
- **IPC calls:** com.nexus.git::status; bus prefix sub `com.nexus.git.`
- **API surfaces:** api.kernel, api.events, api.views
- **Cross-plugin imports:** none
- **Events subscribed:** `workspace:opened`, `workspace:closed`; kernel `com.nexus.git.*`
- **Hidden couplings:** registers a statusBarLeft view without declaring `nexus.statusBar` (the slot owner)
- **Sources:** shell/src/plugins/nexus/gitStatus/index.ts:16-128

### nexus.launcher
- **dependsOn (manifest):** nexus.workspace
- **IPC calls:** indirect — dispatches `nexus.workspace.{open,openWithTemplate,openRemote,setRoot}` commands
- **API surfaces:** api.events, api.commands, api.notifications, api.views
- **Cross-plugin imports:** none
- **Events subscribed:** `workspace:opened`, `workspace:closed`
- **Hidden couplings:** depends on `nexus.workspace` for commands (declared); recents store reads forge config directly
- **Sources:** shell/src/plugins/nexus/launcher/index.ts:33-156

### nexus.memory
- **dependsOn (manifest):** none
- **IPC calls:** com.nexus.storage::note_append (via `kernelClient.ts`)
- **API surfaces:** api.configuration, api.views, api.commands, api.kernel, api.notifications, api.input (via captureStore)
- **Cross-plugin imports:** none
- **Events subscribed:** none (registers Tauri global-shortcut via `@tauri-apps/plugin-global-shortcut`)
- **Hidden couplings:** direct `@tauri-apps/plugin-global-shortcut` import bypasses the `api.platform` adapter — orchestrator allowlist territory; com.nexus.storage kernel dep undeclared
- **Sources:** shell/src/plugins/nexus/memory/index.ts:17-336; kernelClient.ts:1-12

### nexus.multibufferSync
- **dependsOn (manifest):** nexus.editor
- **IPC calls:** com.nexus.editor::{open_session/snapshot, refresh_excerpts} (via EditorKernelClient); bus prefix sub `com.nexus.editor.changed.`
- **API surfaces:** api.kernel, api.events
- **Cross-plugin imports:** `../editor/types`, `../editor/kernelClient` (declared dep — OK)
- **Events subscribed:** `files:open`; kernel `com.nexus.editor.changed.*`
- **Hidden couplings:** none — declares editor dep matching imports
- **Sources:** shell/src/plugins/nexus/multibufferSync/index.ts:18-138; multibufferRegistry.ts:9
