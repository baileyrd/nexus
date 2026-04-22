import { useEffect, useRef } from 'react'
import { Terminal } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'
import './terminal.css'
import type { KernelAPI, EventsAPI } from '../../../types/plugin'
import { useTerminalStore } from './terminalStore'

const PLUGIN_ID = 'com.nexus.terminal'
// Handler ids verified in crates/nexus-terminal/src/core_plugin.rs.
// `send_input` (text + auto-newline) is NOT used — raw keystrokes
// from xterm must go through `send_raw_input` so control sequences
// (arrows, Ctrl-C, tab) reach the shell verbatim.
const CMD_SEND_RAW_INPUT = 'send_raw_input'
// `read_raw_since` folds the old `pump` + `read_output` tick into one
// call that returns raw PTY bytes past a monotonic cursor. xterm parses
// ANSI natively, so feeding bytes verbatim is both simpler and — for
// interactive shells — correct: the line-buffered `read_output` path
// never surfaces pre-Enter keystrokes because they sit in
// LineBuffer.pending until a newline arrives.
const CMD_READ_RAW_SINCE = 'read_raw_since'

// Output is pull-model in the current kernel contract — no event
// topic is published for new output yet. 120ms is the smallest
// interval that still feels "live" without pinning a core.
const POLL_INTERVAL_MS = 120
// PTY-read deadline folded into each `read_raw_since` call. Kept well
// below POLL_INTERVAL_MS so each tick releases the server Mutex long
// enough for concurrent send_raw_input calls (typing) to acquire it —
// std::sync::Mutex is unfair and would otherwise starve input under a
// tighter schedule.
const PUMP_TIMEOUT_MS = 30

interface TerminalViewProps {
  kernel: KernelAPI
  events: EventsAPI
}

/**
 * Backend `ReadRawSinceResponse`. `cursor` is a u64 on the server; over
 * JSON-IPC we accept it as either a number (values below 2^53 — the
 * usual case for a single session's lifetime) or a string (serde's
 * escape hatch for larger values).
 */
interface ReadRawSinceResponse {
  cursor: number | string
  /** Raw bytes as a JSON number array (serde Vec<u8> over IPC). */
  data: number[]
}

/**
 * Resolve a CSS custom property against the document root to a plain
 * colour string — xterm renders to canvas and reads theme values
 * literally, so `var(--fg)` would show up as an empty string inside
 * its internal colour parser. Falls back to `fallback` when the
 * property is unset (covers boot-before-tokens race) or resolves to
 * an empty string.
 */
function readCssVar(name: string, fallback: string): string {
  const raw = getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim()
  return raw.length > 0 ? raw : fallback
}

