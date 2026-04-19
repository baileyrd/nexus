// src/plugins/core/titleBar/index.ts
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { TitleBarView } from './TitleBarView'
import { getCurrentWindow } from '@tauri-apps/api/window'

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
      getCurrentWindow().minimize()
    })
    api.commands.register('window.maximize', async () => {
      const win = getCurrentWindow()
      if (await win.isMaximized()) win.unmaximize()
      else win.maximize()
    })
    api.commands.register('window.close', () => {
      getCurrentWindow().close()
    })
  },
}
