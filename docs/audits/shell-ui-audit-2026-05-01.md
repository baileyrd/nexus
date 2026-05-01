# Nexus Shell UI Architecture Audit

Date: 2026-05-01
Target: `nexus-shell` (Tauri desktop) — `\\wsl.localhost\fedoralinux-43\home\baileyrd\projects\nexus\shell`
Auditor: shell-ui-architecture-audit (static-only run; no live runtime instrumentation)
Scope: outer chrome / regions / nav / theming / extensibility / multi-window — *not* feature pages

---

## 0. Summary

Nexus ships a remarkably ambitious shell for a 0.1.0 product: a microkernel-backed Tauri desktop frame whose entire visible UI is a plugin contribution graph (everything from the activity bar to the status bar to the editor tabs is loaded by `ExtensionHost.loadAll()`), a Leaf/ViewRegistry workspace tree with persistent per-vault layout, a sandboxed iframe runtime for community plugins gated by a capability consent flow, and a multi-window popout architecture that re-uses the same plugin set under a `popoutMode` context key. The architectural vision is coherent and the registry/host contract is unusually well-tested for a project this size (90+ shell test files including sandbox protocol, popout, persistence, keybinding, URI handler).

That said, the frame around those plugins has accumulated several concrete defects that will hurt before they kill anyone:

- **No React error boundary anywhere in the shell tree** — a render-time throw in any plugin contribution takes the whole app down to the `showFatal` div in `main.tsx`. Combined with imperative plugin DOM ownership in `LeafHost`, this is the single highest-leverage thing to fix.
- **Tauri capability scope is too broad** — `windows: ["main", "*"]` in `capabilities/default.json` grants the same `fs:allow-read-text-file`/`write-text-file`/`mkdir`/`remove`/`rename` set to every popout window, which sit on the same JS context boundary as community plugins.
- **CSP-blessed `unsafe-inline` styles + the absence of `createPortal`** — every modal renders inline at z-indices ranging from `1` to `9999`, with at least eight distinct values that overlap in inconsistent ways (capability banner `900`, capability modal `1200`, confirm `1100`, capture `1100`, MCP tool-call `1000`, NewBaseDialog `1080`, ForgeSelector `9500`, command palette overlay `9999`).
- **No responsive logic at all** — zero `@media` queries in `shell/src` and no breakpoint utilities. The shell hard-codes 36-px tab strips, 24-px ribbon widths, fixed `1280×800` defaults; on a 13" laptop screen split with a meeting tile the chrome becomes the content.
- **Accessibility posture is partial** — keyboard handling is comprehensive at the registry level (KeybindingRegistry with conflict detection, `when` clause evaluation, palette navigation), but ARIA roles and focus management are uneven plugin-by-plugin and there's no global focus-trap discipline for modals.
- **Logging discipline is split** — Rust side does it right (`tracing-subscriber` + env filter + `nexus-panic-log`), TS side has 241 raw `console.*` calls across 69 files with no centralized client logger; nothing reaches the Rust log path from the renderer.

None of these are existential. The microkernel boundary, the capability gating, the popout sync handshake (ADR 0020 §3), the IPC drift script, and the deactivation soft-cap are all the marks of a project that has already thought hard about the right things. The remediation backlog is finite and well-localised — most entries touch a handful of files in `shell/src/shell/`, `shell/src/host/`, and `shell/src-tauri/`.

Overall verdict: **Warn**. Two **High** items (error boundary, capability scope), one **Medium** that is a structural debt (z-index/portal sprawl), the rest **Medium/Low**.

---

## 1. Verdict Table

