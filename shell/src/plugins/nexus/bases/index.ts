// Phase 1 of docs/bases-shell-plan.md — routing + skeleton leaf.
// The base_* IPC handlers on com.nexus.storage (ids 16/17/21/26 for
// read/index/query/list and 40–48 for CRUD) already ship; this
// plugin claims the `.bases` extension so opening one mounts our
// view instead of falling through to CodeMirror.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { BasesView } from './BasesView'
import { basesPaneViewCreator } from './BasesPaneView'
import { makeBasesKernelClient } from './kernelClient'
import { NewBaseDialog } from './NewBaseDialog'
import { useNewBaseStore } from './newBaseStore'
import { setRuntime } from './runtime'
import { withActiveBases } from './activeBases'

const COMMAND_NEW = 'nexus.bases.new'

/** P2-03 — default file extensions claimed by the bases view.
 *  `.bases` directories ship the multi-file YAML form; `.base` is the
 *  Obsidian single-file YAML variant (ADR 0019, read-only). Override
 *  via the `nexus.bases.fileExtensions` setting (string[] of bare
 *  extensions, no leading dot) — useful if you ship a forge with a
 *  custom bases extension. */
const DEFAULT_FILE_EXTENSIONS: readonly string[] = ['bases', 'base']
const FILE_EXTENSIONS_SETTING = 'nexus.bases.fileExtensions'
const EVENT_FILE_OPEN = 'files:open'
const DIALOG_VIEW_ID = 'nexus.bases.newDialog'

/** Command ids exported so other modules (e.g. menu/toolbar
 *  contributions) can reference them without hard-coding strings. */
export const BASES_COMMANDS = {
  undo: 'nexus.bases.undo',
  redo: 'nexus.bases.redo',
  cut: 'nexus.bases.cut',
  copy: 'nexus.bases.copy',
  paste: 'nexus.bases.paste',
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
        { id: BASES_COMMANDS.cut, title: 'Bases: Cut', category: 'Bases' },
        { id: BASES_COMMANDS.copy, title: 'Bases: Copy', category: 'Bases' },
        { id: BASES_COMMANDS.paste, title: 'Bases: Paste', category: 'Bases' },
      ],
      // Mirror the canvas pattern (canvas.focused) — chords only fire
      // when a `.bases` leaf actually owns focus. The activeBases
      // handle is published on focusin from BasesView.
      //
      // The cut/copy/paste keybindings additionally guard on
      // `!bases.editing` so a Mod-V inside an active CellEditor
      // textfield inserts text instead of triggering a paste.
      keybindings: [
        { command: BASES_COMMANDS.undo, key: 'ctrl+z', mac: 'cmd+z', when: 'bases.focused' },
        { command: BASES_COMMANDS.redo, key: 'ctrl+shift+z', mac: 'cmd+shift+z', when: 'bases.focused' },
        { command: BASES_COMMANDS.redo, key: 'ctrl+y', mac: 'cmd+y', when: 'bases.focused' },
        { command: BASES_COMMANDS.cut, key: 'ctrl+x', mac: 'cmd+x', when: 'bases.focused && !bases.editing' },
        { command: BASES_COMMANDS.copy, key: 'ctrl+c', mac: 'cmd+c', when: 'bases.focused && !bases.editing' },
        { command: BASES_COMMANDS.paste, key: 'ctrl+v', mac: 'cmd+v', when: 'bases.focused && !bases.editing' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const client = makeBasesKernelClient(api.kernel)
    setRuntime(api, client)

    api.viewRegistry.register(
      'bases',
      basesPaneViewCreator((relpath) => {
        if (!relpath) {
          return createElement(
            'div',
            {
              style: {
                padding: 16,
                color: 'var(--text-muted)',
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
    const fileExtensions = api.configuration.getValue<string[]>(
      FILE_EXTENSIONS_SETTING,
      [...DEFAULT_FILE_EXTENSIONS],
    )
    api.viewRegistry.registerExtensions(fileExtensions, 'bases')

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
    api.commands.register(BASES_COMMANDS.cut, () => withActiveBases((h) => h.cut()))
    api.commands.register(BASES_COMMANDS.copy, () => withActiveBases((h) => h.copy()))
    api.commands.register(BASES_COMMANDS.paste, () => withActiveBases((h) => h.paste()))

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
