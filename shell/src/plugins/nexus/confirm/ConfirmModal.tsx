import { useEffect, useRef } from 'react'
import { useConfirmStore } from './confirmStore'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'

/**
 * Centred-overlay confirm dialog. Replaces the platform `window.confirm`
 * with a styled modal that respects the design tokens. Multiple
 * concurrent `api.input.confirm` calls queue server-side (see
 * confirmStore.enqueue); the modal advances through them one at a time.
 *
 * Keyboard:
 *   • Enter   → confirm
 *   • Esc     → cancel
 *   • Tab     → cycles between Confirm / Cancel; Confirm autofocuses
 */
export function ConfirmModal() {
  const current = useConfirmStore((s) => s.current)
  const resolve = useConfirmStore((s) => s.resolveCurrent)

  const confirmRef = useRef<HTMLButtonElement | null>(null)

  // Focus the confirm button when a new request lands so Enter
  // confirms by default. requestAnimationFrame to let the modal
  // mount before we grab focus.
  useEffect(() => {
    if (!current) return
    requestAnimationFrame(() => confirmRef.current?.focus())
  }, [current?.id])

  useEffect(() => {
    if (!current) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        resolve(false)
      } else if (e.key === 'Enter') {
        // Don't steal Enter from inputs that might be focused inside
        // a parent overlay — gate on focus being on the modal itself.
        const target = e.target as HTMLElement | null
        if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) return
        e.preventDefault()
        resolve(true)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [current, resolve])

  if (!current) return null

  const confirmLabel = current.confirmLabel ?? 'Confirm'
  const cancelLabel = current.cancelLabel ?? 'Cancel'
  const danger = current.danger === true

  return (
    <Modal>
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="nexus-confirm-message"
      onClick={(e) => {
        if (e.target === e.currentTarget) resolve(false)
      }}
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
          gap: 16,
        }}
      >
        <div
          id="nexus-confirm-message"
          style={{
            color: 'var(--text-normal)',
            lineHeight: 1.5,
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
          }}
        >
          {current.message}
        </div>
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button
            type="button"
            onClick={() => resolve(false)}
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
            {cancelLabel}
          </button>
          <button
            ref={confirmRef}
            type="button"
            onClick={() => resolve(true)}
            style={{
              padding: '6px 14px',
              background: danger ? 'var(--risk)' : 'var(--interactive-accent)',
              color: 'var(--background-primary)',
              border: 'none',
              borderRadius: 'var(--radius-s)',
              font: 'inherit',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
    </Modal>
  )
}
