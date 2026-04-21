// Legacy template plugin — retained on disk but NOT loaded from main.tsx.
// The panel-area concept was retired by Phase 7 of the leaf-migration
// (see docs/leaf-migration-plan.md). The bottom-dock restoration is
// tracked under follow-up task #11; when that lands this file will
// either be deleted or rewritten against the new workspace node.
import type { Plugin } from '../../../types/plugin'
import { usePanelAreaStore } from './panelAreaStore'

export { usePanelAreaStore } from './panelAreaStore'
export type { PanelTab } from './panelAreaStore'

void usePanelAreaStore // keep the symbol reachable for dead-code analysis

export const panelAreaPlugin: Plugin = {
  manifest: {
    id: 'core.panel-area',
    name: 'Panel Area',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: 'panel.toggle', title: 'Toggle Panel', category: 'View' },
      ],
      keybindings: [
        { command: 'panel.toggle', key: 'ctrl+j', mac: 'cmd+j' },
      ],
    },
  },
  activate() {
    // No-op: the workspace owns bottom-dock state (task #11 pending).
  },
}