| #  | Dimension                       | Verdict | One-line                                                                                                       |
|----|---------------------------------|---------|----------------------------------------------------------------------------------------------------------------|
| 03 | Layout & Composition            | Warn    | Strong region model and Leaf/dock split-tree; modal layer not portaled; z-index sprawl; no responsive logic.   |
| 04 | Navigation & Routing            | Pass    | No URL router by design; navigation is leaf/view + deep-link via `UriHandlerRegistry`; first-match-wins is sound. |
| 05 | Accessibility                   | Warn    | Keybinding/palette infra is solid; modal aria-modal mostly present; focus trapping ad-hoc; no a11y test pass.  |
| 06 | State & Data Flow               | Pass    | Zustand stores + ContextKeyService + EventBus + kernel-mirrored ThemeStore, with explicit "kernel = source of truth" pattern. |
| 07 | Performance                     | Warn    | Imperative `LeafHost` keeps mounted views fast; no code splitting / lazy imports; Inter+IBM Plex+JetBrains pulled from googleapis. |
| 08 | Theming & Design Tokens         | Warn    | Bridge from kernel `--nx-*` to Obsidian-style names is well-structured; legacy aliases are explicit; tokens coexist with hex fallbacks across plugin tsx. |
| 09 | Cross-Platform Parity           | Warn    | Tauri targets all OSes; custom `decorations: false` titlebar means in-shell `WindowControls` must do the work — see body-class state machine. |
| 10 | Extensibility                   | Pass    | ExtensionHost two-pass loader, ownership-tracked registries, sandboxed iframe + capability consent — best-in-class for the size. |
| 11 | Observability                   | Warn    | Rust tracing+panic-log right; TS 241 `console.*` calls, no centralized renderer logger; no error→backend pipe. |
| 12 | Multi-Window (Popout)           | Warn    | ADR 0020 close-handshake is correct and tested; capability scope is too broad for a popout boundary; popout ID validation is good. |
| 13 | Persistence                     | Pass    | Atomic tmp+rename in both Rust (`shell-state.json`) and TS (`workspace.json`); migration tests present; v1→v2 themeStore migration explicit. |

---

## 2. Static Inventory (Phase 01)

| Concern               | Value                                                                                  |
|-----------------------|----------------------------------------------------------------------------------------|
| Shell type            | Desktop (Tauri 2). Decisive: `shell/src-tauri/tauri.conf.json`, `shell/src-tauri/Cargo.toml#tauri = "2"`. |
| Frontend framework    | React 18.3 + Vite 5.4, ES2021 target (chrome105 on Windows / safari13 elsewhere).      |
| State                 | Zustand 5 + a per-domain store family (`themeStore`, `configStore`, `paneModeStore`, `docStore`, `layoutStore`, `pluginsStatusStore`, plus `workspaceStore` and a kernel-mirrored theme store). |
| CSS pipeline          | Plain CSS, single `shell/src/shell/shell.css`, design tokens hoisted into `index.html` `<style>` (cf. comment lines 13–28 explaining why). |
| Compiler              | tsc + esbuild (Vite); `@vitejs/plugin-react`.                                          |
| Tauri version         | v2.0.0 (`@tauri-apps/api`, `@tauri-apps/cli`, `tauri = "2"` in `Cargo.toml`).          |
| Tauri plugins         | `fs`, `dialog`, `deep-link`, `window-state`, `global-shortcut`.                        |
| Window config         | 1280×800 default, 600×400 minimum, `decorations: false`, custom in-shell `WindowControls`. |
| CSP                   | `default-src 'self'`; `style-src 'self' 'unsafe-inline'`; production `script-src 'self'` (no `'unsafe-eval'`); dev allows `'unsafe-eval'`. |
| Build outputs         | `shell/dist/` (Vite), `shell/src-tauri/target/` (cargo). E2E build sets `VITE_E2E=true` + `--features custom-protocol`. |
| Shell layer paths     | `src/main.tsx`, `src/shell/App.tsx`, `src/shell/PopoutShell.tsx`, `src/workspace/WorkspaceRenderer.tsx`, `src/host/ExtensionHost.ts`, `src/registry/SlotRegistry.ts`. |
| Tauri entry           | `src-tauri/src/main.rs` (panic-log install + tracing init) → `lib.rs::run()`.          |
| Tauri commands        | 22 (matches CLAUDE.md): 7 kernel, 5 plugin-mgmt, 4 persistence, 1 utility, 5 popout. Lib registered at `shell/src-tauri/src/lib.rs:460-483`. |
| Slot taxonomy         | Chrome-only after Phase 7 cleanup: `overlay`, `titleBar`, `activityBar`, `statusBarLeft`, `statusBarRight`, `paneMode`. Non-chrome panes go through `viewRegistry` + workspace docks. |
| Plugin tiers          | `DEFAULT_ON` + `DEFAULT_OFF` catalog (config key `plugins.enabled`); native core plugins + iframe-sandboxed community plugins; per-plugin `granted_caps.json`. |
| Test files            | 90+ shell tests (registries, sandbox protocol, popout, persistence, keybinding, plugin-API, plugin-import-hygiene). |

