# Nexus — Multi-Disciplinary Expert Review

> **As of:** 2026-05-31. Scope: full workspace (38 crates, shell, packages, docs) reviewed across six lenses — architecture, systems, code quality, UI/frontend, integration, and test/QA. Each item is `file:line`-grounded so you can jump straight to the work.
>
> **Status legend:** open items are unchecked `[ ]`. Mark closed items `[x] ✅ Closed` with the commit SHA, mirroring [`gaps-inconsistencies-2026-05-21.md`](gaps-inconsistencies-2026-05-21.md).
>
> **GitHub issues:** R1–R19 are tracked as [#184–#202](https://github.com/baileyrd/nexus/issues?q=label%3Aexpert-review-2026-05-31) (label `expert-review-2026-05-31`). R6 (#189) follows up closed #77; R7 (#190) follows up closed #113.
>
> **Headline:** the four microkernel invariants hold and two are test-enforced (`dep_invariants.rs`, cap-matrix completeness). Findings are *evolution artifacts and enforcement gaps*, not structural decay. The single highest-leverage fix is **R1 (CI)** — it activates the ~4,200 existing tests, the drift gate, and clippy/fmt in one move and would have prevented R2.

## Severity counts

| Severity | Open |
|----------|------|
| Critical | 0 |
| High | 2 |
| Medium | 1 |
| Low | 1 |

> **Status as of 2026-06-10:** 15 of 19 items closed (see SHAs below). Still open: R3 (#186, re-scoped — only the async-dispatch leg remains), R4 (#187), R5 (#188), R16 (#199). Follow-up review: [`repo-review-2026-06-10.md`](repo-review-2026-06-10.md).

---

## Critical

### R1. No CI runs tests, lint, fmt, or typecheck — merges to `main` are gated on nothing
- [x] ✅ Closed — `5f46689` (PR-gated `ci.yml`: cargo test/clippy/fmt + pnpm lint/typecheck/test), hardened by `975cf39`, `da41404`

`.github/workflows/` contains only `ipc-drift-check.yml` and `release-windows.yml`. None of the ~4,200 Rust tests, `cargo clippy`, `cargo fmt --check`, `pnpm test/lint/typecheck` run on PRs. The only test runners are `scripts/test_*.sh` with a hard-coded WSL path. The test *authoring* quality is excellent (A); the *enforcement* is absent (D). This is the root cause that let R2 slip through.

**Fix:** Add a PR-gated workflow: `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --check`, and `pnpm -r {test,lint,typecheck}`, reusing the existing `Swatinem/rust-cache`. Broaden the IPC-drift trigger beyond its hard-coded path filter or fold it into the test job.

---

## High

### R2. IPC bindings are drifted now — committed tree fails the project's own drift gate
- [x] ✅ Closed — `54258f0` (regenerated bindings incl. `Cancelled`; drift gate green on CI)

`IpcError::Cancelled` / `IpcErrorKind::Cancelled` were added 2026-05-21 (`687eb49`, "cooperative IPC cancellation") in `crates/nexus-plugin-api/src/error.rs`, but the generated `packages/nexus-extension-api/src/generated/IpcError.ts` and `IpcErrorKind.ts` were last regenerated 2026-05-20 (`c56e5aa`) and contain zero `Cancelled`/`cancelled`. `scripts/check_ipc_drift.sh` exits 1 today. **Live impact:** the shell bridge emits `IpcErrorKind::Cancelled` on popout-window close (`shell/src-tauri/src/bridge.rs:677`); a frontend typed against the stale union has no `"cancelled"` case, so window-close cancellations surface as an unknown kind. Consumers: `shell/src/host/KernelIpcError.ts`, `shell/src/host/PluginAPI.ts`, `packages/nexus-extension-api/src/sandbox/{context,protocol}.ts`.

**Fix:** Run `scripts/check_ipc_drift.sh` to completion on CI-class hardware (a full run cold-compiles ~25 crates with `--features ts-export`) and commit the regenerated bindings — including anything it surfaces on the other 23 wired crates.

### R3. WASM sandbox IPC bridge is a *weaker* contract than the kernel path
- [ ] **Open — re-scoped.** Capability parity (`required_caller_caps_for_args` + `is_handler_internal_only`) and distinct `HOST_ERR_*` codes landed in `59d2fc9` (`host_fns.rs:651-690`). Remaining: the bridge still uses sync `dispatch()` (`host_fns.rs:702`) — no async path, timeout, or cancellation. Tracked in #186.

`host::invoke_command` (`crates/nexus-plugins/src/host_fns.rs:508-610`) checks only `Capability::IpcCall`. It does **not** call `required_caller_caps_for_args` or `is_handler_internal_only`, so a sandboxed WASM plugin holding only `ipc.call` can reach per-handler-cap-gated and even `internal = true` handlers that the same plugin could not reach through the kernel context (`crates/nexus-kernel/src/context_impl.rs:164-187`). It is also sync-only (`dispatch()`, no `dispatch_async`) — no async handlers, no timeout/cancellation — and collapses every failure to an opaque `HOST_ERROR (-1)`, dropping the `IpcErrorEnvelope` the rest of the system standardized on. The sandboxed tier is supposed to be a *subset* of the in-tree path; here it is less restricted. Related to the amplifier-laundering gap (#77).

**Fix:** Route `host::invoke_command` through the same `required_caller_caps_for_args` + `is_handler_internal_only` checks as the kernel `ipc_call`; return a serialized `IpcErrorEnvelope` (or distinct codes per `IpcErrorKind`); add the async dispatch path with timeout/cancellation.

### R4. `@nexus/extension-api` exposes three divergent plugin contracts; the "frozen 1.0.0" one is unimplemented
- [ ] **Open** — interim honest-status documentation landed in `3b66d4d`; contract unification still pending (#187).

The headline `NexusPluginContext` / `ScriptPlugin` advertised as frozen 1.0.0 (`packages/nexus-extension-api/src/index.ts:206,440`) is implemented by **no runtime**. The actual sandbox runtime implements a different shape (`packages/nexus-extension-api/src/sandbox/plugin.ts:13`), and the in-process shell host implements a third (`shell/src/types/plugin.ts:177`). Plugin authors coding against the published contract target a surface the host doesn't provide.

**Fix:** Make one shape canonical; add a type-level conformance test asserting the runtime context satisfies the exported contract; drop the "1.0.0 frozen" framing until it's true.

---

## Medium

### R5. Three substantial crates compile but are never wired by bootstrap
- [ ] **Open** — reframed as staging libraries with `bootstrap_coverage.rs` guard in `2a65225`; Phase-2 wiring decision still pending (#188).

`nexus-memory` (935 LoC, AI memory layer), `nexus-context` (563 LoC, context-assembly pipeline), and `nexus-protocol` (677 LoC, speech-act layer) are real code, not stubs, but none is a dependency of `nexus-bootstrap` (`crates/nexus-bootstrap/Cargo.toml:46-75`), so none is registered as a core plugin or reachable via `ipc_call`. Only `nexus-context` consumes `nexus-memory`; `nexus-protocol` has zero in-tree consumers. They are the recent "Moves 4–7" landing ahead of integration. As-is they undercut the "every subsystem registered by bootstrap" claim.

**Fix:** Finish wiring through bootstrap + cap-matrix, or feature-gate/exclude from `members` until landed. Add a test asserting every non-leaf, non-frontend crate is registered by bootstrap.

### R6. IPC-laundering via amplifier plugins (kernel-side enforcement incomplete)
- [x] ✅ Closed — `45982e5` (formal threat model + accepted residual risk documented)

Unlisted handlers default to `IpcCall`-only (`crates/nexus-plugins/src/loader.rs:2728-2737` returns empty = no extra caps). Amplifier plugins (agent, workflow) holding `IpcCall` can launder calls into high-impact handlers (`terminal::create_session`, `mcp::connect`) the matrix doesn't gate further. Mitigated by scoping agent/workflow contexts to minimal cap sets (`crates/nexus-bootstrap/src/lib.rs:573-688`) and audit-tagged `tracing::warn!`, but kernel-side enforcement of the transitive surface is not complete. Honestly documented at `docs/0.1.2/capabilities.md:111`.

**Fix:** Adopt a kernel-side default-deny posture for high-impact handlers (require explicit cap-matrix opt-in rather than `IpcCall`-only default), or formally accept the risk with the mitigation set documented.

### R7. Generated-artifact asymmetry: 335 ts-rs bindings vs 257 schemars schemas
- [x] ✅ Closed — typed-args migration series `b53c95b`…`76b4a6d` (storage, git, lsp, dap, acp, security)

Hand-rolled `serde_json::Value` handlers (`nexus-storage::read_file`, `nexus-git`, `nexus-mcp`, `nexus-lsp`, `nexus-dap`) read fields by hand and emit ad-hoc `json!` responses instead of typed `#[serde(deny_unknown_fields)]` structs, so they're invisible to both the `ipc_strictness` gate and the JSON-schema generator (`crates/nexus-bootstrap/tests/ipc_strictness.rs:18-23`). MCP/external consumers get an incomplete schema picture and unknown-field regressions on these handlers go uncaught.

**Fix:** Migrate the remaining hand-rolled handlers to typed `deny_unknown_fields` structs.

### R8. Oversized modules hurt maintainability
- [x] ✅ Closed — split series `b809e92` (terminal), `84eab76` (SettingsPanelView), `2f12ec1` (storage tests), `430bcdc` (cli args), `df7e551` (editor constants)

Largest offenders: `crates/nexus-terminal/src/core_plugin.rs` (3,541), `crates/nexus-plugins/src/loader.rs` (3,437), `shell/src/plugins/core/settings/SettingsPanelView.tsx` (3,378), `crates/nexus-storage/src/lib.rs` (2,987), `crates/nexus-plugins/src/manifest.rs` (2,549), `crates/nexus-cli/src/main.rs` (2,460), `shell/src/plugins/nexus/editor/index.ts` (2,509). The 23 `#[allow(clippy::too_many_lines)]` sites are the worklist.

**Fix:** Split IPC command handlers into per-command-group submodules; decompose the large React view. Start with `nexus-terminal/core_plugin.rs` and `SettingsPanelView.tsx`.

### R9. Systems lifecycle / scaling refinements
- [x] ✅ Closed — `ee58616` (Tantivy reader reuse) + `1a8af3b` (run-store eviction, shutdown cancellation)

- Tantivy `IndexReader` is rebuilt per query rather than reused (`crates/nexus-storage/src/search.rs:165-170`).
- ai-runtime run-store `inner` map is never evicted — slow memory leak over long sessions (`crates/nexus-ai-runtime/src/scheduler.rs:9-12`).
- `on_stop` doesn't drain in-flight AI tasks before returning (`crates/nexus-ai-runtime/src/core_plugin.rs:291`).

**Fix:** Cache/reload the Tantivy reader; bound the run-store with eviction; drain or cancel in-flight tasks on shutdown.

### R10. Host→plugin layering inversion in the shell
- [x] ✅ Closed — `ee58616` (fail-loud missing services) + `9793d0f` (EditorHostSurface inversion seam)

The shell host statically imports the editor plugin (`fencedCodeRegistry`, `useEditorStore`) at `shell/src/host/PluginAPI.ts:13,20`, so `api.editor` breaks if the editor plugin is absent. `api.fs` / `configuration` / `notifications` silently no-op when their backing service is missing (`shell/src/host/PluginAPI.ts:255-340`) rather than surfacing a clear error.

**Fix:** Invert the dependency (editor registers its surface with the host) and make missing-service calls fail loudly.

---

## Low

### R11. Doc drift: stale crate and command counts
- [x] ✅ Closed — `54258f0` (README/CLAUDE.md/docs counts). CONTRIBUTING.md was missed; fixed by the 2026-06-10 review (V3).

Workspace has 38 members (`Cargo.toml`) but docs say 35: `README.md:9`, `CLAUDE.md`, `docs/0.1.2/architecture.md:7`, `docs/0.1.2/crates.md:5,49`. Shell registers 30 Tauri commands but docs say 29: `CLAUDE.md`, `docs/0.1.2/shell.md`. Add the three new crates (`nexus-memory`, `nexus-context`, `nexus-protocol`) to the inventory tables.

### R12. Sandbox-incompatible `window.prompt`/`confirm` still present (4 sites)
- [x] ✅ Closed (`42bbd0c`) — routed through the host dialog surface.

### R13. Sandbox watchdog can false-crash healthy plugins
- [x] ✅ Closed (`c87e2fc`) — sandbox watchdog auto-pong added.

### R14. Accessibility baseline missing on ~half of views
- [x] ✅ Closed (`963d941`) — a11y baseline + guard established.

### R15. ~234 production `.unwrap()` calls
- [x] ✅ Closed — `3747e03` (hot-path expects), `de9de82` (workspace cleanup 17 → 0), locked by `da41404` (`clippy::unwrap_used = deny` on production targets).

### R16. `panic=abort` + lock-poison `.expect()` escalates one subsystem panic to full crash
- [ ] **Open** — review poison-`.expect()` sites so a single plugin panic can't abort the whole process.

### R17. Sync-handler timeout is advisory only
- [x] ✅ Closed (`42bbd0c`) — limitation documented.

### R18. `dep_invariants.rs` is a denylist, not an allowlist
- [x] ✅ Closed (`42bbd0c`) — allowlist variant added (`ipc_proxies_only_link_allowed_in_tree_crates`).

### R19. Stray `console.*` calls and module-wide `#![allow(dead_code)]`
- [x] ✅ Closed — `54258f0` (`no-console` eslint rule) + `963d941` (dead_code allows narrowed).

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
