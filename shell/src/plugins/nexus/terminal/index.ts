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
// Recovery snapshot deadline. Long enough to drain a sizeable backlog
// (~MB of stdout from a build) but well below the user-visible "the
// terminal froze" threshold.
const TERMINAL_RECOVERY_TIMEOUT_MS = 250

const VIEW_ID = 'nexus.terminal.panelView'
const ACTIVITY_ITEM_ID = 'nexus.terminal.activityItem'

const COMMAND_TOGGLE = 'nexus.terminal.toggle'
const COMMAND_FOCUS = 'nexus.terminal.focus'
// WI-05: dedicated command to reveal the Saved Commands sub-view.
// Listed in the command palette so the user can pull it up without
// hunting for an activity-bar entry.
const COMMAND_SAVED_SHOW = 'nexus.terminal.savedCommands.show'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'
const EVENT_TERMINAL_FOCUS = 'nexus.terminal:focus'

const CONTEXT_KEY_VISIBLE = 'nexus.terminal.visible'

// Lucide-style terminal glyph — chevron + underscore in a 24x24 box.
// Stroke-only path matches the iconPath contract used by
// nexus.files / nexus.search.
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
        {
          id: COMMAND_SAVED_SHOW,
          title: 'Show Saved Commands',
          category: 'Terminal',
        },
      ],
      keybindings: [
        // VS Code convention: Ctrl-Backquote toggles the integrated
        // terminal. The KeybindingRegistry matches by `e.key`, so
        // `'`'` is the literal backquote character produced by that
        // chord on both Windows and macOS default layouts.
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

    // Phase 7: legacy SlotRegistry slot:'panelArea' entry removed.
    // TerminalView now mounts exclusively through the Leaf/View pipeline.
    viewRegistry.register(
      'terminal',
      terminalPaneViewCreator(() =>
        createElement(TerminalView, {
          kernel: api.kernel,
          events: api.events,
          openExternal: (target) => api.platform.shell.openExternal(target),
        }),
      ),
    )

    // ── WI-12 — event-driven output stream ──────────────────────────
    //
    // The kernel publishes one custom event per chunk at
    // `com.nexus.terminal.output.<session_id>`; subscribing on the
    // prefix gives us a single forwarder that fans out to every
    // session in the store. The store handles routing, gap detection,
    // and recovery — see terminalStore.ts.
    //
    // Lag-recovery: when handleStreamChunk detects a seq gap it calls
    // back into `recoverFn` (registered below). recoverFn invokes
    // `read_raw_since` with the per-session byte cursor to backfill,
    // then the store re-baselines on the next stream chunk (option a
    // from the WI-12 brief).
    //
    // Cleanup: PluginRegistry tracks the unsub returned by
    // `api.kernel.on` and sweeps it on plugin unload, so we only
    // explicitly tear down on `workspace:closed` to drop the Rust
    // forwarder task during a kernel-shutdown window.
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
      try {
        streamUnsub()
      } catch (err) {
        clientLogger.warn('[nexus.terminal] stream unsubscribe failed:', err)
      }
      streamUnsub = null
    }

    // ── Session lifecycle ───────────────────────────────────────────
    //
    // Exactly one session per workspace for this first cut. Multi-
    // session tabs ship later. `create_session` with `working_dir =
    // workspace root` so shells open in the right place; omitting
    // `shell` lets the kernel pick the platform default (verified in
    // ServerSpawnConfig — None falls back to platform-default
    // detection).
    const ensureSession = async (): Promise<void> => {
      if (useTerminalStore.getState().sessionId !== null) return
      if (!(await api.kernel.available())) return
      const workspaceRoot = useWorkspaceStore.getState().rootPath
      try {
        const resp = await api.kernel.invoke<CreateSessionResponse>(
          PLUGIN_ID,
          HANDLER_CREATE_SESSION,
          {
            working_dir: workspaceRoot ?? undefined,
            name: 'terminal',
          },
        )
        useTerminalStore.getState().setSession(resp.id)
        clientLogger.info('[nexus.terminal] session created:', resp.id)
      } catch (err) {
        clientLogger.warn('[nexus.terminal] create_session failed:', err)
      }
    }

    const destroySession = async (): Promise<void> => {
      const id = useTerminalStore.getState().sessionId
      useTerminalStore.getState().setSession(null)
      if (id === null) return
      try {
        await api.kernel.invoke(PLUGIN_ID, HANDLER_CLOSE_SESSION, { id })
      } catch (err) {
        // Kernel may already be shutting down (workspace:closed path
        // tears it down before this handler runs). Not worth
        // surfacing.
        clientLogger.info('[nexus.terminal] close_session skipped:', err)
      }
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void subscribeStream()
      void ensureSession()
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      void destroySession()
      unsubscribeStream()
      useTerminalStore.getState().resetStreams()
      useTerminalStore.getState().setVisible(false)
      api.context.set(CONTEXT_KEY_VISIBLE, false)
    })

    // ── Commands ────────────────────────────────────────────────────
    //
    // Post-migration the terminal lives as a Leaf in the bottom drawer
    // (a tabbed pane, just like any other view). Focus = ensure + reveal
    // + emit the focus event the TerminalView subscribes to.
    //
    // Toggle behavior mirrors Obsidian / VS Code: if the terminal is
    // already visible, collapse the drawer — do NOT detach the leaf, so
    // the terminal session (PTY buffer, cursor, scrollback) survives
    // across toggles. Otherwise ensure + reveal, uncollapsing as needed.
    const ensureAndReveal = async () => {
      const leaf = await workspace.ensureLeafOfType('terminal', 'bottom')
      workspace.revealLeaf(leaf)
      useTerminalStore.getState().setVisible(true)
      api.context.set(CONTEXT_KEY_VISIBLE, true)
      void ensureSession()
      return leaf
    }

    api.commands.register(COMMAND_TOGGLE, async () => {
      const existing = workspace.getLeavesOfType('terminal')
      const bottomVisible = !workspace.bottomSplit.collapsed
      const activeId = workspace.activeLeafId
      const anyActive = existing.some((l) => l.id === activeId)
      if (existing.length > 0 && bottomVisible && anyActive) {
        // Collapse (preserve terminal state) rather than detach.
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

    // ── Saved Commands sub-view (WI-05) ─────────────────────────────
    //
    // Registered as a sidebar leaf rather than a slot inside
    // TerminalView so the user can keep the terminal output visible
    // while picking a command. Click-to-execute reads the active
    // sessionId out of `terminalStore` and sends `send_input`
    // (HANDLER_SEND_INPUT, kernel-side appends a newline). If no
    // session exists, the view falls through to `ensureAndReveal`
    // which creates one.
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
        }),
      ),
    )

    api.commands.register(COMMAND_SAVED_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType(
        SAVED_COMMANDS_VIEW_TYPE,
        'left',
      )
      workspace.revealLeaf(leaf)
      // Eagerly seed the cache so the view is non-empty on first open.
      // The view itself also calls loadSaved on mount, but doing it
      // here avoids the empty-flash between mount and first response.
      if (await api.kernel.available()) {
        void useSavedCommandsStore.getState().loadSaved(api.kernel)
      }
    })

    // Reset the saved-commands cache when the workspace closes so the
    // next workspace doesn't see stale rows from the previous forge's
    // procmgr_commands table.
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
    //
    // Mirror the nexus.files / nexus.gitStatus pattern: if the
    // kernel is already available by the time we activate (common on
    // a persisted-workspace boot where workspace:opened fires before
    // this plugin's listener is registered), ensure a session exists
    // now. We don't auto-show the panel — the user has to toggle it.
    if (await api.kernel.available()) {
      void subscribeStream()
      void ensureSession()
    }

    // Seed the context key so `when`-clauses can read it before the
    // first visibility flip.
    api.context.set(CONTEXT_KEY_VISIBLE, false)
  },
}
