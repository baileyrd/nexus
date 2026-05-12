// src/stores/configStore.ts
// Per-forge key/value store for all plugin configuration values.
//
// Source of truth lives in `<forge>/.forge/app.toml` under `[settings]`.
// On `workspace:opened` we IPC-load that table; subsequent `set()` calls
// IPC-write back (debounced). On `workspace:closed` we clear to defaults.
//
// Why not zustand's `persist` middleware: persist is designed for a
// single, browser-global localStorage realm. Settings are per-forge,
// switching forges should swap them, and the CLI / TUI need to read
// the same file. A custom hydrate/flush loop is the honest fit.

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { eventBus } from '../host/EventBus'
import { clientLogger } from '../clientLogger'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const CMD_READ = 'settings_read'
const CMD_WRITE = 'settings_write'
/** Settling window before a `set` is flushed to TOML. Several
 *  rapid sets (e.g. a slider drag) collapse into one IPC write. */
const FLUSH_DEBOUNCE_MS = 200

interface ConfigStore {
  values: Record<string, unknown>
  /** True after the first successful `hydrateFromForge`. Consumers
   *  that need to wait for the on-disk values (e.g. editor CSS
   *  vars) can subscribe to this. */
  hydrated: boolean
  get: <T>(key: string, defaultValue: T) => T
  set: (key: string, value: unknown) => void
  reset: (key: string) => void
  resetAll: () => void
}

const pendingWrites = new Map<string, unknown>()
let flushTimer: ReturnType<typeof setTimeout> | null = null
let hydrating = false

async function flushPending(): Promise<void> {
  if (pendingWrites.size === 0) return
  const batch = Array.from(pendingWrites.entries())
  pendingWrites.clear()
  for (const [key, value] of batch) {
    try {
      await invoke('kernel_invoke', {
        pluginId: STORAGE_PLUGIN_ID,
        commandId: CMD_WRITE,
        args: { key, value },
        timeoutMs: null,
      })
    } catch (err) {
      // Best-effort persistence: in-memory state is still correct,
      // and we'll retry on the next set. Log so a forge that's
      // gone read-only (permissions, full disk) is visible.
      clientLogger.warn(`[configStore] settings_write '${key}' failed`, err)
    }
  }
}

function scheduleFlush(key: string, value: unknown): void {
  if (hydrating) return
  pendingWrites.set(key, value)
  if (flushTimer) clearTimeout(flushTimer)
  flushTimer = setTimeout(() => {
    flushTimer = null
    void flushPending()
  }, FLUSH_DEBOUNCE_MS)
}

export const useConfigStore = create<ConfigStore>()((set, get) => ({
  values: {},
  hydrated: false,

  get: <T>(key: string, defaultValue: T): T => {
    const val = get().values[key]
    return val !== undefined ? (val as T) : defaultValue
  },

  set: (key: string, value: unknown) => {
    set(s => ({ values: { ...s.values, [key]: value } }))
    eventBus.emit(`config:changed:${key}`, value)
    scheduleFlush(key, value)
  },

  reset: (key: string) => {
    set(s => {
      const { [key]: _, ...rest } = s.values
      return { values: rest }
    })
    eventBus.emit(`config:changed:${key}`, undefined)
    scheduleFlush(key, null)
  },

  resetAll: () => {
    const keys = Object.keys(get().values)
    set({ values: {} })
    for (const k of keys) {
      eventBus.emit(`config:changed:${k}`, undefined)
      scheduleFlush(k, null)
    }
  },
}))

/** React hook — re-renders when the specific config key changes */
export function useConfigValue<T>(key: string, defaultValue: T): T {
  return useConfigStore(s => {
    const val = s.values[key]
    return val !== undefined ? (val as T) : defaultValue
  })
}

/** Non-reactive access for use outside React */
export const configStore = {
  get: <T>(key: string, defaultValue: T): T =>
    useConfigStore.getState().get(key, defaultValue),
  set: (key: string, value: unknown) =>
    useConfigStore.getState().set(key, value),
}

/**
 * Pull `[settings]` from the active forge's `app.toml` and overwrite
 * the in-memory store. Per-key `config:changed:*` events fire for
 * every key that changed (including keys removed in the new snapshot),
 * so existing `onChange` subscribers re-apply naturally. Marks the
 * store `hydrated` on success; failure leaves it `hydrated = false`
 * (callers see defaults). Safe to call multiple times — switching
 * forges hits this path.
 */
export async function hydrateFromForge(): Promise<void> {
  hydrating = true
  try {
    const next = await invoke<Record<string, unknown> | null>('kernel_invoke', {
      pluginId: STORAGE_PLUGIN_ID,
      commandId: CMD_READ,
      args: {},
      timeoutMs: null,
    })
    const nextValues = (next ?? {}) as Record<string, unknown>
    const prev = useConfigStore.getState().values
    useConfigStore.setState({ values: nextValues, hydrated: true })
    // Fire per-key events so onChange handlers don't need a separate
    // `config:hydrated` codepath. Compare by reference; for value
    // equality we'd need a deep-compare we don't have, but a rare
    // spurious event is cheaper than reaching for one.
    const allKeys = new Set([...Object.keys(prev), ...Object.keys(nextValues)])
    for (const k of allKeys) {
      if (prev[k] !== nextValues[k]) eventBus.emit(`config:changed:${k}`, nextValues[k])
    }
  } catch (err) {
    clientLogger.warn('[configStore] settings_read failed; using defaults', err)
  } finally {
    hydrating = false
  }
}

/**
 * Drop in-memory state back to empty defaults. Called on
 * `workspace:closed` so settings from forge A don't bleed into
 * forge B if it hasn't hydrated yet. Skips the IPC write path
 * (closing the forge does not delete its on-disk settings).
 */
export function resetForWorkspaceClose(): void {
  hydrating = true
  try {
    const keys = Object.keys(useConfigStore.getState().values)
    useConfigStore.setState({ values: {}, hydrated: false })
    for (const k of keys) eventBus.emit(`config:changed:${k}`, undefined)
  } finally {
    hydrating = false
  }
}
