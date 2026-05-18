# Architecture Adherence Audit

> **As of:** 2026-05-17. Audit of whether the code lives up to the architectural invariants documented in [`architecture.md`](architecture.md) (microkernel) and [`shell.md`](shell.md) (Tauri desktop shell). Verdicts grounded in `Cargo.toml` reads, the enforcement tests, and direct greps — not in archived docs.

## TL;DR

| Invariant | Status | One-liner |
|-----------|:------:|-----------|
| **File-as-truth** | ✅ | `.forge/index.db` writes are gated to `nexus-storage`. Watcher confirmed. |
| **Microkernel isolation** | ✅ | `nexus-kernel` only links `nexus-types` + `nexus-plugin-api`. Subsystems don't impersonate each other. |
| **IPC over direct calls** | ⚠️ | MCP/ACP/Remote fully routed. **CLI has 3 intentional direct uses** (`nexus-terminal`, `nexus-collab`, `nexus-security`) — documented in source, called out below. |
| **Capabilities gate everything** | ✅ | All ~280 IPC handlers classified in `cap_matrix.toml`. `cap_matrix_complete` test runs unconditionally. |
| **Single-target shell** | ✅ | Legacy `app/` + `crates/nexus-app/` retired; only benign references remain (docs, tests that enforce non-existence, fixture content). |
| **Thin Tauri bridge** | ⚠️ | 29 commands (up from 25 documented earlier — see correction §S-B). One borderline-feature command (`notify_desktop`) is a deliberate exception. |
| **Empty-by-default chrome** | ⚠️ | App.tsx mounts only slot/workspace containers, BUT imports a state hook from `plugins/nexus/workspace` directly — dependency-inversion smell. |
| **Iframe sandbox + pluginId boundary** | (not deeply verified) | Sandbox modules exist under `shell/src/host/sandbox/`; F-8.1.2 `assertValidPluginId` shipped per `docs/PRDs/IMPLEMENTATION_STATUS.md` archive. A focused security audit pass would be the next layer. |

---

## Microkernel adherence

Authoritative tests live in:
- `crates/nexus-bootstrap/tests/dep_invariants.rs` — FORBIDDEN dep-pair list
- `crates/nexus-bootstrap/tests/cap_matrix_complete.rs` — every handler must be classified
- `crates/nexus-bootstrap/tests/ipc_topic_prefix_invariant.rs` — every kernel-bus publish must use the plugin's own namespace
- `crates/nexus-plugin-api/tests/kernel_free_guard.rs` — `nexus-plugin-api` must not link kernel internals

All four tests run **unconditionally** (no `#[ignore]`). Regressions caught at CI time.

### M-A. dep_invariants.rs coverage

Current FORBIDDEN list (`dep_invariants.rs:17-62`):

```
nexus-cli, nexus-tui, nexus-mcp, nexus-ai, nexus-database  → nexus-storage
nexus-cli, nexus-tui                                       → nexus-database, nexus-ai-runtime
nexus-mcp                                                  → nexus-ai, nexus-ai-runtime
nexus-acp                                                  → nexus-agent, nexus-ai, nexus-storage
nexus-remote                                               → nexus-agent, nexus-ai, nexus-storage
nexus-database                                             → rusqlite
nexus-kernel                                               → rusqlite, nexus-kv
```

