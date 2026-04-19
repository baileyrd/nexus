// src/plugins/core/fileExplorer/index.ts
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { FileExplorerView } from './FileExplorerView'
import { open as openDialog } from '@tauri-apps/plugin-dialog'

export const fileExplorerPlugin: Plugin = {
  manifest: {
    id: 'core.file-explorer',
    name: 'File Explorer',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    dependsOn: ['core.filesystem-service', 'core.activity-bar'],
    contributes: {
      commands: [
        { id: 'fileExplorer.openFolder', title: 'Open Folder',  category: 'File' },
        { id: 'fileExplorer.newFile',    title: 'New File',     category: 'File' },
        { id: 'fileExplorer.newFolder',  title: 'New Folder',   category: 'File' },
        { id: 'fileExplorer.refresh',    title: 'Refresh Explorer' },
      ],
      keybindings: [
        { command: 'fileExplorer.openFolder', key: 'ctrl+k ctrl+o', mac: 'cmd+k cmd+o' },
      ],
      configuration: {
        pluginId: 'core.file-explorer',
        title: 'File Explorer',
        order: 10,
        schema: [
          {
            key: 'fileExplorer.showHidden',
            title: 'Show hidden files',
            type: 'boolean',
            default: false,
            description: 'Show files and folders beginning with a dot',
          },
          {
            key: 'fileExplorer.sortOrder',
            title: 'Sort order',
            type: 'select',
            options: ['name', 'modified', 'type'],
            default: 'name',
            description: 'How to sort files in the tree',
          },
        ],
      },
    },
  },

  activate(api: PluginAPI) {
    api.views.register('fileExplorer', {
      slot: 'sidebarContent',
      component: FileExplorerView,
      priority: 10,
    })

    api.activityBar.addItem({
      id: 'fileExplorer',
      icon: 'files',
      title: 'Explorer',
      viewId: 'fileExplorer',
      priority: 10,
    })

    api.commands.register('fileExplorer.openFolder', async () => {
      const selected = await openDialog({ directory: true, multiple: false })
      if (selected) api.events.emit('fileExplorer:folderOpened', { path: selected })
    })

    api.commands.register('fileExplorer.newFile', async () => {
      const name = await api.input.prompt('File name:')
      if (!name) return
      api.notifications.show({ message: `Created: ${name}`, type: 'success', duration: 2000 })
    })

    api.commands.register('fileExplorer.refresh', () => {
      api.events.emit('fileExplorer:refresh', {})
    })

    api.configuration.register(fileExplorerPlugin.manifest.contributes!.configuration!)
  },
}