Provider/context stack at boot (from `main.tsx::boot()` + `App.tsx`):

1. `installBodyClasses()` (platform / focus / frameless body classes; runs before React mount).
2. `useThemeStore` persist rehydration (sets `data-density` on `<html>` pre-render).
3. `PluginRegistry` + `ExtensionHost` (singletons exposed via `setRegistry` / `setHost`).
4. `keybindings.bindStorage(keybindingOverrideStorage)` then `loadOverrides()` (FU-9, must run before first key).
5. `contextKeyService.set('popoutMode', true)` if applicable.
6. `host.loadAll(plugins)` — two-pass eager + lazy + topo-sort.
7. Community plugin scan → `runInstallTimeConsent` → sandbox orchestrator instantiation.
8. `contextKeyService.set('shellReady', true)` — App's gate for workspace hydration.

Notable absences: **no `<ErrorBoundary>` component anywhere in `shell/src`**, **no `React.lazy` / `Suspense` usage**, **no `createPortal`**, **no `@media` queries**.

---

## 3. Dimension Findings

### 03 — Layout & Composition · Warn

**Strengths.** Region model is explicit and Obsidian-faithful. `App.tsx::App` lays out `.shell-root` → `.shell-overlay` (fixed inset:0, pointer-events:none, z-index 9999) + `.workspace` (`.workspace-ribbon.mod-left` for the activity bar, `.workspace-main-region` for the body region). `WorkspaceRenderer.tsx` then composes left/main/right docks plus a bottom sidedock plus a `floating[]` array, all driven by an in-place mutable tree (`workspace.rootSplit`) with `layout-change` event for re-render — the imperative `LeafHost` pattern (`memo` + `display:none` for hidden tabs) keeps tab switches instant and is a deliberate, documented choice (`WorkspaceRenderer.tsx:5-12`).

The Pane-Mode escape hatch in `App.tsx:191-211` (a single `paneMode` slot entry takes over the body region while keeping the activity bar) is a clean way to host full-bleed views (launcher, settings) without contorting the dock tree.

The window chrome is positioned absolutely over the workspace tree (`WorkspaceRenderer.tsx:120-133`, `WindowControls` at `top:0; right:0; zIndex:100`) and the tab strip pads `paddingRight: 128` when the right sidedock is collapsed (`WorkspaceRenderer.tsx:678`) so the tabs and the OS-control cluster don't collide. That's the right answer; just be aware it is fragile against window-control width changes.

**Issues.**

- **SH-001 (High) — no React error boundary.** `main.tsx:54-63` ships a `showFatal` shim that replaces the `#root` innerHTML when `boot()` rejects, but there is no `ErrorBoundary` around `<App />` or any subtree. A render-time throw inside any plugin's slot contribution (e.g. an `ActivityBarView`, `StatusBarView`, `LauncherView`) bubbles to React's default behavior — full unmount, blank screen, and the user has no graceful recovery. *Evidence:* `rg ErrorBoundary|componentDidCatch|getDerivedStateFromError shell/src` returns nothing.
- **SH-002 (Medium) — modal layer is not portaled and z-index values are scattered.** Eighteen `zIndex:` literals across 22 files, ranging from `1` to `9999`. Concrete collisions waiting to happen: capability banner `900` is below the 1.2k modal layer (`CapabilityModalView.tsx:90`) but above the editor inline-toolbar `65` (`inlineToolbar.ts:104`); ForgeSelector `9500` sits above all kernel-managed modals but below the command palette overlay `9999`; bases timeline uses raw `1` and `2`. `rg createPortal|<Portal shell/src` returns nothing. *Evidence:* `WorkspaceRenderer.tsx:128`, `ConfirmModal.tsx:69`, `CapabilityModalView.tsx:90`, `LauncherView.tsx:292`, `ForgeSelector.tsx:146`.
- **SH-003 (Medium) — no responsive layout logic.** Zero `@media` queries and zero breakpoint utilities (no Tailwind, no `useMediaQuery`). The activity bar is a fixed 24-px column, tab strip is fixed 36 px, default window 1280×800, minimum window 600×400. There is no narrow-viewport collapse strategy (no hamburger menu, no responsive hide-rules). On a 13" MacBook side-by-side with another window, the chrome will eat a large fraction of horizontal space and the right sidedock cannot collapse below the 150-px `DOCK_MIN_SIZE` while still showing content. *Evidence:* `rg '@media' shell/src` returns no matches; `WorkspaceRenderer.tsx:152-157`.
- **SH-004 (Low) — `--ui-size` 13/12/14 px density scale is hard-coded in `index.html`** rather than emitted by the kernel theme. Picking a "compact" density doesn't widen the chrome or change icon sizes — they use literal `width: 36`, `width: 40`, `width: 24` throughout `WorkspaceRenderer.tsx`. Density is therefore a font-size-only mode in practice. *Evidence:* `index.html:175-178`, `WorkspaceRenderer.tsx:154-157`, 670, 681, 715, 754.

