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

    const onOpenFolder = async () => {
      const picked = await api.commands.execute(COMMAND_OPEN)
      if (typeof picked === 'string' && picked.length > 0) {
        // nexus.workspace set the root + emitted workspace:opened; we
        // just need to persist to recents.
        await useLauncherStore.getState().openPath(picked)
      }
    }

    const onActivatePath = async (path: string) => {
      // Promote to recents first so the persisted "last" is correct,
      // then let nexus.workspace set the root + emit the event.
      await useLauncherStore.getState().openPath(path)
      await api.commands.execute(COMMAND_SET_ROOT, path)
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
