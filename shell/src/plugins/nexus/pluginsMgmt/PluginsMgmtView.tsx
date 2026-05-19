import { useEffect, useMemo, useRef, useState } from 'react'
import type { Capability } from '@nexus/extension-api'
import {
  usePluginsMgmtStore,
  type AvailablePluginRow,
  type BuiltInPluginRow,
  type CommunityPluginRow,
  type PluginRow,
} from './pluginsMgmtStore'
import { getApi } from './pluginsMgmtRuntime'
import {
  CAPABILITY_INFO,
  bucketByRisk,
  chipColours,
  hasHighRisk,
  type RiskLevel,
} from './capabilityInfo'
import { usePluginsStatusStore } from '../../../stores/pluginsStatusStore'

const COMMAND_TOGGLE_COMMUNITY = 'nexus.plugins.toggleCommunity'
const COMMAND_REVIEW_CAPS = 'nexus.plugins.reviewCapabilities'
const COMMAND_ENABLE_BUILTIN = 'nexus.plugins.enableBuiltin'

/**
 * Modal listing every plugin the shell has loaded — built-in (nexus.* /
 * core.*) and community (drop-folder). Mirrors nexus.commandPalette's
 * overlay pattern: fixed backdrop, backdrop-click to dismiss, autofocused
 * filter input.
 */
export function PluginsMgmtView() {
  const visible = usePluginsMgmtStore((s) => s.visible)
  const query = usePluginsMgmtStore((s) => s.query)
  const rows = usePluginsMgmtStore((s) => s.rows)
  const setQuery = usePluginsMgmtStore((s) => s.setQuery)
  const close = usePluginsMgmtStore((s) => s.close)

  const inputRef = useRef<HTMLInputElement | null>(null)
  const [highRiskOnly, setHighRiskOnly] = useState(false)

  const filtered = useMemo<PluginRow[]>(() => {
    const q = query.trim().toLowerCase()
    let next = rows
    if (highRiskOnly) {
      next = next.filter(
        (r) =>
          r.kind !== 'available' &&
          r.capabilities &&
          hasHighRisk(r.capabilities),
      )
    }
    if (!q) return next
    return next.filter((r) => {
      const haystack = [
        r.id,
        r.name,
        r.version,
        r.kind === 'community' ? r.description ?? '' : '',
        r.kind === 'community' ? r.author ?? '' : '',
      ]
        .join(' ')
        .toLowerCase()
      return haystack.includes(q)
    })
  }, [rows, query, highRiskOnly])

  useEffect(() => {
    if (visible) {
      const id = requestAnimationFrame(() => inputRef.current?.focus())
      return () => cancelAnimationFrame(id)
    }
  }, [visible])

  if (!visible) return null

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      // Same rationale as CommandPalette: the App.tsx global keydown
      // short-circuits on INPUT focus so the registered `escape`
      // keybinding never fires here. Close directly.
      e.preventDefault()
      e.stopPropagation()
      close()
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
        background: 'oklch(0 0 0 / 0.45)',
        pointerEvents: 'auto',
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'flex-start',
        paddingTop: 80,
      }}
    >
      <div
        style={{
          width: 720,
          maxWidth: '90vw',
          maxHeight: '75vh',
          background: 'var(--background-secondary)',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 'var(--radius-l)',
          boxShadow: 'var(--shadow)',
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        {/* Header strip: title + filter input */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 12,
            padding: '12px 16px',
            borderBottom: '1px solid var(--divider-color)',
          }}
        >
          <div
            style={{
              color: 'var(--text-normal)',
              fontFamily: 'var(--font-interface)',
              fontSize: 15,
              fontWeight: 600,
            }}
          >
            Plugins
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <label
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 6,
                color: 'var(--text-faint)',
                fontFamily: 'var(--font-interface)',
                fontSize: 12,
                cursor: 'pointer',
                userSelect: 'none',
              }}
              title="Show only plugins with at least one high-risk capability"
            >
              <input
                type="checkbox"
                checked={highRiskOnly}
                onChange={(e) => setHighRiskOnly(e.target.checked)}
              />
              High-risk only
            </label>
            <input
              ref={inputRef}
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={onInputKeyDown}
              placeholder="Filter plugins…"
              spellCheck={false}
              autoComplete="off"
              style={{
                width: 260,
                background: 'var(--background-primary)',
                border: '1px solid var(--divider-color)',
                borderRadius: 'var(--radius-s)',
                outline: 0,
                color: 'var(--text-normal)',
                fontFamily: 'var(--font-interface)',
                fontSize: 13,
                padding: '6px 10px',
              }}
            />
          </div>
        </div>

        {/* Body: scrollable list */}
        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: 'auto',
          }}
        >
          {filtered.length === 0 ? (
            <div
              style={{
                padding: '32px 16px',
                textAlign: 'center',
                color: 'var(--text-faint)',
                fontFamily: 'var(--font-interface)',
                fontSize: 13,
              }}
            >
              No plugins match
            </div>
          ) : (
            <SectionedRows rows={filtered} />
          )}
        </div>

        {/* Footer hint */}
        <div
          style={{
            padding: '8px 16px',
            borderTop: '1px solid var(--divider-color)',
            textAlign: 'right',
            color: 'var(--text-faint)',
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
          }}
        >
          Drop plugin folders into ~/.nexus-shell/plugins/ and restart.
        </div>
      </div>
    </div>
  )
}

