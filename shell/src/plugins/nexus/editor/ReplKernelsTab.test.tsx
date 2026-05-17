// BL-142 Phase 3 — component smoke tests for the REPL Kernels
// Settings tab. The pure-factor coverage lives in
// `replKernelsTabModel.test.ts`; this file exercises the React
// component end-to-end against happy-dom so we catch wiring drift
// (initial hydration, dirty-state tracking, Save round-trip
// through configStore).
//
// Pattern matches `ErrorBoundary.test.tsx` / `useFocusTrap.test.tsx`
// — `createRoot` + `act` directly, no @testing-library dependency.

import { describe, it, beforeEach, afterEach } from 'node:test'
import assert from 'node:assert/strict'
import React from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react-dom/test-utils'

import { ReplKernelsTab } from './ReplKernelsTab'
import { CONFIG_REPL_KERNELS } from './replKernels'
import { useConfigStore } from '../../../stores/configStore'

function seedConfig(json: string) {
  // Use the underlying setState directly so we don't schedule a
  // Tauri flush during seeding — the test runner has no Tauri
  // bridge, so flushes are best-effort no-ops anyway, but skipping
  // the scheduler keeps the per-test setup deterministic.
  useConfigStore.setState({
    values: { [CONFIG_REPL_KERNELS]: json },
    hydrated: true,
  })
}

function readPersisted(): string | undefined {
  return useConfigStore.getState().values[CONFIG_REPL_KERNELS] as
    | string
    | undefined
}

