// shell/src/plugins/nexus/extensionsTab/index.ts
//
// OI-08 — registers the "Running Extensions" Settings tab.
//
// Manifest declares the tab metadata so the rail entry shows up
// before activation; `activate()` calls
// `api.settings.registerTab('extensions', ExtensionsTab, ...)` to
// wire the renderer. The store that backs the tab
// (`stores/pluginsStatusStore.ts`) self-subscribes to the EventBus
// at module load — it does *not* depend on this plugin activating
// first, so error events from earlier-loaded plugins are still
// captured.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ExtensionsTab } from './ExtensionsTab'
// Importing the store at module load also primes its EventBus
// subscriptions, so the store is live by the time `host.loadAll`
// gets to its first plugin.
import '../../../stores/pluginsStatusStore'

const TAB_ID = 'extensions'

export const extensionsTabPlugin: Plugin = {
  manifest: {
    id: 'nexus.extensionsTab',
    name: 'Extensions Tab',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    contributes: {
      settingsTabs: [
        {
          id: TAB_ID,
          title: 'Extensions',
          group: 'options',
          priority: 40,
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.settings.registerTab(TAB_ID, ExtensionsTab, {
      title: 'Extensions',
      group: 'options',
      priority: 40,
    })
  },
}
