import type { Plugin, PluginAPI } from '../../../types/plugin'
import { SidebarView } from './SidebarView'
import { useLayoutStore } from '../../../stores/layoutStore'

export const sidebarPlugin: Plugin = {
  manifest: {
    id: 'core.sidebar',
    name: 'Sidebar',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: 'sidebar.toggle', title: 'Toggle Sidebar', category: 'View' },
      ],
      keybindings: [
        { command: 'sidebar.toggle', key: 'ctrl+b', mac: 'cmd+b' },
      ],
    },
  },
  activate(api: PluginAPI) {
    api.views.register('sidebar', {
      slot: 'sidebar',
      component: SidebarView,
      priority: 0,
    })
    api.commands.register('sidebar.toggle', () => {
      useLayoutStore.getState().toggleSidebar()
    })
  },
}
