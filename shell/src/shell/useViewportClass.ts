// SH-003: ResizeObserver-driven viewport class hook.
//
// Writes one of `is-narrow`, `is-medium`, or `is-wide` onto <body> based on
// the root element's current width. React / CSS media-queries can't observe
// Tauri WebviewWindow resizes (no `resize` event until the frame is focused),
// but ResizeObserver fires reliably on documentElement.
//
// Breakpoints mirror common desktop tile sizes:
//   < 768 px  → body.is-narrow   (small floating / half-screen window)
//   768–1279  → body.is-medium   (typical laptop, ¾-screen)
//   ≥ 1280 px → body.is-wide     (full HD+)
//
// Usage: call once in App.tsx (top-level component). The hook is a no-op
// in test environments where ResizeObserver is unavailable.

import { useEffect } from 'react'

export const NARROW_BREAKPOINT = 768
export const WIDE_BREAKPOINT = 1280

const CLASSES = ['is-narrow', 'is-medium', 'is-wide'] as const

function getClass(width: number): typeof CLASSES[number] {
  if (width < NARROW_BREAKPOINT) return 'is-narrow'
  if (width < WIDE_BREAKPOINT) return 'is-medium'
  return 'is-wide'
}

function applyClass(width: number): void {
  const cls = getClass(width)
  const { classList } = document.body
  for (const c of CLASSES) {
    if (c === cls) classList.add(c)
    else classList.remove(c)
  }
}

export function useViewportClass(): void {
  useEffect(() => {
    if (typeof ResizeObserver === 'undefined') return

    applyClass(document.documentElement.clientWidth)

    const ro = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (!entry) return
      applyClass(entry.contentRect.width)
    })

    ro.observe(document.documentElement)
    return () => ro.disconnect()
  }, [])
}
