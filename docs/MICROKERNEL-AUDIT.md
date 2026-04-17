# Nexus Micro-Kernel Architecture Audit

**Auditor:** Micro-Kernel Architecture Auditor (per `microkernel-auditor.md`)
**Date:** 2026-04-16
**Trust model assumed:** Third-party, untrusted community plugins
**Scope:** Full 10-dimension audit

---

## System Under Audit

**Nexus** is an AI-native, plugin-extensible knowledge environment implemented as a Rust workspace of 17 crates plus a Tauri/React frontend. Its architecture is explicitly marketed as a microkernel: a small kernel crate (`nexus-kernel`) owns the event bus, KV store, plugin context, and capability enum; a separate plugin subsystem (`nexus-plugins`) owns discovery, manifest validation, WASM sandboxing, hot-reload, settings, and IPC; everything else — storage, AI, editor, theme, database, security — is itself a "core plugin" registered at bootstrap (`nexus-bootstrap`).

Three plugin tiers exist:

1. **Core plugins** — native Rust implementing `CorePlugin` trait, registered at bootstrap, auto-granted every capability (`Capability::ALL`). Examples: `com.nexus.storage`, `com.nexus.ai`, `com.nexus.editor`, `com.nexus.theme`, `com.nexus.database`, `com.nexus.security`.
2. **Community plugins** — WASM modules executed in `wasmtime` 42 with fuel + memory limits, capability-gated via a 14-variant enum (`fs.read`, `fs.write`, `net.http`, `ipc.call`, `ui.notify`, etc.).
3. **Script plugins** — JavaScript modules executed in the Tauri WebView; backend treats them as a marker variant and refuses to dispatch to them.

Extension points declared in manifests: `cli_subcommand`, `ipc_command`, `event_subscriber`, `ui_command`, `ui_panel`, `ui_settings_tab`, `ui_ribbon_item`, `ui_status_item`, `slash_command`, `menu_item`, `uri_handler`.

---

## Executive Summary

Nexus is a surprisingly mature microkernel for an alpha project. The capability system is cleanly typed, event-namespace anti-spoofing works, the WASM sandbox applies both fuel and memory limits, path confinement uses canonicalization + prefix comparison, and the error hierarchy is thorough. Observability (structured `tracing`, dedicated audit helpers) is well above average for this stage. The separation between "kernel" and "plugins" is semantically sound even though physically the two crates are tightly coupled.

Against a **third-party-untrusted** trust model, however, five structural gaps matter:

1. **No install-time capability gate.** `nexus-security::risk_level` classifies half the capabilities as HIGH risk (net, external FS, process spawn, IPC), but the loader grants the union of `required + optional` with no user prompt. Any community plugin that requests `net.http` gets `net.http`. The risk metadata is a dead code path today.
2. **Write-side path confinement is TOCTOU-vulnerable.** `host::write_file` canonicalizes the *parent* directory, then writes to the non-canonical absolute path. A malicious plugin can race a symlink into place between those two operations.
3. **No separate plugin contract crate.** `nexus-plugins` depends directly on `nexus-kernel`, so the plugin ABI is whatever the kernel exposes at any given moment. Compiling a community plugin against a specific Nexus version is not possible without committing to the full kernel crate.
4. **`api_version` is parsed but never checked.** The field exists in every manifest; there is no load-time comparison to a supported version range.
5. **Script plugins entirely bypass the backend capability system.** A script-tier plugin runs in the Tauri WebView and is trusted by virtue of being loaded at all; there is no capability set enforced over its reach. For untrusted scripts this is a complete escape hatch.

