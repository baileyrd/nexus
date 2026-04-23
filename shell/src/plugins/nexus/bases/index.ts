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

export const basesPlugin: Plugin = {
  manifest: {
    id: 'nexus.bases',
    name: 'Bases',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace'],
  },

  async activate(api: PluginAPI) {
    const client = makeBasesKernelClient(api.kernel)

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
    viewRegistry.registerExtensions(['bases'], 'bases')
  },
}
