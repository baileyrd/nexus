// src/main.tsx

// Wipe any persisted themeStore state before the store imports itself
// and rehydrates. Earlier runs with core.theme-service loaded flipped
// data-theme='light' via the DOM and, on some paths, seeded this key
// with 'light'. Until we ship a theme-switcher UI, force dark on every
// boot. Runs before any module import so nothing reads stale values.
try { localStorage.removeItem('shell-theme') } catch {}
document.documentElement.dataset.theme = 'dark'

import React from 'react'
import ReactDOM from 'react-dom/client'
import { PluginRegistry } from './host/PluginRegistry'
import { ExtensionHost } from './host/ExtensionHost'
import { contextKeyService } from './host/ContextKeyService'
import { setRegistry } from './host/shellRegistry'
import App from './shell/App'
import './shell/shell.css'
// Importing the store triggers persist rehydration, which sets
// data-theme/data-density on <html> before the first paint.
import { useThemeStore } from './stores/themeStore'
import { useLayoutStore } from './stores/layoutStore'
import type { Plugin } from './types/plugin'
import {
  scanCommunityPlugins,
  loadEnabledCommunityPlugins,
} from './host/communityPluginLoader'

// ── Service plugins ───────────────────────────────────────────────────────────
// Infrastructure only — no UI, no hardcoded product content.
import { configurationServicePlugin } from './plugins/core/configurationService'
import { notificationServicePlugin }  from './plugins/core/notificationService'
import { fileSystemServicePlugin }    from './plugins/core/fileSystemService'
// themeServicePlugin is intentionally not loaded — it carries Nexus-branded
// theme names ("Forge Ember", "Forge Paper") and auto-flips to the light
// theme based on OS preference, which leaves empty slots looking white.
// Dark comes for free from shell.css :root when no data-theme is set.

// ── UI & feature plugins (DISABLED) ───────────────────────────────────────────
// The template's plugins/core/* UI files ship with hardcoded Nexus product
// content ("Forge", "Tantivy · 0 docs", stub SHAs, placeholder file counts).
// They are retained on disk as reference only and must NOT be loaded. Real
// UI will be rebuilt piece-by-piece as nexus.* plugins consuming real backend
// data. See memory: feedback_no_hardcoded_ui.md.

// ── Nexus plugins ─────────────────────────────────────────────────────────────
import { workspacePlugin } from './plugins/nexus/workspace'
import { gitStatusPlugin } from './plugins/nexus/gitStatus'
import { titleBarPlugin } from './plugins/nexus/titleBar'
import { activityBarPlugin } from './plugins/nexus/activityBar'
import { sidebarPlugin } from './plugins/nexus/sidebar'
import { rightPanelPlugin } from './plugins/nexus/rightPanel'
import { launcherPlugin } from './plugins/nexus/launcher'
import { filesPlugin } from './plugins/nexus/files'
import { editorPlugin } from './plugins/nexus/editor'
import { outlinePlugin } from './plugins/nexus/outline'
import { backlinksPlugin } from './plugins/nexus/backlinks'
import { graphPlugin } from './plugins/nexus/graph'
import { searchPlugin } from './plugins/nexus/search'
import { workflowPlugin } from './plugins/nexus/workflow'
import { skillsPlugin } from './plugins/nexus/skills'
import { mcpPlugin } from './plugins/nexus/mcp'
import { agentPlugin } from './plugins/nexus/agent'
import { confirmPlugin } from './plugins/nexus/confirm'
import { commandPalettePlugin } from './plugins/nexus/commandPalette'
import { paneModePlugin } from './plugins/nexus/paneMode'
import { terminalPlugin } from './plugins/nexus/terminal'
import { aiPlugin } from './plugins/nexus/ai'
import { pluginsMgmtPlugin } from './plugins/nexus/pluginsMgmt'
import { processesPlugin } from './plugins/nexus/processes'

function showFatal(message: string) {
  const root = document.getElementById('root')
  if (!root) return
  root.innerHTML = `
    <div style="padding:24px;font-family:system-ui;color:#f48771;background:#1e1e1e;height:100vh;overflow:auto;white-space:pre-wrap;font-size:12px;line-height:1.5;">
      <div style="font-size:14px;font-weight:600;margin-bottom:12px;">Shell failed to boot</div>
      ${message.replace(/</g, '&lt;')}
    </div>
  `
}

