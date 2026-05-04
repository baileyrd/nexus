// Phase 1 of docs/canvas-shell-plan.md — routing + blank surface.
// The five canvas_* IPC handlers (ids 35–39) landed in
// crates/nexus-storage on 2026-04-22. This plugin claims the
// `.canvas` extension so opening one mounts our view instead of
// falling through to CodeMirror.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry } from '../../../workspace'
import { CanvasView } from './CanvasView'
import { canvasPaneViewCreator } from './CanvasPaneView'
import { makeCanvasKernelClient } from './kernelClient'
import { withActiveCanvas } from './activeCanvas'
import { setCanvasApi } from './canvasApi'

/** Command ids are exported so CanvasView can reference them in the
 *  help overlay rather than hard-coding strings. */
export const CANVAS_COMMANDS = {
  new: 'nexus.canvas.new',
  undo: 'canvas.undo',
  redo: 'canvas.redo',
  delete: 'canvas.delete',
  fit: 'canvas.fit',
  fitSelection: 'canvas.fitSelection',
  toggleHelp: 'canvas.toggleHelp',
  closeHelp: 'canvas.closeHelp',
  toggleGrid: 'canvas.toggleGrid',
  toggleBackground: 'canvas.toggleBackground',
  tidy: 'canvas.tidy',
  exportPng: 'canvas.export.png',
  exportSvg: 'canvas.export.svg',
  exportPdf: 'canvas.export.pdf',
} as const

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const EVENT_FILE_OPEN = 'files:open'

interface DirEntry {
  name: string
  isDir: boolean
}

