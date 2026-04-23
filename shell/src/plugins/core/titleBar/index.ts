// src/plugins/core/titleBar/index.ts
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { TitleBarView } from './TitleBarView'

export const titleBarPlugin: Plugin = {
  manifest: {
    id: 'core.title-bar',
    name: 'Title Bar',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: 'window.minimize', title: 'Minimize Window' },
        { id: 'window.maximize', title: 'Maximize Window' },
        { id: 'window.close',    title: 'Close Window'    },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.views.register('titleBar', {
      slot: 'titleBar',
      component: TitleBarView,
      priority: 0,
    })

    api.commands.register('window.minimize', () => {
      api.platform.window.minimize()
    })
    api.commands.register('window.maximize', async () => {
      await api.platform.window.toggleMaximize()
    })
    api.commands.register('window.close', () => {
      api.platform.window.close()
    })
  },
}
