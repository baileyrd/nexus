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
import { listen } from '@tauri-apps/api/event'
import { uriHandlerRegistry } from './registry/UriHandlerRegistry'
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
import { SandboxOrchestrator } from './host/sandbox/SandboxOrchestrator'
import { buildPluginAPI } from './host/PluginAPI'
import { runInstallTimeConsent } from './plugins/core/capabilityPrompt'

// WI-43: built-in plugin registrations live in `plugins/catalog.ts` split
// into default-on (loaded unconditionally) and default-off (opt-in via
// Settings > Plugins, persisted under the `plugins.enabled` config key).
// See docs/planning/PHASE-5-IMPLEMENTATION-PLAN.md §2.
import {
  DEFAULT_ON_PLUGINS,
  DEFAULT_OFF_PLUGINS,
  ALL_PLUGINS,
  PLUGINS_ENABLED_CONFIG_KEY,
} from './plugins/catalog'
import { useConfigStore } from './stores/configStore'

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

  // WI-43: compose the registered set from the default-on list plus any
  // default-off plugins the user has explicitly enabled. The enabled
  // id list is persisted via the `configStore` (zustand + localStorage,
  // key `shell-config`) — the same pathway `api.configuration.setValue`
  // writes through. Reads are synchronous because the store rehydrates
  // on module import, before `boot()` runs.
  const enabledIds = new Set(
    useConfigStore.getState().get<string[]>(PLUGINS_ENABLED_CONFIG_KEY, []),
  )
  const optInPlugins = DEFAULT_OFF_PLUGINS.filter((p) =>
    enabledIds.has(p.manifest.id),
  )
  const plugins: Plugin[] = [...DEFAULT_ON_PLUGINS, ...optInPlugins]
  if (optInPlugins.length > 0) {
    console.info(
      `[Boot] ${optInPlugins.length} opt-in plugin(s) enabled: ` +
        optInPlugins.map((p) => p.manifest.id).join(', '),
    )
  }

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

  // WI-30e: instantiate the sandbox orchestrator once per boot and
  // share it across every sandboxed community plugin. Each
  // `orchestrator.load(spec)` call mounts an iframe + wires a router;
  // `orchestrator.disposeAll()` on shutdown tears every one down.
  //
  // The PluginAPI we hand to the router is built with a dedicated
  // pluginId — `community-sandbox` is a coarse label used only for
  // the per-API tracking registry entries the router writes as a
  // side-effect. Each router actually enforces its own pluginId when
  // it dispatches API calls, so a single `buildPluginAPI` call is
  // safe to share across multiple sandboxed plugins today.
  const sandboxApi = buildPluginAPI(reg, {
    pluginId: 'community-sandbox',
    isCore: false,
  })
  const sandboxOrchestrator = new SandboxOrchestrator({
    api: sandboxApi,
    registry: reg,
  })
  reg.registerService('sandboxOrchestrator', sandboxOrchestrator)

  // The guest runtime bootstrap (`bootstrapSandboxedPlugin`) is shared
  // across every sandboxed plugin. Build the blob URL once; the
  // loader hands it to each `orchestrator.load` call. Production will
  // swap this for a precompiled bundle served from the shell's
  // assets directory once the bundler lands (WI-30e §4 follow-on).
  let cachedRuntimeUrl: string | null = null
  const getRuntimeUrl = async (): Promise<string> => {
    if (cachedRuntimeUrl) return cachedRuntimeUrl
    // Stepping stone: for Phase 3c there is no precompiled runtime
    // bundle, and `hello-world/index.js` hand-rolls the protocol
    // inline (see its module header). We hand back a blob URL for a
    // trivial no-op module so the iframe's srcdoc can import
    // *something* at the `runtimeUrl` position without failing. Once
    // the bundler lands, this factory returns the compiled
    // `@nexus/extension-api/sandbox/runtime` entry.
    const shimSource =
      `// WI-30e runtime shim — replaced by the bundled ` +
      `bootstrapSandboxedPlugin entry once a plugin bundler lands.\n` +
      `export function bootstrapSandboxedPlugin(_plugin) { /* no-op: ` +
      `hand-rolled plugins bootstrap inline */ }\n`
    const blob = new Blob([shimSource], { type: 'application/javascript' })
    cachedRuntimeUrl = URL.createObjectURL(blob)
    return cachedRuntimeUrl
  }

  const communityPlugins = await loadEnabledCommunityPlugins(approvedManifests, {
    orchestrator: sandboxOrchestrator,
    getRuntimeUrl,
  })
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

  // WI-43: expose the default-off catalog entries that are NOT currently
  // enabled so the PluginsMgmt UI can render an "Available (disabled)"
  // section with per-row Enable buttons. The button writes the id into
  // `plugins.enabled` via the configuration service and prompts for a
  // reload — there is no in-session hot-activate path yet.
  const availablePlugins = DEFAULT_OFF_PLUGINS
    .filter((p) => !enabledIds.has(p.manifest.id))
    .map((p) => ({
      id:      p.manifest.id,
      name:    p.manifest.name,
      version: p.manifest.version,
      core:    p.manifest.core,
    }))
  reg.registerService('availablePlugins', availablePlugins)
  // Side-channel for the UI to announce how many total built-ins exist,
  // even when some are disabled — useful for the Plugins modal footer.
  reg.registerService('builtinPluginTotal', ALL_PLUGINS.length)

  const { useSlotStore } = await import('./registry/SlotRegistry')
  const slotSummary = Object.entries(useSlotStore.getState().slots)
    .map(([k, v]) => `${k}:${(v as any[]).length}`)
    .join(' ')
  console.info(`[Boot] Slots: ${slotSummary}`)

  // WI-13 follow-up: receive OS-level `nexus://…` deep-links from the
  // Rust side (see `shell/src-tauri/src/lib.rs` — `on_open_url`) and
  // dispatch through the shared registry. Fire-and-forget: a bad URL
  // string or a missing handler is logged, never thrown, so the deep
  // link pipe can't take down the shell.
  listen<string>('nexus:url-opened', (event) => {
    try {
      const url = new URL(event.payload)
      uriHandlerRegistry.dispatch(url)
    } catch (err) {
      console.warn('[Boot] deep-link payload not parseable:', event.payload, err)
    }
  }).catch((err) => {
    console.warn('[Boot] failed to register deep-link listener:', err)
  })

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
