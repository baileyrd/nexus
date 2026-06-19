import { useCallback, useEffect, useRef, useState } from 'react'
import { clientLogger } from '../../../clientLogger'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebglAddon } from '@xterm/addon-webgl'
import '@xterm/xterm/css/xterm.css'
import './terminal.css'
import type { KernelAPI, EventsAPI } from '../../../types/plugin'
import { useTerminalStore } from './terminalStore'
import { useThemeStore } from '../../../stores/themeStore'
import { configStore } from '../../../stores/configStore'
import { createUrlExtractor } from './urlExtractor'
import type { UrlMatch } from './urls'
import { UrlChips } from './UrlChips'
import { SuggestionChip } from './SuggestionChip'

/** BL-058: number of recent URLs pinned above the terminal output. */
const URL_CHIP_LIMIT = 5

/**
 * Merge a freshly-detected URL into the displayed list.
 * - Dedupes by `resolved` so the same URL appearing twice doesn't
 *   double-pin.
 * - Most recent at the right (insertion-order kept).
 * - Caps at `URL_CHIP_LIMIT` by dropping the oldest.
 */
function pushUrl(prev: UrlMatch[], next: UrlMatch): UrlMatch[] {
  const filtered = prev.filter((m) => m.resolved !== next.resolved)
  filtered.push(next)
  while (filtered.length > URL_CHIP_LIMIT) filtered.shift()
  return filtered
}

/**
 * Probe whether the active GPU is a software rasterizer (llvmpipe,
 * SwiftShader, Microsoft Basic Render Driver, generic Mesa software
 * fallback). Result is cached on first call: GPU identity doesn't
 * change for the life of a WebView.
 *
 * Used by the terminal mount path to decide whether to install
 * xterm's WebGL addon. On software GPUs (typical of WSL2 without
 * GPU passthrough) the WebGL renderer overruns its frame budget on
 * every paint and emits a `task queue exceeded allotted deadline`
 * warning; the DOM renderer is both faster and quieter there.
 */
let cachedSoftwareGpu: boolean | null = null
function isSoftwareRenderedGpu(): boolean {
  if (cachedSoftwareGpu !== null) return cachedSoftwareGpu
  try {
    const probe = document.createElement('canvas')
    const gl =
      (probe.getContext('webgl2') as WebGL2RenderingContext | null) ??
      (probe.getContext('webgl') as WebGLRenderingContext | null) ??
      (probe.getContext('experimental-webgl') as WebGLRenderingContext | null)
    if (!gl) {
      cachedSoftwareGpu = true
      return true
    }
    const dbg = gl.getExtension('WEBGL_debug_renderer_info')
    const renderer: string = dbg
      ? String(gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL))
      : String(gl.getParameter(gl.RENDERER))
    const lc = renderer.toLowerCase()
    cachedSoftwareGpu =
      lc.includes('llvmpipe') ||
      lc.includes('swiftshader') ||
      lc.includes('swrast') ||
      lc.includes('software') ||
      lc.includes('basic render') ||
      lc.includes('softpipe')
    return cachedSoftwareGpu
  } catch {
    // Any probe failure → treat as software so we err on the side of
    // the safer DOM renderer rather than a broken WebGL one.
    cachedSoftwareGpu = true
    return true
  }
}

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
// `resize` propagates xterm's grid size to the PTY so SIGWINCH reaches
// the child shell — without it, `vim` / `less` / progress bars wrap
// against the original 80×24 even after the panel is resized.
const CMD_RESIZE = 'resize'

// WI-12: output is event-driven via the
// `com.nexus.terminal.output.<session_id>` topic — bytes land in xterm
// through `useTerminalStore.handleStreamChunk` (subscription wired in
// index.ts::activate). Dropped chunks are handled by the store's
// `recoverFn` via `read_raw_since` snapshots.
//
// The kernel runs an autonomous drainer thread (see
// crates/nexus-terminal/src/core_plugin.rs::drainer_loop) that pumps
// every active session and publishes stream events without any client
// poll. The 5s tick below is now a defensive backstop: it covers a
// hypothetical drainer stall and keeps `read_raw_since` cursors fresh
// for the seq-gap recovery path. 5s is invisible cost in the steady
// state and small enough that any drop is masked before the user
// notices.
const PTY_POLL_INTERVAL_MS = 5000
const PTY_PUMP_TIMEOUT_MS = 30

