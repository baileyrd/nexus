# Sandbox Security Audit — 2026-05-18

> **Scope:** iframe + WASM sandbox escape paths, pluginId boundary binding, notification bridge. Phase 3 / P3-05 from the implementation plan.
>
> **Reviewer:** Claude (adversarial pass)
> **Codebase revision:** `3c877f22`

## Executive summary

The iframe sandbox is structurally sound: each community plugin gets its own null-origin iframe, its own `SandboxRouter`, its own `PluginAPI` instance (F-8.1.2), and identity is authenticated on every inbound frame via `event.source === iframe.contentWindow`. Cross-plugin impersonation through `postMessage` is not feasible — the host doesn't trust the wire-level `pluginId`; it trusts the closure-bound id on the router that owns that contentWindow.

That said, the audit found **2 High**, **5 Medium**, **4 Low**, and **3 Informational** findings. The High-severity items are (1) `kernel.on` is reachable with **no capability gate** and accepts arbitrary topic prefixes including the empty string, letting any community plugin shoulder-surf every event on the kernel bus; and (2) the WASM `host::read_file` path uses raw `Path::canonicalize` + `starts_with` instead of the hardened `ForgePathValidator` used by `host::write_file`, leaving the read path one symlink-race away from regressing the F-5.3.1 fix.