Additional notable issues: the kernel's `start()`/`shutdown()` are no-ops with real lifecycle logic living in `PluginManager`; two plugin registries exist in parallel (`PluginRegistry` in kernel is empty, `PluginLoader::loaded` is authoritative); `handler_id` uniqueness validation skips `uri_handler`; reentrancy detection surfaces as `CapabilityDenied` (wrong taxonomy); `EventFilter::Variant` takes `&'static str` which makes it unreachable from manifest-declared filters.

Counting severities across all 10 dimensions: **3 red (🔴)**, **8 orange (🟠)**, **10 yellow (🟡)**, **6 green (🟢)**, **7 strengths (✅)**.

This audit would trust Nexus with **first-party plugins today** but would not ship a public plugin marketplace without addressing the 3 red findings (F-5.1.1, F-5.3.1, F-2.1.1) and at least F-6.1.1 and F-9.2.1 (api_version).

---

## Findings

### 1. Kernel Scope & Boundaries

The "kernel" here is `nexus-kernel`. What it owns in-process: capability enum, event bus (`tokio::sync::broadcast`), KV store trait, plugin context trait + concrete `KernelPluginContext`, plugin registry (unused at runtime), `IpcDispatcher` trait, typed error hierarchy, and a `Kernel` struct whose `start`/`shutdown` methods are placeholders.

**F-1.1.1 — Kernel `start()` and `shutdown()` are documented no-ops; real lifecycle lives elsewhere** 🟠

Evidence: `crates/nexus-kernel/src/kernel.rs:100-113` (start: `"plugin discovery not yet implemented; starting with empty plugin set"`); `:126-145` (shutdown: `"no plugins to stop; no storage to flush"`). The doc-comment says `nexus-plugins` will fill it in; `nexus-plugins::PluginManager::load_all` and `shutdown` actually perform the work (`crates/nexus-plugins/src/lib.rs`).

Why it matters: a caller reading `Kernel` in isolation would reasonably believe it orchestrates the plugin lifecycle. It does not. Lifecycle is distributed across at least three independent subsystems (`Kernel`, `PluginLoader`, `PluginManager`) with no single point of truth. This is not just a cosmetic split — if a future `Kernel::start` *does* grow logic, it may miss the live path.

Fix: Either (a) delete `Kernel::start`/`shutdown` and rename the struct to `KernelHandles` since it is just a holder, or (b) have `Kernel::start` actually invoke `PluginManager::load_all` so there is one entry point. Prefer (b).

**F-1.2.1 — Two parallel plugin registries; the kernel's is dead code** 🟡

Evidence: `crates/nexus-kernel/src/plugin_registry.rs` declares `PluginRegistry` with `pub(crate)` mutators (`upsert`, `remove`) — the kernel never calls them. `crates/nexus-plugins/src/loader.rs` maintains the real `loaded: HashMap<String, …>`. `Kernel::plugins()` always returns an empty registry (`crates/nexus-kernel/src/kernel.rs:154`; test at `:219-224` asserts emptiness).

Why it matters: introspection commands (`nexus plugin list` in the CLI) must remember to consult `PluginLoader` rather than `Kernel::plugins()`. Any new developer is likely to use the more prominent kernel API and produce silently-empty output.

Fix: Make `Kernel::plugins()` delegate to the `PluginLoader` (via an `Arc<dyn PluginRegistryReader>` injected at `Kernel::new`), or remove the kernel-side registry entirely.

**F-1.3.1 — Capability list and plugin-facing traits live inside the kernel crate** 🟡

Evidence: `crates/nexus-kernel/src/lib.rs` re-exports `Capability`, `CapabilitySet`, `PluginContext`, `EventBus`, `IpcDispatcher`, `PluginInfo`, `TrustLevel`, error types. `crates/nexus-plugins/Cargo.toml:10` depends on `nexus-kernel` directly.

Why it matters: kernel and plugins are one compilation unit from the plugin's POV. A community plugin that links against `nexus-kernel` to use `Capability` transitively imports every kernel internal. Any change to `event_bus.rs` or `kv_store.rs` is a plugin-ABI change, even if it does not touch `PluginContext`. This also blocks the kernel from growing private logic without polluting the plugin contract. See F-2.1.1 for the dimension-2 implications.

Fix: Extract `Capability`, `CapabilitySet`, `PluginContext`, `PluginInfo`, `TrustLevel`, `IpcDispatcher`, `EventFilter`, `NexusEvent`, and the narrow error variants plugins observe into a `nexus-plugin-api` crate. Both `nexus-kernel` and `nexus-plugins` (and plugin authors) depend on it. The kernel is then free to grow internals.

**F-1.4.1 — Bootstrap reaches into plugin implementations** 🟢

Evidence: `crates/nexus-bootstrap/src/lib.rs:237-332` hardcodes every handler_id constant from `nexus-storage`, `nexus-ai`, `nexus-editor`, `nexus-theme`, `nexus-database` to build their manifests programmatically.

Why it matters: bootstrap is effectively the marshalling layer for core plugins. The constants are stable, but the coupling means bootstrap cannot be generic over "any core plugin" — adding a new core plugin requires editing bootstrap. For a microkernel this is acceptable as long as community plugins never take this path; for M1 they do not, so severity is 🟢 with a suggestion.

Suggestion: Introduce a `CorePluginRegistration { manifest: PluginManifest, plugin: Box<dyn CorePlugin> }` constructor on each core-plugin crate so bootstrap just iterates `&[register_security(), register_storage(), …]`.

---

### 2. Contract Design

**F-2.1.1 — No separate plugin-contract crate; community plugins cannot pin a stable ABI** 🔴

Evidence: `crates/nexus-plugins/Cargo.toml:10` (direct path dependency on `nexus-kernel`); `crates/nexus-kernel/src/lib.rs` (kernel re-exports its own internals as the plugin-facing surface); no `nexus-plugin-api` crate exists anywhere in the workspace.

Why it matters: the plugin ABI is defined by the kernel's public API. Every kernel refactor is a plugin-compatibility-break unless performed with care that is not yet documented anywhere. For an untrusted-plugin ecosystem this is the single most important structural issue — without a stable contract surface, the marketplace model that the README promises is not feasible.

Fix: Establish `nexus-plugin-api` containing: `Capability`, `CapabilitySet`, `TrustLevel`, `PluginInfo`, `PluginContext` (trait), `CorePlugin` (trait), `IpcDispatcher` (trait), `EventFilter`, `NexusEvent` (as a stable JSON-oriented type rather than leaking broadcast semantics), `PluginError` variants that plugins observe, and `PLUGIN_API_VERSION: u32`. Document compatibility via semver plus `api_version` handshake (see F-9.2.1).

**F-2.2.1 — `PluginContext` mixes sync and async methods inconsistently** 🟠

Evidence: `crates/nexus-kernel/src/context.rs` — `read_file`/`write_file`/`kv_get`/`kv_set`/`ipc_call` are `async`, but `publish`/`subscribe`/`log`/`plugin_id`/`has_capability` are sync. `#[async_trait]` is used, so the trait object is already heap-allocated for the async methods.

Why it matters: community WASM plugins cannot `.await` — they are CPU-serial within `wasmtime::Store`. The context trait is therefore invoked from the *host*, not from WASM, and its async methods exist primarily for the native `CorePlugin` invokers (CLI/TUI). But the loader's `dispatch_ipc` path already uses `spawn_blocking` + `tokio::time::timeout` to bridge the sync/async gap in both directions, which is evidence this split has caused pain. See also F-8.3.1.

Fix: Split `PluginContext` into `HostPluginContext` (async, for native invokers) and a host-function–backed ABI for WASM (already effectively the `host::*` functions). This makes the async/sync split intentional per audience.

**F-2.2.2 — No `Send + Sync` bound on `CorePlugin`** 🟡

Evidence: `crates/nexus-plugins/src/loader.rs` — `CorePlugin` trait is dispatched behind `Arc<Mutex<PluginBackend>>`; the `Mutex` provides `Send`, so each call site currently works. But the trait itself has no explicit `Send + Sync` requirement.

Why it matters: a future `CorePlugin` implementation holding `Rc<…>` state compiles cleanly in isolation; the wiring breaks only at the point the loader tries to store it. The mismatch surfaces as confusing error messages downstream.

Fix: `pub trait CorePlugin: Send + Sync { … }`.

**F-2.3.1 — Context trait object is used correctly** ✅

Evidence: `crates/nexus-kernel/src/context.rs` defines `PluginContext` as an object-safe trait; call sites use `&dyn PluginContext` (`crates/nexus-bootstrap/src/lib.rs:150-196`). No generics pollute the trait signatures.

---

### 3. Plugin Manifest & Discovery

**F-3.1.1 — Manifest schema is well-defined and validated** ✅

