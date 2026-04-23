// Tier-1: git surface.
//
// The shell exposes git as a compact status-bar item only (no full
// status pane yet). The kernel `com.nexus.git` handlers (status,
// stage, commit) exist and are tested at the kernel level — driving
// them from e2e without a staging UI would duplicate kernel tests
// rather than cover a user flow.
//
// What we CAN cover: the status handler is invocable from the
// WebView context, and on a non-git fixture vault it returns null
// / rejects cleanly without crashing the shell.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { GitPage } from '../../pages/GitPage.js'

describe('tier1: git', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('git status invocation resolves (value may be null on non-repo vaults)', async () => {
    const snap = await GitPage.status()
    // On the fixture vault there is no .git; the plugin returns null.
    // If a future fixture seeds a repo, branch/is_dirty will be
    // populated. Either outcome is acceptable — we assert the call
    // itself doesn't throw and the shape is sane.
    if (snap !== null) {
      expect(typeof snap).toBe('object')
    }
  })

  // Skipped: no staging UI exists in the shell yet. stage/commit
  // flows are exercised by kernel tests (crates/nexus-git). Revisit
  // once a source-control pane lands.
  it.skip('stages + commits a trivial change via the UI', async () => {
    // no-op
  })
})
