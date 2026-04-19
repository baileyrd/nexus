import type { Plugin, PluginAPI } from '../../../types/plugin'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { exists } from '@tauri-apps/plugin-fs'
import { useWorkspaceStore } from './workspaceStore'
import { WorkspaceStatusItem } from './WorkspaceStatusItem'

const STORAGE_KEY = 'rootPath'
const CONTEXT_KEY_ROOT = 'nexus.workspace.rootPath'
const CONTEXT_KEY_HAS_ROOT = 'nexus.workspace.hasRoot'
const EVENT_OPENED = 'workspace:opened'
const EVENT_CLOSED = 'workspace:closed'
const COMMAND_OPEN = 'nexus.workspace.open'

export const workspacePlugin: Plugin = {
  manifest: {
    id: 'nexus.workspace',
    name: 'Workspace',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        {
          id: COMMAND_OPEN,
          title: 'Open Folder…',
          category: 'Workspace',
        },
      ],
      keybindings: [
        {
          command: COMMAND_OPEN,
          key: 'ctrl+o',
          mac: 'cmd+o',
        },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_ROOT,
          description: 'Absolute path of the current workspace root, or empty string when none.',
          type: 'string',
        },
        {
          key: CONTEXT_KEY_HAS_ROOT,
          description: 'True when a workspace folder is open.',
          type: 'boolean',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const store = useWorkspaceStore.getState()

    const setRoot = (path: string | null) => {
      useWorkspaceStore.getState().setRootPath(path)
      api.context.set(CONTEXT_KEY_ROOT, path ?? '')
      api.context.set(CONTEXT_KEY_HAS_ROOT, path !== null)
      if (path) {
        api.storage.set(STORAGE_KEY, path)
        api.events.emit(EVENT_OPENED, { path })
      } else {
        api.storage.delete(STORAGE_KEY)
        api.events.emit(EVENT_CLOSED, {})
      }
    }

    const persisted = api.storage.get(STORAGE_KEY)
    if (persisted) {
      try {
        if (await exists(persisted)) {
          setRoot(persisted)
        } else {
          api.storage.delete(STORAGE_KEY)
          setRoot(null)
        }
      } catch (err) {
        console.warn('[nexus.workspace] failed to verify persisted path:', err)
        setRoot(null)
      }
    } else {
      setRoot(null)
    }

    api.commands.register(COMMAND_OPEN, async () => {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: 'Open Workspace Folder',
      })
      if (typeof picked === 'string') {
        setRoot(picked)
      }
      return picked ?? null
    })

    store.setOpenHandler(() => {
      api.commands.execute(COMMAND_OPEN)
    })

    api.views.register('nexus.workspace.statusItem', {
      slot: 'statusBarLeft',
      component: WorkspaceStatusItem,
      priority: 10,
    })
  },
}
