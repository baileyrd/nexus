// src/plugins/core/rightPanel/index.ts
// Core inspector plugin — registers the Outline/Backlinks/Graph panel
// into the `rightPanel` slot, exposes a toggle command + keybinding,
// and flips rightPanel.visible on so it shows up by default.

// Legacy template plugin — retained on disk but NOT loaded from main.tsx.
// The active right-panel plugin is `nexus.rightPanel` which drives the
// workspace right sidedock directly.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'

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
    api.commands.register('rightPanel.toggle', () => {
      workspace.setSidedockCollapsed('right', !workspace.rightSplit.collapsed)
    })
  },
}
