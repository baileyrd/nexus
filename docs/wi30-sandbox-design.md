# WI-30 — Community Plugin Sandbox Design

**Status:** Draft, ready for review
**Date:** 2026-04-23
**Scope:** Community-tier plugins only (Q1 default). First-party plugins unchanged.
**Phase:** 3c, depends on Phase 3a (shipped) + Phase 3b (shipped)

## 1. Motivation

Community plugins today run as ES modules in the main WebView with full access to `window`, `document`, `@tauri-apps/*`, and every other plugin's globals (§2). We need a structural isolation boundary *before* advertising a community marketplace: a malicious or buggy plugin must not read the editor DOM, spoof another plugin's toasts, exfiltrate unrelated `localStorage`, or reach `fetch()` / `@tauri-apps/plugin-fs` without passing the capability gate. Iframe with `sandbox="allow-scripts"` (no `allow-same-origin`) delivers that boundary with zero new native deps. Workers cannot own a DOM, the Realms shim still leaves code on the same origin, QuickJS-in-WASM adds a 400KB runtime per plugin and a second FFI to audit. See §11.

## 2. Current state audit

- **CSP is disabled.** `shell/src-tauri/tauri.conf.json:26` sets `"csp": null`. Nothing restricts `script-src`, `connect-src`, or `frame-src`.
- **Community plugins load as ES modules in the main WebView.** `shell/src/host/communityPluginLoader.ts:238-250` reads the bundle via `readTextFile`, wraps it in a `Blob`, passes the blob URL to `import()`. The module lands in the shell realm with full access. The Blob-URL trick was chosen to dodge CSP, not to isolate.
- **One community plugin exists.** `shell/src/plugins/community/hello-world/index.js` (plus repo-dev examples under `plugins/`). Blast radius is tiny.
- **Consent already gates activation.** `shell/src/main.tsx:246` calls `runInstallTimeConsent(communityManifests)` (`shell/src/plugins/core/capabilityPrompt/requestConsent.ts:79`) before `loadEnabledCommunityPlugins` at `main.tsx:263`. Denied plugins never import. Grants persist per version in `granted_caps.json` via `set_plugin_granted_capabilities`.
- **Kernel returns a wire-stable error envelope.** `packages/nexus-extension-api/src/generated/IpcErrorEnvelope.ts` — `{ kind, plugin_id, command, message, retryable }` with `IpcErrorKind` = `timeout | plugin_crashed | capability_denied | dispatch_failed | serialization | unknown`. Reused as sandbox RPC error shape.
- **`api.kernel.on` has an idempotent-unsub pattern** (`shell/src/host/PluginAPI.ts:253-287`): single closure with `disposed` flag, tracked via `registry.trackSubscription`. Model for every cross-boundary subscription (§5.6).
- **No existing iframe or postMessage usage.** `grep -rn "iframe\|postMessage" shell/src` yields one unrelated match. Greenfield.
- **`WebviewPanelConfig` exists conceptually** (`packages/nexus-extension-api/src/index.ts:124-127`): `{ htmlUrl, allowPopups? }`. Nearest analog to §6; sandbox generalizes it.

## 3. Threat model

Community code is *untrusted but not APT-grade* — a hostile author writing plausibly-deniable plugins.

1. **Cross-plugin DOM / state access.** Read DOM of another plugin, editor keystrokes, other plugins' `localStorage`. *Mitigated:* null-origin `srcdoc` iframe, no shared DOM; `localStorage` namespaced + only reachable via host RPC.
2. **`@tauri-apps/*` privilege escalation.** `@tauri-apps/plugin-fs.readTextFile("~/.ssh/id_rsa")`. *Mitigated:* no `allow-same-origin` → the `__TAURI_IPC__` bridge isn't reachable; all FS/dialog/shell go through `api.platform.*` RPC with capability checks.
3. **Credential / cookie theft.** `document.cookie`, IndexedDB, SW registrations. *Mitigated:* opaque origin has no access to the shell's cookie jar; iframe storage is in its own partition.
4. **Arbitrary fetch / network egress.** *Mitigated:* iframe CSP restricts `connect-src` to `'self'`; `api.platform.shell.openExternal` is the only outbound channel and requires a capability.
5. **Prototype pollution of host globals.** *Mitigated:* separate realms; plugin's `Object.prototype` is not ours.
6. **XSS via plugin-contributed HTML.** *Mitigated:* host treats every RPC string as text; PanelNode has no HTML escape hatch (§6).
7. **RPC origin spoofing.** *Mitigated:* host keeps a `WeakMap<Window, PluginHandle>` keyed on `event.source`; identity check, not origin string (origin is `"null"`).

