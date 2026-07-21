// shell/src/plugins/nexus/ai/ChatView.tsx
//
// WI-01 Slice B — multi-turn chat view with markdown + RAG chips.
// WI-01 Slice C — session picker (collapsible drawer) at the top of
// the column. Two-pane was rejected: the AI chat lives in the right
// activity-bar pane, which docks at ~280–320px wide. A 200px session
// rail would leave ~80px for the conversation, which is unusable for
// reading. The drawer pattern keeps the conversation full-width and
// surfaces the picker on demand without stealing horizontal space.
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
import { useConfigValue } from '../../../stores/configStore'
import {
  useAiStore,
  type AiSessionMeta,
  type AiSource,
  type AiCitation,
  type AiTurn,
} from './aiStore'

const COPIED_NOTIFICATION_MS = 1200
import { registerFocuser } from './aiRuntime'
import { renderMarkdown } from '../editor/markdownRender'
import { renderMarkdownWithCitations } from './citationTransform'
import './chat.css'

const EVENT_FILE_OPEN = 'files:open'

export interface ChatViewProps {
  onSend: (question: string) => void | Promise<void>
  onCancel: () => void
  onRetry: () => void | Promise<void>
  /** Emit a cross-plugin event. Lets us open a source file in the
   *  editor without reaching into PluginAPI from the view. */
  onEmit?: (event: string, payload: unknown) => void
  // ── Slice C: session-management bindings ────────────────────────────────
  /** "New chat" — clears + auto-saves outgoing under prior id. */
  onNewChat?: () => void | Promise<void>
  /** Switch to a saved session by id. Cancels in-flight first. */
  onLoadSession?: (id: string) => void | Promise<void>
  /** Remove a session permanently. */
  onDeleteSession?: (id: string) => void | Promise<void>
  /** Rename a session (kernel: session_save with same id, new title). */
  onRenameSession?: (id: string, title: string) => void | Promise<void>
  /** Manual save — used by the explicit "Save" button. Auto-save runs
   *  on assistant-done debounced, but power users want a button too. */
  onSaveSession?: () => void | Promise<void>
  /** Export a saved session to markdown and save it as a forge note
   *  (#384). Per-session (not per-turn) since it renders the whole
   *  conversation, not a single answer. */
  onExportSession?: (id: string) => void | Promise<void>
  /** Open Settings → AI provider section. Wired by the plugin so the
   *  empty-state CTA (and any "no provider" banner) can route the user
   *  to where they configure credentials, no env-var ritual. */
  onOpenSettings?: () => void
}

