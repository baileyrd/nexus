// SH-005: focus trap hook for modal dialogs.
//
// When `active` is true:
//   • Tab / Shift+Tab cycle through tabbable descendants only.
//   • Focus is snapped to the first tabbable element on activation
//     (via requestAnimationFrame so the portal has time to mount).
//   • The element that was focused before activation is restored on deactivation.
//   • Sets `inert` on #root so screen-reader virtual-cursor can't reach
//     the workspace behind the modal; falls back to `aria-hidden` for
//     browsers without inert.
//
// Usage: call inside a modal component with a ref to the dialog container.

import { useEffect, useRef } from 'react'

const TABBABLE_SELECTORS = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
  '[contenteditable="true"]',
].join(',')

function getTabbable(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(TABBABLE_SELECTORS)).filter(
    (el) => el.offsetParent !== null, // skip hidden elements
  )
}

export function useFocusTrap(
  ref: React.RefObject<HTMLElement | null>,
  active: boolean,
): void {
  const savedFocus = useRef<HTMLElement | null>(null)

  useEffect(() => {
    if (!active || !ref.current) return

    // Save current focus to restore later.
    savedFocus.current = document.activeElement as HTMLElement | null

    // Move focus into the modal after portal mount.
    const frame = requestAnimationFrame(() => {
      if (!ref.current) return
      const tabbable = getTabbable(ref.current)
      if (tabbable.length > 0) tabbable[0]!.focus()
      else ref.current.focus()
    })

    // Suppress background accessibility tree.
    const root = document.getElementById('root')
    if (root) {
      if ('inert' in root) {
        ;(root as HTMLElement & { inert: boolean }).inert = true
      } else {
        root.setAttribute('aria-hidden', 'true')
      }
    }

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || !ref.current) return
      const tabbable = getTabbable(ref.current)
      if (tabbable.length === 0) {
        e.preventDefault()
        return
      }
      const first = tabbable[0]!
      const last = tabbable[tabbable.length - 1]!
      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault()
          last.focus()
        }
      } else {
        if (document.activeElement === last) {
          e.preventDefault()
          first.focus()
        }
      }
    }

    document.addEventListener('keydown', onKey, true)

    return () => {
      cancelAnimationFrame(frame)
      document.removeEventListener('keydown', onKey, true)
      // Restore background accessibility tree.
      if (root) {
        if ('inert' in root) {
          ;(root as HTMLElement & { inert: boolean }).inert = false
        } else {
          root.removeAttribute('aria-hidden')
        }
      }
      // Restore focus to wherever the user was before opening the modal.
      if (savedFocus.current && typeof savedFocus.current.focus === 'function') {
        savedFocus.current.focus()
      }
      savedFocus.current = null
    }
  }, [active, ref])
}
