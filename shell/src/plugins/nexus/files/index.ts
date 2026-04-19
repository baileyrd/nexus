import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { FilesTree } from './FilesTree'
import { useFilesStore, type FilesDirEntry } from './filesStore'

const VIEW_ID = 'nexus.files.tree'
const EVENT_FILE_OPEN = 'files:open'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

// Lucide-style folder path. Stroke-only, 24×24 viewbox.
const FOLDER_ICON_PATH =
  'M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2z'

export const filesPlugin: Plugin = {
  manifest: {
    id: 'nexus.files',
    name: 'Files',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    const handleFileActivate = (entry: FilesDirEntry) => {
      api.events.emit(EVENT_FILE_OPEN, { path: entry.path, name: entry.name })
    }

    api.views.register(VIEW_ID, {
      slot: 'sidebarContent',
      component: () => createElement(FilesTree, { onFileActivate: handleFileActivate }),
      priority: 10,
    })

    api.activityBar.addItem({
      id: 'nexus.files.activityItem',
      icon: '',
      iconPath: FOLDER_ICON_PATH,
      title: 'Files',
      viewId: VIEW_ID,
      priority: 10,
    })

    // Reset tree cache when workspace changes so stale children don't show
    // after pointing Nexus at a different folder.
    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      useFilesStore.getState().reset()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useFilesStore.getState().reset()
    })
  },
}