Evidence: `crates/nexus-plugins/src/manifest.rs:1129-1319` (`validate` function). Checks: reverse-DNS regex for ID (line 1134), semver for version (line 1146), known capability names (line 1152-1162), handler_id uniqueness (line 1164-1213), mutual exclusion of `[wasm]` / `[script]` (line 1218-1234), `memory_mb ∈ [1, 256]` (line 1238), `fuel > 0` for community (line 1249), referenced WASM / script / schema files exist (line 1257, 1271, 1306).

**F-3.2.1 — `handler_id` uniqueness check omits `uri_handler`** 🟠

Evidence: `crates/nexus-plugins/src/manifest.rs:1164-1213` chains iterators over `cli_subcommands`, `ipc_commands`, `event_subscribers`, `ui_commands`, `ui_panels`, `ui_settings_tabs`. The `uri_handlers` vec — whose entries carry a `handler_id: u32` (line 323, 539) — is missing from the chain.

Why it matters: a manifest can legally declare `handler_id = 103` for both a UI command and a URI handler. The dispatch table is keyed by `handler_id`, so which handler runs depends on the order of registration. This is a latent correctness bug waiting to be surfaced.

Fix: Add `.chain(manifest.registrations.uri_handlers.iter().map(|r| r.handler_id))` to the iterator chain at line 1205.

**F-3.2.2 — `TrustLevel` deserialized from manifest without signature or provenance check** 🟠

Evidence: `crates/nexus-plugins/src/manifest.rs:572-584` (manifest-to-struct conversion simply copies `trust_level` string through). The loader rejects a `trust_level = "core"` manifest submitted to `PluginLoader::load` (`crates/nexus-plugins/src/loader.rs:384-391` — "core plugins must be registered via PluginLoader::register_core()"), but only because `register_core` is an internal API.

Why it matters: nothing on disk authenticates that a manifest came from a trusted source. A malicious community plugin author can claim `trust_level = "community"` and receive a restricted capability set. This is fine today (community already maps to restricted) but becomes important the moment any new trust level (e.g., "signed") is added, and it blocks future directory-signing or plugin-store integrity work.

Fix: For M2+, require a detached signature in the plugin dir (`manifest.toml.sig`) validated against a trusted key ring when `trust_level != "community"`. Document the threat model in `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`.

**F-3.3.1 — Discovery path is hard-coded in bootstrap; not reachable by the kernel** 🟡

Evidence: `crates/nexus-bootstrap/src/lib.rs:122` — `let plugins_dir = forge_root.join(".forge").join("plugins");`. There is no kernel-level discovery; `Kernel::start` never sees it.

Why it matters: a user who configures a custom `plugin_search_paths` in `.nexus/config.toml` (which the kernel *does* load — `crates/nexus-kernel/src/config.rs`) will have those paths ignored by the live plugin subsystem, because bootstrap hardcodes a different path.

Fix: `PluginLoader::new(&plugins_dir)` should accept a slice of search paths from `KernelConfig`, or the kernel should own the loader directly.

**F-3.3.2 — Duplicate-ID rejection present** ✅

Evidence: `crates/nexus-plugins/src/loader.rs` — `load()` rejects if `self.loaded.contains_key(&manifest.id)` (returns `PluginError::DuplicatePlugin`). Same guard in `register_core`.

---

### 4. Lifecycle Management

**F-4.1.1 — Lifecycle hook set is comprehensive** ✅

Evidence: `crates/nexus-plugins/src/sandbox.rs:282-363` defines 8 lifecycle hooks — `on_init` (0), `on_start` (1), `on_stop` (2), `on_load` (3), `on_enable` (4), `on_disable` (5), `on_unload` (6), `on_settings_changed` (7). Fuel exhaustion during lifecycle is mapped to `PluginError::LifecycleError` (line 398-408).

**F-4.1.2 — Lifecycle transitions persist through the `PluginInfo::status` enum** ✅

Evidence: `crates/nexus-kernel/src/plugin.rs` (`PluginStatus::{Loaded, Initialized, Running, Stopped, Crashed}`). `PluginLoader` updates the status on transitions; `Crashed` set in `reload_plugin` when the new sandbox fails to build (`loader.rs`).

**F-4.2.1 — Per-plugin failures are swallowed as warnings rather than cascading** ✅

Evidence: `crates/nexus-plugins/src/lib.rs` — `PluginManager::load_all` iterates plugin dirs and logs `warn!` on each failure, continuing to the next. No single misbehaving plugin can take the system down.

**F-4.2.2 — Shutdown order is not deterministic** 🟡

Evidence: `crates/nexus-plugins/src/lib.rs` — `shutdown` iterates `self.loader.all_plugin_ids()` in hashmap order; each `on_stop` failure is logged but execution continues.

Why it matters: if plugin B subscribes to events from plugin A and B is stopped after A, the bus publish path is silently lost. This is acceptable during shutdown but makes debugging "why did my shutdown hook not run" harder.

Fix: Stop plugins in reverse-registration order, and emit an audit event for each `on_stop` failure in addition to the `warn!`.

**F-4.3.1 — `reload_plugin` is best-effort and can leave plugins in `Crashed`** 🟡

Evidence: `crates/nexus-plugins/src/loader.rs` — `reload_plugin` calls `on_stop` on the old sandbox (best-effort), builds a new sandbox, and sets `PluginStatus::Crashed` if anything fails. No retry, no rollback.

Why it matters: a transient filesystem hiccup during hot-reload (file still being written) can put a plugin into `Crashed` until the next manual reload. The notify-debouncer-mini's debounce window helps but cannot prevent racing a mid-write state. Explicit retry with a short backoff would be more user-friendly.

Fix: Retry the new-sandbox build once after a 100ms backoff; fall back to keeping the previous sandbox if both attempts fail (the plugin is still usable even if updates are delayed).

**F-4.4.1 — `PluginReloading` state is declared but lookups don't respect it** 🟡

Evidence: `crates/nexus-plugins/src/error.rs:121-124` declares `PluginError::PluginReloading`. Grep: only ever constructed in tests; the live reload path (`reload_plugin`) goes straight from old → new without emitting the intermediate state to callers of `dispatch_ipc`.

