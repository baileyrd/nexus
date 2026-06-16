// shell/src/plugins/nexus/memoryDashboard/index.ts
//
// Command-palette surface over the native memory engine (`com.nexus.memory`).
//
// Provides five commands — Search, Recent, Facts, Entities, Stats — that call
// the memory plugin's IPC handlers and present results through the shared
// pick-modal (`api.input.pick`). This is the v1 "dashboard": a browsable view
// of the cross-model memory store (including its SPO entity graph) from the
// command palette. A dedicated side-pane view can layer on later via
// `api.viewRegistry` (see the search plugin).

import type { Plugin, PluginAPI, PickItem } from '../../../types/plugin'

const MEMORY_PLUGIN = 'com.nexus.memory'

const CMD_SEARCH = 'nexus.memory.search'
const CMD_RECENT = 'nexus.memory.recent'
const CMD_FACTS = 'nexus.memory.facts'
const CMD_ENTITIES = 'nexus.memory.entities'
const CMD_STATS = 'nexus.memory.stats'

const LIST_LIMIT = 30

/** Subset of a `com.nexus.memory` row the dashboard renders. */
interface MemoryRow {
  content: string
  category?: string
  memory_type?: string
  source?: string
  subject?: string
  predicate?: string
  object?: string
}

/** One `{ key, count }` bucket from `stats`/`entities`. */
interface CountRow {
  key: string
  count: number
}

/** Coerce a `search`/`list`/`facts` response (a JSON array of memory objects). */
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
      subject: typeof r.subject === 'string' ? r.subject : undefined,
      predicate: typeof r.predicate === 'string' ? r.predicate : undefined,
      object: typeof r.object === 'string' ? r.object : undefined,
    })
  }
  return out
}

/** Coerce an `entities` response (a JSON array of `{ key, count }`). */
function decodeCounts(raw: unknown): CountRow[] {
  if (!Array.isArray(raw)) return []
  const out: CountRow[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    if (typeof r.key !== 'string') continue
    out.push({ key: r.key, count: typeof r.count === 'number' ? r.count : 0 })
  }
  return out
}

/** Single-line excerpt of `s`, collapsed and clipped to `n` chars. */
function excerpt(s: string, n = 80): string {
  const flat = s.replace(/\s+/g, ' ').trim()
  return flat.length > n ? `${flat.slice(0, n - 1)}…` : flat
}

/** `true` when the row carries an SPO entity fact. */
function isFact(m: MemoryRow): boolean {
  return Boolean(m.subject)
}

/** Render an SPO triple as `subject ─predicate→ object`. */
function tripleLabel(m: MemoryRow): string {
  return `${m.subject} ─${m.predicate ?? '?'}→ ${m.object ?? '?'}`
}

/** Show memories (or facts) in the pick modal; on selection, surface content. */
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
    label: isFact(m) ? tripleLabel(m) : excerpt(m.content) || '(empty memory)',
    description: [m.memory_type, m.category].filter(Boolean).join(' · ') || undefined,
    detail: isFact(m) ? excerpt(m.content) : m.source ? `source: ${m.source}` : undefined,
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
        { id: CMD_FACTS, title: 'Memory: Facts', category: 'Memory' },
        { id: CMD_ENTITIES, title: 'Memory: Entities', category: 'Memory' },
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

    api.commands.register(CMD_FACTS, async () => {
      const subject = await api.input.prompt(
        'Recall facts',
        'Subject to filter by (blank = all facts)',
      )
      if (subject === null) return
      const trimmed = subject.trim()
      const args = trimmed
        ? { subject: trimmed, limit: LIST_LIMIT }
        : { limit: LIST_LIMIT }
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'facts', args)
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory facts failed: ${String(e)}`, type: 'error' })
          return [] as unknown
        })
      const empty = trimmed ? `No facts about "${trimmed}".` : 'No entity facts stored yet.'
      await presentMemories(api, decodeMemories(raw), empty)
    })

    api.commands.register(CMD_ENTITIES, async () => {
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'entities', { limit: LIST_LIMIT })
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory entities failed: ${String(e)}`, type: 'error' })
          return [] as unknown
        })
      const entities = decodeCounts(raw)
      if (entities.length === 0) {
        api.notifications.show({ message: 'No entities yet — store some SPO facts first.', type: 'info' })
        return
      }
      const items: PickItem<CountRow>[] = entities.map((e) => ({
        label: e.key,
        description: `${e.count} fact${e.count === 1 ? '' : 's'}`,
        value: e,
      }))
      const picked = await api.input.pick(items, {
        title: `Entities (${entities.length})`,
        placeholder: 'Select an entity to see the facts that mention it',
      })
      if (!picked) return
      // Drill down: show facts whose subject is the chosen entity.
      const factsRaw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'facts', { subject: picked.key, limit: LIST_LIMIT })
        .catch(() => [] as unknown)
      await presentMemories(api, decodeMemories(factsRaw), `No facts with "${picked.key}" as subject.`)
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
