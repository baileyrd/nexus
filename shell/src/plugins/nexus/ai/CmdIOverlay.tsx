// shell/src/plugins/nexus/ai/CmdIOverlay.tsx
//
// BL-032 — Cmd+I command-anywhere AI overlay.
// BL-033 — context chips gain click-to-remove; a model-switcher
// dropdown sits next to the rail and writes through to the global
// `ai.provider` / `ai.model` config keys (so the same selection
// persists into the chat surface and ghost completions).
//
// Lives in the `overlay` slot. Mirrors `commandPalette/CommandPalette.tsx`
// for backdrop/keydown/focus conventions; the streaming response area
// is the new bit.

import { useEffect, useRef } from 'react'
import { useCmdIStore } from './cmdIStore'
import { submitCmdI } from './cmdIRuntime'
import { getApi } from './cmdIApi'
import { useConfigStore, useConfigValue } from '../../../stores/configStore'
import type { ContextChip } from './contextContributors'

/** Click-to-remove chip rail. Each chip is a button so keyboard users
 *  can tab through and remove with Enter/Space. The visible dismiss
 *  affordance is a trailing "×" that's only rendered on hover/focus
 *  to keep the resting rail clean. */
function ContextChipRail({
  chips,
  onRemove,
}: {
  chips: ContextChip[]
  onRemove: (id: string) => void
}) {
  if (chips.length === 0) return null
  return (
    <div
      role="list"
      aria-label="Prompt context"
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 6,
        padding: '6px 16px 8px',
        borderBottom: '1px solid var(--line-soft)',
      }}
    >
      {chips.map((chip) => (
        <button
          key={chip.id}
          role="listitem"
          type="button"
          onClick={() => onRemove(chip.id)}
          title={`Remove ${chip.label}`}
          aria-label={`Remove ${chip.label}`}
          style={{
            background: 'var(--bg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            padding: '2px 8px',
            fontFamily: 'var(--f-ui)',
            fontSize: '0.78em',
            color: 'var(--fg-muted)',
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            cursor: 'pointer',
          }}
        >
          <span style={{ fontSize: '0.7em', opacity: 0.6 }}>
            {chipKindGlyph(chip.kind)}
          </span>
          <span>{chip.label}</span>
          <span aria-hidden style={{ opacity: 0.6 }}>
            ×
          </span>
        </button>
      ))}
    </div>
  )
}

function chipKindGlyph(kind: ContextChip['kind']): string {
  switch (kind) {
    case 'file':
      return '📄'
    case 'selection':
      return '✂'
    case 'block':
      return '▦'
    case 'row':
      return '≡'
    case 'node':
      return '◇'
    case 'query':
      return '?'
    case 'note':
      return '✎'
    default:
      return '•'
  }
}

/** BL-033 — model switcher. The popover is a native `<select>` so we
 *  inherit keyboard + a11y for free; styling matches the chip rail.
 *  Selecting an entry writes both `ai.provider` and `ai.model` into
 *  the persisted config store, which the AI plugin's
 *  `config:changed:*` listeners pick up and forward to the kernel
 *  via `set_config`. The same keys back the Settings panel surface,
 *  so toggles here become the new global default. */
const MODEL_PRESETS: Array<{
  id: string
  label: string
  provider: string
  model: string
}> = [
  // Anthropic — keep current (4.7) at top.
  { id: 'anthropic:claude-opus-4-7', label: 'Claude Opus 4.7', provider: 'anthropic', model: 'claude-opus-4-7' },
  { id: 'anthropic:claude-sonnet-4-6', label: 'Claude Sonnet 4.6', provider: 'anthropic', model: 'claude-sonnet-4-6' },
  { id: 'anthropic:claude-haiku-4-5', label: 'Claude Haiku 4.5', provider: 'anthropic', model: 'claude-haiku-4-5-20251001' },
  // OpenAI.
  { id: 'openai:gpt-4o', label: 'GPT-4o', provider: 'openai', model: 'gpt-4o' },
  { id: 'openai:gpt-4o-mini', label: 'GPT-4o mini', provider: 'openai', model: 'gpt-4o-mini' },
  // Local.
  { id: 'ollama:llama3.1', label: 'Ollama · llama3.1', provider: 'ollama', model: 'llama3.1' },
]

function ModelSwitcher() {
  const provider = useConfigValue<string>('ai.provider', '')
  const model = useConfigValue<string>('ai.model', '')

  // Build the lookup id the way the presets are keyed; surface a
  // `(custom)` row when the saved provider/model don't match a preset
  // so the user sees what's active without us silently overwriting it.
  const currentId = `${provider}:${model}`
  const isCustom = !MODEL_PRESETS.some((p) => p.id === currentId)
  const isUnset = provider.length === 0

  const onChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const id = e.target.value
    const preset = MODEL_PRESETS.find((p) => p.id === id)
    if (!preset) return
    const cfg = useConfigStore.getState()
    cfg.set('ai.provider', preset.provider)
    cfg.set('ai.model', preset.model)
  }

  return (
    <select
      value={isUnset ? '' : isCustom ? '__custom__' : currentId}
      onChange={onChange}
      title="Model used for this prompt and as the global default"
      aria-label="AI model"
      style={{
        background: 'var(--bg)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        padding: '2px 6px',
        fontFamily: 'var(--f-ui)',
        fontSize: '0.78em',
        color: 'var(--fg-muted)',
        cursor: 'pointer',
      }}
    >
      {isUnset ? (
        <option value="" disabled>
          Default (env)
        </option>
      ) : null}
      {isCustom && !isUnset ? (
        <option value="__custom__" disabled>
          {`${provider}${model ? ` · ${model}` : ''}`}
        </option>
      ) : null}
      {MODEL_PRESETS.map((p) => (
        <option key={p.id} value={p.id}>
          {p.label}
        </option>
      ))}
    </select>
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
  const removedChipIds = useCmdIStore((s) => s.removedChipIds)
  const status = useCmdIStore((s) => s.status)
  const setPrompt = useCmdIStore((s) => s.setPrompt)
  const removeChip = useCmdIStore((s) => s.removeChip)
  const close = useCmdIStore((s) => s.close)

  // BL-033 — derive the visible chip list lazily so a chip removed
  // mid-typing disappears without the underlying contributor having
  // to re-run.
  const visibleChips = chips.filter((c) => !removedChipIds.includes(c.id))

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
        <ContextChipRail chips={visibleChips} onRemove={removeChip} />
        <div
          style={{
            display: 'flex',
            justifyContent: 'flex-end',
            padding: '4px 16px 0',
          }}
        >
          <ModelSwitcher />
        </div>
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