Why it matters: a plugin-to-plugin IPC call that arrives mid-reload races the `Arc<Mutex<PluginBackend>>` and may either hit a stale sandbox or a brief `TryLock` failure reported as `CapabilityDenied { capability: "ipc.call (re-entrant / circular call detected)" }` (see F-5.2.1).

Fix: Set a `reloading: AtomicBool` per plugin, return `PluginError::PluginReloading` from `dispatch_ipc_checked` when set.

---

### 5. Isolation Boundary

**F-5.1.1 — No install-time capability prompt; all requested capabilities are auto-granted** 🔴

Evidence: `crates/nexus-plugins/src/loader.rs:1222-1236` — `build_capabilities` for `TrustLevel::Community` returns `CapabilitySet::from_iter(required + optional)` with no user interaction. `crates/nexus-security/src/risk.rs:44-69` classifies `NetHttp`, `FsReadExternal`, `FsWriteExternal`, `ProcessSpawn`, `IpcCall` as `RiskLevel::High` — a classification that nothing in the loader consumes. `README.md` and the `TrustLevel::Community` doc comment promise install-time approval for HIGH-risk caps; the promise is not kept.

Why it matters: under an untrusted trust model, this is the single most important check. A plugin that declares `required = ["net.http", "fs.read.external", "process.spawn"]` is granted all three on first load, silently, with no `AuditLog::capability_granted` event from the user's perspective.

Fix:
1. At `PluginLoader::load`, after validation, compute `high_risk_caps = manifest.required ∪ manifest.optional |> filter(risk_level == High)`.
2. If non-empty, emit a `com.nexus.security.capability_prompt` event carrying the plugin id and the caps; block load until the user responds.
3. Persist the decision to `<plugin_dir>/granted_caps.json` so prompts are one-time per (plugin, version). On version change, re-prompt for any newly-requested HIGH-risk cap.
4. `optional` capabilities should require `risk_level == Low | Medium` or they collapse into `required` semantics.

**F-5.1.2 — Core plugins hold `Capability::ALL`; bootstrap grants unrestricted reach** 🟢

Evidence: `crates/nexus-plugins/src/loader.rs:1224` — `TrustLevel::Core => CapabilitySet::from_iter(Capability::ALL.iter().copied())`.

Why it matters: acceptable under the current trust model (core plugins are in-tree and reviewed), but the CLI/TUI invoker is also a core plugin (`nexus-bootstrap`) and therefore inherits full reach. If a bug in CLI arg parsing led to injection into an `ipc_call`, there is no capability boundary to catch it.

Suggestion: Define a `TrustLevel::Invoker` that holds all capabilities *except* `FsWriteExternal` and `ProcessSpawn`, and assign CLI/TUI to it.

**F-5.2.1 — IPC reentrancy surfaces as `CapabilityDenied` (wrong error taxonomy)** 🟠

Evidence: `crates/nexus-plugins/src/loader.rs` `SharedPluginLoader::dispatch` uses `try_lock` on the per-plugin backend mutex; failure returns `IpcError::CapabilityDenied { capability: "ipc.call (re-entrant / circular call detected)" }`. The error variant exists specifically for "plugin lacks the capability" and is now overloaded to mean "can't acquire the backend lock".

Why it matters: observability, debugging, and recovery logic are all keyed off error variants. A plugin that receives `CapabilityDenied` with the unusual capability string may attempt to request the cap (e.g., by updating its manifest) — an action that cannot fix reentrancy.

Fix: Introduce `IpcError::ReentrantCall { plugin_id, command }`. The message is self-documenting; no overload of an unrelated variant.

**F-5.3.1 — `host::write_file` has a TOCTOU window between parent canonicalization and write** 🔴

Evidence: `crates/nexus-plugins/src/host_fns.rs:348-426`.
```
let parent = absolute.parent().unwrap_or(&absolute);
let canon_parent = parent.canonicalize() … ;     // line 386
if !canon_parent.starts_with(&forge_root) { … }   // line 402
…
std::fs::write(&absolute, &data)                  // line 412 — uses non-canonical `absolute`
```
`absolute` is not re-resolved after the parent check. If a malicious plugin (or a collaborating process) replaces `parent` with a symlink to `/etc` between lines 402 and 412, the write lands outside the forge root even though the check passed.

Why it matters: against an untrusted plugin with any form of `FsWrite`, this is a sandbox escape. The severity is reduced by the fact that `FsWrite` is already capability-gated — but that is precisely the wrong assumption for untrusted plugins in this trust model.

Fix: Resolve the *target file* canonically after `create_dir_all`, and open it with `OpenOptions::new().write(true).create(true).open(canonical_target)`. Even better: delegate to `ForgePathValidator` (`crates/nexus-security/src/path.rs`), which already does the right thing and whose tests include symlink-escape scenarios (`symlink_outside_root_is_rejected`). **`ForgePathValidator` is written but not used by the host functions.**

**F-5.3.2 — `KernelPluginContext::write_file` has the same TOCTOU pattern** 🟠

Evidence: `crates/nexus-kernel/src/context_impl.rs:156-187`. Parent is canonicalized (`parent.canonicalize()?`, line 174); `starts_with(&forge_root_canonical)` checked (line 175); write performed on `absolute` (line 186), not on a re-resolved path.

Why it matters: same class of attack as F-5.3.1, but via the native plugin path rather than the WASM one. Core plugins hold `FsWrite`, but they are in-tree so the window is less dangerous. Nevertheless, any future second-party tier would reopen the hole.

Fix: Same as F-5.3.1. The fix is small and should be applied to both sites.

**F-5.4.1 — Fuel + memory limits are correctly applied per sandbox** ✅

Evidence: `crates/nexus-plugins/src/sandbox.rs:136-148` — `StoreLimitsBuilder::new().memory_size(config.memory_mb * 1024 * 1024)` plus `store.set_fuel(config.fuel)` iff `fuel > 0`. Fuel exhaustion is distinguishable from other traps (`sandbox.rs:379-393` `map_trap_error`).

**F-5.4.2 — WASI is not enabled; `wasmtime::Linker` only binds `host::*` functions** ✅

Evidence: `crates/nexus-plugins/src/sandbox.rs:150-154` — no `wasmtime_wasi::add_to_linker` call; only `crate::host_fns::register_host_fns(&mut linker)`. Community plugins cannot escape via WASI filesystem, environment, or clocks — only via the explicit host functions enumerated in `host_fns.rs`.

