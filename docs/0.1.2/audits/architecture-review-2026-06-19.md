# Architecture Review — Kernel / AI-first-class / Everything-a-Plugin

> **As of:** 2026-06-19. A focused review answering three questions: does the
> code adhere to the **microkernel architecture**, is **AI a first-class
> citizen**, and is **everything set up as a plugin**? Verdicts are grounded in
> `Cargo.toml` reads, the enforcement tests, and direct source reads — when the
> archived docs disagree with the code, the code wins.
>
> This is a dated, point-in-time review scoped to the three questions above. The
> broader living audit is [`../architecture-adherence.md`](../architecture-adherence.md);
> this doc defers to it for the full invariant matrix rather than duplicating it.

## TL;DR

| Axis | Verdict | One-liner | Enforced by |
|------|:------:|-----------|-------------|
| **Kernel architecture** | ✅ | `nexus-kernel` links only the two leaf crates; every IPC call passes capability gates before dispatch. | `dep_invariants.rs`, `kernel_free_guard.rs`, `cap_matrix_complete.rs` |
| **AI is first-class** | ✅ | AI is three CorePlugins reachable uniformly via `ipc_call` from every frontend — not a bolt-on, not frontend-special-cased. | bootstrap registration + `cap_matrix.toml` + `dep_invariants.rs` |
| **Everything-a-plugin** | ✅ | 24 backend CorePlugins; the shell starts empty and mounts only plugin contributions; the Tauri bridge carries zero feature commands. | `bootstrap_coverage.rs`, `tauri_command_boundary.rs`, `plugin-import-hygiene.test.ts` |

**No architectural violations were found on any axis.** Adherence is enforced by
unconditional CI tests (no `#[ignore]`), so regressions fail the build rather
than relying on convention. The only items surfaced are minor documentation
drift (stale counts), already tracked as hardening — see [§4](#4-minor-items--doc-drift-not-violations).

---

## 1. Kernel architecture adherence ✅

### 1.1 Microkernel isolation (invariant #2)

`crates/nexus-kernel/Cargo.toml` depends only on the two leaf crates
`nexus-types` and `nexus-plugin-api`, plus kernel-agnostic external utilities
(`tokio`, `tokio-util`, `tracing`, `serde`/`serde_json`, `toml`, `uuid`,
`chrono`, `thiserror`, `async-trait`). There are **zero** subsystem dependencies
and no `rusqlite` / `nexus-kv` leakage.

- `nexus-plugin-api` is held kernel-free by
  `crates/nexus-plugin-api/tests/kernel_free_guard.rs` — a 37-name forbidden
  list scanned at both the `Cargo.toml` and source level, unconditionally.
- The IPC-proxy crates (`nexus-mcp`, `nexus-acp`, `nexus-remote`) are allowlisted
  in `crates/nexus-bootstrap/tests/dep_invariants.rs` to link *only*
  kernel/plugins/plugin-api/types/bootstrap — so they can reach subsystems only
  through `ipc_call`. The test also self-checks against synthesised
  `cfg`-conditional deps so a `[target.'cfg(unix)'.dependencies]` foot-gun can't
  slip past.
- Subsystems depend on the kernel; the kernel never depends on a subsystem. The
  one cross-service edge worth noting — `nexus-crdt → nexus-editor` — is
  domain-adjacent (collaborative editing over the editor's `Operation`/`BlockTree`
  types) and carries a rationale comment in its `Cargo.toml`.

### 1.2 IPC over direct calls (invariant #3)

Every kernel-mediated call funnels through a single choke point —
`ipc_call_inner` in `crates/nexus-kernel/src/context_impl.rs`. Three gates run
**before** the handler is reached, each audit-logged on failure:

1. Unconditional `Capability::IpcCall` check.
2. Args-aware per-handler requirements via
   `IpcDispatcher::required_caller_caps_for_args(...)` (ADR 0022 Phase 2) — so a
   handler can demand different caps depending on its arguments.
3. Trust-level gate (`is_handler_internal_only(...)`, P1-02) — internal-only
   handlers require `TrustLevel::Core`.

CLI, TUI, and the Tauri shell all reach storage / AI / editor / etc. through
`context.ipc_call(...)` (the shell via the `kernel_invoke` bridge command). The
shell's Tauri crate links zero service crates (`nexus-kernel`, `nexus-bootstrap`,
`nexus-plugin-api`, `nexus-plugins`, `nexus-panic-log`, `nexus-remote` only).

