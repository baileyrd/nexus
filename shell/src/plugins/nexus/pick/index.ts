// BL-077 follow-up — list-picker overlay plugin.
//
// Mirrors the `nexus.confirm` shape: registers a single overlay
// view; the modal reads from `usePickStore`. `api.input.pick(...)`
// (host/PluginAPI.ts) lazy-imports `requestPick` so the store
// enqueues a request and the modal pops up. Multiple in-flight
// `pick` calls serialise behind the same store queue as confirm.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { PickModal } from './PickModal'

const VIEW_ID = 'nexus.pick.modal'

export const pickPlugin: Plugin = {
  manifest: {
    id: 'nexus.pick',
    name: 'Pick',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.views.register(VIEW_ID, {
      slot: 'overlay',
      component: PickModal,
      // Sit at the same priority as confirm (90). Two overlays
      // shouldn't render simultaneously — pick + confirm are both
      // user-blocking flows and the input layer serialises them via
      // their respective stores.
      priority: 90,
    })
  },
}
