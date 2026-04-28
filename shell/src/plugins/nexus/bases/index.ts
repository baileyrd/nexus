// Phase 1 of docs/bases-shell-plan.md — routing + skeleton leaf.
// The base_* IPC handlers on com.nexus.storage (ids 16/17/21/26 for
// read/index/query/list and 40–48 for CRUD) already ship; this
// plugin claims the `.bases` extension so opening one mounts our
// view instead of falling through to CodeMirror.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry } from '../../../workspace'
import { BasesView } from './BasesView'
import { basesPaneViewCreator } from './BasesPaneView'
import { makeBasesKernelClient } from './kernelClient'
import { NewBaseDialog } from './NewBaseDialog'
import { useNewBaseStore } from './newBaseStore'
import { setRuntime } from './runtime'
import { withActiveBases } from './activeBases'

const COMMAND_NEW = 'nexus.bases.new'
const EVENT_FILE_OPEN = 'files:open'
const DIALOG_VIEW_ID = 'nexus.bases.newDialog'

/** Command ids exported so other modules (e.g. menu/toolbar
 *  contributions) can reference them without hard-coding strings. */
export const BASES_COMMANDS = {
  undo: 'nexus.bases.undo',
  redo: 'nexus.bases.redo',
} as const

export const basesPlugin: Plugin = {
  manifest: {
    id: 'nexus.bases',
    name: 'Bases',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace'],
    contributes: {
      commands: [
        {
          id: COMMAND_NEW,
          title: 'New base…',
          category: 'Bases',
        },
        { id: BASES_COMMANDS.undo, title: 'Bases: Undo', category: 'Bases' },
        { id: BASES_COMMANDS.redo, title: 'Bases: Redo', category: 'Bases' },
      ],
      // Mirror the canvas pattern (canvas.focused) — chords only fire
      // when a `.bases` leaf actually owns focus. The activeBases
      // handle is published on focusin from BasesView.
      keybindings: [
        { command: BASES_COMMANDS.undo, key: 'ctrl+z', mac: 'cmd+z', when: 'bases.focused' },
        { command: BASES_COMMANDS.redo, key: 'ctrl+shift+z', mac: 'cmd+shift+z', when: 'bases.focused' },
        { command: BASES_COMMANDS.redo, key: 'ctrl+y', mac: 'cmd+y', when: 'bases.focused' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const client = makeBasesKernelClient(api.kernel)
    setRuntime(api, client)

    viewRegistry.register(
      'bases',
      basesPaneViewCreator((relpath) => {
        if (!relpath) {
          return createElement(
            'div',
            {
              style: {
                padding: 16,
                color: 'var(--fg-muted, #9ca3af)',
                fontSize: 12,
              },
            },
            'Bases leaf without a path',
          )
        }
        return createElement(BasesView, { relpath, client })
      }),
    )

    // `.bases` is a directory, not a file — the files tree intercepts
    // clicks on `.bases` entries and emits a files:open with the
    // directory relpath, same as any file. The editor plugin routes
    // the resulting mount through viewRegistry.getTypeForExt().
    // `.bases` (directory) and `.base` (Obsidian single-file YAML —
    // ADR 0019, read-only) both mount the same view component. The
    // BasesView branches on extension to pick the correct loader.
    viewRegistry.registerExtensions(['bases', 'base'], 'bases')

    api.views.register(DIALOG_VIEW_ID, {
      slot: 'overlay',
      component: NewBaseDialog,
      priority: 70,
    })

    // Undo/redo dispatch to the currently-focused bases leaf. A
    // missing handle is a silent no-op (e.g. palette invocation with
    // no base open), matching the `when`-gated keybindings.
    api.commands.register(BASES_COMMANDS.undo, () => withActiveBases((h) => h.undo()))
    api.commands.register(BASES_COMMANDS.redo, () => withActiveBases((h) => h.redo()))

    api.commands.register(COMMAND_NEW, async (args?: unknown) => {
      // Caller may pass `{ parent: string }` to scope the new base to
      // a subdirectory (e.g. invoked from a right-click on a folder).
      const parent =
        typeof args === 'object' && args && 'parent' in args && typeof (args as { parent?: unknown }).parent === 'string'
          ? ((args as { parent: string }).parent)
          : ''
      const result = await useNewBaseStore.getState().request(parent)
      if (!result) return
      api.events.emit(EVENT_FILE_OPEN, {
        relpath: result.relpath,
        name: result.relpath.split('/').pop() ?? result.relpath,
      })
    })
  },
}
