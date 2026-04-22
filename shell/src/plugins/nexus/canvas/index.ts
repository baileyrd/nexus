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

export const canvasPlugin: Plugin = {
  manifest: {
    id: 'nexus.canvas',
    name: 'Canvas',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace'],
  },

  async activate(api: PluginAPI) {
    const client = makeCanvasKernelClient(api.kernel)

    viewRegistry.register(
      'canvas',
      canvasPaneViewCreator((relpath) => {
        if (!relpath) {
          return createElement('div', {
            style: {
              padding: 16,
              color: 'var(--fg-muted, #9ca3af)',
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
  },
}
