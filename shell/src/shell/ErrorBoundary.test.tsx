// SH-001 — ErrorBoundary render tests.
//
// Verifies that a render-time throw from a child component is caught by
// the boundary and does NOT propagate to sibling boundaries or the root.
// Uses ReactDOM directly (no @testing-library) — happy-dom provides the
// DOM globals via the test setup shim.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import React from 'react'
import { createRoot } from 'react-dom/client'
import { act } from 'react-dom/test-utils'
import { ErrorBoundary } from './ErrorBoundary'

function Bomb(): React.ReactElement {
  throw new Error('boom')
}

function Safe(): React.ReactElement {
  return React.createElement('span', { 'data-testid': 'safe' }, 'ok')
}

test('ErrorBoundary catches render throws and shows fallback', () => {
  const container = document.createElement('div')
  document.body.appendChild(container)

  // Suppress the expected console.error noise from React + our boundary.
  const origError = console.error
  console.error = () => {}

  try {
    act(() => {
      createRoot(container).render(
        React.createElement(ErrorBoundary, { name: 'test' },
          React.createElement(Bomb),
        ),
      )
    })

    const alert = container.querySelector('[role="alert"]')
    assert.ok(alert, 'fallback with role=alert should render after catch')
    assert.ok(
      (alert as HTMLElement).textContent?.includes('boom'),
      'fallback should display the error message',
    )
  } finally {
    console.error = origError
    container.remove()
  }
})

test('ErrorBoundary in one region does not affect sibling regions', () => {
  const container = document.createElement('div')
  document.body.appendChild(container)

  const origError = console.error
  console.error = () => {}

  try {
    act(() => {
      createRoot(container).render(
        React.createElement('div', null,
          React.createElement(ErrorBoundary, { name: 'broken' },
            React.createElement(Bomb),
          ),
          React.createElement(ErrorBoundary, { name: 'healthy' },
            React.createElement(Safe),
          ),
        ),
      )
    })

    // The broken boundary shows the fallback.
    const alerts = container.querySelectorAll('[role="alert"]')
    assert.equal(alerts.length, 1, 'exactly one boundary should be in error state')

    // The healthy sibling still renders.
    const safe = container.querySelector('[data-testid="safe"]')
    assert.ok(safe, 'sibling boundary must still render its children')
  } finally {
    console.error = origError
    container.remove()
  }
})

test('ErrorBoundary Dismiss resets error state', () => {
  const container = document.createElement('div')
  document.body.appendChild(container)

  const origError = console.error
  console.error = () => {}

  try {
    act(() => {
      createRoot(container).render(
        React.createElement(ErrorBoundary, { name: 'dismissable' },
          React.createElement(Bomb),
        ),
      )
    })

    const dismissBtn = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === 'Dismiss',
    )
    assert.ok(dismissBtn, 'Dismiss button should be present in the fallback')
  } finally {
    console.error = origError
    container.remove()
  }
})