// ── Section renderer ────────────────────────────────────────────────────────

/**
 * WI-43: three buckets — Installed (built-in, loaded), Community (drop-folder),
 * and Available (default-off, shipped-but-dormant). Section headers are
 * suppressed when a bucket is empty so the modal stays terse.
 */
function SectionedRows({ rows }: { rows: PluginRow[] }) {
  const installed = rows.filter((r): r is BuiltInPluginRow => r.kind === 'builtin')
  const community = rows.filter((r): r is CommunityPluginRow => r.kind === 'community')
  const available = rows.filter((r): r is AvailablePluginRow => r.kind === 'available')

  return (
    <>
      {installed.length > 0 && (
        <>
          <SectionHeader label={`Installed (${installed.length})`} />
          {installed.map((r) => (
            <BuiltInRow key={`builtin:${r.id}`} row={r} />
          ))}
        </>
      )}
      {community.length > 0 && (
        <>
          <SectionHeader label={`Community (${community.length})`} />
          {community.map((r) => (
            <CommunityRow key={`community:${r.id}`} row={r} />
          ))}
        </>
      )}
      {available.length > 0 && (
        <>
          <SectionHeader
            label={`Available — disabled (${available.length})`}
          />
          {available.map((r) => (
            <AvailableRow key={`available:${r.id}`} row={r} />
          ))}
        </>
      )}
    </>
  )
}

function SectionHeader({ label }: { label: string }) {
  return (
    <div
      style={{
        padding: '8px 16px 6px 16px',
        background: 'var(--background-primary)',
        color: 'var(--text-muted)',
        fontFamily: 'var(--font-interface)',
        fontSize: 10,
        fontWeight: 600,
        letterSpacing: '0.08em',
        textTransform: 'uppercase',
        borderBottom: '1px solid var(--divider-color)',
        position: 'sticky',
        top: 0,
        zIndex: 1,
      }}
    >
      {label}
    </div>
  )
}

// ── Row components ──────────────────────────────────────────────────────────

/**
 * Read the live lifecycle state for `pluginId` from `pluginsStatusStore`,
 * falling back to a per-row snapshot value when no `plugin:activated` /
 * `plugin:deactivated` / `plugin:error` event has fired yet for that id.
 *
 * The store is primed at module load (via the side-effect import in
 * `pluginsMgmt/index.ts`) and subscribes to the EventBus before any
 * plugin activates, so once boot has progressed past `host.loadAll` the
 * store value is authoritative. Until then the fallback covers the
 * window where the modal could open before any lifecycle event has
 * propagated (rare — the modal command requires user input, but the
 * fallback keeps default-on plugins from rendering as `inactive`).
 */
function useLivePluginState(
  pluginId: string,
  fallback: { state: string; error?: string },
): { state: string; error?: string } {
  const status = usePluginsStatusStore((s) => s.byId[pluginId])
  if (!status) return fallback
  return { state: status.state, error: status.lastError?.message }
}

interface BuiltInRowProps {
  row: BuiltInPluginRow
}

