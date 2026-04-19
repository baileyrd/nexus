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
const CMD_PUMP = 'pump'
const CMD_READ_OUTPUT = 'read_output'

// Output is pull-model in the current kernel contract — the terminal
// plugin ships only `pump` (drain the PTY into the line buffer) and
// `read_output` (return `[start, start+count)`). No event topic is
// published for new output yet. 50ms is the smallest interval that
// still feels "live" without pinning a core; the kernel's own pump
// call holds a 100ms deadline inside the PTY read, so the IPC round
// trip is the only thing we're paying for each tick.
const POLL_INTERVAL_MS = 50
// One PTY-read deadline per pump, matches the kernel's default.
const PUMP_TIMEOUT_MS = 100

interface TerminalViewProps {
  kernel: KernelAPI
  events: EventsAPI
}

interface OutputLine {
  timestamp_ms: number
  content: string
  /** Raw bytes as a JSON number array (serde Vec<u8> over IPC). */
  raw: number[]
  repeats: number
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
    try {
      fit.fit()
    } catch {
      // Container may not be laid out yet on first render; the
      // ResizeObserver below will retry on the next frame.
    }

    // Track last-read line offset. The kernel side keeps a monotonic
    // line buffer; on each tick we only write lines we haven't seen.
    let nextLineIndex = 0
    let disposed = false
    let pollTimer: number | null = null
    let lastSessionId: string | null = null

    /**
     * One poll tick: pump the PTY, read any new lines, write their
     * raw bytes into xterm. xterm handles ANSI / cursor motion
     * natively — we only need to append a newline since OutputLine's
     * raw bytes are stored without the trailing `\n`.
     */
    const tick = async () => {
      if (disposed) return
      const id = useTerminalStore.getState().sessionId
      if (!id) return
      // Session changed out from under us (workspace switch). Reset
      // the cursor so we start at the new session's line 0.
      if (id !== lastSessionId) {
        nextLineIndex = 0
        lastSessionId = id
        term.reset()
      }
      try {
        await kernel.invoke(PLUGIN_ID, CMD_PUMP, { id, timeout_ms: PUMP_TIMEOUT_MS })
      } catch {
        // PTY may be closed mid-tick (workspace close race). Swallow;
        // the outer close handler will clear the session id.
        return
      }
      let lines: OutputLine[]
      try {
        lines = await kernel.invoke<OutputLine[]>(PLUGIN_ID, CMD_READ_OUTPUT, {
          id,
          start: nextLineIndex,
        })
      } catch {
        return
      }
      if (lines.length === 0) return
      for (const line of lines) {
        // Raw bytes preserve ANSI escape sequences the shell emitted
        // (colours, cursor moves). xterm's `write` accepts Uint8Array
        // as well as strings.
        const bytes = new Uint8Array(line.raw)
        term.write(bytes)
        term.write('\r\n')
      }
      nextLineIndex += lines.length
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
