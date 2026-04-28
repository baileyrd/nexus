import type { Plugin, PluginAPI } from '../../../types/plugin'
import { CommandPalette } from './CommandPalette'
import { useCommandPaletteStore } from './commandPaletteStore'
import { CONFIG_KEY_PALETTE_LIMIT, DEFAULT_MAX_PALETTE_RESULTS } from './match'
import { setApi } from './paletteRuntime'

const VIEW_ID = 'nexus.commandPalette.overlay'

const COMMAND_OPEN = 'nexus.commandPalette.open'
const COMMAND_CLOSE = 'nexus.commandPalette.close'
const CONTEXT_KEY_VISIBLE = 'nexus.commandPalette.visible'

export const commandPalettePlugin: Plugin = {
  manifest: {
    id: 'nexus.commandPalette',
    name: 'Command Palette',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        {
          id: COMMAND_OPEN,
          title: 'Open Command Palette',
          category: 'Command Palette',
        },
        {
          id: COMMAND_CLOSE,
          title: 'Close Command Palette',
          category: 'Command Palette',
        },
      ],
      keybindings: [
        // Primary binding. Mirrors VS Code / virtually every IDE.
        { command: COMMAND_OPEN, key: 'ctrl+shift+p', mac: 'cmd+shift+p' },
        // Alias — to be reclaimed when a file quick-open plugin lands
        // (the spec carves out Ctrl+P for that). Until then it opens
        // the same command-only palette.
        { command: COMMAND_OPEN, key: 'ctrl+p', mac: 'cmd+p' },
        // Note: this `escape` binding exists for completeness, but
        // the App.tsx global handler short-circuits on INPUT focus —
        // so when the user actually has focus in the palette input,
        // this binding never fires. The component handles Escape
        // directly in its onKeyDown for that reason.
        {
          command: COMMAND_CLOSE,
          key: 'escape',
          when: CONTEXT_KEY_VISIBLE,
        },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_VISIBLE,
          description: 'True while the command palette is open.',
          type: 'boolean',
        },
      ],
      configuration: {
        pluginId: 'nexus.commandPalette',
        title: 'Command Palette',
        order: 30,
        schema: [
          {
            key: CONFIG_KEY_PALETTE_LIMIT,
            title: 'Maximum results',
            description:
              'Cap on the number of commands shown in the palette. Long lists are noise.',
            type: 'number',
            default: DEFAULT_MAX_PALETTE_RESULTS,
          },
        ],
      },
    },
  },

  async activate(api: PluginAPI) {
    setApi(api)

    api.commands.register(COMMAND_OPEN, () => {
      useCommandPaletteStore.getState().open()
    })

    api.commands.register(COMMAND_CLOSE, () => {
      useCommandPaletteStore.getState().close()
    })

    // Mirror the store's `visible` flag into the context-key service
    // so other plugins can `when`-clause against it (e.g. our own
    // Escape binding above).
    api.context.set(CONTEXT_KEY_VISIBLE, useCommandPaletteStore.getState().visible)
    useCommandPaletteStore.subscribe((state, prev) => {
      if (state.visible !== prev.visible) {
        api.context.set(CONTEXT_KEY_VISIBLE, state.visible)
      }
    })

    api.views.register(VIEW_ID, {
      slot: 'overlay',
      component: CommandPalette,
      priority: 10,
    })

    api.configuration.register(commandPalettePlugin.manifest.contributes!.configuration!)
  },
}
