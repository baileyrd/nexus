// Modal for creating a new `.bases` directory. Picks a starter
// template (Blank / Tasks / CRM / Projects / Notes) and a filename,
// then calls `createBase` via the kernel client. On success, returns
// the created relpath so the caller can emit `files:open`.

import { useEffect, useRef, useState } from 'react'
import { useNewBaseStore } from './newBaseStore'
import { BASE_TEMPLATES, type BaseTemplate } from './templates'
import { getBasesClient } from './runtime'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'

const EXT = '.bases'

export function NewBaseDialog() {
  const current = useNewBaseStore((s) => s.current)
  const resolve = useNewBaseStore((s) => s.resolveCurrent)

  const [template, setTemplate] = useState<BaseTemplate>(BASE_TEMPLATES[0])
  const [name, setName] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const nameInputRef = useRef<HTMLInputElement>(null)

  // Reset on each new request.
  useEffect(() => {
    if (!current) return
    setTemplate(BASE_TEMPLATES[0])
    setName('')
    setErr(null)
    setBusy(false)
    requestAnimationFrame(() => nameInputRef.current?.focus())
  }, [current?.id])

  useEffect(() => {
    if (!current) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        if (!busy) resolve(null)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [current, resolve, busy])

  if (!current) return null

  const submit = async () => {
    const trimmed = name.trim()
    if (!trimmed) {
      setErr('Enter a name.')
      return
    }
    if (/[\\/:*?"<>|]/.test(trimmed)) {
      setErr('Name contains an illegal character.')
      return
    }
    const stem = trimmed.toLowerCase().endsWith(EXT)
      ? trimmed.slice(0, -EXT.length)
      : trimmed
    const dirName = `${stem}${EXT}`
    const relpath = current.defaultParent
      ? `${current.defaultParent}/${dirName}`
      : dirName
    const client = getBasesClient()
    if (!client) {
      setErr('Bases plugin is not ready.')
      return
    }
    try {
      setBusy(true)
      setErr(null)
      await client.createBase(relpath, template.schema, template.seedRecords)
      resolve({ relpath })
    } catch (e) {
      setErr((e as Error).message ?? String(e))
      setBusy(false)
    }
  }

  return (
    <Modal>
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="nexus-new-base-title"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) resolve(null)
      }}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.55)',
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
          width: 'min(520px, 100%)',
          background: 'var(--bg)',
          color: 'var(--fg)',
          border: '1px solid var(--line)',
          borderRadius: 'var(--r)',
          boxShadow: '0 12px 48px rgba(0, 0, 0, 0.4)',
          fontFamily: 'var(--f-ui)',
          fontSize: 'var(--ui-size, 13px)',
          padding: 20,
          display: 'flex',
          flexDirection: 'column',
          gap: 14,
        }}
      >
        <div id="nexus-new-base-title" style={{ fontWeight: 600, fontSize: 14 }}>
          New base
        </div>

        <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <span style={{ color: 'var(--fg-muted)', fontSize: 11 }}>Name</span>
          <input
            ref={nameInputRef}
            type="text"
            value={name}
            placeholder="Tasks"
            onChange={(e) => setName(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                void submit()
              }
            }}
            style={{
              padding: '6px 10px',
              background: 'var(--bg-raised, #252529)',
              color: 'var(--fg)',
              border: '1px solid var(--line-soft, #2a2a2e)',
              borderRadius: 'var(--r)',
              font: 'inherit',
              outline: 'none',
            }}
          />
          {current.defaultParent && (
            <span style={{ color: 'var(--fg-dim, #6b7280)', fontSize: 11 }}>
              in {current.defaultParent}/
            </span>
          )}
        </label>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <span style={{ color: 'var(--fg-muted)', fontSize: 11 }}>Template</span>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: 6,
            }}
          >
            {BASE_TEMPLATES.map((t) => {
              const selected = t.id === template.id
              return (
                <button
                  key={t.id}
                  type="button"
                  onClick={() => setTemplate(t)}
                  style={{
                    textAlign: 'left',
                    padding: '8px 10px',
                    background: selected
                      ? 'var(--bg-selection, #2a2a35)'
                      : 'var(--bg-raised, #252529)',
                    color: 'var(--fg)',
                    border: `1px solid ${selected ? 'var(--accent, #60a5fa)' : 'var(--line-soft, #2a2a2e)'}`,
                    borderRadius: 'var(--r)',
                    font: 'inherit',
                    cursor: 'pointer',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 2,
                  }}
                >
                  <span style={{ fontWeight: 500 }}>{t.label}</span>
                  <span style={{ color: 'var(--fg-muted)', fontSize: 11 }}>
                    {t.description}
                  </span>
                </button>
              )
            })}
          </div>
        </div>

        {err && (
          <div style={{ color: 'var(--risk, #f48771)', fontSize: 12 }}>{err}</div>
        )}

        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button
            type="button"
            disabled={busy}
            onClick={() => resolve(null)}
            style={{
              padding: '6px 14px',
              background: 'var(--bg-raised)',
              color: 'var(--fg)',
              border: '1px solid var(--line-soft)',
              borderRadius: 'var(--r)',
              font: 'inherit',
              cursor: busy ? 'not-allowed' : 'pointer',
              opacity: busy ? 0.6 : 1,
            }}
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void submit()}
            style={{
              padding: '6px 14px',
              background: 'var(--accent, #60a5fa)',
              color: 'var(--bg)',
              border: 'none',
              borderRadius: 'var(--r)',
              font: 'inherit',
              fontWeight: 500,
              cursor: busy ? 'not-allowed' : 'pointer',
              opacity: busy ? 0.6 : 1,
            }}
          >
            {busy ? 'Creating…' : 'Create'}
          </button>
        </div>
      </div>
    </div>
    </Modal>
  )
}
