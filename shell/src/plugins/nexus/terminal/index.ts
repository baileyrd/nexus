import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { viewRegistry, workspace } from '../../../workspace'
import { TerminalView } from './TerminalView'
import { terminalPaneViewCreator } from './TerminalPaneView'
import {
  useTerminalStore,
  type OutputStreamPayload,
  type RecoverFn,
} from './terminalStore'
import {
  SAVED_COMMANDS_VIEW_TYPE,
  savedCommandsPaneViewCreator,
} from './SavedCommandsPaneView'
import { SavedCommandsView } from './SavedCommandsView'
import { useSavedCommandsStore } from './savedCommandsStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'

const PLUGIN_ID = 'com.nexus.terminal'
const HANDLER_CREATE_SESSION = 'create_session'
const HANDLER_CLOSE_SESSION = 'close_session'
// WI-12: kernel publishes one event per session at this prefix; the
// session id is the suffix. Subscribing on the prefix gives us a
// single forwarder + one Tauri listener that fans out to all
// (current and future) sessions in the store.
const STREAM_TOPIC_PREFIX = 'com.nexus.terminal.output.'
// Lag-recovery handler — same as the pump path. Returns raw bytes
// past the supplied byte cursor; coordinate-compatible with the
// stream's per-session lastCursor.
const HANDLER_READ_RAW_SINCE = 'read_raw_since'
// Recovery snapshot deadline.
const TERMINAL_RECOVERY_TIMEOUT_MS = 250

const VIEW_ID = 'nexus.terminal.panelView'
const ACTIVITY_ITEM_ID = 'nexus.terminal.activityItem'

const COMMAND_TOGGLE = 'nexus.terminal.toggle'
const COMMAND_FOCUS = 'nexus.terminal.focus'
// Open the saved-commands sidebar.
const COMMAND_SAVED_SHOW = 'nexus.terminal.savedCommands.show'
// Open a new ad-hoc interactive terminal session.
const COMMAND_NEW = 'nexus.terminal.new'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'
const EVENT_TERMINAL_FOCUS = 'nexus.terminal:focus'

const CONTEXT_KEY_VISIBLE = 'nexus.terminal.visible'

// Lucide-style terminal glyph — chevron + underscore in a 24x24 box.
const TERMINAL_ICON_PATH = 'M4 17l6-6-6-6 M12 19h8'

interface CreateSessionResponse {
  id: string
}

