// src/plugins/core/zoom/index.ts
//
// App-wide UI zoom. Scales the entire document via CSS `zoom` on
// <html>; persisted in configStore under `ui.zoom` so the level
// survives reloads.

import type { Plugin, PluginAPI } from '../../../types/plugin'

const CONFIG_KEY = 'ui.zoom'
const CONFIG_KEY_STEP = 'ui.zoomStep'
const CONFIG_KEY_MIN = 'ui.zoomMin'
const CONFIG_KEY_MAX = 'ui.zoomMax'
const CONFIG_KEY_DEFAULT = 'ui.zoomDefault'

const COMMAND_IN = 'core.zoom.in'
const COMMAND_OUT = 'core.zoom.out'
const COMMAND_RESET = 'core.zoom.reset'

const DEFAULT_STEP = 0.1
const DEFAULT_MIN = 0.5
const DEFAULT_MAX = 3.0
const DEFAULT_DEFAULT = 1.0

const clamp = (n: number, min: number, max: number) =>
  Math.min(max, Math.max(min, Math.round(n * 10) / 10))

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
    // Consumes api.configuration to persist/restore the zoom level —
    // configuration-service must be loaded first.
    dependsOn: ['core.configuration-service'],
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
        category: 'appearance',
        schema: [
          {
            key: CONFIG_KEY,
            title: 'Zoom level',
            description:
              'Scale the entire UI (1.0 = 100%). Ctrl+= / Ctrl+- / Ctrl+0 also adjust this.',
            type: 'number',
            default: DEFAULT_DEFAULT,
          },
          {
            key: CONFIG_KEY_STEP,
            title: 'Zoom step',
            description: 'Increment applied by Ctrl+= and Ctrl+-.',
            type: 'number',
            default: DEFAULT_STEP,
          },
          {
            key: CONFIG_KEY_MIN,
            title: 'Zoom minimum',
            description: 'Lower bound for the zoom level.',
            type: 'number',
            default: DEFAULT_MIN,
          },
          {
            key: CONFIG_KEY_MAX,
            title: 'Zoom maximum',
            description: 'Upper bound for the zoom level.',
            type: 'number',
            default: DEFAULT_MAX,
          },
          {
            key: CONFIG_KEY_DEFAULT,
            title: 'Reset target',
            description: 'Level Ctrl+0 resets to.',
            type: 'number',
            default: DEFAULT_DEFAULT,
          },
        ],
      },
    },
  },

  async activate(api: PluginAPI) {
    const num = (key: string, fallback: number): number => {
      const raw = api.configuration.getValue<number>(key, fallback)
      return typeof raw === 'number' && Number.isFinite(raw) ? raw : fallback
    }

    const bounds = () => ({
      min: num(CONFIG_KEY_MIN, DEFAULT_MIN),
      max: num(CONFIG_KEY_MAX, DEFAULT_MAX),
      step: num(CONFIG_KEY_STEP, DEFAULT_STEP),
      reset: num(CONFIG_KEY_DEFAULT, DEFAULT_DEFAULT),
    })

    const read = (): number => {
      const { min, max, reset } = bounds()
      return clamp(num(CONFIG_KEY, reset), min, max)
    }

    const write = (level: number) => {
      const { min, max } = bounds()
      const next = clamp(level, min, max)
      apply(next)
      api.configuration.setValue(CONFIG_KEY, next)
    }

    apply(read())

    api.commands.register(COMMAND_IN, () => write(read() + bounds().step))
    api.commands.register(COMMAND_OUT, () => write(read() - bounds().step))
    api.commands.register(COMMAND_RESET, () => write(bounds().reset))

    // The settings UI writes through `useConfigStore.set` directly,
    // bypassing our `write()` wrapper, so subscribe and re-apply.
    // Also re-clamps any out-of-range value the user types in.
    api.configuration.onChange(CONFIG_KEY, (raw) => {
      const { min, max, reset } = bounds()
      const n = typeof raw === 'number' && Number.isFinite(raw) ? raw : reset
      const next = clamp(n, min, max)
      apply(next)
      if (next !== raw) api.configuration.setValue(CONFIG_KEY, next)
    })
  },
}
