// BL-074 — apply-resolution helpers extracted from ConflictModal so
// the IPC plumbing can be tested independently of the React render
// surface.

import type { PluginAPI } from '../../../types/plugin'
import type { ConflictDetail } from './types'

const EDITOR_PLUGIN_ID = 'com.nexus.editor'
const APPLY_TX_COMMAND = 'apply_transaction'

/** Generate a v4-shaped UUID for transaction ids. The editor's
 *  `Transaction` schema requires an `id` field; the value isn't
 *  inspected by the resolver path so any well-formed UUID works. */
export function uuidV4(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  // Fallback for stripped-down test envs — not cryptographically
  // strong, but valid v4 shape.
  const hex = '0123456789abcdef'
  const rand = (n: number) =>
    Array.from({ length: n }, () => hex[Math.floor(Math.random() * 16)]).join('')
  return `${rand(8)}-${rand(4)}-4${rand(3)}-${'89ab'[Math.floor(Math.random() * 4)]}${rand(3)}-${rand(12)}`
}

/** Build the wire-shape `Transaction` that overwrites `block_id`'s
 *  content with the remote payload. Exported so unit tests can pin
 *  the exact payload the modal dispatches. */
export function buildUseRemoteTransaction(
  blockId: string,
  localContent: string,
  remoteContent: string,
  txId: string = uuidV4(),
  now: number = Date.now(),
): {
  id: string
  operations: Array<{
    kind: 'update_block_content'
    id: string
    old_content: string
    new_content: string
    old_annotations: never[]
    new_annotations: never[]
  }>
  created_at: number
  metadata: {
    user_action: { kind: 'paste' }
    source: 'user'
    ai_edit: false
  }
} {
  return {
    id: txId,
    operations: [
      {
        kind: 'update_block_content' as const,
        id: blockId,
        old_content: localContent,
        new_content: remoteContent,
        old_annotations: [] as never[],
        new_annotations: [] as never[],
      },
    ],
    created_at: now,
    metadata: {
      user_action: { kind: 'paste' as const },
      source: 'user' as const,
      ai_edit: false as const,
    },
  }
}

/** Apply the user's "Use remote" choice for a `concurrent_block_edit`
 *  conflict — dispatches a fresh `UpdateBlockContent` transaction
 *  against the editor IPC. Returns null on success, the error
 *  message on failure.
 *
 *  For `structural_delete_edit` conflicts this is a no-op and
 *  returns the explanatory error — v1 doesn't auto-resolve those
 *  cases (re-creating a deleted block or re-issuing a delete after
 *  re-creation needs more thought; the user goes through the editor). */
export async function applyUseRemote(
  api: PluginAPI,
  relpath: string,
  detail: ConflictDetail,
): Promise<string | null> {
  if (detail.kind !== 'concurrent_block_edit') {
    return 'Only concurrent block edits can be auto-resolved.'
  }
  const local = detail.local_content
  const remote = detail.remote_content
  if (typeof local !== 'string' || typeof remote !== 'string') {
    return 'Conflict payload is missing the content snapshots needed to resolve.'
  }
  const transaction = buildUseRemoteTransaction(detail.block_id, local, remote)
  try {
    await api.kernel.invoke(EDITOR_PLUGIN_ID, APPLY_TX_COMMAND, {
      relpath,
      transaction,
    })
    return null
  } catch (err) {
    return err instanceof Error ? err.message : String(err)
  }
}