### 04 — Navigation & Routing · Pass

The shell intentionally does not have a URL router (correct for a workspace shell). Navigation comes from three places, all well-scoped:

1. The **Leaf/ViewRegistry workspace tree** (`shell/src/workspace/`) — every view instance is a `Leaf` whose `setViewState({ type, …, active })` calls fire activation triggers (`onView:<type>`) before the view's creator runs. Switching tabs is `workspace.setTabActiveIndex(tabsId, i)`.
2. **Deep links** via `tauri-plugin-deep-link` → `shell/src-tauri/src/lib.rs:386-399` emits `nexus:url-opened` → `main.tsx:331-340` listens and dispatches through `UriHandlerRegistry`. First-match-wins per `(scheme, pluginId)` with a clean conflict policy that rejects cross-plugin shadowing (`UriHandlerRegistry.ts:60-74`). `nexus://` is registered in `tauri.conf.json#plugins.deep-link.desktop.schemes`. Tested: `tests/uri-handler-registry.test.ts`.
3. **Commands** via `KeybindingRegistry` + `CommandRegistry` + the `nexus.commandPalette` plugin, with `when` clause evaluation against the global `ContextKeyService`. Activation triggers cover `onCommand:`, `onView:`, `onUri:`, `onLanguage:` and pre-register manifest contributions before the first dispatch — lazy plugins reachable via the palette without forcing activation (`ExtensionHost.ts:65-110`).

