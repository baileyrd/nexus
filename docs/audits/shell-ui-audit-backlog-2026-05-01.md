# Nexus Shell UI Audit — Prioritized Backlog

Date: 2026-05-01
Companion to: `shell-ui-audit-2026-05-01.md`
Ranking: severity-first (Critical → High → Medium → Low). Within a severity tier, ordered by ROI (highest first).

Severity rubric:
- **Critical**: shell broken, unusable for a class of users, or actively hostile.
- **High**: major functional/architectural defect with clear user impact.
- **Medium**: real defect with limited impact, or architectural drift that compounds.
- **Low**: polish, consistency, nice-to-have.

---

## High

### SH-001 · No React error boundary in the shell tree

**Dimension:** 03 Layout & Composition.
**Files:** `shell/src/main.tsx:54-63, 408-419`, `shell/src/shell/App.tsx:171-232`.
**Evidence:** `rg 'ErrorBoundary|componentDidCatch|getDerivedStateFromError' shell/src` returns no matches. `main.tsx::showFatal` only fires when the top-level `boot()` promise rejects — a render-time throw inside a slot contribution (an activity-bar item, status-bar item, modal) bubbles to React's default behavior and unmounts the whole tree.

**Why this is High:** the shell's defining property is that *every* visible region is a plugin contribution. A buggy community plugin's `ActivityBarView` render becomes "the whole app died". The `LeafHost` imperative DOM ownership pattern means the editor area is partially insulated, but every chrome region is exposed.

