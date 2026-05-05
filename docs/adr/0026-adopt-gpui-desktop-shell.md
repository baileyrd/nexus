# ADR 0026: Adopt gpui as Desktop Shell; Retire Tauri/TypeScript Shell

- **Status:** Accepted
- **Date:** 2026-05-05
- **Deciders:** Product / Engineering
- **Context for:** Strategic direction — Developer/knowledge tool with Terminal and AI as first-class citizens
- **Supersedes:** ADR 0011 (desktop shell choice); ADR 0020 (Tauri popout architecture, see consequences)

## Context

Nexus is a developer/knowledge/markdown tool where the terminal and AI are
first-class citizens, not optional panels. That goal exposes a structural
problem in the current Tauri + TypeScript shell:

**The terminal state only exists in JavaScript.** `nexus-terminal` is a byte
pipe: PTY output lands in an `OutputBuffer`, gets ANSI-stripped into a
`LineBuffer`, and streams as raw bytes to xterm.js over IPC. xterm.js (running
inside a WebView) owns the VTE state machine, the screen grid, cursor position,
and character attributes. No Rust code can see the rendered screen. AI agents
calling `read_output` receive stripped text; they cannot see the current prompt,
distinguish command input from output, or read character attributes.

This fragmentation limits every AI-terminal integration: context injection
before LLM calls requires a JS roundtrip that does not exist, pattern
matching operates on incomplete data, and features like "explain this output"
have no reliable way to extract what the user is looking at.

**Warp's open-source release (April 2026) was evaluated.** The `warp_terminal`
crate (AGPL v3) is not usable without copyleft obligations. The `warpui` and
`warpui_core` crates (MIT) are a GPU UI framework with no terminal emulation.
Warp is itself evaluating replacing its Alacritty-derived core with
`libghostty-rs` / `alacritty-terminal`.

**gpui** (Zed Industries, MIT, `github.com/zed-industries/zed`) is the most
mature production-proven Rust GPU UI framework designed for developer tooling.
Zed uses gpui with `alacritty-terminal` for its terminal pane and native AI
integration. The codebase is fully open (MIT), well-documented by reference
implementation, and actively maintained. Key properties:

- GPU-accelerated renderer (Metal / WebGPU) via gpui's own render pipeline.
- `alacritty-terminal` provides the VTE state machine and grid model in Rust
  (see ADR 0027).
- gpui's async model (`BackgroundExecutor`) bridges with tokio cleanly.
- The Nexus microkernel IPC system is frontend-agnostic: replacing the shell
  requires no changes to any backend crate.

The pnpm/TypeScript toolchain is currently a build-time dependency. With a
Rust-native shell it is eliminated from the desktop build path entirely.

**warpui** was considered alongside gpui. It is purpose-built for terminal + AI
tooling, GPU-accelerated via wgpu, and MIT-licensed. It was not chosen because:
its companion terminal crate (`warp_terminal`) is AGPL; it was open-sourced only
in April 2026 with no external users or documentation; and `warpui`'s
Flutter-inspired entity-component-handle model is optimised for Warp's
command-block paradigm rather than Nexus's equal emphasis on knowledge/markdown.
gpui's richer text and layout foundations are a better fit when markdown is a
first-class surface.

## Decision

1. **Create `crates/nexus-gpui/`** as the new desktop frontend crate,
   structured analogously to `crates/nexus-tui/`.
2. **Add `build_gpui_runtime(forge_root: PathBuf) -> Result<Runtime>`** to
   `crates/nexus-bootstrap/src/lib.rs`, registering a `com.nexus.gpui`
   invoker plugin with `Capability::ALL`.
3. **Implement the shell in Rust using gpui**, with all UI contributions
   expressed as Rust `PaneContribution` / `StatusBarContribution` /
   `ActivityBarContribution` traits (see ADR 0028).
4. **Use `alacritty-terminal` as the VTE engine** inside `nexus-terminal`
   (see ADR 0027) so the terminal grid is accessible to Rust code directly.
5. **Use `forceatlas2` + a custom gpui canvas renderer** for the global
   knowledge graph view, replacing the hand-rolled TypeScript force simulator.
   `petgraph::StableGraph` (already in `nexus-storage`) remains the backend
   data structure unchanged.
6. **Retire `shell/`** (Tauri app + TypeScript) once the gpui shell reaches
   feature parity. The last Tauri shell commit will be tagged for recovery.
   `crates/nexus-cli` and `crates/nexus-tui` are unaffected.
