// BL-029 — bridge between the workspace store's `floating[]` state and
// the Tauri-side popout windows defined in `shell/src-tauri/src/windows.rs`.
//
// The store mutations (`workspace.popoutLeaf`, `workspace.closeFloatingWindow`,
// `workspace.setFloatingWindowBounds`) are pure layout edits — they don't
// touch the OS. This module pairs them with the matching Tauri invokes so
// callers get a single high-level entry point.
//
// Layered intentionally: the store stays import-free of `@tauri-apps/api`
// so its tests run under node:test without a Tauri runtime.
//
// Boot-time hydration: when the shell loads a `workspace.json` containing
// non-empty `floating[]`, this module's `restoreFloatingWindows()` opens
// matching popout windows. If a popout was already alive (e.g. HMR
// refresh of the main window), the Tauri command rejects with "popout
// already open", which we treat as a no-op.

import { invoke } from '@tauri-apps/api/core'
import { workspace } from './workspaceStore.ts'
import type { FloatingWindow, Leaf } from './types.ts'

interface PopoutBounds {
  x: number
  y: number
  w: number
  h: number
}

interface PopoutSnapshot {
  label: string
  title: string
  bounds: PopoutBounds | null
}

// ---------------------------------------------------------------------------
// Tauri invoke wrappers — thin adapters with consistent error logging.
// ---------------------------------------------------------------------------

async function popoutWindowTauri(
  id: string,
  leafId: string | undefined,
  title: string | undefined,
  bounds: PopoutBounds | undefined,
): Promise<string> {
  return invoke<string>('popout_window', {
    id,
    leafId: leafId ?? null,
    title: title ?? null,
    bounds: bounds ?? null,
  })
}

async function closePopoutTauri(id: string): Promise<void> {
  await invoke<void>('close_popout_window', { id })
}

async function listPopoutsTauri(): Promise<PopoutSnapshot[]> {
  return invoke<PopoutSnapshot[]>('list_popout_windows')
}

async function getPopoutBoundsTauri(
  id: string,
): Promise<PopoutBounds | null> {
  return invoke<PopoutBounds | null>('get_popout_window_bounds', { id })
}

async function setPopoutBoundsTauri(
  id: string,
  bounds: PopoutBounds,
): Promise<void> {
  await invoke<void>('set_popout_window_bounds', { id, bounds })
}

// ---------------------------------------------------------------------------
// High-level helpers — pair the store mutation with the Tauri invoke.
// ---------------------------------------------------------------------------

/**
 * Pop a leaf out into a fresh OS window. Updates the workspace
 * store and opens the popout in one step.
 *
 * Returns the FloatingWindow id so the caller can pair it with later
 * close / bounds calls.
 *
 * If the Tauri-side window creation fails the store mutation is
 * rolled back via `closeFloatingWindow` so a failed popout doesn't
 * leave the leaf orphaned in `floating[]`.
 */
export async function popoutLeaf(
  leafId: string,
  opts?: { title?: string; bounds?: PopoutBounds },
): Promise<string> {
  const fwId = workspace.popoutLeaf(leafId, opts?.bounds)
  try {
    await popoutWindowTauri(fwId, leafId, opts?.title, opts?.bounds)
  } catch (err) {
    console.error('[popoutWindowBridge] popout_window failed; rolling back', err)
    // Roll back the store mutation so the user sees a consistent state.
    await workspace.closeFloatingWindow(fwId)
    throw err
  }
  return fwId
}

/**
 * Close a popout window. Removes the matching FloatingWindow from
 * the store first, then asks Tauri to close the OS window. The
 * order matters: closing the store first means the saved
 * `workspace.json` no longer references the popout even if Tauri
 * lags or errors on the close call.
 */
export async function closePopout(fwId: string): Promise<void> {
  await workspace.closeFloatingWindow(fwId)
  try {
    await closePopoutTauri(fwId)
  } catch (err) {
    // The store side has already cleaned up. Log but don't throw —
    // a missing OS window is a no-op on the Tauri side already.
    console.warn('[popoutWindowBridge] close_popout_window failed', err)
  }
}

/**
 * Push a bounds update from the popout window's resize/move event
 * into the main-window store. Called from a popout's body listeners.
 */
export async function reportPopoutBounds(
  fwId: string,
  bounds: PopoutBounds,
): Promise<void> {
  workspace.setFloatingWindowBounds(fwId, bounds)
}

/**
 * Restore popout windows from a freshly-hydrated workspace. Called
 * once on shell boot, after `workspace.hydrate(json)`.
 *
 * For every FloatingWindow in `floating[]`, opens a matching Tauri
 * popout window with the persisted bounds. Errors are logged
 * per-window — one bad popout doesn't block the rest.
 *
 * Also reconciles against `list_popout_windows`: a popout that's
 * present in the store but not in Tauri (cold start) gets opened;
 * a popout present in Tauri but not in the store (orphan from a
 * crashed session) gets closed.
 */
export async function restoreFloatingWindows(): Promise<void> {
  let snapshots: PopoutSnapshot[] = []
  try {
    snapshots = await listPopoutsTauri()
  } catch (err) {
    console.warn('[popoutWindowBridge] list_popout_windows failed', err)
  }
  const liveLabels = new Set(snapshots.map((s) => s.label))
  const expectedLabels = new Set(
    workspace.floating.map((fw) => `popout-${fw.id}`),
  )

  // Open the popouts the store expects but Tauri doesn't have.
  for (const fw of workspace.floating) {
    const label = `popout-${fw.id}`
    if (liveLabels.has(label)) continue
    const leafId = firstLeafId(fw)
    try {
      await popoutWindowTauri(fw.id, leafId, undefined, fw.bounds)
    } catch (err) {
      console.warn(`[popoutWindowBridge] failed to restore ${label}`, err)
    }
  }

  // Close any popout windows Tauri owns that the store no longer
  // expects (orphan reconciliation).
  for (const snap of snapshots) {
    if (expectedLabels.has(snap.label)) continue
    const id = snap.label.startsWith('popout-')
      ? snap.label.slice('popout-'.length)
      : null
    if (!id) continue
    try {
      await closePopoutTauri(id)
    } catch (err) {
      console.warn(`[popoutWindowBridge] orphan close failed for ${snap.label}`, err)
    }
  }
}

/**
 * Walk a FloatingWindow subtree and return the first leaf id, if any.
 * Used to pass `leafId` through to `popout_window` so the popout's
 * URL carries the right hint for popout-mode rendering.
 */
function firstLeafId(fw: FloatingWindow): string | undefined {
  const visit = (node: import('./types.ts').WorkspaceParent): Leaf | null => {
    if (node.kind === 'tabs') {
      return node.leaves[0] ?? null
    }
    if (node.kind === 'split') {
      for (const c of node.children) {
        const leaf = visit(c)
        if (leaf) return leaf
      }
      return null
    }
    const withChild = node as { child?: import('./types.ts').WorkspaceParent }
    if (withChild.child) return visit(withChild.child)
    return null
  }
  return visit(fw.child)?.id
}

// Re-export the lower-level helpers for tests / advanced callers.
export {
  popoutWindowTauri,
  closePopoutTauri,
  listPopoutsTauri,
  getPopoutBoundsTauri,
  setPopoutBoundsTauri,
  type PopoutBounds,
  type PopoutSnapshot,
}
