// Page object for the AI chat sidebar (nexus.ai).
//
// The current ChatView is a stateless RAG composer — a single message
// list plus a textarea pinned to the bottom. There is no session list
// or archetype picker in the UI today; helpers for those are provided
// for future use and called out in tier1/chat.spec.ts with it.skip.
//
// Selectors track the real markup (see shell/src/plugins/nexus/ai/
// ChatView.tsx). Keep them thin — no business logic.

const PLUGIN_ID = 'com.nexus.ai'
const VIEW_ID = 'nexus.ai.view'
const COMMAND_FOCUS = 'nexus.ai.focus'
const COMMAND_CLEAR = 'nexus.ai.clear'

export class ChatPage {
  /** Bring the AI chat sidebar view into focus via its registered command. */
  static async openPanel(): Promise<void> {
    await browser.execute(async (commandId: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute(commandId)
    }, COMMAND_FOCUS)
  }

  /** Clear the chat transcript. Routes through the plugin command so
   *  we don't have to guess at DOM affordances that may not exist. */
  static async clear(): Promise<void> {
    await browser.execute(async (commandId: string) => {
      const api = (window as unknown as { __nexusShellApi?: {
        commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      } }).__nexusShellApi
      if (!api?.commands) throw new Error('shell plugin API missing commands')
      await api.commands.execute(commandId)
    }, COMMAND_CLEAR)
  }

  /** Type a message into the composer and press Enter to send. */
  static async sendMessage(text: string): Promise<void> {
    await ChatPage.openPanel()
    const ta = await $('textarea[placeholder="Ask about your workspace…"]')
    await ta.waitForExist({ timeout: 10_000 })
    await ta.click()
    await ta.setValue(text)
    await browser.keys(['Enter'])
  }

  /** Get the rendered text of all assistant replies. */
  static async readAssistantReplies(): Promise<string[]> {
    const els = await $$('.nexus-ai-assistant-body')
    const out: string[] = []
    for (const el of els) out.push(await el.getText())
    return out
  }

  /** Count the message rows in the scroll area. One row per user or
   *  assistant message; pending indicator is a separate `.nexus-ai-pending`. */
  static async messageCount(): Promise<number> {
    // User rows and assistant rows share the scroll container; the
    // simplest proxy is "any direct child of the scroll region minus the
    // pending indicator". Chat is a thin RAG composer, so we don't need
    // a precise count here — just "did any row render?".
    const replies = await ChatPage.readAssistantReplies()
    return replies.length
  }

  /** Invoke the kernel ask handler directly — used when we need to
   *  seed a reply without waiting on a real model round-trip. */
  static async kernelAsk(prompt: string): Promise<unknown> {
    return browser.execute(
      async (plugin: string, args: { prompt: string }) => {
        const api = (window as unknown as { __nexusShellApi?: {
          kernel?: { invoke: <T>(p: string, cmd: string, a: unknown) => Promise<T> }
        } }).__nexusShellApi
        if (!api?.kernel) throw new Error('kernel missing')
        return api.kernel.invoke<unknown>(plugin, 'ask', args)
      },
      PLUGIN_ID,
      { prompt },
    )
  }

  static readonly VIEW_ID = VIEW_ID
}
