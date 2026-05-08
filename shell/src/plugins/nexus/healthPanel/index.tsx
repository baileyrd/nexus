// shell/src/plugins/nexus/healthPanel/index.tsx
//
// BL-093 follow-up — kernel health panel.
//
// Sidebar leaf that polls `com.nexus.security::metrics_snapshot`
// every 5 s and renders a triage view:
//   - Event-bus queue depth (gauge — the closest thing to "is the
//     kernel about to drop subscribers" the snapshot exposes).
//   - IPC counts + p50 / p95 / p99 latency per (plugin, command).
//   - Per-capability granted / denied counters (denials surface
//     first — the most actionable cell).
//   - Per-plugin event-bus publish counters.
//   - Sentinel `metrics_dropped_total` (cap-overflow indicator).
//
// Default-off in the catalog — typical users don't need this; ops
// surface targeted at developers triaging a slow / chatty plugin.

import { createElement, useEffect, useState } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, viewRegistry, workspace, type Leaf } from '../../../workspace'
import { createRoot, type Root } from 'react-dom/client'
import { clientLogger } from '../../../clientLogger'
import {
  buildCapabilityRows,
  buildEventBusRows,
  buildIpcRows,
  formatCount,
  formatDuration,
  type MetricsSnapshot,
} from './metricsFormat'

const PLUGIN_ID = 'nexus.healthPanel'
const VIEW_TYPE = 'health-panel'
const COMMAND_FOCUS = 'nexus.healthPanel.focus'

const SECURITY_PLUGIN_ID = 'com.nexus.security'
const CMD_METRICS_SNAPSHOT = 'metrics_snapshot'

/** Refresh interval for the snapshot poll. 5 s keeps the panel
 *  responsive to a sudden uptick (e.g. a runaway plugin) without
 *  burning kernel cycles when the panel is open and idle. */
const POLL_INTERVAL_MS = 5_000

interface HealthPanelViewProps {
  api: PluginAPI
}

/** Top-level component for the panel. Owns the polling loop + the
 *  rendered snapshot. Errors during a poll are swallowed and surfaced
 *  in the header — a transient kernel hiccup shouldn't blank the
 *  view. */
function HealthPanelView({ api }: HealthPanelViewProps) {
  const [snapshot, setSnapshot] = useState<MetricsSnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [lastFetchMs, setLastFetchMs] = useState<number | null>(null)

  useEffect(() => {
    let cancelled = false
    const refresh = async () => {
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (cancelled || !available) return
      try {
        const data = await api.kernel.invoke<MetricsSnapshot>(
          SECURITY_PLUGIN_ID,
          CMD_METRICS_SNAPSHOT,
          {},
        )
        if (cancelled) return
        setSnapshot(data)
        setError(null)
        setLastFetchMs(Date.now())
      } catch (err) {
        if (cancelled) return
        setError(err instanceof Error ? err.message : String(err))
      }
    }
    void refresh()
    const handle = window.setInterval(() => void refresh(), POLL_INTERVAL_MS)
    return () => {
      cancelled = true
      clearInterval(handle)
    }
  }, [api])

  return (
    <div
      style={{
        padding: 12,
        fontSize: 12,
        color: 'var(--text-normal)',
        lineHeight: 1.4,
        overflowY: 'auto',
        height: '100%',
      }}
    >
      <header style={{ marginBottom: 12 }}>
        <h3 style={{ margin: 0, fontSize: 13 }}>Kernel Health</h3>
        <div style={{ color: 'var(--text-faint)', fontSize: 11 }}>
          {error ? (
            <span style={{ color: 'var(--color-red, #cf222e)' }}>{error}</span>
          ) : lastFetchMs ? (
            `Last refresh: ${new Date(lastFetchMs).toLocaleTimeString()}`
          ) : (
            'Loading…'
          )}
        </div>
      </header>

      {snapshot && <HealthBody snapshot={snapshot} />}
    </div>
  )
}

interface HealthBodyProps {
  snapshot: MetricsSnapshot
}