**Documented exceptions:** three single-process CLI-scope direct imports exist in
`nexus-cli` (`term.rs → nexus_terminal`, `collab.rs → nexus_collab` /
`nexus_security`). Each is annotated inline and codified by
[ADR 0031](../../archive/pre-0.1.2/adr/0031-cli-scope-exceptions-to-ipc-only.md)
with a Phase-H migration plan — these are deliberate exceptions, not drift.

### 1.3 Capabilities gate everything (invariant #4)

- Filesystem operations are capability-gated **and** path-confined: `confine_path`
  canonicalises and prefix-checks against the forge root, and writes use a
  TOCTOU-safe validator (canonicalise-deepest-existing-ancestor, then rebuild the
  target) to close the symlink-swap race. Escapes return `PermissionDenied` and
  log `log_path_traversal_denied`.
- All ~347 IPC handlers across ~20 plugin ids are classified in
  `crates/nexus-bootstrap/cap_matrix.toml`; every row carries either `caps = [...]`
  or `unrestricted = "<rationale>"`. `cap_matrix_complete.rs` walks every
  registered `(plugin, command)` and fails the build if a row is missing — run
  unconditionally.

### 1.4 File-as-truth (invariant #1)

Only `nexus-storage` writes `.forge/index.db` and the Tantivy index in production
code (it owns the watcher per ADR 0003); the remaining writers are test fixtures.
No production code treats the index as the source of record.

> Full invariant detail, the forbidden-dep table, and the topic-prefix invariant
> live in [`../architecture-adherence.md`](../architecture-adherence.md) §M-A…§M-H.

---

## 2. AI is a first-class citizen ✅

AI is not bolted onto a frontend — it is a set of **three proper CorePlugins**,
registered at bootstrap, reachable uniformly through the same IPC seam as every
other subsystem, and capability-gated.

### 2.1 Registered as CorePlugins

| Plugin id | Handlers | Role |
|-----------|:--------:|------|
| `com.nexus.ai` | 28 | provider traits, embeddings, RAG, chat/stream, sessions, indexing, tool loop, entity enrichment, completion |
| `com.nexus.ai.runtime` | 12 | shared tokio worker pool + task scheduling (BL-134) |
| `com.nexus.agent` | 26 | Plan/Step planner, session management, tool execution |

Registration is in `crates/nexus-bootstrap/src/plugins/mod.rs`. Notably
`ai_runtime` registers **before** `ai` (`mod.rs:64-65`) so the runtime's shared
pool handle is published (via `WorkerPool::publish_shared_handle` in its
`wire_context`) before the AI indexing daemon starts — with a `None` fallback so
boot survives if the runtime plugin is absent. `com.nexus.memory` (registered
right after storage) is a sibling that leans on AI over IPC for embeddings and
wiki synthesis.

### 2.2 Reachable uniformly from every frontend

Every frontend reaches AI through the same `ipc_call` surface — there are **zero**
AI-specific commands in the Tauri bridge:

- **CLI** (`nexus-cli/src/commands/ai.rs`) routes every call through `ipc_call`.
- **TUI** routes through the bootstrap `KernelPluginContext`; no `nexus-ai` dep.
- **MCP** (`nexus-mcp`) dispatches tool calls to `com.nexus.ai` over IPC.
- **Shell** — AI chat, AI settings, margin-suggest, and inline prediction are all
  plugin contributions under `shell/src/plugins/nexus/` invoking
  `com.nexus.ai::{stream_chat, predict, …}` via the generic `kernel_invoke`
  bridge. No bespoke `#[tauri::command]` for AI.