Out of scope: Spectre-class side channels, busy-loop DoS (handled by watchdog §5.5), supply-chain attacks on bundled npm deps (WI-39 / marketplace).

## 4. Architecture

### 4.1 High-level

```
┌────────────────────────────────────────────────────────────────────────┐
│  Host shell (main WebView, privileged origin)                          │
│  ────────────────────────────────────────────                          │
│   PluginRegistry + capability grants + @tauri-apps bridge              │
│                                                                        │
│      ▲    ┌──────────────── SandboxOrchestrator ──────────────┐        │
│      │    │  creates iframes, routes RPC, enforces caps       │        │
│      │    └───────▲──────────────────────────────────▲────────┘        │
│      │            │   postMessage (structured clone)  │                │
│  ┌───┴────────────┴─┐                            ┌────┴─────────────┐  │
│  │ Iframe: plugin A │                            │ Iframe: plugin B │  │
│  │ sandbox=allow-   │                            │ (same pattern)   │  │
│  │   scripts        │   srcdoc bootstraps a      │                  │  │
│  │ origin: "null"   │   guest-side RPC stub +    │                  │  │
│  │                  │   dynamic-imports plugin   │                  │  │
│  │  plugin code     │   bundle as ES module      │                  │  │
│  │  calls api.*     │                            │                  │  │
│  └──────────────────┘                            └──────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

- **Host** holds all privileges: Tauri IPC, filesystem, capability-grant table, slot/view registries, first-party plugin state.
- **Sandbox frame** is the RPC stub (`guestBridge.js`, injected via `srcdoc`) plus the plugin bundle. It holds only a proxy `api` object, no privileges.
- **Plugin code** is what the author wrote — it sees an object that looks like `PluginAPI` but every method is a promise-returning RPC call.

### 4.2 Iframe configuration

```html
<iframe
  sandbox="allow-scripts"
  srcdoc="<!doctype html>...guest bootstrap + <script type='module'>import(...)</script>..."
  referrerpolicy="no-referrer"
  loading="eager"
  aria-hidden="true"
  style="display:none"
