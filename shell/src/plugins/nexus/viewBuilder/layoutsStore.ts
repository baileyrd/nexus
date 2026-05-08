// BL-067 Phase 1 — saved-layouts store.
//
// Reads / writes named layouts under `<forge>/.forge/layouts/<name>.layout.json`
// using the kernel storage IPC. Layout files are the same JSON shape as
// `.forge/workspace.json` (the `WorkspaceJSON` produced by
// `workspace.layoutSnapshot()`), so a layout you save is a snapshot
// you can apply right back via `workspace.applySnapshot()`.
//
// The store is Zustand-backed so the View Builder panel can react to
// list / save / delete events without polling.

import { create } from 'zustand'

import type { KernelAPI } from '../../../types/plugin'
import type { WorkspaceJSON } from '../../../workspace/types'
import { clientLogger } from '../../../clientLogger'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const LAYOUTS_DIR = '.forge/layouts'
const LAYOUT_SUFFIX = '.layout.json'

/** One row in the saved-layouts panel. The `name` is the file's
 *  basename minus the `.layout.json` suffix; the `relpath` is the
 *  forge-relative path the storage IPC uses. */
export interface SavedLayoutRow {
  name: string
  relpath: string
}

interface LayoutsState {
  /** Active forge's `<forge>/.forge/layouts/` listing. */
  layouts: SavedLayoutRow[]
  /** True while a list refresh is in flight. */
  loading: boolean
  /** Last error string from a list / save / delete op. */
  error: string | null
  setLayouts: (rows: SavedLayoutRow[]) => void
  setLoading: (b: boolean) => void
  setError: (e: string | null) => void
  reset: () => void
}

export const useLayoutsStore = create<LayoutsState>((set) => ({
  layouts: [],
  loading: false,
  error: null,
  setLayouts: (rows) => set({ layouts: rows.slice().sort((a, b) => a.name.localeCompare(b.name)) }),
  setLoading: (b) => set({ loading: b }),
  setError: (e) => set({ error: e }),
  reset: () => set({ layouts: [], loading: false, error: null }),
}))

/** Forge-relative path for a layout name. Reverses
 *  [`relpathToName`]; round-trip stable for any name that survives
 *  [`normaliseName`]. */
export function nameToRelpath(name: string): string {
  return `${LAYOUTS_DIR}/${name}${LAYOUT_SUFFIX}`
}

/** Pull the layout name out of a forge-relative path under
 *  `.forge/layouts/`. Returns `null` for paths that aren't layout
 *  files. Exported so unit tests can pin the parser. */
export function relpathToName(relpath: string): string | null {
  if (!relpath.startsWith(`${LAYOUTS_DIR}/`)) return null
  if (!relpath.endsWith(LAYOUT_SUFFIX)) return null
  const base = relpath.slice(LAYOUTS_DIR.length + 1, relpath.length - LAYOUT_SUFFIX.length)
  if (base.length === 0 || base.includes('/')) return null
  return base
}

/** Restrict a user-typed name to the safe characters we want on
 *  disk. Keeps letters, digits, dashes, underscores, and spaces;
 *  collapses runs of whitespace; trims; rejects anything left empty.
 *  Throws so the UI can show the message inline. */
export function normaliseName(input: string): string {
  const trimmed = input.trim().replace(/\s+/g, ' ')
  if (trimmed.length === 0) throw new Error('Layout name cannot be empty.')
  if (!/^[A-Za-z0-9 _-]+$/.test(trimmed)) {
    throw new Error('Layout name may only contain letters, digits, spaces, underscore, dash.')
  }
  if (trimmed.length > 80) {
    throw new Error('Layout name is too long (max 80 characters).')
  }
  return trimmed
}

interface ListDirEntry {
  name: string
  relpath: string
  isDir: boolean
}

interface ReadFileReply {
  bytes: number[]
}

const utf8Decoder = new TextDecoder('utf-8')
const utf8Encoder = new TextEncoder()

/** List every `<name>.layout.json` under `<forge>/.forge/layouts/`.
 *  Returns the rows sorted by name; missing-directory + empty-dir
 *  cases both return an empty array. */
