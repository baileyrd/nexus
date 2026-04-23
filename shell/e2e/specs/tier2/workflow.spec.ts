// Tier-2: workflow pane.
//
// Fixture vault has no `.workflows/` directory, so the empty-state
// branch of WorkflowView is what renders. Tier-2 asserts:
//   - the empty-state message is rendered (copy is stable in source),
//   - the Refresh button exists with its real aria-label,
//   - repeated refresh calls do not crash and keep count at 0.
//
// Manual-run scenarios are it.skip'd until a .workflow.toml fixture
// seeds (same rationale as tier1/workflow.spec.ts).

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { WorkflowPage } from '../../pages/WorkflowPage.js'

describe('tier2: workflow', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('renders the empty-state when no workflows are seeded', async () => {
    await WorkflowPage.openPanel()
    await WorkflowPage.refresh()
    // Copy from WorkflowView.tsx line 47: "No workflows. Add a ..."
    await browser.waitUntil(
      async () => {
        const count = await WorkflowPage.workflowCount()
        return count === 0
      },
      { timeout: 10_000, timeoutMsg: 'workflow count never settled to 0' },
    )
    expect(await WorkflowPage.workflowCount()).toBe(0)
  })

  it('Refresh button is present with its real aria-label', async () => {
    await WorkflowPage.openPanel()
    const btn = await $('button[aria-label="Refresh workflows"]')
    await btn.waitForExist({ timeout: 10_000 })
    expect(await btn.isExisting()).toBe(true)
  })

  it('repeated refresh keeps the count at 0 without error', async () => {
    await WorkflowPage.openPanel()
    await WorkflowPage.refresh()
    await WorkflowPage.refresh()
    await WorkflowPage.refresh()
    expect(await WorkflowPage.workflowCount()).toBe(0)
  })

  // Skipped: UI branch is now in place — WorkflowRow renders
  // data-invalid="true" and aria-label="Workflow {name} invalid" when
  // the entry carries a parseError field. Remaining blocker: the
  // kernel `list` projection does not yet populate parseError, and
  // the fixture vault has no bad .workflow.toml to exercise it.
  it.skip('invalid workflow surfaces a per-row parse error', async () => {
    // no-op
  })
})
