import type { Plugin, PluginAPI } from '../../../types/plugin'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { ProcessesView } from './ProcessesView'
import {
  useProcessesStore,
  type PluginItem,
  type SessionItem,
} from './processesStore'

const PLUGIN_ID = 'nexus.processes'
const VIEW_ID = 'nexus.processes.view'
const ACTIVITY_ITEM_ID = 'nexus.processes.activityItem'
const COMMAND_SHOW = 'nexus.processes.show'

const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const SERVICE_PLUGIN_LIST = 'pluginList'
const SERVICE_COMMUNITY_MANIFESTS = 'communityPluginManifests'

const TERMINAL_PLUGIN_ID = 'com.nexus.terminal'
const MCP_HOST_PLUGIN_ID = 'com.nexus.mcp.host'

/**
 * Topic prefixes this pane subscribes to. Each prefix ends in a dot so
 * `CustomPrefix` on the kernel side matches every sub-topic — e.g.
 * `com.nexus.storage.file_modified`, `com.nexus.storage.file_created`,
 * etc. Some of these families (notably `terminal`) don't publish today
 * and contribute nothing to the feed until their core plugins start
 * emitting; subscribing now is a cheap no-op and future-proofs the
 * pane when they come online.
 */
const TOPIC_PREFIXES: readonly string[] = [
  'com.nexus.storage.',
  'com.nexus.git.',
  'com.nexus.terminal.',
  'com.nexus.workflow.',
  'com.nexus.ai.',
  'com.nexus.theme.',
  'com.nexus.mcp.',
  'com.nexus.skills.',
  'com.nexus.agent.',
]

/** Shape registered onto PluginRegistry by main.tsx — kept local to avoid a circular import. */
interface RegistryPluginEntry {
  id: string
  name: string
  version: string
  core: boolean
  state: string
  error?: string
}

// ── Kernel plugin readers ──────────────────────────────────────────────

function readShellPlugins(api: PluginAPI): PluginItem[] {
  const internal = api.internal
  if (!internal) return []

  const out: PluginItem[] = []

  try {
    const raw = internal.getInternalService<RegistryPluginEntry[]>(SERVICE_PLUGIN_LIST)
    for (const p of raw) {
      out.push({
        id: p.id,
        name: p.name,
        version: p.version,
        // The service doesn't distinguish core vs built-in beyond the
        // boolean flag; treat all shell-side plugins as 'builtin' for
        // the UI tag. 'kernel' is reserved for a future kernel-side
        // IPC surface.
        source: 'builtin',
        state: p.state,
        error: p.error,
      })
    }
  } catch (err) {
    // Seed call may run before main.tsx registers the service. Silent —
    // the store is re-seeded when the pane is opened, by which time
    // the service is guaranteed to exist.
    console.debug('[nexus.processes] pluginList service not yet available:', err)
  }

  try {
    const raw = internal.getInternalService<CommunityPluginManifest[]>(
      SERVICE_COMMUNITY_MANIFESTS,
    )
    for (const m of raw) {
      out.push({
        id: m.id,
        name: m.name,
        version: m.version,
        source: 'community',
        state: m.enabled ? 'active' : 'disabled',
      })
    }
  } catch (err) {
    console.debug('[nexus.processes] communityPluginManifests service not yet available:', err)
  }

  return out
}

interface TerminalSessionInfo {
  id: string
  name: string
  shell: string
  working_dir?: string | null
  line_count: number
  created_at: number
}

interface McpServerRow {
  name: string
  command: string
  args?: unknown
  disabled: boolean
}

async function readSessions(api: PluginAPI): Promise<SessionItem[]> {
  const out: SessionItem[] = []

  // Terminal PTY sessions. `list_sessions` is a synchronous kernel
  // handler (HANDLER_LIST_SESSIONS) that returns SessionInfo[].
  try {
    const sessions = await api.kernel.invoke<TerminalSessionInfo[]>(
      TERMINAL_PLUGIN_ID,
      'list_sessions',
    )
    for (const s of sessions) {
      out.push({
        id: s.id,
        kind: 'terminal',
        label: s.name || s.id,
        detail: s.shell.split(/[\\/]/).pop() || s.shell,
      })
    }
  } catch (err) {
    console.debug('[nexus.processes] list_sessions failed:', err)
  }

  // MCP server entries. `list_servers` returns the configured servers
  // from `mcp.toml`; each row is static config rather than a live
  // connection, but it's the best kernel-visible surface today.
  try {
    const servers = await api.kernel.invoke<McpServerRow[]>(
      MCP_HOST_PLUGIN_ID,
      'list_servers',
    )
    for (const s of servers) {
      if (s.disabled) continue
      out.push({
        id: s.name,
        kind: 'mcp',
        label: s.name,
        detail: s.command,
      })
    }
  } catch (err) {
    console.debug('[nexus.processes] list_servers failed:', err)
  }

  return out
}

// ── Plugin ─────────────────────────────────────────────────────────────

