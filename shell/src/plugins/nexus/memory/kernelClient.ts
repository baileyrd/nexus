// Typed wrapper over the `com.nexus.storage::note_append` IPC handler.
//
// Hides the raw `api.kernel.invoke('com.nexus.storage', 'note_append', …)`
// plumbing from the capture-overlay store + tests. Mirrors the shape of
// `editor/kernelClient.ts`. Command string matches the bootstrap manifest
// in `crates/nexus-bootstrap/src/lib.rs` — do not rename without updating
// the bootstrap mapping.

import type { KernelAPI } from '../../../types/plugin.ts'

/** Reverse-DNS id of the storage core plugin. */
export const STORAGE_PLUGIN_ID = 'com.nexus.storage'
/** Command name registered by the storage manifest for `HANDLER_NOTE_APPEND`. */
export const NOTE_APPEND_COMMAND = 'note_append'

/**
 * Mirror of `nexus_storage::ipc::StorageNoteAppendResult` (handler id 53).
 * Same shape as `write_file`'s return so callers can route either through
 * this or `write_file` interchangeably.
 */
export interface StorageNoteAppendResult {
  /** Forge-relative path that was written. */
  path: string
  /** Post-write file size in bytes. */
  size_bytes: number
  /** Unix timestamp (seconds) of the post-write modification. */
  modified_at: number
  /** SHA-256 hex digest of the post-write file content. */
  content_hash: string
}

/**
 * Append `snippet` to the inbox file at `path` through the kernel-routed
 * atomic primitive. The kernel-side dispatch normalises the trailing
 * newline shape so successive captures keep exactly one blank-line
 * separator + exactly one trailing newline regardless of how the
 * existing buffer ended.
 *
 * Forge-relative paths only; absolute paths are rejected at the engine
 * boundary the same way `write_file` rejects them. The caller MUST NOT
 * pre-read + concat + write through `write_file`: the kernel-side append
 * is the only race-free option against the file watcher.
 */
export function appendInbox(
  api: KernelAPI,
  path: string,
  snippet: string,
): Promise<StorageNoteAppendResult> {
  return api.invoke<StorageNoteAppendResult>(STORAGE_PLUGIN_ID, NOTE_APPEND_COMMAND, {
    path,
    snippet,
  })
}