The capability-gating order is correct everywhere it matters — the iframe router rejects denied caps **before** the `api.*` surface runs (router.ts:510-520), and the WASM host functions all check `caller.data().capabilities.contains(…)` before touching any backing resource. The notification bridge `notify_desktop` is unreachable from a sandboxed iframe (Tauri's `__TAURI__` is not exposed across the null-origin boundary, and the only in-app path goes through the in-process toast queue, not OS notifications).

## Methodology

Read every file under `shell/src/host/sandbox/` plus the `RpcEnvelope` definition in `packages/nexus-extension-api/src/sandbox/protocol.ts`. Read `crates/nexus-plugins/src/host_fns.rs` end-to-end and skimmed `sandbox.rs` for the `PluginData` shape. Traced `notify_desktop` from the shell-side bridge command (`shell/src-tauri/src/lib.rs:557-583`) through the `nexus.notifications` plugin (`shell/src/plugins/nexus/notifications/index.ts`) to confirm a sandboxed plugin cannot reach the OS notification surface directly.

Threat model assumed: a malicious community plugin already installed (user accidentally clicked "trust") tries to escalate beyond its granted capabilities or impersonate another plugin. Nation-state attackers with native code execution on the host are out of scope per the brief.

## Findings

### S-01 — `kernel.on` has no capability gate and no topic-prefix sanitization [High]

**Vector.** A sandboxed community plugin with **no capabilities granted** can call `api.kernel.on('', handler)` and receive every kernel event from every other plugin — including events that carry sensitive payloads (e.g. AI prompts/responses, agent transcripts, file-change topics, settings mutations). The plugin learns nothing it could not learn by emitting `kernel.invoke` (which IS capability-gated), but it does so silently and without leaving an audit trail keyed to a denied capability.

**Code path.** `shell/src/host/sandbox/capabilityGuard.ts:65` — `'kernel.on': null` (no required cap). `shell/src/host/sandbox/router.ts:695-704` — passes `topicPrefix` verbatim to `api.kernel.on`. The comment at `capabilityGuard.ts:61-63` already acknowledges this is a placeholder waiting for a future `EventSubscribe` capability (WI-31).

**Recommendation.** Two layers:
1. Until the `EventSubscribe` capability lands, reject empty / whitespace-only / single-character `topicPrefix` values at `router.ts:696` so a plugin cannot fan-subscribe to every topic with a degenerate prefix. Require at least one dot (e.g. `com.nexus.notifications.`).
2. Add an `EventSubscribe` Capability variant in `nexus-plugin-api`, plumb it through `Capability::try_from_str`, and flip `'kernel.on': null` to `'EventSubscribe'`. Note this requires a migration for already-consented plugins — anything that currently uses `kernel.on` will start failing closed.

---

### S-02 — `host::read_file` does not use `ForgePathValidator` (regression risk of F-5.3.1) [High]

**Vector.** `host::write_file` correctly uses `path_validator.validate_for_write(...)` (host_fns.rs:450) which closes the canonicalize-parent-then-open TOCTOU. `host::read_file` (host_fns.rs:649-665) takes a different code path: it joins the requested path onto `forge_root`, calls `Path::canonicalize`, then prefix-checks against `forge_root` as-stored. This is the **pre-F-5.3.1 pattern**.

Concretely:
- The check is `canonical.starts_with(&forge_root)` (host_fns.rs:663). If `forge_root` was injected without canonicalization (e.g. via a path containing a symlinked ancestor on macOS where `/tmp` → `/private/tmp`), the prefix-match silently fails open or fails closed depending on which side got resolved.
- Even when both sides are canonicalized, `starts_with` does **path-component** matching by `PathBuf` semantics, which is correct on Unix but can have edge cases on Windows around case-folding (`C:\forge` vs `C:\FORGE`). The validator handles this; the inline path does not.
- There is no TOCTOU exposure because read is a single syscall after canonicalize, but the asymmetry with `write_file` is an unmaintained code path waiting to drift.

**Code path.** `crates/nexus-plugins/src/host_fns.rs:622-695` (`register_host_read_file`), specifically lines 649-665.

**Recommendation.** Extend `ForgePathValidator` with a `validate_for_read(&Path)` method that mirrors `validate_for_write` but skips the parent-mkdir consideration, and replace the inline canonicalize block at host_fns.rs:649-665 with a call to it. Until then, at minimum canonicalize `forge_root` at `PluginData` construction time so the prefix-check operands are both in canonical form.

---

### S-03 — `notifications.show` carries no plugin-attribution chrome [Medium]

**Vector.** A sandboxed plugin's notification body is rendered in the in-app toast queue with no visual indication of which plugin emitted it. A malicious plugin can post `"⚠ Nexus: your kernel session expired — re-enter your password at https://attacker.example/"` and the user sees an unattributed toast that looks indistinguishable from a first-party message. The `nexus.notifications` plugin's `composeToastMessage` (notifications/index.ts:56-60) even pre-strips the title `"Nexus"` if present, making impersonation cheaper.

The same vector applies to the kernel-event path: a sandboxed plugin with `EventsPublish` capability can emit `com.nexus.notifications.delivered` directly (via `api.events.emit`) and the in-app toast plus — when the window is backgrounded — the OS notification fire with arbitrary title/body. The OS notification surface (`notify_desktop`, lib.rs:557) does **not** prefix the source plugin id.

**Code path.**
- In-app: `shell/src/host/PluginAPI.ts:292-303` — the `notifications.show` builder receives the payload as-is and pushes to the `notificationQueue` service.
- OS-level (indirect): `shell/src/plugins/nexus/notifications/index.ts:78-92` subscribes to the kernel topic and forwards to `notify_desktop` with the payload's title/message unchanged.

**Recommendation.** When a notification originates from a community plugin, the toast/OS-notification renderer should prefix the plugin's display name (from manifest) in a non-overridable way. Reuse the manifest's `name` field; reject HTML/markdown in the message body if it isn't already plaintext-only. For OS notifications specifically, refuse `Channel::Desktop` emission unless the caller holds `UiNotify` AND the manifest is core OR explicitly granted a `NotifyDesktop` capability (which doesn't exist yet — see follow-ups).

---

### S-04 — `events.emit` allows a sandboxed plugin to forge events on **any** topic [Medium]

