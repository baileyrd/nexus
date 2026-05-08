// shell/src/plugins/nexus/terminal/savedCommandsStore.ts
//
// WI-05 — view-model for the Saved Commands sub-view of nexus.terminal.
//
// Wraps the kernel `com.nexus.terminal::saved_*` handlers (CRUD + reorder)
// and caches the row list locally so React renders are synchronous.
//
// Authoritative shape lives in `crates/nexus-terminal/src/saved.rs`
// (`pub struct SavedCommand`). The fields below mirror the serde
// representation (snake_case) so we can pass the rows back to
// `saved_create` / `saved_update` round-trip without any conversion.
//
// All mutations go: optimistic-call → kernel → reload via `saved_list`
// (single round-trip). Reload-after-write is the simplest contract that
// keeps the UI consistent with sqlite (the kernel writes immediately
// and `list()` reads back ordered by `sidebar_order`); a more granular
// "patch in place" path can land later if it becomes a perf concern.

import { create } from 'zustand'
import { configStore } from '../../../stores/configStore'

/**
 * Mirror of `crates/nexus-terminal/src/saved.rs::SavedCommand`.
 *
 * Field naming matches the serde JSON representation (snake_case) so
 * rows round-trip through `saved_create` / `saved_update` verbatim.
 * Optional fields use `null` (matching `Option<T>` over JSON-IPC) or a
 * default empty value where the Rust side derives `Default`.
 */
export interface SavedCommand {
  /** URL-safe primary key. */
  slug: string
  /** Human-readable label. */
  name: string
  /** Absolute shell path; empty string lets the spawn pick the default. */
  shell: string
  /** Full command (may include `&&` / `||` / `;`). */
  shell_cmd: string
  /** Working directory. */
  working_dir: string | null
  /** Per-command env vars. */
  env_vars: Record<string, string>
  /** Optional `.env` file override. */
  env_file: string | null
  /** Icon tag the sidebar renders. */
  icon: string
  /** Whether crashed processes auto-restart. */
  auto_restart: boolean
  /** First-restart delay (ms). */
  auto_restart_delay_ms: number
  /** Hard memory cap (MB) before kill. */
  memory_limit_mb: number | null
  /** Drag-reorder index; lower sorts earlier. */
  sidebar_order: number | null
  /** Ordered pre-command chain. */
  pre_commands: string[]
  /** Unix-seconds creation time. */
  created_at: number
  /** Unix-seconds last-updated time. */
  updated_at: number
}

/** Fields the editor form mutates. The rest of `SavedCommand` is either
 *  derived (timestamps), defaulted (auto_restart_*, env_*) or owned by
 *  the reorder path (sidebar_order). */
export type SavedCommandDraft = Pick<
  SavedCommand,
  'slug' | 'name' | 'shell' | 'shell_cmd' | 'working_dir' | 'icon'
>

/** Subset of PluginAPI we actually need. Typed structurally so the store
 *  can be unit-tested with a minimal mock — no need to fabricate the
 *  full `PluginAPI` aggregate. */
export interface SavedKernelAPI {
  invoke<T = unknown>(pluginId: string, commandId: string, args?: unknown): Promise<T>
}

const PLUGIN_ID = 'com.nexus.terminal'
// Handler ids verified at crates/nexus-terminal/src/core_plugin.rs L67-96.
const CMD_LIST = 'saved_list'
const CMD_CREATE = 'saved_create'
const CMD_UPDATE = 'saved_update'
const CMD_DELETE = 'saved_delete'
const CMD_REORDER = 'saved_reorder'
// BL-066 follow-up — managed-session lifecycle verbs. `run_saved`
// (BL-055) spawns a fresh PTY session named `saved:<slug>`;
// `list_sessions` + `close_session` are the standard terminal verbs.
const CMD_LIST_SESSIONS = 'list_sessions'
const CMD_CLOSE_SESSION = 'close_session'
const CMD_RUN_SAVED = 'run_saved'

/** BL-066 follow-up — convention from BL-055's `run_saved` handler:
 *  every session it spawns is named `saved:<slug>`. The shell-side
 *  poller relies on this string being stable to attribute live
 *  sessions back to their originating saved command. */
export const SAVED_SESSION_NAME_PREFIX = 'saved:'