></iframe>
```

- `allow-scripts` is required to run the plugin at all.
- **`allow-same-origin` is NOT set** — this is the central isolation guarantee. The frame's origin is `"null"`, it cannot read the shell's cookies, cannot reach `localStorage` in the shell, and `@tauri-apps/api/core`'s `__TAURI_IPC__` lookup fails.
- `srcdoc` vs `src=blob:` — **srcdoc is the recommendation.** Blob URLs inherit the creator's origin in some engines; `srcdoc` guarantees the "null origin" outcome. The plugin bundle itself is still loaded via `import(blobUrl)` *from inside the srcdoc*, matching today's loader mechanics (`communityPluginLoader.ts:246`).
- **CSP inside the iframe** is injected via a `<meta http-equiv="Content-Security-Policy">` tag in the srcdoc:
  ```
  default-src 'none';
  script-src 'unsafe-inline' blob:;
  connect-src 'self';
  style-src 'unsafe-inline';
  img-src data: blob:;
  ```
  `'unsafe-inline'` on `script-src` is required because the bootstrap itself is inline in `srcdoc`; the dynamic `import(blobUrl)` covers the plugin bundle. The host's CSP (`tauri.conf.json`) separately gets `frame-src 'self' blob:`.

### 4.3 Bootstrap sequence

1. Manifest discovery (unchanged) — `scanCommunityPlugins()` at `communityPluginLoader.ts:148`.
2. Consent prompt (unchanged) — `runInstallTimeConsent` at `main.tsx:246`; denied plugins short-circuit.
3. `SandboxOrchestrator.spawn(manifest, grantedCaps)` creates a hidden `<iframe>` in `#plugin-sandbox-container`.
4. Host assigns `pluginInstanceId = uuid()`, stores `{ iframe, manifest, grantedCaps, pending, subscriptions }` in `Map<pluginInstanceId, PluginHandle>`.
5. srcdoc loads; guest bootstrap posts `{sys:"handshake/hello", protocolVersion:1}` to `window.parent`.
6. Host validates `event.source === iframe.contentWindow`, responds `{sys:"handshake/accept", pluginInstanceId, apiVersion, protocolVersion, methods}`.
7. Guest gets the plugin bundle (inlined in the accept payload), creates a `Blob`, `import()`s it.
8. Guest calls `plugin.activate(apiProxy)` — `apiProxy` is generated from `methods`; every method returns `rpcCall(name, args)`.
9. Plugin's `commands.register`, `statusBar.createItem`, etc. flow back as RPC requests the host fulfils against `PluginRegistry`.
10. Activation resolves → plugin goes green in `PluginsMgmt`.

## 5. RPC protocol

### 5.1 Message envelope

Every frame of communication, in both directions, is a single JSON-serializable object:

```ts
type Envelope =
  | { id: string; dir: "g2h" | "h2g"; kind: "req"; method: string; args: unknown }
  | { id: string; dir: "g2h" | "h2g"; kind: "res"; ok: true;  result: unknown }
  | { id: string; dir: "g2h" | "h2g"; kind: "res"; ok: false; error: IpcErrorEnvelope }
  | { id: string; dir: "h2g";         kind: "evt"; topic: string; subscriptionId: string; payload: unknown }
  | { id: string; dir: "g2h" | "h2g"; kind: "sys"; system: "handshake/hello" | "handshake/accept" | "handshake/reject" | "ping" | "pong" | "suspend" | "resume" | "unload"; data?: unknown };
```

Rules:

- `id` — caller-generated UUIDv4; `req` creates a pending entry, matching `res` resolves. Each side seeds its own ids so collisions are impossible.
- `dir` — advisory (routing uses `event.source` identity); included so crash dumps are self-describing.
- `kind` — `req` / `res` / `evt` / `sys`. `evt` is host→guest only for subscription delivery (§5.6); not correlated with `id`.
- `method` — dotted, matches `api.*`: `"commands.register"`, `"platform.fs.readText"`.
- **`error` reuses `IpcErrorEnvelope` verbatim** — `{ kind, plugin_id, command, message, retryable }`. Sandbox-only failures use the existing `IpcErrorKind` set: `"capability_denied"` for cap checks, `"dispatch_failed"` for protocol errors, `"plugin_crashed"` for crashes. The kernel enum is not touched.
- Payloads go through `structuredClone` via `postMessage`, so `Date`/`Map`/`Set`/`ArrayBuffer` round-trip; functions do not (deliberate — see §6).

### 5.2 Method catalog

Every field of `PluginAPI` must have a mapping across the boundary. Table below enumerates; "cap" = required capability grant, checked host-side (§5.3); "dir" = who initiates.

