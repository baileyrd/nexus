// src/plugins/core/settings/index.ts
// UI plugin — renders the settings panel into the overlay slot.
// Auto-generates UI from schemas registered by other plugins via
// core.configuration-service.

import { createElement, type ComponentType } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { SettingsPanelView, keybindingOverrideStorage } from './SettingsPanelView'

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
        {
          id: 'workbench.action.openHelp',
          title: 'Open Help',
          category: 'Help',
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
    // Wrap so the Appearance tab (WI-02 part 3) can call kernel-routed
    // theme actions through `api`. The slot system itself doesn't pass
    // props, so we close over `api` here. We cast the wrapped component
    // to `ComponentType<{ api?: PluginAPI }>` so TS lets `createElement`
    // accept the closure prop — `SettingsPanelView`'s default-param
    // signature otherwise hides the prop from JSX/createElement
    // inference. The wrapper gets a stable `displayName` to make
    // React DevTools / e2e selectors readable.
    const Wrapped = SettingsPanelView as ComponentType<{ api?: PluginAPI }>
    const SettingsPanelHost = () => createElement(Wrapped, { api })
    SettingsPanelHost.displayName = 'SettingsPanelHost'
    api.views.register('settingsPanel', {
      slot: 'overlay',
      component: SettingsPanelHost,
      priority: 90,
    })

    api.commands.register('workbench.action.openSettings', () => {
      api.context.set('settingsPanelVisible', true)
    })

    api.commands.register('workbench.action.openKeybindings', () => {
      api.context.set('settingsPanelVisible', true)
      api.context.set('settingsActiveTab', 'keybindings')
    })

    api.commands.register('workbench.action.openHelp', () => {
      // Tauri webviews honour window.open with an external target —
      // the host OS opens the URL in the default browser.
      window.open('https://github.com/baileyrd/nexus', '_blank')
    })

    api.context.set('settingsPanelVisible', false)

    // WI-04 — hydrate keybinding overrides from localStorage so any
    // user-set chord wins over the manifest default at first dispatch.
    // Failures here are non-fatal: the registry just falls back to
    // defaults, which is the correct degraded behaviour. We reach into
    // `api.internal.registry` (core-plugin only) rather than importing
    // shell/host directly — that keeps the WI-23 hygiene allowlist for
    // the settings folder unchanged (only SettingsPanelView is on it).
    const reg = (api.internal?.registry as
      | { keybindings: { loadOverrides: (s: typeof keybindingOverrideStorage) => Promise<void> } }
      | undefined)
    if (reg) {
      void reg.keybindings.loadOverrides(keybindingOverrideStorage)
    }

    // Priority orders bottom items from top to bottom (lower = higher).
    // Help sits above Settings to match Obsidian's chrome order.
    api.activityBar.addItem({
      id: 'core.help.activityBarItem',
      icon: '',
      iconName: 'help',
      title: 'Help',
      viewId: 'core.help.view',
      priority: 99,
      placement: 'bottom',
      command: 'workbench.action.openHelp',
    })

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