function BuiltInRow({ row }: BuiltInRowProps) {
  const live = useLivePluginState(row.id, { state: row.state, error: row.error })
  return (
    <div style={rowOuterStyle}>
      <div style={rowStyle}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              color: 'var(--text-normal)',
              fontFamily: 'var(--font-interface)',
              fontSize: 13,
              fontWeight: 500,
            }}
          >
            {row.name}
          </div>
          <div
            style={{
              color: 'var(--text-faint)',
              fontFamily: 'var(--font-monospace)',
              fontSize: 11,
              marginTop: 2,
            }}
          >
            {row.id}
          </div>
        </div>
        <StateBadge state={live.state} error={live.error} />
        <div
          style={{
            color: 'var(--text-muted)',
            fontFamily: 'var(--font-monospace)',
            fontSize: 11,
            minWidth: 48,
            textAlign: 'right',
          }}
        >
          v{row.version}
        </div>
      </div>
      <CapabilityChips capabilities={row.capabilities} />
    </div>
  )
}

interface CommunityRowProps {
  row: CommunityPluginRow
}

function CommunityRow({ row }: CommunityRowProps) {
  const onToggle = () => {
    if (row.incompatible) return
    void getApi().commands.execute(COMMAND_TOGGLE_COMMUNITY, row.id)
  }
  const onReview = () => {
    void getApi().commands.execute(COMMAND_REVIEW_CAPS, row.id)
  }

  const incompat = row.incompatible
  const incompatTitle = incompat
    ? `Incompatible — requires apiVersion ${incompat.requested}, ` +
      `shell supports ${incompat.supported}`
    : undefined

  // Prefer the lifecycle store's view (`active` / `inactive` / `error`)
  // over the `row.enabled` heuristic — an enabled plugin whose activate
  // threw should render as `error`, not `active`.
  const live = useLivePluginState(row.id, {
    state: row.enabled ? 'active' : 'inactive',
  })

  const summary = row.grantSummary
  const showReview =
    !incompat &&
    row.capabilities !== null &&
    row.capabilities.length > 0
  // "Granted N/M" subtitle only makes sense when the plugin declares at
  // least one HIGH-risk capability (low/medium are auto-granted and the
  // tally would always read 0/0). `declared === null` means the manifest
  // omitted the field — no subtitle at all.
  const showGrantSubtitle =
    summary !== undefined &&
    summary.declared !== null &&
    summary.declared > 0

  return (
    <div style={rowOuterStyle}>
      <div style={rowStyle}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              color: 'var(--text-normal)',
              fontFamily: 'var(--font-interface)',
              fontSize: 13,
              fontWeight: 500,
            }}
          >
            {row.name}
          </div>
          <div
            style={{
              color: 'var(--text-faint)',
              fontFamily: 'var(--font-monospace)',
              fontSize: 11,
              marginTop: 2,
            }}
          >
            {row.id}
            {row.author && (
              <>
                <span style={{ margin: '0 6px' }}>·</span>
                <span>{row.author}</span>
              </>
            )}
          </div>
          {row.description && (
            <div
              style={{
                color: 'var(--text-muted)',
                fontFamily: 'var(--font-interface)',
                fontSize: 12,
                marginTop: 4,
              }}
            >
              {row.description}
            </div>
          )}
          {showGrantSubtitle && summary && (
            <div
              style={{
                color: summary.denied
                  ? 'var(--risk)'
                  : 'var(--text-muted)',
                fontFamily: 'var(--font-interface)',
                fontSize: 11,
                marginTop: 4,
              }}
              title={
                summary.denied
                  ? 'You denied this plugin — click Review to re-approve.'
                  : `${summary.granted} of ${summary.declared} high-risk ` +
                    `capabilities granted`
              }
            >
              {summary.denied
                ? 'denied — click Review to re-approve'
                : `Granted ${summary.granted}/${summary.declared} high-risk`}
            </div>
          )}
          {incompat && (
            <div
              title={incompatTitle}
              style={{
                color: 'var(--risk)',
                fontFamily: 'var(--font-interface)',
                fontSize: 11,
                marginTop: 4,
              }}
            >
              Incompatible — requires apiVersion {incompat.requested},{' '}
              shell supports {incompat.supported}.
            </div>
          )}
        </div>
        {showReview && (
          <button
            type="button"
            onClick={onReview}
            title="Review declared capabilities and grants"
            style={{
              padding: '2px 8px',
              background: 'transparent',
              color: 'var(--text-faint)',
              border: '1px solid var(--divider-color)',
              borderRadius: 'var(--radius-s)',
              fontFamily: 'var(--font-interface)',
              fontSize: 11,
              cursor: 'pointer',
            }}
          >
            Review
          </button>
        )}
        <StateBadge
          state={incompat ? 'error' : live.state}
          error={incompat ? incompatTitle : live.error}
          labelOverride={incompat ? 'incompatible' : undefined}
        />
        <div
          style={{
            color: 'var(--text-muted)',
            fontFamily: 'var(--font-monospace)',
            fontSize: 11,
            minWidth: 48,
            textAlign: 'right',
          }}
        >
          v{row.version}
        </div>
        <Toggle
          enabled={row.enabled}
          onToggle={onToggle}
          disabled={!!incompat}
        />
      </div>
      <CapabilityChips capabilities={row.capabilities} />
    </div>
  )
}