Also self-tested at line 132 against synthesised cfg-conditional deps (issue #83) so `[target.'cfg(unix)'.dependencies]` foot-guns can't slip past.

**Gap candidates** — *defense in depth, not violations*:
- `nexus-mcp`, `nexus-acp`, `nexus-remote` are IPC consumers but only forbid a subset of subsystems. They could **also** be forbidden from `nexus-terminal`, `nexus-editor`, `nexus-git`, `nexus-database`. None of them link these today, but extending the forbidden list closes theoretical future bypass routes.
- No equivalent test for `shell/src-tauri/` because the shell crate sits outside the workspace (`exclude = ["shell"]`). The bridge's `Cargo.toml` is clean today (only `nexus-bootstrap` + `nexus-kernel` + `nexus-plugin-api` + `nexus-plugins` + `nexus-panic-log` + `nexus-remote`), but a parallel dep-invariants test for the shell crate would be the right next step.

### M-B. Cross-service dep graph (service-to-service)

| Crate | Depends on (nexus-*) | Verdict | Rationale |
|-------|----------------------|---------|-----------|
| `nexus-storage` | `nexus-database`, `nexus-formats` | ✅ | database is pure-logic (no SQL); formats is pure parse. Single-process usage. |
| `nexus-ai` | `nexus-security`, `nexus-ai-runtime` | ✅ | security for cred vault + TLS pinning; ai-runtime for `shared_pool_handle()` (no IPC). Commented at `nexus-ai/Cargo.toml:15-18`. |
| `nexus-editor` | `nexus-formats` | ✅ | formats is pure parse. |
| `nexus-git` | `nexus-security` | ✅ | credential vault for push/pull. |
| `nexus-crdt` | `nexus-editor` | ⚠️ smell | Both are domain-close (collab editing) so defensible, but a rationale comment in `Cargo.toml` would document intent. |
| `nexus-database`, `nexus-security`, `nexus-agent`, `nexus-collab`, `nexus-ai-runtime` | (only kernel / plugin-api / types / plugins) | ✅ | leaves of the service graph. |

No unjustified cross-service deps.

### M-C. IPC consumer adherence (frontends + protocol consumers)

| Crate | nexus-* deps | Direct subsystem use? | Verdict |
|-------|--------------|------------------------|---------|
| `nexus-tui` | bootstrap, kernel, types, git | none | ✅ — `nexus-git` is read-only library use |
| `nexus-mcp` | kernel, plugins | none | ✅ |
| `nexus-acp` | kernel, plugins | none | ✅ |
| `nexus-remote` | kernel, plugin-api | none | ✅ |
| `nexus-cli` | bootstrap, kernel, plugins, security, mcp, acp, remote, git, terminal, tui, templates, formats, collab, crdt | **3 documented exceptions** | ⚠️ — see §M-D |
| `shell/src-tauri` (nexus-shell) | bootstrap, kernel, plugin-api, plugins, panic-log, remote | none direct | ✅ |

### M-D. CLI direct-import exceptions

Three IPC bypasses in `nexus-cli` — each documented in source as a deliberate single-process CLI-scope use:

| File:Line | Imports | Rationale |
|-----------|---------|-----------|
| `crates/nexus-cli/src/commands/term.rs:36` | `nexus_terminal::{detect_default_shell, LineBuffer, Session, SessionConfig, ShellSpec}` | Phase G single-process verbs (`nexus term env\|run\|shell`); comment L1-29 documents Phase H will migrate to IPC |
| `crates/nexus-cli/src/commands/collab.rs:22` | `nexus_collab::{parse_ws_url, RelayServer, Token, …}` | Phase 1 CLI bootstrap for relay; comment L1-16 documents pre-daemon design |
| `crates/nexus-cli/src/commands/collab.rs:27` | `nexus_security::CredentialVault` | Direct keyring access for token CLI; low-level primitive |

**Verdict:** legitimate but should be documented in an ADR ("CLI-scope exceptions to IPC-only rule") so future maintainers know not to add more. Currently the rationale lives only in inline comments.

### M-E. cap_matrix coverage

`cap_matrix_complete.rs:30-59` walks every registered `(plugin, command)` and fails if a row is missing from `cap_matrix.toml`. Now runs **unconditionally** (Phase 2 closed 2026-05-15; `#[ignore]` removed).

- ~340 `[[handler]]` rows across ~20 plugin ids.
- Every row carries either `caps = [...]` or `unrestricted = "<rationale>"`.
- `historical_cap_requirements_survive_migration()` ensures the 17 original `add_cap_requirement` rows persist.
- 17 rows flagged with `# AUDIT:` comments — documented at [`reference/audit-flags.md`](reference/audit-flags.md). These are intentional candidates for cap elevation, not gaps.

### M-F. File-as-truth

Writes to `.forge/index.db` and `.forge/search/`:
- Production code: only `nexus-storage` (storage owns the watcher per ADR 0003).
- Test fixtures: `crates/nexus-formats/src/notion/export.rs:679` writes a mock index.db; `crates/nexus-cli/tests/cli-integration.rs:10` and `prd-05-smoke.rs:10` assert the file exists post-init. All scoped to tests.

No production code treats `index.db` as source-of-record. ✅

### M-G. Kernel + plugin-api leaf guarantee

`crates/nexus-kernel/Cargo.toml:9-20`:
```
nexus-plugin-api, nexus-types,
tokio, tracing, serde, serde_json, toml, uuid, chrono, thiserror, async-trait
```
No subsystem deps, no `rusqlite`, no `nexus-kv`. ✅

`crates/nexus-plugin-api/Cargo.toml:15-22` + `crates/nexus-plugin-api/tests/kernel_free_guard.rs:16-37` enforce that `nexus-plugin-api` cannot reference any kernel-internal symbol (37-name FORBIDDEN list, source-level scan).

`crates/nexus-types` — pure type definitions; no nexus-* deps required.

All three are confirmed leaves.

### M-H. Topic-prefix invariant

`ipc_topic_prefix_invariant.rs:85-186` walks every `.publish(...)` call in core plugins + bootstrap and verifies the topic is either:
- The plugin's own namespace (`OWNERS` map at lines 45-66, derived from each plugin's `PLUGIN_ID` constant), or
- In `KERNEL_OWNED_SHARED_TOPICS` (line 80-83 — currently just `com.nexus.activity.appended`), or
- For bootstrap-side publishes: in `BOOTSTRAP_ALLOWED_PREFIXES` (line 74 — currently just `com.nexus.dream_cycle.*`).

