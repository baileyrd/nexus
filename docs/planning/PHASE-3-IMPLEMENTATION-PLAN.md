# Phase 3 Implementation Plan — Security Hardening

**Status:** Plan only (no code changes yet)
**Date:** 2026-04-23
**Author:** Claude (audit + planning run)
**Phase:** 3 of 6 in the shell-migration roadmap (per [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) §5 and [ADR 0011](./adr/0011-adopt-plugin-first-shell.md))
**Prerequisite:** [Phase 1](./PHASE-1-IMPLEMENTATION-PLAN.md) complete (shipped to main); [Phase 2](./PHASE-2-IMPLEMENTATION-PLAN.md) complete (shipped to main).
**Source outline:** [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) §5 "Phase 3 — Security Hardening (3–4 weeks)".
**Backing audits:** [`MICROKERNEL-AUDIT.md`](./MICROKERNEL-AUDIT.md) findings F-5.1.1, F-5.3.1, F-5.3.2, F-2.1.1, F-9.2.1 / [`UI-AUDIT.md`](./UI-AUDIT.md) findings F-8.1.1, F-9.1.1.

---

## 1. Executive summary

Phase 3 moves the shell from "trust model = first-party only" to "trust model = ready-for-community-tier marketplace." Six work items (WI-30 through WI-35) covering iframe sandbox, install-time consent UI, TOCTOU write safety, api_version enforcement surfacing, plugin-contract crate guardrails, and per-plugin crash quarantine.

**Readiness corrections from the audit — three of the six have existing scaffolding that significantly reduces scope:**

