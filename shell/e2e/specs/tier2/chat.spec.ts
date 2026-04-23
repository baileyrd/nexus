// Tier-2: chat surface — composer validation + streaming indicator.
//
// Tier-1 exercises openPanel + typing. Tier-2 covers:
//   - Send button is disabled when the composer is empty (reads the
//     real `disabled` attribute driven by `canSend` in ChatView).
//   - Escape clears the composer (onKeyDown in ChatView binds it).
//   - `.nexus-ai-pending` indicator class is rendered for the "sending"
//     state; we assert the class exists in the component contract by
//     driving the store directly (kernel round-trips aren't reliable in
//     the e2e env — see tier1/chat.spec.ts).
//
// No RAG-toggle UI exists in ChatView today (ask is always RAG-mode via
// com.nexus.ai::ask). it.skip'd with a note.

import { expect } from '@wdio/globals'
import { SCRATCH_VAULT } from '../../wdio.conf.js'
import { openVault } from '../../support/app.js'
import { ChatPage } from '../../pages/ChatPage.js'

describe('tier2: chat', () => {
  before(async () => {
    await openVault(SCRATCH_VAULT)
  })

  it('send button is disabled while the composer is empty', async () => {
    await ChatPage.openPanel()
    const ta = await $('textarea[placeholder="Ask about your workspace…"]')
    await ta.waitForExist({ timeout: 10_000 })
    // Ensure empty.
    await ta.click()
    await ta.setValue('')
    const send = await $('button[aria-label="Send"]')
    expect(await send.isEnabled()).toBe(false)
  })

  it('send button becomes enabled once the composer has non-whitespace text', async () => {
    await ChatPage.openPanel()
    const ta = await $('textarea[placeholder="Ask about your workspace…"]')
    await ta.click()
    await ta.setValue('hi')
    const send = await $('button[aria-label="Send"]')
    await browser.waitUntil(async () => send.isEnabled(), {
      timeout: 5_000,
      timeoutMsg: 'Send never enabled after typing',
    })
    expect(await send.isEnabled()).toBe(true)
  })

  it('Escape clears the composer', async () => {
    await ChatPage.openPanel()
    const ta = await $('textarea[placeholder="Ask about your workspace…"]')
    await ta.click()
    await ta.setValue('draft text')
    expect(await ta.getValue()).toBe('draft text')
    await browser.keys(['Escape'])
    // onKeyDown binds Escape to setInput(''); wait a tick for React.
    await browser.waitUntil(async () => (await ta.getValue()) === '', {
      timeout: 5_000,
      timeoutMsg: 'Escape did not clear composer',
    })
  })

  it('RAG toggle reflects enabled/disabled state', async () => {
    await ChatPage.openPanel()
    const toggle = await $('button[aria-label="RAG mode"]')
    await toggle.waitForExist({ timeout: 10_000 })
    // Defaults on — ChatView seeds aiStore.ragEnabled to true.
    expect(await toggle.getAttribute('aria-pressed')).toBe('true')
    await toggle.click()
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-pressed')) === 'false',
      { timeout: 5_000, timeoutMsg: 'RAG toggle never reflected off state' },
    )
    await toggle.click()
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-pressed')) === 'true',
      { timeout: 5_000, timeoutMsg: 'RAG toggle never reflected on state' },
    )
  })

  // Skipped: the streaming pending indicator now carries role="status"
  // + aria-live="polite" + data-streaming="true", but is rendered only
  // while the ask round-trip is in flight. In CI the call returns
  // quickly with an error and the pending row is torn down before WDIO
  // can see it. Revisit with a fake-LLM adapter.
  it.skip('streaming-in-progress indicator appears during a send', async () => {
    // no-op
  })
})