export function ChatView({
  onSend,
  onCancel,
  onRetry,
  onEmit,
  onNewChat,
  onLoadSession,
  onDeleteSession,
  onRenameSession,
  onSaveSession,
  onExportSession,
  onOpenSettings,
}: ChatViewProps) {
  const status = useAiStore((s) => s.status)
  const turns = useAiStore((s) => s.turns)
  const question = useAiStore((s) => s.question)
  const config = useAiStore((s) => s.config)
  const sessions = useAiStore((s) => s.sessions)
  const activeSessionId = useAiStore((s) => s.activeSessionId)
  const sessionsLoading = useAiStore((s) => s.sessionsLoading)
  const setQuestion = useAiStore((s) => s.setQuestion)
  const [showSessions, setShowSessions] = useState(false)

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

  const handleCitationClick = useCallback(
    (citation: AiCitation) => {
      if (!onEmit) return
      // BL-038: pass `line` so listeners that support it can scroll
      // straight to the cited block. The legacy `relpath` + `name`
      // fields stay verbatim so older listeners ignore the extra key
      // gracefully.
      onEmit(EVENT_FILE_OPEN, {
        relpath: citation.path,
        name: basename(citation.path),
        line: citation.startLine ?? undefined,
      })
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
        background: 'var(--background-primary)',
        color: 'var(--text-normal)',
        fontFamily: 'var(--font-interface)',
        fontSize: 13,
      }}
    >
      <ConfigBanner config={config} onOpenSettings={onOpenSettings} />

      {onNewChat || onLoadSession || onDeleteSession || onRenameSession ? (
        <SessionBar
          sessions={sessions}
          activeId={activeSessionId}
          loading={sessionsLoading}
          expanded={showSessions}
          onToggleExpanded={() => setShowSessions((v) => !v)}
          onNewChat={onNewChat}
          onSaveSession={onSaveSession}
          onLoadSession={onLoadSession}
          onDeleteSession={onDeleteSession}
          onRenameSession={onRenameSession}
          onExportSession={onExportSession}
          hasContent={turns.length > 0}
        />
      ) : null}

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
          <EmptyState
            configured={!!config?.ai}
            onOpenSettings={onOpenSettings}
          />
        ) : null}

        {turns.map((t) =>
          t.kind === 'user' ? (
            <UserBubble key={t.id} turn={t} />
          ) : (
            <AssistantBubble
              key={t.id}
              turn={t}
              onSourceClick={handleSourceClick}
              onCitationClick={handleCitationClick}
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
          borderTop: '1px solid var(--divider-color)',
          padding: '10px 12px',
          background: 'var(--background-secondary)',
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
            color: 'var(--text-normal)',
            fontFamily: 'var(--font-interface)',
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
        background: 'var(--background-secondary)',
        border: '1px solid var(--divider-color)',
        borderRadius: 'var(--radius-s)',
        padding: '8px 10px',
      }}
    >
      <div
        style={{
          fontSize: 10,
          color: 'var(--text-faint)',
          textTransform: 'uppercase',
          letterSpacing: 0.4,
          marginBottom: 4,
        }}
      >
        You
      </div>
      <div
        style={{
          color: 'var(--text-normal)',
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
  onCitationClick,
  onRetry,
}: {
  turn: Extract<AiTurn, { kind: 'assistant' }>
  onSourceClick: (source: AiSource) => void
  onCitationClick: (citation: AiCitation) => void
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
            color: isError ? 'var(--risk)' : 'var(--text-faint)',
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
            background: 'var(--background-secondary)',
            color: 'var(--text-normal)',
            borderRadius: 'var(--radius-s)',
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
                color: 'var(--text-muted)',
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
                  border: '1px solid var(--divider-color)',
                  background: 'transparent',
                  color: 'var(--text-normal)',
                  borderRadius: 'var(--radius-s)',
                  padding: '4px 10px',
                  fontFamily: 'var(--font-interface)',
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
            color: 'var(--text-normal)',
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
          turn.citations.length > 0 ? (
            <CitedMarkdownBody
              source={body}
              citations={turn.citations}
              onCitationClick={onCitationClick}
            />
          ) : (
            <MarkdownBody source={body} />
          )
        ) : (
          <div style={{ color: 'var(--text-muted)', fontStyle: 'italic' }}>
            (no response)
          </div>
        )
      )}

      {turn.status === 'done' && turn.citations.length > 0 ? (
        <CitationChipRow citations={turn.citations} onCitationClick={onCitationClick} />
      ) : turn.status === 'done' && turn.sources.length > 0 ? (
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

/**
 * Render the answer markdown with `[N]` markers transformed into
 * clickable superscript citation chips. BL-038.
 *
 * Strategy: render the full markdown with the renderer first (so code
 * blocks / tables / etc. stay intact), then walk the resulting HTML
 * via a DOMParser, splitting text nodes around `[N]` matches and
 * substituting `<sup class="nexus-citation" data-cite="N">[N]</sup>`.
 * The shell binds a click handler to that class to dispatch
 * `onCitationClick`. Marker substitution is skipped inside `<code>`
 * and `<pre>` so code samples that contain `[1]` literals are left
 * verbatim.
 */
function CitedMarkdownBody({
  source,
  citations,
  onCitationClick,
}: {
  source: string
  citations: AiCitation[]
  onCitationClick: (citation: AiCitation) => void
}) {
  const html = useMemo(
    () => renderMarkdownWithCitations(source, new Set(citations.map((c) => c.index))),
    [source, citations],
  )
  const containerRef = useRef<HTMLDivElement | null>(null)

  // Delegate clicks: cheaper than wiring a listener per chip and
  // keeps the dangerouslySetInnerHTML render simple.
  useEffect(() => {
    const root = containerRef.current
    if (!root) return undefined
    const handler = (ev: MouseEvent) => {
      const target = ev.target as HTMLElement | null
      if (!target) return
      const chip = target.closest<HTMLElement>('[data-cite]')
      if (!chip) return
      const idx = Number(chip.dataset.cite)
      if (!Number.isFinite(idx)) return
      const cit = citations.find((c) => c.index === idx)
      if (cit) {
        ev.preventDefault()
        onCitationClick(cit)
      }
    }
    root.addEventListener('click', handler)
    return () => root.removeEventListener('click', handler)
  }, [citations, onCitationClick])

  return (
    <div
      ref={containerRef}
      className="nexus-ai-assistant-body nexus-markdown-body"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}

function CitationChipRow({
  citations,
  onCitationClick,
}: {
  citations: AiCitation[]
  onCitationClick: (citation: AiCitation) => void
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
      {citations.map((c) => {
        const lineSuffix =
          c.startLine != null
            ? c.endLine != null && c.endLine !== c.startLine
              ? `:${c.startLine}-${c.endLine}`
              : `:${c.startLine}`
            : ''
        return (
          <button
            key={`cite-${c.index}-${c.path}-${c.blockId}`}
            type="button"
            onClick={() => onCitationClick(c)}
            title={
              (c.excerpt ? `${c.excerpt}\n\n` : '') +
              `score ${c.score.toFixed(3)}`
            }
            style={{
              border: '1px solid var(--divider-color)',
              background: 'var(--background-secondary)',
              color: 'var(--text-faint)',
              borderRadius: 'var(--radius-s)',
              padding: '2px 8px',
              fontFamily: 'var(--font-interface)',
              fontSize: 11,
              cursor: 'pointer',
              maxWidth: 260,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            <span style={{ color: 'var(--interactive-accent)', marginRight: 4 }}>[{c.index}]</span>
            {c.path}
            {lineSuffix}
          </button>
        )
      })}
    </div>
  )
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  const copiedMs = useConfigValue('ui.copiedNotificationMs', COPIED_NOTIFICATION_MS)
  const onClick = useCallback(() => {
    if (typeof navigator === 'undefined' || !navigator.clipboard) return
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), copiedMs ?? COPIED_NOTIFICATION_MS)
    })
  }, [text, copiedMs])
  return (
    <button
      type="button"
      onClick={onClick}
      title="Copy answer to clipboard"
      style={{
        marginLeft: 'auto',
        border: '1px solid var(--divider-color)',
        background: 'transparent',
        color: copied ? 'var(--interactive-accent)' : 'var(--text-faint)',
        borderRadius: 'var(--radius-s)',
        padding: '1px 8px',
        fontFamily: 'var(--font-interface)',
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
            border: '1px solid var(--divider-color)',
            background: 'var(--background-secondary)',
            color: 'var(--text-faint)',
            borderRadius: 'var(--radius-s)',
            padding: '2px 8px',
            fontFamily: 'var(--font-interface)',
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

function ConfigBanner({
  config,
  onOpenSettings,
}: {
  config: ReturnType<typeof useAiStore.getState>['config']
  onOpenSettings?: () => void
}) {
  if (!config) return null
  const ai = config.ai
  if (!ai) {
    // Empty / mis-configured. Render a CTA row instead of the old
    // red error: most users land here on first run and the actionable
    // path is "open Settings → AI", not "edit env vars and restart".
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          padding: '8px 12px',
          fontSize: 12,
          color: 'var(--text-muted)',
          background: 'var(--background-secondary)',
          borderBottom: '1px solid var(--divider-color)',
        }}
      >
        <span>No AI provider configured.</span>
        {onOpenSettings && (
          <button
            type="button"
            onClick={onOpenSettings}
            style={{
              padding: '3px 10px',
              background: 'var(--interactive-accent)',
              color: 'var(--interactive-accent-ink)',
              border: '1px solid var(--interactive-accent)',
              borderRadius: 'var(--radius-s)',
              fontSize: 11,
              fontFamily: 'var(--font-interface)',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            Configure
          </button>
        )}
      </div>
    )
  }
  return (
    <div
      style={{
        padding: '4px 12px',
        fontSize: 10,
        color: 'var(--text-faint)',
        background: 'var(--background-secondary)',
        borderBottom: '1px solid var(--divider-color)',
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
        color: 'var(--text-muted)',
        fontSize: 13,
        padding: '2px 0',
      }}
    >
      Thinking…
    </div>
  )
}

function EmptyState({
  configured,
  onOpenSettings,
}: {
  configured: boolean
  onOpenSettings?: () => void
}) {
  return (
    <div
      style={{
        flex: '1 1 auto',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 12,
        padding: '20px 18px',
        color: 'var(--text-faint)',
        fontSize: 13,
        textAlign: 'center',
      }}
    >
      {configured ? (
        <span>Ask about your workspace.</span>
      ) : (
        <>
          <span style={{ maxWidth: 260, lineHeight: 1.5 }}>
            Connect an AI provider to start chatting. Anthropic, OpenAI,
            or a local Ollama server all work.
          </span>
          {onOpenSettings && (
            <button
              type="button"
              onClick={onOpenSettings}
              style={{
                padding: '6px 14px',
                background: 'var(--interactive-accent)',
                color: 'var(--interactive-accent-ink)',
                border: '1px solid var(--interactive-accent)',
                borderRadius: 'var(--radius-s)',
                fontSize: 12,
                fontFamily: 'var(--font-interface)',
                fontWeight: 500,
                cursor: 'pointer',
              }}
            >
              Configure AI provider
            </button>
          )}
        </>
      )}
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
        background: 'var(--background-secondary)',
        color: 'var(--text-normal)',
        borderRadius: 'var(--radius-s)',
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
            border: '1px solid var(--divider-color)',
            background: 'transparent',
            color: 'var(--text-normal)',
            borderRadius: 'var(--radius-s)',
            padding: '4px 10px',
            fontFamily: 'var(--font-interface)',
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
  const color = tone === 'danger' ? 'var(--risk)' : 'var(--interactive-accent)'
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
        border: `1px solid ${disabled ? 'var(--divider-color)' : color}`,
        borderRadius: 'var(--radius-s)',
        background: 'transparent',
        color: disabled ? 'var(--text-muted)' : color,
        cursor: disabled ? 'not-allowed' : 'pointer',
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
        opacity: disabled ? 0.55 : 1,
      }}
    >
      {label}
    </button>
  )
}

// ── Slice C: session bar + list ───────────────────────────────────────────

interface SessionBarProps {
  sessions: AiSessionMeta[]
  activeId: string | null
  loading: boolean
  expanded: boolean
  hasContent: boolean
  onToggleExpanded: () => void
  onNewChat?: () => void | Promise<void>
  onSaveSession?: () => void | Promise<void>
  onLoadSession?: (id: string) => void | Promise<void>
  onDeleteSession?: (id: string) => void | Promise<void>
  onRenameSession?: (id: string, title: string) => void | Promise<void>
  onExportSession?: (id: string) => void | Promise<void>
}

function SessionBar({
  sessions,
  activeId,
  loading,
  expanded,
  hasContent,
  onToggleExpanded,
  onNewChat,
  onSaveSession,
  onLoadSession,
  onDeleteSession,
  onRenameSession,
  onExportSession,
}: SessionBarProps) {
  const activeMeta = useMemo(
    () => sessions.find((s) => s.id === activeId) ?? null,
    [sessions, activeId],
  )
  // Header label: active title if loaded, otherwise "(unsaved)" when
  // there's content, otherwise the count of saved sessions.
  const headerLabel = activeMeta?.title?.trim()
    ? activeMeta.title
    : hasContent
      ? '(unsaved)'
      : sessions.length === 0
        ? 'No saved sessions'
        : `${sessions.length} saved`

  return (
    <div
      style={{
        borderBottom: '1px solid var(--divider-color)',
        background: 'var(--background-secondary)',
        flex: '0 0 auto',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '4px 8px',
          minHeight: 28,
        }}
      >
        <button
          type="button"
          onClick={onToggleExpanded}
          title={expanded ? 'Hide sessions' : 'Show sessions'}
          style={{
            border: '1px solid var(--divider-color)',
            background: 'transparent',
            color: 'var(--text-faint)',
            borderRadius: 'var(--radius-s)',
            padding: '2px 6px',
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
            cursor: 'pointer',
            display: 'flex',
            alignItems: 'center',
            gap: 4,
          }}
        >
          <span aria-hidden style={{ display: 'inline-block', width: 8 }}>
            {expanded ? '▾' : '▸'}
          </span>
          <span
            style={{
              maxWidth: 160,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {headerLabel}
          </span>
        </button>

        <div style={{ flex: '1 1 auto' }} />

        {onSaveSession && hasContent ? (
          <SmallChip
            label="Save"
            onClick={onSaveSession}
            title="Save current conversation"
          />
        ) : null}
        {onNewChat ? (
          <SmallChip
            label="New"
            onClick={onNewChat}
            title="Start a fresh conversation"
            tone="accent"
          />
        ) : null}
      </div>

      {expanded ? (
        <div
          style={{
            borderTop: '1px solid var(--divider-color)',
            maxHeight: 200,
            overflowY: 'auto',
            padding: '4px 0',
          }}
        >
          {loading && sessions.length === 0 ? (
            <div
              style={{
                padding: '6px 12px',
                fontSize: 11,
                color: 'var(--text-muted)',
              }}
            >
              Loading sessions…
            </div>
          ) : sessions.length === 0 ? (
            <div
              style={{
                padding: '6px 12px',
                fontSize: 11,
                color: 'var(--text-muted)',
                fontStyle: 'italic',
              }}
            >
              No saved sessions yet. Send a message to create one.
            </div>
          ) : (
            sessions.map((s) => (
              <SessionListItem
                key={s.id}
                meta={s}
                active={s.id === activeId}
                onLoad={onLoadSession}
                onDelete={onDeleteSession}
                onRename={onRenameSession}
                onExport={onExportSession}
              />
            ))
          )}
        </div>
      ) : null}
    </div>
  )
}

interface SessionListItemProps {
  meta: AiSessionMeta
  active: boolean
  onLoad?: (id: string) => void | Promise<void>
  onDelete?: (id: string) => void | Promise<void>
  onRename?: (id: string, title: string) => void | Promise<void>
  onExport?: (id: string) => void | Promise<void>
}

function SessionListItem({
  meta,
  active,
  onLoad,
  onDelete,
  onRename,
  onExport,
}: SessionListItemProps) {
  // Inline rename: double-click switches to an input, Enter / blur
  // commits, Escape cancels. Pattern picked over a separate "edit"
  // button to keep the row dense — the activity-bar pane is narrow
  // and a third action button would crowd the row.
  const [editing, setEditing] = useState(false)
  const [draftTitle, setDraftTitle] = useState(meta.title)
  const inputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus()
      inputRef.current?.select()
    }
  }, [editing])

  // Reset draft when we exit edit mode or the underlying title changes
  // (e.g., another tab renamed it via session_save). Without this the
  // input would show stale text on next double-click.
  useEffect(() => {
    if (!editing) setDraftTitle(meta.title)
  }, [editing, meta.title])

  const commitRename = useCallback(() => {
    const trimmed = draftTitle.trim()
    setEditing(false)
    if (!trimmed || trimmed === meta.title) return
    if (onRename) void onRename(meta.id, trimmed)
  }, [draftTitle, meta.id, meta.title, onRename])

  const displayTitle = meta.title?.trim() ? meta.title : '(untitled)'

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        padding: '4px 8px',
        background: active ? 'var(--background-modifier-active)' : 'transparent',
        borderLeft: `2px solid ${active ? 'var(--interactive-accent)' : 'transparent'}`,
        cursor: editing ? 'text' : 'pointer',
      }}
      onClick={() => {
        if (editing) return
        if (active) return
        if (onLoad) void onLoad(meta.id)
      }}
      onDoubleClick={(e) => {
        e.stopPropagation()
        if (onRename) setEditing(true)
      }}
      title={editing ? 'Renaming…' : 'Click to load · double-click to rename'}
    >
      {editing ? (
        <input
          ref={inputRef}
          value={draftTitle}
          onChange={(e) => setDraftTitle(e.target.value)}
          onBlur={commitRename}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              commitRename()
            } else if (e.key === 'Escape') {
              e.preventDefault()
              setEditing(false)
              setDraftTitle(meta.title)
            }
          }}
          onClick={(e) => e.stopPropagation()}
          style={{
            flex: '1 1 auto',
            minWidth: 0,
            background: 'var(--background-primary)',
            color: 'var(--text-normal)',
            border: '1px solid var(--interactive-accent)',
            borderRadius: 'var(--radius-s)',
            padding: '2px 6px',
            fontFamily: 'var(--font-interface)',
            fontSize: 12,
            outline: 'none',
          }}
        />
      ) : (
        <div
          style={{
            flex: '1 1 auto',
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            fontSize: 12,
            color: meta.title?.trim() ? 'var(--text-normal)' : 'var(--text-muted)',
            fontStyle: meta.title?.trim() ? 'normal' : 'italic',
          }}
        >
          {displayTitle}
        </div>
      )}

      {!editing && onExport ? (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation()
            void onExport(meta.id)
          }}
          title="Export as note"
          style={{
            flex: '0 0 auto',
            border: '1px solid transparent',
            background: 'transparent',
            color: 'var(--text-muted)',
            borderRadius: 'var(--radius-s)',
            padding: '0 6px',
            fontSize: 12,
            lineHeight: '20px',
            cursor: 'pointer',
          }}
        >
          ⇩
        </button>
      ) : null}
      {!editing && onDelete ? (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation()
            void onDelete(meta.id)
          }}
          title="Delete session"
          style={{
            flex: '0 0 auto',
            border: '1px solid transparent',
            background: 'transparent',
            color: 'var(--text-muted)',
            borderRadius: 'var(--radius-s)',
            padding: '0 6px',
            fontSize: 12,
            lineHeight: '20px',
            cursor: 'pointer',
          }}
        >
          ×
        </button>
      ) : null}
    </div>
  )
}

function SmallChip({
  label,
  onClick,
  title,
  tone,
}: {
  label: string
  onClick: () => void | Promise<void>
  title?: string
  tone?: 'accent'
}) {
  const color = tone === 'accent' ? 'var(--interactive-accent)' : 'var(--text-faint)'
  return (
    <button
      type="button"
      onClick={() => void onClick()}
      title={title}
      style={{
        flex: '0 0 auto',
        border: `1px solid ${tone === 'accent' ? color : 'var(--divider-color)'}`,
        background: 'transparent',
        color,
        borderRadius: 'var(--radius-s)',
        padding: '2px 8px',
        fontFamily: 'var(--font-interface)',
        fontSize: 11,
        cursor: 'pointer',
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
