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
  /**
   * SH-020: false for chrome-only plugins that contribute to slots the popout
   * shell does not render. Absent/true means the plugin runs in popouts.
   */
  readonly popoutCompatible?: boolean
  load(): Promise<Plugin>
}

// ──────────────────────────────────────────────────────────────────────────────
// Default-on set (~14) — loaded unconditionally at boot.
// ──────────────────────────────────────────────────────────────────────────────
export const DEFAULT_ON_PLUGINS: PluginEntry[] = [
  // ── Core services ──────────────────────────────────────────────────────────
  {
    id: 'core.configurationService', name: 'Configuration Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    load: () => import('./core/configurationService').then(m => m.configurationServicePlugin),
  },
  {
    id: 'core.notificationService', name: 'Notification Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    load: () => import('./core/notificationService').then(m => m.notificationServicePlugin),
  },
  {
    id: 'core.fileSystemService', name: 'File System Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    load: () => import('./core/fileSystemService').then(m => m.fileSystemServicePlugin),
  },
  {
    id: 'core.settings', name: 'Settings',
    version: '1.0.0', core: true, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['core.configuration-service', 'nexus.activityBar'],
    load: () => import('./core/settings').then(m => m.settingsPlugin),
  },
  {
    id: 'core.capabilityPrompt', name: 'Capability Prompt',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    popoutCompatible: false,
    load: () => import('./core/capabilityPrompt').then(m => m.capabilityPromptPlugin),
  },
  {
    id: 'core.themeService', name: 'Theme Service',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    load: () => import('./core/themeService').then(m => m.themeServicePlugin),
  },
  {
    id: 'core.zoom', name: 'Zoom',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./core/zoom').then(m => m.zoomPlugin),
  },
  // ── Workspace + git ────────────────────────────────────────────────────────
  {
    id: 'nexus.workspace', name: 'Workspace',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/workspace').then(m => m.workspacePlugin),
  },
  {
    id: 'nexus.gitStatus', name: 'Git Status',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace'],
    load: () => import('./nexus/gitStatus').then(m => m.gitStatusPlugin),
  },
  // ── Chrome ─────────────────────────────────────────────────────────────────
  {
    id: 'nexus.activityBar', name: 'Activity Bar',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    load: () => import('./nexus/activityBar').then(m => m.activityBarPlugin),
  },
  {
    id: 'nexus.sidebar', name: 'Sidebar',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    load: () => import('./nexus/sidebar').then(m => m.sidebarPlugin),
  },
  {
    id: 'nexus.rightPanel', name: 'Right Panel',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    load: () => import('./nexus/rightPanel').then(m => m.rightPanelPlugin),
  },
  {
    id: 'nexus.statusBar', name: 'Status Bar',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.editor'],
    load: () => import('./nexus/statusBar').then(m => m.statusBarPlugin),
  },
  {
    id: 'nexus.launcher', name: 'Launcher',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace'],
    load: () => import('./nexus/launcher').then(m => m.launcherPlugin),
  },
  // ── Editing surface ────────────────────────────────────────────────────────
  {
    id: 'nexus.files', name: 'Files',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/files').then(m => m.filesPlugin),
  },
  {
    id: 'nexus.editor', name: 'Editor',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/editor').then(m => m.editorPlugin),
  },
  {
    id: 'nexus.outline', name: 'Outline',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/outline').then(m => m.outlinePlugin),
  },
  // ── UX primitives ──────────────────────────────────────────────────────────
  {
    id: 'nexus.commandPalette', name: 'Command Palette',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/commandPalette').then(m => m.commandPalettePlugin),
  },
  {
    id: 'nexus.confirm', name: 'Confirm',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/confirm').then(m => m.confirmPlugin),
  },
  {
    id: 'nexus.paneMode', name: 'Pane Mode',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    load: () => import('./nexus/paneMode').then(m => m.paneModePlugin),
  },
  // ── Search ─────────────────────────────────────────────────────────────────
  {
    id: 'nexus.search', name: 'Search',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/search').then(m => m.searchPlugin),
  },
  // ── View creators ──────────────────────────────────────────────────────────
  {
    id: 'nexus.canvas', name: 'Canvas',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/canvas').then(m => m.canvasPlugin),
  },
  {
    id: 'nexus.bases', name: 'Bases',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/bases').then(m => m.basesPlugin),
  },
  // ── Plugin management ──────────────────────────────────────────────────────
  {
    id: 'nexus.pluginsMgmt', name: 'Plugins',
    version: '0.1.0', core: true, activationEvents: ['onStartup'],
    popoutCompatible: false,
    load: () => import('./nexus/pluginsMgmt').then(m => m.pluginsMgmtPlugin),
  },
  {
    id: 'nexus.extensionsTab', name: 'Extensions Tab',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
    load: () => import('./nexus/extensionsTab').then(m => m.extensionsTabPlugin),
  },
  {
    id: 'nexus.memory', name: 'Memory',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    popoutCompatible: false,
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
    load: () => import('./nexus/ai').then(m => m.aiPlugin),
  },
  {
    id: 'nexus.semanticSearch', name: 'Semantic Search',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/semanticSearch').then(m => m.semanticSearchPlugin),
  },
  {
    id: 'nexus.linkSuggest', name: 'Link Suggest',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/linkSuggest').then(m => m.linkSuggestPlugin),
  },
  {
    id: 'nexus.recall', name: 'Recall',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/recall').then(m => m.recallPlugin),
  },
  {
    id: 'nexus.enrich', name: 'Enrich',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/enrich').then(m => m.enrichPlugin),
  },
  {
    id: 'nexus.agent', name: 'Agent',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/agent').then(m => m.agentPlugin),
  },
  {
    id: 'nexus.mcp', name: 'MCP',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/mcp').then(m => m.mcpPlugin),
  },
  {
    id: 'nexus.workflow', name: 'Workflow',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/workflow').then(m => m.workflowPlugin),
  },
  {
    id: 'nexus.skills', name: 'Skills',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/skills').then(m => m.skillsPlugin),
  },
  {
    id: 'nexus.terminal', name: 'Terminal',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/terminal').then(m => m.terminalPlugin),
  },
  {
    id: 'nexus.processes', name: 'Processes',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/processes').then(m => m.processesPlugin),
  },
  {
    id: 'nexus.activityTimeline', name: 'Activity Timeline',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/activityTimeline').then(m => m.activityTimelinePlugin),
  },
  {
    id: 'nexus.graph', name: 'Graph',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/graph').then(m => m.graphPlugin),
  },
  {
    id: 'nexus.graph.global', name: 'Global Graph',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/graph/globalIndex').then(m => m.graphGlobalPlugin),
  },
  {
    id: 'nexus.backlinks', name: 'Backlinks',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/backlinks').then(m => m.backlinksPlugin),
  },
  {
    id: 'nexus.comments', name: 'Comments',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/comments').then(m => m.commentsPlugin),
  },
  {
    id: 'nexus.bookmarks', name: 'Bookmarks',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/bookmarks').then(m => m.bookmarksPlugin),
  },
  {
    id: 'nexus.outgoingLinks', name: 'Outgoing Links',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/outgoingLinks').then(m => m.outgoingLinksPlugin),
  },
  {
    id: 'nexus.fileProperties', name: 'File Properties',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/fileProperties').then(m => m.filePropertiesPlugin),
  },
  {
    id: 'nexus.tags', name: 'Tags',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/tags').then(m => m.tagsPlugin),
  },
  {
    id: 'nexus.allProperties', name: 'All Properties',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./nexus/allProperties').then(m => m.allPropertiesPlugin),
  },
  {
    id: 'community.mermaid', name: 'Mermaid',
    version: '0.1.0', core: false, activationEvents: ['onStartup'],
    load: () => import('./community/mermaid').then(m => m.mermaidPlugin),
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