**Vector.** `router.ts:777-779` routes `events.emit` straight to `this.api.events.emit(event, payload)`. There is no namespace check; a community plugin can emit `'workspace:opened'`, `'plugin:error'`, `'com.nexus.notifications.delivered'`, etc., and the host treats them as authentic. Combined with S-03 this lets a malicious plugin produce convincing fake system notifications, fake editor saves, etc.

Note the WASM equivalent IS namespaced: `host::emit_event` (host_fns.rs:309-372) accepts a `type_id` argument and `event_bus.publish_plugin(&plugin_id, &type_id, …)` namespaces it under the caller's id on the kernel side. The iframe path bypasses that.

**Code path.** `shell/src/host/sandbox/router.ts:777-779`, `shell/src/host/PluginAPI.ts` event-bus wiring (no `pluginId` prefix on the emit path).

**Recommendation.** In the sandbox router, before forwarding to `api.events.emit`, require that `event` starts with the calling plugin's id (`${this.pluginId}.`) — or is in a whitelist of intra-process pub/sub topics that don't carry security significance (e.g. `plugin-private.*`). Reject everything else with a `capability_denied` so a malicious plugin can't forge `plugin:error`, `workspace:opened`, kernel topics, or first-party plugin's own events.

---

### S-05 — `commands.execute` runs **any** registered command id with full host privilege [Medium]

**Vector.** `router.ts:667-671` routes `commands.execute` directly to `api.commands.execute(cmdId, ...args)`. A sandboxed plugin can therefore invoke any first-party command — e.g. `plugins.mgmt.disablePlugin`, `agent.run`, `workspace.openFolder` — at full host privilege, because command handlers run in the shell-realm closure that registered them, not in the caller's sandbox.

This is qualitatively the same hole as the unrestricted `kernel.invoke` that the F-8.1.2 work plugged with `CORE_PLUGIN_NAMESPACE_PREFIX` (router.ts:84). The current code blocks `kernel.invoke` into `com.nexus.*` plugins but leaves the equivalent attack via `commands.execute` open, even though every core plugin registers commands.

**Code path.** `shell/src/host/sandbox/router.ts:667-671`. Compare with the explicit deny at `router.ts:690-692` for `kernel.invoke`.

**Recommendation.** Mirror the `kernel.invoke` deny: maintain a registry mapping command id → owning plugin id; if the owner is in the `com.nexus.*` namespace AND the caller is a sandboxed plugin, refuse with `capability_denied`. Alternatively, gate `commands.execute` behind a new `CommandsExecute` capability that the consent flow only proposes when the manifest declares which commands it intends to invoke. The simpler patch (namespace deny) is enough until a richer command-permission story is needed.

---

### S-06 — Host→plugin response ids are guessable and have no MAC [Medium]

**Vector.** `router.ts:324` mints host-to-plugin request ids as `host-req-${counter}-${Date.now()}`. Both the counter (monotonic from 0) and `Date.now()` are observable to the guest. A misbehaving guest can preemptively post `response` envelopes with fabricated ids (`host-req-1-…`, `host-req-2-…`) and — if it wins the race against the real handler — resolve the host's pending request with attacker-controlled data.

For commands, this means a sandboxed plugin's `dispatch.command` response can be forged by the **same** sandboxed plugin to itself, which is uninteresting (the plugin can already lie to the host caller about the return value). However, the same correlation pattern would let the guest plant a forged response **before** the host even sends the request: the guest could `postMessage` a `response` envelope with `id: 'host-req-1-1747569000000'` at handshake time, and when the host eventually mints that id the entry would already not be in `pendingHostRequests` so it lands in the `warn('unexpected response frame from guest')` branch (router.ts:353). So the actual exploitation requires winning a race after the host posts.

Risk is bounded because:
- `pendingHostRequests` is keyed on the exact id; only a response with the right id resolves the promise. If the guest beats the real handler to it, the host caller (e.g. `commands.execute` -> `dispatch.command`) gets the guest's return value — which is what the guest controls anyway because it owns the handler.
- The realistic attack is on `renderPanel` (`SandboxOrchestrator.ts:436`): a guest's normal `views.render` response can be intercepted/replaced if a second guest message races it. Same closure though — both messages come from the same iframe.

