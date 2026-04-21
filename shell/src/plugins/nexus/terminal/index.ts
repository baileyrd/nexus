import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { TerminalView } from './TerminalView'
import { terminalPaneViewCreator } from './TerminalPaneView'
import { useTerminalStore } from './terminalStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'

const PLUGIN_ID = 'com.nexus.terminal'
const HANDLER_CREATE_SESSION = 'create_session'
const HANDLER_CLOSE_SESSION = 'close_session'

const VIEW_ID = 'nexus.terminal.panelView'
const ACTIVITY_ITEM_ID = 'nexus.terminal.activityItem'

const COMMAND_TOGGLE = 'nexus.terminal.toggle'
const COMMAND_FOCUS = 'nexus.terminal.focus'

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
      commands: [
        { id: COMMAND_TOGGLE, title: 'Toggle Terminal', category: 'Terminal' },
        { id: COMMAND_FOCUS, title: 'Focus Terminal', category: 'Terminal' },
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
    // Phase 7: legacy SlotRegistry slot:'panelArea' entry removed.
    // TerminalView now mounts exclusively through the Leaf/View pipeline.
    viewRegistry.register(
      'terminal',
      terminalPaneViewCreator(() =>
        createElement(TerminalView, { kernel: api.kernel, events: api.events }),
      ),
    )

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
        console.info('[nexus.terminal] session created:', resp.id)
      } catch (err) {
        console.warn('[nexus.terminal] create_session failed:', err)
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
        console.info('[nexus.terminal] close_session skipped:', err)
      }
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void ensureSession()
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      void destroySession()
      useTerminalStore.getState().setVisible(false)
      api.context.set(CONTEXT_KEY_VISIBLE, false)
    })

    // ── Commands ────────────────────────────────────────────────────
    //
    // Post-migration the terminal lives as a Leaf in the right sidedock
    // (a tabbed pane, just like any other view). Toggle = detach if
    // present, otherwise ensure + reveal. Focus = ensure + reveal + emit
    // the focus event the TerminalView subscribes to.
    const ensureAndReveal = async () => {
      const leaf = await workspace.ensureLeafOfType('terminal', 'right')
      workspace.revealLeaf(leaf)
      useTerminalStore.getState().setVisible(true)
      api.context.set(CONTEXT_KEY_VISIBLE, true)
      void ensureSession()
      return leaf
    }

    api.commands.register(COMMAND_TOGGLE, async () => {
      const existing = workspace.getLeavesOfType('terminal')
      if (existing.length > 0) {
        for (const leaf of existing) workspace.detachLeaf(leaf)
        useTerminalStore.getState().setVisible(false)
        api.context.set(CONTEXT_KEY_VISIBLE, false)
        return
      }
      await ensureAndReveal()
    })

    api.commands.register(COMMAND_FOCUS, async () => {
      await ensureAndReveal()
      api.events.emit(EVENT_TERMINAL_FOCUS, {})
    })

    // ── Activity bar item ───────────────────────────────────────────
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: TERMINAL_ICON_PATH,
      title: 'Terminal',
      viewId: VIEW_ID,
      priority: 40,
      command: COMMAND_FOCUS,
    })

    // ── Boot-time reconciliation ────────────────────────────────────
    //
    // Mirror the nexus.files / nexus.gitStatus pattern: if the
    // kernel is already available by the time we activate (common on
    // a persisted-workspace boot where workspace:opened fires before
    // this plugin's listener is registered), ensure a session exists
    // now. We don't auto-show the panel — the user has to toggle it.
    if (await api.kernel.available()) {
      void ensureSession()
    }

    // Seed the context key so `when`-clauses can read it before the
    // first visibility flip.
    api.context.set(CONTEXT_KEY_VISIBLE, false)
  },
}
