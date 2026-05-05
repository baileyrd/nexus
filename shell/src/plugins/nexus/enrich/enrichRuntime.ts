// shell/src/plugins/nexus/enrich/enrichRuntime.ts
//
// BL-045 — auto-enrichment runtime.
//
// On every `files:saved` event for a markdown file, schedule a
// throttled call to `com.nexus.ai::enrich_file`. The throttle is a
// per-file 5-second debounce: rapid consecutive saves of the same
// file collapse into one enrichment proposal, while saves of *other*
// files still fire on their own clocks.
//
// FU-1: enrichment is gated to "inbox scope". A save is in scope iff
// its relpath equals `memory.inboxPath` (BL-043) OR the file carries
// an `#inbox` tag (probed via `com.nexus.storage::query_tags`). The
// "Force enrich active file" palette command (`nexus.enrich.force`)
// bypasses this gate.
//
// The successful proposal is pushed into `useEnrichStore.pending`
// where the accept-gate UI picks it up. Failures are recorded as a
// dismissable error rather than thrown — enrichment is an opt-in
// background luxury and must never interfere with the save itself.

import type { PluginAPI } from '../../../types/plugin'
import { useEnrichStore, type EnrichmentProposal } from './enrichStore'

const THROTTLE_MS = 5000

const AI_PLUGIN = 'com.nexus.ai'
const COMMAND_ENRICH_FILE = 'enrich_file'
const COMMAND_ENRICH_APPLY = 'enrich_apply'

const STORAGE_PLUGIN = 'com.nexus.storage'
const COMMAND_QUERY_TAGS = 'query_tags'

const CONFIG_INBOX_PATH = 'memory.inboxPath'
const INBOX_TAG = 'inbox'

interface FileSavedPayload {
  relpath: string
}

const pending = new Map<string, ReturnType<typeof setTimeout>>()

/** Test-only: cancel everything pending. */
export function cancelAllPending(): void {
  for (const t of pending.values()) clearTimeout(t)
  pending.clear()
}

function isMarkdown(relpath: string): boolean {
  return /\.(md|mdx|markdown)$/i.test(relpath)
}

interface TagResult {
  file_path?: unknown
}

/**
 * Decide whether `relpath` is in inbox scope. Inbox scope = the
 * configured `memory.inboxPath` OR any file currently tagged
 * `#inbox`. Either signal is enough; we only run the cross-file tag
 * lookup when the path-equality check misses.
 *
 * Failures (e.g. storage plugin not yet booted) fall back to the
 * path-equality result so a momentarily-unavailable index doesn't
 * silently disable enrichment for legitimately-tagged files.
 */
export async function isInInboxScope(api: PluginAPI, relpath: string): Promise<boolean> {
  let inboxPath: string | null = null
  try {
    inboxPath = api.configuration.getValue<string | null>(CONFIG_INBOX_PATH, null)
  } catch {
    inboxPath = null
  }
  if (inboxPath && relpath === inboxPath) return true
  try {
    const raw = await api.kernel.invoke(STORAGE_PLUGIN, COMMAND_QUERY_TAGS, {
      name: INBOX_TAG,
    })
    if (Array.isArray(raw)) {
      for (const item of raw as TagResult[]) {
        if (item && typeof item.file_path === 'string' && item.file_path === relpath) {
          return true
        }
      }
    }
  } catch {
    /* tag lookup failed — fall through to false */
  }
  return false
}

export function attachRuntime(api: PluginAPI): () => void {
  const off = api.events.on<FileSavedPayload>('files:saved', (payload) => {
    if (!payload || !payload.relpath) return
    const relpath = payload.relpath
    if (!isMarkdown(relpath)) return

    // Throttle: replace any pending timer for this file with a fresh
    // 5s window. The user typing a few quick saves coalesces into
    // exactly one proposal request.
    const existing = pending.get(relpath)
    if (existing) clearTimeout(existing)
    const handle = setTimeout(() => {
      pending.delete(relpath)
      void runProposalIfInScope(api, relpath)
    }, THROTTLE_MS)
    pending.set(relpath, handle)
  })
  return () => {
    off()
    cancelAllPending()
  }
}

