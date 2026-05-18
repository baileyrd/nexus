# SOLID & DRY Assessment

> **Date:** 2026-05-18.  **Scope:** Nexus Rust workspace (35 crates) + `shell/` Tauri 2 app.  **Method:** direct read of kernel + plugin-api + bootstrap, sampling of representative service crates (`nexus-storage`, `nexus-ai`, `nexus-editor`, `nexus-git`, `nexus-database`, `nexus-theme`, `nexus-workflow`, `nexus-agent`, `nexus-skills`, `nexus-templates`, `nexus-terminal`, `nexus-notifications`, `nexus-comments`, `nexus-collab`, `nexus-formats`, `nexus-audio`), and cross-checking against `docs/0.1.2/architecture.md`, `architecture-adherence.md`, and `cap_matrix.toml`.

## TL;DR

| Principle | Verdict | One-liner |
|-----------|:------:|-----------|
| **S — Single Responsibility** | ⚠️ partial | Crate-level SRP is excellent (one domain per `nexus-<service>` crate). File-level SRP is broken — several `core_plugin.rs` files are 1.5K – 4.4K LOC god-modules holding registration, lifecycle, IPC routing, and inline business logic. |
| **O — Open / Closed** | ✅ strong | New behaviour = new plugin / new handler row + new dispatch arm. `cap_matrix.toml` is data-driven; provider traits in AI/audio/notifications/LSP/DAP/ACP/MCP are open for new providers without touching dispatch. |
| **L — Liskov Substitution** | ✅ | `CorePlugin`, `PluginContext`, `IpcDispatcher`, `KvStore`, provider traits all have substitutable impls (real + test/in-memory). No widening preconditions or narrowing postconditions found. |
| **I — Interface Segregation** | ⚠️ partial | `PluginContext` is wide (file, KV, events, IPC, log in one trait). `CorePlugin` couples `dispatch` + `dispatch_async` + 5 lifecycle hooks on every plugin. Most consumers use < 30 % of either. |
| **D — Dependency Inversion** | ✅ strong | The defining strength of the architecture. Kernel depends only on leaf trait crates (`nexus-types`, `nexus-plugin-api`). Frontends + protocol consumers depend on abstractions (IPC), not concretes. Enforced by `dep_invariants.rs`. |
| **DRY** | ❌ recurring violation | ~15 service crates each redefine private helpers `exec_err` / `parse_args` / `to_value` / `path_arg`. The "decode JSON args → call engine method → wrap error → re-encode" pattern is reproduced ~340 times across `core_plugin.rs` dispatch arms. |

**Bottom line:** the *macro architecture* (microkernel, IPC-only boundary, capability gating, dep-invariant tests) is a textbook application of SOLID — particularly D, O, L. The *micro implementation* of each plugin's dispatch surface has accumulated systemic DRY debt and one fat-interface ISP smell. None of these block correctness; they are maintainability tax that compounds linearly with every new handler.

---

## 1. Single Responsibility (SRP)

### 1.1 Crate level — strong ✅

Each `nexus-<service>` crate owns exactly one bounded subdomain (storage, ai, editor, git, terminal, …). The dep-invariants test (`crates/nexus-bootstrap/tests/dep_invariants.rs`) actively *enforces* that one crate doesn't reach into another's implementation:

```
nexus-cli, nexus-tui, nexus-mcp, nexus-ai, nexus-database → ⛔ nexus-storage
nexus-acp, nexus-remote                                   → ⛔ nexus-agent, nexus-ai, nexus-storage
nexus-kernel                                              → ⛔ rusqlite, nexus-kv
```

This is SRP defended by mechanical test, not by convention.

The kernel itself is well-decomposed (see `crates/nexus-kernel/src/`):

| File | LOC | Single responsibility |
|------|----:|-----------------------|
| `event_bus.rs` | 604 | Tokio broadcast wrapper + namespace anti-spoofing |
| `context_impl.rs` | 732 | `KernelPluginContext` — capability-gated façade |
| `audit.rs` + `audit_store.rs` | 517 | Audit log surface + SQLite persistence |
| `metrics.rs` | 523 | Counters, histograms |
| `config.rs`, `kernel.rs`, `error.rs` | <300 each | One concept each |

