// Golden-path e2e: open → edit → save → reopen roundtrip.
//
// Assumes `pnpm e2e:build` has produced the debug Tauri binary. tauri-driver
// is spawned by wdio.conf.ts `onPrepare`. Each `it` runs sequentially inside
// one browser/app session.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../wdio.conf.js'
import {
  closeTab,
  openFile,
  openVault,
  readEditorText,
  readSavedFile,
  reopen,
  save,
  typeInEditor,
  undo,
} from '../support/app.js'

describe('golden path: edit/save/undo/close', () => {
  // The scratch vault is cloned + the kernel pre-booted against it by
  // wdio.conf.ts `onPrepare` and the Rust `setup` hook respectively.
  const scratch = SCRATCH_VAULT

  before(async () => {
    await openVault(scratch)
  })

  it('edit + save + reopen roundtrip', async () => {
    await openFile('notes/a.md')
    const originalText = await readEditorText()
    expect(originalText).toContain('# Alpha')

    await typeInEditor('\n## Section three\n')
    await save()

    const savedOnDisk = await readSavedFile(scratch, 'notes/a.md')
    expect(savedOnDisk).toContain('## Section three')

    await closeTab()
    await reopen('notes/a.md')

    const reopened = await readEditorText()
    expect(reopened).toContain('## Section three')
  })

  it('undo past save returns to post-save state, then pre-save', async () => {
    await openFile('notes/a.md')
    await typeInEditor('\nFIRST_EDIT')
    await save()
    await typeInEditor('\nSECOND_EDIT')

    await undo()
    // Kernel owns UndoTree across saves (bridge plan decision #3) — one
    // undo should strip SECOND_EDIT but leave FIRST_EDIT (persisted).
    let text = await readEditorText()
    expect(text).toContain('FIRST_EDIT')
    expect(text).not.toContain('SECOND_EDIT')

    await undo()
    text = await readEditorText()
    expect(text).not.toContain('FIRST_EDIT')
  })

  it('close without save preserves disk state', async () => {
    await openFile('notes/a.md')
    const diskBefore = await readSavedFile(scratch, 'notes/a.md')

    await typeInEditor('\nUNSAVED_GARBAGE')
    await closeTab()
    // If a save-prompt dialog appears the harness currently dismisses by
    // pressing Escape. The kernel's default on close is discard-buffer, so
    // no dialog is expected today.
    await browser.keys(['Escape'])

    await reopen('notes/a.md')
    const reopened = await readEditorText()
    expect(reopened).not.toContain('UNSAVED_GARBAGE')

    const diskAfter = await readSavedFile(scratch, 'notes/a.md')
    expect(diskAfter).toBe(diskBefore)
  })
})