export const canvasPlugin: Plugin = {
  manifest: {
    id: 'nexus.canvas',
    name: 'Canvas',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace'],
    contributes: {
      commands: [
        { id: CANVAS_COMMANDS.new, title: 'Canvas: New canvas', category: 'Canvas' },
        { id: CANVAS_COMMANDS.undo, title: 'Canvas: Undo' },
        { id: CANVAS_COMMANDS.redo, title: 'Canvas: Redo' },
        { id: CANVAS_COMMANDS.delete, title: 'Canvas: Delete selection' },
        { id: CANVAS_COMMANDS.fit, title: 'Canvas: Zoom to fit' },
        { id: CANVAS_COMMANDS.fitSelection, title: 'Canvas: Zoom to selection' },
        { id: CANVAS_COMMANDS.toggleHelp, title: 'Canvas: Toggle shortcut help' },
        { id: CANVAS_COMMANDS.closeHelp, title: 'Canvas: Close help overlay' },
        { id: CANVAS_COMMANDS.toggleGrid, title: 'Canvas: Toggle grid' },
        { id: CANVAS_COMMANDS.toggleBackground, title: 'Canvas: Background inspector' },
        { id: CANVAS_COMMANDS.tidy, title: 'Canvas: Tidy (auto-layout)' },
        { id: CANVAS_COMMANDS.exportPng, title: 'Canvas: Export as PNG' },
        { id: CANVAS_COMMANDS.exportSvg, title: 'Canvas: Export as SVG' },
        { id: CANVAS_COMMANDS.exportPdf, title: 'Canvas: Export as PDF' },
      ],
      keybindings: [
        { command: CANVAS_COMMANDS.undo, key: 'ctrl+z', mac: 'cmd+z', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.redo, key: 'ctrl+shift+z', mac: 'cmd+shift+z', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.redo, key: 'ctrl+y', mac: 'cmd+y', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.delete, key: 'delete', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.delete, key: 'backspace', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.fit, key: 'f', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.fitSelection, key: 'shift+f', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.toggleHelp, key: 'shift+/', when: 'canvas.focused' },
        { command: CANVAS_COMMANDS.closeHelp, key: 'escape', when: 'canvas.focused && canvas.helpOpen' },
      ],
      configuration: {
        pluginId: 'nexus.canvas',
        title: 'Canvas',
        order: 30,
        schema: [
          {
            key: 'canvas.exportMarginUnits',
            title: 'Export margin (units)',
            description: 'Margin around content in world units when exporting canvas (PNG/SVG/PDF).',
            type: 'number',
            default: 48,
          },
          {
            key: 'canvas.exportMarginPx',
            title: 'Export margin (px)',
            description: 'Margin around content in pixels when exporting via the 2D renderer.',
            type: 'number',
            default: 48,
          },
          {
            key: 'canvas.maxExportEdge',
            title: 'Max export edge (px)',
            description: 'Hard cap on the longest pixel edge of an exported canvas image.',
            type: 'number',
            default: 8192,
          },
          {
            key: 'canvas.colorSwatches',
            title: 'Color swatches',
            description: 'Hex color strings shown in the node/edge color picker.',
            // 'array' is not yet in the ConfigSchema union; cast so the
            // settings registry stores it correctly when the type is
            // eventually added.
            type: 'array' as 'string',
            default: ['#ef4444', '#f59e0b', '#eab308', '#22c55e', '#3b82f6', '#8b5cf6', '#ec4899'],
          },
        ],
      },
    },
  },

  async activate(api: PluginAPI) {
    const client = makeCanvasKernelClient(api.kernel)
    setCanvasApi(api)

    viewRegistry.register(
      'canvas',
      canvasPaneViewCreator((relpath) => {
        if (!relpath) {
          return createElement('div', {
            style: {
              padding: 16,
              color: 'var(--text-muted)',
              fontSize: 12,
            },
          }, 'Canvas leaf without a path')
        }
        return createElement(CanvasView, { relpath, client })
      }),
    )

    // Opens `.canvas` files as leaves of view-type `canvas` via the
    // editor plugin's existing viewTypeForFile() path.
    viewRegistry.registerExtensions(['canvas'], 'canvas')

    // Commands all dispatch to the currently-focused canvas leaf. A
    // missing handle is a no-op (e.g. palette invocation with no
    // canvas open), matching the "when"-gated behaviour of the
    // keybindings themselves.
    // New canvas — auto-name "Untitled.canvas" (with collision suffix),
    // write an empty canvas doc, then emit files:open so the editor
    // routes the new path to the canvas view. Mirrors the Obsidian
    // behaviour invoked from the file-explorer right-click menu.
    api.commands.register(CANVAS_COMMANDS.new, async (...args) => {
      const arg = args[0]
      const parent =
        arg && typeof arg === 'object' && 'parent' in arg && typeof (arg as { parent?: unknown }).parent === 'string'
          ? (arg as { parent: string }).parent
          : ''
      try {
        let entries: DirEntry[] = []
        try {
          entries = await api.kernel.invoke<DirEntry[]>(STORAGE_PLUGIN_ID, 'list_dir', {
            relpath: parent,
          })
        } catch {
          // list_dir failure is non-fatal — fall back to "Untitled.canvas"
          // and let canvas_write surface the real error if it collides.
        }
        const taken = new Set(entries.map((e) => e.name.toLowerCase()))
        let name = 'Untitled.canvas'
        let n = 1
        while (taken.has(name.toLowerCase())) {
          name = `Untitled ${n}.canvas`
          n += 1
        }
        const relpath = parent ? `${parent}/${name}` : name
        await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'canvas_write', {
          path: relpath,
          canvas: { version: '1.0', nodes: [], edges: [] },
        })
        api.events.emit(EVENT_FILE_OPEN, { relpath, name })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Failed to create canvas: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(CANVAS_COMMANDS.undo, () => withActiveCanvas((h) => h.undo()))
    api.commands.register(CANVAS_COMMANDS.redo, () => withActiveCanvas((h) => h.redo()))
    api.commands.register(CANVAS_COMMANDS.delete, () => withActiveCanvas((h) => h.deleteSelected()))
    api.commands.register(CANVAS_COMMANDS.fit, () => withActiveCanvas((h) => h.fit()))
    api.commands.register(CANVAS_COMMANDS.fitSelection, () => withActiveCanvas((h) => h.fitSelection()))
    api.commands.register(CANVAS_COMMANDS.toggleHelp, () => withActiveCanvas((h) => h.toggleHelp()))
    api.commands.register(CANVAS_COMMANDS.closeHelp, () => withActiveCanvas((h) => h.closeHelp()))
    api.commands.register(CANVAS_COMMANDS.toggleGrid, () => withActiveCanvas((h) => h.toggleGrid()))
    api.commands.register(CANVAS_COMMANDS.toggleBackground, () =>
      withActiveCanvas((h) => h.toggleBackgroundInspector()),
    )
    api.commands.register(CANVAS_COMMANDS.tidy, () => withActiveCanvas((h) => h.tidy()))
    api.commands.register(CANVAS_COMMANDS.exportPng, () => withActiveCanvas((h) => h.exportPng()))
    api.commands.register(CANVAS_COMMANDS.exportSvg, () => withActiveCanvas((h) => h.exportSvg()))
    api.commands.register(CANVAS_COMMANDS.exportPdf, () => withActiveCanvas((h) => h.exportPdf()))
  },
}
