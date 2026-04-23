// Page object for the bases (database) surface (nexus.bases).
//
// Bases are `.bases` directories rendered through BasesView. The
// "inline database inside a note" pattern (Notion-style) is not yet
// wired in the shell — today databases live in their own `.bases`
// folders and open as their own tab. This page object wraps the
// directory-level open flow; inline-cell editing is marked it.skip
// in tier1/database.spec.ts.

const COMMAND_NEW_BASE = 'nexus.bases.new'

export class DatabasePage {
  /** Invoke the "New base…" command. Opens the NewBaseDialog overlay. */
  static async startNewBase(parent = ''): Promise<void> {
    await browser.execute(
      async (cmd: string, args: { parent: string }) => {
        const api = (window as unknown as { __nexusShellApi?: {
          commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
        } }).__nexusShellApi
        if (!api?.commands) throw new Error('shell plugin API missing commands')
        await api.commands.execute(cmd, args)
      },
      COMMAND_NEW_BASE,
      { parent },
    )
  }

  /** Open a `.bases` directory as a tab via the standard files:open
   *  event. The editor plugin routes the `bases` extension through
   *  the bases pane view creator. */
  static async openBase(relpath: string): Promise<void> {
    await browser.execute((rel: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        events?: { emit: (topic: string, payload: unknown) => void }
      } }).__nexusShellApi
      if (!api?.events) throw new Error('shell plugin API missing events')
      const name = rel.split('/').pop() ?? rel
      api.events.emit('files:open', { relpath: rel, name })
    }, relpath)
  }

  /** Whether the new-base dialog is currently visible. */
  static async dialogVisible(): Promise<boolean> {
    const el = await $('div[role="dialog"]')
    return el.isExisting()
  }
}