export const terminalPlugin: Plugin = {
  manifest: {
    id: 'nexus.terminal',
    name: 'Terminal',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar'],
    contributes: {
      configuration: {
        pluginId: 'nexus.terminal',
        title: 'Terminal',
        order: 20,
        schema: [
          {
            key: 'ui.commandSaveNotificationMs',
            title: 'Command save notification duration',
            description: 'Auto-dismiss duration for "opening terminal" notifications in milliseconds',
            type: 'number' as const,
            default: 3000,
          },
          {
            key: 'ui.commandCopiedNotificationMs',
            title: 'Command sent notification duration',
            description: 'Auto-dismiss duration for "sent to terminal" notifications in milliseconds',
            type: 'number' as const,
            default: 1800,
          },
          {
            key: 'terminal.autoRestartDelayMs',
            title: 'Auto-restart delay',
            description: 'Delay in ms before a saved command auto-restarts.',
            type: 'number' as const,
            default: 2000,
          },
        ],
      },
      commands: [
        { id: COMMAND_TOGGLE, title: 'Toggle Terminal', category: 'Terminal' },
        { id: COMMAND_FOCUS, title: 'Focus Terminal', category: 'Terminal' },
        { id: COMMAND_NEW, title: 'New Terminal', category: 'Terminal' },
        {
          id: COMMAND_SAVED_SHOW,
          title: 'Show Saved Commands',
          category: 'Terminal',
        },
      ],
      keybindings: [
        { command: COMMAND_TOGGLE, key: 'ctrl+`', mac: 'cmd+`' },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_VISIBLE,
          description: 'True when the terminal panel is visible.',
          type: 'boolean',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    api.configuration.register(terminalPlugin.manifest.contributes!.configuration!)

    viewRegistry.register(
      'terminal',
      terminalPaneViewCreator(() =>
        createElement(TerminalView, { kernel: api.kernel, events: api.events }),
      ),
    )

    // ── WI-12 — event-driven output stream ──────────────────────────
    const recoverFn: RecoverFn = async (sessionId, lastCursor) => {
      try {
        const resp = await api.kernel.invoke<{
          cursor: number | string
          data: number[]
        }>(
          PLUGIN_ID,
          HANDLER_READ_RAW_SINCE,
          { id: sessionId, cursor: lastCursor, timeout_ms: TERMINAL_RECOVERY_TIMEOUT_MS },
        )
        const cursorN = Number(resp.cursor)
        return {
          cursor: Number.isFinite(cursorN) ? cursorN : lastCursor,
          data: new Uint8Array(resp.data),
        }
      } catch (err) {
        clientLogger.warn('[nexus.terminal] lag-recovery read_raw_since failed:', err)
        return null
      }
    }
    useTerminalStore.getState().setRecoverFn(recoverFn)

    let streamUnsub: (() => void) | null = null
    const subscribeStream = async () => {
      if (streamUnsub) return
      try {
        streamUnsub = await api.kernel.on<OutputStreamPayload>(
          STREAM_TOPIC_PREFIX,
          (topic, payload) => {
            const sessionId = topic.slice(STREAM_TOPIC_PREFIX.length)
            if (!sessionId) return
            useTerminalStore.getState().handleStreamChunk(sessionId, payload)
          },
        )
      } catch (err) {
        clientLogger.warn('[nexus.terminal] failed to subscribe to output stream:', err)
        streamUnsub = null
      }
    }
    const unsubscribeStream = () => {
      if (!streamUnsub) return
      try { streamUnsub() } catch (err) {
        clientLogger.warn('[nexus.terminal] stream unsubscribe failed:', err)
      }
      streamUnsub = null
    }

    // ── Session lifecycle ───────────────────────────────────────────
    //
    // Sessions are created on-demand:
    //   • "New Terminal" command / toggle with no sessions → ad-hoc shell
    //   • Saved-command row click → dedicated session for that command
    //     (managed by SavedCommandsView calling kernel.invoke directly)
    //
    // On workspace close we close every open session and reset the store.
    const createAdHocSession = async (): Promise<string | null> => {
      if (!(await api.kernel.available())) return null
      const workspaceRoot = useWorkspaceStore.getState().rootPath
      try {
        const resp = await api.kernel.invoke<CreateSessionResponse>(
          PLUGIN_ID,
          HANDLER_CREATE_SESSION,
          { working_dir: workspaceRoot ?? undefined, name: 'terminal' },
        )
        useTerminalStore.getState().addSession(resp.id, { name: 'terminal' })
        useTerminalStore.getState().setActiveSession(resp.id)
        clientLogger.info('[nexus.terminal] ad-hoc session created:', resp.id)
        return resp.id
      } catch (err) {
        clientLogger.warn('[nexus.terminal] create_session failed:', err)
        return null
      }
    }

    const destroyAllSessions = async (): Promise<void> => {
      const sessions = useTerminalStore.getState().sessions
      useTerminalStore.getState().resetSessions()
      for (const id of Object.keys(sessions)) {
        try {
          await api.kernel.invoke(PLUGIN_ID, HANDLER_CLOSE_SESSION, { id })
        } catch (err) {
          // Kernel may already be shutting down.
          clientLogger.info('[nexus.terminal] close_session skipped:', err)
        }
      }
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void subscribeStream()
      // Sessions are created on-demand; no auto-create here.
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      void destroyAllSessions()
      unsubscribeStream()
      useTerminalStore.getState().resetStreams()
      useTerminalStore.getState().setVisible(false)
      api.context.set(CONTEXT_KEY_VISIBLE, false)
    })

    // ── Commands ────────────────────────────────────────────────────
    //
    // `ensureAndReveal` shows the terminal panel. If no session exists
    // yet it creates an ad-hoc shell so there's always something to see.
    const ensureAndReveal = async () => {
      const leaf = await workspace.ensureLeafOfType('terminal', 'bottom')
      workspace.revealLeaf(leaf)
      useTerminalStore.getState().setVisible(true)
      api.context.set(CONTEXT_KEY_VISIBLE, true)

      // Create an ad-hoc session only when the pane would otherwise be
      // blank (no sessions open at all).
      if (Object.keys(useTerminalStore.getState().sessions).length === 0) {
        await createAdHocSession()
      }
      return leaf
    }

    api.commands.register(COMMAND_TOGGLE, async () => {
      const existing = workspace.getLeavesOfType('terminal')
      const bottomVisible = !workspace.bottomSplit.collapsed
      const activeId = workspace.activeLeafId
      const anyActive = existing.some((l) => l.id === activeId)
      if (existing.length > 0 && bottomVisible && anyActive) {
        workspace.setSidedockCollapsed('bottom', true)
        useTerminalStore.getState().setVisible(false)
        api.context.set(CONTEXT_KEY_VISIBLE, false)
        return
      }
      await ensureAndReveal()
      api.events.emit(EVENT_TERMINAL_FOCUS, {})
    })

    api.commands.register(COMMAND_FOCUS, async () => {
      await ensureAndReveal()
      api.events.emit(EVENT_TERMINAL_FOCUS, {})
    })

    // Open a fresh ad-hoc terminal regardless of how many sessions exist.
    api.commands.register(COMMAND_NEW, async () => {
      await createAdHocSession()
      await ensureAndReveal()
      api.events.emit(EVENT_TERMINAL_FOCUS, {})
    })

    // ── Saved Commands sub-view (WI-05) ─────────────────────────────
    viewRegistry.register(
      SAVED_COMMANDS_VIEW_TYPE,
      savedCommandsPaneViewCreator(() =>
        createElement(SavedCommandsView, {
          kernel: api.kernel,
          notifications: api.notifications,
          focusTerminal: () => {
            void ensureAndReveal().then(() => {
              api.events.emit(EVENT_TERMINAL_FOCUS, {})
            })
          },
          onNewTerminal: async () => {
            await createAdHocSession()
            await ensureAndReveal()
            api.events.emit(EVENT_TERMINAL_FOCUS, {})
          },
        }),
      ),
    )

    api.commands.register(COMMAND_SAVED_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType(
        SAVED_COMMANDS_VIEW_TYPE,
        'left',
      )
      workspace.revealLeaf(leaf)
      if (await api.kernel.available()) {
        void useSavedCommandsStore.getState().loadSaved(api.kernel)
      }
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useSavedCommandsStore.getState().reset()
    })

    // ── Activity bar item ───────────────────────────────────────────
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: TERMINAL_ICON_PATH,
      title: 'Terminal',
      viewId: VIEW_ID,
      priority: 40,
      command: COMMAND_TOGGLE,
    })

    // ── Boot-time reconciliation ────────────────────────────────────
    // Subscribe the stream forwarder if the kernel is already up.
    // Sessions are created on demand so we don't auto-create here.
    if (await api.kernel.available()) {
      void subscribeStream()
    }

    api.context.set(CONTEXT_KEY_VISIBLE, false)
  },
}