The kernel does not mix concerns. Each module has a stated invariant in its file docs.

### 1.2 File level — broken ⚠️

`core_plugin.rs` in every service crate is the entry point that the bootstrap calls. Several have grown into single-file god-modules:

| File | LOC |
|------|----:|
| `nexus-editor/src/core_plugin.rs` | **4 425** |
| `nexus-terminal/src/core_plugin.rs` | ~2 850 |
| `nexus-storage/src/core_plugin.rs` | **2 725** |
| `nexus-ai/src/core_plugin.rs` | 1 681 (after `handlers/` split) |
| `nexus-git/src/core_plugin.rs` | 1 528 |

A 2 725-line `core_plugin.rs` holds:

1. ~50 `pub const HANDLER_<NAME>: u32 = N;` declarations (registration)
2. Plugin struct definition + 5 lifecycle hooks (`on_init` / `on_start` / `on_stop` / `on_enable` / `on_disable`)
3. The `fn dispatch(...)` match — a 1 000-line `match handler_id { … }` doing typed-arg decode + engine call + error wrap + JSON encode
4. ~10 free functions implementing inline business logic (`dispatch_entity_search`, `dispatch_entity_merge`, etc.)
5. Private helpers `exec_err` / `parse_args` / `to_value` / `path_arg`

Each is a separable concern. `nexus-ai`, `nexus-workflow`, and `nexus-agent` show the pattern that *should* be uniform — they extract per-handler modules into `src/handlers/<name>.rs` with a `shared.rs` for helpers:

```
crates/nexus-ai/src/handlers/
  mod.rs              ← module list
  shared.rs           ← exec_err / parse_args / to_value
  activity.rs ask.rs config.rs enrich.rs entity.rs index.rs
  predict.rs propose.rs search.rs session.rs
  stream_ask.rs stream_chat.rs
```

That layout localises change: editing `ask.rs` doesn't put you in the same file as `enrich.rs`'s tests. Storage / editor / terminal have not yet adopted it.

**Recommendation:** lift the `handlers/<verb>.rs` + `handlers/shared.rs` layout into the bootstrap's plugin-author guidance and migrate the four largest files. Mechanical refactor, no behaviour change.

### 1.3 Configuration as SRP

`KernelConfig` (`crates/nexus-kernel/src/config.rs`, 280 LOC) is one struct with construction, validation, and TOML (de)serialisation co-located. Acceptable. Service configs are similarly localised in each crate. No God-Config struct.

---

## 2. Open / Closed (OCP)

### 2.1 Strong — plugin model is the canonical OCP example

The shape of the system is "add a plugin, don't edit the kernel":

- New backend capability → new IPC handler in the owning service crate. Kernel dispatch is generic over `(plugin_id, command_id) → handler`.
- New AI / audio / notifications provider → new struct that implements the crate's provider trait. Existing dispatch arms don't change.
- New shell UI → a new contribution registered via `ExtensionHost`; `App.tsx` already mounts slot containers.

### 2.2 Capability matrix — data-driven extension point ✅

`crates/nexus-bootstrap/cap_matrix.toml` is the cleanest OCP artefact in the repo. Each handler is classified once:

```toml
[[handler]]
plugin  = "com.nexus.terminal"
command = "create_session"
caps    = ["process.spawn"]
note    = "spawns arbitrary processes with guest-supplied shell/working_dir/env"
```

Adding a new handler requires no code change in the kernel — append a row. Args-aware policies live in `cap_policies.rs` and are referenced by name. The `cap_matrix_complete` test enforces totality so the open extension surface can't accidentally drop the closed-to-modification half.

### 2.3 Minor friction — the giant `match` per dispatch