export async function listLayouts(kernel: KernelAPI): Promise<SavedLayoutRow[]> {
  let rawEntries: ListDirEntry[]
  try {
    rawEntries = await kernel.invoke<ListDirEntry[]>(STORAGE_PLUGIN_ID, 'list_dir', {
      relpath: LAYOUTS_DIR,
    })
  } catch (err) {
    // The storage plugin returns `NotFound` when the directory
    // doesn't exist yet. That's the expected state for a forge that
    // has never saved a layout — surface as an empty list rather
    // than an error.
    if (isNotFound(err)) return []
    throw err
  }
  const rows: SavedLayoutRow[] = []
  for (const entry of rawEntries) {
    if (entry.isDir) continue
    const name = relpathToName(entry.relpath)
    if (name == null) continue
    rows.push({ name, relpath: entry.relpath })
  }
  return rows.sort((a, b) => a.name.localeCompare(b.name))
}

/** Read and parse a layout file. Throws on parse failure with a
 *  message that points at the file. */
export async function loadLayout(
  kernel: KernelAPI,
  name: string,
): Promise<WorkspaceJSON> {
  const relpath = nameToRelpath(name)
  const reply = await kernel.invoke<ReadFileReply>(STORAGE_PLUGIN_ID, 'read_file', {
    path: relpath,
  })
  const text = utf8Decoder.decode(new Uint8Array(reply.bytes ?? []))
  try {
    return JSON.parse(text) as WorkspaceJSON
  } catch (err) {
    throw new Error(
      `Layout '${name}' is not valid JSON: ${
        err instanceof Error ? err.message : String(err)
      }`,
    )
  }
}

/** Write a layout snapshot to disk. The storage plugin auto-creates
 *  the parent directory. Returns the forge-relative path that was
 *  written. */
export async function saveLayout(
  kernel: KernelAPI,
  name: string,
  snapshot: WorkspaceJSON,
): Promise<string> {
  const relpath = nameToRelpath(name)
  // Ensure the layouts dir exists; ignore the "already exists" path.
  try {
    await kernel.invoke<Record<string, never>>(STORAGE_PLUGIN_ID, 'create_dir', {
      relpath: LAYOUTS_DIR,
    })
  } catch (err) {
    if (!isAlreadyExists(err)) {
      // Don't fail the whole save on a dir-create surprise; fall
      // through to write_file which has the canonical error.
      clientLogger.warn('[nexus.viewBuilder] create_dir surprise:', err)
    }
  }
  const text = JSON.stringify(snapshot, null, 2)
  const bytes = Array.from(utf8Encoder.encode(text))
  await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'write_file', {
    path: relpath,
    bytes,
  })
  return relpath
}

/** Delete a saved layout. Returns `true` on success, `false` when
 *  the file didn't exist. Other errors throw. */
export async function deleteLayout(
  kernel: KernelAPI,
  name: string,
): Promise<boolean> {
  const relpath = nameToRelpath(name)
  try {
    await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'delete_file', { path: relpath })
    return true
  } catch (err) {
    if (isNotFound(err)) return false
    throw err
  }
}

/** Refresh the [`useLayoutsStore`] from disk. Used by the panel on
 *  mount, after a save / delete, and on `workspace:opened`. */
export async function refreshLayouts(kernel: KernelAPI): Promise<void> {
  const store = useLayoutsStore.getState()
  store.setLoading(true)
  store.setError(null)
  try {
    const rows = await listLayouts(kernel)
    store.setLayouts(rows)
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    store.setError(msg)
    store.setLayouts([])
  } finally {
    store.setLoading(false)
  }
}

/** True if the kernel error is the storage plugin's `NotFound`
 *  variant. The kernel surfaces errors as `"<Variant>: <message>"`
 *  strings. */
function isNotFound(err: unknown): boolean {
  return matchesVariant(err, 'NotFound')
}

/** True for the storage plugin's `AlreadyExists` variant. */
function isAlreadyExists(err: unknown): boolean {
  return matchesVariant(err, 'AlreadyExists')
}

function matchesVariant(err: unknown, variant: string): boolean {
  if (err instanceof Error) return err.message.includes(variant)
  if (typeof err === 'string') return err.includes(variant)
  return false
}