**Code path.** `shell/src/host/sandbox/router.ts:308-348` (`sendRequest`) and `:350-363` (`handleHostResponse`).

**Recommendation.** Use a UUID for the host→plugin request id, the same way the guest does. The collision/guess surface drops from a 32-bit counter to 122 bits and the timestamp prefix becomes unnecessary. Trivial change at router.ts:324.

---

### S-07 — `platform.shell.openExternal` has no capability gate or URL allowlist [Medium]

**Vector.** A sandboxed plugin can open arbitrary URLs in the user's default browser without any consent prompt. This is data-exfiltration adjacent (the destination server sees the user's IP, browser fingerprint, optional query-string-encoded data), and also a vector for spoofing — opening `https://nexus-update-required.example.com/` looks like a legitimate "your kernel needs an update" prompt.

**Code path.** `shell/src/host/sandbox/capabilityGuard.ts:96` — `'platform.shell.openExternal': null`. `shell/src/host/sandbox/router.ts:758-760` — passes the target string verbatim to `api.platform.shell.openExternal`. The comment at capabilityGuard.ts:93-95 already flags this as a placeholder.

**Recommendation.** Add a `ShellOpen` capability variant and flip the guard. Reject URLs whose scheme is not `https:` or `mailto:` unless the user has explicitly granted a `ShellOpenAnyScheme` capability (so a plugin can't `file:///etc/passwd` the user's default text editor or `nexus://malicious-deep-link` themselves).

---

### S-08 — F-8.1.2 pluginId boundary binding: NO ACTION — verified correct [Informational]

**Vector verified.** The "plugin A impersonates plugin B" attack would require A to post a frame that gets routed through B's router. This is not possible because:

1. Every sandboxed plugin is in its own iframe (`SandboxOrchestrator.load` enforces uniqueness at `:231-235`).
2. Each iframe gets its own `IframePort` whose `windowListener` checks `ev.source !== this.iframe.contentWindow` and silently drops everything else (IframePort.ts:120).
3. The `event.source` check authenticates by JS-object identity — A's `contentWindow` is a different Window object from B's, and neither can be obtained from the other (the parent window holds both refs in private fields; no `__TAURI__`-like global exposes them).
4. The `pluginId` is set once in `SandboxOrchestrator.load` (`:230-238`) from the orchestrator-controlled `spec.pluginId`, flows into the router via the closure at `:397`, and is **never re-read from any inbound frame**. The `RpcEnvelope` shape (protocol.ts:108-115) has no `pluginId` field — a guest can't even claim a wrong one.
5. The per-plugin `PluginAPI` instance (`main.tsx:322-328`) is built from the orchestrator-set id; `assertValidPluginId` (PluginAPI.ts:65-82) rejects colons, empty strings, and non-strings before any derived key (`plugin:<id>:<key>` localStorage namespace, event tags, registry-track keys) is written.

The `event.source` check survives one realistic edge case: a popup opened by the iframe via `window.open` (would need `allow-popups` in sandbox attribute, which is **not set** — SandboxOrchestrator.ts:327 only grants `allow-scripts`). So no popup attack surface.

**No action.** This is what the spec promises and what the code delivers.

---

### S-09 — WASM `host_fns.rs` error-path information leakage: minimal, but log lines carry full paths [Low]

**Vector.** Every error path in `host_fns.rs` returns a single `i32` (`HOST_ERROR`, `HOST_CAPABILITY_DENIED`, `HOST_BUFFER_OVERFLOW`). The plugin learns nothing about *why* the call failed — no host path strings, no errno, no symlink resolution detail crosses the boundary. That's the right shape.