A self-check (`topic_namespace_helper_matches_kernel_semantics()` at line 367-372) verifies the helper logic matches the kernel's actual prefix-matching.

Dynamic topics (computed at runtime) are reported separately but don't fail the test — these need eyes during review. No CI-time gaps.

---

## Shell adherence

Tests:
- `crates/nexus-bootstrap/tests/plugin_contract_purity.rs` — verifies the bootstrap doesn't accidentally re-introduce legacy `nexus-app` contracts
- `shell/tests/plugin-import-hygiene.test.ts` — verifies shell plugins don't import host internals (mentions `notify_desktop` as one of the few approved bridge commands)
- `shell/src-tauri/tests/tauri_command_boundary.rs` — referenced from the existing audit; tests the Tauri command boundary

### S-A. Single-target invariant

Legacy `crates/nexus-app/` + `app/` references in the active tree:

| File | What |
|------|------|
| `CLAUDE.md`, `CONTRIBUTING.md`, `README.md` | Reference the retirement (intended) |
| `crates/nexus-bootstrap/tests/plugin_contract_purity.rs` | Test that enforces the retirement |
| `scripts/migrate-shell-state.ts` | One-time migration helper |
| `fixtures/forge/areas/Editor Shell Architecture.md` | Fixture content (markdown body) |
| `.design-bundle/project/forge_doc.jsx` | Design bundle artifact |

**Total: 7 hits, all benign.** ✅ No production code references the legacy shell.

### S-B. Thin Tauri bridge — **count corrected**

**Actual command count: 29** (the prior `shell.md` / `CLAUDE.md` claimed 25 — that was stale).

Registered via `tauri::generate_handler![…]` at `shell/src-tauri/src/lib.rs:712-742`:

