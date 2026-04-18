// React terminal panel backed by `com.nexus.terminal` core-plugin
// dispatch (PRD-09 §14). Mirrors the `nexus-tui` terminal panel:
//
// - Spawns a session the first time the panel mounts; reuses it on
//   subsequent renders so scrollback survives tab switches.
// - Polls `term_pump` on a short interval while the panel is visible
//   to surface new PTY output.
// - Line-buffers user input client-side. Enter flushes the buffer
//   through `term_send_input`; Ctrl+C sends raw 0x03 (SIGINT to the
//   process group); Ctrl+D closes the session.
//
// ANSI colour rendering and xterm-style cursor handling are out of
// scope for this slice — `OutputLine.content` is ANSI-stripped, which
// is the right default for a shell-style prompt view. Full terminfo
// pass-through can layer on via `xterm.js` in a later slice.

import type { CSSProperties, KeyboardEvent } from "react";
import { useCallback, useEffect, useRef, useState } from "react";

import {
  termCloseSession,
  termCreateSession,
  termPump,
  termReadOutput,
  termSendInput,
  termSendRawInput,
  type OutputLine,
} from "../../ipc/terminal";
import { parseAnsiLine, type AnsiSpan, type AnsiStyle } from "../../util/ansi";

/** Cadence at which we pump the PTY while the panel is visible. */
const PUMP_INTERVAL_MS = 120;

/** Per-pump PTY read deadline. Short so an idle session doesn't
 *  stall the next render cycle. */
const PUMP_READ_TIMEOUT_MS = 50;

type State =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "ready"; sessionId: string }
  | { kind: "error"; message: string }
  | { kind: "closed" };

function styleToCss(style: AnsiStyle): CSSProperties {
  const effectiveFg = style.inverse ? style.bg : style.fg;
  const effectiveBg = style.inverse ? style.fg : style.bg;
  const css: CSSProperties = {};
  if (effectiveFg) css.color = effectiveFg;
  if (effectiveBg) css.backgroundColor = effectiveBg;
  if (style.bold) css.fontWeight = 700;
  if (style.italic) css.fontStyle = "italic";
  if (style.dim) css.opacity = 0.7;
  const decorations: string[] = [];
  if (style.underline) decorations.push("underline");
  if (style.strike) decorations.push("line-through");
  if (decorations.length > 0) css.textDecoration = decorations.join(" ");
  return css;
}

function AnsiLine({ line }: { line: OutputLine }) {
  // Fast path: no ESC in the raw bytes means plain text — avoid the
  // parser overhead and render straight from the stripped content the
  // backend already computed.
  const hasEscape = line.raw.some((b) => b === 0x1b);
  if (!hasEscape) {
    return <div className="terminal-line">{line.content}</div>;
  }
  const spans: AnsiSpan[] = parseAnsiLine(line.raw);
  return (
    <div className="terminal-line">
      {spans.map((span, i) => (
        <span key={i} style={styleToCss(span.style)}>
          {span.text}
        </span>
      ))}
    </div>
  );
}

export function TerminalPanel() {
  const [state, setState] = useState<State>({ kind: "idle" });
  const [lines, setLines] = useState<OutputLine[]>([]);
  const [input, setInput] = useState("");
  const outputRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Spawn a session on mount. Store the id in ref as well as state
  // so cleanup can see it even if React unmounts before the setter
  // commits — the state reducer can be lossy under rapid mount/
  // unmount, but the ref is always up-to-date.
  const sessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setState({ kind: "starting" });
    termCreateSession({ name: "nexus-desktop" }).then(
      (id) => {
        if (cancelled) {
          // We created a session after unmount — tear it down so the
          // PTY doesn't leak.
          void termCloseSession(id);
          return;
        }
        sessionIdRef.current = id;
        setState({ kind: "ready", sessionId: id });
      },
      (err) => {
        if (!cancelled) {
          setState({ kind: "error", message: String(err) });
        }
      },
    );
    return () => {
      cancelled = true;
      const id = sessionIdRef.current;
      if (id) {
        sessionIdRef.current = null;
        void termCloseSession(id);
      }
    };
  }, []);

  // Pump loop. Only active once the session is ready. Uses setInterval
  // rather than `requestAnimationFrame` because the shell doesn't need
  // 60 fps — a couple of polls per 100 ms is plenty for streaming
  // output.
  useEffect(() => {
    if (state.kind !== "ready") return;
    const id = state.sessionId;
    let stopped = false;

    const tick = async () => {
      if (stopped) return;
      try {
        await termPump(id, PUMP_READ_TIMEOUT_MS);
        if (stopped) return;
        const snap = await termReadOutput(id);
        if (stopped) return;
        setLines(snap);
      } catch (err) {
        if (!stopped) {
          setState({ kind: "error", message: String(err) });
        }
      }
    };

    const handle = window.setInterval(() => {
      void tick();
    }, PUMP_INTERVAL_MS);
    // Kick immediately so the shell's MOTD lands without waiting for
    // the first interval tick.
    void tick();
    return () => {
      stopped = true;
      window.clearInterval(handle);
    };
  }, [state]);

  // Auto-scroll to bottom on new output.
  useEffect(() => {
    const el = outputRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>) => {
      if (state.kind !== "ready") return;
      const id = state.sessionId;
      // Ctrl+C → raw 0x03 (SIGINT). Never triggers while the input
      // isn't focused; native copy/paste shortcuts elsewhere in the
      // shell remain intact.
      if (e.ctrlKey && e.key === "c") {
        e.preventDefault();
        void termSendRawInput(id, [0x03]);
        return;
      }
      // Ctrl+D → close session. Matches the TUI binding.
      if (e.ctrlKey && e.key === "d") {
        e.preventDefault();
        void termCloseSession(id).finally(() => {
          sessionIdRef.current = null;
          setState({ kind: "closed" });
        });
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        const cmd = input;
        setInput("");
        void termSendInput(id, cmd);
      }
    },
    [input, state],
  );

  if (state.kind === "error") {
    return (
      <div className="terminal-panel error" role="alert">
        <p className="label">Terminal error</p>
        <p className="message">{state.message}</p>
      </div>
    );
  }
  if (state.kind === "closed") {
    return (
      <div className="terminal-panel closed">
        <p className="label">Session closed.</p>
        <p className="hint">Close this tab and open a new terminal to reconnect.</p>
      </div>
    );
  }
  if (state.kind === "idle" || state.kind === "starting") {
    return (
      <div className="terminal-panel starting">
        <p className="label">Starting terminal…</p>
      </div>
    );
  }

  return (
    <div className="terminal-panel ready" onClick={() => inputRef.current?.focus()}>
      <div className="terminal-output" ref={outputRef}>
        {lines.map((line, idx) => (
          <AnsiLine key={idx} line={line} />
        ))}
      </div>
      <div className="terminal-input-row">
        <span className="terminal-prompt">$</span>
        <input
          ref={inputRef}
          className="terminal-input"
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          autoFocus
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </div>
    </div>
  );
}
