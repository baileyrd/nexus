import type { Plugin, PluginAPI } from '../../../types/plugin'
import { PanelAreaView } from './PanelAreaView'
import { usePanelAreaStore } from './panelAreaStore'
import { useLayoutStore } from '../../../stores/layoutStore'

export { usePanelAreaStore } from './panelAreaStore'
export type { PanelTab } from './panelAreaStore'

export const panelAreaPlugin: Plugin = {
  manifest: {
    id: 'core.panel-area',
    name: 'Panel Area',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: 'panel.toggle', title: 'Toggle Panel', category: 'View' },
      ],
      keybindings: [
        { command: 'panel.toggle', key: 'ctrl+j', mac: 'cmd+j' },
      ],
    },
  },
  activate(api: PluginAPI) {
    api.views.register('panelArea', {
      slot: 'panelArea',
      component: PanelAreaView,
      priority: 0,
    })
    api.commands.register('panel.toggle', () => {
      useLayoutStore.getState().togglePanelArea()
    })
  },
}
