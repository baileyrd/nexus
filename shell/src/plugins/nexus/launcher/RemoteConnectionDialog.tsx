// BL-148 — modal for collecting an `ssh://...` URI from the launcher.
// Replaces the BL-140 Phase 3b `window.prompt` MVP. The composed URI is
// shown in a read-only Preview field so the user can verify it before
// submitting.

import { useEffect, useMemo, useRef, useState } from 'react'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'

interface RemoteSubmission {
  uri: string
  label: string | null
}

interface Props {
  open: boolean
  busy?: boolean
  onSubmit: (entry: RemoteSubmission) => Promise<void> | void
  onCancel: () => void
}

interface FormErrors {
  host?: string
  port?: string
  path?: string
}

function composeUri(
  username: string,
  host: string,
  port: string,
  path: string,
): string {
  const u = username.trim()
  const h = host.trim()
  const p = port.trim()
  const fp = path.trim()
  if (!h && !fp && !u && !p) return ''
  const userPart = u ? `${u}@` : ''
  const portPart = p ? `:${p}` : ''
  const pathPart = fp.startsWith('/') ? fp : fp ? `/${fp}` : ''
  return `ssh://${userPart}${h}${portPart}${pathPart}`
}

function validate(
  host: string,
  port: string,
  forgePath: string,
): FormErrors {
  const errs: FormErrors = {}
  if (!host.trim()) {
    errs.host = 'Host is required.'
  }
  if (port.trim()) {
    const n = Number(port)
    if (!Number.isInteger(n) || n < 1 || n > 65535) {
      errs.port = 'Port must be a number between 1 and 65535.'
    }
  }
  const trimmedPath = forgePath.trim()
  if (!trimmedPath) {
    errs.path = 'Forge path is required.'
  } else if (!trimmedPath.startsWith('/')) {
    errs.path = 'Forge path must be absolute (start with /).'
  }
  return errs
}

const fieldLabelStyle = {
  display: 'flex',
  flexDirection: 'column' as const,
  gap: 4,
}
const labelTextStyle = { color: 'var(--text-muted)', fontSize: 11 }
const inputStyle = {
  padding: '6px 10px',
  background: 'var(--background-secondary)',
  color: 'var(--text-normal)',
  border: '1px solid var(--divider-color)',
  borderRadius: 'var(--radius-s)',
  font: 'inherit',
  outline: 'none',
}
const errorTextStyle = { color: 'var(--risk)', fontSize: 11 }

