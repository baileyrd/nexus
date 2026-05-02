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
// Both code paths converge on the same state-setting dance inside
// nexus.workspace, keeping the source of truth in one place.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLauncherStore } from './launcherState'
import { LauncherView } from './LauncherView'
import { clientLogger } from '../../../clientLogger'

const EVENT_OPENED = 'workspace:opened'
const EVENT_CLOSED = 'workspace:closed'
const COMMAND_OPEN = 'nexus.workspace.open'
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
    // Populate recents before the view renders
    await useLauncherStore.getState().load()

    // Re-load on open/close so the list reflects reality the next time
    // the user returns to the launcher
    api.events.on(EVENT_OPENED, () => {
      void useLauncherStore.getState().load()
    })
    api.events.on(EVENT_CLOSED, () => {
      void useLauncherStore.getState().load()
    })

    const reportBootFailure = (path: string | null, err: unknown) => {
      const message = err instanceof Error ? err.message : String(err)
      clientLogger.warn('[nexus.launcher] workspace command failed', path, err)
      // api.notifications.show is always defined (PluginAPI contract); if
      // the notification service hasn't loaded yet it silently falls back
      // to a console line, which is fine.
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
          // nexus.workspace only resolves the command after boot_kernel
          // succeeded — so recording to recents here is safe. On failure
          // the command throws and we land in the catch below instead.
          await useLauncherStore.getState().openPath(picked)
        }
      } catch (err) {
        reportBootFailure(null, err)
      }
    }

    const onActivatePath = async (path: string) => {
      try {
        await api.commands.execute(COMMAND_SET_ROOT, path)
        // Only promote to recents after the kernel has booted cleanly —
        // otherwise a broken path would get promoted to the top of the
        // list and the user would hit the same failure next launch.
        await useLauncherStore.getState().openPath(path)
      } catch (err) {
        reportBootFailure(path, err)
      }
    }

    // Wrap the view so it can close over the callbacks without other
    // plugins having to reach into the launcher's store. Written as
    // createElement since this is a .ts file (not .tsx); the child
    // component itself owns the JSX.
    const LauncherSlot = () =>
      createElement(LauncherView, { onOpenFolder, onActivatePath })

    api.views.register('nexus.launcher.view', {
      slot: 'overlay',
      component: LauncherSlot,
      priority: 10,
    })
  },
}
