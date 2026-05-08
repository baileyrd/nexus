// Typed wrapper over the `com.nexus.editor` kernel IPC surface.
//
// All disk-bound and tree-mutating editor state is owned by the Rust
// core plugin; this client hides the raw `api.kernel.invoke('com.nexus.editor',
// '<cmd>', args)` plumbing from view-layer callers. Command strings match
// the mapping in `crates/nexus-bootstrap/src/lib.rs:431-459` — do not
// rename them without updating the bootstrap manifest.

import type { KernelAPI } from '../../../types/plugin.ts'
import type { EditorSnapshot, Transaction } from './types.ts'

/** Reverse-DNS id of the editor core plugin (see `core_plugin.rs:38`). */
export const EDITOR_PLUGIN_ID = 'com.nexus.editor'

/** Reverse-DNS id of the storage core plugin. The editor client
 *  mostly talks to `com.nexus.editor`, but a handful of read/write
 *  paths reach for storage directly — `update_base_record` (record
 *  mutation from the inline `[[{db:…}]]` widget) is the first. */
const STORAGE_PLUGIN_ID = 'com.nexus.storage'

// Command strings exposed by the bootstrap manifest.
const CMD = {
  open: 'open',
  close: 'close',
  getTree: 'get_tree',
  save: 'save',
  applyTransaction: 'apply_transaction',
  undo: 'undo',
  redo: 'redo',
  getMarkdown: 'get_markdown',
  stampBlock: 'stamp_block',
  executeDatabaseView: 'execute_database_view',
  resolveBlockLink: 'resolve_block_link',
} as const

/** Result of a `stamp_block` IPC call. Mirrors the Rust handler's
 *  return shape — `block_id` is the lookup id (post-rekey it equals
 *  `stable_id` for newly-stamped blocks); `stable_id` is the persistent
 *  id that ends up in the on-disk `<!-- ^<uuid> -->` marker. */
export interface StampBlockResult {
  block_id: string
  stable_id: string
  newly_stamped: boolean
}

/**
 * Client that exposes the editor plugin's IPC surface as typed methods.
 *
 * Takes the `KernelAPI` as a constructor argument so unit tests can mock
 * `invoke` directly.
 */
export class EditorKernelClient {
  private readonly api: KernelAPI

  constructor(api: KernelAPI) {
    this.api = api
  }

  /** Open a session and return the initial snapshot. */
  openSession(relpath: string): Promise<EditorSnapshot> {
    return this.api.invoke<EditorSnapshot>(EDITOR_PLUGIN_ID, CMD.open, {
      relpath,
    })
  }

  /** Close a session, dropping its in-memory tree and undo history. */
  async closeSession(relpath: string): Promise<void> {
    await this.api.invoke(EDITOR_PLUGIN_ID, CMD.close, { relpath })
  }

  /** Fetch the current snapshot for an already-open session. */
  getTree(relpath: string): Promise<EditorSnapshot> {
    return this.api.invoke<EditorSnapshot>(EDITOR_PLUGIN_ID, CMD.getTree, {
      relpath,
    })
  }

  /** Apply a transaction and return the post-state snapshot. */
  applyTransaction(
    relpath: string,
    transaction: Transaction,
  ): Promise<EditorSnapshot> {
    return this.api.invoke<EditorSnapshot>(
      EDITOR_PLUGIN_ID,
      CMD.applyTransaction,
      { relpath, transaction },
    )
  }

  /** Move the session's undo cursor one step backward. */
  undo(relpath: string): Promise<EditorSnapshot> {
    return this.api.invoke<EditorSnapshot>(EDITOR_PLUGIN_ID, CMD.undo, {
      relpath,
    })
  }

  /** Move the session's undo cursor one step forward. */
  redo(relpath: string): Promise<EditorSnapshot> {
    return this.api.invoke<EditorSnapshot>(EDITOR_PLUGIN_ID, CMD.redo, {
      relpath,
    })
  }

  /** Persist the session's block tree to disk via `com.nexus.storage`. */
  async saveSession(relpath: string): Promise<void> {
    await this.api.invoke(EDITOR_PLUGIN_ID, CMD.save, { relpath })
  }

  /**
   * Return the canonical markdown serialization of the session's block
   * tree — the exact text `save` would write to disk. Used by the shell
   * to hydrate tab content without a parallel `storage::read_file`, so
   * the rendered text round-trips through the same parser/serializer
   * pair as the on-disk form.
   */
  getMarkdown(relpath: string): Promise<string> {
    return this.api.invoke<string>(EDITOR_PLUGIN_ID, CMD.getMarkdown, {
      relpath,
    })
  }

  /**
   * Promote `blockId` in `relpath` to a stable id (ADR 0017). Returns
   * `{ block_id, stable_id, newly_stamped }`. Idempotent: a second call
   * for the same block returns the existing `stable_id`.
   *
   * Comments and block-link callers anchor to `stable_id` so the
   * reference survives upstream block insertions.
   */
  stampBlock(relpath: string, blockId: string): Promise<StampBlockResult> {
    return this.api.invoke<StampBlockResult>(EDITOR_PLUGIN_ID, CMD.stampBlock, {
      relpath,
      block_id: blockId,
    })
  }

