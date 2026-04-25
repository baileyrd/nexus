// src/plugins/core/zoom/index.ts
//
// App-wide UI zoom. Scales the entire document via CSS `zoom` on
// <html>; persisted in configStore under `ui.zoom` so the level
// survives reloads.

import type { Plugin, PluginAPI } from '../../../types/plugin'

const CONFIG_KEY = 'ui.zoom'

const COMMAND_IN = 'core.zoom.in'
const COMMAND_OUT = 'core.zoom.out'
const COMMAND_RESET = 'core.zoom.reset'

const STEP = 0.1
const MIN = 0.5
const MAX = 3.0
const DEFAULT = 1.0

const clamp = (n: number) => Math.min(MAX, Math.max(MIN, Math.round(n * 10) / 10))

const apply = (level: number) => {
  // CSS `zoom` is non-standard but supported on every webview Tauri
  // ships against (WebView2 / WebKit / WebKitGTK). Setting it on
  // <html> scales chrome, terminal, modals, and overlays uniformly.
  document.documentElement.style.zoom = String(level)
}

export const zoomPlugin: Plugin = {
  manifest: {
    id: 'core.zoom',
    name: 'Zoom',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: COMMAND_IN, title: 'Zoom In', category: 'View' },
        { id: COMMAND_OUT, title: 'Zoom Out', category: 'View' },
        { id: COMMAND_RESET, title: 'Reset Zoom', category: 'View' },
      ],
      keybindings: [
        { command: COMMAND_IN, key: 'ctrl+=', mac: 'cmd+=' },
        // Ctrl+Shift+= is what most US-layout keyboards produce when
        // the user types Ctrl++; bind it explicitly so both chords
        // hit zoom-in.
        { command: COMMAND_IN, key: 'ctrl+shift+=', mac: 'cmd+shift+=' },
        { command: COMMAND_OUT, key: 'ctrl+-', mac: 'cmd+-' },
        { command: COMMAND_RESET, key: 'ctrl+0', mac: 'cmd+0' },
      ],
      configuration: {
        pluginId: 'core.zoom',
        title: 'Zoom',
        order: 5,
        schema: [
          {
            key: CONFIG_KEY,
            title: 'Zoom level',
            description:
              `Scale the entire UI. Range ${MIN}–${MAX} (1.0 = 100%). ` +
              `Ctrl+= / Ctrl+- / Ctrl+0 also adjust this.`,
            type: 'number',
            default: DEFAULT,
          },
        ],
      },
    },
  },

  async activate(api: PluginAPI) {
    const read = (): number => {
      const raw = api.configuration.getValue<number>(CONFIG_KEY, DEFAULT)
      const n = typeof raw === 'number' && Number.isFinite(raw) ? raw : DEFAULT
      return clamp(n)
    }

    const write = (level: number) => {
      const next = clamp(level)
      apply(next)
      api.configuration.setValue(CONFIG_KEY, next)
    }

    apply(read())

    api.commands.register(COMMAND_IN, () => write(read() + STEP))
    api.commands.register(COMMAND_OUT, () => write(read() - STEP))
    api.commands.register(COMMAND_RESET, () => write(DEFAULT))

    // The settings UI writes through `useConfigStore.set` directly,
    // bypassing our `write()` wrapper, so subscribe and re-apply.
    // Also re-clamps any out-of-range value the user types in.
    api.configuration.onChange(CONFIG_KEY, (raw) => {
      const n = typeof raw === 'number' && Number.isFinite(raw) ? raw : DEFAULT
      const next = clamp(n)
      apply(next)
      if (next !== raw) api.configuration.setValue(CONFIG_KEY, next)
    })
  },
}
