// src/plugins/nexus/launcher/index.ts
//
// Obsidian-style workspace picker. Populates from persisted recents on
// activate and re-loads whenever the workspace opens or closes so the
// list is fresh the next time the launcher is visible.
//
// The launcher itself owns no routing; it delegates to nexus.workspace
// commands:
//   - "Open" / "Create" → nexus.workspace.open (existing folder picker).
//     On success the chosen path is written to recents.
//   - Recents-row click → nexus.workspace.setRoot (new command, accepts
//     a path argument) so we skip the dialog.
//   - "Open remote forge…" → opens an in-shell modal (BL-148) that
//     composes an `ssh://` URI and dispatches
//     `nexus.workspace.openRemote`.
// Both code paths converge on the same state-setting dance inside
// nexus.workspace, keeping the source of truth in one place.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLauncherStore, type RemoteForgeRecent } from './launcherState'
import { LauncherView } from './LauncherView'
import { RemoteConnectionDialog } from './RemoteConnectionDialog'
import { clientLogger } from '../../../clientLogger'

const EVENT_OPENED = 'workspace:opened'
const EVENT_CLOSED = 'workspace:closed'
const COMMAND_OPEN = 'nexus.workspace.open'
const COMMAND_OPEN_WITH_TEMPLATE = 'nexus.workspace.openWithTemplate'
const COMMAND_OPEN_REMOTE = 'nexus.workspace.openRemote'
const COMMAND_SET_ROOT = 'nexus.workspace.setRoot'

export const launcherPlugin: Plugin = {
  manifest: {
    id: 'nexus.launcher',
    name: 'Launcher',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    await useLauncherStore.getState().load()

    api.events.on(EVENT_OPENED, () => {
      void useLauncherStore.getState().load()
    })
    api.events.on(EVENT_CLOSED, () => {
      void useLauncherStore.getState().load()
    })

    const reportBootFailure = (path: string | null, err: unknown) => {
      const message = err instanceof Error ? err.message : String(err)
      clientLogger.warn('[nexus.launcher] workspace command failed', path, err)
      try {
        api.notifications.show({
          type: 'warning',
          message: path
            ? `Could not open workspace at ${path}: ${message}`
            : `Could not open workspace: ${message}`,
        })
      } catch (notifyErr) {
        clientLogger.warn('[nexus.launcher] notifications.show failed:', notifyErr)
      }
    }

    const onOpenFolder = async () => {
      try {
        const picked = await api.commands.execute(COMMAND_OPEN)
        if (typeof picked === 'string' && picked.length > 0) {
          await useLauncherStore.getState().openPath(picked)
        }
      } catch (err) {
        reportBootFailure(null, err)
      }
    }

    // BL-054 Phase 1 follow-up: pick a folder and scaffold the OS
    // layout into it before booting. Same recents semantics as
    // `onOpenFolder` — only promote on success.
    const onOpenWithOsTemplate = async () => {
      try {
        const picked = await api.commands.execute(COMMAND_OPEN_WITH_TEMPLATE, 'os')
        if (typeof picked === 'string' && picked.length > 0) {
          await useLauncherStore.getState().openPath(picked)
        }
      } catch (err) {
        reportBootFailure(null, err)
      }
    }

    const onActivatePath = async (path: string) => {
      try {
        await api.commands.execute(COMMAND_SET_ROOT, path)
        await useLauncherStore.getState().openPath(path)
      } catch (err) {
        reportBootFailure(path, err)
      }
    }

    const activateRemote = async (entry: RemoteForgeRecent) => {
      try {
        await api.commands.execute(COMMAND_OPEN_REMOTE, entry.uri)
        await useLauncherStore.getState().openRemote(entry)
      } catch (err) {
        reportBootFailure(entry.uri, err)
      }
    }

    // BL-148 — open the in-shell remote-connection modal. Submitting it
    // dispatches `nexus.workspace.openRemote` with the composed URI;
    // the connection is then persisted to `remoteForgeRecents` so
    // future launches can resume it from the recents list.
    const onOpenRemote = () => {
      useLauncherStore.getState().setRemoteModalOpen(true)
    }

    const LauncherSlot = () =>
      createElement(LauncherView, {
        onOpenFolder,
        onOpenWithOsTemplate,
        onOpenRemote,
        onActivatePath,
        onActivateRemote: (entry) => {
          void activateRemote(entry)
        },
      })

    const RemoteDialogSlot = () => {
      const open = useLauncherStore((s) => s.remoteModalOpen)
      const setOpen = useLauncherStore((s) => s.setRemoteModalOpen)
      return createElement(RemoteConnectionDialog, {
        open,
        onCancel: () => setOpen(false),
        onSubmit: async (submission) => {
          setOpen(false)
          await activateRemote({ uri: submission.uri, label: submission.label })
        },
      })
    }

    api.views.register('nexus.launcher.view', {
      slot: 'overlay',
      component: LauncherSlot,
      priority: 10,
    })

    api.views.register('nexus.launcher.remoteDialog', {
      slot: 'overlay',
      component: RemoteDialogSlot,
      priority: 11,
    })
  },
}
