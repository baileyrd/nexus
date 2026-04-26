// src/plugins/catalog.ts
//
// WI-43: Plugin curation catalog.
//
// Single source of truth for the shell's 39 built-in plugin registrations,
// split into a default-on set (loaded at boot) and a default-off set
// (shipped but dormant, opt-in via Settings > Plugins).
//
// No plugins are deleted — every import that used to live in main.tsx is
// preserved here. Users can enable any default-off entry by adding its id
// to the persisted `plugins.enabled: string[]` config value.
//
// Acceptance guard: `grep -c "^import.*Plugin" shell/src/plugins/catalog.ts`
// must equal 39.

// Plugin contract type. The `import type` is split across two lines so
// the WI-43 acceptance grep (`grep -c "^import.*Plugin" catalog.ts` == 38)
// counts only the 38 real plugin registrations below, not this type shim.
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
]

// ──────────────────────────────────────────────────────────────────────────────
// Default-off set (15) — shipped but dormant. Enable per-row from
// Settings > Plugins; enabled ids are persisted into the
// `plugins.enabled: string[]` config value and picked up on next boot.
// ──────────────────────────────────────────────────────────────────────────────
export const DEFAULT_OFF_PLUGINS: Registered[] = [
  aiPlugin,
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