| Group | Count | Commands |
|-------|------:|----------|
| Kernel + bridge | **10** | `bridge::init_forge`, `boot_kernel`, `boot_remote`, `shutdown_kernel`, `revoke_plugin_capability`, `kernel_invoke`, `kernel_subscribe`, `kernel_unsubscribe`, `kernel_is_booted`, `kernel_connection_state` |
| Plugin management | **5** | `scan_plugin_directory`, `scan_plugin_directory_at`, `set_plugin_enabled`, `get_plugin_granted_capabilities`, `set_plugin_granted_capabilities` |
| Persistence | **6** | `persistence::get_shell_state`, `save_shell_state`, `write_last_forge_path`, `forget_forge_path`, `write_remote_recent`, `forget_remote_recent` |
| Utility | **3** | `path_exists`, `append_shell_log`, `notify_desktop` |
| Popout | **5** | `windows::popout_window`, `close_popout_window`, `list_popout_windows`, `get_popout_window_bounds`, `set_popout_window_bounds` |
| **Total** | **29** | |

#### Commands added since the prior "25" snapshot

- `boot_remote` — required to attach the kernel to a remote forge over SSH (BL-140 Phase 3). Same orchestrator pattern as `boot_kernel`. ✅ Shell-intrinsic.
- `kernel_connection_state` — observability for the remote-reconnect wrapper (BL-146). ✅ Shell-intrinsic.
- `revoke_plugin_capability` — paired with `set_plugin_granted_capabilities`; lets the user revoke a granted cap at runtime. ✅ Plugin-mgmt.
- `notify_desktop` — see §S-C below.
- `persistence::write_remote_recent` / `forget_remote_recent` — extend `recent_forge_paths` to remote URIs. ✅ Persistence.

### S-C. The `notify_desktop` edge case

`shell/src-tauri/src/lib.rs:557-583`. The doc-comment at line 550-556 says:

> Called by the shell-side `nexus.notifications` plugin alongside the in-app toast so the user still sees alerts when the Nexus window is not focused.

This is borderline because the backend `com.nexus.notifications` plugin already exists. The architectural question: why not route OS notifications through that backend handler?

**Answer (verified by reading the file):** The Tauri notification plugin (`tauri_plugin_notification`) is **only reachable from the bridge** — it's a host-platform capability. The backend plugin has no access to the Tauri runtime. So the data flow is:

```
backend nexus-notifications  ──ipc──>  shell-side nexus.notifications plugin
                                                      │
                                                      └── invoke ──> bridge::notify_desktop
                                                                              │
                                                                              └── tauri_plugin_notification
```