However, the **tracing log lines** that fire on those error paths do contain rich detail:
- host_fns.rs:657 — `"host::read_file: canonicalize failed: {e}"` — `e` includes the full requested path resolved against `forge_root`.
- host_fns.rs:444 — `"host::read_file: no path validator configured"` (write path; informational only).
- host_fns.rs:471 — `"host::write_file: invalid path '{}': {msg}"` — includes `requested.display()` and validator message.

These logs go to the kernel's `tracing` subscriber and to `audit::log_*` for capability denials, never to the WASM plugin. So no information leakage across the sandbox boundary itself; the audit log surface is internal.

**Code path.** `crates/nexus-plugins/src/host_fns.rs` — every `tracing::warn!(plugin_id = %plugin_id, …)` line.

**Recommendation.** No action. Logs intentionally carry context for the operator. If audit-log content ever becomes plugin-readable (e.g. a future "show my plugin's denied calls" UI), revisit and redact paths to relative-only form.

---

### S-10 — WASM host functions never panic across the boundary: verified correct [Informational]

**Vector verified.** Every memory access goes through `read_wasm_bytes` / `read_wasm_str` (host_fns.rs:74-86) which return `Option` and use `checked_add`. Every `usize::try_from(i32)` is in a `let Ok(…) else` block. Every `serde_json::from_slice` is matched and returns `HOST_ERROR` on failure. Every `Mutex::lock` is matched (host_fns.rs:104). Every `RwLock::read` is matched (host_fns.rs:730). `String::from_utf8_lossy` is used in `host::log` so even invalid UTF-8 cannot trigger a panic.

No `unwrap` / `expect` / array-index-without-bounds-check in any host function. No `panic!` reachable from a plugin-supplied input. No double-free path (WASM linear memory is `&[u8]` borrowed from `caller.data()` and never freed by host code).

Wasmtime's own boundary is panic-safe by construction (it catches `wasmtime::Trap` from any guest panic and converts to `PluginError`).

**No action.** Verified clean.

---

### S-11 — Iframe srcdoc inline script uses `'unsafe-inline'` in CSP [Low]

**Vector.** The sandbox iframe's CSP (`SandboxOrchestrator.ts:183`) is `default-src 'none'; script-src 'self' blob: data: 'unsafe-inline'`. The `'unsafe-inline'` is required because the srcdoc itself contains the bootstrap `<script type="module">` (SandboxOrchestrator.ts:186-212). This is functionally necessary; the worry is that future maintenance accidentally introduces a second inline script that an attacker could plant via the bundle's `init()` (e.g. by mutating `document.body.innerHTML`).

The realistic attack: a malicious plugin's `init()` calls `document.write("<script>...</script>")` and gets it executed inside the iframe. This is the **same** principal as the plugin itself (the iframe's null-origin) — it can't escape the sandbox, it can only do what it already could via its own bundle. So the practical impact is zero; the concern is purely defense-in-depth.

**Code path.** `shell/src/host/sandbox/SandboxOrchestrator.ts:170-215`.

**Recommendation.** No action required. If a future hardening pass wants to remove `'unsafe-inline'`, switch the bootstrap to a static blob URL imported by `script-src blob:` and drop the inline keyword.

---

### S-12 — Handshake timeout default of 5s gives slow guests a "permanent timeout" state [Low]

**Vector.** `DEFAULT_HANDSHAKE_TIMEOUT_MS = 5_000` (SandboxOrchestrator.ts:141). A guest that imports a large bundle (multi-MB) over a slow network or a CPU-throttled tab can exceed 5s; the orchestrator's `awaitHandshake` returns `'timeout'` and `start()` throws (`:417-426`). The instance is then `dispose()`d (orchestrator.ts:248) — but the iframe-side script may still be running and may still complete the import after the host has torn down its router. The orphan script can `postMessage` for a few hundred ms before `removeChild` finishes the GC; those frames go to the dead router and are dropped at `IframePort.ts:113` (`if (this.closed) return`).

