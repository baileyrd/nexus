# RFC 0003 — `rusty_term`: selectively adopt the VT core + agent-introspection design

- **Status:** Accepted — Tracks A + B landed (`nexus-vt` grid; OSC 133 capture + shell-integration emitters; terminal-as-MCP-resource with change notifications). Open: bundled rush emitting OSC 133 itself; making the server grid authoritative for xterm.js.
- **Owner:** unassigned
- **Created:** 2026-06-17
- **Tracks:** OS-sandbox / AgenticSandbox vision, omp agentic loop, `nexus-terminal`
- **Touches (if accepted):** possible new headless VT crate (`nexus-vt` / vendored `rusty_term` core), `crates/nexus-terminal/` (OSC 133 + grid snapshot handlers), `crates/nexus-mcp/` (terminal-as-resource surface), shell-integration scripts
- **Related:** [RFC 0002 — bundled shell (`rush`)](0002-bundled-shell-rush.md)

---

## Summary

[`baileyrd/rusty_term`](https://github.com/baileyrd/rusty_term) is a complete,
from-scratch **terminal emulator** in Rust: a VT/ANSI parser + grid model, a PTY
backend (Unix openpty / Windows ConPTY), a tokio I/O loop, a native `winit`
window backend (CPU `softbuffer` + GPU `wgpu` renderers, fonts, ligatures,
images), a TUI passthrough mode, and — behind the `l13` feature — a private-OSC
JSON-RPC side channel that exposes the terminal to agents over **MCP**.

This RFC concludes: **do not vendor it wholesale, but selectively adopt two
pieces.** Wholesale adoption conflicts with the single-desktop-target rule
(ADR 0011) and duplicates plumbing `nexus-terminal` already has. The valuable,
separable parts are:

1. **The headless VT `core/` engine** (parser + grid + scrollback) — Nexus has
   **no** server-side grid model today; this would unlock structured agent
   *screen* introspection. Medium effort, real strategic value.
2. **The OSC 133 semantic-prompt + agent-introspection design** (reliable
   command/exit-code/output boundaries, screen/scrollback/cwd/cursor exposed as
   MCP resources with change notifications) — the cheapest, highest-leverage
   win, and it feeds the omp agent loop directly.

## Background

### What `rusty_term` is

A standalone terminal emulator (edition 2024, version 0.1.0) with a deliberately
small dependency surface. Its modules decompose cleanly:

- **`core/`** — the engine: `parser.rs` (VT/ANSI state machine), `grid.rs`
  (cells, scrollback, reflow), `charset.rs`, `color.rs`, `osc.rs`, plus
  `sixel.rs` / `kitty.rs` / `iterm.rs` and a from-scratch image-decode stack
  (base64 → inflate → png → jpeg). Deps: only `libc`, `parking_lot`,
  `unicode-width`, `unicode-segmentation`.
- **`backend/`** — PTY spawn: Unix `openpty` + fork/exec (`libc`), Windows
  ConPTY (`windows-sys`).
- **`runtime/`** — a single tokio async reactor.
- **`gui/`** (feature-gated) — a native `winit` window: CPU (`softbuffer`) and
  GPU (`wgpu`) renderers, `ab_glyph` rasterization, GSUB ligature shaping, mouse
  reporting, IME, image overlays.
- **`render.rs` / `input.rs`** — TUI passthrough mode (relay ANSI to a host
  terminal, tmux-like).
- **`l13`** (feature-gated) — a JSON-RPC 2.0 transport over a private OSC
  (`OSC 5379`) hosting an **MCP** server that exposes the terminal as **tools**
  (`get_screen`, `get_scrollback`, `get_cwd`, `get_title`, `get_dimensions`,
  `get_cursor`) and **resources** (`terminal://screen`, `…/scrollback`,
  `…/cursor`, `…/exit`, `…/command`) with **change notifications** and a typed
  `command_finished` carrying the exit code; plus LSP/ACP negotiation. Reuses
  the sibling `rusty_lsp` crate.

### What Nexus has today

- **Rendering** is done by **xterm.js** in the Tauri shell (`@xterm/xterm` +
  `addon-fit` + `addon-webgl`, driven by `shell/src/plugins/nexus/terminal/`).
  The VT emulation / grid lives in the **webview frontend**, not in Rust.
- **`nexus-terminal`** (`com.nexus.terminal`) owns the PTY (`portable-pty`),
  session management, a ring-buffer / line-buffer of **raw output**, ANSI
  *stripping* (`ansi.rs` — which explicitly "does **not** model full terminal
  state (cursor position, scrollback)"), URL detection, and AI suggestions.
  **There is no server-side VT grid model.**
- **Exit codes** are captured by a **sentinel hack**: `precmd.rs` wraps each
  step with `; printf '<sentinel> %d\n' $?` and scans the line, explicitly
  because "we can't read an OS-reported exit code." There is **no OSC 133**
  semantic-prompt support.

## Three-part decomposition and fit

| Piece | Fit | Why |
|---|---|---|
| **`gui/` native window** (winit/wgpu/softbuffer, fonts, ligatures, image overlays) | ✗ Don't adopt | Directly conflicts with **ADR 0011** — the Tauri shell is the *single* active desktop target. This is an alternative *product*, not a component. xterm.js already covers interactive rendering. |
| **`backend/` PTY + `runtime/` tokio loop** | ✗ Don't adopt | Overlaps `nexus-terminal`'s `portable-pty` session layer; no reason to swap a working, cross-platform abstraction for a hand-rolled one. |
| **`core/` headless VT engine** (parser + grid + scrollback + OSC + images) | ✓ Strongest *code* candidate | Nexus has **no** server-side grid model. A Rust grid unlocks structured agent screen-reads, a real model behind the TUI, and server-authoritative state for headless/remote/sandbox/collab. |
| **`l13` capability surface** (OSC 133 command lifecycle + terminal-as-MCP-resource + change notifications + typed exit code) | ✓ Highest-leverage *design* to adopt | Replaces the sentinel-printf exit-code hack with reliable OSC 133 boundaries, and gives the agent loop a clean "observe the terminal" contract. Nexus already has an MCP server + terminal IPC, so this is mostly adopting the *shape*, not vendoring the transport. |

## Why the two "adopt" pieces matter for the agent vision

The omp agentic loop and the AgenticSandbox both need the agent to **observe and
act on a terminal reliably**:

- *Observe the screen as structure.* "What's on screen right now?" needs a grid
  (rows × cells with attributes), not a raw byte ring buffer. That is exactly
  what `core/grid.rs` provides — and what xterm.js provides only inside the
  webview, where the Rust backend and headless/CLI/TUI frontends can't reach it.
- *Know when a command finished and its result.* OSC 133 semantic prompt marks
  give first-class command-start / command-end / exit-code / captured-output
  boundaries. rusty_term ships shell-integration emitters
  (`extra/shell-integration/{bash,zsh,fish,pwsh}`) and turns the exit code into
  a typed push. This is strictly better than scanning for a printf sentinel and
  is the natural trigger for an agent's next turn.

Note the in-band **transport** (`l13`'s private-OSC JSON-RPC) is *less* relevant
to Nexus: Nexus reaches agents through its own kernel IPC + `nexus-mcp` server,
not through an OSC channel embedded in the child's output stream. It's the
*capability surface* (screen/scrollback/cwd/cursor/exit as resources with
notifications) that is worth importing into `nexus-mcp` / `nexus-terminal`.

## The honest case against

- **Parallel implementation.** A server-side grid duplicates what xterm.js
  already does for the interactive case; it earns its keep only for the
  headless/agent/TUI paths. Scope it to those, or it's maintenance for no user.
- **Maturity & edition.** 0.1.0, edition 2024 (the workspace is 2021 — fine via
  per-crate editions, toolchain `1.94.1` supports it, same wrinkle as RFC 0002).
- **`l13` couples to `rusty_lsp`.** The side channel reuses the sibling crate's
  JSON-RPC/LSP types. If only the *design* is adopted, that coupling is avoided.

## Verdict

**Don't vendor `rusty_term` as a whole** — the GUI conflicts with ADR 0011 and
the PTY/runtime overlap is redundant. **Do** pursue two scoped tracks:

| Track | Work | Effort | Risk |
|---|---|---|---|
| **A. Agent terminal introspection (design-first)** | Adopt OSC 133 semantic prompts (+ ship shell-integration emitters) in `nexus-terminal`, replacing the sentinel exit-code hack; expose screen/scrollback/cwd/cursor/exit as MCP resources/tools with change notifications in `nexus-mcp` | S–M | Low — additive, no ADR conflict |
| **B. Headless VT grid engine** | Vendor/port `rusty_term`'s `core/` (GUI-free: parser + grid + scrollback + OSC) into a leaf `nexus-vt` crate; feed it raw PTY output to maintain a server-side grid for `get_screen`-style reads | M | Medium — parallel to xterm.js; scope to headless/agent/TUI |

## Recommended first step

Start with **Track A**, which is cheap, ADR-clean, and immediately useful to the
omp agent loop: wire OSC 133 command-lifecycle capture into `nexus-terminal`
(emitters + parser hook + a typed "command finished (exit code)" event) and
surface the terminal as an MCP resource. Then evaluate **Track B** once the
agent loop demonstrates concrete demand for structured screen reads — at which
point `rusty_term`'s `core/` is the reference implementation to port.

## Open questions

- **Track B build vs. vendor.** Port `core/` into a Nexus leaf crate (`nexus-vt`)
  vs. depend on a published `rusty_term` core crate (would require upstream to
  split `core/` into its own library — today it's a binary). Recommendation:
  in-tree port of the GUI-free `core/`, consistent with RFC 0002's vendoring
  stance.
- **Grid ownership.** If a server-side grid lands, decide whether it becomes the
  authoritative terminal state (with xterm.js as a pure renderer fed snapshots)
  or stays a parallel agent-only view. The former is cleaner long-term but a
  larger change to the shell terminal plugin.
- **Sequencing with RFC 0002.** A bundled `rush` (RFC 0002) + OSC 133 capture
  (Track A here) together give the sandbox a fully Nexus-owned, agent-observable
  shell+terminal stack.
