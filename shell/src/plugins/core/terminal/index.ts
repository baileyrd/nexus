// src/plugins/core/terminal/index.ts
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLayoutStore } from '../../../stores/layoutStore'
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
    // Phase 7: legacy slot:'panelAreaContent' registration removed.
    api.commands.register('terminal.toggle', () => {
      const store = useLayoutStore.getState()
      if (!store.panelArea.visible) {
        store.togglePanelArea()
        store.setActivePanel('terminal')
      } else if (store.panelArea.activePanel === 'terminal') {
        store.togglePanelArea()
      } else {
        store.setActivePanel('terminal')
      }
    })

    api.commands.register('terminal.clear', () => {
      useTerminalStore.getState().clear()
    })

    api.configuration.register(terminalPlugin.manifest.contributes!.configuration!)
  },
}