export function RemoteConnectionDialog({ open, busy = false, onSubmit, onCancel }: Props) {
  const [username, setUsername] = useState('')
  const [host, setHost] = useState('')
  const [port, setPort] = useState('')
  const [forgePath, setForgePath] = useState('')
  const [label, setLabel] = useState('')
  const [touched, setTouched] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const hostRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!open) return
    setUsername('')
    setHost('')
    setPort('')
    setForgePath('')
    setLabel('')
    setTouched(false)
    setSubmitError(null)
    requestAnimationFrame(() => hostRef.current?.focus())
  }, [open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        if (!busy) onCancel()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onCancel, busy])

  const composed = useMemo(
    () => composeUri(username, host, port, forgePath),
    [username, host, port, forgePath],
  )
  const errors = useMemo(() => validate(host, port, forgePath), [host, port, forgePath])
  const isValid = Object.keys(errors).length === 0

  if (!open) return null

  const showError = (key: keyof FormErrors): string | undefined =>
    touched ? errors[key] : undefined

  const submit = async () => {
    setTouched(true)
    if (!isValid) return
    if (!composed.startsWith('ssh://')) {
      setSubmitError('URI must use the ssh:// scheme.')
      return
    }
    setSubmitError(null)
    try {
      await onSubmit({
        uri: composed,
        label: label.trim() ? label.trim() : null,
      })
    } catch (e) {
      setSubmitError((e as Error).message ?? String(e))
    }
  }

  return (
    <Modal>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="nexus-remote-forge-title"
        onClick={(e) => {
          if (e.target === e.currentTarget && !busy) onCancel()
        }}
        style={{
          position: 'fixed',
          inset: 0,
          background: 'rgba(0, 0, 0, 0.55)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          zIndex: zIndex.overlayModal + 1,
          padding: 32,
          pointerEvents: 'auto',
          fontFamily: 'var(--font-interface)',
          color: 'var(--text-normal)',
        }}
      >
        <div
          style={{
            width: 'min(540px, 100%)',
            background: 'var(--background-primary)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            boxShadow: '0 12px 48px rgba(0, 0, 0, 0.4)',
            fontSize: 'var(--ui-size, 13px)',
            padding: 20,
            display: 'flex',
            flexDirection: 'column',
            gap: 14,
          }}
        >
          <div id="nexus-remote-forge-title" style={{ fontWeight: 600, fontSize: 14 }}>
            Open remote forge
          </div>
          <div style={{ color: 'var(--text-muted)', fontSize: 12 }}>
            Connect to a headless <code>nexus serve</code> over SSH. The full URI is
            composed from the fields below.
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
            <label style={fieldLabelStyle}>
              <span style={labelTextStyle}>Username (optional)</span>
              <input
                type="text"
                value={username}
                placeholder="alice"
                onChange={(e) => setUsername(e.currentTarget.value)}
                style={inputStyle}
                autoComplete="off"
              />
            </label>
            <label style={fieldLabelStyle}>
              <span style={labelTextStyle}>Host</span>
              <input
                ref={hostRef}
                type="text"
                value={host}
                placeholder="devbox.example.com"
                onChange={(e) => setHost(e.currentTarget.value)}
                style={inputStyle}
                autoComplete="off"
              />
              {showError('host') && <span style={errorTextStyle}>{errors.host}</span>}
            </label>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 2fr', gap: 10 }}>
            <label style={fieldLabelStyle}>
              <span style={labelTextStyle}>Port (optional)</span>
              <input
                type="text"
                inputMode="numeric"
                value={port}
                placeholder="22"
                onChange={(e) => setPort(e.currentTarget.value)}
                style={inputStyle}
                autoComplete="off"
              />
              {showError('port') && <span style={errorTextStyle}>{errors.port}</span>}
            </label>
            <label style={fieldLabelStyle}>
              <span style={labelTextStyle}>Friendly name (optional)</span>
              <input
                type="text"
                value={label}
                placeholder="devbox"
                onChange={(e) => setLabel(e.currentTarget.value)}
                style={inputStyle}
                autoComplete="off"
              />
            </label>
          </div>

          <label style={fieldLabelStyle}>
            <span style={labelTextStyle}>Forge path (absolute)</span>
            <input
              type="text"
              value={forgePath}
              placeholder="/srv/nexus/forge"
              onChange={(e) => setForgePath(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  void submit()
                }
              }}
              style={inputStyle}
              autoComplete="off"
            />
            {showError('path') && <span style={errorTextStyle}>{errors.path}</span>}
          </label>

          <label style={fieldLabelStyle}>
            <span style={labelTextStyle}>Preview</span>
            <input
              type="text"
              readOnly
              value={composed || 'ssh://…'}
              style={{
                ...inputStyle,
                background: 'var(--background-primary-alt, var(--background-secondary))',
                color: composed ? 'var(--text-normal)' : 'var(--text-faint)',
                fontFamily: 'var(--font-monospace, monospace)',
                fontSize: 12,
              }}
            />
          </label>

          {/* BL-148 follow-up: identity-file picker (stretch) + test-connection
              button (stretch) land here. */}

          {submitError && <div style={{ color: 'var(--risk)', fontSize: 12 }}>{submitError}</div>}

          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button
              type="button"
              disabled={busy}
              onClick={onCancel}
              style={{
                padding: '6px 14px',
                background: 'var(--background-secondary)',
                color: 'var(--text-normal)',
                border: '1px solid var(--divider-color)',
                borderRadius: 'var(--radius-s)',
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
                background: 'var(--interactive-accent)',
                color: 'var(--interactive-accent-ink, var(--background-primary))',
                border: 'none',
                borderRadius: 'var(--radius-s)',
                font: 'inherit',
                fontWeight: 500,
                cursor: busy ? 'not-allowed' : 'pointer',
                opacity: busy ? 0.6 : 1,
              }}
            >
              {busy ? 'Connecting…' : 'Connect'}
            </button>
          </div>
        </div>
      </div>
    </Modal>
  )
}
