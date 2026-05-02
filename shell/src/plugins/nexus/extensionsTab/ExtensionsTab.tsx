// shell/src/plugins/nexus/extensionsTab/ExtensionsTab.tsx
//
// OI-08 — "Running Extensions" Settings tab.
//
// Read-only observability surface backed by `pluginsStatusStore`
// (OI-09). Shows every plugin that has ever fired a lifecycle event
// on the EventBus, with its current state and last-error message
// when applicable. Clicking Disable routes through the same
// `set_plugin_enabled` Tauri command the Plugins tab uses.

import { useMemo } from 'react'
import { usePluginsStatusStore } from '../../../stores/pluginsStatusStore'
import { clientLogger } from '../../../clientLogger'

// `disableBuiltinPlugin` is dynamic-imported in the click handler, not
// at module top level — see comment on the click handler below for
// why. (Short version: catalog cycles + node-test compat + the
// hygiene rule that keeps plugins from reaching into shell host/.)

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
}

function useRows(): Row[] {
  const byId = usePluginsStatusStore((s) => s.byId)
  return useMemo(() => {
    const rows: Row[] = Object.entries(byId).map(([id, status]) => ({
      id,
      state: status.state,
      lastError: status.lastError,
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
  // Show Disable for any active plugin. `disableBuiltinPlugin` itself
  // gates the operation: it returns `{ ok: false, error: '...required
  // built-in...' }` for DEFAULT_ON plugins, so we let the host be the
  // source of truth rather than duplicating the catalog check here.
  const canDisable = row.state === 'active' || row.state === 'activating'

  const onDisable = async () => {
    // Mid-session disable: unload the plugin AND persist the disabled
    // state so it stays off across restarts. Mirrors the Plugins tab's
    // built-in toggle (`SettingsPanelView.handleBuiltinToggle`). The
    // `invoke('set_plugin_enabled')` path that the community-plugin
    // toggle uses ONLY persists — it does not call `host.unload(id)`,
    // which is why the earlier draft of this button appeared to do
    // nothing on click.
    //
    // Dynamic-imported here so this module's top-level imports stay
    // clean: a static `import` of `host/pluginActivation` would
    // transitively pull in the catalog (cycle, slows first paint)
    // and would need an entry in the import-hygiene allowlist.
    const { disableBuiltinPlugin } = await import('../../../host/pluginActivation')
    const result = await disableBuiltinPlugin(row.id)
    if (!result.ok) {
      clientLogger.error(`[extensions-tab] disable failed for ${row.id}: ${result.error}`)
    }
    // Success: ExtensionHost emits `plugin:deactivated`, the store
    // catches it, the row updates on the next render — no manual
    // refresh here.
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
