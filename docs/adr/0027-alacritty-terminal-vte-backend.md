# ADR 0027: alacritty-terminal as VTE Backend for nexus-terminal

- **Status:** Accepted
- **Date:** 2026-05-05
- **Deciders:** Engineering
- **Context for:** ADR 0026 (gpui shell migration); AI-terminal first-class integration

## Context

`nexus-terminal` currently provides PTY management via `portable-pty` and a
hand-rolled ANSI processor in `src/ansi.rs`. The ANSI processor strips escape
sequences to produce plain text for `LineBuffer` but does not maintain any
terminal state: no cursor position, no character attributes (bold, colour,
underline), no screen grid, no alternate screen, no OSC 133 semantic zones.

This has two consequences:

**For the gpui migration (ADR 0026):** xterm.js is the current VTE engine. It
runs inside the WebView and is the only component that produces a rendered
terminal screen. Removing the WebView requires a Rust-side VTE replacement that
can drive a gpui cell renderer.

**For AI-terminal integration:** `nexus-agent` calls `read_output` and receives
ANSI-stripped text. It cannot distinguish the shell prompt from command output,
read character colours that encode semantics (red = error, green = success),
determine where the cursor is, or know whether the terminal is in the alternate
screen (e.g. `vim` is open). Every AI context-injection call works around
these gaps rather than using them.

**`alacritty-terminal`** (MIT, crates.io, `github.com/alacritty/alacritty`) is
the production-proven Rust VTE + terminal grid library used by Alacritty and
Zed. It provides:

- `alacritty_terminal::Term<L>` — the complete VTE state machine parameterised
  by an event listener `L: EventListener`.
- Full grid model: cells with `char`, foreground/background colour (named,
  indexed, RGB), bold, italic, underline, strikethrough, and hyperlink.
- Scrollback buffer with configurable history limit.
- Cursor tracking, alternate screen, saved cursor (DECSC/DECRC).
- Mouse reporting modes, bracketed paste, focus events.
- `alacritty_terminal::vte` — the underlying Paul Williams ANSI state machine
  (Apache-2.0 / MIT).