**F-5.5.1 — Script (JS) plugins have no capability enforcement backend-side** 🟠

Evidence: `crates/nexus-plugins/src/loader.rs:411-413` — `manifest.script.is_some() → PluginBackend::Script`; `sandbox.rs` / `host_fns.rs` never run. `plugins/hello-js/manifest.toml` has no `[capabilities]` section. Script plugins execute in the Tauri WebView; their reach is whatever the frontend exposes, which is the entire Tauri command surface subject only to the Tauri allowlist.

Why it matters: the stated trust model ("community plugins are capability-gated") is false for the script tier. A plugin author can choose `[script]` over `[wasm]` and acquire broader default reach without requesting any capability. The frontend's Tauri allowlist is the sole barrier. For an untrusted trust model this is unacceptable.

Fix: Either (a) retire the script tier for community-distributed plugins, restricting `[script]` to first-party/core, or (b) apply the capability system in the frontend as well — Tauri commands invoked from a script plugin must check the plugin's declared capabilities before servicing. The second is significant work; the first is a policy decision.

**F-5.5.2 — Script plugin dispatch correctly errors when hit on the backend** ✅

Evidence: `crates/nexus-plugins/src/loader.rs:146` — `PluginBackend::Script → Err(PluginError::ScriptDispatchFrontend)`. This prevents accidental backend invocation of frontend handlers.

**F-5.6.1 — Event namespace spoofing prevention is enforced at publish time** ✅

Evidence: `crates/nexus-kernel/src/event_bus.rs` `publish_plugin(source_plugin_id, type_id, …)` returns `BusError::TypeIdNamespaceMismatch` if `type_id.starts_with(source_plugin_id)` is false. Same rule applied in `context_impl.rs:232-238`. Tested.

**F-5.6.2 — `publish_core` grants the kernel full namespace freedom** 🟢

Evidence: `crates/nexus-kernel/src/event_bus.rs` — `publish_core(event)` has no namespace check; it is intended for `NexusEvent` variants (PluginLoaded, PluginCrashed, etc.). Only callable by the kernel itself, so not a boundary concern.

---

### 6. Host API Design

**F-6.1.1 — Host function surface is minimal, explicit, and consistently capability-gated** ✅

Evidence: `crates/nexus-plugins/src/host_fns.rs:43-54` registers exactly 9 functions: `log`, `kv_get`, `kv_set`, `emit_event`, `read_file`, `write_file`, `invoke_command`, `get_settings`, `notify`. Every data-modifying one checks the corresponding `Capability` before doing work (`host_fns.rs:165, 232, 287, 362, 458, 572, 726`).

**F-6.1.2 — Error-code contract is small and documented** ✅

Evidence: `crates/nexus-plugins/src/host_fns.rs:14-27`. Four constants: `HOST_OK = 0`, `HOST_ERROR = -1`, `HOST_CAPABILITY_DENIED = -1001`, `HOST_BUFFER_OVERFLOW = -1002`. Tests assert distinctness.

**F-6.2.1 — `host::get_settings` has no capability gate** 🟡

Evidence: `crates/nexus-plugins/src/host_fns.rs:655-700`. The docstring (line 649-654) acknowledges this and signposts a future `settings.read` capability if privacy concerns appear. Documented; not currently exploitable.

Why it matters: low today because each plugin sees only its own settings. If a future change routes settings through a shared namespace, this becomes a privilege escalation.

Fix: Low priority; add `settings.read` before launching a public plugin marketplace.

**F-6.2.2 — `host::log` is not capability-gated; an adversarial plugin can spam the log at any level** 🟡

Evidence: `crates/nexus-plugins/src/host_fns.rs:77-136`. No capability check; severity bit `0..=3` maps to tracing levels. A plugin in a tight loop can flood the log.

Why it matters: log-based observability is a resource. Fuel metering bounds WASM execution time, which caps log volume indirectly, but a plugin with generous fuel can still produce hundreds of thousands of log lines per invocation.

Fix: Rate-limit per plugin (e.g., a simple token bucket keyed by `plugin_id`); alternatively, gate ERROR-level logs behind a capability.

**F-6.3.1 — `read_file`/`write_file` auto-escalate to external variants on absolute paths** 🟠

Evidence: `crates/nexus-kernel/src/context_impl.rs:142-154` and `:156-187`. `context.read_file("/etc/passwd")` silently changes the required capability from `FsRead` to `FsReadExternal`. The WASM `host::read_file` similarly normalizes absolute paths via `if requested.is_absolute() { requested.to_path_buf() } else { forge_root.join(requested) }` (`host_fns.rs:584-589`), then canonicalizes and checks `starts_with(forge_root)` — so absolute-path reads are *denied* if outside forge root (good), but the auto-escalate mechanic in the native path (`context_impl.rs`) is undocumented magic.

Why it matters: a plugin author's mental model is "I have `fs.read`, I can read files in my forge". They won't realize that a path outside forge_root silently requires a different capability; their plugin will fail cryptically with `CapabilityDenied(fs.read.external)` on machines where forge_root is a symlink target. The behavior differs between the WASM path (denies outright) and the native path (tries the external capability).

Fix: Make the external-capability promotion explicit — plugins must call `read_file_external(path)` to reach outside forge_root, and never auto-promote. Unify the native and WASM paths.

**F-6.3.2 — Output-buffer size semantics are consistent across read-style host functions** ✅

Evidence: `host::read_file`, `host::kv_get`, `host::invoke_command`, `host::get_settings` all follow the same protocol: "return `HOST_BUFFER_OVERFLOW` when data exceeds `out_cap`; otherwise return bytes written and write into `[out_ptr, out_ptr+len)`".

**F-6.4.1 — IPC arguments flow as `serde_json::Value`; serialization cost is visible but bounded** 🟡

Evidence: Every dispatch serializes `args` to bytes and deserializes on the other side (WASM boundary) or both ends (async IPC via `spawn_blocking`). Acceptable for M1, but there is no batching and no streaming — a 10MB payload between core plugins is a 10MB copy each way.

Suggestion: For M2+ consider a `Bytes` fast-path for known binary commands (e.g., editor operations). Document the JSON-only contract so plugin authors know what to design for.