| WI | INTEGRATION-REVIEW estimate | Audit finding | Audit-corrected |
|---|---|---|---|
| **WI-30** JS sandbox + CSP | "Substantial but critical" / L | **Greenfield.** No iframe or postMessage plumbing exists. `shell/src-tauri/tauri.conf.json:26` sets `"csp": null`. All plugins (core AND community) execute as ES modules in the main WebView with full `@tauri-apps/*` access today. Risk of breaking 32 first-party plugins if applied uniformly. | **XL** — by far the largest WI. Recommend scoping to community-tier only in Phase 3; first-party in Phase 4. |
| **WI-31** Install-time capability prompt | M (per INTEGRATION-REVIEW) | **Half-shipped.** Kernel-side HIGH-risk capability consent is already in `crates/nexus-plugins/src/loader.rs:1587-1651` with `granted_caps.json` persistence per plugin version. Phase 2 WI-18 shipped the display chips. **What is missing is the shell UI modal that calls a bridge command to write the grants file, plus the flow of `capabilities`/`apiVersion` through the Rust scanner → shell.** | **M** (~1 week), was S-M in the review. |
| **WI-32** TOCTOU fixes | S | **Validator already exists** at `crates/nexus-security/src/path.rs:114-158` — `ForgePathValidator::validate_for_write` is specifically designed to close the symlink-swap race, with the Unix symlinked-parent test on line 366. Neither `host_fns.rs::write_file` nor `context_impl.rs::write_file` call it; both still use the canonicalize-parent pattern (annotated "TOCTOU-safe" in comments, but the review's position is that the canonical approach is the `validator_for_write` form). Fix is **wire-up, not design**. | **S** (~3 days) with solid regression tests. |
| **WI-33** `api_version` range check | S | **Already shipped.** `check_api_version` in `crates/nexus-plugins/src/loader.rs:1534-1545` compares against `PLUGIN_API_VERSION_MAJOR` and rejects mismatches via `PluginError::IncompatibleApiVersion` (tested at `loader.rs:1996-2004`). The kernel-side work is done. **What remains is shell-side:** the community plugin scanner in `shell/src-tauri/src/lib.rs:14-28` doesn't deserialize `apiVersion` at all, and `shell/src/types/plugin.ts:78-86` doesn't carry it. Nothing surfaces a version-mismatch error to the end user. | **S** (~2 days). |
| **WI-34** Plugin contract crate guardrail | S | **Contract crate exists and is already clean.** `crates/nexus-plugin-api/Cargo.toml` has no `nexus-kernel` dep. `nexus-plugins` re-exports `PLUGIN_API_VERSION` from `nexus-plugin-api` (`lib.rs:44`). The guardrail to prevent regression has a perfect template in `crates/nexus-bootstrap/tests/dep_invariants.rs` (Phase 1 WI-22 added the legacy freeze to the same tests directory). | **XS** (~1 day). |
| **WI-35** Per-plugin crash quarantine | Stretch | **Partially shipped.** `ExtensionHost.activate()` already try/catches the `plugin.activate(api)` call at `ExtensionHost.ts:151-167` and isolates failures via `this.fail(id, err)`. Trigger-activated failures are also caught in `ActivationTriggers.fire()` at `ActivationTriggers.ts:104-112`. **Gaps:** `CommandRegistry.execute()` at `CommandRegistry.ts:38-54` does NOT try/catch handler invocations; a throwing command handler breaks `executeCommand` chains for every caller. Keybinding dispatch and kernel-event-forwarder handlers similarly lack guards. | **S** (~2-3 days). |

**Net effect:** INTEGRATION-REVIEW estimated "3–4 weeks" for the six WIs assuming one engineer. The audit-corrected estimate is **~3.5 weeks of engineering effort**, but that aggregate hides massive skew — WI-30 alone is 2+ weeks; the other five together are ~1.5 weeks.

**Scoping recommendation:** Split Phase 3 into three sub-phases by risk-versus-payoff:

- **Phase 3a (quick-win hardening, ~1 week):** WI-32 TOCTOU + WI-33 api_version surfacing + WI-34 contract guardrail + WI-35 crash quarantine. Four WIs, zero UX risk, closes four audit findings.
- **Phase 3b (consent UX, ~1 week):** WI-31 install-time capability prompt. Requires UX design review; gated on questions in §8.
- **Phase 3c (sandbox, ~2+ weeks):** WI-30 JS plugin sandbox. **Community-tier only** in Phase 3; first-party extension through the new postMessage RPC is a Phase 4 concern. Gated on ADR.

**Phase 3 acceptance:**

1. `host::write_file` (WASM plugin host) and `KernelPluginContext::write_file` route through `ForgePathValidator::validate_for_write`. Regression test covers symlink-swap race.
2. Community plugin scanner surfaces `apiVersion`; shell-side rejection emits a user-visible notification (not just a kernel log).
3. Settings > Plugins shows an install-time consent prompt for any community plugin declaring HIGH-risk capabilities; user approval writes `granted_caps.json` via a new bridge command.
4. `cargo test -p nexus-bootstrap` includes a new `plugin_contract_purity` test that fails if any crate outside `nexus-kernel` family (`nexus-kernel`, `nexus-bootstrap`, `nexus-plugins`, `nexus-app`, `nexus-shell`) depends on `nexus-kernel`.
5. `CommandRegistry.execute`, `KeybindingRegistry` dispatch, and event-forwarder subscriptions catch plugin-authored errors and isolate them to `console.error` + a `plugin:error` event rather than breaking the caller chain.
6. Community plugins load inside sandboxed iframes (`sandbox="allow-scripts"`, no `allow-same-origin`); CSP is re-enabled in `tauri.conf.json` to at least `script-src 'self' blob:`. First-party plugins continue to load in the main WebView until Phase 4 — gated by an ADR.

---

## 2. Scope summary

### 2.1 Phase 3a — Quick-win hardening (P0, ~1 week)

Ship these first. No open questions; no UX design review; each closes an audit finding with <3 days of code.

| ID | Title | Size | Priority | Audit |
|---|---|---|---|---|
| **WI-32** | Wire `ForgePathValidator::validate_for_write` into both write paths | S | P0 | MK F-5.3.1, F-5.3.2 |
| **WI-33** | Surface `api_version` mismatch to shell (scanner field + UI error) | S | P0 | MK F-9.2.1, UI F-9.1.1 |
| **WI-34** | Plugin-contract crate purity guardrail test | XS | P0 | MK F-2.1.1 |
| **WI-35** | Per-plugin crash quarantine in CommandRegistry / keybindings / event handlers | S | P0 | (stretch in review; promoted) |

### 2.2 Phase 3b — Install-time consent UX (P0, ~1 week)

Blocks any community-marketplace launch. Depends on WI-33 (need `capabilities` flowing through manifest scanner).

| ID | Title | Size | Priority | Audit |
|---|---|---|---|---|
| **WI-31** | Install-time capability prompt modal + grants bridge command | M | P0 | MK F-5.1.1 |

### 2.3 Phase 3c — JS sandbox (P0, 2+ weeks)

The big one. Scope to community-tier only in Phase 3 per §6 risk 3 and §8 open question 1.

| ID | Title | Size | Priority | Audit |
|---|---|---|---|---|
| **WI-30** | Iframe-sandboxed community plugin loader + postMessage RPC + CSP | XL | P0 | UI F-8.1.1, F-5.1.1 (iframe variant) |

**Total Phase 3:** 6 WIs, ~3.5 engineer-weeks if serial; realistic 4 weeks with review + integration time. Parallelization is awkward because WI-30 is end-to-end and touches every layer.

---

## 3. Phase 3a work items (quick-win hardening)

---

### 3.1 WI-32 — TOCTOU write-path fixes (S, P0)

#### 3.1.1 Intent

Close MK F-5.3.1 and F-5.3.2 by routing both plugin-accessible write paths (`host::write_file` for WASM, `KernelPluginContext::write_file` for core) through `ForgePathValidator::validate_for_write` instead of the current inline canonicalize-parent-then-rebuild pattern. The inline pattern was correct-in-spirit but duplicates the validator's logic and drifts over time; consolidating removes the F-5.3.x findings and reduces the blast radius of any future symlink-safety regression.

#### 3.1.2 Current state

- **Validator exists and is complete:** `crates/nexus-security/src/path.rs:16-158`.
  - `ForgePathValidator::new(forge_root)` canonicalizes root at construction — `path.rs:26-36`.
  - `validate(requested)` handles read/existence paths — `path.rs:57-87`.
  - `validate_for_write(requested)` handles writes where the target doesn't exist yet by walking up to the deepest existing ancestor, canonicalizing it, prefix-checking, and re-joining — `path.rs:114-158`. This is exactly the shape the write paths need.
  - Full Unix symlink-escape test coverage at `path.rs:365-375` (`validate_for_write_rejects_symlinked_parent`).

- **WASM write path does its own thing:** `crates/nexus-plugins/src/host_fns.rs:394-485`. The comment at lines 422-428 claims TOCTOU-safe behaviour via `canon_parent.join(file_name)`, but the implementation inlines the validator logic (parent canonicalize + prefix check + `mkdir_all` under an unvalidated parent at line 452) rather than delegating. The `mkdir_all` branch is the subtle bug: `create_dir_all(parent)` is called before `parent.canonicalize()`, so a symlink introduced between the `!parent.exists()` check and the `mkdir_all` call can steer directory creation outside the sandbox. The current validator's `validate_for_write` walks up to the deepest existing ancestor and canonicalizes *that*, sidestepping the issue.

- **Core-plugin write path does its own thing:** `crates/nexus-kernel/src/context_impl.rs:163-213`. Uses a nearly-identical inline pattern. Comment at lines 166-171 claims TOCTOU-safety; the same critique as the WASM path applies (no directory-creation branch here, but the "canonicalize parent, rebuild target" is duplicated logic).

- **Neither crate depends on `nexus-security` today:**
  - `crates/nexus-plugins/Cargo.toml` — no `nexus-security` dep.
  - `crates/nexus-kernel/Cargo.toml` — no `nexus-security` dep.
  - Adding the dep requires checking `dep_invariants.rs` to confirm no new FORBIDDEN entry would be triggered (it wouldn't — `nexus-security` is Layer-1 and both `nexus-plugins` and `nexus-kernel` are Layer-1/2).

#### 3.1.3 Design

Three-commit surgical replacement:

**Commit 1 — Kernel context path.**

In `crates/nexus-kernel/src/context_impl.rs`:
1. Store a `ForgePathValidator` on `KernelPluginContext` instead of (or alongside) the current `forge_root_canonical: PathBuf` field at construction (line 45 area).
2. Replace the `write_file` body (lines 163-213) with: `let target = self.path_validator.validate_for_write(path)?;` followed by `tokio::fs::write(&target, contents).await`.
3. Map `SecurityError::PathTraversal` → `Error::Io(PermissionDenied)` at the boundary so existing callers see the same `Error` variant (don't bleed a new enum variant).
4. Unit test at `context_impl.rs:433`-area: add a case proving a write through a symlinked parent is rejected (on Unix).

**Commit 2 — WASM plugin host path.**

In `crates/nexus-plugins/src/host_fns.rs`:
1. Add `nexus-security = { workspace = true }` to `crates/nexus-plugins/Cargo.toml`.
2. Thread a `ForgePathValidator` (constructed once per plugin load from `plugin_data.forge_root`) through `PluginData` so the host-fn closure can reach it.
3. Replace the body of `register_host_write_file` (lines 394-485) inline-canonicalize branch with a call to `validator.validate_for_write(&requested)`. Preserve the `HOST_OK` / `HOST_CAPABILITY_DENIED` / `HOST_ERROR` return-code contract.
4. Drop the inline `mkdir_all` call — `validate_for_write` handles the "deepest existing ancestor" case; the writing-side is now responsible for `fs::create_dir_all(target.parent())` only after the validator has approved, which is implicit-safe because the canonical ancestor is already prefix-checked.

**Commit 3 — Integration test.**

In `crates/nexus-plugins/tests/` (new or extending an existing file): a symlink-swap race test proving that a malicious symlink introduced between validator construction and write does not let the plugin escape `forge_root`. Unix-only (`#[cfg(unix)]`). Pattern:

```rust
// Create a forge dir and a legitimate subdirectory.
// Load a test WASM plugin; capture the validator.
// After load but before write, replace a subdirectory with a symlink to /tmp.
// Invoke the plugin's write path.
// Assert: the write failed and /tmp is unchanged.
```

#### 3.1.4 Subagent pattern

**Main-thread work.** Three small, sequential, surgical code changes. Subagents add overhead without saving time here. A single Explore agent (optional) could do the initial "list every other `canonicalize\|forge_root\|write_file` call site to check for more instances" sweep — useful precaution but not required.

#### 3.1.5 Commit plan

1. `fix(kernel): route KernelPluginContext::write_file through ForgePathValidator` — closes F-5.3.2.
2. `fix(plugins): route host::write_file through ForgePathValidator` — closes F-5.3.1.
3. `test(plugins): symlink-swap race regression for host write path`.

**Files touched:**
- `crates/nexus-kernel/Cargo.toml` — add `nexus-security` dep.
- `crates/nexus-kernel/src/context_impl.rs` — replace write_file body.
- `crates/nexus-plugins/Cargo.toml` — add `nexus-security` dep.
- `crates/nexus-plugins/src/host_fns.rs` — replace write_file body; thread validator through `PluginData`.
- `crates/nexus-plugins/src/loader.rs` — construct validator per-plugin load (needs the plugin's `forge_root` which `PluginData` already carries).
- `crates/nexus-plugins/tests/toctou_regression.rs` — new test file.

#### 3.1.6 Acceptance

- `cargo test -p nexus-kernel` — green, including the new `write_file_rejects_symlinked_parent` test.
- `cargo test -p nexus-plugins --test toctou_regression` — green on Unix (skipped on Windows).
- `grep -rn "canonicalize" crates/nexus-plugins/src/host_fns.rs` — only matches lie in `read_file` paths, not `write_file`.
- MK F-5.3.1 and F-5.3.2 are marked closed in `docs/planning/MICROKERNEL-AUDIT.md` (status column update).

#### 3.1.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| `validate_for_write` behaviour deviates from the inline pattern's behaviour on some edge case (empty path, root, nonexistent parent all the way up) and breaks an existing plugin | Medium | Run full plugin test suite before merging. Audit `validate_for_write` edge-case tests (`path.rs:337-363`) and add any missing ones that the inline pattern supported. |
| Adding `nexus-security` as a dep of `nexus-kernel` creates a cycle with future refactors | Low | `nexus-security` is Layer 1 under ARCHITECTURE.md §7; it does not depend on `nexus-kernel`. Check once in the commit. |
| The `create_dir_all` removal breaks plugins that relied on write-creating-parents behaviour | Low | The validator doesn't create directories; the write path should `fs::create_dir_all(target.parent())` *after* validation. Preserve that behaviour in the kernel path where it existed (it doesn't in `context_impl.rs`, but it does implicitly in `host_fns.rs:452`). |

#### 3.1.8 Size

**S** — ~3 engineer-days. Breakdown: 1d kernel path + test, 1d plugin host + test, 1d integration symlink-swap test + docs update.

---

### 3.2 WI-33 — Surface `api_version` mismatch to shell (S, P0)

#### 3.2.1 Intent

Close UI F-9.1.1. The kernel already rejects plugins whose `api_version` mismatches `PLUGIN_API_VERSION_MAJOR` (WI-33 in INTEGRATION-REVIEW is stale — this landed). The remaining gap is on the shell side: community plugins loaded through the shell's scanner never have `apiVersion` deserialized, and community-plugin failures during `host.loadAll` surface only as `console.error`. User gets no notification, no actionable error, no "this plugin was written for an older version of Nexus" message.

#### 3.2.2 Current state

- **Kernel side — done.** `crates/nexus-plugins/src/loader.rs:1534-1545` enforces major-version compatibility. `PluginError::IncompatibleApiVersion` at `error.rs:136-143` carries the plugin id, requested version, and supported version. Tested at `loader.rs:1996-2010`.
- **Shell-side scanner — doesn't carry the field.** `shell/src-tauri/src/lib.rs:14-28` defines `CommunityPluginManifest` without `api_version` or `apiVersion` fields. Neither `scan_plugin_directory` nor `scan_plugin_directory_at` attempts to parse it.
- **Shell-side loader — doesn't check it.** `shell/src/host/communityPluginLoader.ts:118-159`'s `loadOnePlugin` reads the JS bundle and invokes it; no check against a shell-side constant happens.
- **Shell-side `PluginManifest` type — missing field.** `shell/src/types/plugin.ts:78-86` has `id, name, version, core, activationEvents, dependsOn, contributes`. No `apiVersion`.
- **Error surfacing today:** a plugin activation failure in `host.loadAll` logs `[Boot] FAILED: ${id}` at `shell/src/main.tsx:219-220` — console only, no user-facing notification.

#### 3.2.3 Design

Four-layer closure:

**Layer A — Rust scanner carries the field.** In `shell/src-tauri/src/lib.rs:14-28`, extend `CommunityPluginManifest` with:
```rust
#[serde(default)]
pub api_version: Option<String>,
```
Default `None` tolerates plugins that omit it (the shell's rejection logic handles that).

**Layer B — Shell-side type + scanner contract.** Extend `CommunityPluginManifest` in `shell/src/host/communityPluginLoader.ts:16-28` with `apiVersion?: string`. Extend `PluginManifest` in `shell/src/types/plugin.ts:78-86` similarly (marked optional so no existing first-party plugin breaks).

**Layer C — Shell-side check before activation.** In `shell/src/host/communityPluginLoader.ts`, add `SHELL_SUPPORTED_API_VERSION = 1` constant. In `loadOnePlugin` (after the `plugin.manifest.id = manifest.id` line at 149), check: if `manifest.apiVersion` is set and its major version `!== SHELL_SUPPORTED_API_VERSION`, throw `IncompatibleApiVersionError` with the mismatched version string. Import `PLUGIN_API_VERSION` from `@nexus/extension-api` (Phase 1 WI-20 derived it via ts-rs).

**Layer D — User-visible surfacing.** In `shell/src/main.tsx:218-224`, extend the `state === 'error'` branch. For any `IncompatibleApiVersionError` (and any community plugin error in general), emit a toast-level notification via the existing `api.notifications` surface. Text: `"Plugin '<name>' requires Nexus API version <requested>; this shell is version <supported>. The plugin was not loaded."`

#### 3.2.4 Subagent pattern

**Single Explore agent** to confirm no other plugins-style manifest-schema exists in the repo that would need updating (low probability, but cheap to check). Prompt: *"Find every deserialization site for `plugin.json` or community plugin manifests across `shell/src-tauri/` and `app/src-tauri/`. Report file:line."* ~3 min.

**Main-thread** writes the four-layer patch.

#### 3.2.5 Commit plan

Two commits (both atomic, reviewable):

1. `feat(shell): surface apiVersion through community plugin scanner` — Layers A, B, C.
2. `feat(shell): notify user when a plugin rejected for api_version mismatch` — Layer D.

**Files touched:**
- `shell/src-tauri/src/lib.rs` — `CommunityPluginManifest` struct.
- `shell/src/host/communityPluginLoader.ts` — TS manifest type + shell-side check + custom error class.
- `shell/src/types/plugin.ts` — optional `apiVersion` field.
- `shell/src/main.tsx` — notification surface in boot error handler.
- `shell/tests/communityPluginLoader.test.ts` (new, or extending existing) — test covering "manifest.apiVersion = '2' → rejection with IncompatibleApiVersionError".

#### 3.2.6 Acceptance

- A community plugin with `"apiVersion": "2"` in its `plugin.json` fails to load with a user-visible notification, not just a console error.
- A community plugin with no `apiVersion` field still loads (back-compat).
- `IncompatibleApiVersionError` is a named error class with `pluginId`, `requestedVersion`, `supportedVersion` fields.
- UI F-9.1.1 marked closed in `docs/planning/UI-AUDIT.md`.

#### 3.2.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| First-party plugins (loaded via `shell/src/main.tsx`, not the community scanner) need an apiVersion check too but don't go through `loadOnePlugin` | Low | First-party plugins ship with the shell binary; their `apiVersion` is implicitly equal to the shell's. No check needed for in-tree plugins. Document this in the notification-emitting commit's message. |
| Kernel-side check in `loader.rs` already rejects WASM plugins with mismatched `api_version`; double-enforcement at shell layer is fine but could surface confusing dual errors | Low | The shell check runs on the JS layer before any IPC; kernel check runs on the WASM layer. They cover disjoint plugin tiers. Document in the commit message. |

#### 3.2.8 Size

**S** — ~2 engineer-days. Breakdown: 0.5d Rust struct + Explore sweep, 0.5d TS types + check, 0.5d notification surface + test, 0.5d doc update.

---

### 3.3 WI-34 — Plugin-contract crate purity guardrail (XS, P0)

#### 3.3.1 Intent

Close MK F-2.1.1 with a structural test. The `nexus-plugin-api` contract crate exists (Phase 1 WI-20), doesn't depend on `nexus-kernel`, and is what community plugin authors should pin. The fix the audit recommends is ensuring community plugins don't transitively pull in `nexus-kernel` internals. Today that's *implicit* — no community plugin has been written yet. A guardrail test makes it *enforced* so a future "convenient" re-export from `nexus-plugin-api` (e.g. `pub use nexus_kernel::EventBus;` in a moment of weakness) is caught at CI time.

#### 3.3.2 Current state

- **Contract crate is clean:** `crates/nexus-plugin-api/Cargo.toml` depends on `serde`, `serde_json`, `thiserror`, `uuid`, `chrono`, `async-trait`, `ts-rs` (optional feature). No `nexus-kernel` dep. Cargo audit would show this crate is linked by `nexus-kernel` / `nexus-plugins` / `nexus-bootstrap` only.
- **`nexus-plugins` re-exports the constant:** `crates/nexus-plugins/src/lib.rs:44` exposes `PLUGIN_API_VERSION` via `pub use nexus_plugin_api::PLUGIN_API_VERSION as PLUGIN_API_VERSION_MAJOR;` — meaning a plugin author can pin to `nexus_plugins` rather than `nexus_plugin_api` today. The guardrail should reflect intent: community plugins pin to `nexus_plugin_api`; they may pin to `nexus_plugins` if they need the plugin loader, but that's weaker.
- **Precedent is perfect:** `crates/nexus-bootstrap/tests/dep_invariants.rs` already enforces crate-level dependency invariants (the FORBIDDEN list at lines 17-40). Phase 1 WI-22 added `legacy_freeze.rs` to the same tests directory using the same workspace-walk pattern.

#### 3.3.3 Design

Single test file: `crates/nexus-bootstrap/tests/plugin_contract_purity.rs`, ~80 LOC.

**Semantics of the test:**

1. The test identifies every "community-tier plugin candidate" crate — any crate under `crates/` (or eventually `plugins/`) whose `Cargo.toml` has a `[package]` section with a well-known marker. For the initial version, there are zero community plugins in-tree; the test exists primarily to fire if one is added. A forward-compatible approach: the test reads a `PLUGIN_PURITY_CRATES` constant list (initially `&[]`) and asserts none of them depend on `nexus-kernel` (direct or via transitive `nexus-plugins` reach-through).

2. An **inverse assertion** — the crate `nexus-plugin-api` must not depend on `nexus-kernel` (regression guard against someone "simplifying" a re-export).

3. A **surface-area assertion** — the `nexus-plugin-api/src/` tree must not have `pub use nexus_kernel::...` anywhere. Literal-string grep. Complements the Cargo-level check.

```rust
// Pseudo:
const FORBIDDEN_CONTRACT_DEP: &str = "nexus-kernel";
const CONTRACT_CRATE: &str = "nexus-plugin-api";

#[test]
fn contract_crate_does_not_depend_on_kernel() { ... }

#[test]
fn contract_crate_source_does_not_reexport_kernel() { ... }

#[test]
fn community_plugin_crates_do_not_depend_on_kernel() {
    // Reads PLUGIN_PURITY_CRATES: &[&str] (initially empty; grows when we
    // ship the first community plugin).
    // For each, asserts no `[dependencies].nexus-kernel` entry.
}
```

The third test is a placeholder-with-teeth — its FORBIDDEN list grows as community plugins land, and shipping a new community plugin becomes an explicit plan-doc decision.

#### 3.3.4 Subagent pattern

**None.** 80 LOC single-file test; agent round-trip costs more than implementation.

#### 3.3.5 Commit plan

Single commit:

`test(bootstrap): plugin contract crate purity guardrail`

**Files touched:**
- `crates/nexus-bootstrap/tests/plugin_contract_purity.rs` — new.
- (Optional) `docs/adr/0012-plugin-contract-purity.md` — restate the invariant for future reviewers. Probably overkill for XS work; leave to discretion.

#### 3.3.6 Acceptance

- `cargo test -p nexus-bootstrap --test plugin_contract_purity` — passes on main.
- Adding `nexus-kernel = { workspace = true }` to `crates/nexus-plugin-api/Cargo.toml` fails the first test.
- Adding `pub use nexus_kernel::EventBus;` to `crates/nexus-plugin-api/src/lib.rs` fails the second test.
- MK F-2.1.1 marked closed.

#### 3.3.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| The literal-grep test (`pub use nexus_kernel::`) has false positives (a comment mentioning the crate name) | Low | Use regex `^\s*pub use nexus_kernel::` or parse with `syn` if the false-positive rate is non-trivial. Start simple. |
| The "community plugin" list being empty makes the test look trivial; reviewers might remove it | Low | Comment the test heavily explaining the grow-with-marketplace intent. |

#### 3.3.8 Size

**XS** — ~1 engineer-day.

---

### 3.4 WI-35 — Per-plugin crash quarantine (S, P0)

#### 3.4.1 Intent

Catch plugin-authored exceptions at every call site the shell itself initiates — so a malicious or buggy plugin cannot break the shell for other plugins by throwing from a command handler, a keybinding, or an event subscription callback. INTEGRATION-REVIEW labeled this "stretch"; the audit shows it's mostly already implemented at plugin *activation* time (Phase 2 WI-19 work), and the remaining gaps are narrow and mechanical. Promoted to P0 because the gap is small and the benefit is large: community plugins can't brick the command palette.

#### 3.4.2 Current state

**Already protected:**
- `ExtensionHost.activate(plugin)` at `shell/src/host/ExtensionHost.ts:151-167`: `try { await plugin.activate(api) } catch (err) { this.registry.unregisterAll(id); ... this.fail(id, err) }`. Full isolation — a plugin that throws in `activate()` logs the error, cleans up partial registrations, transitions to `error` state, and other plugins continue booting.
- `ExtensionHost.unload(id)` at `ExtensionHost.ts:180-184`: catches exceptions from `plugin.deactivate()`.
- `ActivationTriggers.fire(triggerKey)` at `shell/src/host/ActivationTriggers.ts:104-112`: catches errors from trigger-activated plugins; "one bad plugin can't break the trigger source's dispatch loop."

**NOT yet protected:**
- `CommandRegistry.execute(id, ...args)` at `shell/src/registry/CommandRegistry.ts:38-54`: calls `cmd.handler(...args)` with no try/catch. Any throw propagates back to the caller — which is typically `ctrl-shift-P` palette or a keybinding dispatcher. This is the highest-value fix: a command palette that silently dies on a bad plugin is the most user-visible failure mode.
- **Keybinding dispatch** — need to inspect `shell/src/registry/KeybindingRegistry.ts` for the execute path. If it also calls handlers directly (rather than going through `CommandRegistry.execute`), it needs its own guard.
- **Event-forwarder subscriptions** — in `PluginAPI.ts` (Phase 1 WI-06 added subscription tracking), the handler passed to `kernel.on(topic, handler)` is plugin-authored. Event dispatch should try/catch around `handler(...)` so a throw doesn't stall the kernel event loop for every other subscriber.
- **Plugin API method calls originating from first-party code** — e.g. a status bar item's `onClick` is invoked synchronously from React's event handler. Less critical (shell-initiated paths are more focused), but worth a standardized wrapper.

#### 3.4.3 Design

Three-commit progression, each quickly ship-able:

**Commit 1 — CommandRegistry.execute guards.**

Wrap the handler invocation:
```typescript
async execute(id: string, ...args: unknown[]): Promise<unknown> {
  // ... (existing trigger-wake logic) ...
  const cmd = this.commands.get(id)
  if (!cmd?.handler) { ... }
  try {
    return await cmd.handler(...args)
  } catch (err) {
    console.error(`[CommandRegistry] Handler for '${id}' (plugin ${cmd.pluginId}) threw:`, err)
    eventBus.emit('plugin:handlerError', { pluginId: cmd.pluginId, commandId: id, error: err })
    throw err  // re-throw — caller semantics unchanged for non-quarantine callers
  }
}
```
The re-throw preserves current caller contract; the `plugin:handlerError` event is new and reserved for Settings / diagnostic UIs. An explicit log + event is enough — do NOT swallow the throw silently (the caller may legitimately need to react).

*Alternate design choice flagged to user in §8:* should `execute` quarantine silently (swallow + log) or re-throw? Current recommendation: re-throw with logging. Open question.

**Commit 2 — Keybinding dispatch guards.**

Audit `shell/src/registry/KeybindingRegistry.ts` for the execute path; wrap the handler-invocation site in the same try/catch/log/emit pattern. Likely a ~10-line change at the dispatch function.

**Commit 3 — Event-forwarder handler guards.**

In `shell/src/host/PluginAPI.ts` where `kernel.on(topic, handler)` is implemented (the wrap added in Phase 1 WI-06), wrap the handler invocation site so a single plugin's bad subscription callback doesn't stall the forwarder's dispatch loop:
```typescript
const safeHandler = (envelope) => {
  try { handler(envelope) }
  catch (err) {
    console.error(`[PluginAPI] kernel.on handler for plugin ${pluginId} threw:`, err)
    eventBus.emit('plugin:handlerError', { pluginId, source: 'kernel-event', topic, error: err })
  }
}
```
Forward the wrapped `safeHandler` to the underlying Tauri `listen` call rather than the original.

#### 3.4.4 Subagent pattern

**None.** Three small, focused changes; main-thread implementation. Could split to two parallel agents (one for CommandRegistry + KeybindingRegistry, one for PluginAPI) — probably not worth the agent overhead.

#### 3.4.5 Commit plan

1. `feat(registry): quarantine plugin command handler exceptions`
2. `feat(registry): quarantine plugin keybinding handler exceptions`
3. `feat(host): quarantine plugin kernel-event handler exceptions`

Could combine into one commit if review bandwidth permits; three keeps each logical change independent.

**Files touched:**
- `shell/src/registry/CommandRegistry.ts` — wrap execute body.
- `shell/src/registry/KeybindingRegistry.ts` — wrap dispatch body.
- `shell/src/host/PluginAPI.ts` — wrap event-forwarder handler.
- `shell/src/host/EventBus.ts` — add `plugin:handlerError` as a typed event variant (if events are typed).
- `shell/src/host/ExtensionHost.test.ts` and sibling tests — add regression cases (a plugin that throws from `execute`, from a keybinding, from an event subscriber; assert other plugins continue to work).

#### 3.4.6 Acceptance

- A test plugin that throws `new Error("boom")` from its registered command handler:
  - Logs `[CommandRegistry] Handler for '...'` to the console.
  - Emits a `plugin:handlerError` event with the correct `pluginId`, `commandId`, `error`.
  - Does NOT stall the next invocation of the command palette.
  - Does NOT prevent any other plugin from running.
- Same behaviour for keybinding-invoked and kernel-event-invoked handlers.
- `shell/tests/` passes the new regression cases.

#### 3.4.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Current callers rely on `CommandRegistry.execute` throwing to abort a chain (e.g. a "save then close" macro) | Medium | The proposed design re-throws; the only change is the added log + event. Behaviour for caller is identical. Explicitly document in the commit message. |
| Event-forwarder handler wrap changes the `unsubscribe` function identity, breaking the WI-06 subscription-tracking | Low | The wrap should return the inner `unsubscribe` unchanged; only the handler body is wrapped. Test the registry's `trackSubscription` regression from Phase 1 still passes. |
| `plugin:handlerError` event is new and might need downstream subscribers (Settings > Plugins shows errors) to exist before it's useful | Low | Acceptable scope creep into Phase 4; emit the event now, subscribe in a later WI. |

#### 3.4.8 Size

**S** — ~2-3 engineer-days. Breakdown: 0.5d CommandRegistry + test, 0.5d KeybindingRegistry (smaller surface) + test, 1d PluginAPI event-forwarder + regression + tests.

---

## 4. Phase 3b work items (install-time consent UX)

---

### 4.1 WI-31 — Install-time capability prompt (M, P0)

#### 4.1.1 Intent

Close MK F-5.1.1. The kernel already classifies HIGH-risk capabilities and default-denies them unless the user has consented via `granted_caps.json` (per `loader.rs:1587-1701`). Phase 2 WI-18 shipped the capability listing + risk chips in Settings. The missing piece is the **modal prompt** that surfaces HIGH-risk consent requests at install time (or first-activation time, depending on UX choice), and the **bridge command** that writes `granted_caps.json` from the shell.

#### 4.1.2 Current state

- **Kernel persistence of consent — done.** `crates/nexus-plugins/src/loader.rs:1583-1651`:
  - `GrantedCapsFile` struct holds `{ version, granted: Vec<String> }`.
  - `load_granted_high_risk_caps(plugin_dir, plugin_version)` is called at plugin load; grants pinned to plugin version (version bump re-prompts — defensive).
  - `write_grant(plugin_dir, plugin_version, cap, grant)` writes atomically. Private function — not exposed to the shell layer.
- **Kernel default-deny for HIGH-risk — done.** `loader.rs:1666-1701`: HIGH-risk caps not in the grants file are filtered out of the community plugin's `CapabilitySet` and logged at `audit = true` level.
- **Capability display in Settings > Plugins — shipped (Phase 2 WI-18).** `shell/src/plugins/nexus/pluginsMgmt/capabilityInfo.ts` has `CAPABILITY_INFO`, risk buckets, chip colours. `PluginsMgmtView.tsx` renders them with a "High-risk only" filter.
- **`capabilities` field not flowing through the scanner.** Per WI-18 report and confirmed at `shell/src-tauri/src/lib.rs:14-28` and `shell/src/plugins/nexus/pluginsMgmt/index.ts:43` ("Not currently populated by main.tsx — the shell-side PluginManifest has no capabilities field"), the UI has the rendering ready but reads `parseManifestCapabilities(p.capabilities)` from a field that is always `undefined` for both built-in and community plugins today.
- **No bridge command to read / write `granted_caps.json`.** `shell/src-tauri/src/bridge.rs` (checked: 7 bridge commands, no grants-related commands). No shell-side UI can trigger consent.
- **No modal overlay pattern reuse.** `shell/src/plugins/core/confirm` exists but it's a simple confirm dialog; a capability consent prompt needs richer content (risk chips, per-capability description, "deny this one" toggles).
- **No "install event" hook.** Community plugins today are discovered → auto-load-enabled → load. There's no separation between "discovered" and "user-approved." The simplest path is first-activation-time consent rather than first-scan-time consent.

#### 4.1.3 Design

**Key UX decision flagged to user in §8:**
- *Blocking modal on first activation* vs *non-blocking "needs consent" banner in Settings > Plugins*?

Design below assumes **blocking modal on first activation** (the MK F-5.1.1 fix recommendation) with a "Deny all and disable this plugin" escape hatch. If the user prefers the non-blocking variant, the workflow changes but the underlying plumbing (bridge command, grants parsing) is identical.

**Four-layer implementation:**

**Layer A — Manifest flow-through (depends on WI-33).** Extend `CommunityPluginManifest` (Rust scanner + TS type) with `capabilities: Option<Vec<String>>` (analogous to `apiVersion`). Pass into `shell/src/plugins/nexus/pluginsMgmt` via the existing `communityPluginManifests` service registration. The UI already reads `p.capabilities` defensively — once the scanner populates it, chips render.

**Layer B — Grants bridge commands.** In `shell/src-tauri/src/bridge.rs`, add:
```rust
#[tauri::command]
async fn plugin_get_grants(plugin_id: String) -> Result<Vec<String>, String> { ... }

#[tauri::command]
async fn plugin_set_grants(plugin_id: String, plugin_version: String, grants: Vec<String>) -> Result<(), String> { ... }
```
Both write/read `<plugins_dir>/<plugin_id>/granted_caps.json` via the existing kernel-private helpers. *Note:* the current `write_grant` function in `loader.rs` is `pub(crate)`; Phase 3b needs it to either gain public visibility or the bridge re-implements the serialization against `GrantedCapsFile`. Recommendation: expose a narrow public API from `nexus-plugins` — `pub fn set_grant(plugin_dir, plugin_version, cap, grant) -> Result<()>` — and have the bridge call through it. This keeps the path validation + atomic write in the kernel crate.

**Layer C — Consent modal plugin.** New plugin `shell/src/plugins/nexus/capabilityConsent/` (or extend `pluginsMgmt`):
- Subscribes to plugin activation events (`plugin:activationRequested`, a NEW event emitted at `ExtensionHost.activate` start for community plugins with HIGH-risk capabilities in manifest).
- Shows a modal with: plugin name, version, list of HIGH-risk capability chips with per-capability description, "Grant all" / "Grant selected" / "Deny and disable plugin" buttons.
- On grant: calls `plugin_set_grants` bridge command with the selected capability strings.
- On deny: calls a `plugin_disable_community` bridge command that flips `enabled: false` in the plugin.json and reloads.

*Alternative*: instead of a new activation event, intercept at the `communityPluginLoader.loadOnePlugin` boundary — check `granted_caps.json` directly from the bridge before returning the loaded module, and throw a `NeedsConsentError` that the shell handles. Simpler plumbing; slightly less flexible. **Recommendation: intercept at loader, not activation** — keeps the plugin loading story linear and avoids mid-activation rollback.

**Layer D — One-time migration for existing plugins.** If Phase 3b ships after community plugins are in the wild, existing plugins may already be loaded without explicit consent (because Phase 2 default-allowed everything on the shell side — kernel was already restrictive, but the shell never prompted). Migration: on first boot after Phase 3b, scan all installed community plugins for HIGH-risk capabilities without a matching grants file, disable them, and show a single banner: "N plugin(s) need permission review." Let the user tap through consents one by one.

#### 4.1.4 Subagent pattern

Higher-leverage opportunity than Phase 3a items — three distinct surfaces that can parallelize:

- **Agent 1 (Rust):** Bridge commands for `plugin_get_grants` / `plugin_set_grants` / `plugin_disable_community` + public `set_grant` in `nexus-plugins`. Prompt: *"Add three bridge commands to `shell/src-tauri/src/bridge.rs` that read/write `granted_caps.json`. The write path must go through a NEW `pub fn set_grant` function exposed from `crates/nexus-plugins/src/loader.rs` (today `write_grant` is private — make it public under `nexus_plugins::capabilities` or similar sub-module, preserving atomic-write semantics). Return shapes: get → `Vec<String>`, set → `Result<(), String>`. Include tests."*

- **Agent 2 (TS modal):** Consent modal React component + plugin. Prompt: *"Create a new plugin `shell/src/plugins/nexus/capabilityConsent/` with a modal overlay that renders when `api.events.on('plugin:consentRequested')` fires. Props: `{ pluginId, pluginName, pluginVersion, highRiskCaps: Capability[] }`. Render using the existing `capabilityInfo` chip machinery. User actions: Grant all → `api.kernel.invoke('shell', 'plugin_set_grants', { grants: highRiskCaps })` / Grant selected / Deny. Closes modal on resolution."*

- **Agent 3 (TS loader interception):** Modify `communityPluginLoader.loadOnePlugin` to check consent before resolving. Prompt: *"In `shell/src/host/communityPluginLoader.ts::loadOnePlugin`, after reading the manifest but before `return plugin`, if the plugin declares HIGH-risk capabilities, call a new `ensureConsent(pluginId, pluginVersion, highRiskCaps)` helper that invokes the bridge command `plugin_get_grants`, compares against declared HIGH-risk caps, and if any are missing, emits `plugin:consentRequested` and awaits the user's response (resolved via the consent modal). If denied, throw `ConsentDeniedError`."*

Main thread wires the three together, runs integration, writes migration script.

#### 4.1.5 Commit plan

Five commits (each independently testable):

1. `feat(plugins): expose public set_grant API from nexus-plugins`
2. `feat(shell): add plugin_get_grants and plugin_set_grants bridge commands`
3. `feat(shell): flow capabilities field through community plugin scanner` (depends on or overlaps with WI-33's pattern — may land concurrently)
4. `feat(shell): capability consent modal plugin`
5. `feat(shell): intercept community plugin load for HIGH-risk consent + migration banner`

**Files touched (~14 files):**
- `crates/nexus-plugins/src/lib.rs` — re-export `set_grant`.
- `crates/nexus-plugins/src/loader.rs` — extract `set_grant` to public.
- `shell/src-tauri/src/bridge.rs` — three new bridge commands.
- `shell/src-tauri/src/lib.rs` — `CommunityPluginManifest::capabilities`.
- `shell/src/host/communityPluginLoader.ts` — consent interception + manifest type.
- `shell/src/types/plugin.ts` — optional `capabilities` field.
- `shell/src/plugins/nexus/capabilityConsent/{index.ts, ConsentModalView.tsx, consentStore.ts}` — new plugin, ~300 LOC.
- `shell/src/plugins/nexus/pluginsMgmt/` — migration banner render.
- `shell/src/main.tsx` — initial-boot migration sweep for pre-3b installs.
- Tests for each surface.

#### 4.1.6 Acceptance

- Installing a community plugin with `capabilities = ["fs.write", "net.http"]` in its manifest triggers a consent modal on first activation showing the two HIGH-risk chips.
- "Grant all" writes `granted_caps.json` correctly; restart preserves grants.
- "Deny" disables the plugin (writes `enabled: false` to the plugin.json) and records no grants.
- Existing community plugins loaded pre-3b trigger the migration banner on boot; tapping through grants normalizes their state.
- Plugin version bump re-prompts (already enforced kernel-side via `GrantedCapsFile::version`; UI should surface "this plugin was updated; please re-review permissions").
- MK F-5.1.1 marked closed.

#### 4.1.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Modal UX feels intrusive; user fatigues and blindly grants everything | High | Review design with the user (§8 Q2): blocking modal vs non-blocking banner vs settings-page prompt. Default-deny still provides safety net if user ignores the banner. |
| First-boot migration of existing plugins is a one-time flow — hard to test | Medium | Unit test the migration function with fixture plugin.json + simulated pre-3b state. Keep the migration banner explicit ("N plugins") so users who forget can return to it. |
| Modal-during-activation creates a race: multiple plugins could request consent simultaneously at boot | Medium | Queue consent prompts; show one at a time. Phase 3b design should explicitly handle this (not just "show N modals in parallel"). |
| Exposing `set_grant` publicly from `nexus-plugins` expands the plugin-contract surface for bridge-layer consumers | Low | Keep it under a non-stability-promised sub-module (e.g. `nexus_plugins::capabilities_internal`) rather than adding to `nexus-plugin-api`. Not for plugin authors. |

#### 4.1.8 Size

**M** — ~1 engineer-week. Breakdown: 1d bridge + set_grant, 1d manifest flow-through, 2d modal + consent flow, 1d migration + regression tests, 1d polish + docs update.

---

## 5. Phase 3c work items (JS sandbox)

---

### 5.1 WI-30 — Community plugin iframe sandbox + postMessage RPC (XL, P0)

#### 5.1.1 Intent

Close UI F-8.1.1. Today, community plugins execute as ES modules in the main WebView via the Blob-URL trick in `communityPluginLoader.ts:134-138`, inheriting full access to the shell's `window`, `document`, `@tauri-apps/*`, and every other first-party plugin's global state. The fix is structural: community plugins load inside sandboxed `<iframe sandbox="allow-scripts">` (no `allow-same-origin`) with postMessage as the sole communication surface to the host. The host exposes a proxy object that mimics `PluginAPI` shape, enforces capabilities, and marshals calls over postMessage.

This is the most architecturally significant WI in Phase 3 — it changes the plugin execution model. Scoping matters: **first-party plugins stay in the main WebView** through Phase 3 (moving them would require rewriting all 32 of them). **Community plugins are iframe-isolated** from Phase 3 forward.

#### 5.1.2 Current state

- **CSP is disabled.** `shell/src-tauri/tauri.conf.json:26` — `"csp": null`. Re-enabling CSP is part of this WI.
- **No iframe or postMessage plumbing anywhere in the shell.**
  - `grep -rn "iframe\|postMessage" shell/src/` returns one match at `shell/src/plugins/nexus/graph/forceLayout.ts:2` — a comment about `pnpm add` being "sandbox-blocked" (unrelated to iframe sandboxing).
  - No precedent to build on. Full design required.
- **Community plugins are loaded in-process via Blob URL.** `communityPluginLoader.ts:134-138`:
  ```typescript
  const blob = new Blob([source], { type: 'application/javascript' })
  const url  = URL.createObjectURL(blob)
  const mod = await import(/* @vite-ignore */ url)
  ```
  The imported module shares the main-world realm. From inside a community plugin, `window.__TAURI__.invoke(...)` works; `window.top` is reachable; every shell module import is visible via `import()` if the plugin knows the URL.
- **First-party plugins ALSO use this pattern** — they're registered directly in `shell/src/main.tsx:174-201` via static import. Moving them to iframes would break `api.kernel.invoke`, `api.views.register`, `api.workspace.*`, and every other synchronous-shaped API. Risk is real (INTEGRATION-REVIEW §6 risk 3).
- **Today, there is exactly ONE community plugin:** `shell/src/plugins/community/hello-world/` per INTEGRATION-REVIEW appendix. So the blast radius of sandboxing community-tier is tiny.

#### 5.1.3 Design

This WI is big; the plan below is the architectural sketch — detailed design review happens at kickoff after the Phase 3b open question (§8 Q1) is resolved.

**Core architecture: two plugin runtime tiers.**

- **First-party / core plugins** — continue as main-realm ES modules. Load via `shell/src/main.tsx` direct imports. Use `PluginAPI` synchronously. Unchanged through Phase 3.
- **Community plugins** — load into a sandboxed iframe, communicate exclusively via postMessage with an RPC layer that proxies a *new, async-everywhere* `PluginAPI` shape.

**Iframe host container:**

Each community plugin gets its own iframe element, created lazily at load time, attached to a hidden container (`<div id="plugin-sandbox-container" />`) in `shell/index.html`. Iframe attributes:
```html
<iframe
  sandbox="allow-scripts"
  srcdoc="<html><body><script>...</script></body></html>"
  style="display:none"
  data-plugin-id="..."
/>
```
Notably **no `allow-same-origin`**, which means:
- The iframe has a null origin — cannot read parent.
- No localStorage, no cookies (both tied to origin).
- `fetch` to any URL hits CORS as a non-credentialed request.
- `window.parent.postMessage` is available — that's our RPC channel.

The iframe's HTML bootstraps a small host stub (~100 LOC) that:
1. On `DOMContentLoaded`, posts a "ready" message.
2. Awaits a "load-plugin" message carrying the plugin's JS source (transferred as a string over postMessage; the host reads the bundle from disk via the existing fs plugin and relays it).
3. `eval()`s or `new Function()`s the plugin source. Plugin exports a default Plugin object.
4. Installs a proxy `api` object whose every method marshals calls via `window.parent.postMessage`.

**PostMessage RPC protocol:**

Define in `shell/src/host/sandboxRpc.ts` (new). Message shapes:
```typescript
// Plugin → Host
type PluginMessage =
  | { kind: 'ready'; pluginId: string }
  | { kind: 'rpc-request'; id: string; method: string; args: unknown[] }
  | { kind: 'event-emit'; topic: string; payload: unknown }

// Host → Plugin
type HostMessage =
  | { kind: 'load-plugin'; source: string; manifest: PluginManifest; capabilities: Capability[] }
  | { kind: 'rpc-response'; id: string; result?: unknown; error?: string }
  | { kind: 'event-dispatch'; topic: string; payload: unknown }
```

The `method` field on `rpc-request` is a dotted path into the API surface: `kernel.invoke`, `views.register`, `storage.set`, etc. The host dispatches via a switch over the method string, enforces the plugin's capability set, calls the real `PluginAPI` method, returns result or error in `rpc-response`.

**Capability enforcement at the RPC boundary:**

Every RPC method has a required-capability annotation (static map, `methodCapabilityMap: Record<string, Capability>`). Before dispatching, the host checks `pluginCaps.has(required)` and rejects with `{ error: 'CapabilityDenied: fs.write' }` if not.

**API shape changes (the painful part):**

The existing `PluginAPI` shape has plenty of synchronous getters (`api.commands.all()`, `api.storage.get(key)`). These don't translate to postMessage (which is async-only). Options:

- **Option A — All-async `PluginAPI` for community tier.** Every method returns `Promise<T>`. Plugin authors write community plugins differently from core. Forks the API surface.
- **Option B — Snapshot-based proxy.** On `load-plugin`, the host sends a snapshot of synchronous state (e.g. the current command list); the proxy serves reads from the snapshot and fires async updates via `event-dispatch`. Messier but preserves API shape.
- **Option C — Subset of PluginAPI.** Community tier gets a narrower `CommunityPluginAPI` that drops sync methods entirely. Reduces what community plugins can do by design.

**Recommendation: Option C** for Phase 3c. Write `CommunityPluginAPI` as a new type in `@nexus/extension-api` with `kernel.invoke`, `kernel.on`, `views.register`, `commands.register`, `notifications.show`, and a handful of other async-friendly methods. Drop synchronous reads. This surfaces the architectural reality that community plugins are second-class by design and authors plan around it.

**CSP re-enablement:**

`shell/src-tauri/tauri.conf.json`:
```json
"csp": "default-src 'self'; script-src 'self' blob: 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self' http://ipc.localhost; frame-src 'self' blob:"
```
- `'unsafe-inline'` on `script-src` is needed for the iframe's `srcdoc` bootstrap; explicitly scope to only that, revisit post-3c.
- `frame-src 'self' blob:` allows the sandbox iframes.
- `connect-src` covers Tauri's IPC channel plus any plugin networking.

Re-enabling CSP may break currently-working first-party behaviour; expect to iterate on the directives during Phase 3c testing.

**Migration path for the one existing community plugin (`hello-world`):**

1. Verify it compiles under the new `CommunityPluginAPI` shape. If it uses a sync method, adapt to async (likely trivial for a hello-world).
2. Add an integration test in `shell/tests/sandbox-smoke.ts` that loads the plugin, invokes one command, sees the result round-trip.

#### 5.1.4 Subagent pattern

High-leverage fan-out; this is the closest thing to WI-01 (AI chat) in Phase 2 — big vertical slice split into layers.

**Agent 0 — design review (before any code).** Prompt: *"Audit CSP configurations in Tauri 2.x apps. Report: (a) what directives are required for a sandboxed-iframe plugin model with blob-URL iframes, (b) what breaks if we restore default CSP. (c) known Tauri-specific directives (http://ipc.localhost). Produce a proposed CSP string and the list of first-party code paths likely to break. ~800 words."* **Use context7 for current Tauri docs.**

**Agent 1 — Iframe host + RPC protocol.** Prompt: *"Create `shell/src/host/sandboxRpc.ts` with the Host-side RPC dispatcher per [design]. Include: message type definitions, dispatch switch with capability enforcement, pending-request tracking (map id → { resolve, reject }), error serialization. Unit tests against a mock MessageChannel. Do NOT wire to real iframes yet — that's Agent 3."*

**Agent 2 — CommunityPluginAPI type.** Prompt: *"Define `CommunityPluginAPI` interface in `packages/nexus-extension-api/src/community.ts`. Shape: async-only methods — kernel.invoke, kernel.on, views.register, commands.register, notifications.show, plus a handful of others you judge necessary from reading the existing `PluginAPI` in `shell/src/types/plugin.ts`. Export from the package. Include JSDoc citing the Phase 3c rationale."*

**Agent 3 — Iframe lifecycle + loader.** Prompt: *"Modify `shell/src/host/communityPluginLoader.ts` so loading a community plugin creates a sandboxed iframe (per [design]), posts the `load-plugin` message, and returns a proxy Plugin object that routes `activate(api)` calls into the iframe. Include the iframe cleanup path on `unload`. Integration test: load a tiny inline plugin, call its activate, see it register one command via RPC, invoke the command from the host, assert result round-trips."*

**Agent 4 — CSP re-enablement.** Prompt: *"Update `shell/src-tauri/tauri.conf.json` to re-enable CSP per [design]. Identify and fix any first-party code paths that break under the new CSP — likely: inline event handlers, inline styles, external CDN resources. Report: every directive needed, every code change required, any behaviour change observable at runtime."*

**Agent 5 — hello-world port + integration test.** Prompt: *"Port `shell/src/plugins/community/hello-world/` to the new `CommunityPluginAPI` shape. Write an end-to-end test that boots the shell, loads the plugin, invokes its hello command, asserts the notification fires. Document any API gaps discovered."*

Main-thread work: orchestrate the fan-out, review each agent's output, integrate, write the Phase 3c acceptance smoke test.

#### 5.1.5 Commit plan

~8 commits over the ~2 week stretch:

1. `docs: Phase 3c sandbox architecture review` (Agent 0 output, no code).
2. `feat(extension-api): CommunityPluginAPI async-only interface`
3. `feat(host): sandbox RPC dispatcher with capability enforcement`
4. `feat(host): iframe-sandboxed community plugin loader`
5. `feat(host): capability enforcement at postMessage boundary`
6. `chore(shell): re-enable CSP for iframe-sandbox model`
7. `refactor(plugins/community/hello-world): port to CommunityPluginAPI`
8. `test(sandbox): end-to-end integration smoke`

#### 5.1.6 Files touched (estimated)

- `docs/adr/0012-community-plugin-sandbox.md` — new ADR capturing the tier-split.
- `packages/nexus-extension-api/src/community.ts` — new `CommunityPluginAPI`.
- `packages/nexus-extension-api/src/index.ts` — export new type.
- `shell/src-tauri/tauri.conf.json` — CSP update.
- `shell/index.html` — `#plugin-sandbox-container` div.
- `shell/src/host/sandboxRpc.ts` — new, ~400 LOC dispatcher.
- `shell/src/host/sandboxedPluginProxy.ts` — new, ~200 LOC proxy Plugin wrapper.
- `shell/src/host/communityPluginLoader.ts` — rework to create iframes.
- `shell/src/host/ExtensionHost.ts` — minor: distinguish core vs community activation.
- `shell/src/plugins/community/hello-world/*.ts` — port.
- `shell/tests/sandbox.test.ts`, `shell/tests/sandbox-e2e.test.ts` — new tests.

#### 5.1.7 Acceptance

- A community plugin loads into a sandboxed iframe. Inspecting the DOM shows `<iframe sandbox="allow-scripts">` without `allow-same-origin`.
- From inside the plugin, `window.top`, `window.parent.document`, and `window.__TAURI__` are all undefined or inaccessible.
- `api.kernel.invoke('com.nexus.storage', 'read_file', { path: '...' })` works end-to-end via postMessage.
- A plugin without `fs.read` capability gets `CapabilityDenied` when calling `kernel.invoke` for a storage command.
- `tauri.conf.json` CSP is restored (non-null value).
- First-party plugins continue to load and function normally (no regression).
- `shell/tests/sandbox-e2e.test.ts` passes.
- UI F-8.1.1 marked closed.

#### 5.1.8 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| CommunityPluginAPI diverges from PluginAPI enough that authors need separate documentation + sample plugins | High | Document the split explicitly in the ADR. Ship a community plugin template alongside the first Phase 3c release. Plan for convergence in Phase 4 (if feasible). |
| postMessage marshaling overhead adds latency to plugin IPC | Medium | Benchmark before/after on a representative RPC; if >10ms per call, consider a shared MessageChannel + batched dispatch. |
| CSP re-enablement breaks first-party plugins (inline handlers, CDN resources) | High | Agent 0's report catches the surface; Agent 4 implements fixes. Expect at least one round of "CSP violation, adjust directive" iteration. |
| Iframe sandbox blocks features plugins legitimately need (e.g. clipboard API requires `allow-same-origin` in some configurations) | Medium | Surface via `capability-backed` API — `api.clipboard.copy` dispatches via postMessage to the host, which executes the clipboard op in the main realm. Clipboard becomes a capability. |
| The single existing community plugin is insufficient to validate the model; real surprises emerge when 3rd-party plugins ship | Medium | Deliberately write 2-3 more test plugins during Phase 3c that exercise different surfaces (views, kernel subscriptions, HTTP). |
| Phase 3c's scope blows past 2 weeks because of CSP-debugging surprises | High | Time-box to 3 weeks; if not done, park at the nearest commit-safe point and defer remaining work (the least-critical: porting additional community plugins) to Phase 4. |

#### 5.1.9 Size

**XL** — ~2-3 engineer-weeks. Breakdown: 2d architecture + CSP research (Agent 0), 3d RPC + CommunityPluginAPI (Agents 1,2), 4d iframe lifecycle + loader (Agent 3), 2d CSP re-enablement + fixes (Agent 4), 2d port + integration (Agent 5), 2d regression + docs. Plan for +1 week buffer; this is new territory.

---

## 6. Dependency graph & parallelization

### 6.1 Dependencies between WIs

```
 Phase 2 complete (extension-api stable; bridge subscriptions; capability UI)
                              │
         ┌────────────────────┼────────────────────┐
         │                    │                    │
         ▼                    ▼                    ▼
   ┌───────────┐       ┌───────────┐        ┌───────────┐
   │  WI-32    │       │  WI-33    │        │  WI-34    │
   │  TOCTOU   │       │  apiVer   │        │  Contract │
   │  fixes    │       │  surface  │        │  purity   │
   │  (S, P0)  │       │  (S, P0)  │        │  (XS, P0) │
   └─────┬─────┘       └─────┬─────┘        └─────┬─────┘
         │                   │                    │
         └───────────┬───────┴────────────────────┘
                     ▼
              ┌─────────────┐
              │   WI-35     │  independent
              │   Crash     │  of all others
              │   quarantine│
              │   (S, P0)   │
              └─────────────┘

 ═══════════════════ Phase 3a done — shippable hardening ═══════════════

                     ┌──────────────────────────┐
        WI-33 ──────▶│         WI-31            │  depends on WI-33
      (manifest      │   Install-time consent   │  for `capabilities`
       flow-through) │   modal + bridge cmds    │  field pattern
                     │       (M, P0)            │
                     └───────────┬──────────────┘
                                 │
 ═════════════════ Phase 3b done — consent UX shipped ═════════════════

                     ┌──────────────────────────┐
                     │         WI-30            │  independent, but
                     │   Community iframe       │  benefits from WI-31
                     │   sandbox + postMessage  │  being settled
                     │       (XL, P0)           │  (granted caps enforced
                     └──────────────────────────┘   at RPC boundary)

 ═══════════════════ Phase 3c done — community safe ═══════════════════
```

### 6.2 Single-engineer serialization (~3.5 weeks)

- **Week 1** — Phase 3a: WI-32 (3d) + WI-33 (2d). Ship both before end of week.
- **Week 2** — Finish Phase 3a: WI-34 (1d) + WI-35 (3d). Start WI-31 (1d).
- **Week 3** — Phase 3b: WI-31 (4d).
- **Week 4+** — Phase 3c: WI-30 kickoff, architecture review, first commits.
- **Week 5-6** — Phase 3c implementation.

### 6.3 Two-engineer parallelization (~2.5 calendar weeks)

- **Engineer A (Rust-leaning):** WI-32 → WI-34 → WI-31 (bridge commands + grants API) → WI-30 (Agents 0, 1).
- **Engineer B (TS-leaning):** WI-33 → WI-35 → WI-31 (modal + loader interception) → WI-30 (Agents 2, 3, 4, 5).

Critical path runs through WI-30 (~2 weeks end-to-end).

### 6.4 Agent-heavy run (one engineer + Claude, ~3 weeks)

Phase 3a WIs are small enough that agent overhead isn't worth it — do those in the main thread (~1 week). Phase 3b has three parallelizable layers (Rust bridge, TS modal, TS loader interception) — fan out to agents (~1 week). Phase 3c is the best agent opportunity with six distinct sub-agents running in sequence (Agent 0 → 1 → 2 → 3 → 4 → 5), but depends heavily on main-thread integration (~1 week agents + 1 week integration).

---

## 7. Risks & mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| WI-30 sandbox model forces CommunityPluginAPI divergence that community plugin authors can't easily adapt to | High | ADR up front; ship a template + sample plugin; document the two-tier model in package README. Consider scoping Phase 3c further (only RPC surface; keep iframe for Phase 4) if friction is too high. |
| WI-31 blocking-modal UX is intrusive and users fatigue-grant everything | High | User research spike before committing design; explore non-blocking banner + "review permissions" settings panel. §8 Q2 surfaces the call. |
| WI-30 CSP re-enablement surfaces several first-party breakages that drag out debug time | High | Time-box CSP work to 2 days; if not converging, land a permissive CSP (`default-src * blob:`) with a TODO to tighten in Phase 4. Avoid ship blockage. |
| WI-32 `ForgePathValidator::validate_for_write` has an edge case the inline pattern tolerated (e.g. writing through a non-existent intermediate directory that gets mkdir'd in the same call) | Medium | The validator walks up to the deepest existing ancestor — behaviour is specifically designed for this case. Confirm with a targeted test before landing. |
| WI-35 `CommandRegistry.execute` re-throw vs swallow choice picks the wrong default; existing callers break | Medium | Re-throwing is the conservative default (preserves current semantics). §8 Q3 flags this to the user. |
| WI-31 modal orchestration becomes re-entrant (two community plugins request consent simultaneously at boot) | Medium | Queue consent requests; document the serialization. Acceptance test covers it. |
| Phase 3a WIs individually trivial but collectively easy to "forget to close the audit" — findings remain open in MICROKERNEL-AUDIT.md even after code ships | Low | Each commit message explicitly states which F-ID it closes; final Phase 3 ship includes a single commit updating both audit docs. |
| Phase 3c's scope creep risks blocking Phase 4 milestone | Medium | Ship Phase 3a + 3b first as `v0.2.0-security-hardening`; scope-cut Phase 3c to "sandbox running, CSP restored, hello-world ported" rather than "all community plugins migrated." |

---

## 8. Open questions for user before execution

These decisions materially shape the implementation. Surface at Phase 3 kickoff; defaults in the plan apply otherwise.

1. **WI-30 sandbox tiering — community-only or all plugins?**
   The plan assumes iframe-sandbox applies to community-tier plugins only; first-party plugins (all 32 currently registered in `shell/src/main.tsx`) continue in the main WebView. The INTEGRATION-REVIEW §6 risk 3 flags the alternative (rewrite everything against postMessage RPC) as ~complete plugin API redesign. *Recommendation:* community-only in Phase 3; defer first-party sandboxing to Phase 4 with an ADR. Alternative: sandbox first-party too, and treat Phase 3c as a 4-6 week effort.

2. **WI-31 consent UX — blocking modal or non-blocking banner?**
   - **Blocking modal on first activation (MK F-5.1.1 recommendation):** strongest safety; interrupts boot.
   - **Non-blocking "N plugin(s) need permission review" banner in Settings > Plugins:** less intrusive; relies on user to follow through; default-deny still protects until they do.
   - **Hybrid: Blocking modal on explicit "install plugin" action, banner for pre-existing plugins:** best of both, more UX work.
   *Recommendation:* blocking modal as the default; switch to hybrid if user-test reveals friction.

3. **WI-35 CommandRegistry.execute throw semantics — re-throw or swallow?**
   - **Re-throw (current plan):** callers that chain commands (e.g. macros, undoable actions) work unchanged. Quarantine is log + event only.
   - **Swallow + log:** callers never see handler errors; clean isolation. But existing code that relies on a thrown error to abort a macro chain is silently broken.
   *Recommendation:* re-throw, document the `plugin:handlerError` event for diagnostic consumers.

4. **WI-31 grants pinned to plugin version — behaviour on minor-version bumps?**
   Kernel currently re-prompts on ANY version mismatch (`loader.rs:1604`). For minor/patch bumps (bug fix release), re-prompting feels excessive. Options:
   - **Status quo:** any version bump re-prompts.
   - **Semver-aware:** only re-prompt on major-version bump; accept within major.
   - **Capability-scoped:** re-prompt only if the new version's manifest adds HIGH-risk capabilities not previously declared.
   *Recommendation:* semver-aware with audit log of accepted minor upgrades. Small kernel change.

5. **Phase 3a acceptance gate — ship 3a independently or bundle with 3b/3c?**
   Phase 3a closes four audit findings and has zero UX risk. Shipping it as `v0.1.x-security-patch` makes sense if external users are already on `v0.1.0`. Phase 3b/3c wait for the feature train. Alternative: hold everything for a single `v0.2.0-security` release.
   *Recommendation:* ship 3a independently if any external users exist; otherwise bundle.

---

## 9. What this plan does NOT cover

- **Phase 4 frontend unification.** Including: CLI/TUI/MCP/Desktop command taxonomy sync, retiring `crates/nexus-app`, folding CLI launcher into shell. Separate plan.
- **First-party plugin sandboxing.** Deferred to Phase 4 per §8 Q1.
- **Marketplace infrastructure.** Static JSON index, `nexus plugin install <id>`, marketplace UI. Phase 5.
- **Plugin signing / code-signing / reproducible build verification.** Not in scope.
- **Capability-audit trail / user-visible "what did plugin X do recently" panel.** Nice-to-have; not Phase 3.
- **`capability_granted` / `capability_denied` event bus topics exposed to non-plugin consumers** — currently logged via `audit=true` tracing; could surface in a "plugin security dashboard" later.
- **Per-plugin resource budgets (CPU, memory).** UI F-8.3.x; Phase 5 territory.
- **Bug fixes or feature additions unrelated to security.** This is a security-specific phase.

---

## 10. Next action

1. **Kickoff check** — resolve the five §8 open questions. Fast — all have recommended defaults.
2. **Start Phase 3a in parallel:**
   - WI-32 (kernel write path) — straightforward; main-thread work.
   - WI-33 (api_version surfacing) — small Explore sweep + code.
   - WI-34 (contract purity test) — single file.
   - WI-35 (crash quarantine) — three files.
   All four can land in 5-7 business days. Gives the shell immediate hardening wins.
3. **Ship Phase 3a as its own release** (`v0.1.1-security-hardening` or similar) if §8 Q5 resolves that way.
4. **Begin Phase 3b.** WI-31 after WI-33's manifest flow-through pattern is settled.
5. **Start Phase 3c with Agent 0 design review.** Don't commit implementation code until the CSP research + sandbox architecture doc is accepted.
6. **Checkpoint at the 3a → 3b boundary** against this plan's estimates; reforecast before Phase 3c which is where the real variance lives.

Each WI's commit plan is self-contained (§3-§5); land them incrementally per the Phase 0 workflow. Update `docs/planning/MICROKERNEL-AUDIT.md` and `docs/planning/UI-AUDIT.md` in the same commit that closes each finding — don't leave the audit docs stale.
