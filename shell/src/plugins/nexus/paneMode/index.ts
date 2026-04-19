import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'

const COMMAND_ENTER = 'nexus.paneMode.enter'
const COMMAND_EXIT = 'nexus.paneMode.exit'
const CONTEXT_KEY_ACTIVE = 'nexus.paneMode.active'
const CONTEXT_KEY_ACTIVE_VIEW_ID = 'nexus.paneMode.activeViewId'

export const paneModePlugin: Plugin = {
  manifest: {
    id: 'nexus.paneMode',
    name: 'Pane Mode',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: COMMAND_ENTER, title: 'Enter Pane Mode', category: 'View' },
        { id: COMMAND_EXIT, title: 'Exit Pane Mode', category: 'View' },
      ],
      keybindings: [
        // Escape exits pane mode — but only if the command palette is
        // not visible. The palette has its own escape handler and must
        // win when both are active.
        {
          command: COMMAND_EXIT,
          key: 'escape',
          when: 'nexus.paneMode.active && !nexus.commandPalette.visible',
        },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_ACTIVE,
          description:
            'True when a plugin has taken over the body via pane mode.',
          type: 'boolean',
        },
        {
          key: CONTEXT_KEY_ACTIVE_VIEW_ID,
          description: 'ViewId of the current pane-mode view, or empty.',
          type: 'string',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    api.commands.register(COMMAND_ENTER, async (viewId?: unknown) => {
      if (typeof viewId !== 'string' || viewId.length === 0) {
        console.warn(
          '[nexus.paneMode] enter called without a viewId string arg; ignoring.',
        )
        return
      }
      usePaneModeStore.getState().enter(viewId)
    })

    api.commands.register(COMMAND_EXIT, async () => {
      usePaneModeStore.getState().exit()
    })

    // Seed context keys to current store state.
    const seed = usePaneModeStore.getState().activeViewId
    api.context.set(CONTEXT_KEY_ACTIVE, seed !== null)
    api.context.set(CONTEXT_KEY_ACTIVE_VIEW_ID, seed ?? '')

    // Keep context keys in sync on store transitions. Only re-publish
    // when the relevant derived value changes, to avoid spurious churn.
    usePaneModeStore.subscribe((state, prev) => {
      const active = state.activeViewId !== null
      const prevActive = prev.activeViewId !== null
      if (active !== prevActive) {
        api.context.set(CONTEXT_KEY_ACTIVE, active)
      }
      if (state.activeViewId !== prev.activeViewId) {
        api.context.set(CONTEXT_KEY_ACTIVE_VIEW_ID, state.activeViewId ?? '')
      }
    })
  },
}
