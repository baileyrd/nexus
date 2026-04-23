// Tier-1: editor surface (beyond the golden-path).
//
// The golden-path spec already covers edit/save/reopen + undo +
// close-without-save. This file adds coverage for: opening a second
// note, cross-tab switching, and a save round-trip through the page
// object (proving the wrapper compiles + drives the same path).

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault, readSavedFile } from '../../support/app.js'
import { EditorPage } from '../../pages/EditorPage.js'

describe('tier1: editor', () => {
  const scratch = SCRATCH_VAULT

  before(async () => {
    await openVault(scratch)
  })

  it('opens a second note (notes/b.md) after closing the first', async () => {
    await EditorPage.openNote('notes/a.md')
    await EditorPage.waitForMounted()
    const a = await EditorPage.readText()
    expect(a.length).toBeGreaterThan(0)

    await EditorPage.closeTab()
    await EditorPage.openNote('notes/b.md')
    await EditorPage.waitForMounted()
    const b = await EditorPage.readText()
    expect(b.length).toBeGreaterThan(0)
    // a.md and b.md are distinct fixture files; their first headings
    // should differ. We don't hardcode the exact heading string here
    // — just assert the body text changed between opens.
    expect(b).not.toBe(a)
  })

  it('typed edits persist to disk through the page-object save path', async () => {
    await EditorPage.openNote('notes/b.md')
    await EditorPage.waitForMounted()
    await EditorPage.type('\n<!-- tier1-editor-probe -->\n')
    await EditorPage.save()

    const onDisk = await readSavedFile(scratch, 'notes/b.md')
    expect(onDisk).toContain('tier1-editor-probe')
  })
})
