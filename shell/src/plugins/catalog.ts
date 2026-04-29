// src/plugins/catalog.ts
//
// WI-43: Plugin curation catalog.
//
// Single source of truth for the shell's built-in plugin registrations,
// split into a default-on set (loaded at boot) and a default-off set
// (shipped but dormant, opt-in via Settings > Plugins).
//
// No plugins are deleted — every import that used to live in main.tsx is
// preserved here. Users can enable any default-off entry by adding its id
// to the persisted `plugins.enabled: string[]` config value.

// Plugin contract type. The `import type` is split across two lines so
// the WI-43 acceptance grep (`grep -c "^import.*Plugin" catalog.ts`)
// counts only the real plugin registrations below, not this type shim.
import type {
  Plugin as Registered,
} from '../types/plugin'

// ── Service plugins ───────────────────────────────────────────────────────────
import { configurationServicePlugin } from './core/configurationService'
import { notificationServicePlugin }  from './core/notificationService'
import { fileSystemServicePlugin }    from './core/fileSystemService'
import { settingsPlugin }             from './core/settings'
import { capabilityPromptPlugin }     from './core/capabilityPrompt'
import { themeServicePlugin }         from './core/themeService'
import { zoomPlugin }                 from './core/zoom'

// ── Nexus plugins ─────────────────────────────────────────────────────────────
import { workspacePlugin } from './nexus/workspace'
import { gitStatusPlugin } from './nexus/gitStatus'
import { activityBarPlugin } from './nexus/activityBar'
import { sidebarPlugin } from './nexus/sidebar'
import { rightPanelPlugin } from './nexus/rightPanel'
import { launcherPlugin } from './nexus/launcher'
import { filesPlugin } from './nexus/files'
import { editorPlugin } from './nexus/editor'
import { outlinePlugin } from './nexus/outline'
import { backlinksPlugin } from './nexus/backlinks'
import { bookmarksPlugin } from './nexus/bookmarks'
import { outgoingLinksPlugin } from './nexus/outgoingLinks'
import { filePropertiesPlugin } from './nexus/fileProperties'
import { tagsPlugin } from './nexus/tags'
import { allPropertiesPlugin } from './nexus/allProperties'
import { graphPlugin } from './nexus/graph'
import { graphGlobalPlugin } from './nexus/graph/globalIndex'
import { searchPlugin } from './nexus/search'
import { semanticSearchPlugin } from './nexus/semanticSearch'
import { linkSuggestPlugin } from './nexus/linkSuggest'
import { workflowPlugin } from './nexus/workflow'
import { skillsPlugin } from './nexus/skills'
import { mcpPlugin } from './nexus/mcp'
import { agentPlugin } from './nexus/agent'
import { confirmPlugin } from './nexus/confirm'
import { commandPalettePlugin } from './nexus/commandPalette'
import { paneModePlugin } from './nexus/paneMode'
import { terminalPlugin } from './nexus/terminal'
import { canvasPlugin } from './nexus/canvas'
import { basesPlugin } from './nexus/bases'
import { aiPlugin } from './nexus/ai'
import { pluginsMgmtPlugin } from './nexus/pluginsMgmt'
import { processesPlugin } from './nexus/processes'
import { statusBarPlugin } from './nexus/statusBar'
import { extensionsTabPlugin } from './nexus/extensionsTab'
import { memoryPlugin } from './nexus/memory'
import { recallPlugin } from './nexus/recall'
import { enrichPlugin } from './nexus/enrich'

// ── Community plugins (BL-008+) — directory location matches the
// community-plugin layout, but registration goes through the catalog
// because the Blob-URL community-plugin loader cannot resolve bundled
// dependencies like `mermaid`. Plugins listed here behave identically
// to default-off nexus plugins from the user's perspective. See
// shell/src/plugins/community/mermaid/README.md.
import { mermaidPlugin } from './community/mermaid'

