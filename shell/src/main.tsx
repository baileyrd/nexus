// src/main.tsx

// Color tokens come from the kernel theme via the `--nx-*` bridge in
// index.html. The legacy `data-theme` attribute is no longer used.

import React from 'react'
import ReactDOM from 'react-dom/client'
import { clientLogger } from './host/clientLogger'
import { PluginRegistry } from './host/PluginRegistry'
import { ExtensionHost } from './host/ExtensionHost'
import { contextKeyService } from './host/ContextKeyService'
import { setRegistry } from './host/shellRegistry'
import { setHost } from './host/shellHost'
import { installBodyClasses } from './host/bodyClasses'
import { eventBus } from './host/EventBus'
import { PLUGIN_LIST_CHANGED_EVENT } from './host/pluginActivation'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { uriHandlerRegistry } from './registry/UriHandlerRegistry'
import App from './shell/App'
import {
  PopoutShell,
  isPopoutMode,
  POPOUT_CLOSED_EVENT,
  POPOUT_BOUNDS_CHANGED_EVENT,
} from './shell/PopoutShell'
import { workspace as workspaceStore } from './workspace/workspaceStore'
import { closePopoutTauri } from './workspace/popoutWindowBridge'
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
// F-8.1.1-fo1: virtual module emitted by `vite.sandbox-runtime-plugin.ts`
// — a string holding the bundled `bootstrapSandboxedPlugin` runtime. We
// blob-wrap it once at boot and hand the URL to every sandboxed plugin
// load via `getRuntimeUrl`, so plugins no longer need to hand-roll the
// postMessage protocol.
import sandboxRuntimeSource from 'virtual:sandbox-runtime'

// WI-43 / SH-009: built-in plugin registrations live in `plugins/catalog.ts`
// as PluginEntry descriptors with dynamic-import factories. Default-on entries
// are loaded at boot; default-off entries are loaded only when the user enables
// them. Each dynamic import becomes a separate Vite chunk (vendor libs are
// grouped via manualChunks in vite.config.ts).
import {
  ALL_PLUGINS,
  DEFAULT_ON_PLUGINS,
  DEFAULT_OFF_PLUGINS,
  PLUGINS_ENABLED_CONFIG_KEY,
  buildLegacyIdAliases,
} from './plugins/catalog'
import { useConfigStore } from './stores/configStore'
import { keybindingOverrideStorage } from './registry/keybindingOverrideStorage'

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

