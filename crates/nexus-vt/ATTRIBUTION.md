# Attribution — `nexus-vt`

This crate is an **in-tree port** of the GUI-free core of
[`baileyrd/rusty_term`](https://github.com/baileyrd/rusty_term), a from-scratch
terminal emulator in Rust, adopted per
[RFC 0003 — `rusty_term` terminal emulator](../../docs/0.1.2/rfcs/0003-terminal-emulator-rusty-term.md)
(Track B).

## What was ported

The platform-independent VT engine, copied essentially verbatim from
`rusty_term/src/core/`:

- `parser.rs` — the VT100/ECMA-48 escape-sequence state machine
- `grid.rs` — the screen buffer: cells, cursor, scrollback, alternate screen,
  scrolling region, reflow, and OSC 133 command/exit-code tracking
- `cell.rs`, `charset.rs`, `color.rs`, `osc.rs` — the cell atom, character-set
  designations, the ANSI palette + SGR resolution, and OSC dispatch
- the leaf image decoders `base64.rs`, `inflate.rs`, `png.rs`, `jpeg.rs`,
  `kitty.rs`, `sixel.rs`, `iterm.rs`
- `tests.rs` — the engine test suite

Its only dependencies are `unicode-width` and `unicode-segmentation` (the same
two `rusty_term`'s core uses).

## What was deliberately NOT ported

- **The `winit`/`softbuffer`/`wgpu` GUI** (`src/gui/`). It conflicts with
  ADR 0011 (the Tauri shell is the single desktop target) and is an alternative
  *product*, not a component. The vendored core still carries its
  `#[cfg(feature = "gui")]` hooks; the `gui` feature is declared but never
  enabled and pulls no deps.
- **The PTY backend + tokio runtime** (`src/backend/`, `src/runtime/`).
  `nexus-terminal` already owns the PTY via `portable-pty`.
- **The L13 in-band side channel** (`src/core/channel.rs`): an OSC-5379 JSON-RPC
  transport that exposes the terminal over MCP/LSP/ACP and pulls the sibling
  `rusty_lsp` crate + `serde_json`. Nexus surfaces the terminal through its own
  kernel IPC + MCP server (RFC 0003 Track A), so `core/channel.rs` here is a
  dependency-free **no-op stub** with the same `pub(crate)` surface the core
  calls — letting `grid.rs`/`osc.rs`/`parser.rs` stay byte-identical to upstream
  while the private-OSC handling and resource-change pushes do nothing. The
  upstream channel test module (`core/tests.rs`) is kept in-tree for provenance
  but gated behind the never-enabled `channel_tests` feature.

## Nexus-specific changes

- Added headless-introspection accessors to `Grid` (`screen_text`,
  `scrollback_text`, `last_exit_code`, `last_output`) and a high-level [`Vt`]
  facade (`src/lib.rs`) over `Grid` + `AnsiParser`.
- OSC 133 command tracking — `#[cfg(feature = "l13")]` upstream, where it was
  coupled to the channel transport — is kept on via the `l13` feature, which is
  **on by default and pulls no extra crates** here (the transport is the no-op
  stub). It is the reason this crate exists.

[`Vt`]: crate::Vt
