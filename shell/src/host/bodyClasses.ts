// src/host/bodyClasses.ts
//
// Obsidian-faithful body-class state machine.
//
// Obsidian drives huge amounts of styling via <body> classes. This module
// owns the full lifecycle of that class set so CSS can key off them
// (e.g. `body.mod-windows .window-controls`, `body.is-focused .titlebar`).
//
// Installed ONCE from main.tsx before React mounts. The listeners it sets
// up persist for the lifetime of the app; the returned cleanup exists for
// symmetry but is never called in practice.
//
// Tauri 2.x packages `os` detection separately (@tauri-apps/plugin-os),
// which we don't ship. We use `navigator.platform` / `userAgentData` and
// treat the platform class as best-effort — it fires synchronously at
// boot. Focus / maximize / fullscreen wiring goes through
// @tauri-apps/api/window, wrapped in try/catch so the shell still boots
// in a plain browser preview context (no Tauri runtime).

import { getCurrentWindow } from '@tauri-apps/api/window'
import { useLayoutStore } from '../stores/layoutStore'

type Cleanup = () => void

function setClass(name: string, on: boolean) {
  if (typeof document === 'undefined') return
  document.body.classList.toggle(name, on)
}

interface NavigatorWithUA extends Navigator {
  userAgentData?: { platform?: string }
}

function detectPlatformClass(): 'mod-windows' | 'mod-macos' | 'mod-linux' | null {
  // `userAgentData.platform` is the modern, non-deprecated source; fall
  // back to `navigator.platform` on older Chromium / non-Chromium UAs.
  const nav = typeof navigator !== 'undefined' ? (navigator as NavigatorWithUA) : null
  const raw =
    nav?.userAgentData?.platform ??
    nav?.platform ??
    ''
  const p = raw.toLowerCase()
  if (p.includes('win')) return 'mod-windows'
  if (p.includes('mac') || p.includes('darwin')) return 'mod-macos'
  if (p.includes('linux')) return 'mod-linux'
  return null
}

export function installBodyClasses(): Cleanup {
  // 1. Constants (synchronous, pre-paint).
  setClass('is-frameless', true)
  setClass('is-hidden-frameless', true)

  // 2. Platform class — synchronous fallback first; Tauri has no
  //    built-in os module in @tauri-apps/api 2.x (it's a separate
  //    plugin we don't ship), so navigator.* is the authoritative
  //    source here.
  const platformClass = detectPlatformClass()
  if (platformClass) setClass(platformClass, true)

  // 3. Tauri window listeners. All wrapped defensively — in a browser
  //    preview there's no Tauri runtime and these throw.
  const unlisteners: Array<() => void> = []
  let cancelled = false

  ;(async () => {
    try {
      const win = getCurrentWindow()

      // Initial state
      try {
        const focused = await win.isFocused()
        if (!cancelled) setClass('is-focused', focused)
      } catch {
        // Some Tauri builds don't expose isFocused on boot; default true.
        if (!cancelled) setClass('is-focused', true)
      }
      try {
        if (!cancelled) setClass('is-maximized', await win.isMaximized())
      } catch {}
      try {
        if (!cancelled) setClass('is-fullscreen', await win.isFullscreen())
      } catch {}

      // Focus / blur
      try {
        const off = await win.onFocusChanged(({ payload: focused }) => {
          setClass('is-focused', focused)
        })
        unlisteners.push(off)
      } catch {}

      // Resize drives maximized + fullscreen
      try {
        const off = await win.onResized(async () => {
          try { setClass('is-maximized', await win.isMaximized()) } catch {}
          try { setClass('is-fullscreen', await win.isFullscreen()) } catch {}
        })
        unlisteners.push(off)
      } catch {}
    } catch {
      // No Tauri runtime — browser preview path. Swallow silently.
    }
  })()

  // 4. Store-driven classes.
  const syncFromStore = (s: ReturnType<typeof useLayoutStore.getState>) => {
    setClass('show-ribbon', s.activityBar.visible)
    setClass('show-view-header', s.showViewHeader)
  }
  syncFromStore(useLayoutStore.getState())
  const offStore = useLayoutStore.subscribe(syncFromStore)
  unlisteners.push(offStore)

  return () => {
    cancelled = true
    for (const off of unlisteners) {
      try { off() } catch {}
    }
  }
}