function HealthBody({ snapshot }: HealthBodyProps) {
  const ipcRows = buildIpcRows(snapshot)
  const eventBusRows = buildEventBusRows(snapshot)
  const capabilityRows = buildCapabilityRows(snapshot)
  const totalDeniedCaps = capabilityRows.reduce((acc, r) => acc + r.denied, 0)

  return (
    <>
      {/* Gauges — three at-a-glance single-value cards */}
      <section style={{ display: 'flex', gap: 8, marginBottom: 16, flexWrap: 'wrap' }}>
        <Gauge label="Event-bus queue" value={formatCount(snapshot.event_bus_queue_depth)} />
        <Gauge
          label="Capability denials"
          value={formatCount(totalDeniedCaps)}
          warn={totalDeniedCaps > 0}
        />
        <Gauge
          label="Metrics dropped"
          value={formatCount(snapshot.metrics_dropped_total)}
          warn={snapshot.metrics_dropped_total > 0}
        />
      </section>

      {/* IPC table */}
      {ipcRows.length > 0 && (
        <section style={{ marginBottom: 16 }}>
          <h4 style={{ margin: '0 0 6px', fontSize: 12 }}>IPC calls</h4>
          <table style={tableStyle}>
            <thead>
              <tr>
                <th style={cellHeadLeft}>Plugin / command</th>
                <th style={cellHeadRight}>Total</th>
                <th style={cellHeadRight}>Errors</th>
                <th style={cellHeadRight}>p50</th>
                <th style={cellHeadRight}>p95</th>
                <th style={cellHeadRight}>p99</th>
              </tr>
            </thead>
            <tbody>
              {ipcRows.map((row) => (
                <tr key={`${row.plugin}::${row.command}`}>
                  <td style={cellLeft}>
                    <code style={{ fontSize: 11 }}>{row.plugin}</code>
                    <span style={{ color: 'var(--text-faint)' }}>::</span>
                    <span>{row.command}</span>
                  </td>
                  <td style={cellRight}>{formatCount(row.total)}</td>
                  <td style={{ ...cellRight, color: row.errors > 0 ? 'var(--color-red, #cf222e)' : undefined }}>
                    {formatCount(row.errors)}
                  </td>
                  <td style={cellRight}>{formatDuration(row.histogram?.p50_ns ?? 0)}</td>
                  <td style={cellRight}>{formatDuration(row.histogram?.p95_ns ?? 0)}</td>
                  <td style={cellRight}>{formatDuration(row.histogram?.p99_ns ?? 0)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {/* Capability table */}
      {capabilityRows.length > 0 && (
        <section style={{ marginBottom: 16 }}>
          <h4 style={{ margin: '0 0 6px', fontSize: 12 }}>Capability checks</h4>
          <table style={tableStyle}>
            <thead>
              <tr>
                <th style={cellHeadLeft}>Plugin / capability</th>
                <th style={cellHeadRight}>Granted</th>
                <th style={cellHeadRight}>Denied</th>
              </tr>
            </thead>
            <tbody>
              {capabilityRows.map((row) => (
                <tr key={`${row.plugin}::${row.capability}`}>
                  <td style={cellLeft}>
                    <code style={{ fontSize: 11 }}>{row.plugin}</code>
                    <span style={{ color: 'var(--text-faint)' }}>::</span>
                    <span>{row.capability}</span>
                  </td>
                  <td style={cellRight}>{formatCount(row.granted)}</td>
                  <td style={{ ...cellRight, color: row.denied > 0 ? 'var(--color-red, #cf222e)' : undefined }}>
                    {formatCount(row.denied)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {/* Event-bus table */}
      {eventBusRows.length > 0 && (
        <section style={{ marginBottom: 16 }}>
          <h4 style={{ margin: '0 0 6px', fontSize: 12 }}>Event-bus publishes</h4>
          <table style={tableStyle}>
            <thead>
              <tr>
                <th style={cellHeadLeft}>Plugin</th>
                <th style={cellHeadRight}>Total</th>
              </tr>
            </thead>
            <tbody>
              {eventBusRows.map((row) => (
                <tr key={row.plugin}>
                  <td style={cellLeft}>
                    <code style={{ fontSize: 11 }}>{row.plugin}</code>
                  </td>
                  <td style={cellRight}>{formatCount(row.total)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {ipcRows.length === 0 && eventBusRows.length === 0 && capabilityRows.length === 0 && (
        <p style={{ color: 'var(--text-faint)' }}>
          No activity recorded yet. Trigger a few IPC calls or wait a bit
          for the kernel to populate.
        </p>
      )}
    </>
  )
}

interface GaugeProps {
  label: string
  value: string
  warn?: boolean
}

function Gauge({ label, value, warn = false }: GaugeProps) {
  return (
    <div
      style={{
        flex: '1 1 100px',
        minWidth: 100,
        padding: '8px 10px',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 4,
        background: 'var(--background-secondary)',
      }}
    >
      <div style={{ fontSize: 10, color: 'var(--text-faint)', textTransform: 'uppercase' }}>
        {label}
      </div>
      <div
        style={{
          fontSize: 18,
          fontWeight: 600,
          color: warn ? 'var(--color-red, #cf222e)' : 'var(--text-normal)',
        }}
      >
        {value}
      </div>
    </div>
  )
}

const tableStyle = {
  width: '100%',
  borderCollapse: 'collapse' as const,
  fontSize: 11,
}
const cellHeadLeft = {
  textAlign: 'left' as const,
  padding: '4px 6px',
  borderBottom: '1px solid var(--background-modifier-border)',
  color: 'var(--text-faint)',
  fontWeight: 600,
}
const cellHeadRight = { ...cellHeadLeft, textAlign: 'right' as const }
const cellLeft = {
  padding: '3px 6px',
  borderBottom: '1px solid var(--background-modifier-border)',
}
const cellRight = { ...cellLeft, textAlign: 'right' as const }

class HealthPanelPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(
    leaf: Leaf,
    private readonly api: PluginAPI,
  ) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(HealthPanelView, { api: this.api }))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const healthPanelPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Kernel Health',
    version: '0.1.0',
    core: false,
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Show Kernel Health', category: 'View' },
      ],
    },
  },

  activate(api: PluginAPI) {
    viewRegistry.register(VIEW_TYPE, (leaf) => new HealthPanelPaneView(leaf, api))

    api.commands.register(COMMAND_FOCUS, async () => {
      try {
        const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
        workspace.revealLeaf(leaf)
      } catch (err) {
        clientLogger.warn('[nexus.healthPanel] focus failed:', err)
      }
    })
  },
}
