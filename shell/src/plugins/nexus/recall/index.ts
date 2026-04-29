// shell/src/plugins/nexus/recall/index.ts
//
// BL-044 — MEM recall hotkey plugin.
//
// Cmd/Ctrl+Shift+R opens an in-window overlay that semantic-searches
// the user's capture-notes (BL-043 inbox path) via
// `com.nexus.ai::semantic_search` (BL-040) and lets the user insert a
// quote-formatted snippet at the active editor caret or copy it to the
// clipboard. The PRD line is "Reuses BL-032's Cmd+I overlay shell with
// a different content adapter" — we deliberately scaffold a separate
// overlay rather than thread a mode flag through the AI plugin so the
// recall feature can ship + version independently of Cmd+I.
//
// Dependencies:
//   - `nexus.ai` enabled and configured with an embedding provider
//     (otherwise `semantic_search` returns "no provider configured").
//   - `nexus.memory` (BL-043) ideally also configured so the inbox-path
//     scope filter has something to bind to. Without it the overlay
//     surfaces all semantic results; see `recallRuntime.filterToInboxScope`
//     for the v1 degrade contract.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { RecallOverlay } from './RecallOverlay'
import { setRecallApi } from './recallApi'
import { useRecallStore } from './recallStore'
import { cancelPendingSearch } from './recallRuntime'

const VIEW_ID_OVERLAY = 'nexus.recall.overlay'
const COMMAND_OPEN = 'nexus.recall.open'
const COMMAND_CLOSE = 'nexus.recall.close'
const CONTEXT_KEY_VISIBLE = 'nexus.recall.visible'

const CONFIG_HOTKEY = 'recall.hotkey'
/** Default hotkey — `mod+shift+r`. The shell keymap layer maps `mod`
 *  to Cmd on macOS and Ctrl elsewhere. */
const DEFAULT_HOTKEY_KEY = 'ctrl+shift+r'
const DEFAULT_HOTKEY_MAC = 'cmd+shift+r'

export const recallPlugin: Plugin = {
  manifest: {
    id: 'nexus.recall',
    name: 'AI Recall',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      configuration: {
        pluginId: 'nexus.recall',
        title: 'AI Recall',
        order: 71,
        schema: [
          {
            key: CONFIG_HOTKEY,
            title: 'Recall hotkey',
            description:
              'In-window keybinding that opens the recall overlay. ' +
              'CodeMirror parlance, e.g. "Mod-Shift-r". ' +
              'Reload required after editing.',
            type: 'string' as const,
            default: 'Mod-Shift-r',
          },
        ],
      },
      commands: [
        { id: COMMAND_OPEN, title: 'Recall from capture notes', category: 'AI' },
        { id: COMMAND_CLOSE, title: 'Dismiss recall overlay', category: 'AI' },
      ],
      keybindings: [
        {
          command: COMMAND_OPEN,
          key: DEFAULT_HOTKEY_KEY,
          mac: DEFAULT_HOTKEY_MAC,
        },
        {
          command: COMMAND_CLOSE,
          key: 'escape',
          when: CONTEXT_KEY_VISIBLE,
        },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_VISIBLE,
          description: 'True while the recall overlay is open.',
          type: 'boolean',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.configuration.register(recallPlugin.manifest.contributes!.configuration!)
    setRecallApi(api)

    api.commands.register(COMMAND_OPEN, () => {
      useRecallStore.getState().open()
    })
    api.commands.register(COMMAND_CLOSE, () => {
      cancelPendingSearch()
      useRecallStore.getState().close()
    })

    // Mirror the overlay's `visible` flag into the context-key service
    // so the `escape` keybinding's `when` clause resolves correctly.
    // The same pattern as the Cmd+I overlay.
    api.context.set(CONTEXT_KEY_VISIBLE, useRecallStore.getState().visible)
    useRecallStore.subscribe((state, prev) => {
      if (state.visible !== prev.visible) {
        api.context.set(CONTEXT_KEY_VISIBLE, state.visible)
      }
    })

    api.views.register(VIEW_ID_OVERLAY, {
      slot: 'overlay',
      // Sit just above the capture overlay (25) so a stacked open is
      // visually obvious; both modal-on-modal is allowed by the slot
      // system but the recall overlay should win focus when both are
      // mounted (which only happens transiently during hotkey races).
      priority: 26,
      component: RecallOverlay,
    })
  },
}