| Method | Dir | Args shape | Returns | Cap |
|---|---|---|---|---|
| `commands.register(id, handler)` | g→h | `{ id, handlerId }` (handlerId = guest-local fn ref) | `void` | — |
| `commands.execute(id, ...args)` | g→h | `{ id, args }` | `unknown` | — |
| `commands.all()` | g→h | `{}` | `CommandInfo[]` | — |
| *command dispatch* | h→g | `{ handlerId, args }` | `unknown` | — |
| `kernel.invoke(targetId, cmd, args, timeoutMs)` | g→h | `{ targetId, cmd, args, timeoutMs }` | `unknown` or `IpcErrorEnvelope` err | as declared by kernel |
| `kernel.on(topicPrefix, handler)` | g→h | `{ topicPrefix, subscriptionId }` | subscriptionId | `EventSubscribe` |
| *kernel event* | h→g | `evt { topic, subscriptionId, payload }` | — | — |
| `kernel.available()` | g→h | `{}` | `boolean` | — |
| `platform.fs.readText(path)` | g→h | `{ path }` | `string` | `FsRead` |
| `platform.fs.writeText(path, content)` | g→h | `{ path, content }` | `void` | `FsWrite` |
| `platform.fs.readDir(path)` | g→h | `{ path }` | `PlatformDirEntry[]` | `FsRead` |
| `platform.fs.{exists,mkdir,remove,rename}` | g→h | path-args | `void` / `boolean` | `FsRead`/`FsWrite` |
| `platform.dialog.{openFile,openDirectory,saveFile}` | g→h | options | `string \| string[] \| null` | `DialogShow` |
| `platform.window.{minimize,toggleMaximize,close,isMaximized}` | g→h | `{}` | `void` / `boolean` | `WindowControl` |
| `platform.window.onResize(handler)` | g→h | `{ subscriptionId }` | subId | `WindowControl` |
| *resize event* | h→g | `evt` | — | — |
| `platform.shell.openExternal(target)` | g→h | `{ target }` | `void` | `ShellOpen` |
| `events.on(event, handler)` | g→h | `{ event, subscriptionId }` | subId | — |
| `events.emit(event, payload)` | g→h | `{ event, payload }` | `void` | — |
| *event dispatch* | h→g | `evt` | — | — |
| `storage.{get,set,delete,clear}` | g→h | `{ key?, value? }` | string/void | — (namespaced per-plugin) |
| `notifications.show(notification)` | g→h | `{ notification }` | `void` | — |
| `statusBar.createItem(config)` | g→h | `{ config }` | `StatusBarHandle` (proxy) | — |
| `context.{get,set,evaluate}` | g→h | see source | bool/void | — |
| `configuration.register(section)` | g→h | `{ section }` | `void` | — |
| `configuration.getValue(key, default)` | g→h | `{ key, default }` | `T` | — |
| `configuration.setValue(key, value)` | g→h | `{ key, value }` | `void` | `ConfigWrite` |
| `configuration.onChange(key, handler)` | g→h | `{ key, subscriptionId }` | unsub | — |
| `uri.register(scheme, handler)` | g→h | `{ scheme, handlerId }` | unsub | `UriHandler` |
| *uri dispatch* | h→g | `{ handlerId, url }` | — | — |
| `input.{prompt,confirm}` | g→h | `{ message, placeholder? }` | string/bool | — |
| `activityBar.{addItem,removeItem}` | g→h | config / id | `void` | — |
| `views.register(viewId, config)` | g→h | **see §6** — `component` field CANNOT cross the boundary | — | — |
| `workspace.*` / `viewRegistry.*` | — | **cannot cross** | — | — |

**`workspace` + `viewRegistry` can't cross as-is.** Both are live object references (`shell/src/workspace/*`) with methods that mutate shell state synchronously. Options: (a) expose a frozen snapshot via `api.workspace.getInfo()` returning `{ root, name }` and a subscription via `workspace.onChanged`; (b) fan out each method as an RPC. Recommendation: **(a)** — sandboxed plugins get a read-only view. Mutation points (open file, create leaf) route through `commands.execute` with the existing first-party command ids, which already enforce capabilities.

**`views.register` can't cross as-is** because the `component: ComponentType` field is a React function reference. See §6.

### 5.3 Capability enforcement

