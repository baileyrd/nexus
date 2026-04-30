// BL-029 — popout-mode shell.
//
// Loaded into a child `WebviewWindow` opened by the `popout_window`
// Tauri command (see `shell/src-tauri/src/windows.rs`). The popout's
// URL is `index.html?popout=<fwId>&leaf=<leafId>`; `main.tsx` checks
// for the `popout` query param and mounts this component instead of
// the full `<App>`.
//
// Phase 2a scope (this commit): wire the close-request sync so the
// main window's `floating[]` state stays consistent when the user
// closes a popout via the OS-X button (or any other native close
// path). The full popout-side leaf hydration — running plugin
// activation in the popout webview, hydrating workspace.json, and
// mounting a `LeafHost` for the requested leaf — is staged in Phase
// 2b. ADR 0020 documents the design decisions for both slices.
//
// Why close-event sync ships ahead of leaf rendering:
//  - It validates the cross-window architecture from ADR 0020 §2/§3
//    end-to-end (Tauri global events + main-window listener +
//    workspace-store mutation).
//  - Without it, closing a popout via OS-X leaves a stale entry in
//    `floating[]`, which the next main-window reload tries to
//    re-open — a real bug surfaced once Phase 1 landed.
//  - It is a self-contained, testable change that does not require
//    the popout to boot the plugin host.

import React, { useEffect } from 'react'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

/** Tauri-app event emitted by a popout right before it closes. The
 *  main window listens for this and removes the matching
 *  FloatingWindow entry from its workspace store. Payload is the
 *  popout's FloatingWindow id (the `fwId` from the URL). */
export const POPOUT_CLOSED_EVENT = 'nexus:popout-closed'

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
 * Subscribe to the OS-level close request and emit
 * `nexus:popout-closed` with the popout's fwId before letting the
 * close proceed. ADR 0020 §3: closing a popout removes the leaf from
 * `floating[]`; the kernel session keeps unsaved buffer state alive.
 *
 * The Tauri event is global (no `target`), so the main window's
 * `listen('nexus:popout-closed', ...)` picks it up regardless of
 * which popout fired it.
 *
 * Failures are logged and ignored — a popout that can't notify the
 * main window still has to close (the user expects OS-X to work),
 * and the next `restoreFloatingWindows()` reconciliation on a main-
 * window reload will close the orphan record anyway.
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

export function PopoutShell(): JSX.Element {
  const info = readPopoutInfo()

  const fwId = info?.fwId ?? null
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

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 8,
        background: 'var(--background-primary, #1e1e1e)',
        color: 'var(--text-normal, #ccc)',
        fontFamily: 'system-ui, sans-serif',
        padding: 24,
        textAlign: 'center',
      }}
    >
      <div style={{ fontSize: 14, fontWeight: 600 }}>Nexus Popout</div>
      <div style={{ fontSize: 12, opacity: 0.7 }}>
        Popout window initialized.
      </div>
      <div style={{ fontSize: 11, opacity: 0.5, fontFamily: 'monospace' }}>
        fwId: {info?.fwId ?? '(none)'}
        <br />
        leafId: {info?.leafId ?? '(none)'}
      </div>
      <div style={{ fontSize: 11, opacity: 0.4, marginTop: 16, maxWidth: 360 }}>
        Detached panel rendering will land in BL-029 Phase 2b. The host
        kernel and IPC are already reachable from this window via
        <code> kernel_invoke</code>; the close handshake is wired so
        OS-X cleanly removes this popout from the main window's
        floating[] state (ADR 0020 §3).
      </div>
    </div>
  )
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
