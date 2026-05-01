import { useEffect, useRef } from 'react'
import { useMcpStore, type ToolCallState } from './mcpStore'
import { Icon } from '../../../icons'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'

interface ToolCallModalProps {
  /**
   * Caller-supplied dispatcher. Receives the parsed JSON args object
   * (already validated by the modal). Returns when the kernel call
   * resolves; the modal reads result/error/status off the store so
   * the dispatcher just orchestrates IPC.
   */
  onRun: (server: string, tool: string, args: Record<string, unknown>) => Promise<void>
}

/**
 * Centred-overlay modal for invoking an MCP tool with free-form JSON
 * args. The kernel's `list_tools` doesn't expose `inputSchema` today,
 * so we don't render a typed form — the user edits a JSON object
 * directly. Common cases (`{}`, single-string arg) cover most tools
 * fine; structured input belongs to a follow-up once the kernel
 * surfaces tool schemas.
 *
 * Result rendering matches the MCP `Content` enum loosely: a `text`
 * variant gets prose, anything else gets a JSON dump. Errors (parse
 * + kernel) surface in a top-of-result band so retry doesn't require
 * scrolling.
 */
export function ToolCallModal({ onRun }: ToolCallModalProps) {
  const toolCall = useMcpStore((s) => s.toolCall)
  const setText = useMcpStore((s) => s.setToolCallArgsText)
  const close = useMcpStore((s) => s.closeToolCall)
  const setStatus = useMcpStore((s) => s.setToolCallStatus)

  const textareaRef = useRef<HTMLTextAreaElement | null>(null)

  // Focus the args textarea on open. Re-runs when the modal target
  // changes (server/tool combo) so opening a new tool resets focus.
  useEffect(() => {
    if (!toolCall) return
    requestAnimationFrame(() => textareaRef.current?.focus())
  }, [toolCall?.serverName, toolCall?.toolName])

  // Esc closes (only when no run is in flight — bailing mid-call
  // doesn't actually cancel the kernel-side dispatch, just hides the
  // result, which is more confusing than waiting).
  useEffect(() => {
    if (!toolCall) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && toolCall.status !== 'running') {
        e.preventDefault()
        close()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [toolCall, close])

  if (!toolCall) return null

  const handleRun = () => {
    let parsed: Record<string, unknown>
    try {
      const raw = JSON.parse(toolCall.argsText || '{}')
      if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) {
        throw new Error('Args must be a JSON object.')
      }
      parsed = raw as Record<string, unknown>
    } catch (err) {
      setStatus('error', { error: err instanceof Error ? err.message : String(err) })
      return
    }
    void onRun(toolCall.serverName, toolCall.toolName, parsed)
  }

  return (
    <Modal>
    <div
      onClick={(e) => {
        // Close on backdrop click only — guard against propagated
        // clicks bubbling up from the inner card.
        if (e.target === e.currentTarget && toolCall.status !== 'running') close()
      }}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.5)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: zIndex.overlayModal,
        padding: 32,
        pointerEvents: 'auto',
      }}
    >
      <div
        style={{
          width: 'min(640px, 100%)',
          maxHeight: '100%',
          display: 'flex',
          flexDirection: 'column',
          background: 'var(--bg)',
          color: 'var(--fg)',
          border: '1px solid var(--line)',
          borderRadius: 'var(--r)',
          boxShadow: '0 12px 48px rgba(0, 0, 0, 0.4)',
          fontFamily: 'var(--f-ui)',
          fontSize: 'var(--ui-size, 13px)',
          overflow: 'hidden',
        }}
      >
        <Header
          serverName={toolCall.serverName}
          toolName={toolCall.toolName}
          onClose={close}
          disabled={toolCall.status === 'running'}
        />
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
            padding: 16,
            overflow: 'auto',
          }}
        >
          <ArgsField
            value={toolCall.argsText}
            onChange={setText}
            disabled={toolCall.status === 'running'}
            ref={textareaRef}
          />
          <Footer
            onRun={handleRun}
            onCancel={close}
            status={toolCall.status}
          />
          {toolCall.error ? <ErrorBanner message={toolCall.error} /> : null}
          {toolCall.result ? <ResultPanel result={toolCall.result} /> : null}
        </div>
      </div>
    </div>
    </Modal>
  )
}

