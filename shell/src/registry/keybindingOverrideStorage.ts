// src/registry/keybindingOverrideStorage.ts
// localStorage adapter for keybinding overrides.
//
// Previously lived in SettingsPanelView.tsx; moved here so the registry
// bootstrap (main.tsx) can bind it before any plugin loads, rather than
// waiting for the settings plugin to activate.
//
// The storage key is unchanged — existing user overrides survive the refactor.

import type { OverrideStorage } from './KeybindingRegistry'

const OVERRIDES_STORAGE_KEY = 'plugin:core.settings:keybinding-overrides'

export const keybindingOverrideStorage: OverrideStorage = {
  async read() {
    try {
      const raw = localStorage.getItem(OVERRIDES_STORAGE_KEY)
      if (!raw) return {}
      const parsed = JSON.parse(raw) as unknown
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        return parsed as Record<string, string>
      }
      return {}
    } catch {
      return {}
    }
  },
  async write(overrides) {
    localStorage.setItem(OVERRIDES_STORAGE_KEY, JSON.stringify(overrides))
  },
}
