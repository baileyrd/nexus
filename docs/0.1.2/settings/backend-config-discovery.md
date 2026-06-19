# Backend Config Discovery — Design Spec

> **Status:** Proposal (2026-06-19). Fulfils [`settings-integration-2026-06-19.md`](settings-integration-2026-06-19.md)
> §5 Recommendation #1 ("Make backend configs discoverable"). Not yet implemented.
> Companion to the [`README.md`](README.md) config-surface index.

## 1. Problem

Nexus has **three parallel settings worlds** (see the integration audit §1):

- **(A) Shell config schemas** — a plugin declares `contributes.configuration` and the
  Settings panel renders it generically. Persisted to one flat `[settings]` table in
  `<forge>/.forge/app.toml`.
- **(B) Backend service TOML** — per-service `Config` structs load `ai.toml`, `mcp.toml`,
  `lsp.toml`, `dap.toml`, `sandbox.toml`, `notifications.toml`, kernel `.nexus/config.toml`.
- **(C) Per-plugin `settings.json`** — community plugin JSON Schema.

The Settings panel only knows about World (A). World (B) — the settings users most often
*want* to change for MCP servers, language servers, debug adapters, sandbox policy, and
kernel limits — is **invisible to the UI**: there is no mechanism to discover or enumerate
backend `Config` structs. This spec proposes that mechanism.

## 2. Goals / non-goals

**Goals**

- A **uniform IPC contract** that any config-owning service implements to expose its config
  to the UI: discover (`config_schema`), read (`config_get`), write (`config_set`).
- **Auto-derived schemas** from the existing Rust `Config` structs via `schemars`, so there
  is no second hand-maintained schema to drift.
- A **single generic renderer** in the shell (one "Backend Services" settings surface) that
  consumes those schemas — no per-service bespoke UI.
- **File-as-truth preserved.** Each service's own TOML stays authoritative; the shell never
  shadows backend config into `app.toml`.
- **Cheap opt-in.** A service joins by adding three handler ids, three cap-matrix rows, and
  implementing one small trait — most of the body is shared.

**Non-goals**

- Replacing World (A). Shell-only settings (ghost-text margin, line numbers, …) stay
  declarative; this is strictly for backend `Config` structs.
- Surfacing `nexus-kernel` config in v1 (it is not a plugin, has no IPC surface, lives at
  `.nexus/config.toml`, and is restart-only — see §10).
- Editing secrets in plaintext round-trips (see §7 secret handling).
- A live two-way binding for restart-required settings — those are written + flagged, not
  hot-applied (see §8).

## 3. Current state (grounded)

What already exists that we build on:

- **Dispatch.** Each `CorePlugin` (trait at `crates/nexus-plugins/src/loader.rs:91`) routes on
  a `u32` handler id via `dispatch` / `dispatch_async` (`loader.rs:137,155`). The
  `command:&str → handler_id:u32` map is a static `IPC_HANDLERS` slice per crate
  (e.g. `crates/nexus-ai/src/core_plugin.rs:257`, `crates/nexus-mcp/src/core_plugin.rs:87`).
  A shared `define_dispatch_helpers!()` macro (`crates/nexus-plugins/src/lib.rs:54`) already
  injects `parse_args::<T>()` / `exec_err()`.
- **An existing by-kind config proxy.** `nexus-storage` already implements
  `config_read` / `config_reset` keyed by `{ kind: "app"|"workspace"|"mcp"|"ai" }`
  (`crates/nexus-storage/src/core_plugin.rs:91-94,440-441`,
  `crates/nexus-storage/src/handlers/config.rs`). This is the closest precedent and is
  reused for file-only configs (§10).
- **A working backend round-trip.** `aiSettings` already pushes UI values to the backend via
  `com.nexus.ai::set_config` and reads back via `com.nexus.ai::config`
  (`shell/src/plugins/nexus/ai/aiRuntime.ts:289-413`). It is a per-service *shadow mirror*
  (declarative fields → `app.toml` → `config:changed` → mirror → IPC). This spec generalises
  that one-off into a protocol and lets the mirror be retired (§11, Phase 4).