function Header({
  serverName,
  toolName,
  onClose,
  disabled,
}: {
  serverName: string
  toolName: string
  onClose: () => void
  disabled: boolean
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '10px 16px',
        borderBottom: '1px solid var(--line-soft)',
        background: 'var(--bg-raised)',
        flex: '0 0 auto',
      }}
    >
      <span
        style={{
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          color: 'var(--fg-muted)',
        }}
      >
        Call tool
      </span>
      <span
        style={{
          flex: '1 1 auto',
          fontFamily: 'var(--f-mono, monospace)',
          fontSize: 12,
          color: 'var(--fg)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
        title={`${serverName}::${toolName}`}
      >
        <span style={{ color: 'var(--fg-dim)' }}>{serverName}</span>
        <span style={{ color: 'var(--fg-dim)' }}> · </span>
        <span style={{ color: 'var(--accent)' }}>{toolName}</span>
      </span>
      <button
        type="button"
        aria-label="Close"
        onClick={onClose}
        disabled={disabled}
        onMouseEnter={(e) => {
          if (!disabled) (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
        }}
        onMouseLeave={(e) => {
          (e.currentTarget as HTMLButtonElement).style.background = 'transparent'
        }}
        style={{
          width: 24,
          height: 24,
          padding: 0,
          border: 0,
          background: 'transparent',
          color: 'var(--fg-muted)',
          cursor: disabled ? 'default' : 'pointer',
          borderRadius: 'var(--r)',
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          opacity: disabled ? 0.5 : 1,
        }}
      >
        <Icon name="x" size={14} />
      </button>
    </div>
  )
}

const ArgsField = (() => {
  // forwardRef-style by wrapping at module scope so the parent can
  // focus the textarea on open. Inline forwardRef would also work;
  // this keeps the type story simpler and avoids the React import.
  function ArgsFieldImpl(
    { value, onChange, disabled, ref }: {
      value: string
      onChange: (v: string) => void
      disabled: boolean
      ref: React.MutableRefObject<HTMLTextAreaElement | null>
    },
  ) {
    return (
      <div>
        <div
          style={{
            fontSize: 11,
            textTransform: 'uppercase',
            letterSpacing: '0.04em',
            color: 'var(--fg-muted)',
            marginBottom: 4,
          }}
        >
          Arguments (JSON)
        </div>
        <textarea
          ref={ref}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          rows={6}
          disabled={disabled}
          spellCheck={false}
          autoCapitalize="off"
          style={{
            width: '100%',
            boxSizing: 'border-box',
            padding: 8,
            background: 'var(--bg-raised)',
            color: 'var(--fg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            fontFamily: 'var(--f-mono, monospace)',
            fontSize: 12,
            lineHeight: 1.45,
            resize: 'vertical',
            outline: 'none',
            opacity: disabled ? 0.6 : 1,
          }}
        />
      </div>
    )
  }
  return ArgsFieldImpl
})()

function Footer({
  onRun,
  onCancel,
  status,
}: {
  onRun: () => void
  onCancel: () => void
  status: ToolCallState['status']
}) {
  const running = status === 'running'
  return (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
      <button
        type="button"
        onClick={onRun}
        disabled={running}
        style={{
          padding: '6px 14px',
          background: 'var(--accent)',
          color: 'var(--bg)',
          border: 'none',
          borderRadius: 'var(--r)',
          font: 'inherit',
          fontWeight: 500,
          cursor: running ? 'default' : 'pointer',
          opacity: running ? 0.5 : 1,
        }}
      >
        {running ? 'Running…' : 'Run'}
      </button>
      <button
        type="button"
        onClick={onCancel}
        disabled={running}
        style={{
          padding: '6px 14px',
          background: 'var(--bg-raised)',
          color: 'var(--fg)',
          border: '1px solid var(--line-soft)',
          borderRadius: 'var(--r)',
          font: 'inherit',
          cursor: running ? 'default' : 'pointer',
          opacity: running ? 0.5 : 1,
        }}
      >
        Cancel
      </button>
      {status === 'done' ? (
        <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--ok)' }}>
          Returned successfully.
        </span>
      ) : null}
    </div>
  )
}

function ErrorBanner({ message }: { message: string }) {
  return (
    <div
      style={{
        padding: '8px 10px',
        background: 'var(--bg-raised)',
        color: 'var(--risk)',
        border: '1px solid var(--risk)',
        borderRadius: 'var(--r)',
        fontSize: 12,
        lineHeight: 1.4,
        whiteSpace: 'pre-wrap',
      }}
    >
      {message}
    </div>
  )
}

function ResultPanel({ result }: { result: ToolCallState['result'] }) {
  if (!result) return null
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      <div
        style={{
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          color: result.isError ? 'var(--risk)' : 'var(--fg-muted)',
        }}
      >
        Result {result.isError ? '· tool reported error' : ''}
      </div>
      {result.content.length === 0 ? (
        <div style={{ color: 'var(--fg-dim)', fontSize: 12, fontStyle: 'italic' }}>
          (empty)
        </div>
      ) : (
        result.content.map((item, i) => <ContentItem key={i} item={item} />)
      )}
    </div>
  )
}

function ContentItem({ item }: { item: unknown }) {
  // MCP content variants use a `type` discriminator. We render `text`
  // as prose and dump anything else as JSON — matches what a typical
  // tool returns and avoids overcommitting on a schema we don't
  // control.
  if (item && typeof item === 'object') {
    const r = item as Record<string, unknown>
    if (r.type === 'text' && typeof r.text === 'string') {
      return (
        <pre
          style={{
            margin: 0,
            padding: 8,
            background: 'var(--bg-raised)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            fontFamily: 'var(--f-mono, monospace)',
            fontSize: 12,
            lineHeight: 1.45,
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            maxHeight: 320,
            overflow: 'auto',
          }}
        >
          {r.text}
        </pre>
      )
    }
  }
  return (
    <pre
      style={{
        margin: 0,
        padding: 8,
        background: 'var(--bg-raised)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        fontFamily: 'var(--f-mono, monospace)',
        fontSize: 11,
        lineHeight: 1.45,
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        color: 'var(--fg-muted)',
        maxHeight: 320,
        overflow: 'auto',
      }}
    >
      {safeJson(item)}
    </pre>
  )
}

function safeJson(v: unknown): string {
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}
