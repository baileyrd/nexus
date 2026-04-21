import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry } from '../../../workspace'
import { McpView } from './McpView'
import { mcpPaneViewCreator } from './McpPaneView'
import { ToolCallModal } from './ToolCallModal'
import {
  useMcpStore,
  type McpPromptRow,
  type McpResourceRow,
  type McpServerEntry,
  type McpToolRow,
} from './mcpStore'

const VIEW_ID = 'nexus.mcp.view'
const TOOL_MODAL_VIEW_ID = 'nexus.mcp.toolCallModal'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'
const EVENT_SIDEBAR_SHOW_VIEW = 'sidebar:showView'

const COMMAND_REFRESH = 'nexus.mcp.refresh'
const COMMAND_SHOW = 'nexus.mcp.show'

const MCP_PLUGIN_ID = 'com.nexus.mcp.host'
// Verified against crates/nexus-mcp/src/core_plugin.rs::dispatch + dispatch_async:
//   `list_servers`   args `{}`                                → `[{ name, command, args[], disabled }]`
//   `connect`        args `{ server }`                        → `{ ok, server }`
//   `disconnect`     args `{ server }`                        → `{ ok, server, reason? }`
//   `list_tools`     args `{ server }`                        → `[{ name, description }]`
//   `list_resources` args `{ server }`                        → `[{ uri, name, description, mime_type }]`
//   `list_prompts`   args `{ server }`                        → `[{ name, description }]`
//   `call_tool`      args `{ server, tool, arguments? }`      → `{ content[], is_error }`
const LIST_SERVERS = 'list_servers'
const CONNECT = 'connect'
const DISCONNECT = 'disconnect'
const LIST_TOOLS = 'list_tools'
const LIST_RESOURCES = 'list_resources'
const LIST_PROMPTS = 'list_prompts'
const CALL_TOOL = 'call_tool'

// MCP servers spawn a subprocess on connect — pick a generous ceiling
// over the 30s default timeout. Capability listing is auto-connected
// on first call (`get_or_connect`), so the cold path can stretch.
const CONNECT_TIMEOUT_MS = 60_000

function decodeServers(raw: unknown): McpServerEntry[] {
  if (!Array.isArray(raw)) return []
  const out: McpServerEntry[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const name = typeof r.name === 'string' ? r.name : null
    if (!name) continue
    out.push({
      name,
      command: typeof r.command === 'string' ? r.command : '',
      args: Array.isArray(r.args) ? r.args.filter((a): a is string => typeof a === 'string') : [],
      disabled: r.disabled === true,
    })
  }
  return out.sort((a, b) => a.name.localeCompare(b.name))
}

function decodeTools(raw: unknown): McpToolRow[] {
  if (!Array.isArray(raw)) return []
  return raw
    .map((item) => {
      if (!item || typeof item !== 'object') return null
      const r = item as Record<string, unknown>
      if (typeof r.name !== 'string') return null
      return {
        name: r.name,
        description: typeof r.description === 'string' ? r.description : '',
      }
    })
    .filter((x): x is McpToolRow => x !== null)
}

function decodeResources(raw: unknown): McpResourceRow[] {
  if (!Array.isArray(raw)) return []
  return raw
    .map((item) => {
      if (!item || typeof item !== 'object') return null
      const r = item as Record<string, unknown>
      const uri = typeof r.uri === 'string' ? r.uri : null
      if (!uri) return null
      return {
        uri,
        name: typeof r.name === 'string' ? r.name : '',
        description: typeof r.description === 'string' ? r.description : '',
        mimeType: typeof r.mime_type === 'string' ? r.mime_type : '',
      }
    })
    .filter((x): x is McpResourceRow => x !== null)
}

function decodeCallToolResult(raw: unknown): { content: unknown[]; isError: boolean } {
  // Defensive: the kernel returns `{ content: [...], is_error: bool }`
  // verbatim from the rmcp client. Treat any non-object response or
  // missing fields as an empty success — the kernel won't normally
  // emit that shape, but it keeps the modal rendering predictable.
  if (!raw || typeof raw !== 'object') return { content: [], isError: false }
  const r = raw as Record<string, unknown>
  return {
    content: Array.isArray(r.content) ? r.content : [],
    isError: r.is_error === true,
  }
}

function decodePrompts(raw: unknown): McpPromptRow[] {
  if (!Array.isArray(raw)) return []
  return raw
    .map((item) => {
      if (!item || typeof item !== 'object') return null
      const r = item as Record<string, unknown>
      if (typeof r.name !== 'string') return null
      return {
        name: r.name,
        description: typeof r.description === 'string' ? r.description : '',
      }
    })
    .filter((x): x is McpPromptRow => x !== null)
}

