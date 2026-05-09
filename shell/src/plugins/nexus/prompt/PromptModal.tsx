import { useEffect, useRef, useState } from 'react'

import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'
import { usePromptStore } from './promptStore'

/**
 * Styled prompt modal — replaces the legacy `window.prompt` fallback
 * that `api.input.prompt` used to call directly. Mirrors
 * ConfirmModal's frame so the two flows feel like one family.
 *
 * Keyboard:
 *   • Enter — commit (resolves with the input value, including '')
 *   • Esc   — cancel (resolves null)
 */
export function PromptModal() {
  const current = usePromptStore((s) => s.current)
  const resolve = usePromptStore((s) => s.resolveCurrent)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const [value, setValue] = useState('')

  useEffect(() => {
    if (!current) return
    setValue(current.initialValue)
    requestAnimationFrame(() => {
      inputRef.current?.focus()
      inputRef.current?.select()
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.id])

  if (!current) return null

  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      resolve(null)
    } else if (e.key === 'Enter') {
      // Don't intercept Enter inside any nested input — but the only
      // input here is the prompt's own, so this guard is mostly for
      // future-proofing against textarea additions.
      e.preventDefault()
      resolve(value)
    }
  }

  return (
    <Modal>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="nexus-prompt-message"
        onClick={(e) => {
          if (e.target === e.currentTarget) resolve(null)
        }}
        onKeyDown={onKey}
        style={{
          position: 'fixed',
          inset: 0,
          background: 'rgba(0, 0, 0, 0.55)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          zIndex: zIndex.overlayModal,
          pointerEvents: 'auto',
          padding: 32,
        }}
      >
        <div
          style={{
            width: 'min(420px, 100%)',
            background: 'var(--background-primary)',
            color: 'var(--text-normal)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            boxShadow: '0 12px 48px rgba(0, 0, 0, 0.4)',
            fontFamily: 'var(--font-interface)',
            fontSize: 'var(--ui-size, 13px)',
            padding: 18,
            display: 'flex',
            flexDirection: 'column',
            gap: 14,
          }}
        >
          <div
            id="nexus-prompt-message"
            style={{
              color: 'var(--text-normal)',
              lineHeight: 1.5,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
            }}
          >
            {current.message}
          </div>
          <input
            ref={inputRef}
            type="text"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder={current.placeholder}
            style={{
              width: '100%',
              padding: '6px 8px',
              background: 'var(--background-secondary)',
              color: 'var(--text-normal)',
              border: '1px solid var(--divider-color)',
              borderRadius: 'var(--radius-s)',
              font: 'inherit',
              outline: 'none',
            }}
          />
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button
              type="button"
              onClick={() => resolve(null)}
              style={{
                padding: '6px 14px',
                background: 'var(--background-secondary)',
                color: 'var(--text-normal)',
                border: '1px solid var(--divider-color)',
                borderRadius: 'var(--radius-s)',
                font: 'inherit',
                cursor: 'pointer',
              }}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => resolve(value)}
              style={{
                padding: '6px 14px',
                background: 'var(--interactive-accent)',
                color: 'var(--background-primary)',
                border: 'none',
                borderRadius: 'var(--radius-s)',
                font: 'inherit',
                fontWeight: 500,
                cursor: 'pointer',
              }}
            >
              OK
            </button>
          </div>
        </div>
      </div>
    </Modal>
  )
}
