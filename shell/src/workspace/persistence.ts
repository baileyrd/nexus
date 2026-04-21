// Layout persistence for the Leaf + ViewRegistry workspace model.
// Plan reference: /home/baileyrd/projects/nexus/docs/leaf-migration-plan.md §Phase 6.
//
// On-disk location: `<vault>/.forge/workspace.json`. Persists the full
// WorkspaceJSON produced by `workspace.serialize()`. Loaded once at boot
// (before `hydrate`), saved debounced (250ms) whenever the layout changes.
//
// File I/O goes through the kernel bridge's `com.nexus.storage:read_file` /
// `write_vault_file` handlers. Those accept vault-relative paths and are the
// only Tauri-backed file primitives the shell has access to that work with
// the user's current vault (tauri-plugin-fs is scope-limited; see the
// `path_exists` comment in src-tauri/src/lib.rs:179-182).
//
// Writes go through `write_vault_file` (not `write_file`) because
// workspace.json lives under `.forge/` and must NOT be indexed into FTS or
// the knowledge graph. `write_vault_file` does atomic write + mkdir_p only;
// no post-write hooks.

import { invoke } from '@tauri-apps/api/core'
import type { SerializedNode, WorkspaceJSON } from './types.ts'
import { workspace } from './workspaceStore.ts'

// ---------------------------------------------------------------------------
// Kernel bridge shim — a module-level function pointer so tests can swap it
// out. See createDebouncedSaver tests.
// ---------------------------------------------------------------------------

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const READ_FILE_COMMAND = 'read_file'
const WRITE_VAULT_FILE_COMMAND = 'write_vault_file'

const WORKSPACE_REL = '.forge/workspace.json'

export interface KernelBridge {
  /** Read vault-relative file. Returns null on not-found. */
  readVaultFile(relPath: string): Promise<string | null>
  /** Write vault-relative file. Must create parents as needed. */
  writeVaultFile(relPath: string, content: string): Promise<void>
}

interface ReadFileResponse {
  bytes?: number[]
}

/**
 * Default kernel bridge — talks to `com.nexus.storage` via Tauri's
 * `kernel_invoke` command. The storage engine's `write_vault_file`
 * handler creates parent directories automatically (atomic_write in
 * crates/nexus-storage/src/lib.rs).
 */
const defaultBridge: KernelBridge = {
  async readVaultFile(relPath: string): Promise<string | null> {
    // Don't probe via file_exists: that handler queries the SQLite file index,
    // which is intentionally bypassed for .forge/ writes. Let read_file fail
    // naturally on missing files — FileNotFound arrives as a rejected invoke.
    try {
      const resp = await invoke<ReadFileResponse>('kernel_invoke', {
        pluginId: STORAGE_PLUGIN_ID,
        commandId: READ_FILE_COMMAND,
        args: { path: relPath },
        timeoutMs: null,
      })
      const bytes = resp.bytes ?? []
      return new TextDecoder().decode(new Uint8Array(bytes))
    } catch (err) {
      const msg = String((err as { message?: string })?.message ?? err)
      if (!/not found|FileNotFound|no such file/i.test(msg)) {
        console.warn('[workspace.persistence] readVaultFile failed', err)
      }
      return null
    }
  },

  async writeVaultFile(relPath: string, content: string): Promise<void> {
    const bytes = Array.from(new TextEncoder().encode(content))
    await invoke<unknown>('kernel_invoke', {
      pluginId: STORAGE_PLUGIN_ID,
      commandId: WRITE_VAULT_FILE_COMMAND,
      args: { path: relPath, bytes },
      timeoutMs: null,
    })
  },
}

let activeBridge: KernelBridge = defaultBridge

/**
 * Test seam: override the kernel bridge. Production code never touches this.
 * Returns a disposer that restores the previous bridge.
 */
export function __setKernelBridge(bridge: KernelBridge): () => void {
  const prev = activeBridge
  activeBridge = bridge
  return () => {
    activeBridge = prev
  }
}

// ---------------------------------------------------------------------------
// Schema guards — minimal, hand-rolled. Zod would be cleaner but the plan
// forbids new dependencies. Goal: reject shapes that would crash hydrate;
// tolerate extra fields for forward compat.
// ---------------------------------------------------------------------------

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

