// Page object for the terminal panel (nexus.terminal wraps the core
// terminal plugin's placeholder textarea-based terminal). See
// shell/src/plugins/core/terminal/TerminalView.tsx — it handles a
// small set of built-ins (`echo`, `clear`, `help`) plus a friendly
// "Command not found" fallback.

const VIEW_ID = 'nexus.terminal.panelView'
const COMMAND_FOCUS = 'nexus.terminal.focus'

export class TerminalPage {
  static async openPanel(): Promise<void> {
    await browser.execute(async (cmd: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute(cmd)
    }, COMMAND_FOCUS)
  }

  /** Focus the input, type a command, press Enter. */
  static async runCommand(cmd: string): Promise<void> {
    await TerminalPage.openPanel()
    const input = await $('.terminal-input')
    await input.waitForExist({ timeout: 10_000 })
    await input.click()
    await input.setValue(cmd)
    await browser.keys(['Enter'])
  }

  /** Read all output lines as plain text. */
  static async readAllLines(): Promise<string[]> {
    const lines = await $$('.terminal-line')
    const out: string[] = []
    for (const l of lines) out.push(await l.getText())
    return out
  }

  static readonly VIEW_ID = VIEW_ID
}
