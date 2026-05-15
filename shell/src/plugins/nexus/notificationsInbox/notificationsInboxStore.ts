import { create } from 'zustand'

/**
 * BL-136 Phase 2 — store for the Notification Center.
 *
 * Mirrors the SQLite-backed inbox owned by `nexus-notifications`
 * (Phase 1). Hydrated via `com.nexus.notifications::inbox_list` and
 * kept in sync through two bus topics:
 *
 *   - `com.nexus.notifications.inbox.appended`  (insert)
 *   - `com.nexus.notifications.delivered`        (live toast surface;
 *      used as a back-compat hint that an inbox row is about to
 *      appear — the append topic is authoritative).
 *
 * Wire shape matches `InboxEntry` in `crates/nexus-notifications/src/inbox.rs`.
 * We hand-define the type here so the store compiles when the
 * notifications plugin is disabled (the IPC handler returns
 * `inbox not wired` in that case, which we catch and treat as empty).
 */

/** Severity wire tags. Matches `nexus-notifications::config::Severity`. */
export type InboxSeverity = 'debug' | 'info' | 'warn' | 'error'

/** Channel wire tags. Matches `nexus-notifications::Channel`. */
export type InboxChannel = 'desktop' | 'discord' | 'telegram' | 'email'

/** One row from `inbox_list`. */
export interface InboxEntry {
  id: string
  source: string
  severity: InboxSeverity
  title: string | null
  body: string
  channels: InboxChannel[]
  ts: number
  read_at: number | null
  dismissed_at: number | null
  payload_json: string | null
}

/** Reply shape for `inbox_stats`. */
export interface InboxStats {
  total: number
  unread: number
  by_source: Record<string, number>
}

/** Cap on entries kept in the store. Matches the Rust-side default
 *  (`crates/nexus-notifications/src/inbox.rs::DEFAULT_MAX_ROWS`). */
export const INBOX_ENTRIES_CAP = 1000

interface NotificationsInboxState {
  /** Newest entry first. */
  entries: InboxEntry[]
  /** Source filter — `null` = all sources. */
  sourceFilter: string | null
  /** `true` once the initial hydrate has resolved (so the panel can
   *  swap "Loading…" for the empty-state). */
  hydrated: boolean

  hydrate(entries: InboxEntry[]): void
  prepend(entry: InboxEntry): void
  markRead(ids: string[]): void
  markDismissed(ids: string[]): void
  setSourceFilter(s: string | null): void
  clear(): void
}

export const useNotificationsInboxStore = create<NotificationsInboxState>(
  (set) => ({
    entries: [],
    sourceFilter: null,
    hydrated: false,

    hydrate: (entries) =>
      set({
        entries: entries.slice(0, INBOX_ENTRIES_CAP),
        hydrated: true,
      }),

    prepend: (entry) =>
      set((s) => {
        // Dedup by id — the appended-topic fire is the authoritative
        // signal but a workspace reopen + concurrent insert could
        // race. Cheap O(n) since N <= cap.
        if (s.entries.some((e) => e.id === entry.id)) return s
        const next =
          s.entries.length >= INBOX_ENTRIES_CAP
            ? [entry, ...s.entries.slice(0, INBOX_ENTRIES_CAP - 1)]
            : [entry, ...s.entries]
        return { entries: next }
      }),

    markRead: (ids) =>
      set((s) => {
        const setIds = new Set(ids)
        const now = Math.floor(Date.now() / 1000)
        return {
          entries: s.entries.map((e) =>
            setIds.has(e.id) && e.read_at === null ? { ...e, read_at: now } : e,
          ),
        }
      }),

    markDismissed: (ids) =>
      set((s) => {
        const setIds = new Set(ids)
        const now = Math.floor(Date.now() / 1000)
        return {
          entries: s.entries.map((e) => {
            if (!setIds.has(e.id) || e.dismissed_at !== null) return e
            return {
              ...e,
              dismissed_at: now,
              read_at: e.read_at ?? now,
            }
          }),
        }
      }),

    setSourceFilter: (s) => set({ sourceFilter: s }),
    clear: () => set({ entries: [], sourceFilter: null }),
  }),
)

/**
 * Coarse stats derived from the local store. The IPC `inbox_stats`
 * handler returns the same shape against the SQLite store; the
 * panel uses the local view so it stays consistent with the live
 * `prepend`/`markRead` updates without re-querying after every
 * mutation.
 */
export function deriveStats(entries: InboxEntry[]): InboxStats {
  let unread = 0
  let total = 0
  const bySource: Record<string, number> = {}
  for (const e of entries) {
    if (e.dismissed_at !== null) continue
    total += 1
    if (e.read_at === null) {
      unread += 1
      bySource[e.source] = (bySource[e.source] ?? 0) + 1
    }
  }
  return { total, unread, by_source: bySource }
}

/**
 * Parse the optional `payload_json` blob into a `{ task_id?: string }`
 * shape — returns null when the column is empty or malformed. Used by
 * the jump-to-source button to decide whether to surface the action.
 */
export function parsePayloadTaskId(payload: string | null): string | null {
  if (!payload) return null
  try {
    const obj = JSON.parse(payload)
    if (obj && typeof obj === 'object' && typeof obj.task_id === 'string') {
      return obj.task_id
    }
  } catch {
    // ignore — malformed payload doesn't crash the panel
  }
  return null
}
