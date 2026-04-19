import { create } from 'zustand'

/**
 * One MCP server row, projected from `com.nexus.mcp.host::list_servers`.
 *
 * `disabled` mirrors the `disabled` flag in `mcp.toml`. The kernel
 * doesn't expose connection state — the shell tracks it locally as
 * `status` based on the outcome of its own connect / list_* invokes.
 */
export interface McpServerEntry {
  name: string
  command: string
  args: string[]
  disabled: boolean
}

export type ServerStatus =
  | 'idle'
  | 'connecting'
  | 'connected'
  | 'disconnecting'
  | 'error'

export interface McpToolRow {
  name: string
  description: string
}

export interface McpResourceRow {
  uri: string
  name: string
  description: string
  mimeType: string
}

export interface McpPromptRow {
  name: string
  description: string
}

/** Per-server runtime state. */
export interface ServerState {
  status: ServerStatus
  /** Last error from connect/list/disconnect; null when no error. */
  error: string | null
  /** Cached lists, populated on first expand or explicit reload. */
  tools: McpToolRow[] | null
  resources: McpResourceRow[] | null
  prompts: McpPromptRow[] | null
  /** Whether a list_* fetch is currently in flight. */
  loadingDetails: boolean
}

interface McpStoreState {
  loading: boolean
  loadError: string | null
  servers: McpServerEntry[]
  /** Per-server runtime state, keyed by server name. */
  state: Record<string, ServerState>
  expandedName: string | null

  setLoading(b: boolean): void
  setLoadError(e: string | null): void
  setServers(s: McpServerEntry[]): void
  setStatus(name: string, status: ServerStatus, error?: string | null): void
  setDetails(
    name: string,
    details: {
      tools: McpToolRow[]
      resources: McpResourceRow[]
      prompts: McpPromptRow[]
    },
  ): void
  setLoadingDetails(name: string, b: boolean): void
  toggleExpanded(name: string): void
  reset(): void
}

const INITIAL_SERVER_STATE: ServerState = {
  status: 'idle',
  error: null,
  tools: null,
  resources: null,
  prompts: null,
  loadingDetails: false,
}

function patchState(
  s: McpStoreState,
  name: string,
  patch: Partial<ServerState>,
): Record<string, ServerState> {
  return {
    ...s.state,
    [name]: { ...(s.state[name] ?? INITIAL_SERVER_STATE), ...patch },
  }
}

export const useMcpStore = create<McpStoreState>((set) => ({
  loading: false,
  loadError: null,
  servers: [],
  state: {},
  expandedName: null,

  setLoading: (b) => set({ loading: b }),
  setLoadError: (e) => set({ loadError: e }),
  setServers: (servers) => set({ servers }),
  setStatus: (name, status, error = null) =>
    set((s) => ({
      state: patchState(s, name, {
        status,
        error: status === 'error' ? error : null,
      }),
    })),
  setDetails: (name, details) =>
    set((s) => ({
      state: patchState(s, name, {
        tools: details.tools,
        resources: details.resources,
        prompts: details.prompts,
        loadingDetails: false,
        // A successful list implies the server is reachable.
        status: 'connected',
        error: null,
      }),
    })),
  setLoadingDetails: (name, b) =>
    set((s) => ({ state: patchState(s, name, { loadingDetails: b }) })),
  toggleExpanded: (name) =>
    set((s) => ({ expandedName: s.expandedName === name ? null : name })),
  reset: () =>
    set({
      loading: false,
      loadError: null,
      servers: [],
      state: {},
      expandedName: null,
    }),
}))

export function getServerState(name: string): ServerState {
  return useMcpStore.getState().state[name] ?? INITIAL_SERVER_STATE
}
