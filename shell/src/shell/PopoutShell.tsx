// BL-029 — popout-mode shell.
//
// Loaded into a child `WebviewWindow` opened by the `popout_window`
// Tauri command (see `shell/src-tauri/src/windows.rs`). The popout's
// URL is `index.html?popout=<fwId>&leaf=<leafId>`; `main.tsx` checks
// for the `popout` query param and mounts this component instead of
// the full `<App>`.
//
// Phase 2a (shipped 2026-04-30) wired the close-request handshake.
// Phase 2b (this commit) lights up actual leaf rendering: the popout
// boots the DEFAULT_ON plugin set with `popoutMode = true` set on the
// shared context key service (so `nexus.workspace` skips kernel
// lifecycle calls — see ADR 0020 §1), waits for `shellReady`, hydrates
// its own copy of `workspace.json` read-only, locates the requested
// FloatingWindow / leaf, and mounts a single `LeafHost` for it. ADR
// 0020 §4 — popouts fail closed: an unresolvable fwId or leafId
// renders an explicit error state instead of silently falling back.

import React, { useEffect, useMemo, useState } from 'react'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useContextKey } from '../host/ContextKeyService'
import { useWorkspaceStore as useNexusWorkspaceStore } from '../plugins/nexus/workspace/workspaceStore'
import {
  workspace as workspaceStore,
  useWorkspaceStore,
} from '../workspace/workspaceStore'
import { LeafHost } from '../workspace/WorkspaceRenderer'
import { buildDefaultLayout, loadWorkspace } from '../workspace'
import type { Leaf, FloatingWindow, WorkspaceParent } from '../workspace/types'

/** Tauri-app event emitted by a popout right before it closes. The
 *  main window listens for this and removes the matching
 *  FloatingWindow entry from its workspace store. Payload is the
 *  popout's FloatingWindow id (the `fwId` from the URL). */
export const POPOUT_CLOSED_EVENT = 'nexus:popout-closed'

/**
 * SH-021: Tauri-app event emitted by a popout when it is moved or
 * resized. The main window listens for this and calls
 * `workspace.setFloatingWindowBounds(fwId, bounds)` so bounds survive
 * restart. Payload: `{ fwId: string, bounds: PopoutBounds }`.
 */
export const POPOUT_BOUNDS_CHANGED_EVENT = 'nexus:popout-bounds-changed'

interface PopoutBoundsPayload {
  fwId: string
  bounds: { x: number; y: number; w: number; h: number }
}

interface PopoutInfo {
  fwId: string
  leafId: string | null
}

function readPopoutInfo(): PopoutInfo | null {
  if (typeof window === 'undefined') return null
  const params = new URLSearchParams(window.location.search)
  const fwId = params.get('popout')
  if (!fwId) return null
  const leafId = params.get('leaf')
  return { fwId, leafId }
}

/**
 * SH-021: Subscribe to OS move/resize events and emit
 * `nexus:popout-bounds-changed` so the main window can persist the
 * new bounds. Events are debounced to 300ms to avoid flooding the
 * main window during continuous drag/resize.
 */
async function installBoundsListener(fwId: string): Promise<() => void> {
  try {
    const win = getCurrentWindow()
    let timer: ReturnType<typeof setTimeout> | undefined
    const flush = async () => {
      try {
        const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()])
        const payload: PopoutBoundsPayload = {
          fwId,
          bounds: { x: pos.x, y: pos.y, w: size.width, h: size.height },
        }
        await emit(POPOUT_BOUNDS_CHANGED_EVENT, payload)
      } catch {
        // Swallow — bounds persistence is best-effort.
      }
    }
    const schedule = () => {
      clearTimeout(timer)
      timer = setTimeout(flush, 300)
    }
    const [unMove, unResize] = await Promise.all([
      win.onMoved(schedule),
      win.onResized(schedule),
    ])
    return () => {
      clearTimeout(timer)
      unMove()
      unResize()
    }
  } catch (err) {
    console.warn('[PopoutShell] bounds listener registration failed', err)
    return () => {}
  }
}

