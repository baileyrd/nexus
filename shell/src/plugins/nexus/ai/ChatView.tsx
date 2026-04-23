// shell/src/plugins/nexus/ai/ChatView.tsx
//
// WI-01 Slice B — multi-turn chat view with markdown + RAG chips.
//
// Renders `useAiStore.turns` as a scrollable list above a fixed
// composer. Each turn:
//   - user:    plain pre-wrap bubble
//   - asst-streaming: plain text bubble + blinking caret, no copy yet
//   - asst-done:      sanitized markdown body + copy button + chips
//   - asst-error:     red bubble + retry button
//
// State lives in `useAiStore`; runtime calls in `aiRuntime`. This
// file is purely presentational.
//
// Cross-plugin import: we reuse `renderMarkdown` from the editor
// plugin (canvas/CanvasOverlay.tsx already does the same — established
// pattern). Sharing the helper keeps the AI bubbles styled identically
// to the markdown editor preview.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useAiStore, type AiSource, type AiTurn } from './aiStore'
import { registerFocuser } from './aiRuntime'
import { renderMarkdown } from '../editor/markdownRender'
import './chat.css'

const EVENT_FILE_OPEN = 'files:open'

export interface ChatViewProps {
  onSend: (question: string) => void | Promise<void>
  onCancel: () => void
  onRetry: () => void | Promise<void>
  /** Emit a cross-plugin event. Lets us open a source file in the
   *  editor without reaching into PluginAPI from the view. */
  onEmit?: (event: string, payload: unknown) => void
}

export function ChatView({ onSend, onCancel, onRetry, onEmit }: ChatViewProps) {
  const status = useAiStore((s) => s.status)
  const turns = useAiStore((s) => s.turns)
  const question = useAiStore((s) => s.question)
  const config = useAiStore((s) => s.config)
  const setQuestion = useAiStore((s) => s.setQuestion)

  const textareaRef = useRef<HTMLTextAreaElement | null>(null)
  const scrollRef = useRef<HTMLDivElement | null>(null)
  // Tracks whether the user has scrolled away from the bottom. Toggled
  // on every scroll event; consulted by the auto-scroll effect so we
  // only auto-scroll when the user is "following along". Mirrors the
  // pattern called out in wi01-chatpanel-reference.md §7 ("legacy was
  // unconditional autoscroll, the port should consider a guard").
  const stickToBottomRef = useRef(true)

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

  // Auto-grow textarea up to 140px (matches Slice A).
  useLayoutEffect(() => {
    const ta = textareaRef.current
    if (!ta) return
    ta.style.height = 'auto'
    ta.style.height = `${Math.min(140, ta.scrollHeight)}px`
  }, [question])

  // Track whether the user has scrolled away from the bottom. The
  // 32px threshold matches the gap between turn bubbles — within one
  // bubble of the bottom counts as "still following".
  const onScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    stickToBottomRef.current = distanceFromBottom < 32
  }, [])

  // Auto-scroll on every chunk / new turn — but only if the user is
  // still anchored to the bottom. If they've scrolled up to read an
  // earlier turn, leave the scrollTop alone.
  useLayoutEffect(() => {
    if (!stickToBottomRef.current) return
    const el = scrollRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [turns])

  const isInFlight = status === 'asking' || status === 'streaming'
  const canSend = !isInFlight && question.trim().length > 0

  // Surface the most recent assistant error in the banner. Older
  // errored turns stay visible inline as their own bubble.
  const lastError = useMemo(() => {
    for (let i = turns.length - 1; i >= 0; i -= 1) {
      const t = turns[i]
      if (t.kind === 'assistant' && t.status === 'error' && t.error) return t.error
    }
    return null
  }, [turns])

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

  const handleSourceClick = useCallback(
    (source: AiSource) => {
      if (!onEmit) return
      onEmit(EVENT_FILE_OPEN, { relpath: source.path, name: basename(source.path) })
    },
    [onEmit],
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
      <ConfigBanner config={config} />

      <div
        ref={scrollRef}
        onScroll={onScroll}
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
        {turns.length === 0 && status !== 'asking' && !lastError ? (
          <EmptyState />
        ) : null}

        {turns.map((t) =>
          t.kind === 'user' ? (
            <UserBubble key={t.id} turn={t} />
          ) : (
            <AssistantBubble
              key={t.id}
              turn={t}
              onSourceClick={handleSourceClick}
              onRetry={t.status === 'error' ? () => void onRetry() : undefined}
            />
          ),
        )}

        {status === 'asking' && turns.length === 0 ? <PendingRow /> : null}

        {lastError && status === 'error' ? (
          <ErrorBanner error={lastError} onRetry={() => void onRetry()} />
        ) : null}
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

// ── Bubbles ────────────────────────────────────────────────────────────────

function UserBubble({ turn }: { turn: Extract<AiTurn, { kind: 'user' }> }) {
  return (
    <div
      style={{
        alignSelf: 'flex-end',
        maxWidth: '88%',
        background: 'var(--bg-raised)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        padding: '8px 10px',
      }}
    >
      <div
        style={{
          fontSize: 10,
          color: 'var(--fg-dim)',
          textTransform: 'uppercase',
          letterSpacing: 0.4,
          marginBottom: 4,
        }}
      >
        You
      </div>
      <div
        style={{
          color: 'var(--fg)',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
          lineHeight: 1.45,
        }}
      >
        {turn.question}
      </div>
    </div>
  )
}

function AssistantBubble({
  turn,
  onSourceClick,
  onRetry,
}: {
  turn: Extract<AiTurn, { kind: 'assistant' }>
  onSourceClick: (source: AiSource) => void
  onRetry?: () => void
}) {
  const isStreaming = turn.status === 'streaming'
  const isError = turn.status === 'error'
  const body = turn.finalText ?? turn.streamedText

  return (
    <div
      style={{
        alignSelf: 'flex-start',
        maxWidth: '92%',
        width: '100%',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          marginBottom: 4,
        }}
      >
        <div
          style={{
            fontSize: 10,
            color: isError ? 'var(--risk)' : 'var(--fg-dim)',
            textTransform: 'uppercase',
            letterSpacing: 0.4,
          }}
        >
          {isError ? 'Assistant · error' : isStreaming ? 'Assistant · streaming…' : 'Assistant'}
        </div>
        {turn.status === 'done' && turn.finalText ? (
          <CopyButton text={turn.finalText} />
        ) : null}
      </div>

      {isError ? (
        <div
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
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
            }}
          >
            {turn.error?.message ?? 'Unknown error'}
          </div>
          {body ? (
            <div
              style={{
                fontSize: 12,
                color: 'var(--fg-muted)',
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
              }}
            >
              {body}
            </div>
          ) : null}
          {onRetry ? (
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
          ) : null}
        </div>
      ) : isStreaming ? (
        // Streaming: render plain text. Markdown parsing on every
        // chunk is wasteful and would re-flow code blocks mid-token.
        // The legacy reference (§7) confirms code fences render as raw
        // backticks during the stream — port preserves that.
        <div
          style={{
            color: 'var(--fg)',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            lineHeight: 1.5,
          }}
        >
          {body}
          <span className="nexus-ai-pending"> ▍</span>
        </div>
      ) : (
        // Done: parse markdown once, sanitize, render. Empty body
        // (cancelled before any chunk) gets a placeholder.
        body ? (
          <MarkdownBody source={body} />
        ) : (
          <div style={{ color: 'var(--fg-muted)', fontStyle: 'italic' }}>
            (no response)
          </div>
        )
      )}

      {turn.status === 'done' && turn.sources.length > 0 ? (
        <SourceChipRow sources={turn.sources} onSourceClick={onSourceClick} />
      ) : null}
    </div>
  )
}

