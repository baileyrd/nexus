// SH-002: modal portal wrapper.
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
import type { ReactNode } from 'react'

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
}

export function Modal({ children }: Props): ReturnType<typeof createPortal> {
  return createPortal(children, getOrCreateRoot())
}
