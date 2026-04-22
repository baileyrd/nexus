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
} as const

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
}

/**
 * Factory helper for callers that prefer composition over `new`. Mostly a
 * convenience so tests can write `makeEditorClient(mockApi)` alongside
 * their other fixture builders.
 */
export function makeEditorClient(api: KernelAPI): EditorKernelClient {
  return new EditorKernelClient(api)
}
