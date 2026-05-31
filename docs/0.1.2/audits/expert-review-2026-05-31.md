# Nexus — Multi-Disciplinary Expert Review

> **As of:** 2026-05-31. Scope: full workspace (38 crates, shell, packages, docs) reviewed across six lenses — architecture, systems, code quality, UI/frontend, integration, and test/QA. Each item is `file:line`-grounded so you can jump straight to the work.
>
> **Status legend:** open items are unchecked `[ ]`. Mark closed items `[x] ✅ Closed` with the commit SHA, mirroring [`gaps-inconsistencies-2026-05-21.md`](gaps-inconsistencies-2026-05-21.md).
>
> **Headline:** the four microkernel invariants hold and two are test-enforced (`dep_invariants.rs`, cap-matrix completeness). Findings are *evolution artifacts and enforcement gaps*, not structural decay. The single highest-leverage fix is **R1 (CI)** — it activates the ~4,200 existing tests, the drift gate, and clippy/fmt in one move and would have prevented R2.

## Severity counts

| Severity | Open |
|----------|------|
| Critical | 1 |
| High | 3 |
| Medium | 6 |
| Low | 8 |

---

## Critical

### R1. No CI runs tests, lint, fmt, or typecheck — merges to `main` are gated on nothing
- [ ] **Open**

`.github/workflows/` contains only `ipc-drift-check.yml` and `release-windows.yml`. None of the ~4,200 Rust tests, `cargo clippy`, `cargo fmt --check`, `pnpm test/lint/typecheck` run on PRs. The only test runners are `scripts/test_*.sh` with a hard-coded WSL path. The test *authoring* quality is excellent (A); the *enforcement* is absent (D). This is the root cause that let R2 slip through.

**Fix:** Add a PR-gated workflow: `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --check`, and `pnpm -r {test,lint,typecheck}`, reusing the existing `Swatinem/rust-cache`. Broaden the IPC-drift trigger beyond its hard-coded path filter or fold it into the test job.

---

## High

### R2. IPC bindings are drifted now — committed tree fails the project's own drift gate
- [ ] **Open**

`IpcError::Cancelled` / `IpcErrorKind::Cancelled` were added 2026-05-21 (`687eb49`, "cooperative IPC cancellation") in `crates/nexus-plugin-api/src/error.rs`, but the generated `packages/nexus-extension-api/src/generated/IpcError.ts` and `IpcErrorKind.ts` were last regenerated 2026-05-20 (`c56e5aa`) and contain zero `Cancelled`/`cancelled`. `scripts/check_ipc_drift.sh` exits 1 today. **Live impact:** the shell bridge emits `IpcErrorKind::Cancelled` on popout-window close (`shell/src-tauri/src/bridge.rs:677`); a frontend typed against the stale union has no `"cancelled"` case, so window-close cancellations surface as an unknown kind. Consumers: `shell/src/host/KernelIpcError.ts`, `shell/src/host/PluginAPI.ts`, `packages/nexus-extension-api/src/sandbox/{context,protocol}.ts`.

**Fix:** Run `scripts/check_ipc_drift.sh` to completion on CI-class hardware (a full run cold-compiles ~25 crates with `--features ts-export`) and commit the regenerated bindings — including anything it surfaces on the other 23 wired crates.

### R3. WASM sandbox IPC bridge is a *weaker* contract than the kernel path
- [ ] **Open**

