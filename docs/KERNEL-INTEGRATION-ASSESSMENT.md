# Kernel Implementation Assessment
_Assessed: 2026-05-06_

## Overall: 9/10 — The architectural backbone is sound. The gaps are operational, not structural.

The microkernel is deliberately minimal — 2,353 lines across 14 modules, with the heaviest modules
being the event bus (536 lines, 35 tests) and the plugin context implementation (641 lines, 18
tests). The design discipline is visible: the kernel doesn't own plugin lifecycle, WASM sandboxing,
or crash recovery. Those live in `nexus-plugins`. The kernel owns the event bus, the IPC
dispatcher, the capability system, and the plugin context surface. That scope is correct and
well-executed.

---

## What's fully implemented and first-class

**Namespace spoofing defense is airtight.** The event bus enforces that a plugin can only publish
events in its own namespace. The naive `starts_with` check would allow `com.foo` to publish
`com.foobar.event`. The implementation uses a `.`-separator anchor:
`type_id.strip_prefix(plugin_id).is_some_and(|rest| rest.starts_with('.'))`. Tested with
regression tests for the exact boundary case (issue #79).

**Capability checks enforced at every operation.** `FsRead` before any file read. `FsWrite` before
any file write. `IpcCall` before any IPC dispatch. The capability check is the first thing in
every operation — it short-circuits before any I/O runs. 22 capabilities with HIGH/MEDIUM risk
classification. Args-aware policy checks (ADR 0022) let per-command gates exist —
`stream_chat` with `tools=auto` requires `AiToolsWrite`, checked before dispatch.

**Path confinement closes the TOCTOU race.** `ForgePathValidator::validate_for_write()` canonicalizes
the deepest *existing* ancestor, then rebuilds the target path from that. This closes the
symlink-swap race between `canonicalize` and `open` (audit finding F-5.3.2). Read path uses
canonicalize + prefix check. No auto-promotion from `FsRead` to `FsReadExternal` (OI-12 fixed).

**IPC dispatch handles panics and timeouts correctly.**
- Sync handlers run in `spawn_blocking` — a panic becomes `JoinError` → `IpcError::PluginCrashedDuringCall`
- Async handlers wrapped in `tokio::time::timeout` → `IpcError::Timeout` on expiry
- Reentrancy detection via thread-local `ACTIVE_DISPATCHES` stack — prevents sync deadlock
- `IpcErrorEnvelope` is wire-stable JSON with `retryable: bool` — frontends branch on `kind`, not error strings

**Event bus backpressure handled, not silently dropped.** Bounded broadcast channel (default 128,
configurable). Slow subscribers receive `RecvError::Lagged(n)` — they don't block faster ones
and don't silently lose events. Tested.

**dep_invariants test is passing and mechanically enforced.** Reads Cargo.toml files at test time.
`nexus-cli`, `nexus-tui`, `nexus-ai`, `nexus-mcp` cannot directly link `nexus-storage`.
`nexus-kernel` cannot link `rusqlite` or `nexus-kv`. Catches `target.cfg` conditional deps too
(issue #83). The microkernel isolation invariant is compile-time enforced.

**WASM sandbox is comprehensive.** Every host function checks capabilities before executing.
Memory limits via `wasmtime::StoreLimits`. Log output rate-limited via token bucket (1000
lines/sec). Path operations use the same `ForgePathValidator` as native plugins — not a weaker
version.

**Error hierarchy is typed and complete.** Every error carries context (plugin IDs, capability
names, timeout milliseconds). `IpcError::Timeout` is the only retryable variant, marked with
`retryable: bool`. Panics caught and reported. No stringly-typed errors in the kernel.

---

## IPC call flow

```
ipc_call(target, command, args, timeout)
  → require_capability(IpcCall)           // short-circuit if denied; audit log
  → required_caller_caps_for_args(...)    // args-aware per-command gates (ADR 0022)
  → dispatch_async() → Some(future)       // async path
      → tokio::time::timeout(timeout, future)
      → IpcError::Timeout on expiry
  → dispatch_async() → None               // sync fallback
      → spawn_blocking(|| dispatch(args))
      → tokio::time::timeout on join
      → panic → PluginCrashedDuringCall
```

---

## Capability system

22 capabilities in five categories:

| Category | Capabilities | HIGH risk |
|---|---|---|
| Filesystem | `FsRead`, `FsWrite`, `FsReadExternal`, `FsWriteExternal` | External variants |
| Network | `NetHttp`, `NetHttpLocalhost` | `NetHttp` |
| Processes | `ProcessSpawn` | Yes |
| Storage | `KvRead`, `KvWrite` | — |
| IPC | `IpcCall` | Yes |
| Database | `DbQuery`, `DbWrite` | — |
| Events/UI | `EventsPublish`, `UiNotify` | — |
| AI | `AiChat`, `AiIndex`, `AiSessionRead/Write`, `AiConfigWrite`, `AiActivityWrite`, `AiToolsWrite`, `AiToolsMcp` | `AiConfigWrite` |

Enforcement: bitmap lookup in `CapabilitySet`, O(1). Audit-logged on grant and denial.

---

## Event bus

- Tokio broadcast channel (bounded, configurable capacity)
- `EventFilter`: `All` / `Variant(name)` / `CustomPrefix(prefix)` / `CustomExact(id)`
- Publish paths: `publish_plugin` (namespace-validated) / `publish_core` (trusted) / `publish_kernel` (kernel-internal)
- Backpressure: slow subscribers lag, receive `RecvError::Lagged(n)`, don't block fast ones
- Delivery: no guarantee — silent drop if no subscribers; no persistence across restarts
- 35 unit tests covering roundtrip, filters, lag, spoofing rejection, closure semantics

---

## Where it falls short

### 1. No performance benchmarks or latency SLOs

Zero benchmarks in the kernel. IPC call latency, event bus publish throughput, and capability check
overhead are all unknowns. For a central dispatch layer that every subsystem routes through,
lacking tail-percentile data is a real operational risk.

### 2. No event persistence

Events are in-memory only. Kernel crash loses all event history. Audit events (capability
grants/denials, plugin crashes) are only in `tracing` output — not persisted, not queryable,
not recoverable across restarts.

### 3. Plugin lifecycle hooks have no timeout

`on_init()` and `on_start()` have no deadline. A plugin that hangs during initialization blocks
the entire bootstrap sequence forever. No watchdog, no abort. Silent DoS vector if a plugin blocks
on a network resource or unreleased lock during startup.

### 4. No metrics or observability exports

No Prometheus counters. No histograms on IPC latency. No event bus queue depth gauge. Audit
logging via `tracing` is present but produces no time-series data. Cannot determine from outside
the process whether the kernel is healthy, backlogged, or thrashing.

### 5. No capability revocation

Capability grants are permanent for the plugin's lifetime. No `revoke_capability()` method. If a
community plugin is granted `NetHttp` and a post-install audit finds it over-privileged, the forge
must restart. Matters most for community WASM plugins.

### 6. IPC schema is unversioned

All IPC args/returns are `serde_json::Value`. No version field, no schema negotiation, no
deprecation path. A breaking change to a handler's argument shape will produce a silent
deserialization error or misparsed input. Fine in a monorepo today; maintenance risk when
community plugins ship against a stable API.

---

## Scorecard

| Dimension | Score | Notes |
|---|---|---|
| Correctness | 10/10 | Typed, tested, security-audited, known issues closed |
| Capability enforcement | 10/10 | Every operation gated, args-aware, TOCTOU-safe |
| Event bus | 9/10 | Backpressure, namespace guard, filter types; no persistence |
| IPC dispatch | 9/10 | Timeout, panic handling, reentrancy; no schema versioning |
| WASM sandbox | 9/10 | Memory limits, rate limiting, same cap system as native |
| Error handling | 10/10 | Typed hierarchy, wire-stable envelopes, retryable flag |
| Architecture discipline | 10/10 | Minimal kernel; lifecycle/WASM correctly delegated |
| Testing | 9/10 | 160+ tests, good coverage; no chaos/soak/concurrent stress |
| Performance visibility | 3/10 | No benchmarks, no metrics, no latency SLOs |
| Observability | 4/10 | Audit logging works; no metrics, no distributed tracing |

---

## The honest summary

The kernel is doing exactly what a microkernel should: providing a small, trusted, well-tested
core that every subsystem routes through. The security posture is strong — namespace spoofing
closed, path traversal closed, capability checks enforced at every boundary, WASM plugins get
the same enforcement as native ones. The error model is correct and IPC dispatch handles the hard
cases.

The gaps are operational rather than structural. The kernel doesn't know how fast it is, doesn't
record what it does across restarts, and has no way to stop a plugin that hangs during startup.
These surface in production at scale — not in a single-user forge, but with community extensions
and long-running processes.

The highest-value pre-shipping addition: IPC latency histograms and event bus queue depth gauges.
Everything else can wait.

---

## Key source files

```
crates/nexus-kernel/src/
├── event_bus.rs    (536)  — EventBus, namespace guard, filters, backpressure; 35 tests
├── context_impl.rs (641)  — KernelPluginContext: capability checks, path confinement, IPC; 18 tests
├── audit.rs        (204)  — Structured audit event helpers (cap grant/deny, path traversal)
├── error.rs        (297)  — Error hierarchy, IpcErrorEnvelope; 12 tests
├── kernel.rs       (188)  — Kernel struct, lifecycle signal, event bus ownership
├── config.rs       (186)  — TOML config loading + validation
└── kv_store.rs     (118)  — KV store trait + in-memory impl

crates/nexus-plugin-api/src/
├── capability.rs   (297)  — Capability enum (22), CapabilitySet bitmap, risk classification
├── ipc.rs          (88)   — IpcDispatcher trait, IpcError, IpcErrorEnvelope
└── event.rs               — NexusEvent enum, EventFilter, EventSubscription

crates/nexus-bootstrap/tests/
└── dep_invariants.rs (155) — Compile-time IPC boundary enforcement; 2 tests + self-test
```