**Fix:**
1. Add `shell/src/shell/ErrorBoundary.tsx` — a class component with `getDerivedStateFromError` + `componentDidCatch` that captures `(error, info)`, calls a new `clientLogger.error()` (see SH-017), and renders a recover affordance ("Reload window", "Disable last-activated plugin", "Open log").
2. Wrap each top-level region independently in `App.tsx`: separate boundaries around `slots.activityBar`, `slots.statusBarLeft`, `slots.statusBarRight`, `slots.overlay`, and `<Workspace />`. A failure in one region should not take down the others.
3. Wrap each `LeafHost` (or each plugin's contribution within a slot) in a smaller boundary so a single misbehaving view doesn't kill its sidedock.
4. Add a test in `shell/tests/error-boundary.test.tsx` that throws from a fixture plugin's slot component and asserts the rest of the chrome still renders.

**Acceptance:** a deliberate `throw new Error('boom')` in any default-on plugin's slot contribution leaves the rest of the shell usable; the error reaches the persisted log; the user has a 1-click recovery affordance.

---

### SH-019 · Tauri capability scope grants popout windows full fs write access

**Dimension:** 12 Multi-Window.
**Files:** `shell/src-tauri/capabilities/default.json`, `shell/src-tauri/src/windows.rs`.
**Evidence:** `capabilities/default.json:4` lists `"windows": ["main", "*"]`. The same capability set therefore applies to every popout window opened via `popout_window` (`windows.rs:149-189`). The set includes `fs:allow-read-text-file`, `fs:allow-write-text-file`, `fs:allow-mkdir`, `fs:allow-remove`, `fs:allow-rename`, `fs:allow-watch`.

**Why this is High:** popouts host arbitrary leaf views, including views that may be backed by community-plugin iframes. The orchestrator sandboxes the iframe (null-origin, no `allow-same-origin`) and capability-gates the kernel IPC, but the *Tauri command surface* (`tauri-plugin-fs`) reachable from the popout webview is unscoped. A future leaf-view bug or a successful iframe escape would gain the host's fs write set. The blast radius is "any file in the user's home directory the daemon can read", which exceeds the threat model implied by the consent flow.

**Fix:**
1. Tighten `default.json` to `"windows": ["main"]`.
2. Add `shell/src-tauri/capabilities/popout.json` with identifier `"popout"`, `"windows": ["popout-*"]`, and a minimal permission set: `core:default`, `core:window:default` (so popout users can drag/resize), `core:event:allow-listen` / `allow-unlisten` (so they can subscribe to the close handshake), `dialog:allow-open` (if the popout actually needs to open file dialogs — re-evaluate), but *no* `fs:*` writes. If a popout needs file IO, route it through `kernel_invoke` so the kernel's capability check applies.
3. Add a test that asserts `popout-<id>` windows reject `fs:allow-write-text-file` calls.

**Acceptance:** a Tauri `invoke('plugin:fs|write_text_file', …)` from a popout webview returns a permission-denied error.

---

## Medium

### SH-002 · Modal layer not portaled; z-index literals scattered across 8+ values

**Dimension:** 03 Layout & Composition.
**Files (z-index sites):** `shell/src/shell/shell.css:55, 95, 1203, 2048`; `shell/src/workspace/WorkspaceRenderer.tsx:128, 329, 336, 529, 537, 869`; `shell/src/shell/ContextMenu.tsx:36`; `shell/src/plugins/nexus/launcher/LauncherView.tsx:137, 292, 321`; `shell/src/plugins/core/capabilityPrompt/CapabilityBannerView.tsx:49`; `shell/src/plugins/nexus/enrich/EnrichAcceptGate.tsx:56`; `shell/src/workspace/ForgeSelector.tsx:146`; `shell/src/plugins/core/capabilityPrompt/CapabilityModalView.tsx:90`; `shell/src/plugins/nexus/memory/CaptureOverlay.tsx:85`; `shell/src/plugins/nexus/mcp/ToolCallModal.tsx:89`; `shell/src/plugins/nexus/confirm/ConfirmModal.tsx:69`; `shell/src/plugins/nexus/bases/NewBaseDialog.tsx:95`; `shell/src/plugins/nexus/files/ContextMenu.tsx:38`.
**Evidence:** `rg 'createPortal|<Portal' shell/src` returns no matches. Distinct z-index values found: `1, 2, 10, 11, 50, 60, 65, 70, 100, 200, 900, 1000, 1080, 1100, 1200, 9000, 9500, 9999`.

**Fix:**
1. Add `shell/src/shell/ModalRoot.tsx` — a `<div id="modal-root" />` mounted as a sibling of `#root` in `index.html` (or appended in `main.tsx` before React mount).
2. Add `shell/src/shell/Modal.tsx` — wraps children in `createPortal(…, document.getElementById('modal-root')!)`.
3. Define `shell/src/shell/zIndex.ts` exporting a 6-tier scale: `chromeControls` (100), `dropdown` (200), `overlayFloating` (900), `overlayModal` (1100), `overlayToast` (1200), `overlayFatal` (9999). Migrate each modal site to use a tier name, not a literal.
4. Sweep `shell.css` for `z-index:` values and replace with `var(--z-…)` custom properties driven by the same scale.

**Acceptance:** `rg 'zIndex:|z-index:' shell/src` shows only references to the scale, not literals.

---

### SH-003 · No responsive layout logic; chrome eats content at narrow widths

**Dimension:** 03 Layout & Composition.
**Files:** `shell/index.html:181`, `shell/src/shell/shell.css`, `shell/src/workspace/WorkspaceRenderer.tsx:152-157, 670-679, 715, 754`, `shell/src-tauri/tauri.conf.json:13-23`.
**Evidence:** `rg '@media|useMediaQuery|matchMedia' shell/src` returns only the `prefers-color-scheme` match in `themeStore.ts`. Activity bar fixed at 24 px; tab strip fixed at 36 px; sidedock minimum 150 px; window minimum 600×400 (so the user can resize narrower than the natural breakpoint).

**Fix:**
1. Add a `useViewportClass()` hook driven by `ResizeObserver` on `document.documentElement`, writing `body.is-narrow`, `body.is-medium`, `body.is-wide` classes at 768 / 1280 thresholds (or `data-viewport="narrow|medium|wide"`).
2. In `shell.css`, add narrow-mode rules: collapse sidedocks below 768 px, hide right sidedock by default, expose a hamburger that toggles overlay-mode sidedocks.
3. Add a 720-px breakpoint to `defaultLayout` so a fresh boot at narrow viewport doesn't render right + bottom + left docks all expanded.
4. Audit fixed pixel sizes: 36-px tab strip should scale with `--ui-size` density.

**Acceptance:** at 800×600 (smallest practical desktop tile), all primary actions are reachable; the editor is the largest visible region.

---

### SH-005 · Modals don't trap focus; underlying chrome remains in tab order

**Dimension:** 05 Accessibility.
**Files:** `shell/src/plugins/nexus/confirm/ConfirmModal.tsx`, `shell/src/plugins/core/capabilityPrompt/CapabilityModalView.tsx`, `shell/src/plugins/nexus/mcp/ToolCallModal.tsx`, `shell/src/plugins/nexus/bases/NewBaseDialog.tsx`, `shell/src/plugins/nexus/commandPalette/CommandPalette.tsx`.
**Evidence:** `rg 'focus-trap|focusTrap|inert' shell/src` returns no matches. Modals set `aria-modal="true"` and listen for Escape, but Tab navigates out into `slots.activityBar`, `slots.statusBarLeft`, etc.

**Fix:**
1. Add `shell/src/shell/useFocusTrap.ts` — a hook that, given a ref to the modal container, captures Tab / Shift-Tab and cycles focus among the container's tabbable descendants. On open: snapshot `document.activeElement`, focus the first tabbable; on close: restore.
2. In the new `Modal.tsx` (SH-002), call `useFocusTrap(ref, isOpen)` and set `inert` on `#root` while open (or `aria-hidden="true"` for browsers without inert).
3. Test: open ConfirmModal, press Tab 20 times, assert focus stays inside the dialog.

**Acceptance:** when a modal is open, Tab cannot move focus to the activity bar / status bar / tab strip.

---

### SH-006 · App-level keydown handler `stopPropagation`s every match — risks blocking AT key forwarding

**Dimension:** 05 Accessibility.
**Files:** `shell/src/shell/App.tsx:130-146`.
**Evidence:** `App.tsx:139-141` calls `e.preventDefault(); e.stopPropagation();` whenever `reg.keybindings.match()` returns a commandId. The guard against INPUT/TEXTAREA/contenteditable is correct but doesn't account for screen-reader virtual cursor mode.

**Fix:**
1. Gate the `stopPropagation()` on a context key (`accessibilityModeOff`, default true). The settings UI exposes a toggle.
2. Better: don't `stopPropagation` at all — `preventDefault()` is enough to avoid the browser default; let the event bubble so AT can observe.
3. Add a regression test that simulates `keydown` with a registered chord and asserts the event is observable to a `document.body`-level listener.

**Acceptance:** a registered chord runs the command; a `document`-level "after" listener still receives the event.

---

### SH-009 · No code splitting; all default-on plugins ship in main bundle

**Dimension:** 07 Performance.
**Files:** `shell/src/main.tsx:45-50`, `shell/src/plugins/catalog.ts`, `shell/vite.config.ts`.
**Evidence:** `rg 'React\.lazy|Suspense|lazy\(' shell/src` returns no matches.

**Fix:**
1. Move the catalog imports to dynamic-import factories: `DEFAULT_ON_PLUGINS = [() => import('./plugins/nexus/files'), …]`.
2. In `boot()`, await `Promise.all(plugins.map(p => p()))` before passing to `host.loadAll`.
3. Configure Vite `manualChunks` to group: `vendor-react`, `vendor-codemirror`, `vendor-xterm`, plus per-plugin chunks for `nexus.bases`, `nexus.canvas`, `nexus.graph`, `nexus.terminal` (the heavy ones).
4. For DEFAULT_OFF plugins, defer the dynamic import until `enable` is clicked (matches the current "reload required" UX).

**Acceptance:** initial chunk size measurably drops; lazy plugins (nexus.bases, nexus.graph, etc.) appear as separate chunks in the build report.

---

### SH-010 · Google Fonts pulled from `googleapis.com` at boot; CSP `font-src` doesn't allow `gstatic.com`

**Dimension:** 07 Performance / 09 Cross-Platform.
**Files:** `shell/index.html:7-12`, `shell/src-tauri/tauri.conf.json:30-32`.
**Evidence:** `index.html` preconnects to `fonts.googleapis.com` and `fonts.gstatic.com` and pulls `Inter, IBM Plex Serif, JetBrains Mono`. `tauri.conf.json#app.security.csp.font-src` is `["'self'", "data:"]` — gstatic is *not* allowed. Either the prod build has been silently failing to load fonts, or some other allowance is in play.

**Fix:**
1. Self-host the three font families. Drop `Inter-{400,500,600,700}.woff2`, `IBMPlexSerif-{400,500,600}.woff2` (italic 400 if needed), `JetBrainsMono-{400,500}.woff2` into `shell/public/fonts/`.
2. Replace the `<link rel="stylesheet" href="googleapis.com/…">` with inline `@font-face` declarations (also in `index.html`'s inline `<style>` so they're available before React mounts).
3. Drop the `preconnect` lines.

**Acceptance:** offline desktop boot has full chrome typography; CSP is unchanged.

---

### SH-012 · Five overlapping CSS-token alias families create author-intent drift

**Dimension:** 08 Theming & Design Tokens.
**Files:** `shell/index.html:108-173`.
**Evidence:** `index.html` defines: Obsidian canonical (`--background-primary`, `--text-normal`, `--interactive-accent`, ...), legacy Forge (`--bg`, `--fg`, `--line`, `--accent`, `--r`, ...), VSCode-style (`--shell-bg`, `--editor-bg`, `--statusbar-bg`, ...), `--color-*` (settings-panel-only), and `--bg-primary`/`--fg-primary`/`--bg-input` (bases-only). 100+ alias definitions; new tsx files inherit whichever set their author saw first.

**Fix:**
1. Pick the Obsidian set as canonical (already the documented intent).
2. Add an ESLint rule (or a custom-script in `scripts/check_token_usage.sh`) that flags any new `--bg-`, `--fg-`, `--shell-`, `--color-`, `--bg-primary`, `--fg-on-accent` reference in source.
3. Migrate consumers in batches by directory: `shell/src/plugins/nexus/bases/*.tsx` → Obsidian names, then `shell/src/plugins/core/*.tsx`, then everything else.
4. Drop the alias block from `index.html` once `rg --bg-|--fg-|--shell-|--color-bg shell/src` is empty.

**Acceptance:** `index.html` defines only the Obsidian set + density tokens; the alias block is gone.

---

### SH-013 · Hex fallbacks in `var(--token, #1e1e1e)` mask theme-coverage gaps

**Dimension:** 08 Theming & Design Tokens.
**Files:** `shell/src/workspace/WorkspaceRenderer.tsx:172-173, 660, 871, 1086-1090`, many other tsx files.
**Evidence:** `WorkspaceRenderer.tsx:660` uses `'var(--tab-container-background, var(--bg-soft, #2d2d2d))'`. If the kernel theme fails to hydrate or a theme is missing this token, the user sees `#2d2d2d` rather than a token-error.

**Fix:**
1. Ship a dev-only theme variant `theme-debug-magenta` whose every token resolves to `#ff00ff` so missing-token fallbacks become visible during QA.
2. In production, drop the hex fallbacks (let `var(--token)` resolve to the empty string and inherit, surfacing the gap as "missing color").
3. Audit kernel `--nx-*` cascade for completeness against the consumer-token list in `index.html:30-105`.

**Acceptance:** running with `theme-debug-magenta` highlights every site that's missing a kernel token.

---

### SH-015 · Frameless titlebar logic is platform-fragile

**Dimension:** 09 Cross-Platform Parity.
**Files:** `shell/src-tauri/tauri.conf.json:13-23`, `shell/src/shell/WindowControls.tsx`, `shell/src/host/bodyClasses.ts`.
**Evidence:** `tauri.conf.json#windows[0].decorations: false`. `WorkspaceRenderer.tsx:120-133` positions WindowControls at `top:0; right:0`. macOS expects traffic-light buttons in the top-left.

**Fix:**
1. Read `bodyClasses.ts` (referenced as installed before React mounts) — confirm it sets `body.mod-macos | mod-windows | mod-linux` based on `getCurrent().platform()` or an equivalent.
2. In `WorkspaceRenderer.tsx`, wrap the `<WindowControls />` placement in a platform branch: macOS → `top:0; left:0` with traffic-light style; Windows/Linux → `top:0; right:0` current behavior.
3. macOS-specific affordance: respect the system "show window proxy / file path" gesture by writing a meaningful `<title>` that reflects the active leaf.
4. Test on each OS — the static audit cannot.

**Acceptance:** macOS users see the OS-style controls in the top-left; Windows users see Win11-style controls in the top-right.

---

### SH-017 · 241 `console.*` calls across 69 files; no centralized client logger

**Dimension:** 11 Observability.
**Files:** spread across `shell/src/`. Densest: `main.tsx` (16), `host/ExtensionHost.ts` (7), `host/PluginRegistry.ts` (3), `plugins/nexus/launcher/index.ts` (2), `plugins/nexus/recall/RecallOverlay.tsx` (8).
**Evidence:** `rg 'console\.' shell/src` returns 241 matches.

**Fix:**
1. Add `shell/src/host/clientLogger.ts` — a structured logger with `error/warn/info/debug` methods, in-memory ring buffer (last N entries), forwarding to `console.*` AND emitting a `nexus:log` Tauri event so the Rust side can write to `~/.nexus-shell/logs/shell.log`.
2. Add a Tauri command `append_shell_log(entries: Vec<LogEntry>)` that batches + atomic-writes through `tracing`.
3. Sweep replace: `console.error(…)` → `clientLogger.error(…)` etc.; ESLint rule `no-console` to keep new offenders out.
4. Surface the ring buffer in the settings panel under a "Diagnostics" tab.

**Acceptance:** `rg 'console\.' shell/src` returns near-zero matches; `~/.nexus-shell/logs/shell.log` accumulates structured records across runs.

---

### SH-018 · No error → backend pipe; in-render throws vanish on reload

**Dimension:** 11 Observability.
**Files:** `shell/src/host/ExtensionHost.ts:160-168, 293-298`, `shell/src/main.tsx:54-63`.
**Evidence:** `ExtensionHost.fail()` writes to `errors` map and prints; `showFatal` only fires for the top-level `boot()` rejection. There's no `window.onerror` or `window.onunhandledrejection` handler.

**Fix:**
1. In `main.tsx`, register `window.onerror` and `window.onunhandledrejection` and forward through `clientLogger.error(...)` (SH-017).
2. The new `<ErrorBoundary>` (SH-001) reports caught errors through the same channel.
3. The settings/diagnostics tab shows the buffered errors with a "Copy log to clipboard" button.

**Acceptance:** a deliberate `Promise.reject(new Error('test'))` from a plugin lands in the diagnostics buffer and the persisted log file.

---

### SH-020 · Popout boots a full plugin set; not all plugins contribute to popouts

**Dimension:** 12 Multi-Window.
**Files:** `shell/src/main.tsx:206-209`, `shell/src/plugins/catalog.ts`.
**Evidence:** `main.tsx:206-209` skips community-plugin scan + sandbox + consent in popout mode but still calls `host.loadAll(DEFAULT_ON_PLUGINS)`. Plugins like `nexus.workspace`, `nexus.activityBar`, `nexus.statusBar` contribute to chrome that popouts don't render.

**Fix:**
1. Add a `popoutCompatible: boolean` field to plugin manifests; default true.
2. In popout mode, filter `DEFAULT_ON_PLUGINS` to those marked `popoutCompatible: true` (the view-creator plugins: editor, files, search, outline, …) and skip the chrome-only ones.
3. Update ADR 0020 §1 to document the allowlist.
4. Measure: popout boot time should drop noticeably.

**Acceptance:** popout window boot time decreases; the same leaf still renders correctly.

---

## Low

### SH-004 · Density mode is font-size-only; chrome dimensions don't scale

**Dimension:** 03 Layout & Composition.
**Files:** `shell/index.html:175-178`, `shell/src/workspace/WorkspaceRenderer.tsx:154-157, 670, 715, 754, 1082-1083`.

**Fix:** define spacing tokens that depend on density (`--chrome-icon-size`, `--chrome-row-height`) and migrate hardcoded `width: 36`, `width: 24`, `height: 36`, etc. to CSS variables.

---

### SH-007 · No `prefers-reduced-motion` support

**Dimension:** 05 Accessibility.
**Files:** `shell/src/shell/shell.css`, future animation sites.

**Fix:** add a single `@media (prefers-reduced-motion: reduce)` block to `shell.css` that zeroes transition / animation durations. Also: write transitions through a `--motion-duration` token that respects the media query.

---

### SH-008 · Sidebar TabButton fallback dot loses information

**Dimension:** 05 Accessibility.
**Files:** `shell/src/workspace/WorkspaceRenderer.tsx:1100-1115`.

**Fix:** the dot is fine for sighted users with no AT; when the activity bar has spare space, render a 2-letter short-name instead of the dot for unmapped viewTypes. AT users already have aria-label.

---

### SH-011 · App.tsx 500ms debug logger fires on every slot change

**Dimension:** 07 Performance.
**Files:** `shell/src/shell/App.tsx:33-48`.

**Fix:** gate on `import.meta.env.DEV`.

---

### SH-014 · `--ui-size` density decoupled from kernel theme

**Dimension:** 08 Theming.
**Files:** `shell/index.html:175-178`.

**Fix:** move density tokens into the kernel theme cascade (`--nx-density-cozy/-compact/-spacious` shipped per theme); `index.html` reads them.

---

### SH-016 · Vite `build.target` only branches on Windows

**Dimension:** 09 Cross-Platform.
**Files:** `shell/vite.config.ts:39-43`.

**Fix:** branch on macOS (`safari14` if min-supported is current Sonoma) and Linux (WebKit2GTK 2.40+ supports much of ES2022) explicitly, and verify build output on each OS in CI.

---

### SH-021 · Popout cannot persist its own bounds back to workspace.json

**Dimension:** 12/13 Multi-Window/Persistence.
**Files:** `shell/src/shell/PopoutShell.tsx`, `shell/src/workspace/popoutWindowBridge.ts`, `shell/src/workspace/persistence.ts`.

**Fix:** if a popout drag/resize should survive restart, the popout emits `nexus:popout-bounds-changed { fwId, bounds }` and the main window updates its `floating[]` entry + persists. If by design the bounds reset on restart, document it in ADR 0020.

---

## Skipped / Documented Non-Issues

- **No URL router** — by design; deep links via `UriHandlerRegistry`, navigation via `Leaf` + `viewRegistry`. Pass.
- **In-place tree mutation in workspaceStore** — documented at `WorkspaceRenderer.tsx:38-47`; not a bug. Pass.
- **`stringly-typed reg.registerService(...)`** — minor; Pass.
- **Console fallback hex colors in editor decorations** — out of scope (feature, not shell).

---

## Notes for the Next Iteration

A live-runtime second pass would add value on:
- **05 Accessibility**: actually screen-reader the chrome on macOS VoiceOver and Windows Narrator.
- **09 Cross-Platform**: launch on Windows 11, macOS 14, Linux GNOME / KDE; capture window-control alignment.
- **12 Multi-Window**: kill -9 a popout while the main window has unsaved layout edits; verify reconcile.
- **07 Performance**: measure boot time delta after SH-009; measure popout boot time.
- **03 Layout**: actually resize the window to 600 × 400 (tauri.conf.json minimum) and screenshot.

Track the SH-* IDs through implementation. Each PR closing one should reference the audit report explicitly.
