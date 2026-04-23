// Tier-1: AI chat surface.
//
// The shell's ChatView today is a stateless RAG composer. There is
// no session list, no archetype picker, and no "resume prior session"
// UI — those live in the agent pane's history column but not in the
// chat surface. Flows that depend on that UI are it.skip'd with an
// explanatory comment.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { ChatPage } from '../../pages/ChatPage.js'

describe('tier1: chat', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('opens the chat panel and renders the composer', async () => {
    await ChatPage.openPanel()
    const ta = await $('textarea[placeholder="Ask about your workspace…"]')
    await ta.waitForExist({ timeout: 10_000 })
    expect(await ta.isExisting()).toBe(true)
  })

  it('types into the composer (does not assert on model reply)', async () => {
    // We intentionally do not press Send here — a real send round-
    // trips through com.nexus.ai::ask which depends on an LLM
    // backend that isn't guaranteed wired in the e2e environment.
    // The act of filling the composer + seeing it reflected is
    // enough to exercise the input path.
    await ChatPage.openPanel()
    const ta = await $('textarea[placeholder="Ask about your workspace…"]')
    await ta.click()
    await ta.setValue('hello from e2e')
    expect(await ta.getValue()).toBe('hello from e2e')
  })

  // Skipped: no archetype picker and no session list exist in the
  // current ChatView. See shell/src/plugins/nexus/ai/ChatView.tsx —
  // header-less layout, single message list. Revisit once sessions
  // land (tracked under the agent pane's history column today).
  it.skip('switches archetype mid-session', async () => {
    // no-op
  })

  it.skip('resumes a prior chat session', async () => {
    // no-op — chat is stateless in v1
  })
})