### 2.3 Clean composition, no dependency cycles

The AI graph composes over IPC, not Cargo edges:

- `nexus-agent` calls `com.nexus.ai::propose_tool_calls` and **does not**
  Cargo-depend on `nexus-ai` (enforced by `dep_invariants.rs`).
- `nexus-memory` calls `com.nexus.ai::embed_text` and generation over IPC.
- The only AI internal Cargo edge — `nexus-ai → nexus-ai-runtime` — is a
  read-only `shared_pool_handle()` Arc share (no IPC reach-back), documented at
  `nexus-ai/Cargo.toml`.

### 2.4 Capability-gated and configurable

Caller-facing handlers are gated by granular caps in `cap_matrix.toml`
(`ai.chat`, `ai.index`, `ai.session.read`/`write`, `ai.config.write`,
`ai.tools.write`/`mcp`), several **args-aware** — e.g. `stream_chat` only requires
`ai.tools.write` when `tools=auto`. Configuration is externalised to
`<forge>/.forge/ai.toml` (`AiConfig` in `nexus-ai/src/config.rs`) with
per-provider overrides and a runtime `set_config` handler; nothing critical is
hardcoded.

> See [`../memory.md`](../memory.md) for how the memory engine consumes AI over IPC.

---

## 3. Everything is set up as a plugin ✅

### 3.1 Backend — 24 CorePlugins, the rest correctly non-plugins

Exactly **24** CorePlugins are registered deterministically in
`crates/nexus-bootstrap/src/plugins/mod.rs:50-86` (security first so audit events
route, storage next, then the rest in historical order). The workspace
(`Cargo.toml:4-47`) has **42** members; the other **18** are correctly *not*
plugins:

- **Infrastructure:** `nexus-kernel`, `nexus-bootstrap`, `nexus-plugins`,
  `nexus-plugin-api`, `nexus-types`, `nexus-kv`, `nexus-hashline`.
- **Frontends:** `nexus-cli`, `nexus-tui`, `nexus-remote`.
- **Vendored leaf libs:** `nexus-rush` (bundled shell, RFC 0002), `nexus-vt`
  (headless VT core behind `nexus-terminal`, RFC 0003), `nexus-crdt`.
