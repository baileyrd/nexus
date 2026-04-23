// shell/src/plugins/nexus/ai/ChatView.tsx
//
// WI-01 Slice A — minimal chat view. Single Q + single streamed A.
// No conversation history, no RAG source chips, no markdown
// rendering, no copy buttons. Slices B/C add those.
//
// State lives in `useAiStore`; runtime calls in `aiRuntime`. This
// file is purely presentational.

import { useEffect, useLayoutEffect, useRef } from 'react'
import { useAiStore } from './aiStore'
import { registerFocuser } from './aiRuntime'
import './chat.css'

export interface ChatViewProps {
  /** Provided by the plugin's activate() — gives us a stable handle
   *  on the same PluginAPI the runtime is using for kernel.invoke /
   *  kernel.on. Decoupled from the runtime module so the view can
   *  be rendered in isolation for tests. */
  onSend: (question: string) => void | Promise<void>
  onCancel: () => void
  onRetry: () => void | Promise<void>
}

export function ChatView({ onSend, onCancel, onRetry }: ChatViewProps) {
  const status = useAiStore((s) => s.status)
  const question = useAiStore((s) => s.question)
  const streamedAnswer = useAiStore((s) => s.streamedAnswer)
  const finalAnswer = useAiStore((s) => s.finalAnswer)
  const error = useAiStore((s) => s.error)
  const config = useAiStore((s) => s.config)
  const setQuestion = useAiStore((s) => s.setQuestion)

  const textareaRef = useRef<HTMLTextAreaElement | null>(null)
  const scrollRef = useRef<HTMLDivElement | null>(null)

  // Wire the focus command. Drains pendingFocus on mount; clears the
  // focuser on unmount so a stale ref doesn't outlive the view.
  useEffect(() => {
    const focus = () => {
      requestAnimationFrame(() => textareaRef.current?.focus())
    }
    registerFocuser(focus)
    focus()
    return () => registerFocuser(null)
  }, [])

  // Auto-grow textarea up to 140px (matches the prior skeleton).
  useLayoutEffect(() => {
    const ta = textareaRef.current
    if (!ta) return
    ta.style.height = 'auto'
    ta.style.height = `${Math.min(140, ta.scrollHeight)}px`
  }, [question])

  // Auto-scroll the answer pane as chunks land.
  useLayoutEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [streamedAnswer, finalAnswer])

  const isInFlight = status === 'asking' || status === 'streaming'
  const canSend = !isInFlight && question.trim().length > 0
  // `finalAnswer ?? streamedAnswer` — the final wins once it lands.
  const responseText = finalAnswer ?? streamedAnswer

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (canSend) void onSend(question)
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      if (isInFlight) onCancel()
      else setQuestion('')
    }
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        width: '100%',
        background: 'var(--bg)',
        color: 'var(--fg)',
        fontFamily: 'var(--f-ui)',
        fontSize: 13,
      }}
    >
      <ConfigBanner config={config} />

      <div
        ref={scrollRef}
        style={{
          flex: '1 1 auto',
          overflowY: 'auto',
          padding: '12px 14px',
          display: 'flex',
          flexDirection: 'column',
          gap: 10,
          minHeight: 0,
        }}
      >
        {responseText.length > 0 ? (
          <ResponseBlock text={responseText} streaming={status === 'streaming'} />
        ) : status === 'asking' ? (
          <PendingRow />
        ) : !error ? (
          <EmptyState />
        ) : null}

        {error ? <ErrorBanner error={error} onRetry={() => void onRetry()} /> : null}
      </div>

      <div
        style={{
          borderTop: '1px solid var(--line-soft)',
          padding: '10px 12px',
          background: 'var(--bg-raised)',
          display: 'flex',
          alignItems: 'flex-end',
          gap: 8,
        }}
      >
        <textarea
          ref={textareaRef}
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={
            isInFlight ? 'Streaming response…' : 'Ask about your workspace…'
          }
          spellCheck={false}
          rows={1}
          style={{
            flex: '1 1 auto',
            width: '100%',
            minHeight: 36,
            maxHeight: 140,
            resize: 'none',
            background: 'transparent',
            color: 'var(--fg)',
            fontFamily: 'var(--f-ui)',
            fontSize: 13,
            lineHeight: 1.45,
            border: 0,
            outline: 0,
            padding: '6px 8px',
            boxSizing: 'border-box',
          }}
        />
        {isInFlight ? (
          <ActionButton
            label="Stop"
            tone="danger"
            onClick={onCancel}
            title="Stop streaming (Esc)"
          />
        ) : (
          <ActionButton
            label="Send"
            tone="accent"
            disabled={!canSend}
            onClick={() => void onSend(question)}
            title="Send (Enter)"
          />
        )}
      </div>
    </div>
  )
}

