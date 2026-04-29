// shell/src/plugins/nexus/semanticSearch/index.ts
//
// BL-040 — "Search by Meaning" command-palette surface.
//
// Adds a single command (`nexus.semanticSearch.run`) to the palette.
// When invoked it prompts for a query, calls both
// `com.nexus.storage::search` (keyword / FTS) and
// `com.nexus.ai::semantic_search` (embedding-backed) in parallel,
// merges the two lists per the BL-040 ranking rule (see ./merge.ts),
// and opens the top result via the same `files:open` event the
// existing search plugin uses. A notification reports the merged
// hit count so the user gets feedback even when no result was opened.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { mergeResults, type KeywordHit, type SemanticHit } from './merge'

const COMMAND_RUN = 'nexus.semanticSearch.run'

const STORAGE_PLUGIN = 'com.nexus.storage'
const AI_PLUGIN = 'com.nexus.ai'
const KEYWORD_COMMAND = 'search'
const SEMANTIC_COMMAND = 'semantic_search'

const KEYWORD_LIMIT = 30
const SEMANTIC_LIMIT = 30
const MERGED_LIMIT = 30

const EVENT_FILE_OPEN = 'files:open'

/** Forward-slash basename of a forge-relative path. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/**
 * Coerce an unknown response into `KeywordHit[]`.
 *
 * The storage `search` handler returns `Vec<SearchResult>` with
 * `{ file_path, block_id, block_type, excerpt, score }`. We only
 * need `file_path`, `excerpt`, and `score`.
 */
function decodeKeyword(raw: unknown): KeywordHit[] {
  if (!Array.isArray(raw)) return []
  const out: KeywordHit[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const file_path = typeof r.file_path === 'string' ? r.file_path : null
    if (!file_path) continue
    out.push({
      file_path,
      excerpt: typeof r.excerpt === 'string' ? r.excerpt : '',
      score: typeof r.score === 'number' ? r.score : 0,
    })
  }
  return out
}

/**
 * Coerce an unknown response into `SemanticHit[]`.
 *
 * The AI `semantic_search` handler returns `{ matches: Vec<ChunkMatch> }`
 * where each match is `{ file_path, block_id, chunk_text, score }`.
 */
function decodeSemantic(raw: unknown): SemanticHit[] {
  if (!raw || typeof raw !== 'object') return []
  const wrapped = (raw as Record<string, unknown>).matches
  if (!Array.isArray(wrapped)) return []
  const out: SemanticHit[] = []
  for (const item of wrapped) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const file_path = typeof r.file_path === 'string' ? r.file_path : null
    if (!file_path) continue
    out.push({
      file_path,
      block_id: typeof r.block_id === 'number' ? r.block_id : undefined,
      chunk_text: typeof r.chunk_text === 'string' ? r.chunk_text : '',
      score: typeof r.score === 'number' ? r.score : 0,
    })
  }
  return out
}

export const semanticSearchPlugin: Plugin = {
  manifest: {
    id: 'nexus.semanticSearch',
    name: 'Semantic Search',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        {
          id: COMMAND_RUN,
          title: 'Search by Meaning',
          category: 'Search',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.commands.register(COMMAND_RUN, async () => {
      const query = await api.input.prompt(
        'Search by meaning',
        'What are you looking for?',
      )
      if (query === null) return
      const trimmed = query.trim()
      if (!trimmed) return

      // Fan out both lookups in parallel — the slowest of the two
      // sets the wall-clock latency, which is what the user feels.
      const keywordPromise = api.kernel
        .invoke<unknown>(STORAGE_PLUGIN, KEYWORD_COMMAND, {
          query: trimmed,
          limit: KEYWORD_LIMIT,
        })
        .catch(() => [] as unknown)
      const semanticPromise = api.kernel
        .invoke<unknown>(AI_PLUGIN, SEMANTIC_COMMAND, {
          query: trimmed,
          limit: SEMANTIC_LIMIT,
        })
        .catch(() => ({ matches: [] }) as unknown)

      const [rawKeyword, rawSemantic] = await Promise.all([
        keywordPromise,
        semanticPromise,
      ])

      const merged = mergeResults(
        decodeKeyword(rawKeyword),
        decodeSemantic(rawSemantic),
        MERGED_LIMIT,
      )

      if (merged.length === 0) {
        api.notifications.show({
          message: `No results for "${trimmed}".`,
          type: 'info',
        })
        return
      }

      // Open the top hit. Mirrors how the keyword-only search sidebar
      // handles a row click.
      const top = merged[0]
      api.events.emit(EVENT_FILE_OPEN, {
        relpath: top.file_path,
        name: basename(top.file_path) || top.file_path,
      })
      api.notifications.show({
        message: `Opened ${top.file_path} (${merged.length} matches).`,
        type: 'info',
      })
    })
  },
}
