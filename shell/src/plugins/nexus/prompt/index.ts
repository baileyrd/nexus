// Styled `prompt` overlay plugin — replaces the platform `window.prompt`
// fallback that `api.input.prompt` used to invoke directly.
//
// Mirrors `nexus.confirm` / `nexus.pick`: registers a single overlay
// view; the modal reads from `usePromptStore`. `api.input.prompt`
// (host/PluginAPI.ts) lazy-imports `requestPrompt` so the store
// enqueues a request and the modal pops up.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { PromptModal } from './PromptModal'

const VIEW_ID = 'nexus.prompt.modal'

export const promptPlugin: Plugin = {
  manifest: {
    id: 'nexus.prompt',
    name: 'Prompt',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.views.register(VIEW_ID, {
      slot: 'overlay',
      component: PromptModal,
      // Same priority as confirm / pick — only one of them ever
      // renders at a time because each is gated on its own store
      // having a non-null `current`.
      priority: 90,
    })
  },
}