function ConfigBanner({ config }: { config: ReturnType<typeof useAiStore.getState>['config'] }) {
  if (!config) return null
  const ai = config.ai
  if (!ai) {
    return (
      <div
        style={{
          padding: '6px 12px',
          fontSize: 11,
          color: 'var(--risk)',
          background: 'var(--bg-raised)',
          borderBottom: '1px solid var(--line-soft)',
        }}
      >
        No AI provider configured. Set NEXUS_AI_PROVIDER (anthropic, openai, ollama) and restart.
      </div>
    )
  }
  return (
    <div
      style={{
        padding: '4px 12px',
        fontSize: 10,
        color: 'var(--fg-dim)',
        background: 'var(--bg-raised)',
        borderBottom: '1px solid var(--line-soft)',
        textTransform: 'uppercase',
        letterSpacing: 0.4,
      }}
    >
      {ai.provider}
      {ai.model ? ` · ${ai.model}` : ''}
    </div>
  )
}

function ResponseBlock({ text, streaming }: { text: string; streaming: boolean }) {
  return (
    <div
      style={{
        color: 'var(--fg)',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        lineHeight: 1.5,
      }}
    >
      {text}
      {streaming ? <span className="nexus-ai-pending"> ▍</span> : null}
    </div>
  )
}

function PendingRow() {
  return (
    <div
      className="nexus-ai-pending"
      style={{
        color: 'var(--fg-muted)',
        fontSize: 13,
        padding: '2px 0',
      }}
    >
      Thinking…
    </div>
  )
}

function EmptyState() {
  return (
    <div
      style={{
        flex: '1 1 auto',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '20px 18px',
        color: 'var(--fg-dim)',
        fontSize: 13,
        textAlign: 'center',
      }}
    >
      Ask about your workspace.
    </div>
  )
}

function ErrorBanner({
  error,
  onRetry,
}: {
  error: Error
  onRetry: () => void
}) {
  return (
    <div
      role="alert"
      style={{
        border: '1px solid var(--risk)',
        background: 'var(--bg-raised)',
        color: 'var(--fg)',
        borderRadius: 'var(--r)',
        padding: '8px 10px',
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div
        style={{
          color: 'var(--risk)',
          fontSize: 12,
          display: 'flex',
          gap: 6,
          alignItems: 'flex-start',
        }}
      >
        <span aria-hidden>⚠</span>
        <span style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
          {error.message}
        </span>
      </div>
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <button
          type="button"
          onClick={onRetry}
          style={{
            border: '1px solid var(--line-soft)',
            background: 'transparent',
            color: 'var(--fg)',
            borderRadius: 'var(--r)',
            padding: '4px 10px',
            fontFamily: 'var(--f-ui)',
            fontSize: 12,
            cursor: 'pointer',
          }}
        >
          Retry
        </button>
      </div>
    </div>
  )
}

function ActionButton({
  label,
  tone,
  disabled,
  onClick,
  title,
}: {
  label: string
  tone: 'accent' | 'danger'
  disabled?: boolean
  onClick: () => void
  title?: string
}) {
  const color = tone === 'danger' ? 'var(--risk)' : 'var(--accent)'
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      style={{
        flex: '0 0 auto',
        height: 32,
        padding: '0 12px',
        border: `1px solid ${disabled ? 'var(--line-soft)' : color}`,
        borderRadius: 'var(--r)',
        background: 'transparent',
        color: disabled ? 'var(--fg-muted)' : color,
        cursor: disabled ? 'not-allowed' : 'pointer',
        fontFamily: 'var(--f-ui)',
        fontSize: 12,
        opacity: disabled ? 0.55 : 1,
      }}
    >
      {label}
    </button>
  )
}
