// src/plugins/core/configurationService/index.ts
// Service plugin — bootstraps the configuration registry and config store.
// After this activates, api.configuration is available to all plugins.
//
// Also owns the per-forge persistence lifecycle: on `workspace:opened`
// we IPC-load `[settings]` from the forge's `app.toml`; on
// `workspace:closed` we clear back to empty defaults so settings
// don't bleed across forges.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ConfigurationRegistry } from '../../../registry/ConfigurationRegistry'
import {
  configStore,
  hydrateFromForge,
  resetForWorkspaceClose,
} from '../../../stores/configStore'
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

  async activate(api: PluginAPI) {
    const configRegistry = new ConfigurationRegistry()

    // Register the registry so other plugins can contribute sections
    api.internal!.registerInternalService('configurationRegistry', configRegistry)

    // Register the store so api.configuration.getValue/setValue work
    api.internal!.registerInternalService('configStore', configStore)

    // Mirrors core.theme-service's hydrate gate: kernel_invoke fails
    // until `nexus.workspace` opens a forge. We hydrate iff the kernel
    // is already up (persisted-workspace cold start where
    // `workspace:opened` fired before our listener registered) and
    // also subscribe to the event for the normal warm path.
    const tryHydrate = async () => {
      if (!(await api.kernel.available())) return
      await hydrateFromForge()
    }
    await tryHydrate()

    api.events.on('workspace:opened', () => {
      void tryHydrate()
    })
    api.events.on('workspace:closed', () => {
      resetForWorkspaceClose()
    })

    clientLogger.info('[core.configuration-service] ready')
  },
}