export const processesPlugin: Plugin = {
  manifest: {
    // core:true so we can reach api.internal.getInternalService to read
    // pluginList / communityPluginManifests. Same pattern as
    // nexus.pluginsMgmt — the flag is about internal-API access.
    id: PLUGIN_ID,
    name: 'Processes',
    version: '0.1.0',
    core: true,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.paneMode', 'nexus.activityBar'],
    contributes: {
      commands: [
        { id: COMMAND_SHOW, title: 'Show Processes', category: 'View' },
      ],
      keybindings: [
        // No collision with existing bindings: commandPalette uses
        // ctrl+shift+p, pluginsMgmt uses ctrl+shift+x, search uses
        // ctrl+shift+f.
        { command: COMMAND_SHOW, key: 'ctrl+shift+y', mac: 'cmd+shift+y' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    // ── Command ───────────────────────────────────────────────────────
    api.commands.register(COMMAND_SHOW, async () => {
      // Refresh plugin + session snapshots right before pane open so
      // the user sees the latest state every time they enter the view.
      useProcessesStore.getState().setPlugins(readShellPlugins(api))
      const sessions = await readSessions(api)
      useProcessesStore.getState().setSessions(sessions)
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })

    // ── View registration ─────────────────────────────────────────────
    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: ProcessesView,
      priority: 10,
    })

    // ── Activity-bar item ─────────────────────────────────────────────
    // Priority 60 sits after files(10) / search(20) / terminal(40) /
    // ai(50) and before a (hypothetical) settings item. The viewId
    // matches the paneMode slot entry id so the
    // `activityBar:activeChanged` listener below can route the user in.
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconName: 'grid',
      title: 'Processes',
      viewId: VIEW_ID,
      priority: 60,
    })

    // ── Activity-bar routing ──────────────────────────────────────────
    //
    // Sidebar-based plugins route through their focus command (which
    // calls `workspace.ensureLeafOfType + revealLeaf`). Pane-mode
    // plugins intercept `activityBar:activeChanged` instead:
    //
    //   • new viewId matches ours → enter pane mode on our view.
    //   • new viewId is anything else and WE are currently the
    //     pane-mode view → exit so the sidebar plugin takes over. Do
    //     not exit unconditionally — another pane-mode plugin may own
    //     the transition and we'd clobber its entry.
    //   • new viewId is null (toggle off) → exit only if we're active.
    api.events.on<{ viewId: string | null }>(
      EVENT_ACTIVITY_BAR_ACTIVE_CHANGED,
      ({ viewId }) => {
        if (viewId === VIEW_ID) {
          // Freshen before we open — matches the COMMAND_SHOW path.
          useProcessesStore.getState().setPlugins(readShellPlugins(api))
          void readSessions(api).then((sessions) => {
            useProcessesStore.getState().setSessions(sessions)
          })
          void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
        } else {
          const current = usePaneModeStore.getState().activeViewId
          if (current === VIEW_ID) {
            void api.commands.execute(COMMAND_PANE_MODE_EXIT)
          }
        }
      },
    )

    // ── Kernel event subscriptions ────────────────────────────────────
    //
    // We accumulate events into a rolling PROCESS_EVENTS_CAP-capped ring even
    // while the pane is not visible, so opening it shows recent
    // activity rather than an empty log. Lifecycle ties to workspace
    // opened/closed: the kernel only exists between `boot_kernel` and
    // `shutdown`, so we subscribe on `workspace:opened` and tear down
    // on `workspace:closed` to avoid dangling forwarder tasks.
    //
    // Same pattern as nexus.files — see files/index.ts for the
    // rationale on subscription cleanup.

    let kernelUnsubs: Array<() => void> = []

    const handleKernelEvent = (topic: string, payload: unknown) => {
      let payloadJson: string
      try {
        payloadJson = JSON.stringify(payload)
      } catch {
        payloadJson = String(payload)
      }
      useProcessesStore.getState().appendEvent({
        timestampMs: Date.now(),
        topic,
        payloadJson,
      })
    }

    const subscribeKernelEvents = async () => {
      if (kernelUnsubs.length > 0) return
      try {
        kernelUnsubs = await Promise.all(
          TOPIC_PREFIXES.map((prefix) => api.kernel.on(prefix, handleKernelEvent)),
        )
      } catch (err) {
        console.warn('[nexus.processes] failed to subscribe to kernel events:', err)
        kernelUnsubs = []
      }
    }

    const unsubscribeKernelEvents = () => {
      for (const unsub of kernelUnsubs) {
        try {
          unsub()
        } catch (err) {
          console.warn('[nexus.processes] unsubscribe failed:', err)
        }
      }
      kernelUnsubs = []
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      // Seed plugins now that main.tsx has populated pluginList. Also
      // refresh sessions since the kernel was just booted.
      useProcessesStore.getState().setPlugins(readShellPlugins(api))
      void readSessions(api).then((sessions) => {
        useProcessesStore.getState().setSessions(sessions)
      })
      void subscribeKernelEvents()
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useProcessesStore.getState().setSessions([])
      unsubscribeKernelEvents()
    })

    // Cover the restore-on-boot race: nexus.workspace may have already
    // emitted `workspace:opened` before our listener attached. If the
    // kernel is up, subscribe and seed immediately.
    useProcessesStore.getState().setPlugins(readShellPlugins(api))
    if (await api.kernel.available()) {
      void readSessions(api).then((sessions) => {
        useProcessesStore.getState().setSessions(sessions)
      })
      void subscribeKernelEvents()
    }
  },
}
