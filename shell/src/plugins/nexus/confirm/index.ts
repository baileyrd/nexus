import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ConfirmModal } from './ConfirmModal'

const VIEW_ID = 'nexus.confirm.modal'

/**
 * Renders the shared confirm modal in the overlay slot. The modal
 * itself reads from `useConfirmStore` and resolves the pending
 * Promise on click — host/PluginAPI.ts wires `api.input.confirm` to
 * `requestConfirm` (see ./confirmStore.ts), so plugins keep calling
 * `api.input.confirm(...)` and don't need to know this exists.
 *
 * Load order is irrelevant: `requestConfirm` enqueues onto the store
 * even before the modal mounts, and the modal picks up `current` on
 * its first render.
 */
export const confirmPlugin: Plugin = {
  manifest: {
    id: 'nexus.confirm',
    name: 'Confirm',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.views.register(VIEW_ID, {
      slot: 'overlay',
      component: ConfirmModal,
      // Above MCP's tool-call modal (30) and pluginsMgmt (20) so a
      // confirm raised from inside another modal lands on top. Two
      // overlays should rarely overlap, but the destination ordering
      // is explicit.
      priority: 90,
    })
  },
}
