// Page object for the git status surface (nexus.gitStatus).
//
// The shell currently only renders a compact status-bar indicator for
// git (GitStatusItem in statusBarLeft) — there is no full status pane
// in the UI yet. This page object wraps the kernel `com.nexus.git`
// handlers directly so specs can read state; tier1/git.spec.ts marks
// stage + commit flows as it.skip until a staging UI lands.

const PLUGIN_ID = 'com.nexus.git'

export interface GitStatusSnapshot {
  branch: string | null
  is_dirty: boolean | null
  head_oid: string | null
}

export class GitPage {
  /** Read the status-bar git indicator text, if present. */
  static async statusBarText(): Promise<string | null> {
    // Nexus registers the git item in the `statusBarLeft` slot of the
    // status bar. The renderer's container is shell-owned; the item
    // itself is whatever GitStatusItem emits. We use a coarse selector
    // and filter by content shape in the spec.
    const items = await $$('.nexus-status-bar-left > *, [data-slot="statusBarLeft"] > *').getElements()
    if (items.length === 0) return null
    const texts: string[] = []
    for (const it of items) texts.push(await it.getText())
    return texts.join(' | ')
  }

  /** Invoke the kernel git `status` handler directly. */
  static async status(): Promise<GitStatusSnapshot | null> {
    return browser.execute(async (plugin: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        kernel?: {
          invoke: <T>(p: string, cmd: string, a: unknown) => Promise<T>
        }
      } }).__nexusShellApi
      if (!api?.kernel) throw new Error('kernel missing')
      try {
        return await api.kernel.invoke<GitStatusSnapshot>(plugin, 'status', {})
      } catch {
        return null
      }
    }, PLUGIN_ID)
  }

  /** Invoke the kernel git `stage` + `commit` handlers. Used by the
   *  skipped spec — left here so when UI lands, specs can swap the
   *  DOM-driven path for these assertions. */
  static async stageAndCommit(paths: string[], message: string): Promise<unknown> {
    return browser.execute(
      async (plugin: string, args: { paths: string[]; message: string }) => {
        const api = (window as unknown as { __nexusShellApi?: {
          kernel?: {
            invoke: <T>(p: string, cmd: string, a: unknown) => Promise<T>
          }
        } }).__nexusShellApi
        if (!api?.kernel) throw new Error('kernel missing')
        await api.kernel.invoke<unknown>(plugin, 'stage', { paths: args.paths })
        return api.kernel.invoke<unknown>(plugin, 'commit', { message: args.message })
      },
      PLUGIN_ID,
      { paths, message },
    )
  }
}
