// src/main.tsx
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
import './stores/themeStore'
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
import { themeServicePlugin }         from './plugins/core/themeService'

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

  const plugins: Plugin[] = [
    configurationServicePlugin,
    notificationServicePlugin,
    fileSystemServicePlugin,
    themeServicePlugin,
    workspacePlugin,
    gitStatusPlugin,
    titleBarPlugin,
    activityBarPlugin,
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
