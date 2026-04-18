// Minimal ANSI SGR parser for the terminal panel (PRD-09 §3.2).
//
// Reads the `raw` byte stream of an `OutputLine` and produces a list
// of styled text spans suitable for React rendering. Handles:
//
// - SGR color attributes: 30–37 / 90–97 (foreground), 40–47 / 100–107
//   (background), 38;5;N / 48;5;N (256-colour), 38;2;R;G;B (truecolor)
// - Bold (1), dim (2), italic (3), underline (4), inverse (7),
//   strikethrough (9), and their resetters (22, 23, 24, 27, 29).
// - Full reset (0 or empty parameter string).
//
// Unknown SGR codes and non-SGR escape sequences (cursor moves, OSC,
// etc.) are skipped without corrupting the span stream. Other control
// bytes that `strip_ansi` handles (BS, CR) are resolved here too so
// rendered lines look identical to the backend-stripped `content`.

export interface AnsiStyle {
  fg?: string;
  bg?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  inverse?: boolean;
  strike?: boolean;
}

export interface AnsiSpan {
  text: string;
  style: AnsiStyle;
}

const BASIC_COLORS = [
  "#000000", // black
  "#cd3131", // red
  "#0dbc79", // green
  "#e5e510", // yellow
  "#2472c8", // blue
  "#bc3fbc", // magenta
  "#11a8cd", // cyan
  "#e5e5e5", // white
];

const BRIGHT_COLORS = [
  "#666666",
  "#f14c4c",
  "#23d18b",
  "#f5f543",
  "#3b8eea",
  "#d670d6",
  "#29b8db",
  "#ffffff",
];

function xterm256(idx: number): string {
  if (idx < 16) {
    return idx < 8 ? (BASIC_COLORS[idx] ?? "") : (BRIGHT_COLORS[idx - 8] ?? "");
  }
  if (idx >= 232) {
    const g = 8 + (idx - 232) * 10;
    return rgb(g, g, g);
  }
  const n = idx - 16;
  const r = Math.floor(n / 36);
  const gr = Math.floor((n % 36) / 6);
  const b = n % 6;
  const scale = (v: number) => (v === 0 ? 0 : 55 + v * 40);
  return rgb(scale(r), scale(gr), scale(b));
}

function rgb(r: number, g: number, b: number): string {
  const to = (v: number) => v.toString(16).padStart(2, "0");
  return `#${to(r)}${to(g)}${to(b)}`;
}

function applySgr(style: AnsiStyle, params: number[]): AnsiStyle {
  const next: AnsiStyle = { ...style };
  let i = 0;
  if (params.length === 0) return {};
  while (i < params.length) {
    const p = params[i] ?? 0;
    switch (p) {
      case 0:
        i += 1;
        // Full reset; discard every prior attribute.
        for (const k of Object.keys(next)) {
          delete (next as Record<string, unknown>)[k];
        }
        break;
      case 1:
        next.bold = true;
        i += 1;
        break;
      case 2:
        next.dim = true;
        i += 1;
        break;
      case 3:
        next.italic = true;
        i += 1;
        break;
      case 4:
        next.underline = true;
        i += 1;
        break;
      case 7:
        next.inverse = true;
        i += 1;
        break;
      case 9:
        next.strike = true;
        i += 1;
        break;
      case 22:
        next.bold = false;
        next.dim = false;
        i += 1;
        break;
      case 23:
        next.italic = false;
        i += 1;
        break;
      case 24:
        next.underline = false;
        i += 1;
        break;
      case 27:
        next.inverse = false;
        i += 1;
        break;
      case 29:
        next.strike = false;
        i += 1;
        break;
      case 38:
      case 48: {
        const target = p === 38 ? "fg" : "bg";
        const mode = params[i + 1];
        if (mode === 5) {
          const idx = params[i + 2] ?? 0;
          next[target] = xterm256(idx);
          i += 3;
        } else if (mode === 2) {
          const r = params[i + 2] ?? 0;
          const g = params[i + 3] ?? 0;
          const b = params[i + 4] ?? 0;
          next[target] = rgb(r, g, b);
          i += 5;
        } else {
          // Unknown extended mode — advance past the introducer.
          i += 2;
        }
        break;
      }
      case 39:
        delete next.fg;
        i += 1;
        break;
      case 49:
        delete next.bg;
        i += 1;
        break;
      default:
        if (p >= 30 && p <= 37) {
          next.fg = BASIC_COLORS[p - 30];
        } else if (p >= 40 && p <= 47) {
          next.bg = BASIC_COLORS[p - 40];
        } else if (p >= 90 && p <= 97) {
          next.fg = BRIGHT_COLORS[p - 90];
        } else if (p >= 100 && p <= 107) {
          next.bg = BRIGHT_COLORS[p - 100];
        }
        // Unknown SGR — ignore and advance.
        i += 1;
    }
  }
  return next;
}

const decoder = new TextDecoder("utf-8", { fatal: false });

/**
 * Parse a raw byte stream into styled spans. Matches the invariant
 * upheld by the Rust `strip_ansi` helper: cursor / OSC / other non-SGR
 * escape sequences are dropped, BS rewinds one char, CR resets the
 * line buffer, and invalid UTF-8 is replaced with U+FFFD.
 */
export function parseAnsiLine(raw: number[]): AnsiSpan[] {
  const spans: AnsiSpan[] = [];
  let style: AnsiStyle = {};
  let buf: number[] = [];

  const flush = () => {
    if (buf.length === 0) return;
    spans.push({ text: decoder.decode(new Uint8Array(buf)), style: { ...style } });
    buf = [];
  };

  const n = raw.length;
  let i = 0;
  while (i < n) {
    const b = raw[i]!;
    if (b === 0x1b) {
      // ESC-introduced sequence.
      flush();
      const nextByte = raw[i + 1];
      if (nextByte === 0x5b) {
        // CSI: ESC [ params final-byte
        let j = i + 2;
        const paramStart = j;
        while (j < n && raw[j]! >= 0x20 && raw[j]! <= 0x3f) j++;
        const final = raw[j];
        if (final === undefined) {
          i = n;
          break;
        }
        if (final === 0x6d) {
          // 'm' — SGR
          const paramStr = decoder.decode(
            new Uint8Array(raw.slice(paramStart, j)),
          );
          const params = paramStr
            .split(";")
            .map((p) => (p === "" ? 0 : Number.parseInt(p, 10)))
            .filter((v) => !Number.isNaN(v));
          style = applySgr(style, params);
        }
        i = j + 1;
      } else if (nextByte === 0x5d) {
        // OSC: ESC ] ... (BEL | ESC \)
        let j = i + 2;
        while (j < n && raw[j] !== 0x07) {
          if (raw[j] === 0x1b && raw[j + 1] === 0x5c) {
            j += 1;
            break;
          }
          j++;
        }
        i = j + 1;
      } else if (nextByte !== undefined) {
        // Two-byte escape (e.g. ESC c). Skip both.
        i += 2;
      } else {
        i = n;
      }
      continue;
    }

    if (b === 0x08) {
      // Backspace: rewind one byte in the current buffer if any.
      if (buf.length > 0) buf.pop();
      i += 1;
      continue;
    }

    if (b === 0x0d) {
      // CR without LF — reset the whole current line (progress-bar
      // reflow). Drop everything we've collected so far and clear
      // already-flushed spans for this line.
      buf = [];
      spans.length = 0;
      i += 1;
      continue;
    }

    buf.push(b);
    i += 1;
  }

  flush();
  return spans;
}
