// BL-067 Phase 1 — View Builder plugin.
//
// Registers a sidebar leaf for managing named workspace layouts:
//
//   - List saved layouts under `<forge>/.forge/layouts/`
//   - Save the current layout under a user-typed name
//   - Apply a saved layout (replaces the live layout via
//     `workspace.applySnapshot`)
//   - Delete a saved layout
//
// Drag-drop / WYSIWYG canvas, "Export as plugin" code generation, and
// per-panel size / dock-side configuration are deferred BL-067
// follow-ups. This Phase 1 surface ships the programmatic save/load
// half so a user can capture and switch between layouts today.

import { createElement } from 'react'

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { clientLogger } from '../../../clientLogger'

import { ViewBuilderView } from './ViewBuilderView'
import { viewBuilderPaneViewCreator } from './ViewBuilderPaneView'
import {
  deleteLayout,
  loadLayout,
  refreshLayouts,
  saveLayout,
  useLayoutsStore,
} from './layoutsStore'
import { buildExportedFiles, writeExportedPlugin } from './exporter'

const VIEW_ID = 'nexus.viewBuilder.view'
const VIEW_TYPE = 'viewBuilder'

const COMMAND_SHOW = 'nexus.viewBuilder.show'
const COMMAND_SAVE_LAYOUT_AS = 'nexus.viewBuilder.saveLayoutAs'
const COMMAND_SWITCH_LAYOUT = 'nexus.viewBuilder.switchLayout'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

export const viewBuilderPlugin: Plugin = {
  manifest: {
    id: 'nexus.viewBuilder',
    name: 'View Builder',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace'],
  },

  async activate(api: PluginAPI) {
    const handleSave = async (name: string) => {
      const snapshot = workspace.layoutSnapshot()
      await saveLayout(api.kernel, name, snapshot)
      await refreshLayouts(api.kernel)
    }

    const handleApply = async (name: string) => {
      const snapshot = await loadLayout(api.kernel, name)
      await workspace.applySnapshot(snapshot)
    }

    const handleDelete = async (name: string) => {
      await deleteLayout(api.kernel, name)
      await refreshLayouts(api.kernel)
    }

    const handleExport = async (name: string): Promise<string> => {
      const layout = await loadLayout(api.kernel, name)
      const files = buildExportedFiles(name, layout)
      return writeExportedPlugin(api.kernel, files)
    }

    const refresh = () => {
      void refreshLayouts(api.kernel).catch((err) => {
        clientLogger.warn('[nexus.viewBuilder] refresh failed', err)
      })
    }

    const renderView = () =>
      createElement(ViewBuilderView, {
        onApply: handleApply,
        onSave: handleSave,
        onDelete: handleDelete,
        onExport: handleExport,
        onRefresh: refresh,
      })

    viewRegistry.register(VIEW_TYPE, viewBuilderPaneViewCreator(renderView))

    api.activityBar.addItem({
      id: 'nexus.viewBuilder.activityItem',
      icon: '',
      iconName: 'template',
      title: 'View Builder',
      viewId: VIEW_ID,
      priority: 60,
      command: COMMAND_SHOW,
    })

    api.commands.register(COMMAND_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
    })

    api.commands.register(COMMAND_SAVE_LAYOUT_AS, async () => {
      // The shell command palette doesn't expose a free-text prompt,
      // so the dedicated form lives inside the panel. Surface the
      // panel and let the user type the name there.
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
    })

    api.commands.register(COMMAND_SWITCH_LAYOUT, async () => {
      // Same UX rationale — surface the panel; the saved-layouts
      // list there is the canonical switcher. A future palette
      // extension that supports a "subcommand picker" would fit
      // here, but it doesn't exist today.
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      refresh()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      // No cross-forge state to keep — wipe the saved-layouts store
      // so the panel doesn't briefly show stale rows from the
      // previous forge.
      useLayoutsStore.getState().reset()
    })
    if (await api.kernel.available()) {
      refresh()
    }
  },
}
