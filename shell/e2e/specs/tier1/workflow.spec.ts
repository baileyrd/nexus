// Tier-1: workflow pane.
//
// Lists `.workflow.toml` definitions in the forge. The fixture vault
// under e2e/fixtures/vault/ currently has no .workflows/ directory,
// so the spec asserts the empty-state path + the refresh command
// round-trip. Actual manual-run assertions are it.skip until we
// seed a workflow fixture.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { WorkflowPage } from '../../pages/WorkflowPage.js'

describe('tier1: workflow', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('opens the workflow panel and lists zero-or-more workflows', async () => {
    await WorkflowPage.openPanel()
    await WorkflowPage.refresh()
    const count = await WorkflowPage.workflowCount()
    expect(count).toBeGreaterThanOrEqual(0)
  })

  // Skipped: no workflow fixture in e2e/fixtures/vault/.workflows/
  // yet. When one lands, assert count > 0 and exercise runByIndex(0).
  it.skip('runs a workflow manually and shows output', async () => {
    // no-op — needs fixture workflow seeded
  })
})
