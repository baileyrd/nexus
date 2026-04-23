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

  // Skipped: needs a seeded `.bases` fixture directory and stable
  // BasesTable column-header selectors. Fixture vault has no .bases/
  // today.
  it.skip('column header click toggles sort order', async () => {
    // no-op
  })

  // Skipped: row count assertion requires a seeded base with a known
  // record count. See above — no fixture yet.
  it.skip('row count reflects the seed', async () => {
    // no-op
  })

  // Skipped: cell-commit / cell-cancel have no uniquely addressable
  // selectors in BasesTable today (cells are DOM-level inputs with no
  // aria-label distinguishing commit vs cancel). Flagged for a future
  // UI task — needs stable affordances on the grid.
  it.skip('cell edit commit persists; cancel reverts', async () => {
    // no-op
  })
})