async function boot() {
  const reg  = new PluginRegistry()
  const host = new ExtensionHost(reg)

  // Expose via singleton — no circular import
  setRegistry(reg)

  // Force dark theme on boot. themeStore may have 'light' persisted
  // from when core.theme-service was auto-flipping based on OS
  // preference, and we have no theme-switcher UI to correct it yet.
  // Applies data-theme="dark" to <html> so shell.css :root tokens win.
  useThemeStore.getState().setTheme('dark')

  // panelArea is host to nexus.terminal as of Phase 2 item j, but it
  // stays hidden on boot — the user toggles it via Ctrl+Backquote or
  // the terminal activity-bar item. Force-hide here because the
  // persisted layoutStore may have been left visible by an earlier
  // session. rightPanel is owned by nexus.rightPanel — it flips
  // visibility on activate.
  useLayoutStore.setState((s) => ({
    panelArea:  { ...s.panelArea,  visible: false },
  }))

  const plugins: Plugin[] = [
    configurationServicePlugin,
    notificationServicePlugin,
    fileSystemServicePlugin,
    workspacePlugin,
    gitStatusPlugin,
    titleBarPlugin,
    activityBarPlugin,
    sidebarPlugin,
    rightPanelPlugin,
    launcherPlugin,
    filesPlugin,
    editorPlugin,
    outlinePlugin,
    backlinksPlugin,
    graphPlugin,
    searchPlugin,
    workflowPlugin,
    skillsPlugin,
    mcpPlugin,
    agentPlugin,
    confirmPlugin,
    commandPalettePlugin,
    paneModePlugin,
    terminalPlugin,
    aiPlugin,
    pluginsMgmtPlugin,
    processesPlugin,
  ]

  // Validate that all imports resolved to real plugins
  const missing = plugins
    .map((p, i) => [p, i] as const)
    .filter(([p]) => !p || !p.manifest)
  if (missing.length > 0) {
    throw new Error(
      `Plugin imports failed: indices ${missing.map(([, i]) => i).join(', ')}`
    )
  }

  console.info(`[Boot] Loading ${plugins.length} plugins...`)
  await host.loadAll(plugins)

  const all = host.listAll()
  all.forEach(({ id, state }) => {
    if (state === 'error') {
      console.error(`[Boot] FAILED: ${id}`, host.getError(id))
    } else {
      console.info(`[Boot] ${state === 'active' ? '✓' : '?'} ${id}: ${state}`)
    }
  })

  // ── Community plugins ──────────────────────────────────────────────────────
  // Scan for community plugins AFTER core loads so core services are available
  const communityManifests = await scanCommunityPlugins()

  // Register all discovered manifests (enabled + disabled) for the settings UI
  reg.registerService('communityPluginManifests', communityManifests)

  const communityPlugins = await loadEnabledCommunityPlugins(communityManifests)
  if (communityPlugins.length > 0) {
    console.info(`[Boot] Loading ${communityPlugins.length} community plugin(s)...`)
    await host.loadAll(communityPlugins)
  }

  // Expose plugin manifest + state list so the settings panel can show it
  reg.registerService('pluginList', plugins.map(p => ({
    id:      p.manifest.id,
    name:    p.manifest.name,
    version: p.manifest.version,
    core:    p.manifest.core,
    state:   host.getState(p.manifest.id) ?? 'unknown',
    error:   host.getError(p.manifest.id)?.message,
  })))

  const { useSlotStore } = await import('./registry/SlotRegistry')
  const slotSummary = Object.entries(useSlotStore.getState().slots)
    .map(([k, v]) => `${k}:${(v as any[]).length}`)
    .join(' ')
  console.info(`[Boot] Slots: ${slotSummary}`)

  contextKeyService.set('shellReady', true)
}

// Mount React IMMEDIATELY so the user sees SOMETHING even if boot fails mid-way.
// App renders a "Loading plugins..." placeholder until slots populate.
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
)

boot().catch(err => {
  const stack = err instanceof Error ? (err.stack ?? err.message) : String(err)
  console.error('[Boot] Fatal:', err)
  showFatal(stack)
})