/** Wire shape of a single row from `com.nexus.terminal::list_sessions`.
 *  Mirrors `crates/nexus-terminal/src/server.rs::SessionInfo`; only the
 *  fields the running-state poller cares about are kept here. */
export interface RunningSessionRow {
  id: string
  name: string
}

const AUTO_RESTART_DELAY_MS = 2_000
const DEFAULT_ICON = 'terminal'

interface SavedCommandsState {
  /** Cache of `saved_list` output, ordered as the kernel returned them. */
  commands: SavedCommand[]
  /** True once `loadSaved` has completed at least once for this session. */
  loaded: boolean
  /** Last error surfaced by a kernel call; cleared on the next success. */
  error: string | null

  loadSaved(api: SavedKernelAPI): Promise<void>
  createSaved(api: SavedKernelAPI, draft: SavedCommandDraft): Promise<void>
  updateSaved(api: SavedKernelAPI, draft: SavedCommandDraft): Promise<void>
  deleteSaved(api: SavedKernelAPI, slug: string): Promise<void>
  /**
   * Reorder by sending one `saved_reorder` per moved row. The kernel
   * stores `sidebar_order` per-row (no batch endpoint), so we walk
   * `orderedSlugs` and assign positions 0..N-1 in order. A single
   * `loadSaved` at the end normalises the cache.
   */
  reorderSaved(api: SavedKernelAPI, orderedSlugs: string[]): Promise<void>
  /** Clear all cached state. Used by workspace:closed. */
  reset(): void
}

/**
 * Build a full `SavedCommand` from a draft + defaults. The kernel's
 * `saved_create` handler deserialises into `SavedCommand` directly
 * (`serde_json::from_value`) so every field has to be present —
 * `#[serde(default)]` covers a few but not enough to make `Pick` work
 * unaided.
 */
function draftToRow(draft: SavedCommandDraft): SavedCommand {
  const now = Math.floor(Date.now() / 1000)
  return {
    slug: draft.slug,
    name: draft.name,
    shell: draft.shell,
    shell_cmd: draft.shell_cmd,
    working_dir: draft.working_dir,
    env_vars: {},
    env_file: null,
    icon: draft.icon || DEFAULT_ICON,
    auto_restart: false,
    auto_restart_delay_ms: configStore.get('terminal.autoRestartDelayMs', AUTO_RESTART_DELAY_MS) ?? AUTO_RESTART_DELAY_MS,
    memory_limit_mb: null,
    sidebar_order: null,
    pre_commands: [],
    created_at: now,
    updated_at: now,
  }
}

/** Merge an edited draft back onto an existing row so create/update
 *  preserve fields the editor doesn't expose (env, auto_restart, …). */
function mergeDraft(existing: SavedCommand, draft: SavedCommandDraft): SavedCommand {
  return {
    ...existing,
    name: draft.name,
    shell: draft.shell,
    shell_cmd: draft.shell_cmd,
    working_dir: draft.working_dir,
    icon: draft.icon || existing.icon || DEFAULT_ICON,
    updated_at: Math.floor(Date.now() / 1000),
  }
}

export const useSavedCommandsStore = create<SavedCommandsState>((set, get) => ({
  commands: [],
  loaded: false,
  error: null,

  loadSaved: async (api) => {
    try {
      const rows = await api.invoke<SavedCommand[]>(PLUGIN_ID, CMD_LIST)
      set({ commands: rows ?? [], loaded: true, error: null })
    } catch (err) {
      set({ error: String(err) })
    }
  },

  createSaved: async (api, draft) => {
    const row = draftToRow(draft)
    try {
      await api.invoke<SavedCommand>(PLUGIN_ID, CMD_CREATE, row)
      await get().loadSaved(api)
    } catch (err) {
      set({ error: String(err) })
      throw err
    }
  },

  updateSaved: async (api, draft) => {
    const existing = get().commands.find((c) => c.slug === draft.slug)
    const row = existing ? mergeDraft(existing, draft) : draftToRow(draft)
    try {
      await api.invoke<SavedCommand>(PLUGIN_ID, CMD_UPDATE, row)
      await get().loadSaved(api)
    } catch (err) {
      set({ error: String(err) })
      throw err
    }
  },

  deleteSaved: async (api, slug) => {
    try {
      await api.invoke(PLUGIN_ID, CMD_DELETE, { slug })
      // Optimistic local prune so the row vanishes before the refresh
      // round-trip lands. `loadSaved` then rewrites the cache from
      // truth — if the delete somehow failed server-side it'll come
      // back, which is the correct convergence.
      set((s) => ({ commands: s.commands.filter((c) => c.slug !== slug) }))
      await get().loadSaved(api)
    } catch (err) {
      set({ error: String(err) })
      throw err
    }
  },

  reorderSaved: async (api, orderedSlugs) => {
    try {
      // Kernel takes one slug+order at a time. Walk in order so
      // `sidebar_order` ends up dense (0..N-1). The reorder handler
      // accepts an Option<i32>; we always send a concrete index.
      for (let i = 0; i < orderedSlugs.length; i += 1) {
        await api.invoke(PLUGIN_ID, CMD_REORDER, {
          slug: orderedSlugs[i],
          sidebar_order: i,
        })
      }
      await get().loadSaved(api)
    } catch (err) {
      set({ error: String(err) })
      throw err
    }
  },

  reset: () => set({ commands: [], loaded: false, error: null }),
}))