// ── Available (default-off) row ─────────────────────────────────────────────

interface AvailableRowProps {
  row: AvailablePluginRow
}

/**
 * WI-43: a dormant built-in plugin. One-click Enable writes the id into
 * the `plugins.enabled` config key; the modal surfaces a toast saying a
 * reload is needed (no in-session hot-activate yet).
 */
function AvailableRow({ row }: AvailableRowProps) {
  const onEnable = () => {
    void getApi().commands.execute(COMMAND_ENABLE_BUILTIN, row.id)
  }
  return (
    <div style={rowOuterStyle}>
      <div style={rowStyle}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              color: 'var(--text-normal)',
              fontFamily: 'var(--font-interface)',
              fontSize: 13,
              fontWeight: 500,
            }}
          >
            {row.name}
          </div>
          <div
            style={{
              color: 'var(--text-faint)',
              fontFamily: 'var(--font-monospace)',
              fontSize: 11,
              marginTop: 2,
            }}
          >
            {row.id}
          </div>
        </div>
        <div
          style={{
            padding: '2px 8px',
            borderRadius: 'var(--radius-s)',
            background: 'color-mix(in oklch, var(--text-muted) 15%, transparent)',
            color: 'var(--text-muted)',
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
            fontWeight: 500,
            minWidth: 60,
            textAlign: 'center',
          }}
        >
          disabled
        </div>
        <div
          style={{
            color: 'var(--text-muted)',
            fontFamily: 'var(--font-monospace)',
            fontSize: 11,
            minWidth: 48,
            textAlign: 'right',
          }}
        >
          v{row.version}
        </div>
        <button
          type="button"
          onClick={onEnable}
          title="Add to plugins.enabled and reload to activate"
          style={{
            padding: '4px 12px',
            background: 'var(--interactive-accent)',
            color: 'var(--interactive-accent-ink)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
            fontWeight: 500,
            cursor: 'pointer',
          }}
        >
          Enable
        </button>
      </div>
    </div>
  )
}

// ── Badges + toggle ─────────────────────────────────────────────────────────

function StateBadge({
  state,
  error,
  labelOverride,
}: {
  state: string
  error?: string
  /** WI-33: override the rendered label without changing the colour bucket. */
  labelOverride?: string
}) {
  const { bg, fg, label } = badgeColours(state)
  return (
    <div
      title={state === 'error' ? error ?? 'Error' : undefined}
      style={{
        padding: '2px 8px',
        borderRadius: 'var(--radius-s)',
        background: bg,
        color: fg,
        fontFamily: 'var(--font-interface)',
        fontSize: 11,
        fontWeight: 500,
        textTransform: 'capitalize',
        minWidth: 60,
        textAlign: 'center',
      }}
    >
      {labelOverride ?? label}
    </div>
  )
}

function badgeColours(state: string): { bg: string; fg: string; label: string } {
  if (state === 'active') {
    return {
      bg: 'color-mix(in oklch, var(--ok) 20%, transparent)',
      fg: 'var(--ok)',
      label: 'active',
    }
  }
  if (state === 'error') {
    return {
      bg: 'color-mix(in oklch, var(--risk) 20%, transparent)',
      fg: 'var(--risk)',
      label: 'error',
    }
  }
  return {
    bg: 'color-mix(in oklch, var(--text-muted) 15%, transparent)',
    fg: 'var(--text-muted)',
    label: state || 'inactive',
  }
}

