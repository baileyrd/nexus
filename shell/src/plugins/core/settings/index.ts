// src/plugins/core/settings/index.ts
// UI plugin — renders the settings panel into the overlay slot.
// Auto-generates UI from schemas registered by other plugins via
// core.configuration-service.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { SettingsPanelView } from './SettingsPanelView'

export const settingsPlugin: Plugin = {
  manifest: {
    id: 'core.settings',
    name: 'Settings',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    dependsOn: ['core.configuration-service', 'nexus.activityBar'],
    contributes: {
      commands: [
        {
          id: 'workbench.action.openSettings',
          title: 'Open Settings',
          category: 'Preferences',
        },
        {
          id: 'workbench.action.openKeybindings',
          title: 'Open Keyboard Shortcuts',
          category: 'Preferences',
        },
      ],
      keybindings: [
        {
          command: 'workbench.action.openSettings',
          key: 'ctrl+,',
          mac: 'cmd+,',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.views.register('settingsPanel', {
      slot: 'overlay',
      component: SettingsPanelView,
      priority: 90,
    })

    api.commands.register('workbench.action.openSettings', () => {
      api.context.set('settingsPanelVisible', true)
    })

    api.commands.register('workbench.action.openKeybindings', () => {
      api.context.set('settingsPanelVisible', true)
      api.context.set('settingsActiveTab', 'keybindings')
    })

    api.context.set('settingsPanelVisible', false)

    api.activityBar.addItem({
      id: 'core.settings.activityBarItem',
      icon: '',
      iconName: 'settings',
      title: 'Settings',
      viewId: 'core.settings.view',
      priority: 100,
      placement: 'bottom',
      command: 'workbench.action.openSettings',
    })
  },
}
