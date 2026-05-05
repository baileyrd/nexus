# ADR 0028: Rust-Native Contribution API; Community Plugins Headless-First

- **Status:** Accepted
- **Date:** 2026-05-05
- **Deciders:** Engineering / Product
- **Context for:** ADR 0026 (gpui shell migration)
- **Supersedes:** ADR 0015 (iframe sandbox as community-plugin runtime) for the
  desktop shell target

## Context

The current shell plugin API has two layers:

**Native core plugins** (`crates/nexus-*/`) implement `CorePlugin` from
`nexus-plugins` and register IPC handlers. They have no UI coupling — they
publish events and respond to `ipc_call`. This layer is unaffected by the
gpui migration.

**Shell plugins** (`shell/src/plugins/nexus/`, ~34 plugins) are TypeScript
modules that consume `@nexus/extension-api` (`packages/nexus-extension-api/`).
They register contributions (views, commands, status-bar items, activity-bar
icons) through a JavaScript contribution registry, render React components in
the WebView, and reach the kernel through Tauri IPC bridge functions. This
layer is entirely WebView-dependent and cannot survive the removal of the
WebView in ADR 0026.

**Community plugins** run in an iframe sandbox (ADR 0015) inside the WebView.
They use the same `@nexus/extension-api` surface. Removing the WebView removes
the sandbox.

The central question is what replaces `@nexus/extension-api` for a gpui shell.

Three approaches were considered:

1. **Rust trait-based contribution API** — native Rust structs implement
   contribution traits; contributions register at startup.
2. **QuickJS/rquickjs embedded JS runtime** — execute TypeScript plugins inside
   a sandboxed JS engine in Rust. Warp's `node_runtime` + `warp_js` crates
   do this. Preserves @nexus/extension-api compatibility.
3. **wry WebView pane** — render community plugins in an isolated WebView
   widget inside the gpui window (wry is the WebView crate underlying Tauri).

Option 2 carries substantial complexity (JS engine, layout bridge between JS
components and gpui, maintaining TypeScript API compatibility indefinitely) and
defers the architectural clean-up. Option 3 is a viable future path for
community plugin UI but is not the right default — it reintroduces the JS/Rust
boundary that ADR 0026 is eliminating.

Community plugin adoption is currently zero (marketplace not shipped, ADR 0010
deferred signing verification). The cost of the M1 regression (headless
community plugins) is low, and the path to restoring UI contributions via wry
or a WASM UI ABI is preserved as a follow-up.

## Decision

### 1. Rust contribution traits in `crates/nexus-gpui/src/contributions/`

Define the following traits. All must be `Send + Sync + 'static`.

```rust
pub trait PaneContribution: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn icon(&self) -> Icon;
    fn render(&self, cx: &mut ViewContext<WorkbenchView>) -> AnyView;
}

pub trait StatusBarContribution: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn position(&self) -> StatusBarPosition; // Left | Right | Center
    fn render(&self, cx: &mut ViewContext<StatusBarView>) -> AnyView;
}

pub trait ActivityBarContribution: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn icon(&self) -> Icon;
    fn tooltip(&self) -> &'static str;
    fn on_activate(&self, cx: &mut WindowContext);
}

pub trait CommandContribution: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn default_keybinding(&self) -> Option<KeyBinding>;
    fn execute(&self, cx: &mut WindowContext);
}
```

A `ContributionRegistry` (owned by the root `AppState`) holds `Arc<dyn
PaneContribution>` etc. Each native Rust module registers its contributions
at application startup, before the first frame renders.

### 2. Retire `packages/nexus-extension-api/`

The `@nexus/extension-api` TypeScript package is deprecated as of this ADR.
The package directory is replaced with a `CHANGELOG.md` tombstone documenting:
- The Tauri shell version that last supported it.
- The git tag to recover it (`v0.5.0-tauri-shell` or equivalent).
- The migration path: re-implement as a Rust `PaneContribution` or (future)
  as a headless WASM community plugin.

### 3. Community plugins: headless in M1

WASM community plugins (wasmtime, ADR 0016) retain full capability in M1:

