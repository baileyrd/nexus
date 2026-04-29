/**
 * BL-041 — AI background indexing daemon status badge.
 *
 * Polls `com.nexus.ai::index_status` every 2 s and renders a compact
 * "spinner + indexed-files count" indicator in the status bar. Mirrors
 * the polling cadence the daemon's debounce window flushes on, so the
 * badge generally lags the on-disk truth by at most one tick.
 *
 * The badge stays mounted but renders `null` when the daemon has
 * never run (`running === false && total_seen === 0`) — that's the
 * "no AI / no embedder configured" idle state and we don't want to
 * crowd the bar with a permanently grey dot.
 */

import { useEffect, useRef, useState } from 'react'
import type { PluginAPI } from '../../../types/plugin'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_INDEX_STATUS = 'index_status'
const POLL_INTERVAL_MS = 2_000

/** Wire shape of `com.nexus.ai::index_status`. Mirrors the
 *  `IndexStatus` Rust struct in `crates/nexus-ai/src/indexing_daemon.rs`. */
export interface IndexStatusSnapshot {
  indexed_files: number
  pending_files: number
  total_seen: number
  last_error: string | null
  running: boolean
}

interface IndexingStatusProps {
  api: PluginAPI
}

export function IndexingStatus({ api }: IndexingStatusProps) {
  const [status, setStatus] = useState<IndexStatusSnapshot | null>(null)
  // Latch errors so a transient kernel hiccup doesn't blank the badge.
  const lastGoodRef = useRef<IndexStatusSnapshot | null>(null)

  useEffect(() => {
    let cancelled = false

    async function poll() {
      try {
        const snap = await api.kernel.invoke<IndexStatusSnapshot>(
          AI_PLUGIN_ID,
          HANDLER_INDEX_STATUS,
          {},
        )
        if (!cancelled) {
          setStatus(snap)
          lastGoodRef.current = snap
        }
      } catch {
        // Soft-fail: keep last good snapshot so the badge doesn't
        // flicker if the kernel restarts mid-poll.
        if (!cancelled && lastGoodRef.current) {
          setStatus(lastGoodRef.current)
        }
      }
    }

    void poll()
    const id = window.setInterval(poll, POLL_INTERVAL_MS)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [api])

  if (!status) return null

  // Idle / never-ran: hide entirely. "Running but no events seen"
  // (running === true, total_seen === 0) still renders so users know
  // the daemon is alive after a fresh boot.
  if (!status.running && status.total_seen === 0) return null

  const busy = status.pending_files > 0
  const errored = status.last_error !== null
  const colour = errored
    ? 'var(--err, #d33)'
    : busy
      ? 'var(--accent, #4af)'
      : 'var(--ok, #3a3)'

  const label = errored
    ? 'index error'
    : busy
      ? `indexing ${status.pending_files}`
      : `${status.indexed_files} indexed`

  const tooltip = errored
    ? `Index error: ${status.last_error}`
    : `BL-041 indexing daemon — ${status.indexed_files} files indexed, ${status.pending_files} pending, ${status.total_seen} events observed`

  return (
    <span
      title={tooltip}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        padding: '0 2px',
        fontVariantNumeric: 'tabular-nums',
      }}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: '50%',
          flexShrink: 0,
          background: colour,
          boxShadow: busy ? `0 0 4px ${colour}` : 'none',
        }}
      />
      <span>{label}</span>
    </span>
  )
}