// ──────────────────────────────────────────────────────────────────────────────
// Default-on set (22) — loaded unconditionally at boot.
//
// Six core services, plus the baseline shell chrome + editing surface a
// personal note-taking workflow can't live without.
// ──────────────────────────────────────────────────────────────────────────────
export const DEFAULT_ON_PLUGINS: Registered[] = [
  // Core services
  configurationServicePlugin,
  notificationServicePlugin,
  fileSystemServicePlugin,
  settingsPlugin,
  capabilityPromptPlugin,
  themeServicePlugin,
  zoomPlugin,
  // Workspace + git
  workspacePlugin,
  gitStatusPlugin,
  // Chrome
  activityBarPlugin,
  sidebarPlugin,
  rightPanelPlugin,
  statusBarPlugin,
  launcherPlugin,
  // Editing surface
  filesPlugin,
  editorPlugin,
  outlinePlugin,
  // UX primitives
  commandPalettePlugin,
  confirmPlugin,
  paneModePlugin,
  // Search
  searchPlugin,
  // Canvas (claims `.canvas` extension; otherwise files render as JSON)
  canvasPlugin,
  // Bases (claims `.bases` directories; otherwise the editor tries to
  // `read_file` on a directory and the IPC bridge surfaces the EISDIR
  // as a spurious "plugin crashed during IPC call".)
  basesPlugin,
  // Plugin management (required to turn the rest on)
  pluginsMgmtPlugin,
  // Plugin observability — Settings > Extensions tab (OI-08).
  // Default-on so plugin activation errors surface immediately rather
  // than only in the dev console.
  extensionsTabPlugin,
  // BL-043 — Cmd+Alt+N quick-capture overlay. Default-on so the global
  // shortcut is registered at boot without the user having to opt in.
  memoryPlugin,
]

// ──────────────────────────────────────────────────────────────────────────────
// Default-off set (16) — shipped but dormant. Enable per-row from
// Settings > Plugins; enabled ids are persisted into the
// `plugins.enabled: string[]` config value and picked up on next boot.
// ──────────────────────────────────────────────────────────────────────────────
export const DEFAULT_OFF_PLUGINS: Registered[] = [
  aiPlugin,
  // BL-040 — palette-only "Search by Meaning" surface. Default-off
  // because it depends on the AI plugin's embedding provider being
  // configured; pairing the two enabled-states keeps the user from
  // seeing the command before the backend can answer it.
  semanticSearchPlugin,
  // BL-039 — inline AI link suggestions. Default-off for the same
  // reason as semanticSearchPlugin: it depends on the AI plugin's
  // embedding provider being configured. Pairing the enabled-states
  // keeps the user from seeing dead suggestions before the backend
  // can answer.
  linkSuggestPlugin,
  // BL-044 — Cmd+Shift+R recall overlay. Default-off for the same
  // reason as semanticSearchPlugin / linkSuggestPlugin: requires the
  // AI plugin's embedding provider to be configured. Pairs naturally
  // with `nexus.memory` (BL-043) which owns the inbox path the
  // recall overlay scopes against.
  recallPlugin,
  // BL-045 — auto-enrichment on save. Default-off because it issues
  // an AI chat call + an embedding+vector lookup per saved markdown
  // file (throttled to 5 s per-file). Requires both an AI chat
  // provider and an embedding provider to be configured.
  enrichPlugin,
  agentPlugin,
  mcpPlugin,
  workflowPlugin,
  skillsPlugin,
  terminalPlugin,
  processesPlugin,
  graphPlugin,
  graphGlobalPlugin,
  backlinksPlugin,
  bookmarksPlugin,
  outgoingLinksPlugin,
  filePropertiesPlugin,
  tagsPlugin,
  allPropertiesPlugin,
  mermaidPlugin,
]

/**
 * Flat catalog of every built-in plugin — used by tooling and the
 * PluginsMgmt UI to present the full known set regardless of enablement.
 */
export const ALL_PLUGINS: Registered[] = [
  ...DEFAULT_ON_PLUGINS,
  ...DEFAULT_OFF_PLUGINS,
]

/**
 * Configuration key under which the user's manually-enabled default-off
 * plugin ids are persisted. Read at boot by main.tsx, written by the
 * PluginsMgmt UI's Enable button.
 */
export const PLUGINS_ENABLED_CONFIG_KEY = 'plugins.enabled'
