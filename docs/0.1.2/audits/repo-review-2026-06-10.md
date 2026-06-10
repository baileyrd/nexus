# Nexus — Repository Review

> **As of:** 2026-06-10. Scope: full workspace (38 crates, shell, packages, scripts, CI, docs), reviewed against the four microkernel invariants and as a follow-up to [`expert-review-2026-05-31.md`](expert-review-2026-05-31.md).
>
> **Status legend:** open items are unchecked `[ ]`. Mark closed items `[x] ✅ Closed` with the commit SHA.
>
> **Headline:** the post-R1 sprint worked. CI now gates tests/clippy/fmt/pnpm on every PR, `cargo clippy --workspace --all-targets` is clean, `clippy::unwrap_used = deny` is locked on production targets, and 15 of the 19 expert-review findings closed within two days. The architecture invariants hold (re-verified this pass: kernel links only the two leaf crates; frontends route through `ipc_call`; capability checks at dispatch are unconditional and audited; file-as-truth ordering in `write_file` is correct). This review's findings are **operational debt around the edges** — tracker/doc sync, scripts, network-timeout hygiene, and unwired test infrastructure — not structural problems.

## Severity counts

| Severity | Open |
|----------|------|
| High | 2 |
| Medium | 7 |
| Low | 7 |

---

## Tracker / doc synchronization (do these first — 30 minutes total)

### V1. The 2026-05-31 audit doc was never updated as its issues closed
- [ ] **Open** · Low effort