- **Host-side pre-dispatch check.** `SandboxOrchestrator.handleRequest` looks up `method` in `METHOD_CAP_MAP` (e.g., `"platform.fs.readText"` → `"FsRead"`). If the required cap isn't in `handle.grantedCaps`, host responds with `IpcErrorEnvelope { kind:"capability_denied", plugin_id, command: method, retryable:false }`. No bridge call is made.
- **Defense in depth.** `api.kernel.invoke` still routes to `kernel_invoke`, which re-checks capabilities server-side (`crates/nexus-plugins/src/loader.rs`). Sandbox check fails fast with a useful message; kernel is the authoritative backstop.
- **Grants come from consent.** `handle.grantedCaps` is populated at spawn time from `runInstallTimeConsent`'s result, already persisted in `granted_caps.json` (`requestConsent.ts:186-188`). No new persistence layer.

### 5.4 Versioning

Two versions, versioned separately:

1. **`PLUGIN_API_VERSION`** (`1`, `extension-api/src/index.ts:51`) — the plugin-author ABI.
2. **`SANDBOX_PROTOCOL_VERSION`** (new, starts at `1`) — the RPC envelope + method catalog.

Plugin ABI is what the author sees; sandbox protocol is what the bootstrap stub sees. Because the host ships the bootstrap in `srcdoc`, plugin code never sees the wire protocol directly — a plugin written against ABI v1 works under any protocol version.

Handshake: guest sends `{sys:"handshake/hello", data:{protocolVersion:1}}`; host accepts if supported → `handshake/accept` with `{pluginInstanceId, apiVersion, negotiatedProtocol, methods}`; mismatch → `handshake/reject` with `dispatch_failed` and tear-down.

Forward compat is additive. The guest proxy is generated at bootstrap from the `methods` list, so a newer host exposes new methods automatically; calling an unknown method returns `IpcErrorEnvelope { kind:"dispatch_failed", message:"unknown method" }`.

### 5.5 Lifecycle

- **Init** — handshake only, no plugin code. Timeout 2s; no-show → tear down, log "failed to initialize".
- **Activate** — host posts `{sys:"activate"}`; guest imports the bundle and calls `plugin.activate(apiProxy)`. Throws are caught, serialized as `IpcErrorEnvelope`, returned as the activate response. Host marks the plugin "crashed".
- **Suspend** — host posts `{sys:"suspend"}`; guest stops posting. Reserved; likely unused in Phase 3c.
- **Unload** — host posts `{sys:"unload"}`; guest runs `deactivate?.()`, acks, host removes iframe. 500ms grace then force-remove.
- **Crash** — detected by: (a) timeout on pending `req` (default 30s); (b) iframe `error` event; (c) null `contentWindow` post-navigation. On crash: reject every pending `req` with `plugin_crashed`; dispose every entry in `handle.subscriptions`; call `registry.unregisterAll(pluginId)` (existing WI-35 contract) to sweep commands/statusBar/views/activityBar; remove iframe; drop `handle` from the Map.
- **Watchdog.** 5s `ping`/`pong` heartbeat; two consecutive misses → crash.

### 5.6 Streaming / events

All subscriptions (`kernel.on`, `events.on`, `window.onResize`, `uri.register`, `commands.register`, `configuration.onChange`) share one pattern:

1. Guest generates `subscriptionId = uuid()`, stores `{ subscriptionId, handler }` locally.
2. Guest sends `req { method: "kernel.on", args: { topicPrefix, subscriptionId } }`.
3. Host allocates backing subscription (real `registry.track*`, `eventBus.on`), stores `{ subscriptionId, hostDispose }` in `handle.subscriptions`.
4. Host responds `res { ok:true, result:{ subscriptionId } }`. Guest proxy returns `() => rpcCall("unsubscribe", { subscriptionId })` to plugin code.
5. On event fire, host sends `evt { topic, subscriptionId, payload }`; guest dispatches by id.
6. Unsubscribe is idempotent on both sides (mirrors `PluginAPI.ts:268-279`) — plugin dispose and host teardown both flow through the same guarded `hostDispose()`.

**Backpressure.** Slow guest handlers back up the iframe event loop → heartbeat misses → watchdog kills the plugin. Intended failure mode. Additionally: cap in-flight `evt` per subscription at 256; `kernel.on` drops excess, `onResize` coalesces.