// ── BL-066 follow-up — running-session tracking + lifecycle helpers ──────────

/**
 * Walk a list of `list_sessions` rows and bucket every
 * `saved:<slug>`-named session into a `Record<slug, sessionId[]>` map.
 * Rows whose name doesn't carry the prefix or whose slug is empty are
 * skipped. Multiple sessions for the same slug are preserved in
 * encounter order — the Stop button operates on every id, the Restart
 * button operates on the last (most recent) one.
 *
 * Pure function — exported for unit tests so the routing matrix
 * stays nailed without standing up a full kernel mock.
 */
export function extractRunningSavedSessions(
  sessions: ReadonlyArray<RunningSessionRow>,
): Record<string, string[]> {
  const out: Record<string, string[]> = {}
  for (const row of sessions) {
    if (typeof row?.name !== 'string') continue
    if (!row.name.startsWith(SAVED_SESSION_NAME_PREFIX)) continue
    const slug = row.name.slice(SAVED_SESSION_NAME_PREFIX.length)
    if (!slug) continue
    ;(out[slug] ??= []).push(row.id)
  }
  return out
}

/** BL-066 follow-up — fetch the live `list_sessions` snapshot and bucket
 *  it by slug. Errors are surfaced to the caller; the view's poller
 *  catches and ignores them so a transient kernel hiccup doesn't
 *  flicker the UI. */
export async function fetchRunningSavedSessions(
  api: SavedKernelAPI,
): Promise<Record<string, string[]>> {
  const sessions = await api.invoke<RunningSessionRow[]>(
    PLUGIN_ID,
    CMD_LIST_SESSIONS,
  )
  return extractRunningSavedSessions(sessions ?? [])
}

/** BL-066 follow-up — spawn a fresh managed PTY session for a saved
 *  command. Routes through BL-055's `run_saved`; the kernel names the
 *  session `saved:<slug>` so the next poll picks it up via
 *  {@link extractRunningSavedSessions}. */
export async function spawnSavedSession(
  api: SavedKernelAPI,
  slug: string,
): Promise<void> {
  await api.invoke(PLUGIN_ID, CMD_RUN_SAVED, { slug })
}

/** BL-066 follow-up — close every PTY session whose name matches
 *  `saved:<slug>`. Closes are issued sequentially (cheap; the slug's
 *  session count is tiny in practice) so a partial failure is
 *  observable through the rejection. */
export async function stopSavedSession(
  api: SavedKernelAPI,
  sessionIds: ReadonlyArray<string>,
): Promise<void> {
  for (const id of sessionIds) {
    await api.invoke(PLUGIN_ID, CMD_CLOSE_SESSION, { id })
  }
}

/** BL-066 follow-up — Stop + spawn. The stop step waits for every
 *  matching session to close before the new one is requested so the
 *  user-visible session count cleanly transitions from N → 0 → 1. */
export async function restartSavedSession(
  api: SavedKernelAPI,
  slug: string,
  sessionIds: ReadonlyArray<string>,
): Promise<void> {
  await stopSavedSession(api, sessionIds)
  await spawnSavedSession(api, slug)
}
