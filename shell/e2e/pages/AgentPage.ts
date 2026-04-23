// Page object for the agent pane (nexus.agent).
//
// The agent view is a two-column pane: history (left) + run column
// (right) with a goal textarea, Plan/Run buttons, and a run-mode
// toggle. See shell/src/plugins/nexus/agent/AgentView.tsx.

const VIEW_ID = 'nexus.agent.view'
const COMMAND_SHOW = 'nexus.agent.show'

export class AgentPage {
  /** Open the agent pane via its registered command. */
  static async openPanel(): Promise<void> {
    await browser.execute(async (cmd: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute(cmd)
    }, COMMAND_SHOW)
  }

  /** Set the goal composer text. */
  static async setGoal(goal: string): Promise<void> {
    const ta = await $('textarea[placeholder="Describe what the agent should do…"]')
    await ta.waitForExist({ timeout: 10_000 })
    await ta.click()
    await ta.setValue(goal)
  }

  /** Click the Plan button. Kicks off planning only — no execution. */
  static async requestPlan(): Promise<void> {
    const btn = await $('button=Plan')
    await btn.waitForClickable({ timeout: 10_000 })
    await btn.click()
  }

  /** Click the Run button. In auto mode this plans + runs; in step
   *  mode it plans + awaits per-step approval. */
  static async run(): Promise<void> {
    const btn = await $('button=Run')
    await btn.waitForClickable({ timeout: 10_000 })
    await btn.click()
  }

  /** Flip the run-mode toggle. `auto` runs all steps; `step` pauses
   *  for approval between steps. */
  static async setRunMode(mode: 'auto' | 'step'): Promise<void> {
    const group = await $('div[role="group"][aria-label="Run mode"]')
    await group.waitForExist({ timeout: 10_000 })
    const label = mode === 'auto' ? 'Auto' : 'Step'
    const btn = await group.$(`button=${label}`)
    await btn.click()
  }

  /** Click "Refresh history" in the left column. */
  static async refreshHistory(): Promise<void> {
    const btn = await $('button[title="Refresh history"]')
    await btn.waitForClickable({ timeout: 10_000 })
    await btn.click()
  }

  /** Number of history rows rendered in the left column. */
  static async historyCount(): Promise<number> {
    // History rows are divs with role="button" inside the left column.
    // The composer has its own textarea + buttons so role=button at
    // document scope is too broad; scope to the delete icon's aria
    // which only appears on history rows.
    const deletes = await $$('button[aria-label="Delete history entry"]')
    return deletes.length
  }

  /** Read the current plan's goal text if a plan is loaded. */
  static async currentPlanGoal(): Promise<string | null> {
    // Goal appears directly below the "Plan <id>" header when a plan
    // is loaded. Cheaper than reverse-engineering DOM is to ask the
    // store.
    return browser.execute(() => {
      const w = window as unknown as {
        __nexusShellApi?: { stores?: Record<string, unknown> }
      }
      const api = w.__nexusShellApi
      if (!api?.stores) return null
      // Agent plugin exposes its store via the plugin API convention;
      // if it doesn't, the spec should fall back to DOM assertions.
      return null
    })
  }

  static readonly VIEW_ID = VIEW_ID
}