7. **Retire `packages/nexus-extension-api/`** (TypeScript plugin API). A
   tombstone `CHANGELOG.md` will document the migration path (see ADR 0028).

No backend crate changes are required. The IPC system, capability system,
event bus, forge format, and all `nexus-*` service crates are unchanged.

### Phased delivery

| Phase | Scope | Gate |
|---|---|---|
| 0 | Foundations spike: gpui window boots kernel, validates tokio bridge and terminal cell rendering | Spike sign-off |
| 1 | Shell skeleton: window chrome, theme, activity bar, split-pane layout, `build_gpui_runtime` | Boots and opens a forge |
| 2 | Terminal pane: `alacritty-terminal` integration, cell renderer, full input/resize/copy | Terminal parity with xterm.js |
| 3 | Editor/markdown pane: pulldown-cmark renderer, editor, file tree | Markdown parity |
| 4 | AI pane: chat, agent panel, native terminal ↔ AI context | AI parity |
| 5 | Remaining UI contributions: 34 TypeScript plugins ported to Rust | Feature parity |
| 6 | Community plugin system: WASM headless M1 | ADR 0028 executed |
| 7 | Cutover: gpui default, shell/ retired | Tag + delete |

## Consequences

### Positive

- Terminal grid state (cursor, character attributes, screen cells) is a
  first-class Rust value accessible to `nexus-agent` and all IPC callers.
- AI context injection reads the rendered screen natively — no JS roundtrip.
- WebView / WebKit2GTK / libsoup-3.0 / WebView2 build dependencies eliminated.
- Node.js / pnpm toolchain eliminated from the desktop build path.
- Single-language codebase for the desktop application (Rust throughout).
- `nexus-cli` and `nexus-tui` continue to work; only the desktop shell changes.

### Negative / accepted trade-offs

- gpui is not yet 1.0; API stability is not guaranteed. Pin to a specific
  commit in `Cargo.toml` and upgrade deliberately.
- Community plugin UI contributions are not available in M1 (see ADR 0028).
- The entire `shell/src/plugins/nexus/` TypeScript plugin set (~34 plugins,
  ~18,000 lines) must be ported to Rust. This is the dominant delivery cost.
- Markdown rendering loses CSS flexibility (Obsidian-style theming via
  `--css-var` not directly portable). Token-mapped `gpui::Hsla` palette
  replaces it.
- ADR 0020 (Tauri popout window architecture) is superseded. gpui's
  multi-window model replaces `popout_window` / `close_popout_window`; the
  popout feature ships in Phase 5 or later as a gpui-native design.
- Font rendering, IME, dead keys, and wide-character handling must be owned
  by the application. gpui inherits Zed's `cosmic-text` stack which has
  battle-tested most of these concerns.

## Alternatives considered

**Keep Tauri + add `alacritty-terminal` to Rust backend only.** Closes the
AI-terminal gap for text content but leaves the screen grid in xterm.js.
Character attributes, cursor position, and semantic zones still require a
JS roundtrip. The JS/Rust split remains the permanent architectural posture.
Rejected as a half-measure given the stated first-class terminal goal.

**warpui (MIT).** Evaluated and declined (see Context above).

**iced.** Elm-architecture, pure Rust, wgpu-based. The `iced_term` community
widget exists but is less mature than alacritty-terminal + gpui's text system.
Zed's years of investment in gpui for rich text layout (code editor quality)
tilts the comparison decisively toward gpui for a tool where markdown is
first-class.

**egui.** Immediate-mode, easiest to learn. `egui_graphs` for knowledge graph
visualization. No production terminal widget suitable for a developer tool. The
immediate-mode model is a poor fit for a document-centric UI with complex
retained state.

## Cross-references

- [ADR 0011](0011-adopt-plugin-first-shell.md) — superseded
- [ADR 0015](0015-iframe-sandbox-plugin-runtime.md) — superseded for desktop
  shell by ADR 0028; may apply to a future web target
- [ADR 0016](0016-microkernel-native-vs-wasm-plugin-split.md) — WASM community
  plugin split remains valid; execution environment changes per ADR 0028
- [ADR 0020](0020-popout-window-architecture.md) — Tauri popout design
  superseded; gpui multi-window design is a Phase 5 work item
- [ADR 0027](0027-alacritty-terminal-vte-backend.md) — VTE engine choice
- [ADR 0028](0028-rust-native-contribution-api.md) — plugin contribution API
