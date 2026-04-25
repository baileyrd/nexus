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
