// src/plugins/catalog.ts
//
// WI-43: Plugin curation catalog.
//
// SH-009: Each entry is a thin descriptor with just enough metadata for boot-
// time filtering (id, popoutCompatible, dependsOn) plus a `load()` factory
// that returns a Promise<Plugin>. Vite splits each dynamic import into its
// own chunk, so heavy plugins (editor, bases, canvas, graph, terminal, etc.)
// are not parsed until they're actually activated.
//
// The metadata fields (id, name, version, core, activationEvents, dependsOn)
// are duplicated from the plugin's own manifest so callers can filter/inspect
// without loading the module. Any mismatch is a bug in this file.

import type { Plugin } from '../types/plugin'

// ── PluginEntry ───────────────────────────────────────────────────────────────
// Lightweight descriptor. Load the plugin module only when needed.
export interface PluginEntry {
  readonly id: string
  readonly name: string
  readonly version: string
  readonly core: boolean
  readonly activationEvents: string[]
  readonly dependsOn?: string[]
  /** Short, user-facing summary shown in Settings → Plugins. */
  readonly description: string
  /**
   * SH-020: false for chrome-only plugins that contribute to slots the popout
   * shell does not render. Absent/true means the plugin runs in popouts.
   */
  readonly popoutCompatible?: boolean
  /**
   * BL-052 follow-up — historical ids this entry has been known by.
   * Read at boot to migrate the persisted `plugins.enabled` list when
   * a plugin id is renamed; lets users keep their enable/disable
   * state across the rename without manual intervention.
   *
   * The canonical id is the entry's `id` field; legacy ids should be
   * removed from this list once the rename has been in production
   * long enough that no on-disk config could still carry the old
   * value.
   */
  readonly legacyPluginIds?: readonly string[]
  load(): Promise<Plugin>
}

/**
 * BL-052 follow-up — collect every `legacyPluginIds` declaration in
 * the catalog into a flat `legacy → canonical` map. Used by `boot()`
 * to migrate the persisted `plugins.enabled` list across a plugin id
 * rename. Pure function — exported for unit tests.
 *
 * Throws when a legacy id is claimed by more than one canonical
 * entry, so the catalog can't get into an inconsistent state. Same
 * legacy id pointing back at its own canonical id is also rejected
 * (a typo, almost certainly).
 */
export function buildLegacyIdAliases(
  entries: ReadonlyArray<PluginEntry>,
): Record<string, string> {
  const aliases: Record<string, string> = {}
  for (const entry of entries) {
    for (const legacy of entry.legacyPluginIds ?? []) {
      if (legacy === entry.id) {
        throw new Error(
          `catalog: legacy id '${legacy}' on entry '${entry.id}' must differ from the canonical id`,
        )
      }
      const existing = aliases[legacy]
      if (existing && existing !== entry.id) {
        throw new Error(
          `catalog: legacy id '${legacy}' is claimed by both '${existing}' and '${entry.id}'`,
        )
      }
      aliases[legacy] = entry.id
    }
  }
  return aliases
}