- Register IPC handlers.
- Publish and subscribe to kernel events.
- Read/write KV store.
- Call any permitted `ipc_call` target.

What they lose in M1 is **UI surface**: no `PaneContribution`, no
`StatusBarContribution`, no `ActivityBarContribution` registration. Their data
is still reachable by native Rust plugins that query them via IPC, so community
plugins can act as headless data providers whose output native panes display.

ADR 0015 (iframe sandbox) is superseded for the desktop shell. The iframe
sandbox concept may be revived if a future web or Electron target is added.

### 4. Community plugin UI — deferred paths (ADR 0029 candidates)

Two future paths are explicitly preserved and not foreclosed:

**Path A — wry WebView pane:** A `WebViewPane` contribution hosts a sandboxed
`wry::WebView` inside the gpui layout. Community plugins render HTML/React
in this pane. The existing `@nexus/extension-api` surface could be re-exposed
inside the WebView. This is the lowest-effort route to restoring rich community
plugin UI, at the cost of reintroducing a WebView dependency.

**Path B — WASM UI ABI:** Define a stable ABI that WASM plugins implement to
produce gpui-compatible UI descriptions (a retained-mode widget tree). High
complexity; not pursued in M1.

## Consequences

### Positive

- The contribution model is a plain Rust trait — no plugin activation
  lifecycle, no async registration, no JS bridge. Contributions are
  registered synchronously at startup; the shell renders immediately.
- No `@xterm/xterm`, no `react`, no `zustand`, no TypeScript build step in
  the desktop binary.
- Community plugins that are headless today (IPC handlers only) continue to
  work without modification.
- The future wry path (Path A above) is an additive change — it does not
  require undoing any decision made here.

### Negative / accepted trade-offs

- Community plugin UI contributions are not available in M1. Plugin authors
  who have built UI-heavy shell plugins must wait for Path A or Path B.
  Given zero published community plugins today (ADR 0010), the practical
  impact is limited to internal first-party plugins, all of which are being
  ported as native Rust contributions in Phase 5.
- Raising the floor for UI plugin authorship from TypeScript (web-familiar)
  to Rust increases the barrier for external contributors.
- ~34 TypeScript plugins (~18,000 lines) must be ported to Rust. This is a
  bounded cost (Phase 5 of ADR 0026), not an open-ended obligation.

## Alternatives considered

**QuickJS / rquickjs embedded JS engine.** Embeds a JavaScript runtime in Rust
and executes TypeScript plugins without a WebView. Preserves API compatibility.
Rejected because: it defers the architectural clean-up; layout bridging between
JS-rendered components and gpui is an unsolved, complex problem; maintaining
`@nexus/extension-api` compatibility indefinitely becomes a second API surface
to support; and the Warp team's experience (their `node_runtime` + `warp_js`
crates) shows this path is non-trivial even with commercial investment.

**wry WebView pane as default (not deferred).** Keeps WebKit2GTK / WebView2 as
a runtime dependency, reintroduces the JS/Rust boundary ADR 0026 removes, and
makes TypeScript plugins first-class. Rejected as the default because the
primary motivation for ADR 0026 (native terminal grid state visible to Rust /
AI) is best served by maximising the native Rust surface. Preserved as Path A
for community plugins specifically.

**Maintain both shells indefinitely.** Run Tauri and gpui shells in parallel,
with TypeScript plugins working in Tauri and Rust contributions in gpui.
Rejected: doubles the maintenance surface, defeats the coherence goal, and
leaves the AI-terminal integration gap unresolved for the TypeScript plugin set.

## Cross-references

- [ADR 0015](0015-iframe-sandbox-plugin-runtime.md) — superseded for desktop
  shell
- [ADR 0016](0016-microkernel-native-vs-wasm-plugin-split.md) — WASM community
  plugin split still valid; execution environment updated here
- [ADR 0026](0026-adopt-gpui-desktop-shell.md) — the migration that requires
  this decision
- `packages/nexus-extension-api/` — package being deprecated
- `shell/src/plugins/nexus/` — TypeScript contributions being ported (Phase 5)
- `crates/nexus-gpui/src/contributions/` — new contribution registry (to be
  created in ADR 0026 Phase 1)