`libghostty-rs` (Ghostty's VT engine, MIT) was also evaluated. It offers
comparable capabilities but requires Zig toolchain to build the underlying C
library. `alacritty-terminal` is pure Rust and crates.io-published, making it
the lower-friction choice.

`warp_terminal` (Warp's terminal model, AGPL v3) cannot be used without
copyleft obligations.

## Decision

1. **Add `alacritty-terminal` and `vte` to `crates/nexus-terminal/Cargo.toml`.**

2. **Replace `src/ansi.rs`** with a `vte::Perform` implementation
   (`src/perform.rs`) that feeds parsed events into
   `alacritty_terminal::Term<NexusEventListener>`. `NexusEventListener`
   implements `alacritty_terminal::event::EventListener` and forwards terminal
   events (title changes, bell, clipboard, etc.) onto the kernel event bus.

3. **Replace `LineBuffer` (`src/lines.rs`)** with
   `alacritty_terminal::Term` as the authoritative grid per session. The `Term`
   owns scrollback, cursor state, character attributes, and screen mode.
   `LineBuffer` is removed; ANSI-stripped text search uses
   `Term::renderable_content()` instead.

4. **Keep `OutputBuffer` (`src/buffer.rs`) unchanged** during the transition
   period. Raw bytes continue to stream to the Tauri/xterm.js shell via
   `com.nexus.terminal.output.<id>` events while ADR 0026 Phase 2 is in
   progress. After the gpui shell becomes the default, `OutputBuffer` can be
   removed in a follow-up.

5. **Parse OSC 133 shell integration markers** in the `vte::Perform` impl.
   Markers `A` (prompt start), `B` (command start), `C` (output start), and
   `D;exit_code` (command end) are recorded as `SemanticZone` entries on
   the session. This is the foundation for command-block semantics should
   that feature be pursued, and for accurate AI context extraction (AI sees
   where the prompt ends and command output begins).

6. **Add IPC handler 18 `read_screen`** — returns the visible screen as
   `Vec<ScreenRow>` where each row is `Vec<ScreenCell { text, fg, bg, flags }>`.
   This is what the gpui `TerminalView` widget renders and what `nexus-agent`
   queries for structured context.

7. **Add IPC handler 19 `read_screen_text`** — returns the visible screen as
   a plain `String` (cells concatenated, rows separated by `\n`). Cheaper for
   AI context injection when attributes are not needed.

8. **All 17 existing IPC handlers (IDs 1–17) are unchanged.** The `read_output`
   (handler 6), `search_output` (handler 7), `wait_for_pattern` (handler 8),
   and `read_raw_since` (handler 16) paths are updated internally to use
   `Term::renderable_content()` rather than `LineBuffer` but their wire
   formats are identical.

### Dependency additions

```toml
# crates/nexus-terminal/Cargo.toml
alacritty-terminal = "0.24"
vte = "0.15"
```

`portable-pty` is retained for PTY allocation and cross-platform child
spawning. The signal escalation ladder (`libc::kill`, Windows Job Objects)
is unchanged.

## Consequences

### Positive

- `nexus-agent` can call `read_screen_text` to read current terminal state
  in Rust without any JavaScript roundtrip.
- Pattern matching (`wait_for_pattern`) works against rendered cell text
  including prompt/output zone awareness via OSC 133 zones.
- `read_screen` gives the gpui `TerminalView` structured cell data to render,
  replacing the raw-bytes-to-xterm.js pipeline.
- The hand-rolled `src/ansi.rs` ANSI stripper is deleted. Parsing bugs,
  missing sequences, and incomplete SGR handling are replaced by the
  alacritty-maintained `vte` state machine.
- `alacritty-terminal` is not async-native (same contract as `portable-pty`
  today). Callers continue to wrap blocking methods in
  `tokio::task::spawn_blocking`. No runtime model change.

### Negative / accepted trade-offs

- `alacritty-terminal` pre-1.0. Pin to a specific version; upgrade when
  the gpui shell is the primary consumer so xterm.js compatibility is no
  longer a concern.
- `Term` holds a full screen grid per session (default 80×24 cells + scrollback
  history). Memory footprint per session increases from the current ring buffer
  model. The existing 50-session cap and 10 MB `OutputBuffer` limit remain;
  `Term` scrollback history defaults to 10,000 lines (matching `LineBuffer`'s
  current cap).
- Transition period: both `OutputBuffer` (raw bytes → xterm.js) and `Term`
  (grid → gpui renderer / AI) are maintained simultaneously until the Tauri
  shell is retired. This is intentional and bounded.

## Alternatives considered

**`vte` crate alone (no `alacritty-terminal`).** The `vte` crate is a correct
ANSI parser but provides no grid model. Maintaining a custom grid on top of
`vte::Perform` would recreate a subset of `alacritty-terminal` without its
test coverage or community maintenance. Rejected in favour of the complete
library.

**`libghostty-rs`.** MIT, excellent VTE coverage, designed for embedding.
Requires Zig toolchain to build `libghostty-vt` (the underlying C library).
`alacritty-terminal` is pure Rust, crates.io-published, and already used by
Zed in the same architectural pattern (Zed's reference implementation is
directly applicable). Deferred: revisit if `libghostty-rs` goes pure-Rust.

**Keep `src/ansi.rs` permanently.** The current stripper handles ~70% of
real-world sequences. Missing SGR attributes, alternate screen, OSC, and
cursor tracking are permanent gaps. With AI and terminal as first-class
citizens this ceiling is too low.

## Cross-references

- [ADR 0026](0026-adopt-gpui-desktop-shell.md) — gpui shell migration that
  requires this change
- [ADR 0028](0028-rust-native-contribution-api.md) — contribution API changes
- `crates/nexus-terminal/src/ansi.rs` — file being replaced
- `crates/nexus-terminal/src/lines.rs` — `LineBuffer` being replaced
- `crates/nexus-terminal/src/core_plugin.rs` — IPC handlers 1–17 (unchanged),
  18–19 (new)
- Zed reference: `crates/terminal/` and `crates/terminal_view/` in
  `github.com/zed-industries/zed`