// ──────────────────────────────────────────────────────────────────────────────
// Default-on set (~14) — loaded unconditionally at boot.
// ──────────────────────────────────────────────────────────────────────────────
export const DEFAULT_ON_PLUGINS: PluginEntry[] = [
  // ── Core services ──────────────────────────────────────────────────────────
  {
    id: 'core.configuration-service', name: 'Configuration Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    description: 'Reads and writes the shell config store; backs api.configuration for every plugin.',
    load: () => import('./core/configurationService').then(m => m.configurationServicePlugin),
  },
  {
    id: 'core.notification-service', name: 'Notification Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    dependsOn: ['core.configuration-service'],
    description: 'Toast + status notifications surfaced through api.notifications.',
    load: () => import('./core/notificationService').then(m => m.notificationServicePlugin),
  },
  {
    id: 'core.filesystem-service', name: 'File System Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    description: 'Forge-relative file IO with capability checks; the only path to user files.',
    load: () => import('./core/fileSystemService').then(m => m.fileSystemServicePlugin),
  },
  {
    id: 'core.settings', name: 'Settings',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['core.configuration-service', 'nexus.activityBar', 'nexus.pluginsMgmt'],
    description: 'The Settings panel — config sections, themes, keybindings, plugin management.',
    load: () => import('./core/settings').then(m => m.settingsPlugin),
  },
  {
    id: 'core.capabilityPrompt', name: 'Capability Prompt',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'Modal that asks the user to grant or deny high-risk plugin capabilities.',
    load: () => import('./core/capabilityPrompt').then(m => m.capabilityPromptPlugin),
  },
  {
    id: 'core.theme-service', name: 'Theme Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    description: 'Loads, switches, and persists colour themes; exposes CSS variables to the shell.',
    load: () => import('./core/themeService').then(m => m.themeServicePlugin),
  },
  {
    id: 'core.zoom', name: 'Zoom',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['core.configuration-service'],
    description: 'Application-wide UI zoom with persisted level (Ctrl+=, Ctrl+-, Ctrl+0).',
    load: () => import('./core/zoom').then(m => m.zoomPlugin),
  },
  // ── Workspace + git ────────────────────────────────────────────────────────
  {
    id: 'nexus.workspace', name: 'Workspace',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Tabs, splits, and pane state — the leaf/view layout that hosts every editor.',
    load: () => import('./nexus/workspace').then(m => m.workspacePlugin),
  },
  {
    id: 'nexus.gitStatus', name: 'Git Status',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.statusBar', 'com.nexus.git'],
    description: "Status bar branch + dirty indicator for the forge's git repository.",
    load: () => import('./nexus/gitStatus').then(m => m.gitStatusPlugin),
  },
  {
    id: 'nexus.gitPanel', name: 'Git Panel',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.gitStatus', 'com.nexus.git'],
    description: 'Source control sidebar — staged/unstaged files, commit UI, branch picker, and commit log.',
    load: () => import('./nexus/gitPanel').then(m => m.gitPanelPlugin),
  },
  {
    id: 'nexus.collab', name: 'Collaboration',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.activityBar'],
    description:
      'BL-143 — peers panel showing live collaborators, their current file and cursor block, and the relay connection state. Wired to com.nexus.collab.* bus events; only meaningful when [collab] is enabled in .forge/config.toml.',
    load: () => import('./nexus/collab').then(m => m.collabPlugin),
  },
  // ── Chrome ─────────────────────────────────────────────────────────────────
  {
    id: 'nexus.activityBar', name: 'Activity Bar',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'The left-edge icon rail that switches between sidebar views.',
    load: () => import('./nexus/activityBar').then(m => m.activityBarPlugin),
  },
  {
    id: 'nexus.rightPanel', name: 'Right Panel',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'The right-side panel for outline, properties, backlinks and similar context views.',
    load: () => import('./nexus/rightPanel').then(m => m.rightPanelPlugin),
  },
  {
    id: 'nexus.statusBar', name: 'Status Bar',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.editor'],
    description: 'Bottom status bar — cursor position, word count, encoding, plugin badges.',
    load: () => import('./nexus/statusBar').then(m => m.statusBarPlugin),
  },
  {
    id: 'nexus.launcher', name: 'Launcher',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace'],
    description: 'Empty-workspace welcome screen with quick links to recent forges and docs.',
    load: () => import('./nexus/launcher').then(m => m.launcherPlugin),
  },
  // ── Editing surface ────────────────────────────────────────────────────────
  {
    id: 'nexus.files', name: 'Files',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'com.nexus.storage'],
    description: 'File-tree explorer with create / rename / move / delete and drag-and-drop.',
    load: () => import('./nexus/files').then(m => m.filesPlugin),
  },
  {
    id: 'nexus.editor', name: 'Editor',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: [
      'nexus.workspace', 'nexus.files', 'nexus.comments',
      'com.nexus.storage', 'com.nexus.git', 'com.nexus.editor', 'com.nexus.ai',
    ],
    description: 'CodeMirror-based markdown editor with live preview, snippets and link-aware extensions.',
    load: () => import('./nexus/editor').then(m => m.editorPlugin),
  },
  {
    id: 'nexus.outline', name: 'Outline',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel', 'nexus.editor'],
    description: 'Heading outline of the active document, with click-to-jump navigation.',
    load: () => import('./nexus/outline').then(m => m.outlinePlugin),
  },
  {
    id: 'nexus.crdtConflict', name: 'CRDT Conflict',
    version: '0.2.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.collab'],
    description:
      'BL-007 / BL-074 — surfaces CRDT pull-landing conflicts (concurrent block edits, delete-vs-edit) as a toast so the user knows a merge needs review. Resolver modal is a deferred follow-up.',
    load: () => import('./nexus/crdtConflict').then(m => m.crdtConflictPlugin),
  },
  {
    id: 'nexus.notifications', name: 'Notifications',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description:
      'BL-133 — subscribes to com.nexus.notifications.delivered (Desktop channel) and routes the payload through api.notifications.show. Without this plugin the bus event fires with no observable effect.',
    load: () => import('./nexus/notifications').then(m => m.notificationsPlugin),
  },
  {
    id: 'nexus.notificationsInbox', name: 'Notification Center',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description:
      'BL-136 — persistent inbox panel for notifications. Reads from com.nexus.notifications::inbox_list and lets the user mark read / dismiss / jump-to-source. Without this plugin notifications are still toasted live by nexus.notifications, but the history is not surfaced.',
    load: () => import('./nexus/notificationsInbox').then(m => m.notificationsInboxPlugin),
  },
  {
    id: 'nexus.dreamCycle', name: 'Dream Cycle',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description:
      'BL-129 follow-up — subscribes to com.nexus.dream_cycle.proposals and toasts "N new relation proposals from Dream Cycle" after a nightly run. The per-row approve/skip inbox is gated on a future list_draft_relations IPC.',
    load: () => import('./nexus/dreamCycle').then(m => m.dreamCyclePlugin),
  },
  {
    id: 'nexus.diagnostics', name: 'Diagnostics',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.paneMode', 'nexus.activityBar', 'nexus.workspace', 'nexus.editor'],
    description:
      'BL-141 follow-up — global LSP diagnostics panel. Subscribes to com.nexus.lsp.textDocument.publishDiagnostics, groups errors / warnings / info / hints by file with click-to-jump, and ships an "Open all in multibuffer" button that funnels every in-forge diagnostic through editor.open_excerpts.',
    load: () => import('./nexus/diagnostics').then(m => m.diagnosticsPlugin),
  },
  {
    id: 'nexus.notificationsSettings', name: 'Notifications Settings',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description:
      'BL-133 follow-up — Settings → Notifications tab. Per-channel credential entry (Discord/Telegram/SMTP) backed by the OS keyring + "Send test" buttons that dispatch com.nexus.notifications::send directly.',
    load: () => import('./nexus/notificationsSettings').then(m => m.notificationsSettingsPlugin),
  },
  // ── UX primitives ──────────────────────────────────────────────────────────
  {
    id: 'nexus.commandPalette', name: 'Command Palette',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Ctrl+P fuzzy-finder for every command contributed by the shell and plugins.',
    load: () => import('./nexus/commandPalette').then(m => m.commandPalettePlugin),
  },
  {
    id: 'nexus.confirm', name: 'Confirm',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Generic confirm / prompt modal exposed to plugins via api.ui.confirm.',
    load: () => import('./nexus/confirm').then(m => m.confirmPlugin),
  },
  {
    id: 'nexus.pick', name: 'Pick',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'BL-077 follow-up — list-picker modal exposed to plugins via api.input.pick. Backs LSP code-actions, future quick-pick surfaces.',
    load: () => import('./nexus/pick').then(m => m.pickPlugin),
  },
  {
    id: 'nexus.prompt', name: 'Prompt',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Styled prompt modal exposed to plugins via api.input.prompt — replaces the platform window.prompt fallback.',
    load: () => import('./nexus/prompt').then(m => m.promptPlugin),
  },
  {
    id: 'nexus.paneMode', name: 'Pane Mode',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'Toggles edit / read / source modes on the active pane and persists per-leaf state.',
    load: () => import('./nexus/paneMode').then(m => m.paneModePlugin),
  },
  {
    id: 'nexus.themePicker', name: 'Theme Picker',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['core.theme-service', 'nexus.activityBar', 'com.nexus.theme'],
    description: 'Browse and apply colour themes — Ctrl+Shift+T opens the picker overlay.',
    load: () => import('./nexus/themePicker').then(m => m.themePickerPlugin),
  },
  // ── Search ─────────────────────────────────────────────────────────────────
  {
    id: 'nexus.search', name: 'Search',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'com.nexus.storage'],
    description: 'Full-text search across the forge with path:, tag: and prop: operators.',
    load: () => import('./nexus/search').then(m => m.searchPlugin),
  },
  // ── View creators ──────────────────────────────────────────────────────────
  {
    id: 'nexus.canvas', name: 'Canvas',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.editor'],
    description: 'Infinite-canvas surface for sketching, mind-maps and visual note layout.',
    load: () => import('./nexus/canvas').then(m => m.canvasPlugin),
  },
  {
    id: 'nexus.bases', name: 'Bases',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'com.nexus.storage', 'com.nexus.database'],
    description: 'Database-style table views over notes, filtered by frontmatter properties.',
    load: () => import('./nexus/bases').then(m => m.basesPlugin),
  },
  // ── Plugin management ──────────────────────────────────────────────────────
  {
    id: 'nexus.pluginsMgmt', name: 'Plugins',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'Standalone plugin manager modal — toggle, review capabilities, scan drop folders.',
    load: () => import('./nexus/pluginsMgmt').then(m => m.pluginsMgmtPlugin),
  },
  {
    id: 'nexus.memory', name: 'Memory',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'Persistent agent memory store — long-term facts surfaced to AI features.',
    load: () => import('./nexus/memory').then(m => m.memoryPlugin),
  },
]

