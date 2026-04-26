// shell/src/plugins/nexus/extensionsTab/ExtensionsTab.tsx
//
// OI-08 — "Running Extensions" Settings tab.
//
// Read-only observability surface backed by `pluginsStatusStore`
// (OI-09). Shows every plugin that has ever fired a lifecycle event
// on the EventBus, with its current state and last-error message
// when applicable. Clicking Disable on a community / default-off
// plugin routes through the same `set_plugin_enabled` Tauri command
// that the existing Plugins tab uses; built-ins are read-only.

import { useMemo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { usePluginsStatusStore } from '../../../stores/pluginsStatusStore'
import { ALL_PLUGINS, DEFAULT_OFF_PLUGINS } from '../../catalog'

const DEFAULT_OFF_IDS = new Set(DEFAULT_OFF_PLUGINS.map((p) => p.manifest.id))
const BUILTIN_IDS = new Set(ALL_PLUGINS.map((p) => p.manifest.id))

const STATE_BADGES: Record<string, { label: string; color: string }> = {
  active: { label: 'active', color: 'var(--nexus-color-success, #22c55e)' },
  inactive: { label: 'inactive', color: 'var(--nexus-color-muted, #9ca3af)' },
  error: { label: 'error', color: 'var(--nexus-color-danger, #ef4444)' },
  registered: { label: 'registered', color: 'var(--nexus-color-muted, #9ca3af)' },
  activating: { label: 'activating', color: 'var(--nexus-color-info, #3b82f6)' },
  deactivating: { label: 'deactivating', color: 'var(--nexus-color-info, #3b82f6)' },
}

interface Row {
  id: string
  state: string
  lastError?: { message: string; stack?: string }
  isBuiltin: boolean
  isDefaultOff: boolean
}

function useRows(): Row[] {
  const byId = usePluginsStatusStore((s) => s.byId)
  return useMemo(() => {
    const rows: Row[] = Object.entries(byId).map(([id, status]) => ({
      id,
      state: status.state,
      lastError: status.lastError,
      isBuiltin: BUILTIN_IDS.has(id),
      isDefaultOff: DEFAULT_OFF_IDS.has(id),
    }))
    rows.sort((a, b) => {
      const aErr = a.state === 'error' ? 0 : 1
      const bErr = b.state === 'error' ? 0 : 1
      if (aErr !== bErr) return aErr - bErr
      return a.id.localeCompare(b.id)
    })
    return rows
  }, [byId])
}

export function ExtensionsTab() {
  const rows = useRows()

  if (rows.length === 0) {
    return (
      <p className="settings-empty">
        No plugin lifecycle events seen yet. Open a forge to load the catalog.
      </p>
    )
  }

  return (
    <div className="extensions-tab">
      <h3 style={{ marginTop: 0 }}>Running Extensions</h3>
      <p className="settings-help" style={{ marginBottom: '1rem' }}>
        Live state of every plugin the shell has loaded this session. Errors
        surface here the moment a plugin&apos;s <code>activate()</code> throws.
      </p>
      <table style={{ width: '100%', borderCollapse: 'collapse' }}>
        <thead>
          <tr style={{ textAlign: 'left', borderBottom: '1px solid var(--nexus-color-border, #374151)' }}>
            <th style={{ padding: '0.4rem 0.5rem' }}>Plugin</th>
            <th style={{ padding: '0.4rem 0.5rem' }}>State</th>
            <th style={{ padding: '0.4rem 0.5rem' }}>Detail</th>
            <th style={{ padding: '0.4rem 0.5rem' }}></th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <ExtensionRow key={row.id} row={row} />
          ))}
        </tbody>
      </table>
    </div>
  )
}

interface ExtensionRowProps {
  row: Row
}

function ExtensionRow({ row }: ExtensionRowProps) {
  const badge = STATE_BADGES[row.state] ?? { label: row.state, color: 'var(--nexus-color-muted, #9ca3af)' }
  // Disable button is only meaningful for default-off built-ins (the
  // ones that can be enabled/disabled via the Plugins tab). Built-ins
  // that are always-on are not user-disable-able; community plugins
  // are not yet wired through this tab (they have their own toggle in
  // the Plugins tab).
  const canDisable = row.isDefaultOff && row.state !== 'inactive' && row.state !== 'error'

  const onDisable = async () => {
    try {
      await invoke('set_plugin_enabled', { pluginId: row.id, enabled: false })
      // The ExtensionHost will fire `plugin:deactivated`, which the
      // store catches and reflects on the next render — no manual
      // refresh here.
    } catch (err) {
      console.error(`[extensions-tab] disable failed for ${row.id}`, err)
    }
  }

  return (
    <tr style={{ borderBottom: '1px solid var(--nexus-color-border-subtle, #1f2937)' }}>
      <td style={{ padding: '0.4rem 0.5rem', fontFamily: 'var(--nexus-font-mono)' }}>
        {row.id}
      </td>
      <td style={{ padding: '0.4rem 0.5rem' }}>
        <span
          style={{
            display: 'inline-block',
            padding: '0.1rem 0.5rem',
            borderRadius: '0.25rem',
            background: badge.color,
            color: 'white',
            fontSize: '0.85em',
          }}
        >
          {badge.label}
        </span>
      </td>
      <td style={{ padding: '0.4rem 0.5rem', color: 'var(--nexus-color-muted, #9ca3af)' }}>
        {row.lastError ? (
          <span title={row.lastError.stack} style={{ color: 'var(--nexus-color-danger, #ef4444)' }}>
            {row.lastError.message}
          </span>
        ) : (
          ''
        )}
      </td>
      <td style={{ padding: '0.4rem 0.5rem', textAlign: 'right' }}>
        {canDisable ? (
          <button onClick={onDisable} className="settings-button">
            Disable
          </button>
        ) : null}
      </td>
    </tr>
  )
}
