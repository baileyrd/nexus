import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { workspace } from '../../../workspace'
import { TerminalTabsView } from './TerminalTabsView'
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
import {
  HISTORY_VIEW_TYPE,
  historyPaneViewCreator,
} from './HistoryPaneView'
import { HistoryView } from './HistoryView'
import { useHistoryStore } from './historyStore'
import {
  CROSS_SEARCH_VIEW_TYPE,
  crossSearchPaneViewCreator,
} from './CrossSearchPaneView'
import { CrossSearchView } from './CrossSearchView'
import { useWorkspaceStore } from '../workspace/workspaceStore'

const PLUGIN_ID = 'com.nexus.terminal'
const HANDLER_CREATE_SESSION = 'create_session'
const HANDLER_CLOSE_SESSION = 'close_session'
// Update a session's display label so list_sessions / AI consumers see
// the user's manual rename, not just the shell-local tab title.
const HANDLER_RENAME_SESSION = 'rename_session'
// WI-12: kernel publishes one event per session at this prefix; the
// session id is the suffix. Subscribing on the prefix gives us a
// single forwarder + one Tauri listener that fans out to all
// (current and future) sessions in the store.
const STREAM_TOPIC_PREFIX = 'com.nexus.terminal.output.'
// Lag-recovery handler — same as the pump path. Returns raw bytes
// past the supplied byte cursor; coordinate-compatible with the
// stream's per-session lastCursor.
const HANDLER_READ_RAW_SINCE = 'read_raw_since'
// #409 — per-session lifecycle events (session_closed,
// memory_limit_exceeded, soft_limit_exceeded, ...), keyed by session
// id suffix same as the output-stream prefix above. Previously this
// plugin subscribed only to `.output.`, so it never saw a kill/warn
// in real time — the only trace was the opt-in activity-timeline pane.
const EVENT_LIFECYCLE_PREFIX = 'com.nexus.terminal.events.'
// #409 — no per-sample RSS event exists (only threshold-crossing
// ones), so the tab-strip memory chip needs a periodic list_sessions
// poll to show a number for sessions that never cross a threshold.
// Gated on the panel being visible so a hidden terminal pane doesn't
// keep polling in the background.
const HANDLER_LIST_SESSIONS = 'list_sessions'
const RSS_POLL_INTERVAL_MS = 3000

interface TerminalLifecycleEvent {
  kind: string
  id: string
  rss_bytes?: number
  limit_mb?: number
}

interface SessionInfoResponse {
  id: string
  rss_bytes?: number
}
// Recovery snapshot deadline. Long enough to drain a sizeable backlog
// (~MB of stdout from a build) but well below the user-visible "the
// terminal froze" threshold.
const TERMINAL_RECOVERY_TIMEOUT_MS = 250

const VIEW_ID = 'nexus.terminal.panelView'
const ACTIVITY_ITEM_ID = 'nexus.terminal.activityItem'