The handful of ad-hoc query-string nav (`?popout=…&leaf=…` for popout windows) is well-contained and the inputs are validated server-side (`windows.rs::is_valid_popout_id`, character-class enforcement after CVE-grade audit #86).

No router state survives across reloads; that's correct for this shell — the workspace JSON is the persistence layer.

### 05 — Accessibility · Warn

The keybinding stack is genuinely good: `KeybindingRegistry` does conflict detection (`OI-10`), conservative `when` overlap analysis, persisted user overrides via a pluggable `OverrideStorage` (`FU-9`), and emits `plugins:keybindings-conflict` so the settings UI can surface it. Modal dialogs broadly do the right things — `ConfirmModal.tsx:55-58` sets `role="dialog"`, `aria-modal="true"`, `aria-labelledby`, focuses the confirm button on mount, traps Enter/Escape; the command palette's input owns its own key semantics (`CommandPalette.tsx:79-98`).

But:

- **SH-005 (Medium) — no global focus trap for modals.** Modals set `aria-modal` and listen for Escape but don't trap Tab — focus can leave the modal into the underlying activity bar / tab strip / status bar, which are still in the tab order. There's no `inert` attribute on the workspace tree when a modal is open. *Evidence:* `rg focus-trap|focus_trap|inert shell/src` returns nothing.
- **SH-006 (Medium) — App-level keyboard dispatcher swallows all keys.** `App.tsx:130-146` registers `document.addEventListener('keydown', handler)` and calls `e.preventDefault()` + `e.stopPropagation()` on any keybinding match. The guard against INPUT/TEXTAREA/contenteditable is correct but there's no escape route for users with screen readers using virtual cursor / browse mode keys; the `e.stopPropagation()` will block forwarding to assistive tech. Consider gating on `e.defaultPrevented` first or adding an opt-out context key.
- **SH-007 (Low) — no `prefers-reduced-motion` support.** Single match in `themeStore.ts` is for `prefers-color-scheme` (system theme detection). The shell has minimal animation today (CSS transitions in `shell.css`) so the impact is small, but it should still be wired.
- **SH-008 (Low) — sidebar TabButton fallback "neutral dot" loses information.** `WorkspaceRenderer.tsx:1100-1115` notes the previous behavior surfaced "broken-looking" letters; the dot replacement avoids that but still fails to convey the view name to a screen reader on tab focus. The `aria-label` is set to `label`, so a sighted-AT user is fine; users with no AT and no view icon mapping get a featureless dot.

### 06 — State & Data Flow · Pass

The shell is unusually disciplined here. State lives in:

- **Zustand stores per domain** (`stores/themeStore.ts`, `stores/configStore.ts`, `stores/paneModeStore.ts`, etc.) with explicit `persist` middleware where appropriate (`shell-theme`, `shell-config`).
- **`ContextKeyService`** as the global predicate registry that drives `when` clause evaluation in `KeybindingRegistry.match()` and lazy-activation triggers — type-safe, observable, and the single dispatch point during boot (`shellReady`, `popoutMode`).
- **`EventBus`** (`shell/src/host/EventBus.ts`) for cross-plugin pub/sub (`activityBar:itemAdded`, `plugin:activated`, `layout-change`).
- **The kernel as source of truth for theme state** — `themeStore.ts:14-22` documents this explicitly: the store is "a *reflection* of kernel state, never the source of truth." `apply_config` is pushed back to the kernel on rehydrate so engine state matches the persisted UI selection.
- **The mutable workspace tree** with `workspace.on('layout-change', …)` for re-render. Tree mutations preserve top-level identity by design; the `useLayoutVersion` hook bumps a counter to force re-render. Documented at `WorkspaceRenderer.tsx:38-47`.

The boundary between "kernel state" and "shell state" is consistently respected — even `set_plugin_enabled` and `set_plugin_granted_capabilities` write the kernel's on-disk format directly (atomic tmp+rename) rather than going through a runtime IPC, with the explicit comment that the kernel only reads them at boot (`lib.rs:198-216`). That's exactly the right pattern.

One quibble: there's no central type for "things plugins can publish to the shell" (the `pluginList`, `availablePlugins`, `builtinPluginTotal`, `communityPluginManifests`, `communityPluginDenied`, `sandboxOrchestrator` services in `main.tsx:215, 237, 265, 293, 307, 318` are stringly-typed), so future plugins will keep typing the bag. Worth a typed service registry.

### 07 — Performance · Warn

The big perf wins are already in place: imperative `LeafHost` ownership keeps mounted view DOM intact across tab switches (`WorkspaceRenderer.tsx:1191-1244`), `memo` blocks parent-render cascade, the iframe sandbox runtime is bundled once at Vite build time and Blob-wrapped on first sandboxed-plugin load (`main.tsx:267-281`).

**Issues.**

- **SH-009 (Medium) — no code splitting / lazy imports.** The DEFAULT_ON catalog is statically imported in `main.tsx:45-50`; every non-default plugin is imported when the user enables it (`optInPlugins.filter(...)`). All plugin code therefore ships in the main bundle. For a desktop app this is acceptable (the user already downloaded the binary), but the Vite chunk graph is unsegmented and HMR will be slow to converge. *Evidence:* `rg 'React\.lazy|Suspense|lazy\(' shell/src` returns nothing.
- **SH-010 (Medium) — Google Fonts pulled from `googleapis.com` at boot.** `index.html:9-12` preconnects to `fonts.googleapis.com` and `fonts.gstatic.com`. CSP allows this (`font-src 'self' data:` only — wait, that means the fetch is going to fail under prod CSP unless the browser follows the redirect to `gstatic.com`, which is *not* in `connect-src`). Either CSP needs `https://fonts.gstatic.com` added to `font-src`, or the fonts need to be self-hosted (`shell/public/fonts/…`). Self-hosting is the better answer for a desktop app — no online dependency on first paint. *Evidence:* `tauri.conf.json:30-32`, `index.html:7-12`.
- **SH-011 (Low) — `App.tsx` 500-ms `setTimeout` debug logger fires on every slot change** (`App.tsx:33-48`). It's behind `console.info` only but it accumulates state churn across boot. Gate on `import.meta.env.DEV`.

### 08 — Theming & Design Tokens · Warn

The theming architecture is well-conceived: the kernel `com.nexus.theme` plugin computes a `--nx-*` variable cascade (theme + ordered snippets) and the shell bridges them into the consumer-name tokens (`--background-primary`, `--text-normal`, `--interactive-accent`, …) in `index.html`'s inline `<style>` so they're guaranteed live before React mounts. Themes ship in `crates/nexus-theme/themes/*` plus user-orderable snippets (`reorder_snippets`, `toggle_snippet`).

The persisted slice (`activeThemeId`, `kernelMode`, `enabledSnippets`) is restored *before* the kernel hands back resolved variables — `apply_config` first, then `get_theme_config`, then `compute_variables`, then write to `:root` via `setProperty`. Orphan tokens are cleared via the `appliedVariableNames` cache. v1→v2 migration is explicit (`themeStore.ts:386`).

**Issues.**

- **SH-012 (Medium) — heavy back-compat alias soup.** `index.html:108-173` defines five overlapping alias families: Obsidian-style canonical (`--background-primary`, `--text-normal`), legacy Forge (`--bg`, `--fg`, `--line`, `--accent`, `--r`), VSCode-style (`--shell-bg`, `--editor-bg`, `--statusbar-bg`), `--color-*` (settings-panel-only), and `--bg-primary`/`--fg-primary`/`--bg-input` (bases-only). New tsx files inherit whichever set their author saw first; tokens get added to the wrong family; theme-author intent doesn't propagate predictably. The TODO ("Remove these once every consumer uses the Obsidian names above") has been there long enough to count as a sustained debt.
- **SH-013 (Medium) — hex fallbacks in `var(--token, #1e1e1e)` plugin code paper over missing tokens.** `WorkspaceRenderer.tsx:172-173, 660, 871, 1086-1090` and others use `'var(--background-secondary, var(--bg-raised, #252526))'`. If the kernel theme fails to hydrate, the user sees the fallback, not a missing-token error. This makes theme-coverage gaps invisible in QA. Consider a `--missing-token-debug` mode (in dev) that resolves to a hot-pink so gaps surface.
- **SH-014 (Low) — `--ui-size` decoupled from kernel theme.** Density is a CSS-only concept declared in `index.html`; the kernel theme doesn't see it and themes can't ship density-dependent overrides.

### 09 — Cross-Platform Parity · Warn

`tauri.conf.json#bundle.targets: "all"` plus `windows.decorations: false` plus an in-shell `WindowControls` component plus a `bodyClasses.ts` "platform / frameless / focus" state machine that runs once before React mounts (`main.tsx:382`). That's the right shape.

**Issues.**

- **SH-015 (Medium) — frameless titlebar puts the burden on `WindowControls` to be correct on every OS.** macOS expects traffic-light buttons in the top-left, Windows expects min/max/close in the top-right at a specific 46×30 pixel size, Linux is whichever-the-WM-says. The audit can't open the file (it's referenced from `WorkspaceRenderer.tsx:23`) but the absolute `top:0; right:0; zIndex:100` placement strongly suggests Windows-first. macOS users will see traffic lights in the wrong corner unless this is platform-branched. *Evidence:* `WorkspaceRenderer.tsx:120-133`.
- **SH-016 (Low) — Vite `build.target` branches on `TAURI_PLATFORM === 'windows'` only** (`vite.config.ts:39-43`). macOS and Linux both get `safari13` (broadly OK for desktop webviews) but Windows-only branching is an early sign that Linux WebKit2GTK quirks haven't been exercised. Worth a runtime sweep on Linux.

