// Page object for the workflow sidebar view (nexus.workflow).
// See shell/src/plugins/nexus/workflow/WorkflowView.tsx.

const VIEW_ID = 'nexus.workflow.view'
const COMMAND_SHOW = 'nexus.workflow.show'
const COMMAND_REFRESH = 'nexus.workflow.refresh'

export class WorkflowPage {
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

  /** Count workflow rows rendered. Each row has exactly one "Run
   *  workflow" button, so counting those is the simplest proxy. */
  static async workflowCount(): Promise<number> {
    const btns = await $$('button[aria-label="Run workflow"]')
    return btns.length
  }

  /** Click the Run button on the Nth workflow row (0-indexed). */
  static async runByIndex(index: number): Promise<void> {
    const btns = await $$('button[aria-label="Run workflow"]')
    const btn = btns[index]
    if (!btn) throw new Error(`no workflow row at index ${index}`)
    await btn.waitForClickable({ timeout: 10_000 })
    await btn.click()
  }

  static readonly VIEW_ID = VIEW_ID
}