- **Schema generation infra.** `schemars` ("1", locked `1.2.1`) is a workspace dep
  (`Cargo.toml:233`), unconditional for `nexus-mcp/lsp/dap/acp/bootstrap`, `ts-export`-gated
  elsewhere. `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` already calls
  `schemars::schema_for!` (`write_schema<T>()`, `:193-202`) to emit rich JSON Schema
  (Draft 2020-12 with `type`, `description`, `enum`, `format`, `minimum`, `required`,
  `$defs`) into `crates/nexus-bootstrap/schemas/ipc/*.json`, drift-checked by
  `scripts/check_ipc_drift.sh`. There is precedent for "schema → form" in DAP's
  `launch_config_schema`.
- **Capability gating** is declarative in `crates/nexus-bootstrap/cap_matrix.toml`, enforced
  at the kernel boundary (`crates/nexus-kernel/src/context_impl.rs:184-219`), not in handler
  bodies. `com.nexus.ai::set_config` already gates on `ai.config.write` (cap_matrix.toml:162).
- **Shell render surface.** The declarative `ConfigSchema` type
  (`packages/nexus-extension-api/src/index.ts:621`) supports only 6 flat field types
  (`boolean | string | password | number | select | keybinding`), no nested/array/range, and
  every field writes through `configStore.set` → `app.toml`. It is therefore **not** a fit
  for nested backend configs. But plugins *can* register fully custom React settings tabs
  (`api.settings.registerTab(id, Component, meta)`, rendered via `ContributedTabBody`,
  `SettingsPanelView.tsx:512-517`), proven by the editor's `ReplKernelsTab`. The backend-config
  renderer is one such custom surface.

## 4. Design overview

```
┌────────────────────────────────────────────────────────────────────┐
│ Shell: "Backend Services" custom settings tab (one renderer)        │
│   discover services → for each: config_schema + config_get          │
│   render JSON-Schema-driven form → on Apply: config_set             │
└───────────────▲───────────────────────────────────┬────────────────┘
        ipc_call │ config_schema / config_get        │ config_set
                 │ (unrestricted, secret-free)       │ (<svc>.config.write)
┌────────────────┴───────────────────────────────────▼────────────────┐
│ Service plugin com.nexus.<svc>                                       │
│   impl ConfigExposing for Plugin { type Config = <Svc>Config; … }    │
│   3 handler ids → shared handle_config_{schema,get,set}::<C>(…)      │
│   - schema: schemars::schema_for!(C) + metadata                     │
│   - get:    current() → redact secrets → JSON                       │
│   - set:    deserialize patch → validate → save_toml → hot-apply    │
└──────────────────────────────────────────────────────────────────────┘
                                   │ file-as-truth
                          <forge>/.forge/<svc>.toml
```

Three pieces:

1. **A shared trait + generic handlers** in `nexus-plugins` (a leaf crate every service
   already depends on; the kernel must not — invariant 2). Each service implements one small
   trait; the schema/get/set handler bodies are shared generic functions.
2. **A uniform IPC contract** — three well-known command names (§5). Each service still owns
   its own handler ids, `IPC_HANDLERS` row, and cap-matrix rows.
3. **One shell renderer** — a custom settings tab that discovers participating services and
   renders each service's `schemars` JSON Schema with a richer control set than the 6-type
   declarative renderer (§9).

## 5. The IPC contract

Every config-owning service plugin `com.nexus.<svc>` implements three commands:

### `config_schema` → `ConfigSchemaReply`
- **Args:** none (or `{ "section": string }` for services that expose multiple structs).
- **Returns:**
  ```jsonc
  {
    "service": "com.nexus.mcp",
    "title": "MCP",
    "schema": { /* schemars::schema_for!(McpHostConfig), Draft 2020-12 */ },
    "secretFields": ["servers.*.headers.Authorization"],  // JSON-pointer-ish globs
    "restartRequired": false,        // true → UI shows "restart to apply"
    "version": 1
  }
  ```