`expert-review-2026-05-31.md` still shows all 19 items `[ ]` open, while 15 of the corresponding issues (#184–#202) are closed on GitHub. Its own status legend mandates `[x] ✅ Closed` + commit SHA. Anyone reading the doc tree concludes CI is still missing (R1) and bindings are still drifted (R2) — both false.

**Fix:** Mark R1, R2, R6–R15, R17–R19 closed with their landing SHAs (e.g. R1 → `5f46689`, R15 → `de9de82`/`da41404`). Leave R3/R4/R5/R16 open, with R3 re-scoped per V2.

### V2. Issue #186 (R3) is ~80% fixed in-tree but the issue text still describes the original full gap
- [ ] **Open** · Low effort

`host::invoke_command` now enforces per-handler caps via `required_caller_caps_for_args` and rejects `internal = true` handlers for sandboxed callers (`crates/nexus-plugins/src/host_fns.rs:651-690`), and distinct `HOST_ERR_*` codes per `IpcErrorKind` landed in `59d2fc9` (`host_fns.rs:36-89`). The only remaining leg is that dispatch is still the sync `dispatch()` (`host_fns.rs:702`) — no async path, no timeout, no cancellation for WASM-originated IPC.

**Fix:** Re-scope #186 to "async dispatch + timeout/cancellation for the WASM bridge" so the closed capability-parity work is visible, or close it and open a narrower issue.

### V3. CONTRIBUTING.md has drifted badly from the code
- [ ] **Open** · Low effort

`CONTRIBUTING.md:41-43` says the bridge registers **22** commands at `lib.rs:443-466` grouped "7 kernel, 5 plugin-management, 4 persistence, 1 utility, 5 popout"; actual is **29** at `shell/src-tauri/src/lib.rs:735-765` grouped 10/5/6/3/5. `CONTRIBUTING.md:59` says the workspace is **24** crates; actual is **38**. This is the first document a new contributor reads. (#194 fixed these counts in CLAUDE.md/docs but missed CONTRIBUTING.md.)

**Fix:** Update the counts and line references; better, replace hard numbers with links to [`../shell.md`](../shell.md) and [`../crates.md`](../crates.md) so there is one canonical count.

---

## High

### V4. Outbound HTTP clients have no timeouts (AI providers, notifications)
- [ ] **Open** · Low effort

- `nexus_security::tls::build_pinned_client` (`crates/nexus-security/src/tls.rs`) — used by the Anthropic and OpenAI providers via `crates/nexus-ai/src/http_client.rs:12` — sets **no** `.timeout()` / `.connect_timeout()`.
- Ollama uses a bare `reqwest::Client::new()` (`crates/nexus-ai/src/ollama.rs:74`).
- Notifications uses bare `reqwest::blocking::Client::new()` (`crates/nexus-notifications/src/lib.rs:288,402`).
- Only linkpreview configures one (`crates/nexus-linkpreview/src/lib.rs:239-240`, `FETCH_TIMEOUT = 5s`).

A hung provider endpoint stalls an AI handler until the OS TCP timeout (minutes). The kernel IPC deadline mitigates caller-side, but for sync paths the timeout is advisory (R17), and the blocking notification clients have no backstop at all.

**Fix:** Set `.connect_timeout(~10s)` in `build_pinned_client` and per-request timeouts at call sites (overall `.timeout()` is wrong for streaming completions — use connect + idle-read semantics there). Give ollama/notifications the same treatment.

### V5. Built test infrastructure that never runs: fuzz targets and shell E2E
- [ ] **Open** · Medium effort

- `crates/nexus-fuzz` exists with 3 fuzz targets (BL-103) — not wired to any workflow.
- `shell/e2e/` has a full WebdriverIO setup (`wdio.conf.ts`, specs) — not run in CI.
- `shell/src-tauri/tests/` contains a single stub, and `docs/0.1.2/architecture-adherence.md:136` cites a `tauri_command_boundary.rs` test that does not exist.

This is R1's lesson in miniature: authored-but-unenforced QA decays silently.

**Fix:** Add a scheduled (or PR-labeled) CI job running each fuzz target for a bounded time; add a smoke-level E2E job (headless WebKit/X11); either write `tauri_command_boundary.rs` (assert the 29 registered commands + popout/leaf-ID injection guards) or remove the doc claim.

---

## Medium

### V6. No supply-chain gate in CI
- [ ] **Open** · Low effort

70+ external deps (`wasmtime`, `reqwest`, `rusqlite`, `tauri`, …) with no `cargo deny` / `cargo audit` job, and no Dependabot/Renovate config. For a project whose core claim is sandboxing untrusted plugins, an unpatched advisory in wasmtime is a headline risk.

**Fix:** Commit a `deny.toml` (advisories + licenses + duplicate bans) and add a `cargo deny check` job to `ci.yml`.

### V7. All 29 `scripts/*.sh` are single-machine artifacts
- [ ] **Open** · Low effort

Every script hard-codes `/mnt/c/Users/baile/dev/Nexus` or `/home/baileyrd/.cargo/bin` (e.g. `scripts/check_all.sh:3`, `scripts/bench_build.sh:2`); only 3 of 29 set `set -euo pipefail`; shebangs are inconsistent. CLAUDE.md already tells people not to use most of them.

**Fix:** Delete the thin `test_*.sh`/`check_*.sh` cargo wrappers outright (CI is now the reproducible runner), and parameterize the few value-add scripts (`check_ipc_drift.sh` is already the model: root-agnostic + strict mode) via `REPO_ROOT="$(git rev-parse --show-toplevel)"`.

### V8. Knowledge-graph lock-poison panics in storage public API (open #199 made concrete)
- [ ] **Open** · Medium effort

Seven public `StorageEngine` graph methods are documented "Panics if the internal graph RwLock is poisoned" (`crates/nexus-storage/src/lib.rs:1509,1524,1542,1556,1570,1584,1598`). With `panic = "abort"` in the release profile (`Cargo.toml:242`), one poisoned lock aborts the whole desktop app. `context_impl.rs:145-174` already implements the tier-1 recover-and-log pattern the architecture policy (`docs/0.1.2/architecture.md:144-151`) prescribes for read paths.

**Fix:** Apply the same `PoisonError::into_inner()` + `tracing::error!` recovery to the graph read paths; these are the highest-value sites for closing #199.

### V9. Extension-API contract divergence (#187) is the main remaining plugin-author risk
- [ ] **Open** · High effort

`packages/nexus-extension-api/src/index.ts:1-32` now honestly documents that three plugin-context shapes coexist (`NexusPluginContext` vs sandbox `plugin.ts:13` vs shell `types/plugin.ts:177`) — good interim state from `3b66d4d` — but the package still ships `1.0.0` with `publishConfig.access: public` and has zero tests, so nothing prevents further drift.

**Fix:** Pick the canonical shape, add a type-level conformance test (`Satisfies`-style assertion that the sandbox runtime implements the exported contract), and re-version as `0.x` until it's true.

### V10. Storage write path holds a sync `Mutex<rusqlite::Connection>`
- [ ] **Open** · Medium effort · benchmark first

`crates/nexus-storage/src/lib.rs:144` guards the write connection with `std::sync::Mutex`. Writes run on `spawn_blocking`, so this is correct but serializes all writers and can stall on slow filesystems (NFS/encrypted home dirs). Not a bug; a scaling ceiling.

**Fix:** Benchmark under concurrent write load; if it matters, move to a dedicated writer thread + channel (the SQLite-idiomatic shape) rather than an async mutex.

### V11. `context_impl.rs` is 27% of the kernel
- [ ] **Open** · Medium effort

1,110 lines (`crates/nexus-kernel/src/context_impl.rs`): context struct + the dense `ipc_call_inner` dispatch (timeout/cancel/panic-mapping) + tests in one file. It's the best-engineered code in the repo and the hardest to read.

**Fix:** Extract dispatch into `dispatch.rs` and move the test module out, mirroring the #191 splits done elsewhere.

### V12. Audit log is readable by any IPC-capable plugin
- [ ] **Open** · Low effort

`query_audit_log` is effectively unrestricted in the cap matrix, so any plugin holding `ipc.call` can read the full audit trail — including other plugins' denial events and credential-access records (names, not values). Information disclosure, not escalation.

**Fix:** Gate behind a new `audit.read` capability or restrict to `TrustLevel::Core`, mirroring the `resolve_credentials` internal-only treatment.

---

## Low

### V13. Linkpreview DNS-rebinding TOCTOU (documented residual)
- [ ] **Open** — `crates/nexus-linkpreview/src/lib.rs:166-177` validates resolved IPs, then reqwest re-resolves at connect time. The SSRF guard is otherwise exemplary (per-redirect validation, metadata-IP blocks, body cap). Pin the validated resolution into the client (`resolve()` override) to close the window.

### V14. TLS pinning defaults off
- [ ] **Open** — `KernelConfig::tls_pinning_enabled` defaults `false`; consider auto-enabling for known AI provider endpoints when credentials are present, or a startup warning.

### V15. Test-density cold spots
- [ ] **Open** — `nexus-linkpreview` (1 test; the OG/Twitter-card HTML parsing is untested), `nexus-panic-log` (1 test), and `nexus-git`/`nexus-lsp`/`nexus-dap` sit well below workspace median. Linkpreview parsing is the highest-value target (untrusted HTML input).

### V16. Shell chrome imports a plugin store directly
- [ ] **Open** — `shell/src/App.tsx` imports `useWorkspaceStore` from `plugins/nexus/workspace`, the residual empty-shell violation already flagged as §S-A in [`../architecture-adherence.md`](../architecture-adherence.md). Same inversion-seam treatment as #193's `EditorHostSurface` would close it.

### V17. ~32 `as any` casts in shell test files
- [ ] **Open** — concentrated in `marginSuggest`/`marginSuggestTrigger` tests; replace with typed mock helpers so the strict-mode signal stays clean.

### V18. No release process documentation
- [ ] **Open** — no CHANGELOG.md, no RELEASE.md, Windows-only release builds (`release-windows.yml`), no checksums/signing. Fine for pre-0.2, worth writing down before external users arrive.

### V19. Staging-crate wiring decision (#188) still pending
- [ ] **Open** — re-verified: `nexus-memory`/`nexus-context`/`nexus-protocol` are healthy, fully-tested libraries (not rot), now honestly framed as staging libraries (`2a65225`) with `bootstrap_coverage.rs` guarding consumer presence. The remaining work is simply the Phase-2 integration decision; keep #188 as the tracking issue.

---

## What's strong (preserve)

- **Invariant enforcement is now belt-and-braces:** `dep_invariants.rs` (denylist + allowlist + cfg-dep self-test), `cap_matrix_complete.rs`, `bootstrap_coverage.rs`, `ipc_strictness`, and the drift gate all run on every PR via `ci.yml`.
- **The capability dispatch path** (`context_impl.rs:186-221`) checks `ipc.call` → per-handler caps → internal-only trust gate, audits every denial, and recovers from lock poison on the hot read path.
- **Sandbox quality:** WASM gets fuel + epoch + memory limits with no WASI preopens and Ed25519 manifest signatures; the JS sandbox is a null-origin iframe with source-identity checks; SSRF blocking in linkpreview is comprehensive and test-backed (`tests/issue_78_ssrf.rs`).
- **Secrets:** OS-keyring backed, per-plugin namespaced, never logged, internal-only resolution handler.
- **Responsiveness to review:** 15 of 19 expert-review findings closed in ~48h, with the fixes locked in by lints (`unwrap_used = deny`) and tests rather than one-off patches.
- **Near-zero embedded debt:** clippy clean workspace-wide, 1 stray TODO (a docstring false positive), SQL fully parameterized, deterministic and documented bootstrap order.

## Recommended sequence

1. **V1–V3** — tracker/doc sync (one sitting; restores trust in the doc tree).
2. **V4** — HTTP timeouts (small change, removes the only real hang risk).
3. **V6** — `cargo deny` in CI (the missing piece of the R1 fix).
4. **V5** — wire fuzz + E2E + the bridge boundary test.
5. **V8** — graph poison recovery (closes #199 where it matters).
6. Then the backlog: V7 scripts purge, V9 contract unification (#187), V11 dispatch split, V12 audit-read gating, and the Lows.