`fn dispatch(&mut self, handler_id, args)` in each plugin is a `match` that grows with every new handler. New code goes into a new arm — additive — but the *file* is being modified, not just extended. A `HashMap<u32, Box<dyn Fn(...)>>` registration model would close the file. The current design trades that closure for compile-time exhaustiveness checking. Acceptable trade.

---

## 3. Liskov Substitution (LSP)

Substitutability is sound across the workspace.

### 3.1 `CorePlugin` ✅

Every implementor honours the lifecycle contract documented in `crates/nexus-plugins/src/loader.rs:91`: `on_init` may fail; `on_start` may fail; `on_stop` is infallible; `dispatch` returns `Result`. No implementor I sampled widens the precondition on `handler_id` (e.g., requiring `on_start` before `dispatch` would compile but break the contract — not observed).

### 3.2 `PluginContext` ✅

The trait is implemented exclusively by `KernelPluginContext`. Tests use the same impl with an in-memory KV. No alternate impl narrows the postcondition.

### 3.3 `IpcDispatcher` ✅

`required_caller_caps_for_args` defaults to delegating to `required_caller_caps`, so an implementor that overrides only the static form still satisfies the args-aware contract by inheritance. That's correct LSP refinement.

### 3.4 KV store ✅

`InMemoryKvStore` (kernel-internal) and the production SQLite-backed store implement `KvStore` with identical postconditions (idempotent delete, returns `None` on missing read). Test fixtures swap freely.

### 3.5 Provider traits ✅

`AiProvider`, audio STT/TTS providers, notifications channels — each crate's trait has multiple implementors (real + mock) that pass the same dispatcher arm. No "throws `Unsupported` if you call X" smells observed in the surfaces I sampled (a deep audit per crate is the right next pass).

---

## 4. Interface Segregation (ISP)

### 4.1 `PluginContext` — fat interface ⚠️

`crates/nexus-kernel/src/context.rs` defines a single 120-line trait with 12 methods spanning **five concerns**:

| Group | Methods |
|-------|---------|
| Identity | `plugin_id`, `plugin_version`, `has_capability` |
| File system | `read_file`, `write_file`, `delete_file`, `list_files` |
| KV | `kv_get`, `kv_set`, `kv_delete` |
| Events | `publish`, `subscribe` |
| IPC | `ipc_call` |
| Logging | `log` |

A pure-compute plugin (`nexus-database`, `nexus-formats`) uses essentially `log` and nothing else. A frontend plugin uses `ipc_call`. Coupling them all together means:

- Mocking the context for unit tests requires a 12-method stub.
- A breaking change to `read_file`'s signature forces every plugin author to recompile, even those that never touch files.
- Capability surfaces would be cleaner per-trait (`FileSystem`, `Events`, `Ipc`) since each is already gated by its own capability cluster.

**Mitigation today:** capability denial inside the impl means a plugin without `fs.read` cannot actually *use* `read_file` — the wide trait is a *compile-time* coupling, not a *runtime* privilege escalation. So this is a maintainability complaint, not a security one.

**Recommendation:** split into supertraits — `pub trait PluginContext: Identity + FileSystem + KvAccess + Events + Ipc + Log {}` — and let small consumers depend on only the slice they need. This is a non-breaking refactor if the umbrella trait is preserved as an auto-impl.

### 4.2 `CorePlugin` — borderline ⚠️

The `CorePlugin` trait (`crates/nexus-plugins/src/loader.rs:91`) requires every plugin to implement `dispatch` (the only non-default method) plus opt-in defaults for `on_init`, `on_start`, `on_stop`, `on_enable`, `on_disable`, `on_settings_changed`, `dispatch_async`, `wire_context`. The default-method pattern is the right ISP escape hatch — a plugin that doesn't need lifecycle hooks just doesn't implement them.

The remaining smell: `dispatch` returns `Result<Value, PluginError>` (sync) and `dispatch_async` returns `Option<CorePluginFuture>` (async). Mixed sync/async surface forces every consumer to handle both. In practice the loader dispatches async-first and falls back, but plugins authoring a purely-async surface like `nexus-ai` must still provide a sync `dispatch` that returns a `"caller should use dispatch_async"` error (see `crates/nexus-ai/src/core_plugin.rs:352`). That's an ISP indicator — the trait shape doesn't fit the case.