- **Cap:** `unrestricted` (no secrets; schema only).

### `config_get` → `serde_json::Value`
- **Args:** none.
- **Returns:** the current **effective** config as JSON, with secret fields **redacted**
  (replaced by a sentinel, e.g. `"•••set•••"` for present, `null`
  for unset). Shape matches `schema`.
- **Cap:** `unrestricted` ("no secrets returned", mirroring `com.nexus.ai::config`).

### `config_set(patch)` → `serde_json::Value`
- **Args:** `{ "patch": <partial config JSON> }` — a deep-merge patch (RFC-7386-style:
  `null` clears a key). Sentinel secret values are treated as "leave unchanged" so a
  round-trip of a redacted `config_get` never erases a secret.
- **Behaviour:** deserialize merged config → validate (serde + service invariants) →
  persist to the service's own TOML via the existing `nexus-formats` `save_toml` helpers →
  hot-apply where supported (§8) → return the new redacted snapshot.
- **Cap:** `<svc>.config.write` (e.g. `mcp.config.write`), mirroring `ai.config.write`.

Notes:
- **Discovery.** The shell enumerates candidate services from a static allow-list shipped in
  the backend-services plugin (the set of `com.nexus.*` that implement the contract), then
  probes `config_schema`; a service that returns `MethodNotFound` is simply hidden. (A future
  refinement: a kernel-level `list_config_services` registry. Out of scope for v1.)
- **Why a patch, not a full document.** Avoids lost-update races and lets the UI send only
  what changed; also sidesteps re-sending redacted secrets.

## 6. Schema generation (schemars) + drift

- Each participating `Config` struct gains `#[derive(JsonSchema)]` (alongside
  `Serialize, Deserialize`). For services where `schemars` is `ts-export`-gated today, the
  derive is either promoted to unconditional (mcp/lsp/dap/acp already are) or kept behind the
  feature where the `config_schema` body is also gated.
- `config_schema` returns `serde_json::to_value(schemars::schema_for!(C))` plus metadata —
  the **same generator already used** by `ipc_schema_emit.rs`. No second schema to maintain.
- **Drift coverage.** Extend `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` to emit each
  participating `Config`'s schema to `schemas/ipc/<svc>_config.json`, so
  `scripts/check_ipc_drift.sh` (and CI) fail if a struct changes without regenerating. This
  also gives reviewers a committed, diffable view of every backend config shape.
- **UI hints.** schemars custom attributes (`#[schemars(extend("x-secret" = true))]`,
  `#[schemars(range(min = …))]`, `#[schemars(description = …)]`) carry secret/range/enum hints
  into the schema so the renderer needs no out-of-band knowledge. `secretFields` in the reply
  is the normalised list derived from `x-secret`.

## 7. Secret handling

- Fields holding credentials (API keys, tokens, SMTP/Discord/Telegram secrets) are annotated
  `x-secret` in the schema and listed in `secretFields`.
- `config_get` **redacts**: present → sentinel, absent → `null`. Secrets are never returned
  over the read path (matching the existing `ai::config` "no secrets returned" guarantee).
- `config_set` treats the sentinel as "unchanged"; a real new value replaces it. This makes a
  load-edit-save round-trip safe without ever transmitting the existing secret to the UI.
- The renderer shows secret fields as masked `password` inputs with a "leave blank to keep"
  affordance.
- Secrets continue to support `${ENV}` substitution on load (the `nexus-formats` loaders
  already do this); the UI surfaces, but does not resolve, `${ENV}` placeholders.

## 8. Persistence & live-apply

- **Persistence** is delegated to the service's existing on-disk struct + `nexus-formats`
  `load_toml`/`save_toml` (`crates/nexus-formats/src/config/mod.rs:107-145`). The shell does
  **not** write `app.toml` for backend settings — file-as-truth is preserved per service.
