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
const CMD_RECALL = 'nexus.memory.recall'
const CMD_REINDEX = 'nexus.memory.reindex'
const CMD_RECENT = 'nexus.memory.recent'
const CMD_FACTS = 'nexus.memory.facts'
const CMD_ENTITIES = 'nexus.memory.entities'
const CMD_TAGS = 'nexus.memory.tags'
const CMD_VITALITY = 'nexus.memory.vitality'
const CMD_SYNC = 'nexus.memory.sync'
const CMD_CAPTURE = 'nexus.memory.capture'
const CMD_CONSOLIDATE = 'nexus.memory.consolidate'
const CMD_WIKI_COMPILE = 'nexus.memory.wikiCompile'
const CMD_WIKI_PAGES = 'nexus.memory.wikiPages'
const CMD_STATS = 'nexus.memory.stats'

const LIST_LIMIT = 30

/** Subset of a `com.nexus.memory` row the dashboard renders. */
interface MemoryRow {
  id?: string
  content: string
  category?: string
  memory_type?: string
  source?: string
  subject?: string
  predicate?: string
  object?: string
}

/** One `{ key, count }` bucket from `stats`/`entities`/`tags`. */
interface CountRow {
  key: string
  count: number
}

/** Aggregate statistics from the `stats` handler. */
interface MemoryStats {
  count: number
  by_category: CountRow[]
  by_memory_type: CountRow[]
  by_source: CountRow[]
}

