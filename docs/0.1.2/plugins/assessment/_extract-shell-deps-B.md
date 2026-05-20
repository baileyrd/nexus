# Shell deps extraction — batch B

Second-half (alphabetical) of live `nexus.*` shell plugins from `shell/src/plugins/catalog.ts`. IPC pattern is `api.kernel.invoke(pluginId, handler, args)`; events via `api.events.on(topic, …)` (in-process bus) and `api.kernel.on(topic, …)` (kernel-bus topics). Cross-plugin imports flagged when a plugin reaches into a sibling's directory without declaring the dep.

## Live nexus plugins (second half)

### nexus.notifications
- **dependsOn (manifest):** none
- **IPC calls:** Tauri `invoke('notify_desktop')` (bridge command, not a kernel handler)
- **API surfaces:** `api.kernel.on`, `api.kernel.available`, `api.notifications`, `api.events`
- **Cross-plugin imports:** none
- **Events subscribed:** `workspace:opened`, `workspace:closed`, kernel topic `com.nexus.notifications.delivered`
- **Hidden couplings:** consumes `com.nexus.notifications.delivered` (Rust producer) without a declared dep — relies on shell-side topic only.
- **Sources:** shell/src/plugins/nexus/notifications/index.ts:15, :23, :78

### nexus.notificationsInbox
- **dependsOn (manifest):** `nexus.paneMode`, `nexus.activityBar`
- **IPC calls:** `com.nexus.notifications::inbox_list`, `inbox_mark_read`, `inbox_dismiss`
- **API surfaces:** `api.kernel.invoke`, `api.kernel.on`, `api.kernel.available`, `api.views`, `api.activityBar`, `api.commands`, `api.events`
- **Cross-plugin imports:** `../../../stores/paneModeStore` (shell-global store reach-in to paneMode plugin's owned store)
- **Events subscribed:** `activityBar:activeChanged`, `workspace:opened`, `workspace:closed`, kernel topic `com.nexus.notifications.inbox.appended`, plus emits `nexus.notificationsInbox:jump-to-task`
- **Hidden couplings:** depends on `com.nexus.notifications` IPC (Rust) — not in dependsOn list (shell-plugin deps only).
- **Sources:** shell/src/plugins/nexus/notificationsInbox/index.ts:56, :83, :98, :205

### nexus.notificationsSettings
- **dependsOn (manifest):** none
- **IPC calls:** `com.nexus.security::list_secret_names`, `delete_secret`, `set_secret`; `com.nexus.notifications::send`
- **API surfaces:** `api.kernel.invoke`, `api.settings.registerTab`
- **Cross-plugin imports:** none
- **Events subscribed:** none
- **Hidden couplings:** uses module-scope `_api` singleton (notificationsSettingsRuntime.ts) so React tab can reach kernel without prop drilling.
- **Sources:** shell/src/plugins/nexus/notificationsSettings/NotificationsSettings.tsx:71, :89, :100, :115; notificationsSettingsRuntime.ts:9

### nexus.notion
- **dependsOn (manifest):** none
- **IPC calls:** `com.nexus.formats::import_notion`, `export_notion`
- **API surfaces:** `api.kernel.invoke`, `api.kernel.available`, `api.commands`, `api.input.prompt`, `api.notifications`
- **Cross-plugin imports:** none
- **Events subscribed:** none
- **Hidden couplings:** also imports `@tauri-apps/plugin-dialog` directly for native folder/file pickers — bypasses any shell-side abstraction.
- **Sources:** shell/src/plugins/nexus/notion/index.ts:4-9, :85, :139

### nexus.osObservability
- **dependsOn (manifest):** `nexus.workspace`
- **IPC calls:** `com.nexus.ai::activity_list`; `com.nexus.workflow::list`, `run_history`, `next_fire`, `run`
- **API surfaces:** `api.kernel.invoke`, `api.kernel.on`, `api.kernel.available`, `api.viewRegistry`, `api.activityBar`, `api.commands`, `api.events`, `api.notifications`
- **Cross-plugin imports:** `import type { ActivityEntry } from '../activityTimeline/activityTimelineStore'`; also `../../../workspace`
- **Events subscribed:** `workspace:opened`, `workspace:closed`, kernel topic `com.nexus.activity.appended`
- **Hidden couplings:** undeclared structural coupling to `nexus.activity` (activityTimeline) via type import.
- **Sources:** shell/src/plugins/nexus/observability/index.ts:17, :60, :81, :221

### nexus.osArchitecture
- **dependsOn (manifest):** `nexus.workspace`
- **IPC calls:** `com.nexus.storage::read_file`; `com.nexus.skills::list`; `com.nexus.workflow::list`
- **API surfaces:** `api.kernel.invoke`, `api.kernel.available`, `api.viewRegistry`, `api.activityBar`, `api.commands`, `api.events`
- **Cross-plugin imports:** `../../../workspace` (shell host)
- **Events subscribed:** `workspace:opened`, `workspace:closed`
- **Hidden couplings:** soft-deps on `com.nexus.skills` + `com.nexus.workflow` (Rust crates) — comment says "drift detection" gracefully degrades.
- **Sources:** shell/src/plugins/nexus/osArchitecture/index.ts:24-27, :86, :107-108, :165

### nexus.outgoingLinks
- **dependsOn (manifest):** none
- **IPC calls:** `com.nexus.storage::outgoing_links`
- **API surfaces:** `api.viewRegistry`, `api.commands`, `api.events`
- **Cross-plugin imports:** `import { useEditorStore } from '../editor/editorStore'`; `import { getKernel } from '../files/kernelClient'`
- **Events subscribed:** none directly; emits `files:open`
- **Hidden couplings:** reaches into both `nexus.editor` (editorStore) and `nexus.files` (kernelClient) without declaring them as deps; uses files' kernelClient singleton instead of `api.kernel`.
- **Sources:** shell/src/plugins/nexus/outgoingLinks/index.tsx:5-6, :13, :73

### nexus.outline
- **dependsOn (manifest):** `nexus.rightPanel`
- **IPC calls:** none directly; calls into editor runtime's `kernelClient.getTree`/`getMarkdown` (which themselves invoke `com.nexus.editor`)
- **API surfaces:** `api.viewRegistry`, `api.commands`, `api.events`
- **Cross-plugin imports:** `import { useEditorStore } from '../editor/editorStore'`; `import { getEditorRuntime } from '../editor/runtime'`; `import type { BlockTree, EditorChangedPayload } from '../editor/types'`
- **Events subscribed:** `rightPanel:registerTab` (emits), `workspace:closed`, `editor:activeHeadingChanged`, `nexus.outline:requestRefresh`
- **Hidden couplings:** hard structural coupling to `nexus.editor` (editorRuntime, editorStore, types) — not in dependsOn (which only lists rightPanel).
- **Sources:** shell/src/plugins/nexus/outline/index.ts:8-10, :145-148, :195

### nexus.paneMode
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** `api.commands`, `api.context`
- **Cross-plugin imports:** `../../../stores/paneModeStore` (its own canonical store, lives shell-global)
- **Events subscribed:** none
- **Hidden couplings:** the paneModeStore lives under `shell/src/stores/` rather than the plugin dir, so any other plugin reaching `usePaneModeStore` bypasses the plugin's public surface.
- **Sources:** shell/src/plugins/nexus/paneMode/index.ts:2, :50-67

### nexus.pick
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** `api.views.register` (overlay slot)
- **Cross-plugin imports:** none
- **Events subscribed:** none
- **Hidden couplings:** modal reads from `pickStore`; `api.input.pick` (host-side) lazy-imports `requestPick` from this plugin's store — coupling lives in host/PluginAPI not the plugin.
- **Sources:** shell/src/plugins/nexus/pick/index.ts:11, :25-33

### nexus.prompt
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** `api.views.register` (overlay slot)
- **Cross-plugin imports:** none
- **Events subscribed:** none
- **Hidden couplings:** same pattern as pick — `api.input.prompt` host-side lazy-imports `requestPrompt` from this plugin.
- **Sources:** shell/src/plugins/nexus/prompt/index.ts:11, :24-33

### nexus.pluginsMgmt
- **dependsOn (manifest):** none
- **IPC calls:** Tauri `invoke('get_plugin_granted_capabilities')`, `invoke('set_plugin_enabled')` (bridge commands, not kernel handlers)
- **API surfaces:** `api.internal.getInternalService`, `api.commands`, `api.events`, `api.context`, `api.views`, `api.notifications`
- **Cross-plugin imports:** `../../catalog`, `../../core/capabilityPrompt` (core sibling), `../../../host/pluginActivation`, `../../../host/communityPluginLoader`, `../../../host/shellRegistry`, `../../../stores/pluginsStatusStore`
- **Events subscribed:** `PLUGIN_LIST_CHANGED_EVENT`
- **Hidden couplings:** core:true plugin reaching deep into host modules and the catalog; many couplings but legitimate for a plugin manager.
- **Sources:** shell/src/plugins/nexus/pluginsMgmt/index.ts:1, :22-37, :121, :541

### nexus.processes
- **dependsOn (manifest):** `nexus.paneMode`, `nexus.activityBar`
- **IPC calls:** `com.nexus.terminal::list_sessions`; `com.nexus.mcp.host::list_servers`; subscribes to 9 kernel topic prefixes (`com.nexus.storage.`, `git.`, `terminal.`, `workflow.`, `ai.`, `theme.`, `mcp.`, `skills.`, `agent.`)
- **API surfaces:** `api.kernel.invoke`, `api.kernel.on`, `api.kernel.available`, `api.internal.getInternalService`, `api.commands`, `api.views`, `api.activityBar`, `api.events`
- **Cross-plugin imports:** `import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'`; `../../../stores/paneModeStore`
- **Events subscribed:** `activityBar:activeChanged`, `workspace:opened`, `workspace:closed`
- **Hidden couplings:** core:true (declared so) to use `api.internal`; consumes `pluginList`/`communityPluginManifests` services registered by main.tsx at boot — undeclared structural reliance on shell boot sequence.
- **Sources:** shell/src/plugins/nexus/processes/index.ts:39-49, :133, :153, :293

### nexus.recall
- **dependsOn (manifest):** `nexus.ai`
- **IPC calls:** `com.nexus.ai::semantic_search`
- **API surfaces:** `api.commands`, `api.context`, `api.configuration`, `api.keybindings`, `api.views`
- **Cross-plugin imports:** none (recallApi singleton holds api ref for overlay)
- **Events subscribed:** none
- **Hidden couplings:** filterToInboxScope reads `memory.inboxPath` config — implicit soft dep on `nexus.memory` plugin (its config schema). dependsOn doesn't list it.
- **Sources:** shell/src/plugins/nexus/recall/index.ts:46, :92-141; recallRuntime.ts:169, :180

### nexus.rightPanel
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** `api.context`, `api.events`, `api.commands`
- **Cross-plugin imports:** `../../../workspace` (shell host)
- **Events subscribed:** `rightPanel:registerTab`, `rightPanel:unregisterTab`; `workspace.on('layout-change')`
- **Hidden couplings:** uses workspace host directly rather than going through PluginAPI for layout state.
- **Sources:** shell/src/plugins/nexus/rightPanel/index.ts:2, :54, :60-72

### nexus.search
- **dependsOn (manifest):** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`
- **IPC calls:** `com.nexus.storage::search`
- **API surfaces:** `api.kernel`, `api.configuration`, `api.viewRegistry`, `api.commands`, `api.events`
- **Cross-plugin imports:** `../../../workspace`; `../../../stores/configStore` (searchRuntime.ts)
- **Events subscribed:** `workspace:closed`; emits `files:open`
- **Hidden couplings:** searchRuntime caches the kernel handle in a module-scoped singleton — same anti-pattern as recallApi/pickerRuntime.
- **Sources:** shell/src/plugins/nexus/search/index.ts:31, :69; searchRuntime.ts:18-25, :177

### nexus.searchPanel
- **dependsOn (manifest):** none
- **IPC calls:** `com.nexus.storage::find_in_files`, `replace_in_files` (invoked from `SearchPanelView` via `api.kernel` passed through props)
- **API surfaces:** `api.viewRegistry`, `api.commands`, `api.events`, `api.kernel` (passed to view)
- **Cross-plugin imports:** `../../../workspace`
- **Events subscribed:** `workspace:closed`
- **Hidden couplings:** no dependsOn declared though `find_in_files`/`replace_in_files` are core to function. Lazy activation via `onCommand`/`onView`.
- **Sources:** shell/src/plugins/nexus/searchPanel/index.tsx:1-9, :73-94

### nexus.semanticSearch
- **dependsOn (manifest):** none
- **IPC calls:** `com.nexus.storage::search`; `com.nexus.ai::semantic_search`
- **API surfaces:** `api.kernel.invoke`, `api.commands`, `api.input.prompt`, `api.events`, `api.notifications`
- **Cross-plugin imports:** none
- **Events subscribed:** none; emits `files:open`
- **Hidden couplings:** functionally requires `nexus.ai` (or `com.nexus.ai`) to be present and configured — undeclared.
- **Sources:** shell/src/plugins/nexus/semanticSearch/index.ts:19-22, :117, :123, :151

### nexus.sidebar
- **dependsOn (manifest):** none
- **IPC calls:** none
- **API surfaces:** none (activate is a no-op)
- **Cross-plugin imports:** none
- **Events subscribed:** none
- **Hidden couplings:** retained purely for `dependsOn: ['nexus.sidebar']` resolution in legacy plugins; functionally dead.
- **Sources:** shell/src/plugins/nexus/sidebar/index.ts:14-28

### nexus.skills
- **dependsOn (manifest):** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`
- **IPC calls:** `com.nexus.skills::list` (additional invokes likely in SkillEditor/SkillsView via passed `api.kernel`)
- **API surfaces:** `api.kernel.invoke`, `api.kernel.available`, `api.viewRegistry`, `api.activityBar`, `api.commands`, `api.events`
- **Cross-plugin imports:** `../../../workspace`
- **Events subscribed:** `workspace:opened`, `workspace:closed`
- **Hidden couplings:** depends on `com.nexus.skills` Rust crate; `subscribeSkillsChanged` is a module-local in-store eventbus, not via api.events.
- **Sources:** shell/src/plugins/nexus/skills/index.ts:21-25, :111, :138, :189

### nexus.statusBar
- **dependsOn (manifest):** `nexus.workspace`, `nexus.editor` (with soft `nexus.backlinks` documented in comment)
- **IPC calls:** `com.nexus.ai::index_trigger`, `index_status` (polled every 2s from `IndexingStatus`)
- **API surfaces:** `api.views.register` (statusBarRight slot), `api.kernel.invoke`, `api.notifications`
- **Cross-plugin imports:** Component imports likely reach `nexus.backlinks` zustand store (see code comment); not verified in this batch
- **Events subscribed:** none directly
- **Hidden couplings:** comment in index.tsx:16-18 explicitly notes soft-dep on `nexus.backlinks` deliberately omitted from `dependsOn` so status-bar doesn't break when backlinks default-off.
- **Sources:** shell/src/plugins/nexus/statusBar/index.tsx:18, :50; IndexingStatus.tsx:19, :52

### nexus.tags
- **dependsOn (manifest):** none
- **IPC calls:** `com.nexus.storage::read_frontmatter`, `query_tags`
- **API surfaces:** `api.viewRegistry`, `api.commands`, `api.events`
- **Cross-plugin imports:** `import { useEditorStore } from '../editor/editorStore'`; `import { getKernel } from '../files/kernelClient'`
- **Events subscribed:** none; emits `files:open`
- **Hidden couplings:** same anti-pattern as outgoingLinks — reaches into nexus.editor + nexus.files without declaring deps; uses files' kernelClient singleton.
- **Sources:** shell/src/plugins/nexus/tags/index.tsx:5-6, :13, :74, :86

### nexus.templates
- **dependsOn (manifest):** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`
- **IPC calls:** `com.nexus.templates::list`, `apply`
- **API surfaces:** `api.kernel.invoke`, `api.kernel.available`, `api.viewRegistry`, `api.activityBar`, `api.commands`, `api.events`, `api.input.prompt`, `api.notifications`
- **Cross-plugin imports:** `../../../workspace`
- **Events subscribed:** `workspace:opened`, `workspace:closed`
- **Hidden couplings:** executes command `nexus.files.openByPath` which lives in nexus.files — not declared.
- **Sources:** shell/src/plugins/nexus/templates/index.ts:8-10, :47, :76, :243

### nexus.terminal
- **dependsOn (manifest):** `nexus.workspace`, `nexus.activityBar`
- **IPC calls:** `com.nexus.terminal::create_session`, `close_session`, `read_raw_since`; subscribes to topic prefix `com.nexus.terminal.output.`
- **API surfaces:** `api.kernel.invoke`, `api.kernel.on`, `api.kernel.available`, `api.viewRegistry`, `api.activityBar`, `api.commands`, `api.context`, `api.events`, `api.platform.shell.openExternal`, `api.configuration`
- **Cross-plugin imports:** `import { useWorkspaceStore } from '../workspace/workspaceStore'`
- **Events subscribed:** `workspace:opened`, `workspace:closed`, `nexus.terminal:focus`
- **Hidden couplings:** reaches into nexus.workspace's own store directly rather than via host `workspace` global — undeclared structural coupling (but the dep IS in manifest).
- **Sources:** shell/src/plugins/nexus/terminal/index.ts:29, :84, :213, :237, :273

### nexus.themePicker
- **dependsOn (manifest):** `nexus.activityBar` (catalog also lists `core.theme-service`)
- **IPC calls:** `com.nexus.theme::compute_variables`, `apply_theme`, `set_plugin_overrides`, `reload` (in ThemeBuilder.tsx)
- **API surfaces:** `api.commands`, `api.context`, `api.views`, `api.activityBar`, `api.kernel.invoke`, `api.kernel.on`
- **Cross-plugin imports:** `../../../stores/themeStore`
- **Events subscribed:** kernel topic `THEME_CHANGED_EVENT` (`com.nexus.theme.changed`)
- **Hidden couplings:** Heavy use of `getPickerApi()` module-scope singleton inside ThemeBuilder. `core.theme-service` listed in catalog dependsOn but NOT in manifest — drift between catalog.ts:307 and the plugin's own manifest at index.ts:22.
- **Sources:** shell/src/plugins/nexus/themePicker/index.ts:22, :90; ThemeBuilder.tsx:214, :227, :342

### nexus.viewBuilder
- **dependsOn (manifest):** `nexus.workspace`
- **IPC calls:** `com.nexus.storage::list_dir`, `read_file`, `create_dir`, `write_file`, `delete_file` (in layoutsStore.ts + exporter.ts)
- **API surfaces:** `api.kernel`, `api.viewRegistry`, `api.activityBar`, `api.commands`, `api.events`, `api.kernel.available`
- **Cross-plugin imports:** `../../../workspace`
- **Events subscribed:** `workspace:opened`, `workspace:closed`
- **Hidden couplings:** writes layouts under `.forge/layouts/` and `.forge/plugins/` via raw storage IPC — bypasses any higher-level layout/plugin-scaffold contract.
- **Sources:** shell/src/plugins/nexus/viewBuilder/index.ts:51, :57; layoutsStore.ts:106, :160, :172; exporter.ts:278, :287

### nexus.workflow
- **dependsOn (manifest):** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`
- **IPC calls:** `com.nexus.workflow::list`, `run`, `validate`
- **API surfaces:** `api.kernel.invoke`, `api.kernel.available`, `api.viewRegistry`, `api.activityBar`, `api.commands`, `api.events`, `api.notifications`
- **Cross-plugin imports:** `../../../workspace`; `../constants` (sibling plugin-dir constants file)
- **Events subscribed:** `workspace:opened`, `workspace:closed`
- **Hidden couplings:** none beyond the standard `com.nexus.workflow` Rust dep.
- **Sources:** shell/src/plugins/nexus/workflow/index.ts:7, :18-26, :89, :117, :132, :175

### nexus.workspace
- **dependsOn (manifest):** none
- **IPC calls:** Tauri `invoke('shutdown_kernel')`, `init_forge`, `boot_kernel`, `boot_remote`, `kernel_is_booted`, `path_exists`, `get_shell_state` (bridge commands, not kernel handlers)
- **API surfaces:** `api.context`, `api.storage`, `api.events`, `api.commands`, `api.views.register` (statusBarLeft)
- **Cross-plugin imports:** none (its own workspaceStore lives in this plugin's dir; other plugins reach into it)
- **Events subscribed:** emits `workspace:opened`/`workspace:closed` — the cornerstone events many other plugins subscribe to
- **Hidden couplings:** uses `@tauri-apps/api/core` + `@tauri-apps/plugin-dialog` directly — by design (it's the host-platform-primitive bridge for kernel lifecycle).
- **Sources:** shell/src/plugins/nexus/workspace/index.ts:2-3, :132, :147, :150, :157
