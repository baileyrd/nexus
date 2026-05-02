import type { Plugin, PluginAPI } from '../../../types/plugin'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import { clientLogger } from '../../../clientLogger'
import { useWorkspaceStore } from './workspaceStore'
import { WorkspaceStatusItem } from './WorkspaceStatusItem'

const STORAGE_KEY = 'rootPath'
const CONTEXT_KEY_ROOT = 'nexus.workspace.rootPath'
const CONTEXT_KEY_HAS_ROOT = 'nexus.workspace.hasRoot'
const EVENT_OPENED = 'workspace:opened'
const EVENT_CLOSED = 'workspace:closed'
const COMMAND_OPEN = 'nexus.workspace.open'
const COMMAND_SET_ROOT = 'nexus.workspace.setRoot'
const COMMAND_CLOSE = 'nexus.workspace.close'

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
        {
          id: COMMAND_CLOSE,
          title: 'Close Workspace',
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

    // Single source of truth for every workspace state transition. Each
    // transition pairs store/context/storage/event updates with the matching
    // kernel lifecycle step:
    //   null  → path  : init_forge → boot_kernel → store update → event
    //   path  → other : shutdown_kernel → init_forge → boot_kernel → event
    //   path  → null  : shutdown_kernel → store null → event
    //   null  → null  : no-op on kernel; just sync context/storage
    //
    // Ordering contract (downstream plugin authors): `workspace:opened`
    // is emitted AFTER boot_kernel resolves, so any handler may assume
    // `api.kernel.available()` is true and issue `api.kernel.invoke` calls
    // immediately. Between a `workspace:closed` and the subsequent
    // `workspace:opened` the kernel is NOT booted.
    //
    // On boot failure the store is force-cleared to null and the original
    // error is re-thrown so callers (e.g. nexus.launcher) can decide not to
    // record the path into their recents list.
    // BL-029 Phase 2b — popout webviews share the main window's already-
    // booted kernel via Tauri managed state. The popout MUST NOT issue
    // `init_forge` / `boot_kernel` / `shutdown_kernel` (the latter would
    // tear down the main window's kernel). In popout mode `setRoot` is a
    // pure UI-state sync.
    const popoutMode = api.context.get('popoutMode') === true

    const setRoot = async (path: string | null): Promise<void> => {
      const prev = useWorkspaceStore.getState().rootPath

      // No-op fast path for null → null; still make sure the UI surfaces
      // reflect "no workspace" on a fresh boot.
      if (prev === null && path === null) {
        useWorkspaceStore.getState().setRootPath(null)
        api.context.set(CONTEXT_KEY_ROOT, '')
        api.context.set(CONTEXT_KEY_HAS_ROOT, false)
        api.storage.delete(STORAGE_KEY)
        api.events.emit(EVENT_CLOSED, {})
        return
      }

      // Shut down the previous kernel first if one is booted — covers both
      // the switch (path → other) and close (path → null) cases. Skipped
      // in popout mode (kernel is owned by the main window).
      if (prev !== null && !popoutMode) {
        try {
          await invoke('shutdown_kernel')
        } catch (err) {
          clientLogger.warn('[nexus.workspace] shutdown_kernel failed (continuing):', err)
        }
      }

      if (path !== null && !popoutMode) {
        try {
          await invoke('init_forge', { path })
          // In the e2e harness the Rust `setup` hook may have already
          // booted the kernel (NEXUS_E2E_VAULT path) — in that case
          // skip boot_kernel rather than erroring with "kernel already
          // booted". Normal runs hit the `!alreadyBooted` branch.
          const alreadyBooted = await invoke<boolean>('kernel_is_booted')
          if (!alreadyBooted) {
            await invoke('boot_kernel', { path })
          }
        } catch (err) {
          clientLogger.error('[nexus.workspace] kernel boot failed for', path, err)
          // Force-clear so the UI reflects "no workspace" rather than
          // stalling on a half-booted state.
          useWorkspaceStore.getState().setRootPath(null)
          api.context.set(CONTEXT_KEY_ROOT, '')
          api.context.set(CONTEXT_KEY_HAS_ROOT, false)
          api.storage.delete(STORAGE_KEY)
          api.events.emit(EVENT_CLOSED, {})
          throw err
        }
      }

      // Kernel is now in the desired state. Sync the UI surfaces.
      useWorkspaceStore.getState().setRootPath(path)
      api.context.set(CONTEXT_KEY_ROOT, path ?? '')
      api.context.set(CONTEXT_KEY_HAS_ROOT, path !== null)
      if (path) {
        api.storage.set(STORAGE_KEY, path)
        clientLogger.info('[nexus.workspace] saved root:', path)
        api.events.emit(EVENT_OPENED, { path })
      } else {
        api.storage.delete(STORAGE_KEY)
        api.events.emit(EVENT_CLOSED, {})
      }
    }

    // Primary source: plugin-local localStorage (normal runs). Fallback:
    // the shell-state file's lastForgePath — populated by the Rust setup
    // hook when NEXUS_E2E_VAULT is set, so the e2e harness's pre-booted
    // kernel is picked up here without any special e2e branching.
    let persisted = api.storage.get(STORAGE_KEY)
    if (!persisted) {
      try {
        const state = await invoke<{ lastForgePath: string | null }>('get_shell_state')
        if (state.lastForgePath) {
          persisted = state.lastForgePath
          clientLogger.info('[nexus.workspace] restoring from shell-state lastForgePath')
        }
      } catch (err) {
        clientLogger.warn('[nexus.workspace] get_shell_state failed:', err)
      }
    }
    clientLogger.info('[nexus.workspace] boot — persisted root:', persisted ?? '<none>')
    if (persisted) {
      try {
        const stillExists = await invoke<boolean>('path_exists', { path: persisted })
        if (stillExists) {
          clientLogger.info('[nexus.workspace] restoring', persisted)
          try {
            await setRoot(persisted)
          } catch (err) {
            // boot_kernel failed against the persisted path (corrupt forge,
            // migration needed, etc.). setRoot already cleared storage +
            // emitted workspace:closed, so the launcher will appear. Just
            // log and move on rather than propagating out of activate().
            clientLogger.warn(
              '[nexus.workspace] kernel boot failed for persisted path, falling back to launcher:',
              err,
            )
          }
        } else {
          clientLogger.info('[nexus.workspace] persisted path no longer exists, clearing')
          api.storage.delete(STORAGE_KEY)
          await setRoot(null)
        }
      } catch (err) {
        clientLogger.warn('[nexus.workspace] failed to verify persisted path:', err)
        await setRoot(null)
      }
    } else {
      await setRoot(null)
    }

    api.commands.register(COMMAND_OPEN, async () => {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: 'Open Workspace Folder',
      })
      if (typeof picked === 'string') {
        // Let boot errors propagate so the launcher can skip recents.
        await setRoot(picked)
        return picked
      }
      return null
    })

    // Direct path activation — the launcher's recents row bypasses the
    // folder-picker and hands us a path we already trust. Centralises
    // the full setRoot dance (kernel + store + context + storage + event)
    // so the launcher doesn't duplicate any of it. Errors propagate so
    // the caller can decide how to react (e.g. not persist to recents).
    api.commands.register(COMMAND_SET_ROOT, async (...args: unknown[]) => {
      const path = args[0]
      if (typeof path !== 'string' || path.length === 0) {
        return null
      }
      await setRoot(path)
      return path
    })

    // Close the current workspace. Drains the kernel and clears UI state.
    // No keybinding — future "Close Workspace" menu item will hang off this.
    api.commands.register(COMMAND_CLOSE, async () => {
      await setRoot(null)
      return null
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
