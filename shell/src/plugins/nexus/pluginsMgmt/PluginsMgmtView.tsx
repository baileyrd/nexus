import { useEffect, useMemo, useRef } from 'react'
import {
  usePluginsMgmtStore,
  type BuiltInPluginRow,
  type CommunityPluginRow,
  type PluginRow,
} from './pluginsMgmtStore'
import { getApi } from './pluginsMgmtRuntime'

const COMMAND_TOGGLE_COMMUNITY = 'nexus.plugins.toggleCommunity'

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

  const filtered = useMemo<PluginRow[]>(() => {
    const q = query.trim().toLowerCase()
    if (!q) return rows
    return rows.filter((r) => {
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
  }, [rows, query])

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
          background: 'var(--bg-raised)',
          border: '1px solid var(--line)',
          borderRadius: 'var(--r-lg)',
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
            borderBottom: '1px solid var(--line-soft)',
          }}
        >
          <div
            style={{
              color: 'var(--fg)',
              fontFamily: 'var(--f-ui)',
              fontSize: 15,
              fontWeight: 600,
            }}
          >
            Plugins
          </div>
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
              background: 'var(--bg)',
              border: '1px solid var(--line-soft)',
              borderRadius: 'var(--r)',
              outline: 0,
              color: 'var(--fg)',
              fontFamily: 'var(--f-ui)',
              fontSize: 13,
              padding: '6px 10px',
            }}
          />
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
                color: 'var(--fg-dim)',
                fontFamily: 'var(--f-ui)',
                fontSize: 13,
              }}
            >
              No plugins match
            </div>
          ) : (
            filtered.map((r) =>
              r.kind === 'builtin' ? (
                <BuiltInRow key={`builtin:${r.id}`} row={r} />
              ) : (
                <CommunityRow key={`community:${r.id}`} row={r} />
              ),
            )
          )}
        </div>

        {/* Footer hint */}
        <div
          style={{
            padding: '8px 16px',
            borderTop: '1px solid var(--line-soft)',
            textAlign: 'right',
            color: 'var(--fg-dim)',
            fontFamily: 'var(--f-ui)',
            fontSize: 11,
          }}
        >
          Drop plugin folders into ~/.nexus-shell/plugins/ and restart.
        </div>
      </div>
    </div>
  )
}

// ── Row components ──────────────────────────────────────────────────────────

interface BuiltInRowProps {
  row: BuiltInPluginRow
}

function BuiltInRow({ row }: BuiltInRowProps) {
  return (
    <div style={rowStyle}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            color: 'var(--fg)',
            fontFamily: 'var(--f-ui)',
            fontSize: 13,
            fontWeight: 500,
          }}
        >
          {row.name}
        </div>
        <div
          style={{
            color: 'var(--fg-dim)',
            fontFamily: 'var(--f-mono)',
            fontSize: 11,
            marginTop: 2,
          }}
        >
          {row.id}
        </div>
      </div>
      <StateBadge state={row.state} error={row.error} />
      <div
        style={{
          color: 'var(--fg-muted)',
          fontFamily: 'var(--f-mono)',
          fontSize: 11,
          minWidth: 48,
          textAlign: 'right',
        }}
      >
        v{row.version}
      </div>
    </div>
  )
}

interface CommunityRowProps {
  row: CommunityPluginRow
}

function CommunityRow({ row }: CommunityRowProps) {
  const onToggle = () => {
    void getApi().commands.execute(COMMAND_TOGGLE_COMMUNITY, row.id)
  }

  return (
    <div style={rowStyle}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            color: 'var(--fg)',
            fontFamily: 'var(--f-ui)',
            fontSize: 13,
            fontWeight: 500,
          }}
        >
          {row.name}
        </div>
        <div
          style={{
            color: 'var(--fg-dim)',
            fontFamily: 'var(--f-mono)',
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
              color: 'var(--fg-muted)',
              fontFamily: 'var(--f-ui)',
              fontSize: 12,
              marginTop: 4,
            }}
          >
            {row.description}
          </div>
        )}
      </div>
      <StateBadge state={row.enabled ? 'active' : 'inactive'} />
      <div
        style={{
          color: 'var(--fg-muted)',
          fontFamily: 'var(--f-mono)',
          fontSize: 11,
          minWidth: 48,
          textAlign: 'right',
        }}
      >
        v{row.version}
      </div>
      <Toggle enabled={row.enabled} onToggle={onToggle} />
    </div>
  )
}

// ── Badges + toggle ─────────────────────────────────────────────────────────

function StateBadge({ state, error }: { state: string; error?: string }) {
  const { bg, fg, label } = badgeColours(state)
  return (
    <div
      title={state === 'error' ? error ?? 'Error' : undefined}
      style={{
        padding: '2px 8px',
        borderRadius: 'var(--r)',
        background: bg,
        color: fg,
        fontFamily: 'var(--f-ui)',
        fontSize: 11,
        fontWeight: 500,
        textTransform: 'capitalize',
        minWidth: 60,
        textAlign: 'center',
      }}
    >
      {label}
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
    bg: 'color-mix(in oklch, var(--fg-muted) 15%, transparent)',
    fg: 'var(--fg-muted)',
    label: state || 'inactive',
  }
}

function Toggle({ enabled, onToggle }: { enabled: boolean; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      aria-pressed={enabled}
      style={{
        width: 36,
        height: 18,
        borderRadius: 9,
        border: '1px solid var(--line)',
        background: enabled ? 'var(--accent)' : 'var(--bg)',
        padding: 0,
        position: 'relative',
        cursor: 'pointer',
        transition: 'background 120ms ease',
        flexShrink: 0,
      }}
    >
      <span
        style={{
          display: 'block',
          width: 12,
          height: 12,
          borderRadius: '50%',
          background: enabled ? 'var(--accent-ink)' : 'var(--fg-muted)',
          position: 'absolute',
          top: 2,
          left: enabled ? 20 : 2,
          transition: 'left 120ms ease',
        }}
      />
    </button>
  )
}

// ── Shared styles ────────────────────────────────────────────────────────────

const rowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 12,
  padding: '10px 16px',
  borderBottom: '1px solid var(--line-soft)',
}
