// Tier-2: git surface.
//
// The shell has no staging UI yet (tier1/git.spec.ts already notes
// this). Tier-2 sticks to the kernel-invocation path GitPage exposes
// and covers:
//   - repeated status calls are idempotent (no accumulating state),
//   - the return shape is either null (non-repo vault — today) or an
//     object with the documented keys.
//
// Branch-name display and external-change refresh are both gated on a
// source-control pane that doesn't ship; both are it.skip'd.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { GitPage } from '../../pages/GitPage.js'

describe('tier2: git', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('repeated status calls are idempotent', async () => {
    const first = await GitPage.status()
    const second = await GitPage.status()
    // Either both null (non-repo vault) or both objects with a matching
    // branch field. We don't assert on OID equality — a future fixture
    // that seeds a repo might tick HEAD, but branch name won't drift
    // between two calls in the same second.
    if (first === null) {
      expect(second).toBeNull()
    } else {
      expect(second).not.toBeNull()
      expect((second as { branch: unknown }).branch).toEqual(
        (first as { branch: unknown }).branch,
      )
    }
  })

  it('status shape has the documented keys when non-null', async () => {
    const snap = await GitPage.status()
    if (snap === null) {
      // Fixture vault has no .git today; null is the expected path.
      expect(snap).toBeNull()
      return
    }
    // Keys from GitStatusSnapshot in GitPage.ts.
    expect(snap).toHaveProperty('branch')
    expect(snap).toHaveProperty('is_dirty')
    expect(snap).toHaveProperty('head_oid')
  })

  // Skipped: no source-control pane in the shell yet, so there's no
  // DOM-level branch-name display to refresh against an external git
  // mutation. Kernel-level refresh is already covered by nexus-git
  // tests.
  it.skip('status refreshes after an external change', async () => {
    // no-op
  })

  it.skip('branch name is surfaced in a status pane', async () => {
    // no-op
  })
})
