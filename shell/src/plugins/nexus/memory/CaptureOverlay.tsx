// Capture overlay — small modal shown on Cmd+Alt+N. Reads from
// `useCaptureStore`; writes go through the `nexus.memory.captureCommit`
// command (registered in `./index.ts`) so the kernel-routing path is
// owned by the plugin, not the view.
//
// Keyboard:
//   • Esc       → cancel (close overlay, no write)
//   • Mod+Enter → save (fires `nexus.memory.captureCommit`)

import { useEffect, useRef } from 'react'

import { useCaptureStore } from './captureStore'
import type { CommandsAPI } from '../../../types/plugin.ts'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'

const COMMAND_COMMIT = 'nexus.memory.captureCommit'

export interface CaptureOverlayProps {
  /** Wired from the plugin's `activate` so the modal can fire commit
   *  through the same plugin command path that Mod+Enter uses inside the
   *  textarea. Tests render the component without this prop and drive
   *  `commitCapture` directly. */
  commands?: CommandsAPI
}

export function CaptureOverlay({ commands }: CaptureOverlayProps = {}) {
  const open = useCaptureStore((s) => s.open)
  const draft = useCaptureStore((s) => s.draft)
  const error = useCaptureStore((s) => s.error)
  const setDraft = useCaptureStore((s) => s.setDraft)
  const close = useCaptureStore((s) => s.close)

  const textareaRef = useRef<HTMLTextAreaElement | null>(null)

  // Focus the textarea when the overlay opens. requestAnimationFrame so
  // the modal mounts before we steal focus.
  useEffect(() => {
    if (!open) return
    requestAnimationFrame(() => textareaRef.current?.focus())
  }, [open])

  // Esc to dismiss. Scoped to the modal lifetime — no leaks when the
  // overlay closes.
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        close()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, close])

  if (!open) return null

  const fireCommit = () => {
    if (!commands) return
    void commands.execute(COMMAND_COMMIT)
  }

  const onTextareaKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Mod+Enter saves; bare Enter inserts a newline as usual.
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault()
      fireCommit()
    }
  }

  return (
    <Modal>
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="nexus-memory-capture-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) close()
      }}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.55)',
        display: 'flex',
        alignItems: 'flex-start',
        justifyContent: 'center',
        zIndex: zIndex.overlayModal,
        padding: 64,
        pointerEvents: 'auto',
      }}
    >
      <div
        style={{
          width: 'min(560px, 100%)',
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
          gap: 12,
        }}
      >
        <div
          id="nexus-memory-capture-title"
          style={{ fontSize: 13, fontWeight: 500, color: 'var(--text-normal)' }}
        >
          Quick capture to Inbox
        </div>
        <textarea
          ref={textareaRef}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onTextareaKey}
          rows={6}
          placeholder="Type or paste — Mod+Enter to save, Esc to cancel"
          style={{
            width: '100%',
            background: 'var(--background-secondary)',
            color: 'var(--text-normal)',
            border: '1px solid var(--divider-color)',
            borderRadius: 'var(--radius-s)',
            padding: 10,
            font: 'inherit',
            resize: 'vertical',
            outline: 'none',
          }}
        />
        {error !== null && (
          <div
            role="alert"
            style={{
              color: 'var(--risk)',
              fontSize: 12,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
            }}
          >
            {error}
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button
            type="button"
            onClick={close}
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
            onClick={fireCommit}
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
            Save
          </button>
        </div>
      </div>
    </div>
    </Modal>
  )
}
