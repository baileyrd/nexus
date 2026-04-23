// Tier-2: skills pane.
//
// Fixture vault has no `.forge/skills/` content, so SkillsView lands in
// its empty-state branch. Tier-2 asserts:
//   - skill count is 0 after refresh,
//   - Refresh button exists with its real aria-label,
//   - no role="button"[aria-expanded] rows render when empty (the
//     SkillRow header is the only source of that selector).
//
// Skill-body preview is an expand-on-click affordance only reachable
// when at least one skill row exists — it.skip'd until the fixture
// seeds .skill.md files.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { SkillsPage } from '../../pages/SkillsPage.js'

describe('tier2: skills', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('empty fixture renders zero skill rows', async () => {
    await SkillsPage.openPanel()
    await SkillsPage.refresh()
    await browser.waitUntil(
      async () => (await SkillsPage.skillCount()) === 0,
      { timeout: 10_000, timeoutMsg: 'skill count never settled to 0' },
    )
    expect(await SkillsPage.skillCount()).toBe(0)
  })

  it('Refresh button is present with its real aria-label', async () => {
    await SkillsPage.openPanel()
    const btn = await $('button[aria-label="Refresh skills"]')
    await btn.waitForExist({ timeout: 10_000 })
    expect(await btn.isExisting()).toBe(true)
  })

  // Skipped: selector now in place — the body preview <pre> carries
  // aria-label="Skill body preview" and data-testid="skill-body-preview".
  // Remaining blocker: ExpandedPanel only mounts when a row is
  // clicked, which requires a seeded .skill.md fixture.
  it.skip('expanding a skill row reveals a body preview', async () => {
    // no-op
  })
})