async function boot(opts: { popoutMode?: boolean } = {}) {
  const popoutMode = opts.popoutMode === true

  // BL-029 Phase 2b — set the popoutMode context key BEFORE plugin
  // activation so plugins (`nexus.workspace` chiefly) can adapt their
  // boot path. ADR 0020 §1: the popout boots the same DEFAULT_ON
  // plugin set as the main window, but skips kernel-lifecycle calls
  // (the kernel is owned by the main window via Tauri managed state).
  if (popoutMode) {
    contextKeyService.set('popoutMode', true)
  }

  const reg  = new PluginRegistry()
  const host = new ExtensionHost(reg)

  // Bind override storage before any plugin activates so keybinding
  // overrides are hydrated before the first key dispatch.
  reg.keybindings.bindStorage(keybindingOverrideStorage)
  void reg.keybindings.loadOverrides()

  // Expose via singletons — no circular import
  setRegistry(reg)
  setHost(host)

  // OI-16 — graceful shutdown hook for script plugins. `beforeunload`
  // fires on Cmd+Q (when Tauri delegates to the WebView), Ctrl+R, HMR
  // reload, and any other navigation that tears down the page. The
  // deactivate fan-out is gated by a 1s per-plugin soft cap so a single
  // misbehaving plugin can't appear to hang the close. Plugins that
  // need a guaranteed flush should write synchronously — the browser
  // doesn't reliably await async work past `beforeunload` — but
  // fire-and-forget cleanup gets a fighting chance, and the path runs
  // to completion under programmatic / HMR reload where the WebView
  // isn't actually disposed.
  window.addEventListener('beforeunload', () => {
    void host.deactivateAllForShutdown(1000)
  })

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
    clientLogger.info('[Boot] __nexusShellApi attached (VITE_E2E=true)')
  }

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
  //
  // One-time migration: catalog IDs were corrected to match their plugin
  // manifest IDs. Remap any stale stored IDs so existing users keep
  // their enabled-plugin selections after the rename.
  //
  // Two sources contribute to the migration map:
  //   - Hardcoded renames preserved from before the catalog grew a
  //     `legacyPluginIds` field. Kept for users who upgraded before
  //     the field landed; new renames should declare
  //     `legacyPluginIds` on the catalog entry instead.
  //   - BL-052 follow-up — every entry's `legacyPluginIds` flows
  //     through `buildLegacyIdAliases` so the catalog itself
  //     declares its own back-compat aliases.
  const HARDCODED_RENAMES: Record<string, string> = {
    'nexus.graphGlobal': 'nexus.graph.global',
    'nexus.mermaid':     'community.mermaid',
  }
  const CATALOG_ID_RENAMES: Record<string, string> = {
    ...HARDCODED_RENAMES,
    ...buildLegacyIdAliases(ALL_PLUGINS),
  }
  const rawEnabledIds = useConfigStore.getState().get<string[]>(PLUGINS_ENABLED_CONFIG_KEY, [])
  const migratedEnabledIds = rawEnabledIds.map(id => CATALOG_ID_RENAMES[id] ?? id)
  if (migratedEnabledIds.some((id, i) => id !== rawEnabledIds[i])) {
    useConfigStore.getState().set(PLUGINS_ENABLED_CONFIG_KEY, migratedEnabledIds)
  }
  const enabledIds = new Set(migratedEnabledIds)
  const optInEntries = DEFAULT_OFF_PLUGINS.filter(e => enabledIds.has(e.id))
  // SH-020: popout windows skip chrome-only plugins (activity bar, sidebar,
  // status bar, settings, etc.) that contribute to slots the popout shell
  // does not render. Plugins opt out by setting `popoutCompatible: false`
  // in their entry; absence defaults to true.
  const defaultOnSet = popoutMode
    ? DEFAULT_ON_PLUGINS.filter(e => e.popoutCompatible !== false)
    : DEFAULT_ON_PLUGINS
  // SH-009: dynamic-import factories — load all selected plugin modules in
  // parallel before handing them to the host.
  const plugins: Plugin[] = await Promise.all(
    [...defaultOnSet, ...optInEntries].map(e => e.load()),
  )
  if (optInEntries.length > 0) {
    clientLogger.info(
      `[Boot] ${optInEntries.length} opt-in plugin(s) enabled: ` +
        optInEntries.map(e => e.id).join(', '),
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

  clientLogger.info(`[Boot] Loading ${plugins.length} plugins...`)
  await host.loadAll(plugins)

  const all = host.listAll()
  all.forEach(({ id, state }) => {
    if (state === 'error') {
      clientLogger.error(`[Boot] FAILED: ${id}`, host.getError(id))
    } else {
      clientLogger.info(`[Boot] ${state === 'active' ? '✓' : '?'} ${id}: ${state}`)
    }
  })

  // ── Community plugins ──────────────────────────────────────────────────────
  // ADR 0020 §1 — popouts skip the community-plugin scan + install-time
  // consent + sandbox orchestrator. Community plugins primarily contribute
  // to the main-window chrome and the sandbox bootstrap is non-trivial cost
  // we pay once per JS context. Popout mode short-circuits to the slot
  // summary + popout-close listener so the rest of the boot tail still
  // runs.
  if (popoutMode) {
    contextKeyService.set('shellReady', true)
    return
  }

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
      clientLogger.info(
        `[Boot] ${deniedCommunityIds.size} community plugin(s) denied ` +
        `by consent prompt: ${[...deniedCommunityIds].join(', ')}`,
      )
    }
  } catch (err) {
    clientLogger.warn('[Boot] capability consent flow failed; continuing:', err)
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
  // F-8.1.2: the orchestrator builds a *per-plugin* `PluginAPI` via
  // this factory using the authoritative pluginId set at handshake
  // time. Storage namespacing (`plugin:<id>:…`), event tagging
  // (`activityBar:itemAdded`, `settings:tabsChanged`), and registry
  // ownership all derive from the boundary id — there is no shared
  // 'community-sandbox' bucket that would cross-pollute concurrent
  // sandboxed plugins. `assertValidPluginId` inside `buildPluginAPI`
  // is the choke point that rejects colon-bearing / empty / non-string
  // ids before any of those derived keys can be written.
  const sandboxOrchestrator = new SandboxOrchestrator({
    apiFactory: (sandboxedPluginId) =>
      buildPluginAPI(reg, {
        pluginId: sandboxedPluginId,
        isCore: false,
      }),
    registry: reg,
  })
  reg.registerService('sandboxOrchestrator', sandboxOrchestrator)

  // The guest runtime bootstrap (`bootstrapSandboxedPlugin`) is shared
  // across every sandboxed plugin. The runtime source is bundled at
  // Vite build time by `vite.sandbox-runtime-plugin.ts` and imported
  // as the virtual module `virtual:sandbox-runtime`. We Blob-wrap it
  // once on first call; subsequent calls return the cached URL so a
  // single iframe-import per session pays the cost.
  let cachedRuntimeUrl: string | null = null
  const getRuntimeUrl = async (): Promise<string> => {
    if (cachedRuntimeUrl) return cachedRuntimeUrl
    const blob = new Blob([sandboxRuntimeSource], {
      type: 'application/javascript',
    })
    cachedRuntimeUrl = URL.createObjectURL(blob)
    return cachedRuntimeUrl
  }

  const communityPlugins = await loadEnabledCommunityPlugins(approvedManifests, {
    orchestrator: sandboxOrchestrator,
    getRuntimeUrl,
  })
  if (communityPlugins.length > 0) {
    clientLogger.info(`[Boot] Loading ${communityPlugins.length} community plugin(s)...`)
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
  // SH-009: use PluginEntry metadata directly; no need to load the module.
  const availablePlugins = DEFAULT_OFF_PLUGINS
    .filter(e => !enabledIds.has(e.id))
    .map(e => ({
      id:      e.id,
      name:    e.name,
      version: e.version,
      core:    e.core,
    }))
  reg.registerService('availablePlugins', availablePlugins)
  // Side-channel for the UI to announce how many total built-ins exist,
  // even when some are disabled — useful for the Plugins modal footer.
  reg.registerService('builtinPluginTotal', DEFAULT_ON_PLUGINS.length + DEFAULT_OFF_PLUGINS.length)

  // Plugins activate during `host.loadAll` *before* the four services
  // above are registered, so any subscriber that reads them from
  // `activate()` sees an empty registry. Fire the change event now to
  // give activate-time consumers (nexus.pluginsMgmt, nexus.processes,
  // settings) a single chance to seed their views from the now-complete
  // registry. `refreshPluginServices` emits the same event later when
  // mid-session enable/disable mutates the lists.
  eventBus.emit(PLUGIN_LIST_CHANGED_EVENT, null)

  const { useSlotStore } = await import('./registry/SlotRegistry')
  const slotSummary = Object.entries(useSlotStore.getState().slots)
    .map(([k, v]) => `${k}:${(v as any[]).length}`)
    .join(' ')
  clientLogger.info(`[Boot] Slots: ${slotSummary}`)

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
      clientLogger.warn('[Boot] deep-link payload not parseable:', event.payload, err)
    }
  }).catch((err) => {
    clientLogger.warn('[Boot] failed to register deep-link listener:', err)
  })

  // BL-029 Phase 2a — popout close-event sync (ADR 0020 §3).
  // A popout webview emits `nexus:popout-closed` on its
  // `onCloseRequested` hook just before the OS tears it down.
  // Removing the matching FloatingWindow from `floating[]` keeps the
  // main window's state authoritative and avoids the next
  // `restoreFloatingWindows()` reconciliation re-opening a popout the
  // user just dismissed.
  //
  // We *also* call `close_popout_window` defensively. Closing the
  // store entry first (synchronously, via `closeFloatingWindow`)
  // means the persisted `workspace.json` no longer references the
  // popout even if the OS-side close has already finalized; the
  // Tauri call is idempotent so a no-op there is fine.
  listen<{ fwId?: string }>(POPOUT_CLOSED_EVENT, async (event) => {
    const fwId = event.payload?.fwId
    if (typeof fwId !== 'string' || fwId.length === 0) {
      clientLogger.warn('[Boot] popout-closed event missing fwId:', event.payload)
      return
    }
    try {
      await workspaceStore.closeFloatingWindow(fwId)
    } catch (err) {
      clientLogger.warn('[Boot] popout-closed: closeFloatingWindow failed', err)
    }
    try {
      await closePopoutTauri(fwId)
    } catch {
      // Idempotent; the OS window has typically already gone away.
    }
  }).catch((err) => {
    clientLogger.warn('[Boot] failed to register popout-closed listener:', err)
  })

  // SH-021: Popout bounds persistence. Popouts emit
  // `nexus:popout-bounds-changed` on every move/resize; the main window
  // updates the matching FloatingWindow entry and the persistence
  // layer's `layout-change` subscription writes it to workspace.json.
  listen<{ fwId?: string; bounds?: { x: number; y: number; w: number; h: number } }>(
    POPOUT_BOUNDS_CHANGED_EVENT,
    (event) => {
      const { fwId, bounds } = event.payload ?? {}
      if (
        typeof fwId !== 'string' ||
        fwId.length === 0 ||
        bounds == null ||
        typeof bounds.x !== 'number'
      ) {
        return
      }
      workspaceStore.setFloatingWindowBounds(fwId, bounds)
    },
  ).catch((err) => {
    clientLogger.warn('[Boot] failed to register popout-bounds-changed listener:', err)
  })

  contextKeyService.set('shellReady', true)
}