// ──────────────────────────────────────────────────────────────────────────────
// Default-off set — shipped but dormant. Enable per-row from
// Settings > Plugins; enabled ids are persisted into the
// `plugins.enabled: string[]` config value and picked up on next boot.
// ──────────────────────────────────────────────────────────────────────────────
export const DEFAULT_OFF_PLUGINS: PluginEntry[] = [
  {
    id: 'nexus.ai', name: 'AI',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Inline AI assistant — chat, edit, completion. Ctrl+I opens the side dock.',
    load: () => import('./nexus/ai').then(m => m.aiPlugin),
  },
  {
    id: 'nexus.semanticSearch', name: 'Semantic Search',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['com.nexus.storage', 'com.nexus.ai'],
    description: 'Embedding-based search across the forge — find notes by meaning, not keyword.',
    load: () => import('./nexus/semanticSearch').then(m => m.semanticSearchPlugin),
  },
  {
    id: 'nexus.linkSuggest', name: 'Link Suggest',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'AI-powered suggestions for [[wiki-links]] you might want to add to the current note.',
    load: () => import('./nexus/linkSuggest').then(m => m.linkSuggestPlugin),
  },
  {
    id: 'nexus.recall', name: 'Recall',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Spaced-repetition flashcards generated from highlights and frontmatter blocks.',
    load: () => import('./nexus/recall').then(m => m.recallPlugin),
  },
  {
    id: 'nexus.audio', name: 'Audio (Web Speech)',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'BL-118 — Speak and transcribe via the browser\'s Web Speech API, with fallback to the kernel-side audio backend.',
    load: () => import('./nexus/audio').then(m => m.audioPlugin),
  },
  {
    id: 'nexus.enrich', name: 'Enrich',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'AI enrichment pipeline — auto-tag, summarise, and extract entities from notes.',
    load: () => import('./nexus/enrich').then(m => m.enrichPlugin),
  },
  {
    id: 'nexus.agent', name: 'Agent',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Autonomous multi-step agent runner with planning and tool-use over your forge.',
    load: () => import('./nexus/agent').then(m => m.agentPlugin),
  },
  {
    id: 'nexus.mcp', name: 'MCP',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Model Context Protocol bridge — connect external MCP servers as tools and resources.',
    load: () => import('./nexus/mcp').then(m => m.mcpPlugin),
  },
  {
    id: 'nexus.workflow', name: 'Workflow',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Author and run multi-step workflows that chain commands, prompts and scripts.',
    load: () => import('./nexus/workflow').then(m => m.workflowPlugin),
  },
  {
    id: 'nexus.skills', name: 'Skills',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Reusable AI skill library — invoke saved prompts and automations from the palette.',
    load: () => import('./nexus/skills').then(m => m.skillsPlugin),
  },
  {
    id: 'nexus.templates', name: 'Templates',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.files'],
    description: 'Insert templated notes from a library, with variable expansion and folder targeting.',
    load: () => import('./nexus/templates').then(m => m.templatesPlugin),
  },
  {
    id: 'nexus.notionImport', name: 'Notion Import',
    version: '0.2.0', core: false, activationEvents: ['onStartup'],
    legacyPluginIds: ['nexus.notion'],
    description: 'Import Notion exports as markdown, preserving frontmatter and attachment links.',
    load: () => import('./nexus/notion').then(m => m.notionPlugin),
  },
  {
    id: 'nexus.terminal', name: 'Terminal',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'com.nexus.terminal'],
    description: 'Embedded shell tabs running inside the panel area, scoped to the forge directory.',
    load: () => import('./nexus/terminal').then(m => m.terminalPlugin),
  },
  {
    id: 'nexus.processes', name: 'Processes',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'View and manage long-running background tasks (indexers, agents, MCP servers).',
    load: () => import('./nexus/processes').then(m => m.processesPlugin),
  },
  {
    id: 'nexus.activity', name: 'Activity',
    version: '0.2.0', core: false, activationEvents: ['onStartup'],
    legacyPluginIds: ['nexus.activityTimeline'],
    description: 'Chronological feed of every observable side effect — AI calls, file writes, git commits, terminal sessions, workflow runs.',
    load: () => import('./nexus/activityTimeline').then(m => m.activityTimelinePlugin),
  },
  {
    id: 'nexus.graph', name: 'Graph',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Local link-graph visualisation centred on the active note.',
    load: () => import('./nexus/graph').then(m => m.graphPlugin),
  },
  {
    id: 'nexus.graph.global', name: 'Global Graph',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Forge-wide link graph — every note and edge, with clustering and search overlay.',
    load: () => import('./nexus/graph/globalIndex').then(m => m.graphGlobalPlugin),
  },
  {
    id: 'nexus.backlinks', name: 'Backlinks',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Side panel listing every note that links to the current document.',
    load: () => import('./nexus/backlinks').then(m => m.backlinksPlugin),
  },
  {
    id: 'nexus.comments', name: 'Comments',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Inline review comments anchored to selections, persisted alongside the note.',
    load: () => import('./nexus/comments').then(m => m.commentsPlugin),
  },
  {
    id: 'nexus.bookmarks', name: 'Bookmarks',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Pin notes, headings, and search queries for one-keystroke access.',
    load: () => import('./nexus/bookmarks').then(m => m.bookmarksPlugin),
  },
  {
    id: 'nexus.noteContext', name: 'Note Context',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.rightPanel', 'nexus.graph'],
    description:
      'Phase 4.3 — single right-panel accordion showing the active note\'s backlinks, outgoing links, tags, and a per-file graph. Default-off during the multi-step rollout (sibling plugins nexus.backlinks / nexus.outgoingLinks / nexus.tags stay live until step 6); enable manually to preview.',
    load: () => import('./nexus/noteContext').then(m => m.noteContextPlugin),
  },
  {
    id: 'nexus.healthPanel', name: 'Kernel Health',
    version: '0.1.0', core: false,
    activationEvents: ['onCommand:nexus.healthPanel.focus', 'onView:health-panel'],
    description:
      'BL-093 follow-up — kernel health panel. Polls com.nexus.security::metrics_snapshot every 5s and surfaces IPC counts + p50/p95/p99 latency, capability denials, event-bus queue depth, and the metrics-dropped sentinel. Default-off; targeted at developers triaging plugin behaviour.',
    load: () => import('./nexus/healthPanel').then(m => m.healthPanelPlugin),
  },
  {
    id: 'nexus.searchPanel', name: 'Search in Files',
    version: '0.1.0', core: false,
    activationEvents: ['onCommand:nexus.searchPanel.focus', 'onView:search-panel'],
    dependsOn: ['com.nexus.storage'],
    description:
      'BL-078 — multi-file find / replace across the forge. Plain-text, regex, case-sensitive, and whole-word; results grouped by file with click-to-open and per-file or workspace-wide replace.',
    load: () => import('./nexus/searchPanel').then(m => m.searchPanelPlugin),
  },
  {
    id: 'nexus.debugger', name: 'Debugger',
    version: '0.1.0', core: false,
    activationEvents: ['onCommand:nexus.debugger.focus', 'onView:debugger-panel'],
    description:
      'BL-081 — DAP debugger panel. Toolbar (continue / step over / step in / step out / pause / stop), call stack, scopes + variables, watch expressions, breakpoints, and output. Backed by `com.nexus.dap`; configure adapters in `<forge>/.forge/dap.toml`.',
    load: () => import('./nexus/debugger').then(m => m.debuggerPlugin),
  },
  {
    id: 'nexus.outgoingLinks', name: 'Outgoing Links',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.editor', 'nexus.files'],
    description: 'Side panel listing every link, embed, and unresolved reference in the active note.',
    load: () => import('./nexus/outgoingLinks').then(m => m.outgoingLinksPlugin),
  },
  {
    id: 'nexus.fileProperties', name: 'File Properties',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: "Edit the active note's frontmatter as a typed key/value form.",
    load: () => import('./nexus/fileProperties').then(m => m.filePropertiesPlugin),
  },
  {
    id: 'nexus.tags', name: 'Tags',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    dependsOn: ['nexus.editor', 'nexus.files'],
    description: 'Browse and filter notes by #tag, with counts and nested-tag drill-down.',
    load: () => import('./nexus/tags').then(m => m.tagsPlugin),
  },
  {
    id: 'community.mermaid', name: 'Mermaid',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    description: 'Renders ```mermaid``` code blocks as inline diagrams in read-mode previews.',
    load: () => import('./community/mermaid').then(m => m.mermaidPlugin),
  },
  {
    id: 'nexus.osArchitecture', name: 'Architecture',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'Render architecture.md as a domain → task hierarchy with drift detection against installed skills + workflows (BL-054 Phase 2). Pair with `nexus forge init --template os`.',
    load: () => import('./nexus/osArchitecture').then(m => m.osArchitecturePlugin),
  },
  {
    id: 'nexus.osObservability', name: 'Observability',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.activity'],
    description: 'Three-tab observability panel — usage rollup over the AI activity log, foundation-workflow status with manual run, and a vault-feed of file activity under raw/, wiki/, output/ (BL-054 Phase 4).',
    load: () => import('./nexus/observability').then(m => m.osObservabilityPlugin),
  },
  {
    id: 'nexus.viewBuilder', name: 'View Builder',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    description: 'Save / switch / delete named workspace layouts under .forge/layouts/. Read-only inventory of registered view types. Drag-drop canvas + export-as-plugin are deferred follow-ups (BL-067 Phase 1).',
    load: () => import('./nexus/viewBuilder').then(m => m.viewBuilderPlugin),
  },
]

/**
 * Flat catalog of every built-in plugin — used by tooling and the
 * PluginsMgmt UI to present the full known set regardless of enablement.
 */
export const ALL_PLUGINS: PluginEntry[] = [
  ...DEFAULT_ON_PLUGINS,
  ...DEFAULT_OFF_PLUGINS,
]

/**
 * Configuration key under which the user's manually-enabled default-off
 * plugin ids are persisted. Read at boot by main.tsx, written by the
 * PluginsMgmt UI's Enable button.
 */
export const PLUGINS_ENABLED_CONFIG_KEY = 'plugins.enabled'