function MarkdownBody({ source }: { source: string }) {
  // Re-parse only when the source string changes. marked + DOMPurify
  // are both synchronous and cheap, but the sanitized HTML can be
  // sizable for long answers — memoize so React doesn't re-set
  // dangerouslySetInnerHTML on unrelated re-renders.
  const html = useMemo(() => renderMarkdown(source), [source])
  return (
    <div
      className="nexus-ai-assistant-body nexus-markdown-body"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  const onClick = useCallback(() => {
    if (typeof navigator === 'undefined' || !navigator.clipboard) return
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    })
  }, [text])
  return (
    <button
      type="button"
      onClick={onClick}
      title="Copy answer to clipboard"
      style={{
        marginLeft: 'auto',
        border: '1px solid var(--line-soft)',
        background: 'transparent',
        color: copied ? 'var(--accent)' : 'var(--fg-dim)',
        borderRadius: 'var(--r)',
        padding: '1px 8px',
        fontFamily: 'var(--f-ui)',
        fontSize: 10,
        cursor: 'pointer',
        textTransform: 'uppercase',
        letterSpacing: 0.4,
      }}
    >
      {copied ? 'Copied' : 'Copy'}
    </button>
  )
}

function SourceChipRow({
  sources,
  onSourceClick,
}: {
  sources: AiSource[]
  onSourceClick: (source: AiSource) => void
}) {
  return (
    <div
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 4,
        marginTop: 6,
      }}
    >
      {sources.map((s, i) => (
        <button
          key={`${s.path}-${s.blockId ?? i}`}
          type="button"
          onClick={() => onSourceClick(s)}
          title={
            (s.excerpt ? `${s.excerpt.slice(0, 240)}${s.excerpt.length > 240 ? '…' : ''}\n\n` : '') +
            (typeof s.score === 'number' ? `score ${s.score.toFixed(3)}` : '')
          }
          style={{
            border: '1px solid var(--line-soft)',
            background: 'var(--bg-raised)',
            color: 'var(--fg-dim)',
            borderRadius: 'var(--r)',
            padding: '2px 8px',
            fontFamily: 'var(--f-ui)',
            fontSize: 11,
            cursor: 'pointer',
            maxWidth: 220,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {s.path}
        </button>
      ))}
    </div>
  )
}

// ── Chrome ────────────────────────────────────────────────────────────────

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

// ── helpers ───────────────────────────────────────────────────────────────

function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}
