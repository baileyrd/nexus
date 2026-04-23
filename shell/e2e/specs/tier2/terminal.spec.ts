// Tier-2: terminal panel.
//
// The core terminal is still a placeholder (see TerminalView.tsx) with
// three built-ins: `echo`, `clear`, `help`, plus a "Command not found"
// fallback for anything else. Tier-2 covers:
//   - `help` prints the usage line,
//   - `clear` wipes all rendered lines,
//   - unknown commands render the error line without crashing.
//
// Skipped:
//   - history up-arrow: the TerminalView has no history buffer — the
//     input is a single controlled textarea with no key handler beyond
//     Enter. Revisit when a PTY wrapper lands.
//   - exit codes: the placeholder doesn't emit exit codes.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { TerminalPage } from '../../pages/TerminalPage.js'

describe('tier2: terminal', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('`help` prints the available-commands line', async () => {
    await TerminalPage.runCommand('help')
    const lines = await TerminalPage.readAllLines()
    const joined = lines.join('\n')
    // TerminalView.tsx line 30: `Available: clear, help, echo <text>`
    expect(joined).toContain('Available:')
    expect(joined).toContain('echo')
  })

  it('`clear` wipes all rendered output lines', async () => {
    // Seed some output first so "cleared" is meaningful.
    await TerminalPage.runCommand('echo seed-for-clear')
    let lines = await TerminalPage.readAllLines()
    expect(lines.length).toBeGreaterThan(0)

    await TerminalPage.runCommand('clear')
    await browser.waitUntil(
      async () => (await TerminalPage.readAllLines()).length === 0,
      { timeout: 5_000, timeoutMsg: 'terminal lines not cleared' },
    )
    lines = await TerminalPage.readAllLines()
    expect(lines.length).toBe(0)
  })

  it('unknown command renders a "Command not found" error line', async () => {
    await TerminalPage.runCommand('nosuchbin')
    const lines = await TerminalPage.readAllLines()
    const joined = lines.join('\n')
    // TerminalView.tsx line 35.
    expect(joined).toContain('Command not found')
  })

  // Skipped: TerminalView has no command history buffer. Up-arrow is
  // a no-op in the current placeholder. Revisit once a real PTY
  // wrapper ships.
  it.skip('up-arrow recalls the previous command', async () => {
    // no-op
  })

  // Skipped: placeholder terminal does not emit exit codes — commands
  // map to built-in strings only. Revisit with tauri-plugin-shell.
  it.skip('exit code is surfaced for the last command', async () => {
    // no-op
  })
})