- **Utilities:** `nexus-panic-log`, `nexus-fuzz`.
- **Standalone binary:** `nexus-memory-hub` (deployable HTTP sync server).
- **Not-yet-wired staging libs:** `nexus-context`, `nexus-protocol` (tracked by #188).

`crates/nexus-bootstrap/tests/bootstrap_coverage.rs` asserts every workspace
member is either registered as a plugin or explicitly exempted — so a new service
crate can't silently skip registration.

### 3.2 Shell — starts empty, mounts only contributions

`shell/src/shell/App.tsx` mounts only slot/workspace containers
(`<SlotSurface>`, `<Workspace>`, `<ErrorBoundary>`) — no plugin imports, reading
`nexus.workspace.rootPath` from the shell-owned `ContextKeyService` rather than
any plugin store. ~67 plugins across `shell/src/plugins/{core,nexus,community}/`
load dynamically through `ExtensionHost` (eager/lazy in dependency order) and
register contributions via `SlotRegistry`. The empty-by-default invariant is
test-enforced: `shell/tests/plugin-import-hygiene.test.ts` forbids host/chrome
code from importing plugin internals (with an annotated allowlist for the known
composition-root seams).

### 3.3 Thin Tauri bridge — no feature commands

The bridge in `shell/src-tauri/src/lib.rs` registers exactly **29** commands, all
shell-management or host-platform primitives — 10 kernel+bridge, 5 plugin-mgmt,
6 persistence, 3 utility, 5 popout. None are feature capabilities; feature work
routes through `kernel_invoke → ipc_call`. The borderline case, `notify_desktop`,
is a legitimate host-platform primitive (only the bridge can reach
`tauri_plugin_notification`; the backend `com.nexus.notifications` plugin invokes
it). `crates/nexus-bootstrap/tests/tauri_command_boundary.rs` pins the command set
(and lives in the cargo workspace so `cargo test --workspace` actually runs it,
since `nexus-shell` is outside the workspace).

### 3.4 Community plugins — sandboxed and capability-gated both ways

- **WASM** via `wasmtime` (`crates/nexus-plugins/src/sandbox.rs`): per-call fuel +
  epoch deadline; host imports (`ipc_call`, `publish_event`, `kv_get`/`set`,
  `log`) each check the cap matrix.
- **JS** via iframe (`shell/src/host/sandbox/`): RPC bridge with a
  `capabilityGuard` method→capability map and a handshake-bound `pluginId`
  (`assertValidPluginId` rejects empty/colon-bearing ids) so a plugin can't spoof
  another's identity.

> Per-plugin contracts: [`../plugins/core.md`](../plugins/core.md) and
> [`../plugins/community.md`](../plugins/community.md).

---

## 4. Minor items — doc drift, not violations

None of these are architectural defects; they are stale counts and pre-existing
hardening already on the books (mapped to the AA-IDs in
[`../architecture-adherence.md`](../architecture-adherence.md) so they aren't
re-opened as new work).

| Item | Observed | Live value | Bucket |
|------|----------|-----------|--------|
| Workspace crate count | `CLAUDE.md` says 41, [`../crates.md`](../crates.md)/[`../README.md`](../README.md) say "39 crates" | **42** members (`Cargo.toml:4-47`) | AA-09 (doc-sync) |
| IPC handler count | [`../README.md`](../README.md) & [`../ipc-handlers.md`](../ipc-handlers.md) say "~280" | **~347** classified rows (`cap_matrix.toml`) | AA-09 (doc-sync) |
| Shell plugin count | [`../shell.md`](../shell.md) cites ~17 core / ~65 nexus | filesystem ~7 core / ~60 nexus; 67 catalog entries | AA-09 (doc-sync) |

Open hardening (defense-in-depth, already tracked):

- **AA-01** — extend `dep_invariants.rs` FORBIDDEN list so `nexus-mcp/acp/remote`
  also can't link `nexus-terminal/editor/git/database` (they don't today).
- **AA-02** — add a parallel dep test for `shell/src-tauri/Cargo.toml` (it sits
  outside the cargo workspace, so the existing test can't see it).
- **AA-05** — add a "Host-platform primitives" section to [`../shell.md`](../shell.md).
- **AA-07** — extend `scripts/check_ipc_drift.sh` to cover `nexus-security` and
  `nexus-collab` (both have IPC types not yet ts-exported).
- **AA-08** — a dedicated iframe-sandbox security/red-team pass (the wiring is
  confirmed; the escape paths were not exhaustively audited here).

---

## 5. Verdict

The architecture is **strongly adhered to** on all three axes the review set out
to verify:

- **Kernel architecture** — microkernel isolation, IPC-over-direct-calls, and
  capability-gating all hold, enforced by unconditional tests. ✅
- **AI is a first-class citizen** — three proper CorePlugins, uniformly
  IPC-reachable from every frontend, capability-gated, composing without cycles,
  with zero frontend special-casing. ✅
- **Everything is a plugin** — 24 backend CorePlugins with coverage enforced; the
  shell starts empty and mounts only contributions; the Tauri bridge carries no
  feature commands; community plugins are sandboxed and capability-gated. ✅

The remaining items are documentation sync and previously-identified hardening —
none are bugs, and none are release-blocking.