export function TerminalView({ kernel, events }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    // ── Theme: resolve CSS tokens against the root element. xterm
    // needs concrete values because the viewport is a canvas, not a
    // DOM tree that participates in CSS variable cascade.
    const theme = {
      // Transparent so panel background (--bg-raised via CSS) wins —
      // xterm blends ANSI bg colours on top of this.
      background: '#00000000',
      foreground: readCssVar('--fg', '#e6e6e6'),
      cursor: readCssVar('--accent', '#7aa2f7'),
      cursorAccent: readCssVar('--bg-raised', '#1a1a1a'),
      selectionBackground: readCssVar('--accent-soft', '#3a3a5a'),
    }

    const fontFamily =
      readCssVar('--f-mono', 'ui-monospace, SFMono-Regular, Menlo, monospace')

    const term = new Terminal({
      theme,
      fontFamily,
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: false,
      convertEol: false,
      scrollback: 5000,
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(container)
    term.focus()
    const focusTerm = () => {
      term.textarea?.focus()
    }
    container.addEventListener('click', focusTerm)
    container.addEventListener('pointerdown', focusTerm)
    try {
      fit.fit()
    } catch {
      // Container may not be laid out yet on first render; the
      // ResizeObserver below will retry on the next frame.
    }

    // Monotonic byte offset of the last PTY byte we've written into
    // xterm. Advanced by each tick's response cursor. Number (not
    // BigInt) is sufficient for any realistic session lifetime —
    // 2^53 bytes is ~9 PB. The backend returns either number or
    // string, so we normalise at read time.
    let cursor = 0
    let disposed = false
    let pollTimer: number | null = null
    let lastSessionId: string | null = null

    /**
     * One poll tick: pump-and-read via `read_raw_since`, then feed
     * raw bytes straight into xterm. xterm parses ANSI / cursor
     * motion natively; we never need to synthesise newlines because
     * the shell's own output (including echoed keystrokes) carries
     * them when appropriate.
     */
    const tick = async () => {
      if (disposed) return
      const id = useTerminalStore.getState().sessionId
      if (!id) return
      // Session changed out from under us (workspace switch). Reset
      // cursor + clear xterm so we start fresh.
      if (id !== lastSessionId) {
        cursor = 0
        lastSessionId = id
        term.reset()
      }
      let resp: ReadRawSinceResponse
      try {
        resp = await kernel.invoke<ReadRawSinceResponse>(
          PLUGIN_ID,
          CMD_READ_RAW_SINCE,
          { id, cursor, timeout_ms: PUMP_TIMEOUT_MS },
        )
      } catch {
        // PTY may be closed mid-tick (workspace close race). Swallow;
        // the outer close handler will clear the session id.
        return
      }
      // Cursor arrives as number or string depending on the IPC
      // encoder; Number() handles both (string → parse, number →
      // identity). NaN would reset us to 0, which is safe.
      const nextCursor = Number(resp.cursor)
      cursor = Number.isFinite(nextCursor) ? nextCursor : cursor
      if (resp.data.length === 0) return
      term.write(new Uint8Array(resp.data))
    }

    pollTimer = window.setInterval(() => {
      void tick()
    }, POLL_INTERVAL_MS)

    // ── Input: keystrokes go straight to the PTY via send_raw_input
    // so xterm-generated control sequences (arrow keys, Ctrl-C,
    // tab-completion) reach the shell verbatim. send_input appends a
    // newline which would be wrong for arbitrary keystrokes.
    const onDataSub = term.onData((data) => {
      const id = useTerminalStore.getState().sessionId
      if (!id) return
      const bytes = Array.from(new TextEncoder().encode(data))
      void kernel
        .invoke(PLUGIN_ID, CMD_SEND_RAW_INPUT, { id, data: bytes })
        .catch(() => {
          // PTY closed — ignore. Session lifecycle is driven by the
          // workspace open/close events in index.ts.
        })
    })

    // ── Resize: refit whenever the container changes, then tell the
    // PTY. NOTE: the kernel does not yet expose a resize handler (see
    // crates/nexus-terminal/src/core_plugin.rs — handler ids 1-15, no
    // resize). We still call fit() so xterm reflows its own grid; the
    // PTY keeps its initial 80×24 until a resize handler lands. This
    // is visible as wrap weirdness on very wide panels and should be
    // revisited when the kernel surface grows.
    const resizeObs = new ResizeObserver(() => {
      try {
        fit.fit()
      } catch {
        // Size wasn't ready yet; next observation will retry.
      }
    })
    resizeObs.observe(container)

    // ── Focus command support: focus the embedded xterm when the
    // plugin fires nexus.terminal:focus (emitted by the focus
    // command in index.ts).
    const offFocus = events.on('nexus.terminal:focus', () => {
      term.focus()
    })

    return () => {
      disposed = true
      if (pollTimer !== null) {
        window.clearInterval(pollTimer)
      }
      container.removeEventListener('click', focusTerm)
      container.removeEventListener('pointerdown', focusTerm)
      try {
        onDataSub.dispose()
      } catch {}
      try {
        resizeObs.disconnect()
      } catch {}
      try {
        offFocus()
      } catch {}
      try {
        term.dispose()
      } catch {}
    }
    // Plugin api refs are stable for the life of the app — safe to
    // hold across renders without re-running the effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return <div ref={containerRef} className="nexus-terminal-root" />
}
