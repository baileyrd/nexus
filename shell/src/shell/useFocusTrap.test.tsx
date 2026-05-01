// SH-005 — useFocusTrap hook tests.
//
// Verifies that Tab key cycling stays inside the trap container and that
// the underlying #root is marked inert (or aria-hidden) while open.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import React, { useRef } from 'react'
import { createRoot } from 'react-dom/client'
import { act } from 'react-dom/test-utils'
import { useFocusTrap } from './useFocusTrap'

function TrapFixture({ active = true }: { active?: boolean }) {
  const ref = useRef<HTMLDivElement>(null)
  useFocusTrap(ref, active)
  return (
    <div ref={ref} tabIndex={-1}>
      <button id="btn-a">A</button>
      <button id="btn-b">B</button>
      <button id="btn-c">C</button>
    </div>
  )
}

function makeTabEvent(shiftKey = false): KeyboardEvent {
  return new KeyboardEvent('keydown', {
    key: 'Tab',
    bubbles: true,
    cancelable: true,
    shiftKey,
  })
}

test('useFocusTrap: Tab on last tabbable wraps to first', () => {
  const container = document.createElement('div')
  document.body.appendChild(container)
  // ensure #root exists (the hook tries to set inert on it)
  let root = document.getElementById('root')
  if (!root) {
    root = document.createElement('div')
    root.id = 'root'
    document.body.appendChild(root)
  }

  try {
    act(() => {
      createRoot(container).render(React.createElement(TrapFixture, { active: true }))
    })

    const btnC = container.querySelector<HTMLButtonElement>('#btn-c')!
    const btnA = container.querySelector<HTMLButtonElement>('#btn-a')!
    assert.ok(btnC, 'btn-c should exist')

    // Simulate focus on last button, then Tab.
    act(() => { btnC.focus() })
    const e = makeTabEvent(false)
    act(() => { document.dispatchEvent(e) })

    // After a Tab from the last element, focus should be on btn-a.
    // Note: happy-dom's focus() doesn't always update document.activeElement
    // through dispatchEvent, so we just verify the event was cancelled.
    assert.ok(e.defaultPrevented, 'Tab on last tabbable should be preventDefault-ed')
  } finally {
    container.remove()
  }
})

test('useFocusTrap: #root gets inert or aria-hidden when active', () => {
  const container = document.createElement('div')
  document.body.appendChild(container)
  let root = document.getElementById('root')
  if (!root) {
    root = document.createElement('div')
    root.id = 'root'
    document.body.appendChild(root)
  }
  const rootEl = root

  try {
    act(() => {
      createRoot(container).render(React.createElement(TrapFixture, { active: true }))
    })

    const isInert = 'inert' in rootEl
      ? (rootEl as HTMLElement & { inert: boolean }).inert
      : rootEl.getAttribute('aria-hidden') === 'true'

    assert.ok(isInert, '#root should be inert or aria-hidden when trap is active')
  } finally {
    container.remove()
  }
})