// SH-018: global unhandled-error / unhandled-rejection handlers.
// Forward to clientLogger so errors survive page reload in the ring
// buffer and reach the Rust log when append_shell_log is available.
//
// `ResizeObserver loop completed with undelivered notifications` is a
// benign WebKit/Chromium warning emitted when a ResizeObserver callback
// schedules another layout pass that doesn't finish in the same frame.
// It carries no `event.error` (only `event.message`) and the spec
// guarantees the next frame will deliver the notifications, so the
// loop self-recovers. Filtering it here keeps the log signal-rich
// without hiding real errors — anything with a stack still surfaces.
const RESIZE_OBSERVER_NOISE =
  /^ResizeObserver loop /
window.addEventListener('error', (event) => {
  if (!event.error && RESIZE_OBSERVER_NOISE.test(event.message ?? '')) {
    // preventDefault() also stops the browser's own console.error
    // emit, otherwise xterm/Monaco resize loops will still spam
    // DevTools as `(localhost, line 0)` lines we don't control.
    event.preventDefault()
    return
  }
  clientLogger.error(
    '[Global] Uncaught error',
    event.error ?? event.message,
  )
})
window.addEventListener('unhandledrejection', (event) => {
  clientLogger.error(
    '[Global] Unhandled promise rejection',
    event.reason,
  )
})