`host::invoke_command` (`crates/nexus-plugins/src/host_fns.rs:508-610`) checks only `Capability::IpcCall`. It does **not** call `required_caller_caps_for_args` or `is_handler_internal_only`, so a sandboxed WASM plugin holding only `ipc.call` can reach per-handler-cap-gated and even `internal = true` handlers that the same plugin could not reach through the kernel context (`crates/nexus-kernel/src/context_impl.rs:164-187`). It is also sync-only (`dispatch()`, no `dispatch_async`) — no async handlers, no timeout/cancellation — and collapses every failure to an opaque `HOST_ERROR (-1)`, dropping the `IpcErrorEnvelope` the rest of the system standardized on. The sandboxed tier is supposed to be a *subset* of the in-tree path; here it is less restricted. Related to the amplifier-laundering gap (#77).

**Fix:** Route `host::invoke_command` through the same `required_caller_caps_for_args` + `is_handler_internal_only` checks as the kernel `ipc_call`; return a serialized `IpcErrorEnvelope` (or distinct codes per `IpcErrorKind`); add the async dispatch path with timeout/cancellation.

### R4. `@nexus/extension-api` exposes three divergent plugin contracts; the "frozen 1.0.0" one is unimplemented
- [ ] **Open**

The headline `NexusPluginContext` / `ScriptPlugin` advertised as frozen 1.0.0 (`packages/nexus-extension-api/src/index.ts:206,440`) is implemented by **no runtime**. The actual sandbox runtime implements a different shape (`packages/nexus-extension-api/src/sandbox/plugin.ts:13`), and the in-process shell host implements a third (`shell/src/types/plugin.ts:177`). Plugin authors coding against the published contract target a surface the host doesn't provide.

**Fix:** Make one shape canonical; add a type-level conformance test asserting the runtime context satisfies the exported contract; drop the "1.0.0 frozen" framing until it's true.

---

## Medium

### R5. Three substantial crates compile but are never wired by bootstrap
- [ ] **Open**

`nexus-memory` (935 LoC, AI memory layer), `nexus-context` (563 LoC, context-assembly pipeline), and `nexus-protocol` (677 LoC, speech-act layer) are real code, not stubs, but none is a dependency of `nexus-bootstrap` (`crates/nexus-bootstrap/Cargo.toml:46-75`), so none is registered as a core plugin or reachable via `ipc_call`. Only `nexus-context` consumes `nexus-memory`; `nexus-protocol` has zero in-tree consumers. They are the recent "Moves 4–7" landing ahead of integration. As-is they undercut the "every subsystem registered by bootstrap" claim.

**Fix:** Finish wiring through bootstrap + cap-matrix, or feature-gate/exclude from `members` until landed. Add a test asserting every non-leaf, non-frontend crate is registered by bootstrap.

### R6. IPC-laundering via amplifier plugins (kernel-side enforcement incomplete)
- [ ] **Open** (tracked as #77)

Unlisted handlers default to `IpcCall`-only (`crates/nexus-plugins/src/loader.rs:2728-2737` returns empty = no extra caps). Amplifier plugins (agent, workflow) holding `IpcCall` can launder calls into high-impact handlers (`terminal::create_session`, `mcp::connect`) the matrix doesn't gate further. Mitigated by scoping agent/workflow contexts to minimal cap sets (`crates/nexus-bootstrap/src/lib.rs:573-688`) and audit-tagged `tracing::warn!`, but kernel-side enforcement of the transitive surface is not complete. Honestly documented at `docs/0.1.2/capabilities.md:111`.

**Fix:** Adopt a kernel-side default-deny posture for high-impact handlers (require explicit cap-matrix opt-in rather than `IpcCall`-only default), or formally accept the risk with the mitigation set documented.

### R7. Generated-artifact asymmetry: 335 ts-rs bindings vs 257 schemars schemas
- [ ] **Open** (tracked as #113)

Hand-rolled `serde_json::Value` handlers (`nexus-storage::read_file`, `nexus-git`, `nexus-mcp`, `nexus-lsp`, `nexus-dap`) read fields by hand and emit ad-hoc `json!` responses instead of typed `#[serde(deny_unknown_fields)]` structs, so they're invisible to both the `ipc_strictness` gate and the JSON-schema generator (`crates/nexus-bootstrap/tests/ipc_strictness.rs:18-23`). MCP/external consumers get an incomplete schema picture and unknown-field regressions on these handlers go uncaught.

**Fix:** Migrate the remaining hand-rolled handlers to typed `deny_unknown_fields` structs.

### R8. Oversized modules hurt maintainability
- [ ] **Open**

Largest offenders: `crates/nexus-terminal/src/core_plugin.rs` (3,541), `crates/nexus-plugins/src/loader.rs` (3,437), `shell/src/plugins/core/settings/SettingsPanelView.tsx` (3,378), `crates/nexus-storage/src/lib.rs` (2,987), `crates/nexus-plugins/src/manifest.rs` (2,549), `crates/nexus-cli/src/main.rs` (2,460), `shell/src/plugins/nexus/editor/index.ts` (2,509). The 23 `#[allow(clippy::too_many_lines)]` sites are the worklist.

**Fix:** Split IPC command handlers into per-command-group submodules; decompose the large React view. Start with `nexus-terminal/core_plugin.rs` and `SettingsPanelView.tsx`.

### R9. Systems lifecycle / scaling refinements
- [ ] **Open**

- Tantivy `IndexReader` is rebuilt per query rather than reused (`crates/nexus-storage/src/search.rs:165-170`).
- ai-runtime run-store `inner` map is never evicted — slow memory leak over long sessions (`crates/nexus-ai-runtime/src/scheduler.rs:9-12`).
- `on_stop` doesn't drain in-flight AI tasks before returning (`crates/nexus-ai-runtime/src/core_plugin.rs:291`).

**Fix:** Cache/reload the Tantivy reader; bound the run-store with eviction; drain or cancel in-flight tasks on shutdown.

### R10. Host→plugin layering inversion in the shell
- [ ] **Open**

The shell host statically imports the editor plugin (`fencedCodeRegistry`, `useEditorStore`) at `shell/src/host/PluginAPI.ts:13,20`, so `api.editor` breaks if the editor plugin is absent. `api.fs` / `configuration` / `notifications` silently no-op when their backing service is missing (`shell/src/host/PluginAPI.ts:255-340`) rather than surfacing a clear error.

**Fix:** Invert the dependency (editor registers its surface with the host) and make missing-service calls fail loudly.

---

## Low

### R11. Doc drift: stale crate and command counts
- [ ] **Open**

Workspace has 38 members (`Cargo.toml`) but docs say 35: `README.md:9`, `CLAUDE.md`, `docs/0.1.2/architecture.md:7`, `docs/0.1.2/crates.md:5,49`. Shell registers 30 Tauri commands but docs say 29: `CLAUDE.md`, `docs/0.1.2/shell.md`. Add the three new crates (`nexus-memory`, `nexus-context`, `nexus-protocol`) to the inventory tables.

### R12. Sandbox-incompatible `window.prompt`/`confirm` still present (4 sites)
- [ ] **Open** — break inside the null-origin iframe; route through the host dialog surface.

### R13. Sandbox watchdog can false-crash healthy plugins
- [ ] **Open** — guest never pongs, so a responsive plugin can be torn down. Add a guest-side heartbeat/pong.

### R14. Accessibility baseline missing on ~half of views
- [ ] **Open** — establish an a11y baseline (focus management, ARIA roles, keyboard nav) and a lint/test guard.

### R15. ~234 production `.unwrap()` calls
- [ ] **Open** — concentrated in `nexus-storage`, `nexus-editor`, `nexus-git`. One-time audit pass to convert hot-path sites to `?`/`expect("context")`. (95% of the raw 4,379 unwraps are in tests.)

### R16. `panic=abort` + lock-poison `.expect()` escalates one subsystem panic to full crash
- [ ] **Open** — review poison-`.expect()` sites so a single plugin panic can't abort the whole process.

### R17. Sync-handler timeout is advisory only
- [ ] **Open** — a sync handler on `spawn_blocking` can't be preempted by the deadline; document the limitation or move long sync work off the blocking pool.

### R18. `dep_invariants.rs` is a denylist, not an allowlist
- [ ] **Open** — a new frontend/proxy crate could link a subsystem directly and pass. Add an allowlist variant for frontend/proxy crates. Also consider `#[doc(hidden)]`/sealing the `pub Runtime.loader` handle (`crates/nexus-bootstrap/src/lib.rs:142`), the one intentionally IPC-porous public surface.

### R19. Stray `console.*` calls and module-wide `#![allow(dead_code)]`
- [ ] **Open** — 83 raw `console.*` in `shell/src` bypass `clientLogger.ts` (add a `no-console` eslint rule); ~5 crate/module-wide `#![allow(dead_code)]` (e.g. `crates/nexus-storage/src/schema.rs:8`, `crates/nexus-editor/src/excerpt_map.rs:31`) should be per-item or removed once wired.

---

## What's strong (preserve)

- **Real microkernel:** kernel depends only on the two leaf crates; the shell links no subsystem engine crates (`shell/src-tauri/Cargo.toml:51-57`) — it physically cannot make a direct feature call. Enforced by `dep_invariants.rs` with a self-test.
- **File-as-truth** is the best-realized invariant: `write_file` orders disk → SQLite txn → graph, mutating derived state only after `tx.commit()`; `rebuild_index` provably reconstructs from disk (`crates/nexus-storage/src/lib.rs:198-336,1385-1452`).
- **IPC dispatch path** is the best-engineered code in the repo: capability gate → per-arg caps → trust gate before dispatch, biased cancel-first `tokio::select!`, child-token propagation, panic→`PluginCrashedDuringCall` (`crates/nexus-kernel/src/context_impl.rs:145-263`).
- **`IpcErrorEnvelope`** gives CLI/TUI/MCP/shell/remote one uniform, exhaustively-tested error surface.
- **Architecture-invariant tests** (dep matrix, cap-matrix completeness, registration order) are mature and self-guarding.
- **Near-zero debt:** 2 Rust TODOs (both false positives), 0 `todo!()`/`unimplemented!()`, no `unsafe` in core crates, `deny(missing_docs)` on 10+ crates, `strict: true` TS with no real production `as any`.

---

## Recommended sequence

1. **R1** — add CI test/lint workflow (unblocks everything else).
2. **R2** — run drift script to completion, commit regenerated bindings.
3. **R3** — close WASM `host::invoke_command` capability-parity gap.
4. **R5** — decide fate of `nexus-memory`/`context`/`protocol`.
5. **R4** — reconcile the `@nexus/extension-api` contract.
6. Then the maintainability/scaling backlog: **R8, R9, R11, R7**, and the Low items.