/**
 * Subscribe to the OS-level close request and emit
 * `nexus:popout-closed` with the popout's fwId before letting the
 * close proceed. ADR 0020 §3: closing a popout removes the leaf from
 * `floating[]`; the kernel session keeps unsaved buffer state alive.
 */
async function installCloseHandshake(fwId: string): Promise<() => void> {
  try {
    const unlisten = await getCurrentWindow().onCloseRequested(async () => {
      try {
        await emit(POPOUT_CLOSED_EVENT, { fwId })
      } catch (err) {
        console.warn('[PopoutShell] failed to emit popout-closed event', err)
      }
    })
    return unlisten
  } catch (err) {
    console.warn('[PopoutShell] onCloseRequested registration failed', err)
    return () => {}
  }
}

/**
 * Walk a node and return the first Leaf with the given id. Used to
 * resolve the popout's `leafId` URL param against the FloatingWindow
 * subtree we just hydrated. Returns null if no match.
 */
export function findLeafInNode(node: WorkspaceParent, leafId: string): Leaf | null {
  if (node.kind === 'tabs') {
    return node.leaves.find((l) => l.id === leafId) ?? null
  }
  if (node.kind === 'split') {
    for (const child of node.children) {
      const hit = findLeafInNode(child, leafId)
      if (hit) return hit
    }
    return null
  }
  const withChild = node as { child?: WorkspaceParent }
  if (withChild.child) return findLeafInNode(withChild.child, leafId)
  return null
}

type Resolution =
  | { kind: 'pending' }
  | { kind: 'ready'; leaf: Leaf }
  | { kind: 'error'; reason: string }

/**
 * Hydrate the popout's per-window workspace store from
 * `<forge>/.forge/workspace.json` (read-only — main window owns the
 * write side per ADR 0020 §1) and locate the leaf indicated by the
 * URL params.
 *
 * Failure modes (each maps to ADR 0020 §4):
 *  - workspace.json missing / malformed → fall back to the default
 *    layout, then fail closed because the FW will not exist there.
 *  - fwId not in `floating[]` → "out of sync, close to continue".
 *  - leafId not under the FW (or FW lost its leaf) → same error.
 */
async function resolveLeaf(
  rootPath: string,
  fwId: string,
  leafId: string | null,
): Promise<Resolution> {
  const saved = await loadWorkspace(rootPath)
  const json = saved ?? buildDefaultLayout()
  await workspaceStore.hydrate(json)

  const fw: FloatingWindow | null = workspaceStore.findFloatingWindow(fwId)
  if (!fw) {
    return {
      kind: 'error',
      reason: `Popout window ${fwId} is not in the workspace state.`,
    }
  }

  if (!leafId) {
    return {
      kind: 'error',
      reason: 'Popout URL is missing the leaf id.',
    }
  }
  const leaf = findLeafInNode(fw, leafId)
  if (!leaf) {
    return {
      kind: 'error',
      reason: `Leaf ${leafId} is not present in popout ${fwId}.`,
    }
  }
  return { kind: 'ready', leaf }
}

interface PopoutBodyProps {
  resolution: Resolution
}

function PopoutBody({ resolution }: PopoutBodyProps): JSX.Element {
  if (resolution.kind === 'pending') {
    return (
      <div style={popoutInfoStyle}>
        <div style={popoutTitleStyle}>Loading popout…</div>
      </div>
    )
  }
  if (resolution.kind === 'error') {
    return (
      <div style={popoutInfoStyle}>
        <div style={popoutTitleStyle}>Popout out of sync</div>
        <div style={{ fontSize: 12, opacity: 0.7, maxWidth: 420 }}>
          {resolution.reason}
        </div>
        <div style={{ fontSize: 11, opacity: 0.5, marginTop: 12 }}>
          Close this window to continue.
        </div>
      </div>
    )
  }
  return <LeafHost leaf={resolution.leaf} hidden={false} />
}

