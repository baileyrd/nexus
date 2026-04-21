import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { ChatView } from './ChatView'
import { aiChatViewCreator } from './AiChatView'
import { useAiStore } from './aiStore'
import { setKernel, requestFocus } from './aiRuntime'

const VIEW_ID = 'nexus.ai.view'
const COMMAND_FOCUS = 'nexus.ai.focus'
const COMMAND_CLEAR = 'nexus.ai.clear'

const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

// Lucide-style "sparkles" glyph — four-point star in a 24x24 box,
// stroke-only to match the iconPath contract used by the other
// activity-bar items.
const AI_ICON_PATH = 'M12 3l2.4 5.2L20 10l-5.2 2.4L12 18l-2.4-5.6L4 10l5.6-1.8L12 3z'

export const aiPlugin: Plugin = {
  manifest: {
    id: 'nexus.ai',
    name: 'AI Chat',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Chat', category: 'AI' },
        { id: COMMAND_CLEAR, title: 'Clear Chat', category: 'AI' },
      ],
      keybindings: [
        { command: COMMAND_FOCUS, key: 'ctrl+alt+a', mac: 'cmd+alt+a' },
      ],
    },
  },

  activate(api: PluginAPI) {
    setKernel(api.kernel)

    // Phase 7: legacy SlotRegistry slot:'sidebarContent' entry removed.
    viewRegistry.register('ai-chat', aiChatViewCreator(() => createElement(ChatView)))

    api.activityBar.addItem({
      id: 'nexus.ai.activityItem',
      icon: '',
      iconPath: AI_ICON_PATH,
      title: 'AI Chat',
      viewId: VIEW_ID,
      priority: 50,
      command: COMMAND_FOCUS,
    })

    // Focus command — ensure an ai-chat leaf exists on the right and
    // reveal it; focuser drains on mount.
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('ai-chat', 'right')
      workspace.revealLeaf(leaf)
      requestFocus()
    })

    // Clear command — drop all messages + composer state. There's no
    // kernel-side session to delete: com.nexus.ai::ask is stateless
    // RAG (no server-held conversation context in v1).
    api.commands.register(COMMAND_CLEAR, () => {
      useAiStore.getState().clear()
    })

    // Wipe the store when the workspace closes. Messages from a
    // previous forge don't belong in a freshly opened one.
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useAiStore.getState().clear()
    })
  },
}