  /**
   * Resolve an inline `[[{db:query}]]` block by loading the target
   * `.bases` directory and running its [`DatabaseViewConfig`] through
   * `com.nexus.database::apply_view`. Returns the structured view layout
   * (`applied`) plus the base's [`BaseSchema`] so the renderer can format
   * cells without a second IPC roundtrip.
   *
   * Read-only — does not touch any editor session and emits no
   * `com.nexus.editor.changed.*` event. BL-012 split 1 backs this.
   */
  executeDatabaseView(
    databasePath: string,
    viewConfig: DatabaseViewConfig,
  ): Promise<ExecuteDatabaseViewResponse> {
    // BL-069 DoD: explicit 30 s budget for large datasets. The
    // default kernel timeout is also 30 s, but the spec calls for
    // an explicit value so a future default change can't silently
    // tighten the budget under the renderer.
    return this.api.invoke<ExecuteDatabaseViewResponse>(
      EDITOR_PLUGIN_ID,
      CMD.executeDatabaseView,
      { database_path: databasePath, view_config: viewConfig },
      30_000,
    )
  }

  /**
   * Resolve a `[[<file>#^<block-id>]]` link (BL-049). Returns
   * `{ found, block, root_index }` — `root_index` is the position
   * in `tree.root_blocks` of the target block's root ancestor, used
   * by the navigation UX to scroll the opened tab to the right
   * vicinity. The kernel handler reads from the open session if
   * one exists, otherwise parses the file from disk transiently.
   */
  resolveBlockLink(
    fileRelpath: string,
    blockId: string,
  ): Promise<ResolveBlockLinkResponse> {
    return this.api.invoke<ResolveBlockLinkResponse>(
      EDITOR_PLUGIN_ID,
      CMD.resolveBlockLink,
      { file_relpath: fileRelpath, block_id: blockId },
    )
  }

  /**
   * Update a single record's fields inside a `.bases` directory
   * (`databasePath`) — wraps `com.nexus.storage::base_record_update`
   * (handler 41). Used by the BL-069 inline database-view widget for
   * kanban drag-to-reorder + (future) cell editing; the widget
   * already has this client injected, so threading through a
   * separate bases-plugin client would be churn.
   *
   * `fields` is a sparse field map: any key listed here replaces the
   * existing value, omitted keys keep their previous value. The
   * storage handler returns the full updated `BaseRecord`; we type
   * the response as `unknown` because the editor's renderer doesn't
   * need the typed shape — the widget invalidates its cache and
   * re-fetches via `executeDatabaseView` to get the freshest layout.
   */
  updateBaseRecord(
    databasePath: string,
    recordId: string,
    fields: Record<string, unknown>,
  ): Promise<unknown> {
    return this.api.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_record_update', {
      path: databasePath,
      record_id: recordId,
      fields,
    })
  }
}

/** Response shape of `resolve_block_link` — mirrors the Rust
 *  handler in `crates/nexus-editor/src/core_plugin.rs`. `block` is
 *  null when `found === false`. */
export interface ResolveBlockLinkResponse {
  found: boolean
  block: unknown | null
  root_index: number | null
}

// ── execute_database_view wire types ────────────────────────────────────────

/** Visual layout variants for an inline database-view block — mirrors
 *  the Rust `DatabaseViewType` discriminated union (snake_case `kind`).
 *  See `crates/nexus-editor/src/block.rs:340`. */
export type DatabaseViewType =
  | { kind: 'table' }
  | { kind: 'kanban'; column_by: string }
  | { kind: 'calendar'; date_field: string }
  | { kind: 'gallery'; title_field: string }
  | { kind: 'custom'; 0: string }

/** Config for a `BlockType::DatabaseView` — mirrors the Rust
 *  `DatabaseViewConfig`. Filters and sorts are user-typed strings; the
 *  Rust executor (BL-012 split 1) parses them into structured rules
 *  before handing off to `apply_view`. */
export interface DatabaseViewConfig {
  view_type: DatabaseViewType
  filters: string[]
  sorts: string[]
  group_by: string | null
  hidden_columns: string[]
}

/** A single record after `apply_view` ran. Field map follows
 *  `nexus_types::bases::BaseRecord` — `id` plus arbitrary user fields
 *  spread at the top level via `#[serde(flatten)]`. */
export interface AppliedRecord {
  id: string
  deletedAt?: number | null
  [field: string]: unknown
}

/** A grouped bucket emitted by Kanban / Calendar / List / Timeline
 *  layouts — `key` is the discriminator value (or the `(none)`
 *  sentinel from `MISSING_GROUP_KEY` in the Rust side). */
export interface AppliedGroup {
  key: string
  records: AppliedRecord[]
}

/** Layout payload returned by `apply_view`. Either a flat record list
 *  (Table / Gallery) or a list of grouped buckets (Kanban / Calendar /
 *  List / Timeline). */
export type AppliedLayout =
  | { kind: 'flat'; records: AppliedRecord[] }
  | { kind: 'grouped'; groups: AppliedGroup[] }

/** Result of `apply_view` — preserves the view metadata so the
 *  renderer can pick a layout-specific component without reparsing
 *  the source config. */
export interface AppliedView {
  view_name: string
  view_type: 'table' | 'kanban' | 'calendar' | 'gallery' | 'list' | 'timeline'
  fields: string[]
  layout: AppliedLayout
}

/** Response shape of `execute_database_view` — mirrors the Rust
 *  `crates/nexus-editor/src/database_view.rs:ExecuteDatabaseViewResponse`. */
export interface ExecuteDatabaseViewResponse {
  applied: AppliedView
  schema: {
    version: string
    fields: Record<string, unknown>
  }
}

/**
 * Factory helper for callers that prefer composition over `new`. Mostly a
 * convenience so tests can write `makeEditorClient(mockApi)` alongside
 * their other fixture builders.
 */
export function makeEditorClient(api: KernelAPI): EditorKernelClient {
  return new EditorKernelClient(api)
}