Not a security issue — just an availability one. Listed for completeness.

**Code path.** `shell/src/host/sandbox/SandboxOrchestrator.ts:141`, `:448-487`.

**Recommendation.** No action. The dispose path correctly disarms the port, and dropped post-teardown frames are observed in the unit tests.

---

### S-13 — `notify_desktop` capability ordering: NO ACTION — verified unreachable from sandbox [Informational]

**Vector verified.** A sandboxed iframe with `sandbox="allow-scripts"` (no `allow-same-origin`) cannot call `__TAURI__.invoke('notify_desktop', …)` because:

1. The iframe loads at a null origin; Tauri's webview-inject of the `__TAURI__` global only runs in the main webview, not in nested iframes spawned with a sandbox that strips same-origin.
2. The only paths from a sandboxed plugin to the bridge command go through:
   - `notifications.show` → in-process `notificationQueue` service → in-app toast. Never reaches `notify_desktop`.
   - `events.emit('com.nexus.notifications.delivered', …)` → the `nexus.notifications` plugin's kernel subscriber (notifications/index.ts:78) → `notify_desktop`. This **does** reach the OS notification, but only when `!document.hasFocus()` (notifications/index.ts:89). See S-04 for the missing namespace check on `events.emit` and S-03 for the missing attribution prefix.

The `notify_desktop` Tauri command itself (lib.rs:557-583) does not authenticate its caller — it trusts whoever calls `invoke('notify_desktop', …)` from the main webview. That's by-design within the trust boundary; the main webview is "Nexus" and any plugin reaching `invoke` from there is already pre-trust-checked.

**No action.** The capability check effectively fires at the sandbox boundary (`notifications.show` requires `UiNotify`, capabilityGuard.ts:110) which is **before** the in-app toast push and well before any OS-level notification path. S-03 + S-04 cover the residual attribution/forgery concerns.

---

## Out of scope / explicitly NOT examined

- The Rust kernel's `Capability` enum semantics. Trusted as a black box.
- The consent flow in `runInstallTimeConsent` (main.tsx:290) — is the install prompt fishable? Out of scope for this audit; tracked separately.
- The community plugin manifest signing flow (`crates/nexus-plugins/src/signing.rs`). The audit assumed the user is willing to install a malicious plugin; signing doesn't change the post-install threat model.
- The Tauri allowlist beyond `notify_desktop`. The bridge command list at lib.rs:712-739 was scanned but not individually audited.
- WASM memory bombs / resource exhaustion. Wasmtime fuel/epoch limits are configured in `sandbox.rs` and considered out of scope for this pass.
- React `dangerouslySetInnerHTML` in PanelNode rendering. The design doc claims sanitization; the renderer code was not re-audited.

## Recommended follow-ups

Ranked by adversarial leverage per unit of fix effort:

1. **S-04 (High-leverage Medium):** Namespace-check `events.emit` in the sandbox router to prevent forged system events. ~10 lines; closes a phishing primitive that compounds S-03.
2. **S-05:** Block `commands.execute` of `com.nexus.*`-owned commands from sandboxed plugins, mirroring the `kernel.invoke` deny pattern. ~20 lines.
3. **S-01:** Add an `EventSubscribe` capability and gate `kernel.on`. Bigger change because it requires a plugin-api enum addition and a migration for any plugin already using `kernel.on`, but closes the broad-event-sniffing vector.
4. **S-02:** Add `ForgePathValidator::validate_for_read` and migrate `host::read_file` to use it. Eliminates the asymmetry with `host::write_file`.
5. **S-07:** Add `ShellOpen` capability and require an explicit grant for `platform.shell.openExternal`.
6. **S-03:** Prefix all sandbox-originated notifications (in-app and OS) with the plugin's manifest name in a non-overridable chrome.
7. **S-06:** Switch host→plugin request ids from `host-req-${n}-${ts}` to UUIDs. One-line change.