const popoutInfoStyle: React.CSSProperties = {
  position: 'fixed',
  inset: 0,
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 8,
  background: 'var(--background-primary)',
  color: 'var(--text-normal)',
  fontFamily: 'system-ui, sans-serif',
  padding: 24,
  textAlign: 'center',
}

const popoutTitleStyle: React.CSSProperties = {
  fontSize: 14,
  fontWeight: 600,
}

export function PopoutShell(): JSX.Element {
  const info = useMemo(() => readPopoutInfo(), [])
  const fwId = info?.fwId ?? null
  const leafId = info?.leafId ?? null

  // shellReady flips to true in main.tsx boot() AFTER every plugin has
  // activated. Same gate App.tsx uses, for the same reason: every
  // viewRegistry.register(...) call has run before we hydrate so saved
  // leaves resolve their creator instead of falling back to `empty`.
  const shellReady = useContextKey('shellReady') === true
  const rootPath = useNexusWorkspaceStore((s) => s.rootPath)

  const [resolution, setResolution] = useState<Resolution>({ kind: 'pending' })

  // Close-event handshake — runs once, regardless of resolution outcome.
  useEffect(() => {
    if (!fwId) return
    let dispose: (() => void) | null = null
    void installCloseHandshake(fwId).then((fn) => {
      dispose = fn
    })
    return () => {
      dispose?.()
    }
  }, [fwId])

  // SH-021: Bounds persistence — emits nexus:popout-bounds-changed on
  // every OS move/resize so the main window can persist the new bounds.
  useEffect(() => {
    if (!fwId) return
    let dispose: (() => void) | null = null
    void installBoundsListener(fwId).then((fn) => {
      dispose = fn
    })
    return () => {
      dispose?.()
    }
  }, [fwId])

  // Hydrate workspace + locate leaf, once `shellReady` and `rootPath`
  // are both available. The popout never re-runs hydration on a
  // workspace switch (the popout closes when the user closes its
  // parent forge in the main window — main triggers
  // `closeFloatingWindow` for every fw before its `setRoot(null)`).
  useEffect(() => {
    if (!fwId) {
      setResolution({
        kind: 'error',
        reason: 'Popout URL is missing the fwId.',
      })
      return
    }
    if (!shellReady) return
    if (rootPath === null) return
    if (resolution.kind !== 'pending') return

    let cancelled = false
    void (async () => {
      try {
        const next = await resolveLeaf(rootPath, fwId, leafId)
        if (cancelled) return
        setResolution(next)
      } catch (err) {
        if (cancelled) return
        console.error('[PopoutShell] resolveLeaf failed', err)
        setResolution({
          kind: 'error',
          reason: `Failed to load workspace state: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })()

    return () => {
      cancelled = true
    }
  }, [shellReady, rootPath, fwId, leafId, resolution.kind])

  // Subscribe to layout-change so the popout reacts when the main
  // window edits `floating[]` and our FW disappears (e.g. user closed
  // it from the main window's tab strip). Re-resolves and surfaces the
  // stale-leaf error state per ADR 0020 §4.
  const floating = useWorkspaceStore((s) => s.floating)
  useEffect(() => {
    if (resolution.kind !== 'ready' || !fwId) return
    const stillThere = floating.some((fw) => fw.id === fwId)
    if (!stillThere) {
      setResolution({
        kind: 'error',
        reason: `Popout ${fwId} was closed by the main window.`,
      })
    }
  }, [floating, fwId, resolution.kind])

  return <PopoutBody resolution={resolution} />
}

/**
 * Test seam: returns true when the current URL indicates the shell
 * should boot in popout mode. `main.tsx` calls this with no argument
 * to short-circuit the plugin-load + workspace-render path. Tests
 * pass an explicit search string because happy-dom does not update
 * `window.location.search` on `history.replaceState`.
 */
export function isPopoutMode(search?: string): boolean {
  if (search === undefined) {
    if (typeof window === 'undefined') return false
    search = window.location.search
  }
  return new URLSearchParams(search).has('popout')
}