## 6. UI contribution problem

`ui.views.register(viewId, { component: ComponentType, slot, priority })` cannot cross `postMessage`: a React `ComponentType` is a function closing over JSX runtime, lexical scope, and DOM refs. Structured clone throws on functions. Options:

**(a) PanelNode-only.** Sandboxed plugins contribute declarative UI via `PanelNode` trees (`extension-api/src/index.ts:159-167`). API becomes `ui.views.register(viewId, { slot, render: PanelRenderFn, priority })`; host calls `render()` via RPC, receives a `PanelNode`, dispatches through the host-owned declarative renderer. Redraws driven by `api.views.invalidate(viewId)`.

**(b) Iframe-rendered view.** Plugin's view body is rendered *inside a second sandboxed iframe* the host mounts into the slot — generalization of `WebviewPanelConfig`. Plugin owns a DOM in another null-origin frame; events via a second postMessage channel. Heavy — each view adds an iframe — but unlocks rich UIs.

**(c) Sanitized HTML strings.** Plugin returns HTML, host sanitizes with DOMPurify. Cheapest, least safe, least useful (no interactivity without re-opening serialization).

**Recommendation: (a) as default, (b) reserved for Phase 4, (c) rejected.** PanelNode covers every use the current community plugin has (`hello-world` uses only `notifications` + `statusBar`); `MdxComponent` already proves the declarative round-trip (`extension-api/src/index.ts:184-189`). (b) waits until a concrete plugin demands it. (c) rejected — DOMPurify has a CVE history and sanitizer bypasses are an ongoing adversarial target.

## 7. Migration path for hello-world

`shell/src/plugins/community/hello-world/index.js` uses `notifications.show`, `commands.register`, `statusBar.createItem` — all three vanilla RPC calls (§5.2). No React, no view contributions, no direct Tauri imports.

- **Code changes.** Zero functional. The plugin's `activate(api)` sees the proxy instead of the real object; behaviour identical.
- **Manifest change.** Add `"apiVersion": 1` (WI-33 already needs it). New optional field `"sandbox": "strict"` opts into sandboxing; absence keeps legacy loader. Scanner carries both paths during transition.
- **Transition.**
  1. WI-30 ships: `hello-world` adds the manifest flag; new community plugins are born sandboxed.
  2. One release later: missing `sandbox` defaults to `"strict"`; opt-out via `"sandbox": "legacy"` during grace period.
  3. One release after that: `sandbox: "legacy"` is a hard error. First-party plugins keep the unsandboxed path — that is the Phase 4 boundary.

## 8. Implementation breakdown

| Task | Size | Notes |
|---|---|---|
| Host: `SandboxOrchestrator` (iframe lifecycle, handle map, crash detection, watchdog) | 3d | New module `shell/src/host/sandbox/Orchestrator.ts`. |
| Host: RPC protocol layer (envelope, correlation, method dispatch table) | 4d | `shell/src/host/sandbox/rpc.ts`. Largest single task. |
| Host: capability enforcement middleware | 1d | `METHOD_CAP_MAP` + `assertCap` guard. |
| Host: guest bootstrap (`srcdoc` template + `guestBridge.js`) | 2d | Generates the proxy `api` object from method catalog; tight. |
| Host: subscription bridging (kernel.on, events.on, onResize, configuration.onChange) | 2d | Ports the idempotent-unsub pattern. |
| Host: snapshot `workspace` + `viewRegistry` adapters | 1d | Read-only, polling + evt. |
| Host: PanelNode renderer wire-up for sandboxed views | 2d | `ui.views.register` path + invalidate. |
| CSP: re-enable in `tauri.conf.json`; allowlist shell resources + `frame-src` | 1d | Iterate with dev server; don't break HMR. |
| Plugin: migrate `hello-world` manifest + verify | 0.5d | Trivial. |
| Plugin: sandbox-plugin scaffold in `@nexus/extension-api` (docs + template) | 2d | Example + README. |
| Tests: iframe isolation smoke (tries to reach `window.parent.document`, expects TypeError) | 1d | Puppeteer or jsdom-with-real-iframe. |
| Tests: RPC roundtrip (command register → execute → result) | 1d | |
| Tests: capability denial (plugin without `FsRead` calls `platform.fs.readText`, expects `capability_denied`) | 1d | |
| Tests: crash recovery (plugin throws in activate; host cleans up) | 1d | |
| Tests: unload cleanup (subscriptions swept, iframe removed, registry clean) | 1d | |
| Tests: subscription idempotency (plugin + host both call unsub, no double-fire) | 0.5d | |

