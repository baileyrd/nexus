// src/plugins/core/themeService/index.ts
// Service plugin — manages CSS token themes.

import type { Plugin, PluginAPI } from '../../../types/plugin'

export interface Theme {
  id: string
  name: string
  type: 'dark' | 'light'
  tokens: Record<string, string>
}

// Forge-aligned palettes. Token *values* live in shell.css under
// :root and [data-theme="light"]; themes here just flip data-theme
// and optionally override individual tokens. Tokens map is empty by
// default — a theme can still ship overrides (e.g., custom accent).

const DEFAULT_DARK: Theme = {
  id: 'default-dark',
  name: 'Forge Ember (Dark)',
  type: 'dark',
  tokens: {},
}

const DEFAULT_LIGHT: Theme = {
  id: 'default-light',
  name: 'Forge Paper (Light)',
  type: 'light',
  tokens: {},
}

export class ThemeService {
  private themes = new Map<string, Theme>([
    [DEFAULT_DARK.id,  DEFAULT_DARK],
    [DEFAULT_LIGHT.id, DEFAULT_LIGHT],
  ])
  private activeId = 'default-dark'

  register(theme: Theme) {
    this.themes.set(theme.id, theme)
  }

  activate(id: string) {
    const theme = this.themes.get(id)
    if (!theme) { console.warn(`[ThemeService] Unknown theme: ${id}`); return }
    this.activeId = id
    this.apply(theme)
  }

  current(): Theme {
    return this.themes.get(this.activeId) ?? DEFAULT_DARK
  }

  all(): Theme[] {
    return [...this.themes.values()]
  }

  private apply(theme: Theme) {
    const root = document.documentElement
    // Flip data-theme so the Forge token blocks in shell.css take effect.
    root.dataset.theme = theme.type
    // Clear any previously applied inline overrides before reapplying.
    for (const key of this.lastOverrides) root.style.removeProperty(key)
    this.lastOverrides = []
    for (const [key, value] of Object.entries(theme.tokens)) {
      root.style.setProperty(key, value)
      this.lastOverrides.push(key)
    }
  }

  private lastOverrides: string[] = []
}

export const themeServicePlugin: Plugin = {
  manifest: {
    id: 'core.theme-service',
    name: 'Theme Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    const svc = new ThemeService()
    api.internal!.registerInternalService('themeService', svc)

    // Apply default dark theme immediately
    svc.activate('default-dark')

    // Respect OS preference
    if (window.matchMedia('(prefers-color-scheme: light)').matches) {
      svc.activate('default-light')
    }

    console.info('[core.theme-service] ready')
  },
}
