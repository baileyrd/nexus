// shell/src/plugins/nexus/quickSwitcher/index.ts
//
// C5 (#358) — file quick-switcher (Ctrl+P), reclaiming the alias binding
// the command palette carved out for exactly this feature ("to be
// reclaimed when a file quick-open plugin lands"). Seeded with per-forge
// recent files persisted through the existing settings pipeline
// (api.configuration — the same `settings_read`/`settings_write` round
// trip every other settings-backed plugin uses), fuzzy-matched against
// the whole forge via `query_files`, with create-on-Enter for a query
// that matches nothing.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { QuickSwitcher } from './QuickSwitcher'
import { useQuickSwitcherStore } from './quickSwitcherStore'
import { setApi } from './quickSwitcherRuntime'

const VIEW_ID = 'nexus.quickSwitcher.overlay'

const COMMAND_OPEN = 'nexus.quickSwitcher.open'
const COMMAND_CLOSE = 'nexus.quickSwitcher.close'
const CONTEXT_KEY_VISIBLE = 'nexus.quickSwitcher.visible'

export const quickSwitcherPlugin: Plugin = {
  manifest: {
    id: 'nexus.quickSwitcher',
    name: 'Quick Switcher',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['com.nexus.storage'],
    contributes: {
      commands: [
        { id: COMMAND_OPEN, title: 'Go to File…', category: 'Quick Switcher' },
        { id: COMMAND_CLOSE, title: 'Close Quick Switcher', category: 'Quick Switcher' },
      ],
      keybindings: [
        // C5 — reclaimed from commandPalette's alias (see
        // commandPalette/index.ts's keybindings list).
        { command: COMMAND_OPEN, key: 'ctrl+p', mac: 'cmd+p' },
        { command: COMMAND_CLOSE, key: 'escape', when: CONTEXT_KEY_VISIBLE },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_VISIBLE,
          description: 'True while the file quick-switcher is open.',
          type: 'boolean',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    setApi(api)
    api.commands.register(COMMAND_OPEN, () => {
      useQuickSwitcherStore.getState().open()
    })
    api.commands.register(COMMAND_CLOSE, () => {
      useQuickSwitcherStore.getState().close()
    })

    api.context.set(CONTEXT_KEY_VISIBLE, useQuickSwitcherStore.getState().visible)
    useQuickSwitcherStore.subscribe((state, prev) => {
      if (state.visible !== prev.visible) api.context.set(CONTEXT_KEY_VISIBLE, state.visible)
    })

    api.views.register(VIEW_ID, {
      slot: 'overlay',
      component: () => createElement(QuickSwitcher),
      priority: 10,
    })
  },
}