export const mcpPlugin: Plugin = {
  manifest: {
    id: 'nexus.mcp',
    name: 'MCP',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      commands: [
        { id: COMMAND_REFRESH, title: 'Refresh MCP Servers', category: 'MCP' },
        { id: COMMAND_SHOW, title: 'Show MCP Servers', category: 'MCP' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const refresh = async () => {
      const store = useMcpStore.getState()
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        store.setLoading(false)
        store.setLoadError('Open a workspace to load MCP servers.')
        store.setServers([])
        return
      }
      store.setLoading(true)
      store.setLoadError(null)
      try {
        const raw = await api.kernel.invoke<unknown>(MCP_PLUGIN_ID, LIST_SERVERS, {})
        useMcpStore.getState().setServers(decodeServers(raw))
        useMcpStore.getState().setLoading(false)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useMcpStore.getState().setLoadError(message)
        useMcpStore.getState().setServers([])
        useMcpStore.getState().setLoading(false)
      }
    }

    const fetchDetails = async (name: string) => {
      const store = useMcpStore.getState()
      store.setLoadingDetails(name, true)
      try {
        const [tools, resources, prompts] = await Promise.all([
          api.kernel.invoke<unknown>(MCP_PLUGIN_ID, LIST_TOOLS, { server: name }, CONNECT_TIMEOUT_MS),
          api.kernel.invoke<unknown>(MCP_PLUGIN_ID, LIST_RESOURCES, { server: name }, CONNECT_TIMEOUT_MS),
          api.kernel.invoke<unknown>(MCP_PLUGIN_ID, LIST_PROMPTS, { server: name }, CONNECT_TIMEOUT_MS),
        ])
        useMcpStore.getState().setDetails(name, {
          tools: decodeTools(tools),
          resources: decodeResources(resources),
          prompts: decodePrompts(prompts),
        })
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useMcpStore.getState().setStatus(name, 'error', message)
        useMcpStore.getState().setLoadingDetails(name, false)
      }
    }

    const handleExpand = (name: string) => {
      const store = useMcpStore.getState()
      const wasExpanded = store.expandedName === name
      store.toggleExpanded(name)
      // Only fetch when opening, and only when we don't already have a
      // cached snapshot. Re-fetch via the explicit Connect button or
      // the header refresh.
      if (wasExpanded) return
      const cur = store.state[name]
      const hasCache = cur?.tools !== null && cur?.tools !== undefined
      const srv = store.servers.find((s) => s.name === name)
      if (srv?.disabled) return
      if (!hasCache) {
        useMcpStore.getState().setStatus(name, 'connecting')
        void fetchDetails(name)
      }
    }

    const handleConnect = async (name: string) => {
      const store = useMcpStore.getState()
      store.setStatus(name, 'connecting')
      try {
        await api.kernel.invoke<unknown>(MCP_PLUGIN_ID, CONNECT, { server: name }, CONNECT_TIMEOUT_MS)
        // Connect alone doesn't list capabilities — fetch them now so
        // the expanded panel reflects the live server immediately.
        await fetchDetails(name)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useMcpStore.getState().setStatus(name, 'error', message)
        api.notifications.show({
          type: 'error',
          message: `MCP "${name}" connect failed: ${message}`,
        })
      }
    }

    const handleCallTool = (server: string, tool: string) => {
      // Reading mid-edit args text from a previous open isn't useful —
      // openToolCall resets the modal to `{}` for a fresh start.
      useMcpStore.getState().openToolCall(server, tool)
    }

    const runToolCall = async (server: string, tool: string, args: Record<string, unknown>) => {
      const store = useMcpStore.getState()
      store.setToolCallStatus('running', { error: null, result: null })
      try {
        const raw = await api.kernel.invoke<unknown>(
          MCP_PLUGIN_ID,
          CALL_TOOL,
          // The kernel handler reads `arguments` (mcp protocol naming),
          // not `args` — see crates/nexus-mcp/src/core_plugin.rs:282.
          { server, tool, arguments: args },
          CONNECT_TIMEOUT_MS,
        )
        const result = decodeCallToolResult(raw)
        useMcpStore.getState().setToolCallStatus('done', { error: null, result })
        // Successful tool call also implies the server is up — sync
        // the row pill so a manual disconnect / reconnect dance isn't
        // needed to see "up".
        useMcpStore.getState().setStatus(server, 'connected')
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useMcpStore.getState().setToolCallStatus('error', { error: message })
      }
    }

    const handleDisconnect = async (name: string) => {
      const store = useMcpStore.getState()
      store.setStatus(name, 'disconnecting')
      try {
        await api.kernel.invoke<unknown>(MCP_PLUGIN_ID, DISCONNECT, { server: name })
        useMcpStore.getState().setStatus(name, 'idle')
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useMcpStore.getState().setStatus(name, 'error', message)
        api.notifications.show({
          type: 'error',
          message: `MCP "${name}" disconnect failed: ${message}`,
        })
      }
    }

    const renderMcpView = () =>
      createElement(McpView, {
        onRefresh: () => void refresh(),
        onConnect: (name: string) => void handleConnect(name),
        onDisconnect: (name: string) => void handleDisconnect(name),
        onExpand: handleExpand,
        onCallTool: handleCallTool,
      })

    api.views.register(VIEW_ID, {
      slot: 'sidebarContent',
      component: renderMcpView,
      priority: 50,
    })

    // Phase 5 workspace-View registration (leaf-migration-plan §Phase 5).
    viewRegistry.register('mcp', mcpPaneViewCreator(renderMcpView))

    api.views.register(TOOL_MODAL_VIEW_ID, {
      slot: 'overlay',
      component: () =>
        createElement(ToolCallModal, {
          onRun: runToolCall,
        }),
      priority: 30,
    })

    api.activityBar.addItem({
      id: 'nexus.mcp.activityItem',
      icon: '',
      iconName: 'plug',
      title: 'MCP',
      viewId: VIEW_ID,
      priority: 50,
    })

    api.commands.register(COMMAND_REFRESH, () => {
      void refresh()
    })
    api.commands.register(COMMAND_SHOW, () => {
      api.events.emit(EVENT_SIDEBAR_SHOW_VIEW, { viewId: VIEW_ID })
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refresh()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useMcpStore.getState().reset()
    })
    if (await api.kernel.available()) {
      void refresh()
    }
  },
}
