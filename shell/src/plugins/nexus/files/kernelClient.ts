// Module-scoped holder for the kernel API handle. Set once in the
// plugin's `activate`, read by `FilesTree` when it needs to list a
// directory. Kept out of the React component so the tree doesn't have
// to thread the PluginAPI through every render.

import type { KernelAPI } from '../../../types/plugin'
import type { FilesDirEntry } from './filesStore'
import { clientLogger } from '../../../clientLogger'
import { configStore } from '../../../stores/configStore'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'

let kernel: KernelAPI | null = null

export function setKernel(api: KernelAPI) {
  kernel = api
}

/** Read-only accessor for plugins that piggyback on `nexus.files`'s
 *  kernel handle (e.g. the BL-053 Phase 4 status-dot hook reaches
 *  through here so the FilesTree row can fetch a single value
 *  without re-threading the API through every prop level).
 *  Returns `null` between `workspace:closed` and the next
 *  `workspace:opened`. */
export function getKernel(): KernelAPI | null {
  return kernel
}

/**
 * List the immediate children of a directory inside the active forge.
 *
 * `relpath` is forge-relative and forward-slash separated; the empty
 * string means the forge root. Returns an empty array on any failure
 * — including a call that lands before the kernel is booted — so the
 * tree renders an empty node instead of crashing.
 */
export async function loadChildren(relpath: string): Promise<FilesDirEntry[]> {
  if (!kernel) {
    clientLogger.warn('[nexus.files] loadChildren called before activate; kernel handle missing')
    return []
  }
  try {
    return await kernel.invoke<FilesDirEntry[]>(STORAGE_PLUGIN_ID, 'list_dir', {
      relpath,
    })
  } catch (err) {
    clientLogger.warn('[nexus.files] list_dir failed for', JSON.stringify(relpath), err)
    return []
  }
}

/**
 * Create an empty file at `relpath`. Resolves on success and rejects
 * with the kernel error (already refuses to overwrite per the storage
 * plugin contract). Callers should handle the error — typical cases
 * are "file exists" when the user types an existing name.
 */
export async function createFile(relpath: string): Promise<void> {
  if (!kernel) throw new Error('kernel handle missing')
  await kernel.invoke<Record<string, never>>(STORAGE_PLUGIN_ID, 'create_file', {
    relpath,
  })
}

/** Create a directory at `relpath`. `mkdir -p` semantics on the Rust side. */
export async function createDir(relpath: string): Promise<void> {
  if (!kernel) throw new Error('kernel handle missing')
  await kernel.invoke<Record<string, never>>(STORAGE_PLUGIN_ID, 'create_dir', {
    relpath,
  })
}

/**
 * Rename / move a file or directory inside the active forge. Both
 * `from` and `to` are forge-relative, forward-slash separated. Used
 * by the context-menu Rename action and the drag-drop move flow.
 */
export async function renameEntry(from: string, to: string): Promise<void> {
  if (!kernel) throw new Error('kernel handle missing')
  // C2 (#355) — the storage handler rewrites inbound links in
  // referencing notes when the user's link-update setting is on.
  await kernel.invoke<Record<string, never>>(STORAGE_PLUGIN_ID, 'rename_entry', {
    from,
    to,
    update_links: configStore.get<boolean>('nexus.settings.links.autoUpdate', true),
  })
}

/**
 * Delete a file or directory at `relpath` (recursive for directories).
 * The storage plugin's watcher emits a `file_deleted` event that the
 * tree listens for to refresh the parent listing.
 */
export async function deleteEntry(relpath: string): Promise<void> {
  if (!kernel) throw new Error('kernel handle missing')
  await kernel.invoke<Record<string, never>>(STORAGE_PLUGIN_ID, 'delete_entry', {
    relpath,
  })
}
