// shell/src/stores/pluginsStatusStore.ts
//
// OI-09 — aggregates plugin lifecycle events from the EventBus into a
// per-plugin `{ state, lastError }` map so the Extensions Settings tab
// (OI-08) and any other observability surface can render a live view
// of failed / active / inactive plugins without poking ExtensionHost
// directly.
//
// The store subscribes to `plugin:activated` / `plugin:deactivated` /
// `plugin:error` at module-load time. Because `catalog.ts` imports
// every plugin module (including the one that will pull this store
// in), the subscription is in place before `host.loadAll(plugins)`
// runs in `main.tsx` — meaning the store catches every lifecycle
// event from the very first plugin onwards. No retro-fill from
// `host.listAll()` is needed.

import { create } from 'zustand'
import { eventBus } from '../host/EventBus'
import type { PluginState } from '../host/ExtensionHost'

export interface PluginStatus {
  /**
   * Lifecycle state mirrored from `ExtensionHost.states`. `'error'`
   * means `activate()` threw; `lastError` is populated.
   */
  state: PluginState
  /**
   * Most recent error captured from a `plugin:error` event. Cleared
   * when the plugin transitions back to `'active'` (e.g. after a
   * hot-reload-driven re-activation).
   */
  lastError?: { message: string; stack?: string }
}

interface PluginsStatusState {
  /** Per-plugin status keyed by plugin id. */
  byId: Record<string, PluginStatus>
}

interface PluginsStatusActions {
  /** Replace the entire snapshot — used by tests for hermetic resets. */
  _reset(): void
}

type Store = PluginsStatusState & PluginsStatusActions

function fromError(err: Error): PluginStatus['lastError'] {
  return { message: err.message, stack: err.stack }
}

export const usePluginsStatusStore = create<Store>((set) => ({
  byId: {},
  _reset: () => set({ byId: {} }),
}))

// Live event subscriptions. These run once at module load — no cleanup
// is wired because the store is shell-singleton and outlives every
// plugin. The handlers are pure: read prior state, write a single
// patched object.
eventBus.on<{ pluginId: string }>('plugin:activated', ({ pluginId }) => {
  usePluginsStatusStore.setState((s) => ({
    byId: { ...s.byId, [pluginId]: { state: 'active' } },
  }))
})

eventBus.on<{ pluginId: string }>('plugin:deactivated', ({ pluginId }) => {
  usePluginsStatusStore.setState((s) => ({
    byId: { ...s.byId, [pluginId]: { state: 'inactive' } },
  }))
})

eventBus.on<{ pluginId: string; error: Error }>('plugin:error', ({ pluginId, error }) => {
  usePluginsStatusStore.setState((s) => ({
    byId: {
      ...s.byId,
      [pluginId]: { state: 'error', lastError: fromError(error) },
    },
  }))
})

/**
 * Synchronous accessor — useful for non-React callers (tests, CLI-style
 * code paths) that don't want a hook subscription.
 */
export function getPluginStatus(pluginId: string): PluginStatus | undefined {
  return usePluginsStatusStore.getState().byId[pluginId]
}