const COMMAND_TOGGLE = 'nexus.terminal.toggle'
const COMMAND_FOCUS = 'nexus.terminal.focus'
// Multi-terminal tabs (Zed-style): spawn / close / cycle sessions.
const COMMAND_NEW_TAB = 'nexus.terminal.newTab'
const COMMAND_CLOSE_TAB = 'nexus.terminal.closeTab'
const COMMAND_NEXT_TAB = 'nexus.terminal.nextTab'
const COMMAND_PREV_TAB = 'nexus.terminal.prevTab'
// WI-05: dedicated command to reveal the Saved Commands sub-view.
// Listed in the command palette so the user can pull it up without
// hunting for an activity-bar entry.
const COMMAND_SAVED_SHOW = 'nexus.terminal.savedCommands.show'
// BL-060: dedicated command to reveal the Command History sub-view.
const COMMAND_HISTORY_SHOW = 'nexus.terminal.history.show'
// BL-063: cross-session scrollback search.
const COMMAND_CROSS_SEARCH_SHOW = 'nexus.terminal.crossSearch.show'

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
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'com.nexus.terminal'],
    contributes: {
      configuration: {
        pluginId: 'nexus.terminal',
        title: 'Terminal',
        order: 20,
        category: 'system',
        schema: [
          {
            key: 'terminal.fontSize',
            title: 'Font size',
            description: 'Terminal font size in pixels. Applies immediately to open terminals.',
            type: 'number' as const,
            default: 13,
          },
          {
            key: 'terminal.scrollback',
            title: 'Scrollback lines',
            description:
              'Lines of output history kept per terminal. Applies immediately to open terminals.',
            type: 'number' as const,
            default: 5000,
          },
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
          {
            // BL-059 follow-up — comma-separated priority list passed to
            // `com.nexus.terminal::open_in_terminal`. Earlier names win
            // when multiple emulators are installed. Leave blank for the
            // built-in cross-platform default. Valid tags (kebab-case
            // also accepted): iterm2, wezterm, ghostty, kitty,
            // alacritty, windows-terminal, gnome-terminal, konsole,
            // xfce4-terminal, mac-terminal, x-terminal-emulator, xterm.
            key: 'terminal.externalPriority',
            title: 'External terminal priority',
            description:
              'Comma-separated emulator preference for the "Open in external" action. Earlier entries win. Valid: iterm2, wezterm, ghostty, kitty, alacritty, windows-terminal, gnome-terminal, konsole, xfce4-terminal, mac-terminal, x-terminal-emulator, xterm. Leave blank for the built-in cross-platform default.',
            type: 'string' as const,
            default: '',
          },
        ],
      },
      commands: [
        { id: COMMAND_TOGGLE, title: 'Toggle Terminal', category: 'Terminal' },
        { id: COMMAND_FOCUS, title: 'Focus Terminal', category: 'Terminal' },
        { id: COMMAND_NEW_TAB, title: 'New Terminal', category: 'Terminal' },
        {
          id: COMMAND_CLOSE_TAB,
          title: 'Close Terminal',
          category: 'Terminal',
        },
        {
          id: COMMAND_NEXT_TAB,
          title: 'Next Terminal Tab',
          category: 'Terminal',
        },
        {
          id: COMMAND_PREV_TAB,
          title: 'Previous Terminal Tab',
          category: 'Terminal',
        },
        {
          id: COMMAND_SAVED_SHOW,
          title: 'Show Saved Commands',
          category: 'Terminal',
        },
        {
          id: COMMAND_HISTORY_SHOW,
          title: 'Show Command History',
          category: 'Terminal',
        },
        {
          id: COMMAND_CROSS_SEARCH_SHOW,
          title: 'Search All Sessions',
          category: 'Terminal',
        },
      ],
      keybindings: [
        // VS Code convention: Ctrl-Backquote toggles the integrated
        // terminal. The KeybindingRegistry matches by `e.key`, so
        // `'`'` is the literal backquote character produced by that
        // chord on both Windows and macOS default layouts.
        { command: COMMAND_TOGGLE, key: 'ctrl+`', mac: 'cmd+`' },
        // VS Code convention for spawning an additional integrated
        // terminal. Ctrl/Cmd-Shift-Backquote opens a fresh tab.
        { command: COMMAND_NEW_TAB, key: 'ctrl+shift+`', mac: 'cmd+shift+`' },
        // BL-063 — cross-session scrollback search. Originally bound
        // to ⌘⇧F to mirror VS Code's "find in files" — but BL-078
        // ships a workspace-wide find-in-files panel that's a much
        // closer match for that muscle memory. Moved here to ⌘⇧G
        // (terminal-only "find") so the file-search keybinding is
        // available for the workspace surface.
        {
          command: COMMAND_CROSS_SEARCH_SHOW,
          key: 'ctrl+shift+g',
          mac: 'cmd+shift+g',
        },
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

    // ── Session lifecycle ───────────────────────────────────────────
    //
    // Multi-terminal tabs (Zed-style): the panel hosts one leaf whose
    // view renders a tab strip plus one live xterm per session. Each
    // tab is backed by a distinct kernel session. `create_session` with
    // `working_dir = workspace root` so shells open in the right place;
    // omitting `shell` lets the kernel pick the platform default
    // (verified in ServerSpawnConfig — None falls back to
    // platform-default detection).
    //
    // Tabs are numbered sequentially for the life of the workspace; the
    // counter never reuses a number even after a tab closes, matching
    // the "Terminal 1 / 2 / 3 …" labelling users expect.
    let tabCounter = 0

    // ── Tab persistence ─────────────────────────────────────────────
    //
    // Terminal sessions themselves are ephemeral (a fresh PTY + id is
    // minted every boot), but the user's tab layout — how many tabs and
    // any names they pinned — is worth restoring. We persist an ordered
    // list of `{ title, custom }` keyed by vault root (one workspace's
    // tabs shouldn't leak into another) via the plugin's localStorage-
    // backed `api.storage`. On open we recreate that many sessions and
    // re-apply each saved title; auto-named tabs re-derive their label
    // from the new shell's OSC/cwd, while pinned ones keep the saved
    // name. We never persist an empty list, so the workspace:closed
    // teardown (which clears the store) can't wipe saved state.
    interface PersistedTab {
      title: string
      custom: boolean
    }
    const tabsStorageKey = (): string | null => {
      const root = useWorkspaceStore.getState().rootPath
      return root ? `tabs:${root}` : null
    }
    const persistTabs = (): void => {
      const key = tabsStorageKey()
      if (!key) return
      const tabs = useTerminalStore.getState().tabs
      if (tabs.length === 0) return
      const payload: PersistedTab[] = tabs.map((t) => ({
        title: t.title,
        custom: t.custom,
      }))
      try {
        api.storage.set(key, JSON.stringify(payload))
      } catch (err) {
        clientLogger.warn('[nexus.terminal] persistTabs failed:', err)
      }
    }
    const loadPersistedTabs = (): PersistedTab[] => {
      const key = tabsStorageKey()
      if (!key) return []
      const raw = api.storage.get(key)
      if (!raw) return []
      try {
        const parsed: unknown = JSON.parse(raw)
        if (!Array.isArray(parsed)) return []
        return parsed
          .filter(
            (e): e is PersistedTab =>
              typeof e === 'object' &&
              e !== null &&
              typeof (e as PersistedTab).title === 'string' &&
              typeof (e as PersistedTab).custom === 'boolean',
          )
          .map((e) => ({ title: e.title, custom: e.custom }))
      } catch {
        return []
      }
    }

    /**
     * Spawn one kernel session + tab. `seed` supplies the initial label
     * and pin state when restoring; for a fresh tab we derive an
     * auto-title from the workspace folder name (the cwd fallback before
     * the shell emits its own OSC title), or `Terminal N` when there's
     * no workspace root.
     */
    const createTerminal = async (
      seed?: PersistedTab,
    ): Promise<string | null> => {
      if (!(await api.kernel.available())) return null
      const workspaceRoot = useWorkspaceStore.getState().rootPath
      const folder = workspaceRoot
        ? workspaceRoot.replace(/[/\\]+$/, '').split(/[/\\]/).pop() ?? ''
        : ''
      const title =
        seed?.title ?? (folder.length > 0 ? folder : `Terminal ${++tabCounter}`)
      const custom = seed?.custom ?? false
      try {
        const resp = await api.kernel.invoke<CreateSessionResponse>(
          PLUGIN_ID,
          HANDLER_CREATE_SESSION,
          {
            working_dir: workspaceRoot ?? undefined,
            name: title,
          },
        )
        useTerminalStore.getState().addTab({ id: resp.id, title, custom })
        persistTabs()
        clientLogger.info('[nexus.terminal] session created:', resp.id)
        return resp.id
      } catch (err) {
        clientLogger.warn('[nexus.terminal] create_session failed:', err)
        return null
      }
    }

    // Ensure terminal tabs exist (boot / first reveal): restore the
    // persisted layout when present, else open a single default tab.
    const ensureSession = async (): Promise<void> => {
      if (useTerminalStore.getState().tabs.length > 0) return
      const saved = loadPersistedTabs()
      if (saved.length > 0) {
        for (const seed of saved) {
          await createTerminal(seed)
        }
        return
      }
      await createTerminal()
    }

    // Push a manual rename to the kernel session label (so list_sessions
    // / AI consumers see it too) and pin it in the store. The store
    // update is synchronous so the tab relabels immediately; the IPC
    // call is best-effort.
    const renameTerminal = (id: string, title: string): void => {
      useTerminalStore.getState().renameTab(id, title)
      persistTabs()
      void api.kernel
        .invoke(PLUGIN_ID, HANDLER_RENAME_SESSION, { id, name: title })
        .catch((err) => {
          clientLogger.info('[nexus.terminal] rename_session skipped:', err)
        })
    }

    const closeTerminal = async (id: string): Promise<void> => {
      // Drop the tab first so the UI updates immediately; the kernel
      // close is best-effort (the PTY may already be gone).
      useTerminalStore.getState().removeTab(id)
      persistTabs()
      try {
        await api.kernel.invoke(PLUGIN_ID, HANDLER_CLOSE_SESSION, { id })
      } catch (err) {
        clientLogger.info('[nexus.terminal] close_session skipped:', err)
      }
    }

    const destroyAllSessions = async (): Promise<void> => {
      const ids = useTerminalStore.getState().tabs.map((t) => t.id)
      useTerminalStore.getState().setActiveSession(null)
      for (const id of ids) {
        try {
          await api.kernel.invoke(PLUGIN_ID, HANDLER_CLOSE_SESSION, { id })
        } catch (err) {
          // Kernel may already be shutting down (workspace:closed path
          // tears it down before this handler runs). Not worth
          // surfacing.
          clientLogger.info('[nexus.terminal] close_session skipped:', err)
        }
      }
    }

    // Phase 7: legacy SlotRegistry slot:'panelArea' entry removed.
    // The terminal mounts exclusively through the Leaf/View pipeline.
    api.viewRegistry.register(
      'terminal',
      terminalPaneViewCreator(() =>
        createElement(TerminalTabsView, {
          kernel: api.kernel,
          events: api.events,
          openExternal: (target) => api.platform.shell.openExternal(target),
          onNewTab: () => {
            void createTerminal().then(() => {
              api.events.emit(EVENT_TERMINAL_FOCUS, {})
            })
          },
          onCloseTab: (id: string) => {
            void closeTerminal(id)
          },
          onRenameTab: (id: string, title: string) => {
            renameTerminal(id, title)
          },
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

    // Persist auto-title changes. createTerminal / renameTerminal /
    // closeTerminal persist their own mutations synchronously; this
    // subscription catches the remaining path — `applyAutoTitle` fired
    // from TerminalInstance when the shell emits an OSC title or cwd.
    // We diff a title+pin signature so active-tab switches (which don't
    // change labels) don't trigger redundant writes.
    let lastTabSig = ''
    useTerminalStore.subscribe((state) => {
      const sig = state.tabs.map((t) => `${t.title} ${t.custom}`).join('')
      if (sig === lastTabSig) return
      lastTabSig = sig
      persistTabs()
    })

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

    // #409 — react to kill/warn in real time instead of only via the
    // opt-in activity-timeline pane.
    let lifecycleUnsub: (() => void) | null = null
    const subscribeLifecycle = async () => {
      if (lifecycleUnsub) return
      try {
        lifecycleUnsub = await api.kernel.on<TerminalLifecycleEvent>(
          EVENT_LIFECYCLE_PREFIX,
          (_topic, payload) => {
            if (typeof payload.rss_bytes === 'number') {
              useTerminalStore.getState().setRssBytes(payload.id, payload.rss_bytes)
            }
            if (payload.kind === 'memory_limit_exceeded') {
              api.notifications.show({
                type: 'error',
                message: `Terminal session killed: memory exceeded ${payload.limit_mb}MB`,
              })
            } else if (payload.kind === 'soft_limit_exceeded') {
              api.notifications.show({
                type: 'warning',
                message: `Terminal session approaching memory limit (${payload.limit_mb}MB)`,
              })
            }
          },
        )
      } catch (err) {
        clientLogger.warn('[nexus.terminal] failed to subscribe to lifecycle events:', err)
        lifecycleUnsub = null
      }
    }
    const unsubscribeLifecycle = () => {
      if (!lifecycleUnsub) return
      try {
        lifecycleUnsub()
      } catch (err) {
        clientLogger.warn('[nexus.terminal] lifecycle unsubscribe failed:', err)
      }
      lifecycleUnsub = null
    }

    // #409 — periodic RSS poll for the tab-strip memory chip. Only
    // threshold-crossing events exist on the bus (no per-sample
    // event), so most sessions (never crossing the soft limit) would
    // otherwise show no chip at all. Gated on panel visibility so a
    // hidden terminal doesn't poll in the background.
    let rssPollTimer: ReturnType<typeof setInterval> | null = null
    const pollRss = async () => {
      if (!useTerminalStore.getState().visible) return
      if (useTerminalStore.getState().tabs.length === 0) return
      try {
        const sessions = await api.kernel.invoke<SessionInfoResponse[]>(
          PLUGIN_ID,
          HANDLER_LIST_SESSIONS,
        )
        for (const session of sessions) {
          if (typeof session.rss_bytes === 'number') {
            useTerminalStore.getState().setRssBytes(session.id, session.rss_bytes)
          }
        }
      } catch (err) {
        clientLogger.warn('[nexus.terminal] list_sessions poll failed:', err)
      }
    }
    const startRssPoll = () => {
      if (rssPollTimer) return
      rssPollTimer = setInterval(() => void pollRss(), RSS_POLL_INTERVAL_MS)
    }
    const stopRssPoll = () => {
      if (!rssPollTimer) return
      clearInterval(rssPollTimer)
      rssPollTimer = null
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void subscribeStream()
      void subscribeLifecycle()
      startRssPoll()
      void ensureSession()
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      void destroyAllSessions()
      unsubscribeStream()
      unsubscribeLifecycle()
      stopRssPoll()
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

    // ── Multi-terminal tab commands ─────────────────────────────────
    //
    // New = reveal the panel and spawn a fresh session regardless of
    // how many are already open (unlike Focus, which only ensures one
    // exists). Close = drop the active tab. Next/Prev cycle the active
    // tab with wrap-around.
    api.commands.register(COMMAND_NEW_TAB, async () => {
      const leaf = await workspace.ensureLeafOfType('terminal', 'bottom')
      workspace.revealLeaf(leaf)
      useTerminalStore.getState().setVisible(true)
      api.context.set(CONTEXT_KEY_VISIBLE, true)
      await createTerminal()
      api.events.emit(EVENT_TERMINAL_FOCUS, {})
    })

    api.commands.register(COMMAND_CLOSE_TAB, async () => {
      const id = useTerminalStore.getState().activeSessionId
      if (id) await closeTerminal(id)
    })

    const cycleTab = (delta: number) => {
      const { tabs, activeSessionId, setActiveSession } =
        useTerminalStore.getState()
      if (tabs.length < 2) return
      const idx = tabs.findIndex((t) => t.id === activeSessionId)
      if (idx === -1) return
      const next = (idx + delta + tabs.length) % tabs.length
      setActiveSession(tabs[next].id)
      api.events.emit(EVENT_TERMINAL_FOCUS, {})
    }
    api.commands.register(COMMAND_NEXT_TAB, () => cycleTab(1))
    api.commands.register(COMMAND_PREV_TAB, () => cycleTab(-1))

    // ── Saved Commands sub-view (WI-05) ─────────────────────────────
    //
    // Registered as a sidebar leaf rather than a slot inside
    // TerminalView so the user can keep the terminal output visible
    // while picking a command. Click-to-execute reads the active
    // sessionId out of `terminalStore` and sends `send_input`
    // (HANDLER_SEND_INPUT, kernel-side appends a newline). If no
    // session exists, the view falls through to `ensureAndReveal`
    // which creates one.
    api.viewRegistry.register(
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

    // ── Command History sub-view (BL-060) ───────────────────────────
    //
    // Sibling to Saved Commands. Lives in the same sidebar slot but as
    // its own leaf type so the user can have one or both visible at a
    // time. Re-run / promote / delete are wired through the
    // `com.nexus.terminal::adhoc_*` handlers; promote-to-saved updates
    // both stores.
    api.viewRegistry.register(
      HISTORY_VIEW_TYPE,
      historyPaneViewCreator(() =>
        createElement(HistoryView, {
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

    api.commands.register(COMMAND_HISTORY_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType(HISTORY_VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
      if (await api.kernel.available()) {
        void useHistoryStore.getState().loadHistory(api.kernel)
      }
    })

    // ── Cross-session scrollback search (BL-063) ─────────────────────
    //
    // Sibling sidebar leaf to Saved Commands / History. The view
    // owns its own debounced search input — index.ts only handles
    // registration + reveal.
    api.viewRegistry.register(
      CROSS_SEARCH_VIEW_TYPE,
      crossSearchPaneViewCreator(() =>
        createElement(CrossSearchView, {
          kernel: api.kernel,
          notifications: api.notifications,
        }),
      ),
    )

    api.commands.register(COMMAND_CROSS_SEARCH_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType(CROSS_SEARCH_VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
    })

    // Reset the saved-commands cache when the workspace closes so the
    // next workspace doesn't see stale rows from the previous forge's
    // procmgr_commands / procmgr_adhoc_history tables.
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useSavedCommandsStore.getState().reset()
      useHistoryStore.getState().reset()
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