function isSerializedNode(v: unknown): v is SerializedNode {
  if (!isObj(v)) return false
  const kind = v.kind
  if (typeof kind !== 'string') return false
  switch (kind) {
    case 'split':
      if (typeof v.id !== 'string') return false
      if (v.direction !== 'horizontal' && v.direction !== 'vertical') return false
      if (!Array.isArray(v.children)) return false
      return (v.children as unknown[]).every(isSerializedNode)
    case 'tabs':
      if (typeof v.id !== 'string') return false
      if (typeof v.activeIndex !== 'number') return false
      if (!Array.isArray(v.leaves)) return false
      return (v.leaves as unknown[]).every(isSerializedLeaf)
    case 'root':
    case 'floating':
      if (typeof v.id !== 'string') return false
      return isSerializedNode(v.child)
    case 'leaf':
      return isSerializedLeaf(v)
    default:
      return false
  }
}

function isSerializedLeaf(v: unknown): boolean {
  if (!isObj(v)) return false
  if (v.kind !== 'leaf') return false
  if (typeof v.id !== 'string') return false
  const vs = v.viewState
  if (!isObj(vs)) return false
  return typeof vs.type === 'string'
}

function isWorkspaceJSON(v: unknown): v is WorkspaceJSON {
  if (!isObj(v)) return false
  if (!('main' in v) || !('left' in v) || !('right' in v)) return false
  if (!isSerializedNode(v.main)) return false
  if (!isSerializedNode(v.left)) return false
  if (!isSerializedNode(v.right)) return false
  // `bottom` is optional for backwards-compat with workspace.json files
  // written before the bottom drawer landed. Only validate if present.
  if ('bottom' in v && v.bottom !== undefined && !isSerializedNode(v.bottom)) {
    return false
  }
  if (v.active !== null && typeof v.active !== 'string') return false
  if ('lastOpenFiles' in v && !Array.isArray(v.lastOpenFiles)) return false
  return true
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Load `<vault>/.forge/workspace.json`. Returns `null` if the file is
 * absent, contains invalid JSON, or fails schema validation. Logs on
 * validation failure — never throws. Callers fall back to the default
 * layout.
 */
export async function loadWorkspace(
  vaultPath: string,
): Promise<WorkspaceJSON | null> {
  if (!vaultPath) return null
  void vaultPath // vault-relative semantics: bridge knows the active vault
  const text = await activeBridge.readVaultFile(WORKSPACE_REL)
  if (text === null) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch (err) {
    console.warn('[workspace.persistence] malformed workspace.json, falling back to default', err)
    return null
  }
  if (!isWorkspaceJSON(parsed)) {
    console.warn('[workspace.persistence] workspace.json failed schema validation, falling back to default')
    return null
  }
  return parsed
}

/**
 * Write `json` to `<vault>/.forge/workspace.json`. Parent directory is
 * created by the storage engine's atomic_write.
 */
export async function saveWorkspace(
  vaultPath: string,
  json: WorkspaceJSON,
): Promise<void> {
  if (!vaultPath) return
  void vaultPath
  const text = JSON.stringify(json, null, 2)
  try {
    await activeBridge.writeVaultFile(WORKSPACE_REL, text)
  } catch (err) {
    console.error('[workspace.persistence] saveWorkspace failed', err)
  }
}

/**
 * Return a debounced saver. Rapid calls within `ms` coalesce into a
 * single write with the latest JSON. Simple setTimeout cancel/reset.
 */
export function createDebouncedSaver(
  vaultPath: string,
  ms = 250,
): (json: WorkspaceJSON) => void {
  let timer: ReturnType<typeof setTimeout> | null = null
  let pending: WorkspaceJSON | null = null

  return (json: WorkspaceJSON): void => {
    pending = json
    if (timer !== null) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = null
      const toSave = pending
      pending = null
      if (toSave) void saveWorkspace(vaultPath, toSave)
    }, ms)
  }
}

/**
 * Subscribe to every event that mutates the serialized layout and
 * trigger a debounced save with `workspace.serialize()`. Returns a
 * disposer that unsubscribes from all listeners.
 *
 * Events mirror Obsidian's write-trigger set (`docs/07-plugin-api.md §4.1`):
 *   - layout-change       structural tree mutation
 *   - view-changed        a leaf's view type or state changed
 *   - active-leaf-change  active leaf moved
 *   - pinned-change       leaf pinned/unpinned
 */
export function installAutoSave(vaultPath: string): () => void {
  const save = createDebouncedSaver(vaultPath)
  const trigger = (): void => save(workspace.serialize())

  const offLayout = workspace.on('layout-change', trigger)
  const offView = workspace.on('view-changed', trigger)
  const offActive = workspace.on('active-leaf-change', trigger)
  const offPinned = workspace.on('pinned-change', trigger)

  return () => {
    offLayout()
    offView()
    offActive()
    offPinned()
  }
}