### 10 — Extensibility · Pass

This is the strongest dimension. The architecture is unusually mature:

- **`PluginRegistry`** with sub-registries (`commands`, `config`, `keybindings`, `settingsTabs`, `statusBar`, `slotRegistry` singleton, `uriHandlerRegistry` singleton) and a per-plugin **ownership index** that auto-cleans on unload — including a separate subscriptions index for kernel-bus listeners and tagged keybinding overrides that respect Settings-UI overrides on top.
- **`ExtensionHost`** with a documented two-pass loader (eager activation + lazy `onView/onCommand/onUri/onLanguage` triggers, with manifest contributions pre-registered so the palette sees them before activation), topological dep ordering (core before community within each tier), per-plugin `error` state that won't retry, and a soft-cap deactivation sweep on `beforeunload` (1s per plugin).
- **`SandboxOrchestrator`** for community plugins: null-origin iframe, srcdoc + dynamic-imported guest runtime, ping/pong watchdog with `plugin:crashed` event, host-side command bridge, PanelNode refresh channel, per-plugin `PluginAPI` factory bound to the authoritative pluginId at handshake time (with `assertValidPluginId` rejecting cross-plugin storage cross-pollution).
- **Capability consent flow** with per-plugin `granted_caps.json` (atomic tmp+rename, kernel-format pinned to manifest version so a version bump re-prompts). Wire form is the dotted kernel string (`fs.read`, `process.spawn`, …) and the Rust side validates against `Capability::from_str` at write time so a malicious frontend can't persist garbage (issue #86).

