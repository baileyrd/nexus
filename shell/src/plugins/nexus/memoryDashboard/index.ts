// shell/src/plugins/nexus/memoryDashboard/index.ts
//
// Command-palette surface over the native memory engine (`com.nexus.memory`).
//
// Provides three commands — Search, Recent, Stats — that call the memory
// plugin's IPC handlers and present results through the shared pick-modal
// (`api.input.pick`). This is the v1 "dashboard": a browsable view of the
// cross-model memory store from the command palette. A dedicated side-pane
// view can layer on later via `api.viewRegistry` (see the search plugin).

import type { Plugin, PluginAPI, PickItem } from '../../../types/plugin'

const MEMORY_PLUGIN = 'com.nexus.memory'

const CMD_SEARCH = 'nexus.memory.search'
const CMD_RECENT = 'nexus.memory.recent'
const CMD_STATS = 'nexus.memory.stats'

const LIST_LIMIT = 30

/** Subset of a `com.nexus.memory` row the dashboard renders. */
interface MemoryRow {
  content: string
  category?: string
  memory_type?: string
  source?: string
}

/** Coerce a `search`/`list` response (a JSON array of memory objects). */
function decodeMemories(raw: unknown): MemoryRow[] {
  if (!Array.isArray(raw)) return []
  const out: MemoryRow[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    out.push({
      content: typeof r.content === 'string' ? r.content : '',
      category: typeof r.category === 'string' ? r.category : undefined,
      memory_type: typeof r.memory_type === 'string' ? r.memory_type : undefined,
      source: typeof r.source === 'string' ? r.source : undefined,
    })
  }
  return out
}

/** Single-line excerpt of `s`, collapsed and clipped to `n` chars. */
function excerpt(s: string, n = 80): string {
  const flat = s.replace(/\s+/g, ' ').trim()
  return flat.length > n ? `${flat.slice(0, n - 1)}…` : flat
}

/** Show memories in the pick modal; on selection, surface the full content. */
async function presentMemories(
  api: PluginAPI,
  rows: MemoryRow[],
  emptyMessage: string,
): Promise<void> {
  if (rows.length === 0) {
    api.notifications.show({ message: emptyMessage, type: 'info' })
    return
  }
  const items: PickItem<MemoryRow>[] = rows.map((m) => ({
    label: excerpt(m.content) || '(empty memory)',
    description: [m.memory_type, m.category].filter(Boolean).join(' · ') || undefined,
    detail: m.source ? `source: ${m.source}` : undefined,
    value: m,
  }))
  const picked = await api.input.pick(items, {
    title: `Memory (${rows.length})`,
    placeholder: 'Select a memory to view its full content',
  })
  if (picked) {
    api.notifications.show({
      message: picked.content || '(empty memory)',
      type: 'info',
      duration: 8000,
    })
  }
}

export const memoryDashboardPlugin: Plugin = {
  manifest: {
    id: 'nexus.memoryDashboard',
    name: 'Memory Dashboard',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['com.nexus.memory'],
    contributes: {
      commands: [
        { id: CMD_SEARCH, title: 'Memory: Search', category: 'Memory' },
        { id: CMD_RECENT, title: 'Memory: Recent', category: 'Memory' },
        { id: CMD_STATS, title: 'Memory: Stats', category: 'Memory' },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.commands.register(CMD_SEARCH, async () => {
      const query = await api.input.prompt('Search memory', 'What are you looking for?')
      if (query === null) return
      const trimmed = query.trim()
      if (!trimmed) return
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'search', { query: trimmed, limit: LIST_LIMIT })
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory search failed: ${String(e)}`, type: 'error' })
          return [] as unknown
        })
      await presentMemories(api, decodeMemories(raw), `No memories match "${trimmed}".`)
    })

    api.commands.register(CMD_RECENT, async () => {
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'list', { limit: LIST_LIMIT })
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory list failed: ${String(e)}`, type: 'error' })
          return [] as unknown
        })
      await presentMemories(api, decodeMemories(raw), 'No memories stored yet.')
    })

    api.commands.register(CMD_STATS, async () => {
      const stats = await api.kernel
        .invoke<{ count?: number }>(MEMORY_PLUGIN, 'stats', {})
        .catch(() => ({}) as { count?: number })
      const count = typeof stats.count === 'number' ? stats.count : 0
      api.notifications.show({ message: `${count} memories stored.`, type: 'info' })
    })
  },
}