**Total: ~23 engineer-days ≈ 4.5 weeks.** Confidence: medium. The plan's "2+ weeks" is optimistic; realistic effort is 3–5 weeks depending on review cycles and how many hidden assumptions in `PluginRegistry.trackSubscription` need relaxing.

## 9. Risks

1. **Structured-clone perf on hot paths.** `api.context.get(key)` is synchronous today; under RPC it's 1–5ms. Plugins calling it per-keystroke will feel it. Mitigation: push context changes to guests via `evt` and let guest cache; defer until a hotspot appears.
2. **postMessage origin spoofing.** Identify sandboxes by `event.source === iframe.contentWindow`, never by origin string (which is `"null"`). Enforcement lives in the orchestrator's single message listener.
3. **Blob URL fingerprinting.** Minor; iframe has no network egress without capability, so local-only.
4. **React compat.** `hello-world` uses no JSX, but future plugins will want it and §6(a) forces PanelNode; authors will push back.
5. **Watchdog false positives.** Heavy sync work during activate could miss pings. Mitigation: heartbeat starts *after* activate resolves; activate has its own longer timeout.
6. **`unsafe-inline` in iframe CSP.** Required for srcdoc bootstrap. Acceptable because the iframe has no privileges; document as a deviation.
7. **Memory pressure.** Each iframe is a full JS realm. Fine for community-tier (<10 realistic); revisit if marketplace explodes.

## 10. Open questions

1. **CSP strictness in the host.** Block `eval` everywhere (including first-party plugins) or permit in the sandbox iframe only? **Recommendation:** permit `'unsafe-eval'` nowhere in the host; allow `'unsafe-inline'` in the iframe's internal CSP only for the srcdoc bootstrap. First-party plugins that currently use `eval` (grep needed — haven't checked) need to be audited as a pre-req.
2. **HTML-string view contributions.** Sanitize with DOMPurify (already in deps) or forbid entirely in favor of PanelNode? **Recommendation:** forbid in Phase 3c. Re-evaluate if a marketplace plugin demands it; introduce only via path §6(b), never §6(c).
3. **Sandbox protocol version vs `PLUGIN_API_VERSION`.** Tie together or keep separate? **Recommendation:** keep separate. ABI is the plugin-author contract; protocol is a host-internal concern. Plugins should never need to branch on protocol version.

## 11. Alternatives considered

**Realms shim (ShadowRealm / `@endo/ses`).** Per-plugin lockdown inside the same origin. Rejected: ShadowRealm is stage-3 and not shipped across Tauri's WebView targets; SES is a large runtime dep; same-origin still reaches `document.cookie`, `localStorage`, and `@tauri-apps/api/core` unless we wrap every global — an endless game. Iframe null-origin is one attribute.

**Web Workers.** No DOM access; plugins that contribute views can't run. Workers inherit parent origin so the Tauri bridge is still reachable. Wrong problem (parallelism vs. isolation).

**Service Workers.** Even more constrained; lifecycle owned by the browser. Not suitable for user-driven start/stop.

**QuickJS-in-WASM.** Strongest in-process isolation. Rejected: 400KB+ WASM per plugin, no DOM/React, second FFI to audit, strictly worse than our existing nexus-plugins WASM path. Iframes are the platform answer to this threat model.
