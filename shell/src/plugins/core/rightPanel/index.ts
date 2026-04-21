// src/plugins/core/rightPanel/index.ts
// Core inspector plugin — registers the Outline/Backlinks/Graph panel
// into the `rightPanel` slot, exposes a toggle command + keybinding,
// and flips rightPanel.visible on so it shows up by default.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { RightPanelView } from './RightPanelView'
import { useLayoutStore } from '../../../stores/layoutStore'

export const rightPanelPlugin: Plugin = {
  manifest: {
    id: 'core.right-panel',
    name: 'Right Panel',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: 'rightPanel.toggle', title: 'Toggle Right Panel', category: 'View' },
      ],
      keybindings: [
        { command: 'rightPanel.toggle', key: 'ctrl+alt+b', mac: 'cmd+alt+b' },
      ],
    },
  },
  activate(api: PluginAPI) {
    api.views.register('rightPanel', {
      slot: 'rightPanel',
      component: RightPanelView,
      priority: 0,
    })

    api.commands.register('rightPanel.toggle', () => {
      useLayoutStore.getState().toggleRightPanel()
    })
  },
}
