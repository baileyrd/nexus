// SH-002: modal portal wrapper.
// SH-005: focus trap integration.
//
// Renders children into #modal-root (a sibling of #root in index.html)
// so modals escape any stacking context created by workspace ancestors.
// The #modal-root div has pointer-events:none at the container level;
// individual modals add pointer-events:auto on their backdrop.
//
// Falls back to an inline render when #modal-root is absent (test
// environments that don't load index.html) — the getOrCreateRoot helper
// appends a temporary div so portal semantics still apply.

import { createPortal } from 'react-dom'
import { useRef, type ReactNode } from 'react'
import { useFocusTrap } from './useFocusTrap'

function getOrCreateRoot(): HTMLElement {
  const existing = document.getElementById('modal-root')
  if (existing) return existing
  const el = document.createElement('div')
  el.id = 'modal-root'
  document.body.appendChild(el)
  return el
}

interface Props {
  children: ReactNode
  /** When true (default), activate the focus trap. Pass false to opt out. */
  trapFocus?: boolean
}

function ModalInner({ children, trapFocus = true }: Props) {
  const ref = useRef<HTMLDivElement>(null)
  useFocusTrap(ref, trapFocus)
  return <div ref={ref} style={{ display: 'contents' }}>{children}</div>
}

export function Modal({ children, trapFocus = true }: Props): ReturnType<typeof createPortal> {
  return createPortal(
    <ModalInner trapFocus={trapFocus}>{children}</ModalInner>,
    getOrCreateRoot(),
  )
}