---

### 7. Extension Points

**F-7.1.1 — Extension categories are well-enumerated** ✅

Evidence: `crates/nexus-plugins/src/manifest.rs` declares 11 distinct `[[registrations.*]]` categories. Each has a typed struct, TOML shadow type, and a field in the aggregate `RegistrationsManifest`.

**F-7.2.1 — `EventFilter::Variant(&'static str)` is unreachable from manifest-declared subscribers** 🟠

Evidence: `crates/nexus-kernel/src/event.rs` — `EventFilter::Variant(&'static str)`. `&'static str` cannot be produced from an owned `String` (a manifest field) without leaking the string. The loader's `parse_event_filter` helper (in `loader.rs`) only ever produces `CustomExact` or `CustomPrefix`, never `Variant`.

Why it matters: a plugin that wants to subscribe to `nexus.plugin.loaded` (a kernel-emitted `NexusEvent::PluginLoaded`) must use a different mechanism — today it does not receive these events at all via manifest subscriptions. The `Variant` arm exists but is dead from the manifest path.

Fix: Change `EventFilter::Variant(&'static str)` to `EventFilter::Variant(String)`, or introduce a `KernelEvent` category with a separate filter enum.

**F-7.2.2 — `nexus.host.*` subscribers work correctly via `CustomPrefix`** ✅

Evidence: `plugins/hello-nexus/manifest.toml:67-70` subscribes with `filter = "nexus.host.*"`; the loader converts this to `EventFilter::CustomPrefix("nexus.host.")`. The `hello-nexus` plugin's handler 104 observes these events in practice. (Tested in `crates/nexus-plugins/tests/prd-04-smoke.rs`.)

**F-7.3.1 — UI-only extensions (`ui_ribbon_item`, `ui_status_item`, `slash_command`, `menu_item`) delegate to command IDs rather than handler IDs** ✅

Evidence: `crates/nexus-plugins/src/manifest.rs` — these have `command: String` rather than `handler_id: u32`. This is correct: they are bookmarks into the command registry. It is also why they do not show up in the handler_id uniqueness check (F-3.2.1).

**F-7.4.1 — CLI subcommand conflict handled by a dedicated error variant** ✅

Evidence: `crates/nexus-plugins/src/error.rs:93-99` — `PluginError::DuplicateCliSubcommand { plugin_id, subcommand }`. Detection presumably lives in `PluginLoader` (first plugin wins or second is rejected); not confirmed within audit scope.

---

### 8. Failure Handling & Kill Switch

**F-8.1.1 — Failures are isolated to the individual plugin** ✅

Evidence: `crates/nexus-plugins/src/lib.rs` `PluginManager::load_all` iterates every plugin dir; each load failure is logged and skipped, none cascades. `PluginStatus::Crashed` persists the transition.

**F-8.2.1 — No disable-by-default kill switch; a repeatedly-crashing plugin will retry on every start** 🟠

Evidence: `crates/nexus-plugins/src/loader.rs` / `lib.rs`. A plugin that panics in `on_init` is marked `Crashed`, but its next `PluginManager::load_all` invocation (e.g., after a Nexus restart) will attempt the init again. No persistent "disabled due to crashes" state.

Why it matters: a plugin that crashes on every init will burn startup budget every time. Users must manually delete the plugin dir or edit configuration to escape.

Fix: Track crash count in a persistent `<plugin_dir>/.nexus-state.json`; if `crash_count >= 3` within `crash_window_minutes`, auto-disable the plugin and require an explicit `nexus plugin reset <id>` to reactivate. Emit `com.nexus.plugin.quarantined` event.

**F-8.2.2 — No admin "disable all community plugins" kill switch** 🟠

Evidence: Grep for `safe_mode`, `disable_all`, `--no-plugins`: nothing. `crates/nexus-cli/src/` (not reviewed exhaustively) does not appear to expose this.

Why it matters: when a bad plugin breaks the boot path, the only recovery is filesystem edits. For an untrusted ecosystem, operators need a one-flag bypass.

Fix: `nexus --safe-mode` (and/or env var `NEXUS_SAFE_MODE=1`) that sets `config.hot_reload_enabled = false` and instructs `PluginLoader::load` to skip every `TrustLevel::Community` plugin. Core plugins still load.

**F-8.3.1 — IPC timeout is enforced via `tokio::time::timeout` in both async and sync paths** ✅

Evidence: `crates/nexus-kernel/src/context_impl.rs:249-304` — `ipc_call` wraps the dispatcher future in `timeout(timeout, fut)` (line 275) and the `spawn_blocking` join in `timeout(timeout, join)` (line 292). Panic in the sync path maps to `IpcError::PluginCrashedDuringCall`.

**F-8.4.1 — `ExecutionTimeout` error is produced only for fuel exhaustion, not for wall-clock** 🟡

Evidence: `crates/nexus-plugins/src/sandbox.rs:379-393` — `map_trap_error` branches on `Trap::OutOfFuel`; any other trap is `ExecutionFailed`. There is no wall-clock deadline enforced by the sandbox itself.

Why it matters: WASM plugins can enter tight non-looping code (e.g., large allocation into linear memory, already bounded by memory cap) that consumes fuel slowly in wall-clock terms but bears no user-facing timeout. `WasmConfig::max_execution_ms` is declared (line 463) but not used by `sandbox.rs`.

Fix: Use `wasmtime::Store::set_epoch_deadline` + a background ticker, or call the handler inside `tokio::time::timeout`. `max_execution_ms` should not be a dead field.

**F-8.5.1 — Panics in host-side plugin invocations are captured as typed errors** ✅

Evidence: `crates/nexus-kernel/src/context_impl.rs:294` — `Ok(Err(_panic)) => Err(IpcError::PluginCrashedDuringCall { … })`. The `join.await` observes the panic; dispatch returns an error; the host stays up.

---

### 9. Versioning & Compatibility

**F-9.1.1 — Plugin manifest requires a semver version; parsed with `semver::Version::parse`** ✅

Evidence: `crates/nexus-plugins/src/manifest.rs:1146-1149`. Version string must parse as semver; rejected at validation time.

**F-9.2.1 — `api_version` is declared in every manifest but never compared to a supported range** 🟠

