// Defensive decoders for `com.nexus.comments` IPC responses. The
// kernel returns well-typed JSON, but the shell crosses a Tauri
// boundary and we'd rather drop a malformed thread than crash the
// pane.

import type { Comment, Thread } from './types'

function isString(x: unknown): x is string {
  return typeof x === 'string'
}

function decodeStringArray(raw: unknown): string[] {
  if (!Array.isArray(raw)) return []
  const out: string[] = []
  for (const item of raw) if (isString(item)) out.push(item)
  return out
}

export function decodeComment(raw: unknown): Comment | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (!isString(r.id)) return null
  if (!isString(r.body)) return null
  if (!isString(r.created_at)) return null
  return {
    id: r.id,
    author: isString(r.author) ? r.author : undefined,
    body: r.body,
    mentions: decodeStringArray(r.mentions),
    created_at: r.created_at,
    updated_at: isString(r.updated_at) ? r.updated_at : undefined,
  }
}

export function decodeThread(raw: unknown): Thread | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (!isString(r.id)) return null
  if (!isString(r.block_id)) return null
  if (!isString(r.created_at)) return null
  if (!Array.isArray(r.comments)) return null
  const comments: Comment[] = []
  for (const c of r.comments) {
    const decoded = decodeComment(c)
    if (decoded) comments.push(decoded)
  }
  // The kernel guarantees at least one comment per thread; an empty
  // list after decode means the wire payload was malformed.
  if (comments.length === 0) return null
  return {
    id: r.id,
    block_id: r.block_id,
    resolved: r.resolved === true,
    resolved_at: isString(r.resolved_at) ? r.resolved_at : undefined,
    resolved_by: isString(r.resolved_by) ? r.resolved_by : undefined,
    created_at: r.created_at,
    comments,
  }
}

export function decodeThreadList(raw: unknown): Thread[] {
  if (!Array.isArray(raw)) return []
  const out: Thread[] = []
  for (const item of raw) {
    const t = decodeThread(item)
    if (t) out.push(t)
  }
  return out
}
