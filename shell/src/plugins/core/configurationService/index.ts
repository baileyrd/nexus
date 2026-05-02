// src/plugins/core/configurationService/index.ts
// Service plugin — bootstraps the configuration registry and config store.
// After this activates, api.configuration is available to all plugins.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ConfigurationRegistry } from '../../../registry/ConfigurationRegistry'
import { configStore } from '../../../stores/configStore'
import { clientLogger } from '../../../clientLogger'

export const configurationServicePlugin: Plugin = {
  manifest: {
    id: 'core.configuration-service',
    name: 'Configuration Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    const configRegistry = new ConfigurationRegistry()

    // Register the registry so other plugins can contribute sections
    api.internal!.registerInternalService('configurationRegistry', configRegistry)

    // Register the store so api.configuration.getValue/setValue work
    api.internal!.registerInternalService('configStore', configStore)

    clientLogger.info('[core.configuration-service] ready')
  },
}