**Verdict:** legitimate, but the architecture should formally name this **"host platform capability primitive"** pattern — bridge commands that wrap a Tauri-only capability that no other path can reach. Other examples: the popout commands (wrap Tauri's `WebviewWindow` API), `path_exists` (wrap Tauri's filesystem allowlist), `append_shell_log` (wrap the renderer's panic log path). The rule of thumb: *if the underlying capability requires a `tauri::AppHandle`, a bridge command is the right venue.*

Suggested remediation: add a `## Host-platform primitives` section to [`shell.md`](shell.md) that enumerates the legitimate bridge commands by category, so future authors understand why these are allowed.

### S-D. Shell-side dep cleanliness

`shell/src-tauri/Cargo.toml` direct nexus-* deps (lines 51-57):
- `nexus-kernel`, `nexus-bootstrap`, `nexus-plugin-api`, `nexus-plugins`, `nexus-panic-log`, `nexus-remote`

No subsystem crates. ✅

External tauri-plugin-* deps are limited to: `fs`, `dialog`, `deep-link`, `window-state`, `global-shortcut`, `notification`. Each is a documented Tauri capability the bridge wraps.

### S-E. Empty-by-default chrome — **violation flagged**

`shell/src/shell/App.tsx:8`:

```typescript
import { useWorkspaceStore as useNexusWorkspaceStore } from '../plugins/nexus/workspace/workspaceStore'
```

The root `App` component reaches **into a specific plugin's store** to read `rootPath`. This couples the shell chrome to the `nexus.workspace` plugin's existence — if that plugin were uninstalled, the shell wouldn't compile.

**Verdict:** small dependency-inversion violation of the "shell starts empty" principle. The fix: expose `rootPath` via a shell-owned slot/store that `nexus.workspace` *publishes to*, rather than the shell *importing from* the plugin.

Everything else in App.tsx is fine — it mounts `<Workspace>` (leaf-tree container), `<SlotSurface>` (overlay/status/activity slot containers), `<ErrorBoundary>`. Real UI content comes from contributions registered via `ExtensionHost`.

### S-F. ExtensionHost coverage

`shell/src/host/ExtensionHost.ts` (audited via file listing in `shell/src/host/`): the host orchestrates load + activate + contribution registration for every plugin under `shell/src/plugins/{core,nexus,community}/`. Companion modules in `shell/src/host/`:

- `PluginAPI.ts` — boundary the plugins call (with the `defineSlot is not yet implemented` warning noted in [`reference/todos.md`](reference/todos.md))
- `PluginRegistry.ts` — registered plugin metadata
- `pluginActivation.ts` — activation event router (matches `manifest.activation`)
- `sandbox/` — iframe sandbox for community/script plugins
- `communityPluginLoader.ts` — discovers `~/.nexus-shell/plugins/<id>/`
- `ContextKeyService.ts` — context keys used in `when` clauses for commands/keybindings
- `EventBus.ts` — shell-side event fan-out
- `ActivationTriggers.ts` — trigger evaluation
- `clientLogger.ts`, `layoutSnapshot.ts`, `bodyClasses.ts` — supporting infrastructure

Slot contributions flow through `shell/src/registry/SlotRegistry.ts` (the `useSlotStore` consumed by `App.tsx`). ✅ Every visible chrome element is slot-mounted, not hardcoded.

### S-G. Drift-checked IPC types

`scripts/check_ipc_drift.sh` regenerates and diff-checks:
- `packages/nexus-extension-api/src/generated/ipc/*.ts` (via ts-rs)
- `crates/nexus-bootstrap/schemas/ipc/*.json` (via schemars)
- `docs/generated/capabilities.md`

Per crate participation (the script lists every `cargo test … --features ts-export` invocation): `nexus-plugin-api`, `nexus-storage`, `nexus-ai`, `nexus-linkpreview`, `nexus-git`, `nexus-mcp`, `nexus-lsp`, `nexus-dap`, `nexus-acp`, `nexus-agent`, `nexus-comments`, `nexus-theme`, `nexus-skills`, `nexus-workflow`, `nexus-terminal`, `nexus-database`, `nexus-templates`, `nexus-formats`, `nexus-audio`, `nexus-notifications`, `nexus-ai-runtime` = **21 crates**.

Crates with IPC types that are NOT in the drift check:
- `nexus-security`, `nexus-collab` — both have IPC handlers (cap_matrix has rows for them) and emit boundary types. **Gap candidate** — extending the drift check to these is mechanical.

### S-H. Iframe sandbox + pluginId boundary-binding

The presence of `shell/src/host/sandbox/` confirms the iframe sandbox is wired. The `F-8.1.2` pluginId-boundary-binding fix shipped per the archived `IMPLEMENTATION_STATUS.md` — `assertValidPluginId` rejects empty / colon-bearing ids; the orchestrator binds id at handshake.

This audit pass **did not exhaustively re-verify** the sandbox call paths or audit every pluginId entry point. A focused security audit is the right next layer if you want defence-in-depth assurance. The architecture is correct; the implementation surface is large enough that I'd want a dedicated red-team-style pass before signing off.

### S-I. Legacy command residue

Phase 4 WI-37 removed 9 legacy terminal-related `#[tauri::command]` handlers. Grep for them in the active tree returns zero hits ✅. The remaining 29 bridge commands fit cleanly in the 5 categories above.

---

## Recommended remediation (none are bugs; all are hardening)

| ID | Item | Effort | Owner |
|----|------|:------:|-------|
| **AA-01** | Extend `dep_invariants.rs::FORBIDDEN` to forbid `nexus-mcp`/`acp`/`remote` from `nexus-terminal`/`editor`/`git`/`database` (currently they don't link them, but the test should encode the intent). | S | bootstrap maintainer |
| **AA-02** | Add a parallel `dep_invariants` test for `shell/src-tauri/Cargo.toml` (today the shell crate sits outside `[workspace]` so the existing test can't see it). | M | shell maintainer |
| ~~**AA-03**~~ | ~~Open an ADR titled *"CLI-scope exceptions to IPC-only"* documenting the three `nexus-cli` direct uses (`nexus-terminal`, `nexus-collab`, `nexus-security`) with the Phase-H migration plan.~~ ✅ **Resolved 2026-05-17** — see [ADR 0031](../archive/pre-0.1.2/adr/0031-cli-scope-exceptions-to-ipc-only.md). | S | architecture |
| **AA-04** | Fix `shell/src/shell/App.tsx:8` — invert the dependency on `plugins/nexus/workspace/workspaceStore`. Either expose `rootPath` via a shell-owned slot, or have the plugin push the value into a slot-mounted context the shell consumes. | M | shell maintainer |
| **AA-05** | Add `## Host-platform primitives` subsection to [`shell.md`](shell.md) formally naming the `notify_desktop` / `popout_*` / `path_exists` / `append_shell_log` pattern (bridge wraps a Tauri-only capability). | XS | docs |
| ~~**AA-06**~~ | ~~Add a rationale comment to `crates/nexus-crdt/Cargo.toml` for the `nexus-editor` dep (currently the only undocumented cross-service dep).~~ ✅ **Resolved 2026-05-17** — comment added in `crates/nexus-crdt/Cargo.toml`. | XS | crdt maintainer |
| **AA-07** | Extend `scripts/check_ipc_drift.sh` to cover `nexus-security` and `nexus-collab` (both have IPC types not yet ts-exported). | S | bootstrap maintainer |
| **AA-08** | Run a dedicated **iframe-sandbox security audit** — this adherence pass confirmed the wiring exists but didn't red-team the escape paths. | L | security |
| **AA-09** | Patch [`shell.md`](shell.md) + `CLAUDE.md` to reflect 29 (not 25) Tauri commands and the new groupings (kernel grew from 7 → 10). | XS | docs *(done in same change as this audit)* |

## What this audit did **not** cover

- **Runtime fuel/timeout invariants for WASM plugins** — the per-call fuel + epoch deadline are configured in `WasmConfig::default()`; whether they're actually enforced at every call site under all paths (e.g. async handlers, nested ipc_calls) wasn't verified end-to-end.
- **Iframe sandbox escape paths** — see AA-08.
- **WASM host-function safety** — the `host_fns.rs` surface returns `-1001` / `-1002` codes per [`plugins/community.md`](plugins/community.md), but I didn't verify the host-side error paths can't be exploited.
- **Audit log integrity** — the AUDIT-flagged `clear_audit_log` handler could erase tracks; promoting it to a `security.audit.write` cap is the remediation, but I didn't verify the SQLite schema doesn't have a separate truncate path.

Each of these warrants its own focused pass.

---

## Summary

The architecture is **strongly adhered to**. The four microkernel invariants are all enforced by unconditional tests; the shell architecture is empty-by-default with one minor dependency-inversion smell. The thin-bridge rule has held — 29 commands total, all classifiable as kernel / plugin-mgmt / persistence / utility / popout primitives, with `notify_desktop` being the borderline case that's defensible as a host-platform-primitive wrapper.

The remediation list above is hardening, not bug-fixing. None of the items would block a 0.1.2 release; all would close theoretical future regression paths.
