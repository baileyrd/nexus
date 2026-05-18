// src/registry/keybindingOverrideStorage.ts
// Storage adapter for keybinding overrides.
//
// P2-01 (2026-05-17): override storage now cascades from the per-forge
// configStore (backed by `<forge>/.forge/app.toml [settings]`) with
// localStorage as a fast/offline cache + boot fallback.
//
// Cascade semantics:
//   read():
//     1. If configStore has hydrated and holds an entry under
//        `nexus.keybindings.overrides`, that's the source of truth
//        (forge-portable, version-controllable).
//     2. Otherwise fall back to the localStorage cache — used at
//        early boot before the configurationService plugin activates
//        and on machines that haven't opened a forge yet.
//   write(overrides):
//     1. Always mirror to localStorage so the next boot's pre-hydration
//        reads still pick up the right chord.
//     2. When configStore is hydrated, push into the forge config too —
//        the debounced settings_write in configStore handles persistence.
//
// Migration: the first time a hydrated configStore has *no* entry for
// the setting key but localStorage *does*, the next call to write()
// will populate the forge config from the local cache.

import { useConfigStore } from '../stores/configStore'
import type { OverrideStorage } from './KeybindingRegistry'

const SETTING_KEY = 'nexus.keybindings.overrides'
const LOCAL_STORAGE_KEY = 'plugin:core.settings:keybinding-overrides'

function readLocal(): Record<string, string> {
  try {
    const raw = localStorage.getItem(LOCAL_STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as unknown
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Record<string, string>
    }
    return {}
  } catch {
    return {}
  }
}

function writeLocal(overrides: Record<string, string>): void {
  try {
    localStorage.setItem(LOCAL_STORAGE_KEY, JSON.stringify(overrides))
  } catch {
    // Quota / private-mode / SSR — leave the in-memory state authoritative.
  }
}

export const keybindingOverrideStorage: OverrideStorage = {
  async read() {
    const store = useConfigStore.getState()
    if (store.hydrated) {
      const fromForge = store.get<Record<string, string>>(SETTING_KEY, {})
      if (Object.keys(fromForge).length > 0) return fromForge
    }
    return readLocal()
  },
  async write(overrides) {
    writeLocal(overrides)
    const store = useConfigStore.getState()
    if (store.hydrated) {
      store.set(SETTING_KEY, overrides)
    }
  },
}

/**
 * P2-01 — settings key the cascade reads from.
 * Exposed for the configurationService activate path so it can fire
 * a re-load of the registry overrides after the per-forge settings
 * have hydrated (otherwise forge-only overrides wouldn't take effect
 * until the user next pressed a key, since the registry's
 * `loadOverrides` runs at boot before hydration).
 */
export const KEYBINDING_OVERRIDES_SETTING_KEY = SETTING_KEY
