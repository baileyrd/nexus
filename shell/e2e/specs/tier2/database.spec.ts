// Tier-2: database (bases) — deeper flows are almost entirely gated on
// UI that doesn't ship yet.
//
// Tier-1 exercises the New-Base command + dialog overlay visibility.
// Tier-2 verifies the dialog is stable across re-invocation (the
// command is idempotent — re-invoking does not stack overlays).
//
// Column sort, row count, cell edit commit/cancel all depend on a
// BasesTable with stable selectors against a seeded `.bases` fixture —
// and there is no `.bases` directory in e2e/fixtures/vault today. All
// three are it.skip'd.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { DatabasePage } from '../../pages/DatabasePage.js'

describe('tier2: database (bases)', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('re-invoking New-Base does not stack multiple dialog overlays', async () => {
    await DatabasePage.startNewBase('')
    await DatabasePage.startNewBase('')
    // role="dialog" should be at most one element regardless of how
    // many times the command fires. The NewBaseDialog is a singleton
    // view registered against the `overlay` slot.
    const dialogs = await $$('div[role="dialog"]')
    expect(dialogs.length).toBeLessThanOrEqual(1)
  })

  // Skipped: needs a seeded `.bases` fixture directory. Selectors are
  // now in place (each <th> carries aria-label="Sort by {field}" and
  // aria-sort reflects asc/desc/none) — blocker is fixture-only.
  it.skip('column header click toggles sort order', async () => {
    // no-op
  })

  // Skipped: selectors exist (each row is role="row" with
  // data-testid="record-row-{id}") but fixture vault has no .bases/.
  it.skip('row count reflects the seed', async () => {
    // no-op
  })

  // Skipped: selectors exist (the inline CellEditor carries
  // aria-label="Commit cell"; cancel is keyboard-only via Escape with
  // the editor unmounting). Still needs a seeded base fixture.
  it.skip('cell edit commit persists; cancel reverts', async () => {
    // no-op
  })
})
