// Module-scoped holder for the kernel API handle. Set once in the
// plugin's `activate`, read by `FilesTree` when it needs to list a
// directory. Kept out of the React component so the tree doesn't have
// to thread the PluginAPI through every render.

import type { KernelAPI } from '../../../types/plugin'
import type { FilesDirEntry } from './filesStore'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'

let kernel: KernelAPI | null = null

export function setKernel(api: KernelAPI) {
  kernel = api
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
    console.warn('[nexus.files] loadChildren called before activate; kernel handle missing')
    return []
  }
  try {
    return await kernel.invoke<FilesDirEntry[]>(STORAGE_PLUGIN_ID, 'list_dir', {
      relpath,
    })
  } catch (err) {
    console.warn('[nexus.files] list_dir failed for', JSON.stringify(relpath), err)
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
