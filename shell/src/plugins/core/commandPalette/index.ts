// src/plugins/core/commandPalette/index.ts
// UI plugin — registers the command palette into the overlay slot.
// Reads from the command registry. Replaces the entire palette by
// swapping this plugin for a community alternative.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { CommandPaletteView } from './CommandPaletteView'

export const commandPalettePlugin: Plugin = {
  manifest: {
    id: 'core.command-palette',
    name: 'Command Palette',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        {
          id: 'workbench.action.showCommandPalette',
          title: 'Show Command Palette',
          category: 'View',
        },
      ],
      keybindings: [
        {
          command: 'workbench.action.showCommandPalette',
          key: 'ctrl+shift+p',
          mac: 'cmd+shift+p',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    // Register the palette UI into the overlay slot
    api.views.register('commandPalette', {
      slot: 'overlay',
      component: CommandPaletteView,
      priority: 100,
    })

    // Wire the open command
    api.commands.register('workbench.action.showCommandPalette', () => {
      api.context.set('commandPaletteVisible', true)
    })

    // Set initial visibility
    api.context.set('commandPaletteVisible', false)
  },
}
