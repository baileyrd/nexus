import type { Plugin, PluginAPI } from '../../../types/plugin'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import { useWorkspaceStore } from './workspaceStore'
import { WorkspaceStatusItem } from './WorkspaceStatusItem'

const STORAGE_KEY = 'rootPath'
const CONTEXT_KEY_ROOT = 'nexus.workspace.rootPath'
const CONTEXT_KEY_HAS_ROOT = 'nexus.workspace.hasRoot'
const EVENT_OPENED = 'workspace:opened'
const EVENT_CLOSED = 'workspace:closed'
const COMMAND_OPEN = 'nexus.workspace.open'
const COMMAND_SET_ROOT = 'nexus.workspace.setRoot'

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
        {
          id: COMMAND_SET_ROOT,
          title: 'Set Workspace Root',
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
        console.info('[nexus.workspace] saved root:', path)
        api.events.emit(EVENT_OPENED, { path })
      } else {
        api.storage.delete(STORAGE_KEY)
        api.events.emit(EVENT_CLOSED, {})
      }
    }

    const persisted = api.storage.get(STORAGE_KEY)
    console.info('[nexus.workspace] boot — persisted root:', persisted ?? '<none>')
    if (persisted) {
      try {
        const stillExists = await invoke<boolean>('path_exists', { path: persisted })
        if (stillExists) {
          console.info('[nexus.workspace] restoring', persisted)
          setRoot(persisted)
        } else {
          console.info('[nexus.workspace] persisted path no longer exists, clearing')
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

    // Direct path activation — the launcher's recents row bypasses the
    // folder-picker and hands us a path we already trust. Centralises
    // the full setRoot dance (store + context + storage + event) so
    // the launcher doesn't duplicate any of it.
    api.commands.register(COMMAND_SET_ROOT, async (...args: unknown[]) => {
      const path = args[0]
      if (typeof path !== 'string' || path.length === 0) {
        return null
      }
      setRoot(path)
      return path
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
