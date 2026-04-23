// Tier-2: editor surface — deeper flows than the golden-path + tier1.
//
// Tier-1 covers open/save/reopen/undo. Tier-2 exercises:
//   - multiline edits roundtrip through the page-object save path,
//   - the toggleMode command (source ↔ preview) — a real editor plugin
//     command with no visible aria-labelled trigger (invoked via the
//     commands API the same way support/app.ts drives save/close),
//   - the newUntitled command mounts a CM instance with an empty buffer.
//
// Flows requiring UI that doesn't exist today are it.skip'd:
//   - autosave debounce (no autosave timer lives in the editor plugin;
//     save is explicit via nexus.editor.save).
//   - slash-command dispatch (no `/` palette in the active editor).
//   - markdown format toggles (bold/italic have no aria-labelled UI
//     triggers — only CM6 keybindings which don't survive the shell's
//     global keybinding dispatcher as noted in support/app.ts save()).

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault, readSavedFile } from '../../support/app.js'
import { EditorPage } from '../../pages/EditorPage.js'

describe('tier2: editor', () => {
  const scratch = SCRATCH_VAULT

  before(async () => {
    await openVault(scratch)
  })

  it('multiline edits roundtrip through save + disk read', async () => {
    await EditorPage.openNote('notes/a.md')
    await EditorPage.waitForMounted()
    await EditorPage.type('\n<!-- tier2-line-one -->\n<!-- tier2-line-two -->\n')
    await EditorPage.save()

    const onDisk = await readSavedFile(scratch, 'notes/a.md')
    expect(onDisk).toContain('tier2-line-one')
    expect(onDisk).toContain('tier2-line-two')
  })

  it('toggleMode command switches the editor in and out of source mode', async () => {
    await EditorPage.openNote('notes/a.md')
    await EditorPage.waitForMounted()

    // Source mode → preview. openVault() pins defaultMode=source, so the
    // tab starts with a .cm-content. Toggling once should unmount it.
    await browser.execute(async () => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute('nexus.editor.toggleMode')
    })

    await browser.waitUntil(async () => !(await $('.cm-content').isExisting()), {
      timeout: 10_000,
      timeoutMsg: '.cm-content never unmounted after toggleMode',
    })

    // Toggle back to source — CM remounts.
    await browser.execute(async () => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute('nexus.editor.toggleMode')
    })
    await EditorPage.waitForMounted()
    expect(await $('.cm-content').isExisting()).toBe(true)
  })

  it('newUntitled command mounts a blank CM buffer', async () => {
    await browser.execute(async () => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute('nexus.editor.newUntitled')
    })
    await EditorPage.waitForMounted()
    const text = await EditorPage.readText()
    // Untitled buffers start empty; CM renders a single empty line.
    expect(text.trim().length).toBe(0)
  })

  // Skipped: no autosave timer in the editor plugin — save is explicit
  // via nexus.editor.save. Revisit once debounced-autosave lands.
  it.skip('autosave debounce persists without explicit save', async () => {
    // no-op
  })

  // Skipped: cursor-position restoration on reopen isn't surfaced by
  // any observable selector today — CM6 owns the selection internally
  // and the editor plugin doesn't mirror it to the DOM in a way the
  // harness can query deterministically.
  it.skip('reopen preserves cursor position', async () => {
    // no-op
  })

  // Skipped: no slash-command palette in the active CM6 editor. The
  // palette command exists at the shell level (Ctrl+Shift+P) but is
  // not a `/`-dispatched inline trigger.
  it.skip('slash command dispatches an editor action', async () => {
    // no-op
  })

  // Skipped: markdown format toggles (bold/italic) have no
  // aria-labelled toolbar button in the current editor — only CM6
  // keybindings, which the shell's global keybinding dispatcher
  // intercepts (see support/app.ts save() comment).
  it.skip('markdown format toggles apply bold/italic', async () => {
    // no-op
  })
})
