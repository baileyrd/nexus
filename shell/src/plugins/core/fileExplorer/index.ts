// src/plugins/core/fileExplorer/index.ts
import type { Plugin, PluginAPI } from '../../../types/plugin'

const FILE_CREATION_NOTIFICATION_MS = 2000
const CONFIG_KEY_FILE_CREATION = 'ui.fileCreationNotificationMs'

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
        category: 'files',
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
          {
            key: CONFIG_KEY_FILE_CREATION,
            title: 'File creation notification duration',
            description: 'Auto-dismiss duration for file creation notifications in milliseconds',
            type: 'number' as const,
            default: FILE_CREATION_NOTIFICATION_MS,
          },
        ],
      },
    },
  },

  activate(api: PluginAPI) {
    // Phase 7: legacy slot:'sidebarContent' registration removed.

    // File explorer view is reached via the sidebar tab strip; no
    // activity-bar entry needed (duplicate of sidebar-tab affordance).

    api.commands.register('fileExplorer.openFolder', async () => {
      const selected = await api.platform.dialog.openDirectory()
      if (selected) api.events.emit('fileExplorer:folderOpened', { path: selected })
    })

    api.commands.register('fileExplorer.newFile', async () => {
      const name = await api.input.prompt('File name:')
      if (!name) return
      api.notifications.show({ message: `Created: ${name}`, type: 'success', duration: api.configuration.getValue<number>(CONFIG_KEY_FILE_CREATION, FILE_CREATION_NOTIFICATION_MS) ?? FILE_CREATION_NOTIFICATION_MS })
    })

    api.commands.register('fileExplorer.refresh', () => {
      api.events.emit('fileExplorer:refresh', {})
    })

    api.configuration.register(fileExplorerPlugin.manifest.contributes!.configuration!)
  },
}
