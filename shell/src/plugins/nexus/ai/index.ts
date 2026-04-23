// shell/src/plugins/nexus/ai/index.ts
//
// WI-01 Slice A — plugin manifest + activation. Wires:
//
//   1. Kernel handle into the runtime module so submitQuestion /
//      hydrateConfig can call api.kernel.invoke.
//   2. AiConfig snapshot fetch (one-shot, on activate).
//   3. The single `com.nexus.ai.stream_*` prefix subscription that
//      routes chunks/done into the store. PluginRegistry sweeps the
//      disposer on plugin unload (commit c4d31d3) — we don't need
//      to track it manually.
//   4. View registration: viewType `ai-chat`, rendered by AiChatView
//      wrapping <ChatView/> with onSend/onCancel/onRetry bound to
//      the runtime functions.
//   5. Activity-bar item + focus/clear commands (preserved from the
//      prior skeleton — the chrome integration is unchanged).
//
// Slices B + C will extend the store + runtime; this manifest stays
// stable.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { ChatView } from './ChatView'
import { aiChatViewCreator } from './AiChatView'
import { useAiStore } from './aiStore'
import {
  setKernel,
  requestFocus,
  hydrateConfig,
  subscribeStream,
  submitQuestion,
  cancelInFlight,
  retryLast,
} from './aiRuntime'

const VIEW_ID = 'nexus.ai.view'
const COMMAND_FOCUS = 'nexus.ai.focus'
const COMMAND_CLEAR = 'nexus.ai.clear'

const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

// Lucide-style "sparkles" glyph — four-point star in a 24x24 box,
// stroke-only to match the iconPath contract used by the other
// activity-bar items.
const AI_ICON_PATH =
  'M12 3l2.4 5.2L20 10l-5.2 2.4L12 18l-2.4-5.6L4 10l5.6-1.8L12 3z'

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

  async activate(api: PluginAPI) {
    setKernel(api.kernel)

    // Bind runtime functions to this plugin's PluginAPI handle so the
    // view can fire them without re-importing the API. Closures keep
    // the wiring local to this file and out of the view component.
    const onSend = (q: string) => submitQuestion(api, q)
    const onCancel = () => cancelInFlight()
    const onRetry = () => retryLast(api)

    viewRegistry.register(
      'ai-chat',
      aiChatViewCreator(() =>
        createElement(ChatView, { onSend, onCancel, onRetry }),
      ),
    )

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
    // reveal it; the view's mount-time focuser drains pendingFocus.
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('ai-chat', 'main')
      workspace.revealLeaf(leaf)
      requestFocus()
    })

    // Clear command — wipe the current Q/A + composer state. Slice A
    // has no kernel-side conversation to delete; the runtime's
    // cancel hook also stops any in-flight stream from rendering.
    api.commands.register(COMMAND_CLEAR, () => {
      cancelInFlight()
      useAiStore.getState().reset()
    })

    // Wipe the store when the workspace closes. Answers from a
    // previous forge don't belong in a freshly opened one. Don't
    // tear down the subscription — PluginRegistry handles that on
    // plugin unload.
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      cancelInFlight()
      useAiStore.getState().reset()
      useAiStore.setState({ config: null })
    })

    // Fan out two awaits: subscription must be live before any
    // submit could fire (otherwise we'd miss the first chunks);
    // config hydration is best-effort and non-blocking for UX.
    await subscribeStream(api)
    void hydrateConfig(api)
  },

  // No deactivate — PluginRegistry.unregisterAll sweeps the kernel
  // subscription tracked by api.kernel.on (commit c4d31d3).
}
