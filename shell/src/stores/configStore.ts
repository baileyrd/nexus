// src/stores/configStore.ts
// Persistent key-value store for all plugin configuration values.
// Written by the settings panel UI; read by plugin components.

import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { eventBus } from '../host/EventBus'

interface ConfigStore {
  values: Record<string, unknown>
  get: <T>(key: string, defaultValue: T) => T
  set: (key: string, value: unknown) => void
  reset: (key: string) => void
  resetAll: () => void
}

export const useConfigStore = create<ConfigStore>()(
  persist(
    (set, get) => ({
      values: {},

      get: <T>(key: string, defaultValue: T): T => {
        const val = get().values[key]
        return val !== undefined ? (val as T) : defaultValue
      },

      set: (key: string, value: unknown) => {
        set(s => ({ values: { ...s.values, [key]: value } }))
        // Notify subscribers (e.g. api.configuration.onChange())
        eventBus.emit(`config:changed:${key}`, value)
      },

      reset: (key: string) => {
        set(s => {
          const { [key]: _, ...rest } = s.values
          return { values: rest }
        })
      },

      resetAll: () => set({ values: {} }),
    }),
    { name: 'shell-config' }
  )
)

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
