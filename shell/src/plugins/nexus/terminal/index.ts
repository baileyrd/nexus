import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry } from '../../../workspace'
import { TerminalView } from './TerminalView'
import { terminalPaneViewCreator } from './TerminalPaneView'
import { useTerminalStore } from './terminalStore'
import { useLayoutStore } from '../../../stores/layoutStore'
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
const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
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
    // ── View registration ───────────────────────────────────────────
    //
    // The panelArea slot already exists in SlotRegistry. Wrap
    // TerminalView so it receives the kernel/events refs — it owns a
    // plain xterm instance that needs to invoke the kernel directly
    // on every keystroke and poll tick, so passing the API through
    // props keeps the component testable without pulling in the
    // whole plugin API singleton.
    api.views.register(VIEW_ID, {
      slot: 'panelArea',
      component: () =>
        createElement(TerminalView, { kernel: api.kernel, events: api.events }),
      priority: 10,
    })

    // Phase 5 workspace-View registration (leaf-migration-plan §Phase 5).
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
      // Drop the panel on workspace close. Reopening the workspace
      // will not auto-show the terminal — the user toggles it back.
      useLayoutStore.setState((s) => ({
        panelArea: { ...s.panelArea, visible: false },
      }))
      useTerminalStore.getState().setVisible(false)
      api.context.set(CONTEXT_KEY_VISIBLE, false)
    })

    // ── Visibility plumbing ─────────────────────────────────────────
    //
    // panelArea lives outside the sidebar, so the activity-bar's
    // default `sidebar:showView` emission isn't useful to us. We
    // instead listen for `activityBar:activeChanged` — when the
    // terminal item becomes active, show panelArea + ensure a
    // session exists. We deliberately DO NOT auto-hide panelArea
    // when the activity bar switches away (the user can have the
    // terminal open alongside a sidebar view). They close it via the
    // keybinding or by clicking the activity-bar item again (which
    // toggles it off in activityBar's own click handler).
    //
    // The activityBar plugin will also emit `sidebar:showView` with
    // our viewId. The sidebar plugin ignores unknown viewIds (no
    // sidebarContent slot registered for nexus.terminal.panelView),
    // so this is harmless.
    const setVisible = (visible: boolean) => {
      useLayoutStore.setState((s) => ({
        panelArea: { ...s.panelArea, visible },
      }))
      useTerminalStore.getState().setVisible(visible)
      api.context.set(CONTEXT_KEY_VISIBLE, visible)
    }

    api.events.on<{ viewId: string | null }>(
      EVENT_ACTIVITY_BAR_ACTIVE_CHANGED,
      ({ viewId }) => {
        if (viewId === VIEW_ID) {
          setVisible(true)
          void ensureSession()
        }
      },
    )

    // ── Commands ────────────────────────────────────────────────────
    api.commands.register(COMMAND_TOGGLE, () => {
      const currentlyVisible = useLayoutStore.getState().panelArea.visible
      const next = !currentlyVisible
      setVisible(next)
      if (next) {
        void ensureSession()
      }
    })

    api.commands.register(COMMAND_FOCUS, () => {
      setVisible(true)
      void ensureSession()
      // TerminalView subscribes to this event and calls term.focus()
      // on the embedded xterm instance.
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