Tests cover the protocol, orchestrator, e2e, and runtime bundle. The `@nexus/extension-api` workspace package is the contract surface.

The one note (Low): `reg.registerService(name, …)` is stringly-typed; a typed service registry would protect future plugins.

### 11 — Observability · Warn

**Rust side: solid.** `nexus_panic_log::install("nexus-shell")` runs first thing in `main.rs:13` so a panic during Tauri init still lands at `~/.nexus-shell/logs/panic.log`. `tracing-subscriber` with `env-filter` defaults to INFO and respects `RUST_LOG`. Errors in the deep-link emit, popout sync, and shutdown path log to stderr through `tracing` (or `eprintln!` where the path predates the bridge — this should be normalized).

**TS side: under-instrumented.**

- **SH-017 (Medium) — 241 raw `console.*` calls across 69 files; no centralized client logger.** No log forwarding to the Rust side, no structured fields, no severity routing. A user reporting "the activity bar disappeared" can't easily share logs that survived a beforeunload. *Evidence:* `rg 'console\.' shell/src` returns 241 occurrences.
- **SH-018 (Medium) — no error → backend pipe.** When `host.loadAll` fails for a plugin, the error is set in `errors` map and printed; nothing surfaces to a global error state nor is anything written through a Tauri command for crash-report aggregation. Combined with the missing error boundary, an in-render throw vanishes after the user reloads.

### 12 — Multi-Window (Popout) · Warn

ADR 0020 is implemented as documented and has tests (`tests/popout-shell.test.ts`, `src/shell/PopoutShell.test.ts`).

- The close handshake (popout emits `nexus:popout-closed` on `onCloseRequested`; main listens, removes the FW from `floating[]`, defensively calls `close_popout_window` which is idempotent) is the right pattern for keeping the persisted state authoritative across racing OS-side closes.
- Popout boots the same DEFAULT_ON plugin set with `popoutMode = true` set on `ContextKeyService` so plugins can adapt (the workspace plugin skips kernel lifecycle calls; the kernel is owned by the main window via Tauri managed state).
- ID validation is character-class strict and length-capped (`is_valid_popout_id` / `is_valid_leaf_id` in `windows.rs`) — explicitly hardened against the audit-#86 "smuggle extra query params" attack.

**Issues.**

