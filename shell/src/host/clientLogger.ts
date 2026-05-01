// SH-017: centralized renderer-side logger.
//
// Replaces raw console.* calls across shell/src. Keeps a fixed-size
// ring buffer (last RING_SIZE entries) that the Diagnostics UI can read.
// Forwards to console.* for visibility during development.
//
// SH-018 integration: when a Tauri `append_shell_log` command is
// available (main window boot, not popouts), entries are flushed to the
// Rust-side log at ~1s intervals so they survive page reloads.
//
// Usage:
//   import { clientLogger } from '../host/clientLogger'
//   clientLogger.info('[Boot] foo', someValue)
//   clientLogger.error('[App] render failed', err)

export type LogLevel = 'debug' | 'info' | 'warn' | 'error'

export interface LogEntry {
  ts: number
  level: LogLevel
  message: string
  args: unknown[]
}

const RING_SIZE = 200

class ClientLogger {
  private ring: LogEntry[] = []
  private flushTimer: ReturnType<typeof setTimeout> | null = null

  // ── public API ──────────────────────────────────────────────────────

  debug(message: string, ...args: unknown[]): void {
    this.write('debug', message, args)
  }

  info(message: string, ...args: unknown[]): void {
    this.write('info', message, args)
  }

  warn(message: string, ...args: unknown[]): void {
    this.write('warn', message, args)
  }

  error(message: string, ...args: unknown[]): void {
    this.write('error', message, args)
  }

  /** Return a snapshot of the ring buffer (oldest → newest). */
  getEntries(): readonly LogEntry[] {
    return this.ring
  }

  /** Clear the ring buffer. Does not affect the persisted log. */
  clear(): void {
    this.ring = []
  }

  // ── internals ────────────────────────────────────────────────────────

  private write(level: LogLevel, message: string, args: unknown[]): void {
    const entry: LogEntry = { ts: Date.now(), level, message, args }

    // Maintain ring: drop oldest when full.
    if (this.ring.length >= RING_SIZE) this.ring.shift()
    this.ring.push(entry)

    // Mirror to console for devtools visibility.
    const consoleFn = level === 'debug' ? console.debug
      : level === 'info'  ? console.info
      : level === 'warn'  ? console.warn
      : console.error
    if (args.length > 0) consoleFn(message, ...args)
    else consoleFn(message)

    // Schedule a Tauri flush (fire-and-forget, ignored if not available).
    this.scheduleFlush()
  }

  private scheduleFlush(): void {
    if (this.flushTimer !== null) return
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null
      void this.flush()
    }, 1000)
  }

  private async flush(): Promise<void> {
    // append_shell_log is registered in lib.rs (SH-017 Rust side, not yet
    // landed). Guard with try/catch so the logger never throws.
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const entries = this.ring.map((e) => ({
        ts: e.ts,
        level: e.level,
        message: e.args.length > 0
          ? `${e.message} ${e.args.map(safeStr).join(' ')}`
          : e.message,
      }))
      await invoke('append_shell_log', { entries })
    } catch {
      // Tauri command not available (popout, test, or Rust side not yet
      // implemented). Silent; ring buffer still accumulates.
    }
  }
}

function safeStr(v: unknown): string {
  if (v instanceof Error) return v.stack ?? v.message
  try { return JSON.stringify(v) } catch { return String(v) }
}

export const clientLogger = new ClientLogger()