async function runProposalIfInScope(api: PluginAPI, relpath: string): Promise<void> {
  const inScope = await isInInboxScope(api, relpath)
  if (!inScope) return
  await runProposal(api, relpath)
}

async function runProposal(api: PluginAPI, relpath: string): Promise<void> {
  try {
    const raw = await api.kernel.invoke(AI_PLUGIN, COMMAND_ENRICH_FILE, {
      path: relpath,
    })
    const proposal = raw as EnrichmentProposal
    if (!proposal || typeof proposal.body_hash !== 'string') {
      // Malformed response — ignore.
      return
    }
    useEnrichStore.getState().setPending(proposal)
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    // Soft-fail: store the error on the panel but don't toast — most
    // failures are "no AI provider configured" which is a user-facing
    // setting, not a runtime bug.
    useEnrichStore.getState().setError(msg)
  }
}

/**
 * Force an enrichment proposal for the currently-active editor tab,
 * bypassing the inbox-scope filter. Used by the
 * `nexus.enrich.force` palette command.
 */
export async function forceEnrichActiveFile(api: PluginAPI): Promise<void> {
  const active = api.editor.active()
  if (!active) {
    api.notifications.show({
      type: 'warning',
      message: 'No active editor tab to enrich.',
    })
    return
  }
  if (!isMarkdown(active.relpath)) {
    api.notifications.show({
      type: 'warning',
      message: 'Enrichment only applies to markdown files.',
    })
    return
  }
  await runProposal(api, active.relpath)
}

/**
 * AIG-06 — which fields of the head proposal to apply. Selecting a
 * subset sends a filtered proposal where omitted fields are blanked
 * (`""` / `[]`). The kernel's `merge_frontmatter` treats those empty
 * values as "leave existing alone" rather than "delete" — see
 * `crates/nexus-ai/src/enrichment.rs::merge_frontmatter`.
 */
export type EnrichFieldSelection =
  | 'all'
  | 'tags'
  | 'summary'
  | 'related'

/**
 * Apply the head (oldest) pending proposal, optionally filtered to
 * a single field. Defaults to `'all'` for backwards compatibility
 * with the original button.
 */
export async function applyPending(
  api: PluginAPI,
  fields: EnrichFieldSelection = 'all',
): Promise<void> {
  const state = useEnrichStore.getState()
  const proposal = state.pending.values().next().value
  if (!proposal) return
  const filtered = filterProposal(proposal, fields)
  state.setApplying(true)
  try {
    const raw = await api.kernel.invoke(AI_PLUGIN, COMMAND_ENRICH_APPLY, {
      proposal: filtered,
    })
    const result = raw as { applied: boolean; reason?: string | null }
    if (result.applied) {
      useEnrichStore.getState().dismiss(proposal.path)
      api.notifications.show({
        type: 'info',
        message: messageForFields(fields, proposal.path),
      })
    } else {
      useEnrichStore.getState().setError(
        result.reason ?? 'enrich_apply rejected the proposal',
      )
      useEnrichStore.getState().setApplying(false)
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    useEnrichStore.getState().setError(msg)
    useEnrichStore.getState().setApplying(false)
  }
}

/** Pure helper — exported so unit tests can verify the wire shape
 *  without driving `applyPending` through a mock kernel. */
export function filterProposal(
  proposal: EnrichmentProposal,
  fields: EnrichFieldSelection,
): EnrichmentProposal {
  if (fields === 'all') return proposal
  return {
    ...proposal,
    tags: fields === 'tags' ? proposal.tags : [],
    summary: fields === 'summary' ? proposal.summary : '',
    related: fields === 'related' ? proposal.related : [],
  }
}

function messageForFields(fields: EnrichFieldSelection, path: string): string {
  switch (fields) {
    case 'tags':
      return `Applied tags to ${path}`
    case 'summary':
      return `Applied summary to ${path}`
    case 'related':
      return `Applied related links to ${path}`
    case 'all':
      return `Enriched ${path}`
  }
}
