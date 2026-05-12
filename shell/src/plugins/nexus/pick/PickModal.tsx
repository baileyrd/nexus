import { useEffect, useMemo, useRef, useState } from 'react'

import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'
import { usePickStore } from './pickStore'

/**
 * Centred-overlay list-picker modal. Mirrors `ConfirmModal`'s shape
 * so users see one visual style for both flows; differs in wiring a
 * filter input + arrow-key navigation across rows.
 *
 * Keyboard:
 *   • ↑ / ↓     — move selection
 *   • Enter     — pick the highlighted row
 *   • Esc       — dismiss (resolves null)
 *   • Anything  — debounce-free substring filter on label/description/detail
 */
export function PickModal() {
  const current = usePickStore((s) => s.current)
  const resolve = usePickStore((s) => s.resolveCurrent)

  const inputRef = useRef<HTMLInputElement | null>(null)
  const [filter, setFilter] = useState('')
  const [selectedIdx, setSelectedIdx] = useState(0)

  // Reset filter / selection whenever a fresh request lands.
  // We deliberately depend on `current?.id` rather than `current`
  // itself — the request payload (items / title) is immutable per
  // request and re-firing on every store mutation would clobber the
  // user's typed filter while they're scrolling rows.
   
  useEffect(() => {
    setFilter('')
    setSelectedIdx(0)
    if (current) {
      requestAnimationFrame(() => inputRef.current?.focus())
    }
  }, [current?.id])

  const filtered = useMemo(() => {
    if (!current) return []
    const q = filter.trim().toLowerCase()
    if (q === '') return current.items
    return current.items.filter((item) => {
      const haystack = [
        item.label,
        item.description ?? '',
        item.detail ?? '',
      ]
        .join(' ')
        .toLowerCase()
      return haystack.includes(q)
    })
  }, [current, filter])

  // Keep the selection in range as the filter narrows the list.
  useEffect(() => {
    if (selectedIdx >= filtered.length) {
      setSelectedIdx(Math.max(0, filtered.length - 1))
    }
  }, [filtered.length, selectedIdx])

  if (!current) return null

  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      resolve(null)
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIdx((i) => Math.min(filtered.length - 1, i + 1))
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIdx((i) => Math.max(0, i - 1))
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      const picked = filtered[selectedIdx]
      if (picked) resolve(picked)
      else resolve(null)
    }
  }

  return (
    <Modal>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="nexus-pick-title"
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
            width: 'min(540px, 100%)',
            maxHeight: '70vh',
            background: 'var(--background-primary)',
            color: 'var(--text-normal)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            boxShadow: '0 12px 48px rgba(0, 0, 0, 0.4)',
            fontFamily: 'var(--font-interface)',
            fontSize: 'var(--ui-size, 13px)',
            display: 'flex',
            flexDirection: 'column',
            overflow: 'hidden',
          }}
        >
          {current.title && (
            <div
              id="nexus-pick-title"
              style={{
                padding: '12px 14px 4px',
                color: 'var(--text-muted)',
                fontSize: '0.85em',
              }}
            >
              {current.title}
            </div>
          )}
          <div style={{ padding: 8 }}>
            <input
              ref={inputRef}
              type="text"
              value={filter}
              onChange={(e) => {
                setFilter(e.target.value)
                setSelectedIdx(0)
              }}
              placeholder={current.placeholder ?? 'Type to filter…'}
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
          </div>
          <div
            role="listbox"
            style={{
              flex: '1 1 auto',
              overflowY: 'auto',
              padding: '4px 0',
            }}
          >
            {filtered.length === 0 && (
              <div
                style={{
                  padding: '12px 14px',
                  color: 'var(--text-muted)',
                  fontStyle: 'italic',
                }}
              >
                No matches.
              </div>
            )}
            {filtered.map((item, idx) => {
              const isSel = idx === selectedIdx
              return (
                <div
                  key={`${item.label}::${idx}`}
                  role="option"
                  aria-selected={isSel}
                  onMouseEnter={() => setSelectedIdx(idx)}
                  onClick={() => resolve(item)}
                  style={{
                    padding: '8px 14px',
                    cursor: 'pointer',
                    background: isSel
                      ? 'var(--background-modifier-hover)'
                      : 'transparent',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 2,
                  }}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
                    <span style={{ color: 'var(--text-normal)' }}>{item.label}</span>
                    {item.description && (
                      <span style={{ color: 'var(--text-muted)', fontSize: '0.85em' }}>
                        {item.description}
                      </span>
                    )}
                  </div>
                  {item.detail && (
                    <span style={{ color: 'var(--text-muted)', fontSize: '0.8em' }}>
                      {item.detail}
                    </span>
                  )}
                </div>
              )
            })}
          </div>
        </div>
      </div>
    </Modal>
  )
}