function Toggle({
  enabled,
  onToggle,
  disabled = false,
}: {
  enabled: boolean
  onToggle: () => void
  disabled?: boolean
}) {
  return (
    <button
      onClick={onToggle}
      aria-pressed={enabled}
      aria-disabled={disabled || undefined}
      disabled={disabled}
      style={{
        width: 36,
        height: 18,
        borderRadius: 9,
        border: '1px solid var(--background-modifier-border)',
        background: enabled ? 'var(--interactive-accent)' : 'var(--background-primary)',
        padding: 0,
        position: 'relative',
        cursor: disabled ? 'not-allowed' : 'pointer',
        transition: 'background 120ms ease',
        flexShrink: 0,
        opacity: disabled ? 0.4 : 1,
      }}
    >
      <span
        style={{
          display: 'block',
          width: 12,
          height: 12,
          borderRadius: '50%',
          background: enabled ? 'var(--interactive-accent-ink)' : 'var(--text-muted)',
          position: 'absolute',
          top: 2,
          left: enabled ? 20 : 2,
          transition: 'left 120ms ease',
        }}
      />
    </button>
  )
}

// ── Capability chips ────────────────────────────────────────────────────────
//
// Renders the per-plugin declared capability list as a row of small,
// risk-coloured chips. Three states:
//
//   capabilities === null  — manifest field missing.        "(unknown)"
//   capabilities === []    — declared but empty.            "(none)"
//   non-empty array        — chips, sorted high → medium → low.
//
// Tooltip on each chip is the human description from `CAPABILITY_INFO`,
// prefixed with the risk level so screen-reader users get the same
// signal a sighted user gets from the colour.

function CapabilityChips({
  capabilities,
}: {
  capabilities: Capability[] | null
}) {
  if (capabilities === null) {
    return (
      <div style={chipRowStyle}>
        <span style={mutedNoteStyle} title="Plugin manifest does not declare a capabilities list">
          (unknown)
        </span>
      </div>
    )
  }
  if (capabilities.length === 0) {
    return (
      <div style={chipRowStyle}>
        <span style={mutedNoteStyle} title="Plugin declared no capabilities">
          (none)
        </span>
      </div>
    )
  }

  const buckets = bucketByRisk(capabilities)
  // Render high → medium → low so the user's eye lands on the
  // most-dangerous capabilities first.
  const ordered: Array<{ risk: RiskLevel; cap: Capability }> = [
    ...buckets.high.map((cap) => ({ risk: 'high' as const, cap })),
    ...buckets.medium.map((cap) => ({ risk: 'medium' as const, cap })),
    ...buckets.low.map((cap) => ({ risk: 'low' as const, cap })),
  ]

  return (
    <div style={chipRowStyle}>
      {ordered.map(({ risk, cap }) => {
        const meta = CAPABILITY_INFO[cap]
        const c = chipColours(risk)
        return (
          <span
            key={cap}
            title={`${risk.toUpperCase()} — ${meta?.description ?? cap}`}
            style={{
              padding: '2px 8px',
              borderRadius: 'var(--radius-s)',
              background: c.bg,
              color: c.fg,
              border: `1px solid ${c.border}`,
              fontFamily: 'var(--font-monospace)',
              fontSize: 10,
              fontWeight: 500,
              lineHeight: 1.4,
              whiteSpace: 'nowrap',
            }}
          >
            {cap}
          </span>
        )
      })}
    </div>
  )
}

const chipRowStyle: React.CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 4,
  padding: '4px 16px 10px 16px',
}

const mutedNoteStyle: React.CSSProperties = {
  color: 'var(--text-muted)',
  fontFamily: 'var(--font-monospace)',
  fontSize: 10,
  fontStyle: 'italic',
}

// ── Shared styles ────────────────────────────────────────────────────────────

const rowOuterStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  borderBottom: '1px solid var(--divider-color)',
}

const rowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 12,
  padding: '10px 16px',
}