Evidence: `crates/nexus-plugins/src/manifest.rs:372, 584` — the `api_version` field is parsed from the TOML into the `PluginManifest` struct and then never consulted. Grep: zero comparisons against a constant or range anywhere in `nexus-plugins` or `nexus-kernel`. The field exists purely as documentation.

Why it matters: this is the primary forward-compatibility mechanism declared in the manifest and in `plugins/hello-nexus/manifest.toml:6`. Without enforcement, a breaking host-function change cannot be rolled out safely — every installed plugin "speaks api_version 1" by declaration, but they may link against ABI 1 or ABI 2.

Fix:
1. Define `pub const PLUGIN_API_VERSION: u32 = 1;` in the future `nexus-plugin-api` crate (see F-2.1.1).
2. At `PluginLoader::load`, compare `manifest.api_version` against `PLUGIN_API_VERSION` (ideally with a documented forward-compat rule: same major = ok, newer minor = ok, different major = reject).
3. Return a typed `PluginError::IncompatibleApiVersion { plugin_id, requested, supported }`.

**F-9.3.1 — No ABI stability tooling (`abi_stable`, `#[repr(C)]`, etc.)** 🟢

Evidence: `nexus-plugins/Cargo.toml` — no `abi_stable` or `stabby` dependency. Community plugins are WASM (their own ABI story) or native-Rust core plugins (compiled in-tree, no ABI concern). For the current architecture this is correct.

Why it matters: a future `plugin-dylib` tier would need ABI-stability tooling. For now, no action.

**F-9.4.1 — Capability enum changes would silently break installed plugins** 🟡

Evidence: `crates/nexus-kernel/src/capability.rs` — the `Capability` enum and its string names are the de-facto ABI for capability declarations. If a future release renames `fs.read` to `filesystem.read`, existing manifests would fail validation at `Capability::from_str` without a migration path.

Fix: When a capability rename is needed, accept the old string via an alias map (e.g., `"fs.read" → Capability::FsRead`; also accept `"filesystem.read"` → same variant). Document deprecation.

---

### 10. Observability

**F-10.1.1 — Structured audit helpers are in place and tested** ✅

Evidence: `crates/nexus-security/src/audit.rs` — five functions (`log_capability_granted`, `log_capability_denied`, `log_plugin_lifecycle`, `log_credential_access`, `log_path_traversal_denied`). Each emits a `tracing::{info|warn}!` with `audit = true` so downstream subscribers can filter. Tests verify that each call produces a captured event with the right fields.

**F-10.1.2 — Audit helpers are not uniformly called from the code paths they describe** 🟠

Evidence: Grep for `log_capability_granted` / `log_capability_denied` / `log_path_traversal_denied`: **zero non-test callers**. The `KernelPluginContext::require_capability` method emits its own `tracing::warn!(…"capability denied")` (`context_impl.rs:78-82`) rather than calling the audit helper. `host_fns.rs` path-traversal sites (`write_file:403`, `read_file:600`) emit `tracing::warn!` but do not call `log_path_traversal_denied`.

Why it matters: the audit helpers exist to produce events with the `audit = true` marker so the subscriber can split audit events from general logs. Without uniform use, the audit subscriber cannot provide a complete picture — a sophisticated attacker's path-traversal attempt would appear only as a regular warn! and could be missed by audit-only filters.

Fix: Route every capability grant/denial and every traversal rejection through the `nexus-security::audit` helpers. Add a clippy lint or a code-review checklist so future sites use the helpers.

**F-10.1.3 — Events are propagated to the plugin bus as well as to tracing** ✅

Evidence: `crates/nexus-kernel/src/event_bus.rs` — `NexusEvent::{PluginLoaded, PluginStarted, PluginStopped, PluginCrashed, CapabilityGranted, CapabilityDenied, …}`. `crates/nexus-security/src/core_plugin.rs:56-75` — `on_start`/`on_stop` publish `com.nexus.security.started`/`stopped`. External tools can subscribe to the bus rather than parsing logs.

**F-10.2.1 — Plugin-level tracing consistently tags `plugin_id`** ✅

Evidence: Every `host_fns.rs` host function clones `plugin_id` early and includes it via the `plugin_id = %plugin_id` structured field on each `tracing::warn!` / `tracing::error!`. `KernelPluginContext::log` also tags.

**F-10.3.1 — No metrics / telemetry emitted** 🟡

Evidence: No `metrics` crate, `opentelemetry`, or custom counter registry anywhere in the workspace. Observability is 100% log-based.

Why it matters: for a production plugin host you want P99 IPC latency per (plugin, command), fuel consumption histograms, and crash-rate counters. At alpha this is acceptable; at v1 it is not.

Suggestion: Wire `metrics` + `metrics-exporter-prometheus` at the bootstrap layer; emit `nexus_ipc_call_seconds{plugin, command}`, `nexus_wasm_fuel_remaining{plugin}`, `nexus_plugin_crashes_total{plugin}`.

---

## Strengths

Nexus has several things that a micro-kernel auditor rarely sees at this stage, and they deserve explicit recognition:

1. **Event bus with anti-spoofing enforcement.** `publish_plugin` requires `type_id.starts_with(source_plugin_id)`. A community plugin cannot emit events claiming to be from `com.nexus.storage`. Simple, effective, tested.

2. **Fuel + memory metering applied per sandbox.** `StoreLimitsBuilder::memory_size(memory_mb * 1MB)` plus `store.set_fuel(fuel)`, with `Trap::OutOfFuel` distinguished in the error map. Community plugins cannot exhaust host resources without being terminated.

3. **WASI disabled.** The `wasmtime::Linker` only binds the explicit `host::*` function set. Community plugins have no latent file, clock, or environment access — everything is gated through named host functions.

4. **Comprehensive typed error hierarchy.** `crates/nexus-kernel/src/error.rs` plus `crates/nexus-plugins/src/error.rs` give callers 30+ precise variants. Boundary events (lifecycle errors, timeouts, capability denials, manifest validation errors) are all distinct.

5. **Path confinement with symlink awareness.** `ForgePathValidator` in `nexus-security` follows the correct pattern (canonicalize, then prefix-check), and its test suite (`crates/nexus-security/src/path.rs:229-263`) explicitly covers Unix symlink-escape attempts. The fact that it is not yet used by `host_fns.rs` is F-5.3.1, but the implementation itself is right.