**Recommendation (lower priority):** split `CorePlugin` into `SyncCorePlugin` and `AsyncCorePlugin`, or make the trait expose a unified `async fn dispatch` and require plugins to choose sync vs `tokio::task::block_in_place` internally. The current shape is workable but verbose.

### 4.3 Capability enum granularity ✅

The 33-variant `Capability` enum (`crates/nexus-plugin-api/src/capability.rs`) is the *opposite* of an ISP problem — fine-grained per-action capabilities (`AiChat`, `AiIndex`, `AiSessionRead`, `AiSessionWrite`, `AiConfigWrite`, `AiActivityWrite`, `AiToolsWrite`, `AiToolsMcp`, …) deliberately segregate so a plugin can ask for exactly what it needs. Textbook ISP applied to runtime permissions.

---

## 5. Dependency Inversion (DIP)

The strongest principle in the codebase. Three concentric layers of dependency inversion.

### 5.1 Kernel layer

`nexus-kernel/Cargo.toml`:
```toml
nexus-plugin-api  # leaf trait crate
nexus-types       # leaf type crate
# + tokio, tracing, serde, …
```

No subsystem dependency. The kernel programs against `IpcDispatcher`, `KvStore`, `PluginContext` traits — concrete impls (the SQLite KV, the plugin loader's dispatcher) are *injected* at construction time:

```rust
// crates/nexus-kernel/src/context_impl.rs:71
pub fn new(
    …,
    kv: Arc<dyn KvStore>,
    event_bus: Arc<EventBus>,
    ipc_dispatcher: Option<Arc<dyn IpcDispatcher>>,
) -> Result<Self>
```

Test contexts pass `InMemoryKvStore` + `None` for the dispatcher; production contexts pass the SQLite-backed store + the loader. The kernel is oblivious to either.

### 5.2 Bootstrap as composition root

`crates/nexus-bootstrap` is the **only** crate in the workspace allowed to link every service. Frontends (`nexus-cli`, `nexus-tui`, `nexus-mcp`, `shell/src-tauri`) depend on `nexus-bootstrap` to obtain a `Runtime`, then route through `context.ipc_call(plugin_id, command, args)`. Frontends have **no compile-time dependency on storage, ai, editor, git** — they see only the IPC abstraction.

This is canonical DIP: high-level policy (CLI verbs, TUI panes, shell screens) depends on the abstraction (IPC), not on the low-level mechanism (SQLite, providers, libgit2).

### 5.3 The dep-invariants test

`crates/nexus-bootstrap/tests/dep_invariants.rs` mechanically asserts the DIP arrows can't be reversed. CI fails if a frontend grows a direct dep on storage. This is a **test that protects an architectural property** — rare and valuable.

### 5.4 Three documented exceptions

`nexus-cli` directly imports `nexus-terminal`, `nexus-collab`, `nexus-security`. Each is annotated in source as a Phase-G / Phase-1 CLI-scope exception, and tracked in ADR 0031 (`docs/archive/pre-0.1.2/adr/0031-cli-scope-exceptions-to-ipc-only.md`). The exceptions are bounded, named, and accountable — not architectural rot.

---

## 6. DRY

### 6.1 Per-handler boilerplate — ~340 reproductions ❌

Counted: **448 `exec_err(format!(...))` call sites** across the workspace, and **~120 in `nexus-storage/src/core_plugin.rs` alone**. Each dispatch arm follows the same five-step shape:

```rust
HANDLER_X => {
    let arg: ArgType = parse_args(args, "x")?;            // step 1: decode
    let result = engine                                    // step 2: call domain
        .do_x(&arg)
        .map_err(|e| exec_err(format!("x: {e}")))?;        // step 3: wrap error
    to_value(&result, "x")                                 // step 4: encode + name
}
```

Across the workspace this dance happens roughly **340 times** (the size of the registered handler set). The cost is paid in:

- New-handler authoring friction (six near-identical lines per command).
- Drift — some arms call `exec_err(format!("name: {e}"))`, others call `exec_err(format!("Name failed: {e}"))`, others elide the prefix. Result: inconsistent IPC error strings reaching the shell.
- Reviewer load — a 1 000-line `match` is hard to diff.

A small `dispatch!` macro or a `Handler` trait per command would localise the four-step recipe into one declarative line per handler. Several crates have already extracted `handlers/<name>.rs` modules (`nexus-ai`, `nexus-workflow`, `nexus-agent`) which addresses the *file*-level duplication but not the per-arm boilerplate.

### 6.2 Helper duplication — `exec_err` × 19 ❌

Counted: **19 separate definitions of `fn exec_err(...) -> PluginError`** scattered across `nexus-storage`, `nexus-editor`, `nexus-terminal`, `nexus-ai/{generate_docs,handlers/shared,activity_log}`, `nexus-workflow/handlers/shared`, `nexus-agent/handlers/shared`, `nexus-skills`, `nexus-templates`, `nexus-database`, `nexus-theme`, `nexus-formats`, `nexus-notifications`, `nexus-collab`, `nexus-audio`, `nexus-comments`, `nexus-git`. Same body, ±10 chars. Same story for:

- `fn parse_args<T: DeserializeOwned>` — re-declared in storage, database, theme, terminal, templates, …
- `fn to_value<T: Serialize>` — re-declared in storage, database, theme, terminal, skills, templates, workflow, agent, …
- `fn path_arg` — re-declared in storage, git.

These are mechanical helpers, no per-crate variation. They belong in **`nexus-plugin-api`** (the trait crate every plugin already links), exported as `nexus_plugin_api::dispatch::{exec_err, parse_args, to_value, path_arg}`. One commit could delete ~300 lines of duplication and align error formatting across the board.

### 6.3 Handler ID constant blocks — duplicated structure ⚠️

Every `core_plugin.rs` opens with a 50–100-line block of `pub const HANDLER_<NAME>: u32 = N;` declarations, then bootstrap re-references each one in a `(name, id)` tuple list:

```rust
// crates/nexus-bootstrap/src/plugins/storage.rs
&with_v1_aliases(&[
    ("query_files", nexus_storage::core_plugin::HANDLER_QUERY_FILES),
    ("read_file",   nexus_storage::core_plugin::HANDLER_READ_FILE),
    ("backlinks",   nexus_storage::core_plugin::HANDLER_BACKLINKS),
    …
])
```

So each handler is named twice (the const + the bootstrap tuple) and the integer id once. A derive macro or a `inventory`-style registration could collapse this to one site. Out of scope for a minimal cleanup, but worth flagging.

### 6.4 Per-plugin registration scaffolding — well factored ✅

By contrast, `crates/nexus-bootstrap/src/plugins/{plugin}.rs` files do **not** duplicate. They share `core_manifest_with_ipc`, `with_v1_aliases`, `LifecycleFlags`, `RegisterCoreResultExt` helpers from `mod.rs`. The 23 per-plugin registration files are 30–80 LOC each and mechanically uniform. That's DRY done right.

### 6.5 Capability list — 33 variants, no duplication ✅

`Capability` is one enum, parsed from one string syntax, used by every plugin. Single source of truth.

### 6.6 IPC type generation pipeline — single point of truth ✅

`scripts/check_ipc_drift.sh` regenerates TS bindings + JSON schemas + capability doc from the Rust types via `ts-rs` + `schemars`. Three downstream artefacts, one upstream definition. No drift possible because CI runs the diff.

---

## 7. Shell-side notes

The TypeScript shell is mostly outside the scope of a Rust SOLID review, but two findings carry over:

- **DIP violation in App.tsx** (`shell/src/shell/App.tsx:8`) — root chrome reaches into `plugins/nexus/workspace/workspaceStore` to read `rootPath`. The shell should expose a slot the plugin publishes to, not import the plugin's store. Tracked as AA-04 in `architecture-adherence.md`.
- **OCP via `ExtensionHost`** — `shell/src/host/ExtensionHost.ts` is the slot/contribution registry. Every visible chrome element is contribution-mounted, not hard-coded. Adding a UI feature = adding a plugin = no shell-source edit. Strong OCP.

---

## 8. Remediation list

Roughly ordered by `(impact ÷ effort)`. None are correctness bugs.

| ID | Item | Type | Effort | Impact |
|----|------|------|:------:|:------:|
| **SD-01** | Promote `exec_err` / `parse_args` / `to_value` / `path_arg` to `nexus_plugin_api::dispatch` and delete the 19 redefinitions. | DRY | S | M |
| **SD-02** | Add a `dispatch!(handler_id, args, [(HANDLER_X, "x", engine.do_x), …])` macro (or `Handler` trait) so the per-arm boilerplate collapses to one line. | DRY / SRP | M | H |
| **SD-03** | Migrate `nexus-storage` / `nexus-editor` / `nexus-terminal` / `nexus-git` to the `handlers/<verb>.rs` + `handlers/shared.rs` layout already used by `nexus-ai` / `nexus-workflow` / `nexus-agent`. | SRP | M | H |
| **SD-04** | Split `PluginContext` into supertraits (`Identity` + `FileSystem` + `KvAccess` + `Events` + `Ipc` + `Log`) with the current trait kept as auto-impl umbrella. Lets pure-compute plugins depend on `Identity + Log`. | ISP | M | M |
| **SD-05** | Split `CorePlugin` into `SyncCorePlugin` + `AsyncCorePlugin`, or unify to a single `async fn dispatch`. Removes the "AI command is async; caller should use dispatch_async" sentinel-error pattern. | ISP | M | M |
| **SD-06** | Drive handler-id ↔ name registration from a single source (derive macro or `inventory` slice) so bootstrap doesn't repeat each name. | DRY | M | L |
| **SD-07** | Add a `#![warn(clippy::too_many_lines)]` or a file-LOC budget to CI for `core_plugin.rs` so the four largest files can't grow further. | SRP guardrail | S | M |
| **SD-08** | Fix `shell/src/shell/App.tsx:8` — invert the dependency on `plugins/nexus/workspace/workspaceStore`. (Already tracked as AA-04.) | DIP | S | L |

## 9. What this assessment did *not* cover

- **Provider-trait LSP audit per crate** — sampled high-level shape; a per-provider audit (do all `AiProvider` impls return the same `Unsupported` shape? do all `TtsProvider` impls honour the same async cancellation contract?) is the right next layer.
- **Test-double LSP** — verified test contexts construct via the same impl; did not exhaustively check every fixture-mock honours postconditions.
- **Shell plugin (TypeScript) SOLID** — App.tsx flagged via existing audit; the `ExtensionHost` + slot registry surface deserves its own pass.
- **WASM community-plugin ABI** — `nexus-plugins/src/loader.rs` and `host_fns.rs` not deeply read; the WASM boundary is its own SOLID/security review.

---

## Summary

The Nexus *macro architecture* is one of the cleanest applications of SOLID I've seen at this size:

- **D** is mechanically enforced by a CI test.
- **O** is realised by the plugin model and the data-driven cap matrix.
- **L** holds for every trait sampled.

The *micro implementation* has two consistent smells, both tractable:

- **S** is broken at the file level for the four largest `core_plugin.rs` files. Already partially mitigated by the `handlers/` split in `nexus-ai`, `nexus-workflow`, `nexus-agent` — make it uniform.
- **DRY** is broken at the per-handler boilerplate level. ~340 dispatch arms repeat the same 4-step recipe, 19 crates redefine the same private helpers. One small macro + one promotion to `nexus-plugin-api` would close most of the gap.

**I** is a deliberate trade — the wide `PluginContext` is convenient for plugin authors at the cost of test-mock weight. Splittable into supertraits without breaking callers.

None of these gate a 0.1.2 release. All are hardening / maintainability investments that pay back the more handlers the project gains.
