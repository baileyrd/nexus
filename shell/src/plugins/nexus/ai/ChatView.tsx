import { useEffect, useLayoutEffect, useMemo, useRef } from 'react'
import { useAiStore, type AiMessage } from './aiStore'
import { registerFocuser, send } from './aiRuntime'
import { renderMarkdown } from '../editor/markdownRender'
import '../editor/markdown.css'
import './chat.css'

/**
 * Sidebar chat view backed by com.nexus.ai::ask. Messages scroll up
 * top; input pinned at bottom. Send on Enter (Shift+Enter for
 * newline, Escape to clear the composer).
 *
 * This is a deliberately thin first cut — stateless RAG asks only.
 * Streaming + multi-turn memory are separate follow-ups.
 */
export function ChatView() {
  const messages = useAiStore((s) => s.messages)
  const input = useAiStore((s) => s.input)
  const sending = useAiStore((s) => s.sending)
  const setInput = useAiStore((s) => s.setInput)

  const scrollRef = useRef<HTMLDivElement | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement | null>(null)

  // Expose focus() to the focus command + autofocus on first mount.
  useEffect(() => {
    const focus = () => {
      requestAnimationFrame(() => textareaRef.current?.focus())
    }
    registerFocuser(focus)
    focus()
    return () => registerFocuser(null)
  }, [])

  // Auto-scroll to bottom whenever the message count changes or the
  // sending indicator flips. useLayoutEffect so we scroll in the same
  // frame the new row paints — avoids a visible jump.
  useLayoutEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [messages.length, sending])

  // Auto-grow textarea up to 140px.
  useLayoutEffect(() => {
    const ta = textareaRef.current
    if (!ta) return
    ta.style.height = 'auto'
    const next = Math.min(140, ta.scrollHeight)
    ta.style.height = `${next}px`
  }, [input])

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      void send()
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      setInput('')
    }
  }

  const canSend = input.trim().length > 0 && !sending

  const body =
    messages.length === 0 ? (
      <EmptyState onPickPrompt={(p) => setInput(p)} />
    ) : (
      <div
        ref={scrollRef}
        style={{
          flex: '1 1 auto',
          overflowY: 'auto',
          padding: '12px 14px',
          display: 'flex',
          flexDirection: 'column',
          gap: 10,
        }}
      >
        {messages.map((m) => (
          <MessageRow key={m.id} message={m} />
        ))}
        {sending ? <PendingRow /> : null}
      </div>
    )

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
      {body}
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
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Ask about your workspace…"
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
        <SendButton disabled={!canSend} onClick={() => void send()} />
      </div>
    </div>
  )
}

function MessageRow({ message }: { message: AiMessage }) {
  if (message.role === 'user') {
    return (
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <div
          style={{
            background: 'var(--accent-soft)',
            color: 'var(--fg)',
            borderRadius: 'var(--r)',
            padding: '6px 10px',
            maxWidth: '85%',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            lineHeight: 1.45,
          }}
        >
          {message.content}
        </div>
      </div>
    )
  }

  if (message.role === 'assistant') {
    return <AssistantRow message={message} />
  }

  if (message.role === 'error') {
    return (
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
          {message.content}
        </span>
      </div>
    )
  }

  // system
  return (
    <div
      style={{
        color: 'var(--fg-muted)',
        fontSize: 12,
        fontStyle: 'italic',
        whiteSpace: 'pre-wrap',
      }}
    >
      {message.content}
    </div>
  )
}

function AssistantRow({ message }: { message: AiMessage }) {
  const html = useMemo(() => renderMarkdown(message.content), [message.content])
  const hasSources = Array.isArray(message.sources) && message.sources.length > 0
  return (
    <div style={{ color: 'var(--fg)' }}>
      <div
        className="nexus-markdown-body nexus-ai-assistant-body"
        dangerouslySetInnerHTML={{ __html: html }}
      />
      {hasSources ? (
        <div
          style={{
            marginTop: 4,
            paddingTop: 6,
            borderTop: '1px dashed var(--line-soft)',
            display: 'flex',
            flexDirection: 'column',
            gap: 2,
          }}
        >
          <div
            style={{
              color: 'var(--fg-dim)',
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: 0.3,
            }}
          >
            Sources
          </div>
          {message.sources!.map((s, i) => (
            <div
              key={`${s.file_path}:${s.block_id ?? i}`}
              title={s.excerpt ?? s.file_path}
              style={{
                color: 'var(--fg-muted)',
                fontSize: 11,
                fontFamily: 'var(--f-mono)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {s.file_path}
            </div>
          ))}
        </div>
      ) : null}
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
      …
    </div>
  )
}

function SendButton({
  disabled,
  onClick,
}: {
  disabled: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      aria-label="Send"
      title="Send (Enter)"
      onClick={onClick}
      disabled={disabled}
      onMouseEnter={(e) => {
        if (!disabled) {
          ;(e.currentTarget as HTMLButtonElement).style.background =
            'var(--bg-hover)'
        }
      }}
      onMouseLeave={(e) => {
        ;(e.currentTarget as HTMLButtonElement).style.background = 'transparent'
      }}
      style={{
        flex: '0 0 32px',
        width: 32,
        height: 32,
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 0,
        border: 0,
        borderRadius: 'var(--r)',
        background: 'transparent',
        color: disabled ? 'var(--fg-muted)' : 'var(--accent)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.4 : 1,
      }}
    >
      <svg
        width={16}
        height={16}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.75}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {/* Lucide "arrow-up" glyph */}
        <path d="M12 19V5" />
        <path d="m5 12 7-7 7 7" />
      </svg>
    </button>
  )
}

const EXAMPLE_PROMPTS = [
  'Summarise my notes',
  "What's in the roadmap?",
  'Find TODOs',
]

function EmptyState({ onPickPrompt }: { onPickPrompt: (p: string) => void }) {
  return (
    <div
      style={{
        flex: '1 1 auto',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '20px 18px',
        gap: 14,
        textAlign: 'center',
      }}
    >
      <div style={{ color: 'var(--fg-dim)', fontSize: 13, maxWidth: 220 }}>
        Ask anything about your workspace.
      </div>
      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          justifyContent: 'center',
          gap: 6,
          maxWidth: 240,
        }}
      >
        {EXAMPLE_PROMPTS.map((p) => (
          <button
            key={p}
            type="button"
            onClick={() => onPickPrompt(p)}
            style={{
              border: '1px solid var(--line-soft)',
              background: 'transparent',
              color: 'var(--fg-muted)',
              borderRadius: 999,
              padding: '4px 10px',
              fontFamily: 'var(--f-ui)',
              fontSize: 11,
              cursor: 'pointer',
            }}
          >
            {p}
          </button>
        ))}
      </div>
    </div>
  )
}