6. **Manifest validation is thorough.** Nine distinct rules enforced at load time (ID regex, semver, capability existence, handler-id uniqueness, wasm/script exclusion, memory range, fuel, file existence, settings-schema presence). The validation tests (`crates/nexus-plugins/src/manifest.rs:1396-1570`) cover every rule with positive and negative cases.

7. **Reentrancy detection exists, even if it uses the wrong error variant.** `try_lock` on the per-plugin backend mutex prevents a plugin from recursively calling itself via `ipc_call` and deadlocking. The detection is there; only the error taxonomy needs a fix (F-5.2.1).

---

## Prioritized Action List

Single-sentence, actionable items ordered by impact for a third-party-untrusted trust model:

1. **Implement an install-time capability prompt for HIGH-risk capabilities** (F-5.1.1). Non-negotiable before any public marketplace.
2. **Fix the `host::write_file` and `KernelPluginContext::write_file` TOCTOU windows** by routing through `ForgePathValidator` (F-5.3.1, F-5.3.2).
3. **Extract a `nexus-plugin-api` crate** that contains only the plugin-facing contract; have `nexus-kernel` and plugin authors depend on it (F-2.1.1).
4. **Enforce `api_version` at load time** against a `PLUGIN_API_VERSION` constant in `nexus-plugin-api` (F-9.2.1).
5. **Decide the fate of Script plugins for untrusted distribution**: retire the community-script tier, or apply the capability system to the frontend (F-5.5.1).
6. **Add a `--safe-mode` flag that skips community plugins** (F-8.2.2) and a **persistent crash counter with auto-quarantine** (F-8.2.1).
7. **Enforce `max_execution_ms`** via `set_epoch_deadline` so the field is not dead (F-8.4.1).
8. **Add `uri_handlers` to the handler_id uniqueness check** (F-3.2.1).
9. **Replace `EventFilter::Variant(&'static str)` with `EventFilter::Variant(String)`** so manifest subscribers can address typed kernel events (F-7.2.1).
10. **Introduce `IpcError::ReentrantCall`** and stop overloading `CapabilityDenied` (F-5.2.1).
11. **Route all capability grant/denial and path-traversal sites through `nexus-security::audit`** (F-10.1.2).
12. **Either fix or remove `Kernel::start`/`shutdown`** so there is a single lifecycle entry point (F-1.1.1).
13. **Remove or reconcile the kernel-side `PluginRegistry` with `PluginLoader::loaded`** (F-1.2.1).
14. **Add `Send + Sync` to the `CorePlugin` trait** (F-2.2.2).
15. **Rate-limit `host::log`** to bound log-flood potential (F-6.2.2).
16. **Document the auto-promotion behavior of absolute-path reads/writes** or remove it outright (F-6.3.1).

---

## Suspected Issues (Not Fully Investigated)

A handful of threads emerged during the walk that deserve a closer look but exceed the scope of a single pass:

- **Subscriptions persistence integrity.** `subscriptions.json` (per-plugin disabled-subscription list) is loaded via `serde_json::from_str` and silently defaults to an empty set on any parse error (`loader.rs` `load_disabled_subscriptions`). A corrupt file silently re-enables every subscription. Worth either checksumming or failing loudly.
- **Fuel replenishment strategy.** Each call consumes fuel but nothing in `dispatch` tops it up between calls — a long-lived plugin with `fuel = 1_000_000` will eventually return `OutOfFuel` on every handler. Check whether the loader refills fuel per call or expects plugins to be short-lived per-dispatch.
- **Tauri frontend contribution bridge.** The backend `PluginManager::aggregate` builds UI contributions; how Tauri reads them and renders untrusted plugin icons / tooltips was not reviewed. XSS via `tooltip` string would be a real concern for a marketplace.
- **MCP server trust boundary.** The `rmcp` dependency exposes an MCP server, but who speaks to it and with what authentication was not walked. If a misconfigured MCP endpoint is reachable off-host, it bypasses the plugin capability model entirely.
- **Hot-reload on macOS / Windows timing.** `notify-debouncer-mini`'s behavior across platforms is known to differ; F-4.3.1 flagged one class of issue, but a platform-specific reliability pass would be worth its own review.

---

## Methodology Notes

**What was read (in order):** `microkernel-auditor.md` (the auditor spec) → `Nexus/README.md` → workspace `Cargo.toml` → every file in `crates/nexus-kernel/src/` → every file in `crates/nexus-plugins/src/` → `crates/nexus-security/src/{lib,risk,path,audit,credential,core_plugin}.rs` → `crates/nexus-bootstrap/src/lib.rs` → both sample plugins' manifests and `plugins/hello-js/plugin.js`.

**What was not read:** full frontend contribution bridge; the CLI / TUI crates beyond their bootstrap entry; `nexus-storage` / `nexus-ai` / `nexus-editor` / `nexus-theme` / `nexus-database` internals (they participate as core plugins but their internals are out of microkernel scope); `rmcp` server surface; tests for the Tauri layer.

**Where the auditor made judgment calls:** severity choices lean HIGH where the trust model is "third-party untrusted" — the same code reviewed under a "first-party only" model would downgrade F-5.3.1 and F-5.5.1 to 🟠 and F-2.1.1 to 🟡. The auditor consulted `nexus-security::risk_level` for HIGH-risk capability classification and took its classification as authoritative because it compiles (exhaustive match over `Capability`) and is tested.

**What was verified by second-pass grep or read:** every cited line number was read directly. The F-5.3.1 claim was confirmed by reading `host_fns.rs` lines 385-412 top-to-bottom and re-tracing the `absolute` variable's usage. The `api_version` enforcement claim (F-9.2.1) was confirmed by grepping the entire `crates/` directory for `api_version` and finding zero comparison sites. The "handler_id omits uri_handlers" claim was confirmed by reading `manifest.rs:1164-1213` and separately enumerating every struct with `handler_id: u32` (7 matches; only 6 appear in the chain).

**Scope limits:** dimensions 7 (Extension Points) and 10 (Observability) could warrant their own deeper reviews; the findings here reflect what the microkernel-boundary lens surfaced rather than an exhaustive extension or observability audit.
