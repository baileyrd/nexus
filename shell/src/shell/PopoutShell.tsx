// BL-029 — popout-mode shell.
//
// Loaded into a child `WebviewWindow` opened by the `popout_window`
// Tauri command (see `shell/src-tauri/src/windows.rs`). The popout's
// URL is `index.html?popout=<fwId>&leaf=<leafId>`; `main.tsx` checks
// for the `popout` query param and mounts this component instead of
// the full `<App>`.
//
// Phase 1 scope (this commit): show a placeholder summarising the
// requested popout id + leaf id, and the OS-side close affordance is
// the native window decoration. The full popout-side leaf hydration
// (mounting the same View instance the main window had, sharing state
// over Tauri events, in-popout title bar with detach/dock controls)
// lands in BL-029 Phase 2.
//
// Why a placeholder is acceptable Phase 1 ground:
//  - The Tauri-side primitives + workspace-store API are the
//    foundational work. Without them, no UI surface can detach a
//    panel.
//  - The popout window, once spawned, can already host the
//    `__nexusShellApi` and `kernel_invoke` IPC since managed state is
//    process-wide. Future commits add the view-mounting layer on top
//    of this placeholder without changing the boot path.

import React from 'react'

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

export function PopoutShell(): JSX.Element {
  const info = readPopoutInfo()
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
        Detached panel rendering will land in BL-029 Phase 2. The host
        kernel and IPC are already reachable from this window via
        <code> kernel_invoke</code>.
      </div>
    </div>
  )
}

/**
 * Test seam: returns true when the current URL indicates the shell
 * should boot in popout mode. `main.tsx` calls this to short-circuit
 * the plugin-load + workspace-render path.
 */
export function isPopoutMode(): boolean {
  if (typeof window === 'undefined') return false
  return new URLSearchParams(window.location.search).has('popout')
}