// Install Obsidian-faithful body-class state machine. Runs once, before
// React mounts, so platform / frameless / focus classes are present on
// first paint and CSS can key off them (`body.mod-windows`, etc.). The
// Tauri listeners it registers persist for the app lifetime.
installBodyClasses()

// BL-029 — popout window mode. A child WebviewWindow opened by the
// `popout_window` Tauri command loads this same `index.html` with
// `?popout=<fwId>&leaf=<leafId>` query params. We short-circuit the
// full plugin-loading + workspace-render path and mount a focused
// popout shell instead, so the popout window doesn't double-boot the
// kernel / community plugins / sandbox orchestrator.
if (isPopoutMode()) {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <PopoutShell />
    </React.StrictMode>,
  )
  // BL-029 Phase 2b — run the slimmed-down boot so plugin view creators
  // are registered before `PopoutShell` hydrates the workspace and mounts
  // its leaf. The popout's React tree gates rendering on `shellReady`
  // (set by `boot()` after `host.loadAll`) the same way `<App>` does.
  boot({ popoutMode: true }).catch((err) => {
    const stack = err instanceof Error ? (err.stack ?? err.message) : String(err)
    clientLogger.error('[Boot/popout] Fatal:', err)
    showFatal(stack)
  })
} else {
  // Mount React IMMEDIATELY so the user sees SOMETHING even if boot fails mid-way.
  // App renders a "Loading plugins..." placeholder until slots populate.
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )

  boot().catch(err => {
    const stack = err instanceof Error ? (err.stack ?? err.message) : String(err)
    clientLogger.error('[Boot] Fatal:', err)
    showFatal(stack)
  })
}
