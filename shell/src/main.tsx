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
import { installBodyClasses } from './host/bodyClasses'
import { eventBus } from './host/EventBus'
import { invoke } from '@tauri-apps/api/core'
import App from './shell/App'
import './shell/shell.css'
// Importing the store triggers persist rehydration, which sets
// data-theme/data-density on <html> before the first paint.
import { useThemeStore } from './stores/themeStore'
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
import { settingsPlugin }             from './plugins/core/settings'
import {
  capabilityPromptPlugin,
  runInstallTimeConsent,
} from './plugins/core/capabilityPrompt'
// WI-02 part 2: themeServicePlugin is now a kernel-sync bridge, not the
// old in-process palette holder. It hydrates `useThemeStore` from the
// `com.nexus.theme` kernel plugin and subscribes to
// `com.nexus.theme.changed` so palette swaps from any source flow
// through the store and onto :root. The shell.css :root defaults
// remain in place so there's no flash if hydrate is slow.
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
import { activityBarPlugin } from './plugins/nexus/activityBar'
import { sidebarPlugin } from './plugins/nexus/sidebar'
import { rightPanelPlugin } from './plugins/nexus/rightPanel'
import { launcherPlugin } from './plugins/nexus/launcher'
import { filesPlugin } from './plugins/nexus/files'
import { editorPlugin } from './plugins/nexus/editor'
import { outlinePlugin } from './plugins/nexus/outline'
import { backlinksPlugin } from './plugins/nexus/backlinks'
import { bookmarksPlugin } from './plugins/nexus/bookmarks'
import { outgoingLinksPlugin } from './plugins/nexus/outgoingLinks'
import { filePropertiesPlugin } from './plugins/nexus/fileProperties'
import { tagsPlugin } from './plugins/nexus/tags'
import { allPropertiesPlugin } from './plugins/nexus/allProperties'
import { graphPlugin } from './plugins/nexus/graph'
import { graphGlobalPlugin } from './plugins/nexus/graph/globalIndex'
import { searchPlugin } from './plugins/nexus/search'
import { workflowPlugin } from './plugins/nexus/workflow'
import { skillsPlugin } from './plugins/nexus/skills'
import { mcpPlugin } from './plugins/nexus/mcp'
import { agentPlugin } from './plugins/nexus/agent'
import { confirmPlugin } from './plugins/nexus/confirm'
import { commandPalettePlugin } from './plugins/nexus/commandPalette'
import { paneModePlugin } from './plugins/nexus/paneMode'
import { terminalPlugin } from './plugins/nexus/terminal'
import { canvasPlugin } from './plugins/nexus/canvas'
import { basesPlugin } from './plugins/nexus/bases'
import { aiPlugin } from './plugins/nexus/ai'
import { pluginsMgmtPlugin } from './plugins/nexus/pluginsMgmt'
import { processesPlugin } from './plugins/nexus/processes'
import { statusBarPlugin } from './plugins/nexus/statusBar'

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

  // ── E2E test hook ─────────────────────────────────────────────────────────
  // Gated on VITE_E2E alone. The e2e build step (`pnpm e2e:build`) sets
  // VITE_E2E=true before `vite build`, so the flag is inlined at build time
  // and only ever present in e2e-targeted bundles. Production and normal
  // `pnpm dev`/`pnpm build` runs never set it. Note: `import.meta.env.DEV`
  // is false under `vite build`, so gating on it here would never let the
  // production e2e bundle expose the global — which is why we don't.
  if (import.meta.env.VITE_E2E === 'true') {
    ;(window as unknown as { __nexusShellApi: unknown }).__nexusShellApi = {
      // Shared subsystems (not plugin-scoped). Commands and events are
      // process-wide; the e2e harness dispatches through them exactly as a
      // registered plugin would.
      kernel: {
        invoke: <T = unknown>(
          pluginId: string,
          commandId: string,
          args: unknown = {},
          timeoutMs?: number,
        ): Promise<T> =>
          invoke<T>('kernel_invoke', {
            pluginId,
            commandId,
            args,
            timeoutMs: timeoutMs ?? null,
          }),
        available: (): Promise<boolean> => invoke<boolean>('kernel_is_booted'),
      },
      commands: {
        execute: (id: string, ...args: unknown[]) =>
          reg.commands.execute(id, ...args),
        all: () => reg.commands.all(),
      },
      events: {
        emit: (topic: string, payload: unknown) => eventBus.emit(topic, payload),
        on: <T = unknown>(topic: string, handler: (p: T) => void) =>
          eventBus.on<T>(topic, handler),
      },
      storage: {
        get: (key: string) => localStorage.getItem(key),
        set: (key: string, value: string) => localStorage.setItem(key, value),
        delete: (key: string) => localStorage.removeItem(key),
      },
      // Escape hatch for future specs that need deeper surface area without
      // another round-trip through this file.
      registry: reg,
    }
    console.info('[Boot] __nexusShellApi attached (VITE_E2E=true)')
  }

  // Force dark theme on boot. themeStore may have 'light' persisted
  // from when core.theme-service was auto-flipping based on OS
  // preference, and we have no theme-switcher UI to correct it yet.
  // Applies data-theme="dark" to <html> so shell.css :root tokens win.
  useThemeStore.getState().setTheme('dark')

  // Terminal visibility on boot: it is a Leaf inside the right sidedock
  // (see nexus.terminal). The user toggles it via Ctrl+Backquote or the
  // terminal activity-bar item; there is no boot-time force-hide since
  // the persisted workspace tree is authoritative.

  const plugins: Plugin[] = [
    configurationServicePlugin,
    notificationServicePlugin,
    fileSystemServicePlugin,
    settingsPlugin,
    capabilityPromptPlugin,
    themeServicePlugin,
    workspacePlugin,
    gitStatusPlugin,
    activityBarPlugin,
    sidebarPlugin,
    rightPanelPlugin,
    launcherPlugin,
    filesPlugin,
    editorPlugin,
    outlinePlugin,
    backlinksPlugin,
    bookmarksPlugin,
    outgoingLinksPlugin,
    filePropertiesPlugin,
    tagsPlugin,
    allPropertiesPlugin,
    graphPlugin,
    graphGlobalPlugin,
    searchPlugin,
    workflowPlugin,
    skillsPlugin,
    mcpPlugin,
    agentPlugin,
    confirmPlugin,
    commandPalettePlugin,
    paneModePlugin,
    terminalPlugin,
    canvasPlugin,
    basesPlugin,
    aiPlugin,
    pluginsMgmtPlugin,
    processesPlugin,
    statusBarPlugin,
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

  // WI-31: run the install-time consent prompt BEFORE activation. This
  // awaits any blocking modals (high-risk capabilities) and persists
  // the user's choices to each plugin's `granted_caps.json` — which is
  // what the kernel reads on the next forge boot. The returned
  // `denied` set filters plugins out of activation this session so a
  // denied plugin doesn't run with partial caps.
  let deniedCommunityIds: Set<string> = new Set()
  try {
    const consent = await runInstallTimeConsent(communityManifests)
    deniedCommunityIds = consent.denied
    if (deniedCommunityIds.size > 0) {
      console.info(
        `[Boot] ${deniedCommunityIds.size} community plugin(s) denied ` +
        `by consent prompt: ${[...deniedCommunityIds].join(', ')}`,
      )
    }
  } catch (err) {
    console.warn('[Boot] capability consent flow failed; continuing:', err)
  }
  // Expose the denied set so PluginsMgmt can render "denied" rows.
  reg.registerService('communityPluginDenied', deniedCommunityIds)

  const approvedManifests = communityManifests.filter(
    (m) => !deniedCommunityIds.has(m.id),
  )
  const communityPlugins = await loadEnabledCommunityPlugins(approvedManifests)
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

// Install Obsidian-faithful body-class state machine. Runs once, before
// React mounts, so platform / frameless / focus classes are present on
// first paint and CSS can key off them (`body.mod-windows`, etc.). The
// Tauri listeners it registers persist for the app lifetime.
installBodyClasses()

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
