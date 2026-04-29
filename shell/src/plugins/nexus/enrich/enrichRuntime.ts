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
      void runProposal(api, relpath)
    }, THROTTLE_MS)
    pending.set(relpath, handle)
  })
  return () => {
    off()
    cancelAllPending()
  }
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

/** Apply the currently-pending proposal. UI handler. */
export async function applyPending(api: PluginAPI): Promise<void> {
  const state = useEnrichStore.getState()
  const proposal = state.pending
  if (!proposal) return
  state.setApplying(true)
  try {
    const raw = await api.kernel.invoke(AI_PLUGIN, COMMAND_ENRICH_APPLY, {
      proposal,
    })
    const result = raw as { applied: boolean; reason?: string | null }
    if (result.applied) {
      useEnrichStore.getState().dismiss()
      api.notifications.show({
        type: 'info',
        message: `Enriched ${proposal.path}`,
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