- **Live-apply** is per-service and advertised via `restartRequired` plus a finer per-field
  `x-restart` hint where a struct mixes hot and cold fields (e.g. notifications: routing rules
  are hot, transport credentials are cold):

  | Service | After `config_set` |
  |---------|--------------------|
  | `nexus-ai` | **Live** — swaps `Arc<RwLock<AiConfig>>`; next call sees it. (But see §11 "hard": persistence currently lives in the TS shell; this spec moves the file write into the handler.) |
  | `nexus-notifications` | **Live** for routing (`reload_config_from_disk` already swaps the `Arc`); **restart** for transport creds (flag `x-restart`). |
  | `nexus-mcp / lsp / dap` | File is load-once at `on_init`; connections are lazy. `config_set` updates the in-memory `Arc<RwLock<…>>` so **new** connections pick it up; existing sessions unaffected. Mark `restartRequired:false` with a per-section note. |
  | `nexus-security` (sandbox) | **Restart required** — loaded once, injected at bootstrap. `config_set` writes the file + returns `restartRequired:true`. |

## 9. Shell rendering

- **Surface:** a single custom settings tab registered via
  `api.settings.registerTab('backend.services', BackendServicesTab, { group: 'options' })`,
  rendered through the existing `ContributedTabBody` path (no change to the shared
  `ConfigSchema` union or the `app.toml` write path).
- **Form generation:** a small JSON-Schema→control mapper (Draft 2020-12 subset):

  | Schema | Control |
  |--------|---------|
  | `type: boolean` | checkbox |
  | `type: integer/number` (+ `minimum`/`maximum`) | number input with bounds |
  | `type: string` | text input (`enum` → `<select>`; `x-secret` → masked password) |
  | `type: array` | add/remove list editor (items recurse) |
  | `type: object` / `$ref → $defs` | nested fieldset (recurse) |
  | `format: uri/uint32/…` | input type + client-side validation |

  This mapper lives entirely in the custom tab, so it can support the nested/array/range
  shapes the 6-type declarative renderer cannot.
- **UX:** per-service section; load via `config_get`; local dirty state; **Apply** sends a
  `config_set` patch (only changed leaves); validation errors from the handler surface inline;
  a `restartRequired`/`x-restart` field shows a "restart to take effect" banner. Optionally a
  read-only "raw TOML" disclosure for power users.
- **Discovery:** the tab probes the allow-list with `config_schema`; missing handlers ⇒ the
  service is hidden (graceful degradation during phased rollout).

## 10. Service-by-service rollout

From the backend survey:

| Service | Lift | Notes |
|---------|------|-------|
| **notifications** | **Easy** | Full serde + `Default` + live file-watch reload already; add `JsonSchema` + the trio; `x-restart` on transport creds. |
| **security / sandbox** | **Easy** | `SandboxConfig` is full serde + `Default`; add `JsonSchema`, a `config_set` (currently get-only `sandbox_policy`), `restartRequired:true`. |
| **storage (by-kind)** | **Easy** | Already has `config_read`/`config_reset` by `kind`; generalise into the trio (add `config_write`) for pure file-only configs that lack a hot-apply owner. |
| **mcp / lsp / dap** | **Medium** | serde present but partial — `lsp/dap` spec entries are `Deserialize`-only (need `Serialize`+`JsonSchema`); add whole-config get/set beside the existing per-entry `register_*` verbs. |
| **ai** | **Hard** | Two divergent `AiConfig` structs (env-sourced runtime `nexus-ai::config.rs:9` with no serde, vs on-disk `nexus-formats::config::ai.rs:26`); persistence currently owned by the TS shell. Requires reconciling the two and moving the file write into the handler before the `aiSettings` mirror can be retired. |
| **kernel** | **Out of scope (v1)** | Not a plugin, no IPC, `.nexus/config.toml`, restart-only. If surfaced later, do it read-mostly via a storage-style by-kind proxy with `restartRequired:true`. |

## 11. Phased implementation plan

- **Phase 0 — Contract + helper + pilot.** Add the `ConfigExposing` trait and generic
  `handle_config_{schema,get,set}::<C>` to `nexus-plugins`. Add schemars emit + drift coverage
  for config structs. Land **one pilot service end-to-end: notifications** (easiest, already
  has reload). Ship the `BackendServicesTab` renderer with the JSON-Schema mapper. Add
  cap-matrix rows (`notifications.config.write`, etc.).