interface TerminalInstanceProps {
  /** Kernel session id this xterm is bound to for its entire lifetime. */
  sessionId: string
  /**
   * Whether this instance is the foreground tab. Hidden instances stay
   * mounted (so their PTY keeps streaming into the scrollback) but are
   * `display:none`; when this flips true we refit + focus so the grid
   * matches the now-visible container.
   */
  active: boolean
  kernel: KernelAPI
  events: EventsAPI
  /**
   * BL-058: opens a URL in the user's default handler. Wired through
   * `api.platform.shell.openExternal` from `index.ts::activate` so
   * this view stays off the `@tauri-apps/*` import path the
   * plugin-import-hygiene guardrail enforces.
   */
  openExternal: (target: string) => Promise<void>
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
 * literally, so `var(--text-normal)` would show up as an empty string inside
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

/** Longest auto-derived tab title we keep — a runaway OSC title (some
 *  shells stuff the whole command line in there) shouldn't blow out the
 *  tab strip. The store trims; this caps. */
const AUTO_TITLE_MAX = 40

/**
 * Normalise a shell-emitted OSC window title into a tab label: collapse
 * internal whitespace, strip a leading shell-prompt host prefix
 * (`user@host: `) that adds no signal in a single-host app, and cap the
 * length. Returns an empty string for input that's all whitespace so the
 * store's `applyAutoTitle` no-ops on it.
 */
function cleanOscTitle(raw: string): string {
  let t = raw.replace(/\s+/g, ' ').trim()
  // `user@host: ~/path` → `~/path` — the host segment is noise here.
  const m = /^[^@\s]+@[^:\s]+:\s*(.+)$/.exec(t)
  if (m) t = m[1]
  if (t.length > AUTO_TITLE_MAX) t = `…${t.slice(t.length - AUTO_TITLE_MAX + 1)}`
  return t
}

/**
 * Derive a tab label from an OSC 7 cwd report (`file://host/abs/path`).
 * Returns the final path segment (percent-decoded), or an empty string
 * when the payload doesn't parse — the cwd fallback only fires when the
 * shell emits no window title of its own.
 */
function cwdLabelFromOsc7(data: string): string {
  try {
    // OSC 7 payload is a file URI; the path may be percent-encoded.
    const url = new URL(data)
    if (url.protocol !== 'file:') return ''
    const path = decodeURIComponent(url.pathname).replace(/\/+$/, '')
    if (path.length === 0 || path === '/') return '/'
    const base = path.slice(path.lastIndexOf('/') + 1)
    return base.length > AUTO_TITLE_MAX ? base.slice(0, AUTO_TITLE_MAX) : base
  } catch {
    return ''
  }
}

export function TerminalInstance({
  sessionId,
  active,
  kernel,
  events,
  openExternal,
}: TerminalInstanceProps) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  // BL-058: URLs surfaced from the output stream. Per-mount state so a
  // remount starts fresh.
  const [urls, setUrls] = useState<UrlMatch[]>([])
  const dismissUrls = useCallback(() => setUrls([]), [])

  // Imperative handles populated on mount; the `active` effect below
  // needs to refit/focus the live xterm without re-running the heavy
  // mount effect.
  const refitRef = useRef<(() => void) | null>(null)
  const focusRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    // ── Theme: resolve CSS tokens against the root element. xterm
    // needs concrete values because the viewport is a canvas, not a
    // DOM tree that participates in CSS variable cascade.
    const buildTheme = () => ({
      // Transparent so panel background (--background-secondary via CSS) wins —
      // xterm blends ANSI bg colours on top of this.
      background: '#00000000',
      foreground: readCssVar('--text-normal', '#e6e6e6'),
      cursor: readCssVar('--interactive-accent', '#7aa2f7'),
      cursorAccent: readCssVar('--background-secondary', '#1a1a1a'),
      selectionBackground: readCssVar('--interactive-accent-soft', '#3a3a5a'),
    })
    const theme = buildTheme()

    const fontFamily =
      readCssVar('--font-monospace', 'ui-monospace, SFMono-Regular, Menlo, monospace')

