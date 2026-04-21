import type { Plugin } from '../../../types/plugin'

/**
 * Phase 7 (leaf-migration-plan.md): the legacy sidebar host + its
 * slot:'sidebar' registration + the `sidebar:showView`/`sidebar:hide`
 * event pair were removed when the left sidedock became a workspace
 * sidedock rendered by <Workspace>. Plugins that previously emitted
 * those events now call `workspace.ensureLeafOfType + revealLeaf`
 * from their focus command.
 *
 * The plugin manifest is kept so that `dependsOn: ['nexus.sidebar']`
 * declarations in existing plugins still resolve without requiring a
 * host-wide rename. The activate hook is a no-op.
 */
export const sidebarPlugin: Plugin = {
  manifest: {
    id: 'nexus.sidebar',
    name: 'Sidebar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate() {
    // no-op — retained purely for dependency-graph compatibility.
  },
}