- **Phase 1 — Easy.** sandbox (+restart flag); generalise storage by-kind for file-only
  configs.
- **Phase 2 — Medium.** mcp, lsp, dap (add Serialize+JsonSchema, whole-config get/set).
- **Phase 3 — Hard / special.** Reconcile the two `AiConfig` structs; move ai persistence into
  the handler; then **retire the `aiSettings` shadow-mirror** (Phase 4 of the original audit's
  intent) so ai is configured through the same path as everyone else.
- **Phase 4 — Docs + polish.** Update the audit doc's Rec #1 to "implemented", refresh the
  integration matrix, remove promoted rows from `hardcoded-rust.md`.

Each phase is independently shippable and visible in the UI as more services light up.

## 12. Capability model

Add to `cap_matrix.toml`, per participating service:

```toml
[[handler]]
plugin  = "com.nexus.<svc>"
command = "config_schema"   # unrestricted — schema only
access  = "unrestricted"

[[handler]]
plugin  = "com.nexus.<svc>"
command = "config_get"      # unrestricted — secret-free snapshot
access  = "unrestricted"

[[handler]]
plugin  = "com.nexus.<svc>"
command = "config_set"
caps    = ["<svc>.config.write"]
```

The `cap_matrix_complete` test enforces a row per handler, so the trio is covered by existing
CI. Handler bodies do **not** self-check caps (consistent with the codebase).

## 13. Testing

- **Schema emit / drift:** `ipc_schema_emit.rs` emits each `Config`'s schema; CI drift-check
  fails on un-regenerated changes.
- **Round-trip:** per service, `config_get` → mutate → `config_set` → `config_get` asserts the
  change persisted and secrets survived a redacted round-trip.
- **Persistence:** `config_set` writes the expected TOML (golden-file per service).
- **Cap matrix:** completeness test (existing) + a negative test that `config_set` without the
  cap is denied at the boundary.
- **Shell:** unit-test the JSON-Schema→control mapper against the committed
  `schemas/ipc/<svc>_config.json` fixtures so the renderer and the backend schema can't drift.

## 14. Risks & open questions

- **Two `AiConfig` structs.** The runtime/env struct and the on-disk struct are unbridged in
  Rust; the shell owns the file today. Reconciling them is the main hard lift; until then ai
  stays on the legacy mirror and is *not* part of the generic tab.
- **schemars ergonomics for maps/enums.** `HashMap`-shaped config (e.g. `servers` keyed by id)
  renders as `additionalProperties`; the mapper needs a key-add affordance. Tagged enums
  (adapter kinds) render as `oneOf` — the mapper must handle a discriminator selector.
- **Restart-required honesty.** The UI must not imply a cold setting is live; the per-field
  `x-restart` hint is load-bearing.
- **Discovery without a registry.** v1 uses a static allow-list; a kernel-level service
  registry would be cleaner but is a larger change.
- **Secret redaction correctness.** Globs in `secretFields` must match the actual JSON paths,
  including inside arrays/maps — covered by the round-trip test.

## 15. Effort estimate

- Phase 0 (trait + helper + drift + notifications pilot + renderer): ~the bulk of the work;
  most subsequent services are a derive + trait impl + 3 handler ids + 3 cap rows.
- Phases 1–2 (sandbox, storage, mcp/lsp/dap): small per service once Phase 0 lands.
- Phase 3 (ai reconciliation): a focused project of its own; sequence last.

## 16. Doc updates on landing

- `settings-integration-2026-06-19.md` §5 Rec #1 → link here / mark in-progress→done.
- `README.md` (settings index) → add a "backend config discovery" row.
- `architecture.md` → note the config-discovery IPC convention in the IPC tier.
- `forge-config.md` → cross-reference (the per-service TOML files remain the source of truth).
- `hardcoded-rust.md` → remove rows as values are promoted to surfaced config.
