// shell/src/plugins/core/capabilityPrompt/CapabilityModalView.tsx
//
// WI-31 — Blocking high-risk consent modal. Fires once per plugin at
// install/first-enable (and on major/minor version bumps). Blocks
// plugin activation until the user approves or denies.

import { useEffect, useMemo, useRef, useState } from 'react'
import type { Capability } from '@nexus/extension-api'
import {
  CAPABILITY_INFO,
  bucketByRisk,
  chipColours,
  type RiskLevel,
} from '../../nexus/pluginsMgmt/capabilityInfo'
import { useCapabilityPromptStore } from './capabilityPromptStore'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'

const HEADER_BY_REASON: Record<
  'first-install' | 'version-bump' | 'capability-change',
  string
> = {
  'first-install': 'New plugin requests permissions',
  'version-bump': 'Plugin updated — please review new permissions',
  'capability-change': 'Plugin added new permissions',
}

export function CapabilityModalView() {
  const current = useCapabilityPromptStore((s) => s.currentModal)
  const resolve = useCapabilityPromptStore((s) => s.resolveCurrent)

  // High-risk caps start checked when they were previously granted;
  // unchecked for first-install / newly-added caps so the user has to
  // opt in deliberately.
  const [selected, setSelected] = useState<Set<Capability>>(new Set())
  const approveRef = useRef<HTMLButtonElement | null>(null)

  // Reset selection whenever the prompt head changes — FIFO semantics.
  useEffect(() => {
    if (!current) {
      setSelected(new Set())
      return
    }
    const priorSet = new Set(current.previouslyGranted)
    const preSelected = current.caps.filter((c) => {
      const meta = CAPABILITY_INFO[c]
      if (!meta) return false
      // Non-high-risk caps are auto-granted by the kernel regardless —
      // include them in the "selected" tally for display symmetry but
      // the approve path doesn't need to persist them.
      if (meta.risk !== 'high') return true
      return priorSet.has(c)
    })
    setSelected(new Set(preSelected))
    // Focus Approve button so Enter confirms — same pattern as
    // nexus.confirm.
    requestAnimationFrame(() => approveRef.current?.focus())
  }, [current?.pluginId, current?.version])

  if (!current) return null

  const toggle = (cap: Capability) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(cap)) next.delete(cap)
      else next.add(cap)
      return next
    })
  }

  const approve = () => {
    resolve(true, [...selected])
  }
  const deny = () => {
    resolve(false, [])
  }

  const buckets = bucketByRisk(current.caps)

  return (
    <Modal>
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="capability-modal-title"
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.6)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: zIndex.overlayToast,
        padding: 32,
        pointerEvents: 'auto',
      }}
      // Intentionally NO backdrop-click-to-dismiss. High-risk consent
      // must be explicit.
    >
      <div
        style={{
          width: 'min(560px, 100%)',
          maxHeight: '80vh',
          background: 'var(--bg)',
          color: 'var(--fg)',
          border: '1px solid var(--line)',
          borderRadius: 'var(--r)',
          boxShadow: '0 16px 48px rgba(0, 0, 0, 0.5)',
          fontFamily: 'var(--f-ui)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        <header
          style={{
            padding: '16px 20px',
            borderBottom: '1px solid var(--line-soft)',
          }}
        >
          <div
            id="capability-modal-title"
            style={{ fontSize: 15, fontWeight: 600 }}
          >
            {HEADER_BY_REASON[current.reason]}
          </div>
          <div
            style={{
              fontSize: 12,
              color: 'var(--fg-dim)',
              marginTop: 4,
              fontFamily: 'var(--f-mono)',
            }}
          >
            {current.pluginName}
            <span style={{ margin: '0 6px', opacity: 0.5 }}>·</span>
            {current.pluginId}
            <span style={{ margin: '0 6px', opacity: 0.5 }}>·</span>v
            {current.version}
          </div>
        </header>

        <div
          style={{
            padding: '12px 20px',
            flex: 1,
            minHeight: 0,
            overflowY: 'auto',
          }}
        >
          <p
            style={{
              margin: 0,
              fontSize: 13,
              lineHeight: 1.5,
              color: 'var(--fg)',
            }}
          >
            This plugin requests the following capabilities. High-risk
            items (in red) require your approval — uncheck any you want
            to deny. Lower-risk capabilities are listed for context and
            will be granted when you approve.
          </p>

          <CapBucketSection
            label="High risk"
            risk="high"
            caps={buckets.high}
            selected={selected}
            onToggle={toggle}
            checkable
          />
          <CapBucketSection
            label="Medium risk"
            risk="medium"
            caps={buckets.medium}
            selected={selected}
            onToggle={toggle}
            checkable={false}
          />
          <CapBucketSection
            label="Low risk"
            risk="low"
            caps={buckets.low}
            selected={selected}
            onToggle={toggle}
            checkable={false}
          />
        </div>

        <footer
          style={{
            display: 'flex',
            gap: 8,
            justifyContent: 'flex-end',
            padding: '12px 20px',
            borderTop: '1px solid var(--line-soft)',
          }}
        >
          <button
            type="button"
            onClick={deny}
            style={{
              padding: '6px 14px',
              background: 'var(--bg-raised)',
              color: 'var(--fg)',
              border: '1px solid var(--line-soft)',
              borderRadius: 'var(--r)',
              font: 'inherit',
              cursor: 'pointer',
            }}
          >
            Deny
          </button>
          <button
            ref={approveRef}
            type="button"
            onClick={approve}
            style={{
              padding: '6px 14px',
              background: 'var(--accent)',
              color: 'var(--accent-ink)',
              border: 'none',
              borderRadius: 'var(--r)',
              font: 'inherit',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            Approve
          </button>
        </footer>
      </div>
    </div>
    </Modal>
  )
}

interface BucketSectionProps {
  label: string
  risk: RiskLevel
  caps: Capability[]
  selected: Set<Capability>
  onToggle: (cap: Capability) => void
  checkable: boolean
}

function CapBucketSection({
  label,
  risk,
  caps,
  selected,
  onToggle,
  checkable,
}: BucketSectionProps) {
  // Hook must run unconditionally — moved above the early return to
  // satisfy react-hooks/rules-of-hooks (was a real bug: conditional
  // hook invocation depending on `caps.length`).
  const colours = useMemo(() => chipColours(risk), [risk])
  if (caps.length === 0) return null
  return (
    <section style={{ marginTop: 14 }}>
      <div
        style={{
          fontSize: 11,
          fontWeight: 600,
          textTransform: 'uppercase',
          letterSpacing: 0.5,
          color: colours.fg,
          marginBottom: 6,
        }}
      >
        {label}
      </div>
      <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
        {caps.map((cap) => {
          const meta = CAPABILITY_INFO[cap]
          const isSelected = selected.has(cap)
          return (
            <li
              key={cap}
              style={{
                display: 'flex',
                alignItems: 'flex-start',
                gap: 8,
                padding: '6px 8px',
                borderRadius: 'var(--r)',
                background: isSelected
                  ? colours.bg
                  : 'transparent',
                border: `1px solid ${isSelected ? colours.border : 'transparent'}`,
                marginBottom: 4,
              }}
            >
              {checkable ? (
                <input
                  type="checkbox"
                  checked={isSelected}
                  onChange={() => onToggle(cap)}
                  aria-label={`Grant ${cap}`}
                  style={{ marginTop: 2 }}
                />
              ) : (
                <span
                  aria-hidden
                  style={{
                    display: 'inline-block',
                    width: 12,
                    height: 12,
                    marginTop: 3,
                    borderRadius: 2,
                    background: colours.fg,
                    opacity: 0.6,
                  }}
                />
              )}
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  style={{
                    fontFamily: 'var(--f-mono)',
                    fontSize: 12,
                    color: 'var(--fg)',
                  }}
                >
                  {cap}
                </div>
                {meta && (
                  <div
                    style={{
                      fontSize: 12,
                      color: 'var(--fg-dim)',
                      marginTop: 1,
                    }}
                  >
                    {meta.description}
                  </div>
                )}
              </div>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
