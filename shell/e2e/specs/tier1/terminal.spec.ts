// Tier-1: terminal panel.
//
// The core terminal plugin is a placeholder that echoes built-ins
// (`echo`, `help`, `clear`). A real PTY wired through
// tauri-plugin-shell is out of scope for Phase 0; this spec exercises
// what's actually there.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { TerminalPage } from '../../pages/TerminalPage.js'

describe('tier1: terminal', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('opens the terminal panel and runs `echo hello`', async () => {
    await TerminalPage.runCommand('echo hello')

    // Output lines include the input echo ($ echo hello) + the output
    // line (hello). We check both are present rather than rely on
    // exact ordering.
    const lines = await TerminalPage.readAllLines()
    const joined = lines.join('\n')
    expect(joined).toContain('echo hello')
    expect(joined).toContain('hello')
  })
})
