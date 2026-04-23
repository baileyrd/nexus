// Page object for the skills sidebar view (nexus.skills).
// See shell/src/plugins/nexus/skills/SkillsView.tsx.

const VIEW_ID = 'nexus.skills.view'
const COMMAND_SHOW = 'nexus.skills.show'
const COMMAND_REFRESH = 'nexus.skills.refresh'

export class SkillsPage {
  static async openPanel(): Promise<void> {
    await browser.execute(async (cmd: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute(cmd)
    }, COMMAND_SHOW)
  }

  static async refresh(): Promise<void> {
    await browser.execute(async (cmd: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute(cmd)
    }, COMMAND_REFRESH)
  }

  /** Number of skill rows rendered. */
  static async skillCount(): Promise<number> {
    // Each skill row wraps a role="button" header with aria-expanded.
    const rows = await $$('div[role="button"][aria-expanded]')
    return rows.length
  }

  /** Expand the Nth skill row by clicking its header. */
  static async expandByIndex(index: number): Promise<void> {
    const rows = await $$('div[role="button"][aria-expanded]')
    const row = rows[index]
    if (!row) throw new Error(`no skill row at index ${index}`)
    await row.click()
  }

  static readonly VIEW_ID = VIEW_ID
}
