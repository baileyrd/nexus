// src/plugins/core/terminal/index.ts
// Legacy template plugin — retained on disk but NOT loaded from main.tsx.
// The active terminal plugin is `nexus.terminal`, which hosts a Leaf in
// the workspace right sidedock. This file's toggle command no-ops
// because the panel-area concept was retired by Phase 7 and its
// bottom-dock replacement is still pending (follow-up task #11).
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useTerminalStore } from './terminalStore'

export const terminalPlugin: Plugin = {
  manifest: {
    id: 'core.terminal',
    name: 'Terminal',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    dependsOn: ['core.panel-area', 'core.configuration-service'],
    contributes: {
      commands: [
        { id: 'terminal.toggle', title: 'Toggle Terminal', category: 'View' },
        { id: 'terminal.new',    title: 'New Terminal',    category: 'View' },
        { id: 'terminal.clear',  title: 'Clear Terminal'                    },
      ],
      keybindings: [
        { command: 'terminal.toggle', key: 'ctrl+`', mac: 'ctrl+`' },
      ],
      configuration: {
        pluginId: 'core.terminal',
        title: 'Terminal',
        order: 30,
        category: 'system',
        schema: [
          {
            key: 'terminal.fontSize',
            title: 'Font size',
            type: 'number',
            default: 13,
            description: 'Terminal font size in pixels',
          },
          {
            key: 'terminal.fontFamily',
            title: 'Font family',
            type: 'string',
            default: "'Cascadia Code', 'Consolas', monospace",
            description: 'Terminal font family',
          },
        ],
      },
    },
  },

  activate(api: PluginAPI) {
    api.commands.register('terminal.toggle', () => {
      // No-op: use `nexus.terminal.toggle` instead.
    })

    api.commands.register('terminal.clear', () => {
      useTerminalStore.getState().clear()
    })

    api.configuration.register(terminalPlugin.manifest.contributes!.configuration!)
  },
}