    // Display settings come from the unified Settings panel (the
    // `nexus.terminal` plugin's `configuration` schema, persisted to the
    // forge's `[settings]` bag). Read non-reactively at mount, so a change
    // applies to newly opened terminals. Guard against a non-numeric /
    // out-of-range value in the settings bag so a bad entry can't break
    // the xterm canvas.
    const fontSizeRaw = configStore.get<number>('terminal.fontSize', 13)
    const fontSize = Number.isFinite(fontSizeRaw) && fontSizeRaw > 0 ? fontSizeRaw : 13
    const scrollbackRaw = configStore.get<number>('terminal.scrollback', 5000)
    const scrollback =
      Number.isFinite(scrollbackRaw) && scrollbackRaw >= 0 ? Math.floor(scrollbackRaw) : 5000

    const term = new Terminal({
      theme,
      fontFamily,
      fontSize,
      cursorBlink: true,
      allowProposedApi: false,
      convertEol: false,
      scrollback,
    })

    // OI-20 — copy/paste keyboard chords. Convention every emulator
    // follows: Cmd+C/V on macOS, Ctrl+Shift+C/V on Linux/Windows. Plain
    // Ctrl+C must keep flowing through to the PTY as SIGINT, so we
    // never claim it. We return `false` from the custom handler to tell
    // xterm "I handled this, do not treat it as input"; returning
    // `true` would dispatch the chord to onData and the PTY would see
    // a literal ``, etc.
    const isMac = typeof navigator !== 'undefined' &&
      navigator.platform.toLowerCase().includes('mac')
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== 'keydown') return true
      const isCopyChord = isMac
        ? ev.metaKey && !ev.ctrlKey && !ev.altKey && ev.key.toLowerCase() === 'c'
        : ev.ctrlKey && ev.shiftKey && !ev.metaKey && ev.key.toLowerCase() === 'c'
      const isPasteChord = isMac
        ? ev.metaKey && !ev.ctrlKey && !ev.altKey && ev.key.toLowerCase() === 'v'
        : ev.ctrlKey && ev.shiftKey && !ev.metaKey && ev.key.toLowerCase() === 'v'
      if (isCopyChord) {
        const sel = term.getSelection()
        if (sel.length > 0) {
          // Best-effort: navigator.clipboard.writeText is available in
          // the Tauri 2 WebView from user-gesture-initiated keydowns.
          // If permissions deny it (rare), there's nothing we can do
          // from a sandboxed JS context — log so the user can see why
          // their copy didn't take.
          void navigator.clipboard.writeText(sel).catch((err) => {
            clientLogger.warn('[Terminal] clipboard write failed:', err)
          })
        }
        ev.preventDefault()
        ev.stopPropagation()
        return false
      }
      if (isPasteChord) {
        void doPasteFromClipboard()
        ev.preventDefault()
        ev.stopPropagation()
        return false
      }
      return true
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(container)
    // WebGL renderer must be loaded after open() so the canvas exists.
    // On context loss (GPU reset, tab suspended too long) dispose the
    // addon — xterm falls back to its DOM renderer automatically and
    // the next mount will re-attach a fresh WebGL context.
    //
    // Skip WebGL entirely on software-rendered GPUs. Under WSL2 the
    // host falls back to llvmpipe / dzn / Mesa software rasterization
    // (visible as `libEGL warning: failed to get driver name` and
    // `dzn is not a conformant Vulkan implementation` at boot). The
    // WebGL renderer still functions there but every frame overruns
    // its task-queue budget, which xterm-addon-webgl logs as
    // `task queue exceeded allotted deadline by Nms`. The DOM
    // renderer is faster on a software GPU and produces no warnings.
    let webgl: WebglAddon | null = null
    if (!isSoftwareRenderedGpu()) {
      webgl = new WebglAddon()
      webgl.onContextLoss(() => {
        webgl?.dispose()
        webgl = null
      })
      try {
        term.loadAddon(webgl)
      } catch {
        // No WebGL support (headless tests, ancient GPU) — fall back to
        // the default DOM renderer silently.
        webgl = null
      }
    }

    // Re-apply theme + font when the kernel theme switches. Subscribed
    // to themeStore.resolvedVariables — that field flips after every
    // hydrate, which fires both on mount and on `THEME_CHANGED_EVENT`.
    // xterm's `options.theme` and `options.fontFamily` setters trigger
    // a full canvas repaint, so the terminal repaints in lock-step
    // with the rest of the chrome.
    const unsubTheme = useThemeStore.subscribe((s, prev) => {
      if (s.resolvedVariables === prev.resolvedVariables) return
      term.options.theme = buildTheme()
      term.options.fontFamily = readCssVar(
        '--font-monospace',
        'ui-monospace, SFMono-Regular, Menlo, monospace',
      )
    })
    // ── Auto-naming (OSC window title + cwd) ────────────────────────
    //
    // Two signals feed the tab's auto-title, with OSC 0/2 preferred:
    //   • onTitleChange — fires on OSC 0/2 (`ESC ] 0;…` / `2;…`), the
    //     window-title sequence most shells emit per prompt.
    //   • OSC 7 — the cwd-report sequence (`ESC ] 7;file://…`). Used only
    //     as a fallback when the shell never sets a window title, so a
    //     bare `sh` still gets a directory-named tab.
    // The store's `applyAutoTitle` ignores both once the user has
    // manually renamed the tab (pinned), so deliberate names stick.
    let sawOscTitle = false
    const titleSub = term.onTitleChange((raw) => {
      const clean = cleanOscTitle(raw)
      if (clean.length === 0) return
      sawOscTitle = true
      useTerminalStore.getState().applyAutoTitle(sessionId, clean)
    })
    const osc7Sub = term.parser.registerOscHandler(7, (data) => {
      if (!sawOscTitle) {
        const label = cwdLabelFromOsc7(data)
        if (label.length > 0) {
          useTerminalStore.getState().applyAutoTitle(sessionId, label)
        }
      }
      // Return false so xterm still runs any built-in OSC 7 handling.
      return false
    })

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

    // Local cursor used by both the on-mount drain and the 5s
    // heartbeat. The store's `streams[id].lastCursor` is the
    // authoritative position the event-stream path has reached;
    // `tick()` syncs from it before each kernel call so the pump
    // never re-fetches bytes the stream already delivered.
    let cursor = 0
    let disposed = false
    let pollTimer: number | null = null

    // BL-058: stream-aware URL extractor. Feeds each completed line
    // through the regex set ported from `nexus-terminal/src/urls.rs`
    // and pushes detected URLs onto the React-state chip strip.
    const urlExtractor = createUrlExtractor((m) => {
      if (disposed) return
      setUrls((prev) => pushUrl(prev, m))
    })

    /** Hand bytes to xterm. The store calls this synchronously from
     *  handleStreamChunk; we keep it lightweight so the broadcast
     *  forwarder isn't held up by xterm's parser work. */
    const writeBytes = (bytes: Uint8Array) => {
      if (disposed) return
      term.write(bytes)
      urlExtractor.push(bytes)
    }

    // This instance is bound to one session for its lifetime — register
    // the sink once so stream chunks for `sessionId` route into this
    // xterm. The store drops the sink registration if a remount has
    // since installed a newer one.
    const sinkUnsub = useTerminalStore.getState().registerSink(sessionId, writeBytes)

    /**
     * One pump tick. Reads any PTY bytes the shell-side stream
     * subscriber hasn't already covered — boot backlog, plus a safety
     * net for the (rare) case where a chunk goes missing or the
     * kernel's autonomous drainer is briefly stalled.
     *
     * `cursor` is synced up from the store's `lastCursor` (advanced
     * by the event-stream path) so we never re-fetch bytes the
     * stream already delivered.
     */
    const tick = async () => {
      if (disposed) return
      const streamCursor =
        useTerminalStore.getState().streams[sessionId]?.lastCursor ?? 0
      if (streamCursor > cursor) cursor = streamCursor
      let resp: ReadRawSinceResponse
      try {
        resp = await kernel.invoke<ReadRawSinceResponse>(
          PLUGIN_ID,
          CMD_READ_RAW_SINCE,
          { id: sessionId, cursor, timeout_ms: PTY_PUMP_TIMEOUT_MS },
        )
      } catch {
        return
      }
      const nextCursor = Number(resp.cursor)
      cursor = Number.isFinite(nextCursor) ? nextCursor : cursor
      if (resp.data.length > 0) {
        term.write(new Uint8Array(resp.data))
      }
      useTerminalStore.getState().advanceCursor(sessionId, cursor)
    }

    // Drain immediately to cover anything that landed before mount,
    // then tick on the heartbeat.
    void tick()
    pollTimer = window.setInterval(() => {
      void tick()
    }, PTY_POLL_INTERVAL_MS)

    // ── Input: keystrokes go straight to the PTY via send_raw_input
    // so xterm-generated control sequences (arrow keys, Ctrl-C,
    // tab-completion) reach the shell verbatim. send_input appends a
    // newline which would be wrong for arbitrary keystrokes.
    const sendBytesToPty = (bytes: Uint8Array | number[]) => {
      const arr = Array.isArray(bytes) ? bytes : Array.from(bytes)
      void kernel
        .invoke(PLUGIN_ID, CMD_SEND_RAW_INPUT, { id: sessionId, data: arr })
        .catch(() => {
          // PTY closed — ignore. Session lifecycle is driven by the
          // tab open/close events in index.ts.
        })
    }
    const onDataSub = term.onData((data) => {
      sendBytesToPty(new TextEncoder().encode(data))
    })

    /**
     * Paste handler shared by the keyboard chord and right-click. Reads
     * the clipboard via the Web API, then forwards as raw PTY input.
     * If the running shell has bracketed-paste mode enabled
     * (`set -o paste`-equivalent in bash 4+ / zsh, surfaced via xterm's
     * `term.modes.bracketedPasteMode`), wrap the payload in
     * `\e[200~ … \e[201~` so the shell knows to treat the lump as
     * pasted text (no auto-execute on embedded newlines, no
     * abbreviation expansion).
     *
     * `navigator.clipboard.readText()` requires a user gesture and the
     * Tauri WebView's clipboard permission. If it throws — usually
     * `NotAllowedError` from a denied permission prompt — surface a
     * warning so users can install `@tauri-apps/plugin-clipboard-manager`
     * as the documented follow-up.
     */
    async function doPasteFromClipboard(): Promise<void> {
      let text: string
      try {
        text = await navigator.clipboard.readText()
      } catch (err) {
        clientLogger.warn(
          '[Terminal] clipboard read denied — install @tauri-apps/plugin-clipboard-manager if this persists:',
          err,
        )
        return
      }
      if (text.length === 0) return
      // Bracketed-paste mode is exposed under `term.modes` in xterm.js.
      // The shape isn't typed in @xterm/xterm public types as of v5, so
      // we read it through an opaque cast. Falsy fallback when the
      // accessor is missing keeps us safe on future xterm versions.
      const bracketed = (term as unknown as {
        modes?: { bracketedPasteMode?: boolean }
      }).modes?.bracketedPasteMode === true
      const enc = new TextEncoder()
      if (bracketed) {
        const start = enc.encode('\x1b[200~')
        const body = enc.encode(text)
        const end = enc.encode('\x1b[201~')
        const out = new Uint8Array(start.length + body.length + end.length)
        out.set(start, 0)
        out.set(body, start.length)
        out.set(end, start.length + body.length)
        sendBytesToPty(out)
      } else {
        sendBytesToPty(enc.encode(text))
      }
    }

    // Right-click → paste. Most terminal emulators expose paste via
    // a context menu or a middle-click; we use right-click here because
    // middle-click on Linux already pastes the X selection (which is
    // separate from CLIPBOARD and not what most users expect from a
    // GUI app). preventDefault stops the WebView's native menu.
    const onContextMenu = (ev: MouseEvent) => {
      ev.preventDefault()
      void doPasteFromClipboard()
    }
    container.addEventListener('contextmenu', onContextMenu)

    // ── Resize: refit xterm's local grid, then propagate cols/rows to
    // the PTY so the child receives SIGWINCH.
    //
    // Two constraints to satisfy together:
    //   1. FitAddon.fit() mutates xterm's child DOM (cell grid sizing).
    //      Calling it synchronously inside the ResizeObserver callback
    //      triggers another resize in the same delivery cycle — that's
    //      the "ResizeObserver loop completed with undelivered
    //      notifications" warning. Deferring to the next animation
    //      frame breaks the cycle.
    //   2. As the panel mounts, the outer container resizes in stages
    //      (initial 0×0 → flex layout settles → final size). A naive
    //      "drop further observations while a frame is pending" would
    //      lose the late stages, leaving xterm fitted to the early
    //      small size — that's the "terminal not filling the panel"
    //      bug.
    //
    // The fix: track the latest contentRect from each observation and
    // schedule a single trailing rAF that reads the live container at
    // fire time. Outer container size only changes from layout reasons
    // (parent flex, viewport resize) — fit()'s own inner DOM mutations
    // don't change the outer contentRect, so the loop is broken without
    // dropping observations.
    let lastCols = -1
    let lastRows = -1
    let lastObservedW = -1
    let lastObservedH = -1
    let pendingRaf: number | null = null
    const refit = () => {
      pendingRaf = null
      try {
        fit.fit()
      } catch {
        // Container has zero size right now (display:none ancestor,
        // detached during transition). Next observation will retry.
        return
      }
      const cols = term.cols
      const rows = term.rows
      if (cols === lastCols && rows === lastRows) return
      lastCols = cols
      lastRows = rows
      void kernel
        .invoke(PLUGIN_ID, CMD_RESIZE, { id: sessionId, cols, rows })
        .catch(() => {
          // Session may have closed between fit() and invoke; ignore.
        })
    }
    // Exposed so the `active` effect can force a refit when this tab
    // returns to the foreground (it was display:none, so the
    // ResizeObserver saw 0×0 and skipped).
    refitRef.current = () => {
      if (pendingRaf === null) {
        pendingRaf = window.requestAnimationFrame(refit)
      }
    }
    focusRef.current = focusTerm
    const resizeObs = new ResizeObserver((entries) => {
      let outerChanged = false
      for (const entry of entries) {
        const { width, height } = entry.contentRect
        if (width !== lastObservedW || height !== lastObservedH) {
          lastObservedW = width
          lastObservedH = height
          outerChanged = true
        }
      }
      if (!outerChanged) return
      if (pendingRaf === null) {
        pendingRaf = window.requestAnimationFrame(refit)
      }
    })
    resizeObs.observe(container)

    // ── Focus command support: focus the embedded xterm when the
    // plugin fires nexus.terminal:focus (emitted by the focus
    // command in index.ts). Only the active tab claims focus so a
    // background terminal can't steal the cursor.
    const offFocus = events.on('nexus.terminal:focus', () => {
      if (useTerminalStore.getState().activeSessionId === sessionId) {
        term.focus()
      }
    })

    return () => {
      disposed = true
      refitRef.current = null
      focusRef.current = null
      if (pollTimer !== null) {
        window.clearInterval(pollTimer)
      }
      container.removeEventListener('click', focusTerm)
      container.removeEventListener('pointerdown', focusTerm)
      container.removeEventListener('contextmenu', onContextMenu)
      try {
        onDataSub.dispose()
      } catch {}
      try {
        resizeObs.disconnect()
      } catch {}
      if (pendingRaf !== null) {
        window.cancelAnimationFrame(pendingRaf)
        pendingRaf = null
      }
      try {
        offFocus()
      } catch {}
      try {
        titleSub.dispose()
      } catch {}
      try {
        osc7Sub.dispose()
      } catch {}
      try {
        sinkUnsub()
      } catch {}
      try {
        unsubTheme()
      } catch {}
      try {
        webgl?.dispose()
      } catch {}
      try {
        term.dispose()
      } catch {}
    }
    // `sessionId` is stable for the life of this instance (the parent
    // keys each TerminalInstance by session id), and the plugin api
    // refs are stable for the life of the app — safe to hold across
    // renders without re-running this heavy mount effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // When this tab returns to the foreground, refit (the container went
  // from display:none → visible, so xterm's grid is stale) and focus.
  useEffect(() => {
    if (!active) return
    refitRef.current?.()
    focusRef.current?.()
  }, [active])

  return (
    <div
      style={{
        // Inactive tabs stay mounted (PTY keeps streaming into
        // scrollback) but hidden, so switching back is instant and
        // lossless. The active tab fills the panel as a flex column.
        display: active ? 'flex' : 'none',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        minHeight: 0,
        minWidth: 0,
      }}
    >
      <UrlChips urls={urls} openExternal={openExternal} onDismiss={dismissUrls} />
      <div
        ref={containerRef}
        className="nexus-terminal-root"
        style={{ flex: 1, minHeight: 0, minWidth: 0 }}
      />
      <SuggestionChip kernel={kernel} sessionId={sessionId} />
    </div>
  )
}
