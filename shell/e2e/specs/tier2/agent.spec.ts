// Tier-2: agent pane — composer validation + mode toggle state.
//
// Tier-1 already exercises openPanel + setGoal + setRunMode. Tier-2
// covers:
//   - Plan button is disabled when the goal is empty (drives the real
//     `disabled` attr from AgentView's `busy || goal.trim() === ''`),
//   - Plan button enables once the goal has content,
//   - history empty-state renders the centered "No past runs." copy on
//     a fresh vault.
//
// Plan generation + step-approval flows remain it.skip'd pending a
// fake-LLM adapter (same rationale as tier1).

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { AgentPage } from '../../pages/AgentPage.js'

describe('tier2: agent', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('Plan and Run buttons are disabled when the goal is empty', async () => {
    await AgentPage.openPanel()
    await AgentPage.setGoal('')
    const plan = await $('button=Plan')
    const run = await $('button=Run')
    await plan.waitForExist({ timeout: 10_000 })
    expect(await plan.isEnabled()).toBe(false)
    expect(await run.isEnabled()).toBe(false)
  })

  it('Plan button enables once the goal has non-whitespace text', async () => {
    await AgentPage.openPanel()
    await AgentPage.setGoal('write a short summary of notes/a.md')
    const plan = await $('button=Plan')
    await browser.waitUntil(async () => plan.isEnabled(), {
      timeout: 5_000,
      timeoutMsg: 'Plan button never enabled after goal text',
    })
    expect(await plan.isEnabled()).toBe(true)
  })

  it('fresh vault shows zero history rows after refresh', async () => {
    await AgentPage.openPanel()
    await AgentPage.refreshHistory()
    // History is persisted under .forge/agent/history — the fixture
    // forge has only a .gitkeep, so the list is empty.
    await browser.waitUntil(
      async () => (await AgentPage.historyCount()) === 0,
      { timeout: 10_000, timeoutMsg: 'history count never settled to 0' },
    )
    expect(await AgentPage.historyCount()).toBe(0)
  })

  it('run-mode toggle exposes both Auto and Step as buttons', async () => {
    await AgentPage.openPanel()
    const group = await $('div[role="group"][aria-label="Run mode"]')
    await group.waitForExist({ timeout: 10_000 })
    const auto = await group.$('button=Auto')
    const step = await group.$('button=Step')
    expect(await auto.isExisting()).toBe(true)
    expect(await step.isExisting()).toBe(true)
  })

  // Skipped: plan/run scenarios depend on a reachable LLM backend.
  // See tier1/agent.spec.ts for the full rationale.
  it.skip('history ordering is newest-first after two runs', async () => {
    // no-op — needs fake LLM adapter
  })

  it('empty forge with no history shows the empty-state element', async () => {
    await AgentPage.openPanel()
    await AgentPage.refreshHistory()
    // HistoryColumn renders a role=status wrapper with aria-label="No
    // history" when the store's history array is empty. Asserts the
    // element's existence without hardcoding the visible copy.
    const empty = await $('[aria-label="No history"]')
    await empty.waitForExist({ timeout: 10_000 })
    expect(await empty.isExisting()).toBe(true)
  })
})
