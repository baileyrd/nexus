// shell/src/plugins/nexus/ai/CmdIOverlay.tsx
//
// BL-032 — Cmd+I command-anywhere AI overlay.
//
// Lives in the `overlay` slot. Mirrors `commandPalette/CommandPalette.tsx`
// for backdrop/keydown/focus conventions; the streaming response area
// is the new bit.

import { useEffect, useRef } from 'react'
import { useCmdIStore } from './cmdIStore'
import { submitCmdI } from './cmdIRuntime'
import { getApi } from './cmdIApi'
import type { ContextChip } from './contextContributors'

/** v1 visual: a flat horizontal list of chips. BL-033 will style and
 *  add click-to-remove. We keep the markup deliberately simple here so
 *  the follow-up can rework the rail without touching the runtime. */
function ContextChipRail({ chips }: { chips: ContextChip[] }) {
  if (chips.length === 0) return null
  return (
    <div
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 6,
        padding: '6px 16px 8px',
        borderBottom: '1px solid var(--line-soft)',
      }}
    >
      {chips.map((chip) => (
        <span
          key={chip.id}
          style={{
            background: 'var(--bg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            padding: '2px 8px',
            fontFamily: 'var(--f-ui)',
            fontSize: '0.78em',
            color: 'var(--fg-muted)',
          }}
        >
          {chip.label}
        </span>
      ))}
    </div>
  )
}

/**
 * Streaming-response panel beneath the prompt input. Renders nothing
 * until the user submits something; from there it shows a spinner
 * until the first chunk arrives, then the streaming body, then the
 * final body — all in the same panel so there's no layout shift.
 */
function ResponsePanel() {
  const status = useCmdIStore((s) => s.status)
  const responseText = useCmdIStore((s) => s.responseText)
  const error = useCmdIStore((s) => s.error)

  if (status === 'idle' || status === 'collecting') return null

  if (status === 'error' && error) {
    return (
      <div
        role="alert"
        style={{
          padding: '12px 16px',
          color: 'var(--danger, #b00020)',
          fontFamily: 'var(--f-ui)',
          fontSize: 13,
          borderTop: '1px solid var(--line-soft)',
        }}
      >
        {error.message}
      </div>
    )
  }

  return (
    <div
      style={{
        padding: '12px 16px',
        maxHeight: 320,
        overflowY: 'auto',
        borderTop: '1px solid var(--line-soft)',
        fontFamily: 'var(--f-ui)',
        fontSize: 13,
        color: 'var(--fg)',
        whiteSpace: 'pre-wrap',
      }}
    >
      {responseText.length === 0 && status === 'submitting' ? (
        <span style={{ color: 'var(--fg-dim)' }}>Thinking…</span>
      ) : (
        responseText
      )}
    </div>
  )
}

/**
 * The overlay itself. Owns Esc/backdrop dismissal + Enter-to-submit
 * + autofocus. Submitted requests run via `cmdIRuntime.submitCmdI`.
 */
export function CmdIOverlay() {
  const visible = useCmdIStore((s) => s.visible)
  const prompt = useCmdIStore((s) => s.prompt)
  const chips = useCmdIStore((s) => s.chips)
  const status = useCmdIStore((s) => s.status)
  const setPrompt = useCmdIStore((s) => s.setPrompt)
  const close = useCmdIStore((s) => s.close)

  const inputRef = useRef<HTMLTextAreaElement | null>(null)

  // Autofocus on each open. requestAnimationFrame to dodge a race with
  // mount — same trick the command palette uses.
  useEffect(() => {
    if (!visible) return
    const id = requestAnimationFrame(() => inputRef.current?.focus())
    return () => cancelAnimationFrame(id)
  }, [visible])

  if (!visible) return null

  const submitDisabled =
    status === 'submitting' || status === 'streaming' || prompt.trim().length === 0

  const onSubmit = () => {
    if (submitDisabled) return
    let api
    try {
      api = getApi()
    } catch {
      // Plugin hasn't activated yet — shouldn't happen because the
      // overlay can't open without the command being registered, but
      // we'd rather no-op than throw inside an event handler.
      return
    }
    void submitCmdI(api)
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Escape') {
      // stopPropagation so the App.tsx-level handler doesn't double-act.
      e.preventDefault()
      e.stopPropagation()
      close()
      return
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      onSubmit()
    }
  }

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) close()
  }

  return (
    <div
      onClick={onBackdropClick}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'oklch(0 0 0 / 0.35)',
        pointerEvents: 'auto',
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'flex-start',
        paddingTop: 120,
      }}
    >
      <div
        role="dialog"
        aria-label="AI command"
        style={{
          width: 640,
          maxWidth: '92vw',
          background: 'var(--bg-raised)',
          border: '1px solid var(--line)',
          borderRadius: 'var(--r-lg)',
          boxShadow: 'var(--shadow)',
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <ContextChipRail chips={chips} />
        <textarea
          ref={inputRef}
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={onKeyDown}
          rows={3}
          placeholder="Ask anything about what you're looking at…"
          spellCheck={false}
          autoComplete="off"
          disabled={status === 'submitting' || status === 'streaming'}
          style={{
            background: 'transparent',
            border: 0,
            outline: 0,
            color: 'var(--fg)',
            fontFamily: 'var(--f-ui)',
            fontSize: 14,
            padding: '12px 16px',
            resize: 'none',
          }}
        />
        <ResponsePanel />
      </div>
    </div>
  )
}