- **SH-019 (High) — capability scope wildcard grants popout windows the same fs/dialog set as main.** `capabilities/default.json` lists `"windows": ["main", "*"]` with a permissive fs set (`fs:allow-read-text-file`, `write-text-file`, `mkdir`, `remove`, `rename`, `watch`). Popouts host arbitrary leaf views, including views that may dynamically dlopen a community plugin's UI iframe. While the iframe sandbox itself is null-origin and capability-gated by the kernel, the *Tauri command surface* of the parent webview is unchanged in a popout. A community plugin that compromises a leaf view's iframe (which is a vector explicitly defended against by the orchestrator) gains the fs set anyway via `kernel_invoke`. Tightening to a popout-specific capability set ("popout-default" with no fs.write paths) would close the gap.
- **SH-020 (Medium) — popout boots a full plugin set even though most plugins contribute nothing.** `main.tsx:206-209` skips the community-plugin scan and sandbox orchestrator in popout mode but still runs `host.loadAll(plugins)` for every default-on plugin. Boot time and memory are paid per popout window. ADR 0020 §1 mentions this is intentional (so plugin view creators are registered before the popout's `LeafHost` mounts), but a leaner "popout-allowlist" set of plugins would reduce overhead.
- **SH-021 (Low) — popout reads workspace.json but cannot write it.** `PopoutShell.tsx:117-119` calls `workspaceStore.hydrate(json)` (which mutates the popout's local store), and the documentation says "main window owns the write side per ADR 0020 §1". If a popout user resizes the popout window, the bounds change is funneled through `set_popout_window_bounds` Tauri command but no path persists it back to `<vault>/.forge/workspace.json` from the popout's side. There may be a path through `floating[]` mutations on the main side but it's not obvious from a static read.

### 13 — Persistence · Pass

Two layers, both correct:

1. **Shell-state** (`<app_config_dir>/shell-state.json`) — Rust-side. Atomic tmp+rename (`persistence.rs:82-88`), `load → mutate → save` round-trip, `MAX_RECENT_FORGES = 8` cap, `version: u32 = 1`, `serde(default)` on every field for forward compat. Tests cover round-trip, missing file, corrupt file (`persistence.rs:139-171`).
2. **Workspace state** (`<vault>/.forge/workspace.json`) — TS-side via the kernel's `com.nexus.storage::write_vault_file`. Schema-guarded (`persistence.ts:104-120`), debounced 250 ms saves, file goes under `.forge/` so FTS / knowledge graph don't index it (`persistence.ts:14-17`). Tests cover hydrate + the migrate-shell-state flow (`tests/migrate-shell-state.test.ts`).

The themeStore persist v1→v2 migration is also explicit (`themeStore.ts:377-389`) — v1 blob passes through, new fields default via shallow-merge, `null activeThemeId` makes hydrate skip the `apply_config` restore on first post-upgrade boot.

The only nit: there's no documented schema-corruption recovery flow beyond "fall back to defaults". A missing `<vault>/.forge/workspace.json` that's actually a partial write produces a default layout; the user loses tab arrangement silently. Consider `.workspace.json.bak` before each save.

---

## 4. What's Going Right (Worth Preserving)

- **The microkernel boundary** is enforced architecturally — the shell talks to backend services only through `kernel_invoke` (`shell/src-tauri/src/lib.rs:474`). The 22-command Tauri surface is grouped by intent, mostly host-management. The instinct to *not* add bespoke `#[tauri::command]` handlers for new feature capability is the single most important habit in this codebase.
- **Capability consent + atomic file writes everywhere** — `granted_caps.json`, `shell-state.json`, `workspace.json` all use tmp + rename. The PR-#86 hardening (validating capability strings against the kernel enum before persistence) is a sign the threat model is taken seriously.
- **The Leaf + ViewRegistry + workspace store pattern** elegantly handles the hard case (multiple instances of the same view type with independent state, persisted layout, in-place tree mutation, fast tab switches). The `display:none` trick + memo + `attachContainer` flow is the right way to do this in React.
- **The two-pass plugin loader** with eager + lazy + topo + dep-promotion is a textbook implementation of the pattern, with the right escape hatches (lazy plugin promoted to eager when it's `dependsOn`'d).
- **Tests are weighted toward the shell contract** — registries, sandbox protocol, popout, persistence, migration — exactly the surfaces where regressions are expensive.
- **ADRs and inline comments are consistently good** — most non-trivial decisions cite a numbered ADR or a backlog item, and the comments explain *why*, not just *what*. This will pay back enormously in 18 months.

---

## 5. Risks & Confidence

**Confidence: medium-high on architecture, medium on dynamic behaviors.**

Static-only audits cannot observe: the actual focus order under a screen reader, whether modals trap focus in practice, real OS-platform titlebar behavior (Windows vs macOS vs Linux), whether the popout-sync handshake handles `kill -9` of a popout cleanly, theming "flash of unstyled content" timing in production builds, popout boot time.

A second pass with a live Tauri dev session and `mcp__Claude_in_Chrome` instrumentation is the right way to verify the `Warn`s on dimensions 03, 05, 09, 12.

---

## 6. Recommended Reading Order for the Backlog

The backlog (`shell-ui-audit-backlog-2026-05-01.md`) is organized strictly by severity. If you only do the top three items, in order:

1. **SH-001** — wrap `<App />` in an `ErrorBoundary` that reports through a single client-logger pipe and shows the user a recover/restart affordance.
2. **SH-019** — split the Tauri capability set into a `popout-default` window-scoped subset that drops the fs writes; tighten `windows: ["main"]` on `default.json`.
3. **SH-002** — introduce a `<ModalRoot />` near `#root` and migrate every modal to `createPortal`, then collapse z-index literals into a 6-value scale (`overlay-base`, `overlay-floating`, `overlay-modal`, `overlay-toast`, `overlay-fatal`, plus `chrome-controls`).

These three remove the highest-leverage failure modes and unlock the rest of the cleanup.
