// Tier-1: agent pane.
//
// Exercises the pane open + goal entry + history column. Plan
// generation and run are driven through the command-API path — we
// don't assert on plan content because it depends on an LLM
// backend. Step-by-step approval UI scenarios are marked it.skip
// since the DOM affordances for per-step approve/skip aren't
// uniquely selectable yet.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { AgentPage } from '../../pages/AgentPage.js'

describe('tier1: agent', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('opens the agent pane and renders composer + history column', async () => {
    await AgentPage.openPanel()
    const goal = await $('textarea[placeholder="Describe what the agent should do…"]')
    await goal.waitForExist({ timeout: 10_000 })
    const refresh = await $('button[title="Refresh history"]')
    expect(await refresh.isExisting()).toBe(true)
  })

  it('refreshes history without error', async () => {
    await AgentPage.openPanel()
    await AgentPage.refreshHistory()
    // History may be empty on a fresh vault; we just care that the
    // refresh button is clickable and the count query doesn't throw.
    const count = await AgentPage.historyCount()
    expect(count).toBeGreaterThanOrEqual(0)
  })

  it('accepts a goal and toggles run-mode', async () => {
    await AgentPage.openPanel()
    await AgentPage.setGoal('summarise notes/a.md')
    await AgentPage.setRunMode('step')
    await AgentPage.setRunMode('auto')
    const ta = await $('textarea[placeholder="Describe what the agent should do…"]')
    expect(await ta.getValue()).toContain('summarise')
  })

  // Skipped: requesting a plan or running end-to-end depends on an
  // LLM backend being reachable. In CI and most local e2e runs the
  // plan call will fail with a provider error — not a product bug.
  // Revisit once we have a deterministic fake-LLM adapter wired for
  // e2e.
  it.skip('generates a plan DAG and renders step rows', async () => {
    // no-op — needs fake LLM adapter
  })

  it.skip('approves steps in step-mode and records history', async () => {
    // no-op — needs fake LLM adapter + stable per-step selectors
  })
})