/** Coerce a `search`/`list`/`facts` response (a JSON array of memory objects). */
export function decodeMemories(raw: unknown): MemoryRow[] {
  if (!Array.isArray(raw)) return []
  const out: MemoryRow[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    out.push({
      id: typeof r.id === 'string' ? r.id : undefined,
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

/** Coerce an `entities`/`tags` response (a JSON array of `{ key, count }`). */
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

/** Coerce a `stats` response into a [`MemoryStats`]. */
function decodeStats(raw: unknown): MemoryStats {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>
  return {
    count: typeof r.count === 'number' ? r.count : 0,
    by_category: decodeCounts(r.by_category),
    by_memory_type: decodeCounts(r.by_memory_type),
    by_source: decodeCounts(r.by_source),
  }
}

/** Render up to `n` `{key, count}` buckets as `key (count)` for a summary line. */
function topBuckets(rows: CountRow[], n = 3): string {
  return rows
    .slice(0, n)
    .map((r) => `${r.key} (${r.count})`)
    .join(', ')
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

// C35 (#388) — the memory plugin's `update`/`delete` IPC handlers already
// existed but nothing in the shell called them: selecting a memory only
// ever toasted its content. These two helpers are the actual IPC dispatch
// for the new "forget" / "edit" actions, kept separate from the pick-modal
// wiring in `presentMemoryActions` so the argument shaping is unit-testable
// without mocking the modal stack (mirrors crdtConflict/applyResolution.ts).

/** Delete memory `id`. Returns `null` on success, an error message otherwise. */
export async function forgetMemory(api: PluginAPI, id: string): Promise<string | null> {
  try {
    const res = await api.kernel.invoke<{ deleted?: boolean }>(MEMORY_PLUGIN, 'delete', { id })
    return res.deleted ? null : 'Memory was already gone.'
  } catch (e) {
    return String(e)
  }
}

/** Patch memory `id`'s content. Returns `null` on success, an error message otherwise. */
export async function updateMemoryContent(
  api: PluginAPI,
  id: string,
  content: string,
): Promise<string | null> {
  try {
    const res = await api.kernel.invoke<{ updated?: boolean }>(MEMORY_PLUGIN, 'update', {
      id,
      content,
    })
    return res.updated ? null : 'Memory was not found.'
  } catch (e) {
    return String(e)
  }
}

/** C41 / #394 — the four cognitive classes a memory can be reclassified into. */
export const MEMORY_TYPES = ['episodic', 'semantic', 'procedural', 'unclassified'] as const
export type MemoryTypeValue = (typeof MEMORY_TYPES)[number]

/** Patch memory `id`'s cognitive class. Returns `null` on success, an error message otherwise. */
export async function reclassifyMemory(
  api: PluginAPI,
  id: string,
  memoryType: MemoryTypeValue,
): Promise<string | null> {
  try {
    const res = await api.kernel.invoke<{ updated?: boolean }>(MEMORY_PLUGIN, 'update', {
      id,
      memory_type: memoryType,
    })
    return res.updated ? null : 'Memory was not found.'
  } catch (e) {
    return String(e)
  }
}

/**
 * Follow-up action menu for a single picked memory: view its full content,
 * edit it in place, or forget (delete) it. Memories without an `id` (should
 * not happen once every store response carries one, but IPC payloads are
 * never fully trusted) fall back to the old read-only toast.
 */
async function presentMemoryActions(api: PluginAPI, m: MemoryRow): Promise<void> {
  if (!m.id) {
    api.notifications.show({ message: m.content || '(empty memory)', type: 'info', duration: 8000 })
    return
  }
  const actions: PickItem<'view' | 'edit' | 'reclassify' | 'delete'>[] = [
    { label: 'View full content', value: 'view' },
    { label: 'Edit content…', value: 'edit' },
    {
      label: 'Reclassify…',
      value: 'reclassify',
      description: `Currently: ${m.memory_type ?? 'unclassified'}`,
    },
    { label: 'Forget (delete)…', value: 'delete' },
  ]
  const action = await api.input.pick(actions, {
    title: excerpt(m.content, 60) || '(empty memory)',
    placeholder: 'Choose an action',
  })
  if (!action) return

  if (action === 'view') {
    api.notifications.show({ message: m.content || '(empty memory)', type: 'info', duration: 8000 })
    return
  }

  if (action === 'edit') {
    const next = await api.input.prompt('Edit memory content', m.content)
    if (next === null) return
    const trimmed = next.trim()
    if (!trimmed || trimmed === m.content) return
    const err = await updateMemoryContent(api, m.id, trimmed)
    api.notifications.show(
      err
        ? { message: `Update failed: ${err}`, type: 'error' }
        : { message: 'Memory updated.', type: 'info' },
    )
    return
  }

  if (action === 'reclassify') {
    const typeItems: PickItem<MemoryTypeValue>[] = MEMORY_TYPES.map((t) => ({
      label: t === m.memory_type ? `${t} (current)` : t,
      value: t,
    }))
    const picked = await api.input.pick(typeItems, {
      title: 'Reclassify as…',
      placeholder: 'Choose a cognitive class',
    })
    if (!picked || picked === m.memory_type) return
    const err = await reclassifyMemory(api, m.id, picked)
    api.notifications.show(
      err
        ? { message: `Reclassify failed: ${err}`, type: 'error' }
        : { message: `Reclassified as ${picked}.`, type: 'info' },
    )
    return
  }

  // delete
  const ok = await api.input.confirm(
    `Forget this memory? This cannot be undone.\n\n${excerpt(m.content, 120) || '(empty memory)'}`,
  )
  if (!ok) return
  const err = await forgetMemory(api, m.id)
  api.notifications.show(
    err
      ? { message: `Delete failed: ${err}`, type: 'error' }
      : { message: 'Memory forgotten.', type: 'info' },
  )
}

/** Show memories (or facts) in the pick modal; on selection, offer view/edit/forget. */
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
    placeholder: 'Select a memory to view, edit, or forget it',
  })
  if (picked) {
    await presentMemoryActions(api, picked)
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
        { id: CMD_RECALL, title: 'Memory: Recall (hybrid)', category: 'Memory' },
        { id: CMD_REINDEX, title: 'Memory: Reindex Vectors', category: 'Memory' },
        { id: CMD_RECENT, title: 'Memory: Recent', category: 'Memory' },
        { id: CMD_FACTS, title: 'Memory: Facts', category: 'Memory' },
        { id: CMD_ENTITIES, title: 'Memory: Entities', category: 'Memory' },
        { id: CMD_TAGS, title: 'Memory: Tags', category: 'Memory' },
        { id: CMD_VITALITY, title: 'Memory: Vitality', category: 'Memory' },
        { id: CMD_SYNC, title: 'Memory: Sync Now', category: 'Memory' },
        { id: CMD_CAPTURE, title: 'Memory: Capture Turn', category: 'Memory' },
        { id: CMD_CONSOLIDATE, title: 'Memory: Consolidate (dedupe)', category: 'Memory' },
        { id: CMD_WIKI_COMPILE, title: 'Memory: Compile Wiki Page', category: 'Memory' },
        { id: CMD_WIKI_PAGES, title: 'Memory: Wiki Pages', category: 'Memory' },
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

    api.commands.register(CMD_RECALL, async () => {
      const query = await api.input.prompt('Recall memory', 'What do you want to recall?')
      if (query === null) return
      const trimmed = query.trim()
      if (!trimmed) return
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'recall', { query: trimmed, limit: LIST_LIMIT })
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory recall failed: ${String(e)}`, type: 'error' })
          return [] as unknown
        })
      await presentMemories(api, decodeMemories(raw), `Nothing recalled for "${trimmed}".`)
    })

    api.commands.register(CMD_REINDEX, async () => {
      api.notifications.show({ message: 'Reindexing memory vectors…', type: 'info' })
      const res = await api.kernel
        .invoke<{ indexed?: number }>(MEMORY_PLUGIN, 'vector_sync', {})
        .catch((e: unknown) => {
          api.notifications.show({ message: `Vector reindex failed: ${String(e)}`, type: 'error' })
          return null
        })
      if (res === null) return
      const indexed = typeof res.indexed === 'number' ? res.indexed : 0
      api.notifications.show({ message: `Reindexed ${indexed} memories for semantic recall.`, type: 'info' })
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

    api.commands.register(CMD_TAGS, async () => {
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'tags', { limit: LIST_LIMIT })
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory tags failed: ${String(e)}`, type: 'error' })
          return [] as unknown
        })
      const tags = decodeCounts(raw)
      if (tags.length === 0) {
        api.notifications.show({ message: 'No tags yet — add memories with tags first.', type: 'info' })
        return
      }
      const items: PickItem<CountRow>[] = tags.map((t) => ({
        label: t.key,
        description: `${t.count} memor${t.count === 1 ? 'y' : 'ies'}`,
        value: t,
      }))
      const picked = await api.input.pick(items, {
        title: `Tags (${tags.length})`,
        placeholder: 'Select a tag to list the memories carrying it',
      })
      if (!picked) return
      // Drill down: list memories carrying the chosen tag.
      const taggedRaw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'list', { tag: picked.key, limit: LIST_LIMIT })
        .catch(() => [] as unknown)
      await presentMemories(api, decodeMemories(taggedRaw), `No memories tagged "${picked.key}".`)
    })

    api.commands.register(CMD_VITALITY, async () => {
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'vitality_report', { limit: LIST_LIMIT })
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory vitality failed: ${String(e)}`, type: 'error' })
          return [] as unknown
        })
      await presentMemories(api, decodeMemories(raw), 'No active memories yet.')
    })

    api.commands.register(CMD_SYNC, async () => {
      api.notifications.show({ message: 'Syncing memory with hub…', type: 'info' })
      const res = await api.kernel
        .invoke<{ pushed?: number; pulled?: number }>(MEMORY_PLUGIN, 'sync', {})
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory sync failed: ${String(e)}`, type: 'error' })
          return null
        })
      if (res === null) return
      const pushed = typeof res.pushed === 'number' ? res.pushed : 0
      const pulled = typeof res.pulled === 'number' ? res.pulled : 0
      api.notifications.show({
        message: `Memory synced — pushed ${pushed}, pulled ${pulled}.`,
        type: 'info',
      })
    })

    api.commands.register(CMD_CAPTURE, async () => {
      const content = await api.input.prompt('Capture to memory', 'Text / turn to remember')
      if (content === null) return
      const trimmed = content.trim()
      if (!trimmed) return
      const res = await api.kernel
        .invoke<{ capture_id?: string; children?: string[] }>(
          MEMORY_PLUGIN,
          'auto_capture',
          { content: trimmed, decompose: true },
        )
        .catch((e: unknown) => {
          api.notifications.show({ message: `Capture failed: ${String(e)}`, type: 'error' })
          return null
        })
      if (res === null) return
      const facts = Array.isArray(res.children) ? res.children.length : 0
      api.notifications.show({
        message: `Captured${facts ? ` and decomposed into ${facts} fact${facts === 1 ? '' : 's'}` : ''}.`,
        type: 'info',
      })
    })

    api.commands.register(CMD_CONSOLIDATE, async () => {
      const res = await api.kernel
        .invoke<{ clusters?: number; superseded?: number }>(MEMORY_PLUGIN, 'consolidate', {})
        .catch((e: unknown) => {
          api.notifications.show({ message: `Consolidate failed: ${String(e)}`, type: 'error' })
          return null
        })
      if (res === null) return
      const superseded = typeof res.superseded === 'number' ? res.superseded : 0
      api.notifications.show({
        message:
          superseded > 0
            ? `Consolidated — superseded ${superseded} duplicate${superseded === 1 ? '' : 's'}.`
            : 'No duplicate memories to consolidate.',
        type: 'info',
      })
    })

    api.commands.register(CMD_WIKI_COMPILE, async () => {
      const topic = await api.input.prompt('Compile wiki page', 'Topic to synthesize from memories')
      if (topic === null) return
      const trimmed = topic.trim()
      if (!trimmed) return
      api.notifications.show({ message: `Synthesizing wiki page for "${trimmed}"…`, type: 'info' })
      type WikiCompileResult = { path?: string; sources?: number; error?: string }
      const res: WikiCompileResult = await api.kernel
        .invoke<WikiCompileResult>(MEMORY_PLUGIN, 'wiki_compile', { topic: trimmed })
        .catch((e: unknown): WikiCompileResult => ({ error: String(e) }))
      if (res.error) {
        api.notifications.show({ message: `Wiki compile failed: ${res.error}`, type: 'error' })
        return
      }
      api.notifications.show({
        message: `Wrote ${res.path ?? 'wiki page'} from ${res.sources ?? 0} memories.`,
        type: 'info',
      })
    })

    api.commands.register(CMD_WIKI_PAGES, async () => {
      const raw = await api.kernel
        .invoke<{ pages?: { slug?: string; path?: string }[] }>(MEMORY_PLUGIN, 'wiki_list', {})
        .catch((e: unknown) => {
          api.notifications.show({ message: `Wiki list failed: ${String(e)}`, type: 'error' })
          return { pages: [] }
        })
      const pages = Array.isArray(raw.pages) ? raw.pages : []
      const slugs = pages.map((p) => p.slug).filter((s): s is string => typeof s === 'string')
      if (slugs.length === 0) {
        api.notifications.show({ message: 'No wiki pages yet — compile one first.', type: 'info' })
        return
      }
      const items: PickItem<string>[] = slugs.map((slug) => ({ label: slug, value: slug }))
      const picked = await api.input.pick(items, {
        title: `Wiki (${slugs.length})`,
        placeholder: 'Select a page to read',
      })
      if (!picked) return
      const page = await api.kernel
        .invoke<{ content?: string }>(MEMORY_PLUGIN, 'wiki_read', { topic: picked })
        .catch(() => ({ content: undefined }))
      api.notifications.show({
        message: page.content ? excerpt(page.content, 400) : `(empty page "${picked}")`,
        type: 'info',
        duration: 12000,
      })
    })

    api.commands.register(CMD_STATS, async () => {
      const raw = await api.kernel
        .invoke<unknown>(MEMORY_PLUGIN, 'stats', {})
        .catch((e: unknown) => {
          api.notifications.show({ message: `Memory stats failed: ${String(e)}`, type: 'error' })
          return null
        })
      if (raw === null) return
      const stats = decodeStats(raw)
      const lines = [
        `${stats.count} memories stored.`,
        topBuckets(stats.by_category) ? `Categories: ${topBuckets(stats.by_category)}` : '',
        topBuckets(stats.by_memory_type) ? `Types: ${topBuckets(stats.by_memory_type)}` : '',
        topBuckets(stats.by_source) ? `Sources: ${topBuckets(stats.by_source)}` : '',
      ].filter(Boolean)
      api.notifications.show({ message: lines.join('\n'), type: 'info', duration: 8000 })
    })
  },
}
