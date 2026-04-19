import type { Plugin, PluginAPI } from '../../../types/plugin'
import { StatusBarLeft, StatusBarRight } from './StatusBarView'

export const statusBarPlugin: Plugin = {
  manifest: {
    id: 'core.status-bar',
    name: 'Status Bar',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    // Register left + right render surfaces.
    api.views.register('statusBarLeft', {
      slot: 'statusBarLeft',  component: StatusBarLeft,  priority: 0,
    })
    api.views.register('statusBarRight', {
      slot: 'statusBarRight', component: StatusBarRight, priority: 0,
    })

    // ── Left cluster ─────────────────────────────────────────────────────
    api.statusBar.createItem({
      id: 'statusBar.sync',
      slot: 'left', priority: 10,
      text: 'Forge synced',
      tooltip: 'Workspace index',
      content: (<><span className="dot" /> Forge synced</>),
    })
    api.statusBar.createItem({
      id: 'statusBar.branch',
      slot: 'left', priority: 20,
      text: 'main · 0000000',
      tooltip: 'Git branch',
      content: (<>main · <code>0000000</code></>),
    })
    api.statusBar.createItem({
      id: 'statusBar.index',
      slot: 'left', priority: 30,
      text: 'Tantivy · 0 docs',
      tooltip: 'Search index',
      content: (<>Tantivy · <code>0 docs</code></>),
    })
    api.statusBar.createItem({
      id: 'statusBar.plugins',
      slot: 'left', priority: 40,
      text: '0 plugins hot',
      tooltip: 'Active plugins',
      className: 'ember',
      content: (<><span className="dot" /> 0 plugins hot</>),
    })

    // ── Right cluster ────────────────────────────────────────────────────
    api.statusBar.createItem({
      id: 'statusBar.position',
      slot: 'right', priority: 10,
      text: 'ln 1, col 1',
      tooltip: 'Cursor position',
    })
    api.statusBar.createItem({
      id: 'statusBar.encoding',
      slot: 'right', priority: 20,
      text: 'MD · UTF-8',
      tooltip: 'File type + encoding',
    })
    api.statusBar.createItem({
      id: 'statusBar.count',
      slot: 'right', priority: 30,
      text: '0 words · 0 chars',
      tooltip: 'Document stats',
    })
    api.statusBar.createItem({
      id: 'statusBar.backlinks',
      slot: 'right', priority: 40,
      text: '0 backlinks missing',
      tooltip: 'Unresolved backlinks',
    })
  },
}