describe('ReplKernelsTab', () => {
  let container: HTMLDivElement
  let root: Root | null = null

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
  })

  afterEach(() => {
    if (root) {
      act(() => {
        root!.unmount()
      })
      root = null
    }
    container.remove()
    // Reset config so cross-test pollution can't leak.
    useConfigStore.setState({ values: {}, hydrated: false })
  })

  const mount = () => {
    act(() => {
      root = createRoot(container)
      root.render(React.createElement(ReplKernelsTab))
    })
  }

  it('shows the empty state when no kernels are configured', () => {
    seedConfig('{}')
    mount()
    const empty = container.querySelector('[data-testid="repl-kernels-empty"]')
    assert.ok(empty, 'empty-state message should render')
    assert.equal(
      container.querySelectorAll('[data-testid="repl-kernel-row"]').length,
      0,
    )
  })

  it('renders one row per configured kernel, in insertion order', () => {
    seedConfig('{"python":"python3 -i","node":"node --interactive"}')
    mount()
    const rows = container.querySelectorAll(
      '[data-testid="repl-kernel-row"]',
    )
    assert.equal(rows.length, 2)
    const langs = Array.from(
      container.querySelectorAll<HTMLInputElement>(
        '[data-testid="repl-kernel-lang"]',
      ),
    ).map((i) => i.value)
    assert.deepEqual(langs, ['python', 'node'])
    const commands = Array.from(
      container.querySelectorAll<HTMLInputElement>(
        '[data-testid="repl-kernel-command"]',
      ),
    ).map((i) => i.value)
    assert.deepEqual(commands, ['python3 -i', 'node --interactive'])
  })

  it('appends a blank row when "+ Add kernel" is clicked', () => {
    seedConfig('{}')
    mount()
    assert.equal(
      container.querySelectorAll('[data-testid="repl-kernel-row"]').length,
      0,
    )
    const add = container.querySelector(
      '[data-testid="repl-kernel-add"]',
    ) as HTMLButtonElement | null
    assert.ok(add, 'Add button must render')
    act(() => {
      add!.click()
    })
    assert.equal(
      container.querySelectorAll('[data-testid="repl-kernel-row"]').length,
      1,
    )
  })

  it('removes a row when its × button is clicked', () => {
    seedConfig('{"python":"python3 -i","node":"node --interactive"}')
    mount()
    const removes = container.querySelectorAll<HTMLButtonElement>(
      '[data-testid="repl-kernel-remove"]',
    )
    assert.equal(removes.length, 2)
    act(() => {
      removes[0].click()
    })
    const langsAfter = Array.from(
      container.querySelectorAll<HTMLInputElement>(
        '[data-testid="repl-kernel-lang"]',
      ),
    ).map((i) => i.value)
    assert.deepEqual(langsAfter, ['node'])
  })

  /**
   * Drive a controlled <input>: React only re-renders on a synthetic
   * onChange. happy-dom dispatches 'input' events for `value =` but
   * React 18 doesn't pick those up reliably, so we set the value
   * via the host element's value-setter (the path React's
   * synthetic-event system recognises) and then fire 'input'.
   *
   * Pulled from a well-known idiom — see the React Testing Library
   * `fireEvent.input` source for the same dance.
   */
  function setInputValue(input: HTMLInputElement, value: string): void {
    const proto = Object.getPrototypeOf(input)
    const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set
    if (setter) setter.call(input, value)
    else input.value = value
    input.dispatchEvent(new Event('input', { bubbles: true }))
  }

  it('persists rows to configStore on Save and clears the dirty flag', () => {
    seedConfig('{}')
    mount()

    // Save starts disabled (no dirty, no rows).
    const saveBtn = () =>
      container.querySelector(
        '[data-testid="repl-kernels-save"]',
      ) as HTMLButtonElement | null
    assert.equal(saveBtn()?.disabled, true)

    // Add a row + populate it.
    act(() => {
      ;(
        container.querySelector(
          '[data-testid="repl-kernel-add"]',
        ) as HTMLButtonElement
      ).click()
    })
    const langInput = container.querySelector(
      '[data-testid="repl-kernel-lang"]',
    ) as HTMLInputElement
    const cmdInput = container.querySelector(
      '[data-testid="repl-kernel-command"]',
    ) as HTMLInputElement
    act(() => {
      setInputValue(langInput, 'python')
    })
    act(() => {
      setInputValue(cmdInput, 'python3 -i')
    })

    // Now dirty + savable.
    assert.equal(saveBtn()?.disabled, false)
    const dirtyHint = container.querySelector(
      '[data-testid="repl-kernels-dirty"]',
    )
    assert.ok(dirtyHint, 'dirty hint should render')

    act(() => {
      saveBtn()!.click()
    })

    // configStore now carries the canonical JSON.
    const persisted = readPersisted()
    assert.ok(persisted)
    assert.equal(JSON.parse(persisted!).python, 'python3 -i')

    // Dirty flag gone.
    assert.equal(
      container.querySelector('[data-testid="repl-kernels-dirty"]'),
      null,
    )
    assert.equal(saveBtn()?.disabled, true)
  })

  it('refuses to save when two rows share the same language tag', () => {
    seedConfig('{}')
    mount()
    const addBtn = container.querySelector(
      '[data-testid="repl-kernel-add"]',
    ) as HTMLButtonElement
    act(() => {
      addBtn.click()
    })
    act(() => {
      addBtn.click()
    })
    const langs = container.querySelectorAll<HTMLInputElement>(
      '[data-testid="repl-kernel-lang"]',
    )
    const cmds = container.querySelectorAll<HTMLInputElement>(
      '[data-testid="repl-kernel-command"]',
    )
    act(() => {
      setInputValue(langs[0], 'python')
      setInputValue(cmds[0], 'python3 -i')
      setInputValue(langs[1], 'python')
      setInputValue(cmds[1], 'pypy3 -i')
    })

    const save = container.querySelector(
      '[data-testid="repl-kernels-save"]',
    ) as HTMLButtonElement
    assert.equal(save.disabled, true, 'duplicate langs should block Save')
    const warn = container.querySelector(
      '[data-testid="repl-kernels-duplicate-warning"]',
    )
    assert.ok(warn, 'duplicate warning should surface')
  })

  it('Reset discards local edits and restores the persisted state', () => {
    seedConfig('{"python":"python3 -i"}')
    mount()
    const lang = container.querySelector(
      '[data-testid="repl-kernel-lang"]',
    ) as HTMLInputElement
    act(() => {
      setInputValue(lang, 'edited-locally')
    })
    // Now dirty.
    const reset = container.querySelector(
      '[data-testid="repl-kernels-reset"]',
    ) as HTMLButtonElement
    assert.equal(reset.disabled, false)
    act(() => {
      reset.click()
    })
    const langAfter = container.querySelector(
      '[data-testid="repl-kernel-lang"]',
    ) as HTMLInputElement
    assert.equal(langAfter.value, 'python', 'lang reverted to persisted')
    // Persisted JSON untouched.
    assert.equal(
      readPersisted(),
      '{"python":"python3 -i"}',
      'Reset should not touch the persisted store',
    )
  })

  it('rehydrates from the store when the persisted value changes from underneath', () => {
    seedConfig('{}')
    mount()
    assert.equal(
      container.querySelectorAll('[data-testid="repl-kernel-row"]').length,
      0,
    )
    // Simulate a forge switch / external write landing in the store.
    act(() => {
      useConfigStore.setState({
        values: { [CONFIG_REPL_KERNELS]: '{"ruby":"irb"}' },
        hydrated: true,
      })
    })
    const langs = Array.from(
      container.querySelectorAll<HTMLInputElement>(
        '[data-testid="repl-kernel-lang"]',
      ),
    ).map((i) => i.value)
    assert.deepEqual(langs, ['ruby'])
  })
})
