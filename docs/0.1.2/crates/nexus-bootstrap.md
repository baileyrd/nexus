# nexus-bootstrap

> Kind: lib · IPC plugin id: — (registers all CorePlugins) · CorePlugin: no · Has settings: cap_matrix.toml · As of: 2026-05-25

## Overview

`nexus-bootstrap` is the central assembly point of the workspace and the **sole crate permitted to depend on every service crate** (microkernel invariant 2). The kernel never links a subsystem; the frontends (CLI, TUI, MCP, shell) never link a subsystem directly either. Bootstrap is the one place where all 23 in-tree `CorePlugin`s, the kernel, the plugin loader, the KV store, the audit store, and the invoker identity are wired together into a single runnable `Runtime`. Everything downstream reaches storage / AI / editor / git / terminal / etc. through `runtime.context.ipc_call(plugin_id, command, args)` — bootstrap is what makes those handlers reachable in the first place.

The public entry points are `build_cli_runtime(forge_root)` and `build_tui_runtime(forge_root)`, both thin wrappers over a private `build(forge_root, invoker_id, invoker_name)`. `build` loads `KernelConfig`, creates `.forge/`, installs the SQLite audit store and the kernel-metrics registry, opens the `nexus-kv` SQLite KV store and injects it into `Kernel::new`, constructs a `PluginLoader` (with search paths, lifecycle timeout, signature-requirement, and a WASM-caps ceiling from config), registers every core plugin via `plugins::register_all`, registers the invoker (CLI/TUI) as a `Core`-trust plugin holding `Capability::ALL`, wraps the loader in a `SharedPluginLoader`, applies the capability matrix, then wires per-plugin `KernelPluginContext`s for the plugins that need to issue nested `ipc_call`s (ai, agent, editor, workflow, audio, ai-runtime). The result is a `Runtime { kernel, context, loader }` where `context` is the invoker's full-capability plugin-facing surface and `loader` must be kept alive for the runtime's lifetime (the context holds an `Arc<dyn IpcDispatcher>` pointing at it).

Plugin registration order is **deterministic and load-bearing**: security first (so audit events route through it before any other plugin emits), storage second (so AI/editor/etc. can `ipc_call` into it during their own lifecycle hooks), then ai-runtime before ai (the runtime publishes its shared worker-pool handle so ai's indexing daemon can reuse it), through to collab last (so every preceding plugin's events are available to the relay bridge). The full sequence lives in `plugins::register_all` and is pinned by two tests (`registration_order.rs`, `manifest_deps_match_boot_order.rs`).

The capability matrix (`cap_matrix.toml`, embedded via `include_str!`) is the single declarative source of truth for which capability each in-tree IPC handler demands of its caller. It replaced a hand-maintained wall of `add_cap_requirement(...)` calls (issue #77, ADRs 0022/0028, BL-117/136/138). It is parsed, fully validated (fail-fast), and applied to the loader at boot, before the first `ipc_call` arrives.

Three cross-cutting responsibilities live here precisely because they would create dependency cycles or violate kernel backend-agnosticism if they lived anywhere else: `SqliteAuditStore` (the kernel exposes the `AuditStore` trait but must not link `rusqlite`), `CrdtPublisher` (glue between `nexus-editor` and `nexus-crdt`, where `nexus-crdt` already depends on `nexus-editor`, so the publisher can't live in either without a cycle), and the IPC JSON-Schema / TS-binding emission pipeline (the only crate that can `use` every subsystem's wire types at once). Bootstrap also owns the `IpcInvoker` abstraction (local vs. remote/SSH), the remote-forge proxy factory, the reconnecting runtime, the Dream-Cycle scheduler, the protocol-host contribution wiring (LSP/DAP/MCP/ACP), and the forge-scaffold templates.

## Position in the dependency graph

- **Direct `nexus-*` dependencies:** *every* service crate — `nexus-formats`, `nexus-kernel`, `nexus-kv`, `nexus-plugins`, `nexus-security`, `nexus-storage`, `nexus-database`, `nexus-ai`, `nexus-ai-runtime`, `nexus-editor`, `nexus-crdt`, `nexus-terminal`, `nexus-theme`, `nexus-git`, `nexus-mcp`, `nexus-lsp`, `nexus-dap`, `nexus-acp`, `nexus-remote`, `nexus-types`, `nexus-agent`, `nexus-skills`, `nexus-templates`, `nexus-workflow`, `nexus-linkpreview`, `nexus-notifications`, `nexus-comments`, `nexus-audio`, `nexus-collab`. This is by design and is the *only* crate allowed to do so (invariant 2); the `dep_invariants.rs` test that forbids tight coupling elsewhere makes this crate the sanctioned exception by routing everything else through it.
- **Notable external dependencies (+ why):** `tokio` (async lifecycle hooks, `ipc_call` await, background schedulers/subscribers), `anyhow` + `thiserror` (boot-error context chains and typed `IpcInvokerError`/`RemoteRuntimeError`), `tracing` (boot diagnostics, degraded-plugin warnings), `rusqlite` (`SqliteAuditStore`), `toml` (config + cap-matrix parsing), `chrono` (audit timestamps/retention), `serde`/`serde_json`/`async-trait`. Dev-only: `tempfile` (scratch forges), `zip` (Notion-import fixtures), and `schemars` (JSON-Schema emission, only under the `ts-export` feature).
- **Crates that depend on this one:** `nexus-cli` and `nexus-tui` (the two in-tree binaries that call `build_cli_runtime`/`build_tui_runtime`). The shell's Tauri bridge consumes it too (outside the Cargo workspace).

## Public API surface

**`lib.rs` — runtime builders + entry surface**
- `Runtime { kernel: Kernel, context: KernelPluginContext, loader: Arc<SharedPluginLoader> }` — the assembled runtime; `Runtime::invoker()` returns an `Arc<dyn IpcInvoker>` backed by a `LocalIpcInvoker`.
- `build_cli_runtime(PathBuf) -> Result<Runtime>` / `build_tui_runtime(PathBuf) -> Result<Runtime>` — assemble kernel + all core plugins + invoker.
- `init_forge(&Path) -> Result<()>` — the one storage op the CLI can't do via `ipc_call` (the forge must exist before the storage plugin's `on_init` can open it); wraps `StorageEngine::init` so `nexus-cli` needn't link `nexus-storage`.
- `CLI_PLUGIN_ID` / `TUI_PLUGIN_ID` constants (`com.nexus.cli` / `com.nexus.tui`).
- `all_caps()`, `agent_capabilities()`, `workflow_capabilities()`, `audio_capabilities()`, `ai_runtime_capabilities()` — the narrowed `CapabilitySet`s wired into the per-plugin contexts (issue #73; only ai/editor/invoker hold the full `Capability::ALL` set).
- Re-exports from `nexus-formats` so CLI/TUI canvas/HTML commands avoid a direct storage dep: `export_to_html`, `CanvasFile`/`CanvasNode`/`CanvasEdge`/…, `parse_canvas`, `serialize_canvas`.
- Private config loaders: `load_digest_config`, `load_notifications_config` (+ `load_discord_webhook_url`/`load_telegram_config`/`load_smtp_config`), `load_webhook_config` — each reads a block from `<forge>/.forge/config.toml` and degrades to defaults on missing/malformed input.

**`plugins/` — per-plugin registration (one module per service)**
- `register_all(loader, forge_root, event_bus)` — the orchestrator; registers all 23 core plugins in deterministic order (see below).
- `core_manifest` / `core_manifest_with_ipc` / `core_manifest_with_ipc_and_deps` — synthesize a core-plugin `PluginManifest` inline (trust_level=core, api_version=1, IPC command→handler-id table, optional `[dependencies]`).
- `with_v1_aliases(&[(cmd, id)])` — doubles each command into a bare + `.v1` alias (ADR 0021 handler versioning).
- `LifecycleFlags` (`on_init`/`on_start`/`on_stop`, plus `NONE`).
- `RegisterCoreResultExt` trait: `or_lifecycle_skip` (BL-095: a lifecycle-hook *timeout* degrades to "skip + publish `com.nexus.kernel.plugin_lifecycle_timeout`" rather than aborting boot) vs. `or_critical` (storage/security: any failure aborts boot). Non-timeout errors always propagate.

**`cap_matrix.rs`** — `apply(&SharedPluginLoader)`: parse + validate + apply the embedded `cap_matrix.toml`; auto-mirrors each row onto every `.v<N>` version alias; supports `caps`, `unrestricted`, `policy`, and `internal` row shapes.

**`cap_policies.rs`** — named args-aware capability closures referenceable from the matrix: `resolve(name)`, `is_registered(name)`, and the one current policy `ai_tools_policy` (ADR 0022 Phase 2 — `tools=auto` ⇒ `ai.tools.write`, `auto_with_mcp` ⇒ `ai.tools.mcp`).

**`audit_sqlite.rs`** — `SqliteAuditStore` (impl `nexus_kernel::audit_store::AuditStore`) with `open(path)`; `init(path)` opens and installs it as the kernel's global audit store.

**`crdt_publisher.rs`** — `CrdtPublisher` (impl `nexus_editor::OpObserver`); `ReloadOutcome`/`ReloadSkip`; `start_pull_landing_subscriber()`; `reload_after_external_change(relpath)`; `DEFAULT_CHECKPOINT_EVERY_OPS`.

**`invoker.rs`** — `IpcInvoker` trait (transport-agnostic `ipc_call`), `LocalIpcInvoker`, `IpcInvokerError`.

**`remote.rs` / `reconnect.rs`** — `RemoteRuntime` + `build_remote_runtime_ssh`/`build_remote_runtime_over_pipes` (SSH/`ssh://` proxy, BL-140); `ReconnectingRuntime` + `ConnectionFactory`/`SshConnectionFactory` (reconnect-with-backoff, subscription replay, BL-146).

**`storage.rs` / `terminal.rs` / `database.rs`** — typed `async` IPC-client helper modules: serialize args → `IpcInvoker::ipc_call` → typed DTO, so CLI/TUI need no direct subsystem dep (storage helpers are the largest set; terminal and database are thinner).

**`agent.rs`** — `KernelToolDispatcher` + `AiChatDriver`: production impls of `nexus-agent`'s two kernel-free boundary traits, bridging them to a `KernelPluginContext`.

**`protocol_host_specs.rs`** — pure converters from manifest-side `ContributedAdapter<…>` to host-side `{Lsp,Dap,Mcp,Acp}` spec shapes (ADR 0027). The only place that mapping lives.

**`{dap,lsp,mcp,acp}_contribution_wiring.rs`** — `wire_*_contributions` / `unwire_*_contributions_for_plugin`: issue `register_*` / `unregister_*` IPC calls for each protocol contribution found across community manifests at plugin load/disable (gated by `protocol.host.contribute`).

**`collab.rs`** — `CollabConfig` + `start_if_enabled(forge_root, bus)`: opt-in WebSocket relay bridge for `com.nexus.editor.ops.*` (BL-143).

**`dream_cycle.rs`** — `DreamCycleScheduler` + `spawn(runtime, forge_root)`: cron-driven graph-maintenance phases (`dedup`, `decay`, `enrich`, `infer`) for long-running invokers (BL-129). C46 (#421) — every long-running invoker now calls `spawn`: the TUI (`crates/nexus-tui/src/app.rs::TuiApp::new`), the Tauri shell (`shell/src-tauri/src/bridge.rs::KernelRuntime::boot_at`, `Local` runtime only — a `Remote` boot's kernel lives on the far end of the SSH connection and spawns its own scheduler there), and `nexus mcp serve` (`crates/nexus-cli/src/commands/mcp.rs::serve`). One-shot CLI invocations don't spawn it — `nexus graph dream-cycle run` already covers on-demand execution of the four phases.

**`forge_template.rs`** — `ForgeTemplate` enum + `apply(root, template)`: idempotent forge scaffolding shared by CLI `forge init --template` and the shell's `init_forge` (BL-054).

## Plugin registration order

`register_all` registers exactly 23 core plugins in this order (pinned by `manifest_deps_match_boot_order.rs`'s `BOOT_ORDER` and `registration_order.rs`):

1. `com.nexus.security` — first, so audit events route through it before any other plugin emits.
2. `com.nexus.storage` — second, so AI/editor/etc. can `ipc_call` into it during their lifecycle hooks.
3. `com.nexus.database`
4. `com.nexus.editor`
5. `com.nexus.theme`
6. `com.nexus.ai.runtime` — **before** ai, so the runtime's shared worker-pool handle is published before ai's indexing daemon starts (it reuses the handle instead of spinning its own tokio runtime; `None` fallback preserves boot if the runtime plugin is absent).
7. `com.nexus.ai`
8. `com.nexus.skills`
9. `com.nexus.templates`
10. `com.nexus.formats`
11. `com.nexus.workflow`
12. `com.nexus.linkpreview`
13. `com.nexus.notifications`
14. `com.nexus.audio`
15. `com.nexus.comments`
16. `com.nexus.agent`
17. `com.nexus.mcp.host`
18. `com.nexus.lsp`
19. `com.nexus.dap`
20. `com.nexus.acp`
21. `com.nexus.git`
22. `com.nexus.terminal`
23. `com.nexus.collab` — last, so every preceding plugin's events are reachable by the relay bridge spawned later in boot.

The invoker plugin (`com.nexus.cli`/`com.nexus.tui`) is registered *after* all 23 (pinned by `registration_order.rs`). Each plugin's manifest may declare `MANIFEST_DEPS` (reverse-DNS ids of plugins that must load earlier); the loader's `check_dependencies` enforces this at boot against the order above, and `manifest_deps_match_boot_order.rs` catches drift at test time.

## Capability matrix (cap_matrix.toml)

`cap_matrix.toml` is the declarative single source of truth for per-handler caller-capability requirements. Every in-tree IPC handler MUST appear as a `[[handler]]` row keyed by `(plugin, command)`, classified as exactly one of:

- `caps = ["..."]` — the caller must hold every listed capability *on top of* the unconditional `ipc.call` check (e.g. `com.nexus.terminal::create_session` → `process.spawn`; `com.nexus.ai::stream_chat` → `ai.chat`; `com.nexus.git::push` → `net.http`; `com.nexus.security::set_secret` → `security.write`).
- `unrestricted = "<rationale>"` — intentionally reachable by any caller holding `ipc.call`; the string documents *why* no extra cap is needed (read-only probe, forge-local mutation gated downstream by storage's `fs.write`, etc.).

Optional fields: `note` (free-form audit text, ignored by the loader), `policy` (a named args-aware closure from `cap_policies.rs`, stacks on top of `caps`), and `internal = true` (P1-02 — the kernel rejects any caller whose `caller_trust_level != Core` regardless of caps; e.g. `com.nexus.ai::resolve_credentials`).

The matrix is embedded at compile time (`include_str!`) and applied by `cap_matrix::apply` in two passes: a **validation pass** that surfaces every authoring error before any loader mutation (malformed TOML, a row with both/neither `caps`/`unrestricted`, an unknown capability string, an unknown policy name, a duplicate `(plugin, command)` row, or a `policy` on an `unrestricted` row all abort boot), then an **apply pass** that mirrors each classification onto the bare command *and* every `<cmd>.v<N>` alias discovered in the live registry, calling `register_handler_caps` / `register_handler_unrestricted` / `add_cap_requirement_fn` / `register_handler_internal_only`. Rows flagged `# AUDIT:` (e.g. `com.nexus.workflow::run`, `com.nexus.mcp.host::call_tool`) preserve current behaviour but are candidates for a future cap-elevation review — the issue-#77 "IpcCall-laundering" surface (workflow/agent reaching high-impact handlers transitively) is tracked there and audit-logged via `nexus_workflow::implied_caps`.

**Completeness invariant:** `cap_matrix_complete.rs` boots a full runtime, enumerates every `(plugin, command)` in the live registry, and fails if any handler lacks a classification — the intended failure mode being "you added a handler without a matrix row." (BL-138 Phase 2 classified every remaining handler, so this test now runs unconditionally rather than `#[ignore]`d.)

## IPC handlers

`nexus-bootstrap` registers **none of its own** — it has no plugin id and never receives `ipc_call`s. The placeholder `InvokerPlugin` (the CLI/TUI identity) declares zero IPC commands and returns `ExecutionFailed` if `dispatch` is ever called, because invokers only *originate* calls. Bootstrap's job is to register every *other* crate's handlers and gate them via the cap matrix.

The one event-publishing surface bootstrap owns is `CrdtPublisher`'s op-envelope publishing: it implements `nexus_editor::OpObserver` and publishes per-op envelopes on `com.nexus.editor.ops.<relpath>` (and conflict envelopes on the conflict topic) under the `com.nexus.editor` plugin namespace — see Events.

## Settings / Config

- **`cap_matrix.toml`** (in the crate root, embedded at compile time) — the only "settings" file bootstrap owns; see the section above.
- **`KernelConfig`** — loaded from the forge by `KernelConfig::load(forge_root)`; supplies `plugin_search_paths`, `lifecycle_timeout_secs`, `require_signatures`, and the `wasm_caps` ceiling.
- **`<forge>/.forge/config.toml`** — bootstrap reads several optional blocks at boot, all degrading to defaults on missing/malformed input: `[digests]` (`nexus_workflow::DigestConfig`), `[webhooks]` (`WebhookConfig`), `[notifications.{discord,telegram,email}]` (legacy channel credentials), and `[collab]` (`CollabConfig`). The unified `<forge>/.forge/notifications.toml` (`NOTIFICATIONS_CONFIG_RELPATH`) takes precedence over the legacy blocks when present.
- **`<forge>/.forge/kv.sqlite3`** — opened via `nexus_kv::SqliteKvStore` and injected into `Kernel::new` (bootstrap creates `.forge/` first; the kernel is backend-agnostic).
- **`<forge>/.forge/.kernel/audit.db`** — opened/installed by `SqliteAuditStore` before any plugin loads; failure is logged but non-fatal (audit falls back to trace-only).

## Events

- **CrdtPublisher op envelopes** — on each editor `apply_transaction`/`undo`/`redo`, the publisher feeds ops through `CrdtDoc::apply_local` and publishes an `OpEnvelope` on `com.nexus.editor.ops.<relpath>` (payload `{ "op": <CrdtOp> }`), emitted under the `EDITOR_PLUGIN_ID` namespace. Conflicts surface as `ConflictEnvelope`s on `nexus_crdt::conflict_topic(relpath)` (`com.nexus.editor.crdt.conflict.<relpath>`).
- **Pull-landing subscription (BL-007)** — `start_pull_landing_subscriber` spawns a background thread subscribed to `com.nexus.git.commit` (fired by `nexus-git`'s state poller when HEAD advances, covering the merge/fast-forward end of a `git pull`). On each event it re-reads every open session's persisted CRDT state, absorbs unseen remote ops via `apply_remote`, and republishes them on the ops topic. The thread holds a `Weak` to the publisher's inner state, so dropping the last `CrdtPublisher` (editor `on_stop`) exits it cleanly with no shutdown signal.
- **Plugin-lifecycle-timeout events** — `or_lifecycle_skip` publishes `com.nexus.kernel.plugin_lifecycle_timeout` (anchored to the synthetic `com.nexus.kernel` namespace) when a non-critical plugin's hook times out, so the shell can surface a "<plugin> failed to start" notice.
- **Collab bridge** — `collab::start_if_enabled` bridges `com.nexus.editor.ops.*` bus events to a remote WebSocket relay and republishes inbound envelopes (opt-in via `[collab]`).

## Internals & notable implementation details

- **Runtime assembly sequence** (`build`): load `KernelConfig` → create `.forge/` → install `SqliteAuditStore` (non-fatal) → open KV store → install `KernelMetrics` (before any lifecycle hook fires) → `Kernel::new` → build `PluginLoader` (search paths, lifecycle timeout, signature requirement, WASM-caps ceiling) → `register_all` → register the invoker as a `Core`-trust plugin with `Capability::ALL` → wrap in `SharedPluginLoader` → `cap_matrix::apply` (before first dispatch) → wire per-plugin contexts (ai, agent, editor, workflow, audio, ai-runtime) → `collab::start_if_enabled` → build the invoker context → return `Runtime`.
- **Per-plugin contexts:** ai/editor/invoker get full `all_caps()`; agent/workflow/audio/ai-runtime get narrowed sets (issue #73) so an LLM-generated plan or user-authored workflow can't exercise `NetHttp`/`ProcessSpawn`/`FsWriteExternal` directly — those only reach through `ipc_call` into the gated target handler. Each context is built via `KernelPluginContext::new(..., Some(dispatcher)).with_trust_level(Core)` and wired with `shared.wire_context(id, ctx)`.
- **Loader retained on the `Runtime`:** the `loader: Arc<SharedPluginLoader>` field is deliberately public (issue #83) because the shell's bridge drains it during `shutdown_kernel` (every `on_stop` must fire) and integration tests reach for it as `Arc<dyn IpcDispatcher>`; everything else routes through `context`.
- **`SqliteAuditStore` schema:** one `audit_events` table (`id, ts_ms, event_type, plugin_id, detail_json`) with indexes on ts/type/plugin; `open` prunes rows older than `AUDIT_LOG_RETENTION_DAYS`; `query` filters by type/plugin/since with a default 1000-row limit; mutex-poison and SQL errors degrade to trace-only rather than panicking.
- **`CrdtPublisher` wiring:** the editor plugin's `register` constructs the publisher, calls `start_pull_landing_subscriber`, and `plugin.set_op_observer(publisher)`. Per-session state (`CrdtDoc` + checkpoint counter + open-time version-vector prune floor) lives behind a `Mutex<HashMap>`. Persistence is atomic (temp + rename) to `.forge/.editor/crdt/<sha-of-relpath>.json`, written on close and every `DEFAULT_CHECKPOINT_EVERY_OPS` (32) ops; on close the op log is compacted to the open-time VV.
- **Capability-matrix application** auto-mirrors onto `.v<N>` aliases and is fail-fast; see the cap-matrix section.
- **TS-export / schema-emit pipeline:** the `ts-export` feature (off by default, fans out to every subsystem's matching `ts-export` feature) enables `tests/ipc_schema_emit.rs`, which `use`s wire types from ~22 subsystem crates and writes per-handler JSON Schemas into `crates/nexus-bootstrap/schemas/ipc/`. The companion `scripts/check_ipc_drift.sh` regenerates these (plus the TS bindings via ts-rs) and fails on any git diff. The schema test additionally asserts every object schema declares `additionalProperties: false` (the P0-2 gate locking in workspace-wide `#[serde(deny_unknown_fields)]`).

## Tests

`crates/nexus-bootstrap/tests/` is the de-facto integration-test home for the whole workspace — it's the only crate that can boot a full runtime, so the per-service IPC suites and the architecture-invariant guards both live here.

**Architecture-invariant guards**
- `dep_invariants.rs` — the microkernel-isolation enforcer. A `FORBIDDEN` table of `(consumer crate, dep)` pairs asserts IPC consumers don't link subsystem engines directly: CLI/TUI/AI/MCP/database must not link `nexus-storage`; CLI/TUI must not link `nexus-database`; `nexus-database` must not link `rusqlite` (SQL goes through storage); MCP/ACP/remote proxies must not link `nexus-ai`/`nexus-agent`/`nexus-storage`/`nexus-terminal`/`nexus-editor`/`nexus-git`/`nexus-database`; `nexus-kernel` must not link `rusqlite`/`nexus-kv`; CLI/TUI/MCP must not link `nexus-ai-runtime`. Also walks `[target.'cfg(...)'.dependencies]` (issue #83) with a self-test proving the cfg traversal catches a synthesised violation. Bootstrap is the sanctioned sole linker of every subsystem.
- `dep_invariants_shell.rs` — the same posture extended to the out-of-workspace Tauri shell manifest (`shell/src-tauri/Cargo.toml`): a `FORBIDDEN_FOR_SHELL` list of 25 subsystem crates, with `nexus-remote` the one intentional exception (BL-140 `ssh://` proxy).
- `plugin_contract_purity.rs` — community-tier crates and `nexus-plugin-api` must pin only the stable contract, never engine internals (Cargo check + literal-grep for `pub use nexus_<impl>::` re-exports + a self-test).
- `core_plugin_loc_budget.rs` (SD-07) — keeps each service crate's `core_plugin.rs` under a LOC budget; editor/terminal are grandfathered above the default and can only shrink.
- `ipc_topic_prefix_invariant.rs` (BL-137) — static scan that every `.publish(...)` in a core plugin (or bootstrap helper) emits a topic in its own namespace or a kernel-owned shared topic.
- `registration_order.rs` (#83) — asserts security registers before every other plugin, storage before its IPC consumers, and the invoker after all core plugins.
- `manifest_deps_match_boot_order.rs` — checks every plugin's `MANIFEST_DEPS` is satisfied by the `BOOT_ORDER` (23 entries) and that the count matches `register_all`.

**Capability / security guards**
- `cap_matrix_complete.rs` — every IPC handler is classified in the matrix; the matrix applies ≥1 classification; the 17+ historical `add_cap_requirement` entries survive the migration.
- `process_spawn_gate.rs` (#77) — `terminal::create_session` / `mcp.host::connect` are denied without `process.spawn`; unrelated targets pass with only `ipc.call`.
- `amplifier_caps.rs` (#73) — agent/workflow contexts hold only their documented narrow caps, never `Capability::ALL`.
- `ai_capability_gate.rs` (ADR 0022) — per-handler AI caps; read-only AI handlers ungated; the args-aware tool policy demands the right caps.
- `protocol_host_contribute_cap.rs` (BL-113) — the four `register/unregister` protocol-host verbs reject a community caller (only invoker/Core trust admitted).
- `event_bus_anti_spoofing.rs` (ADR 0007) — end-to-end: plugin publishes materialise as `Custom`, foreign/substring-prefix namespaces are rejected, the kernel sets `emitting_plugin`.
- `ipc_strictness.rs` (P0-1) — an unknown field on an IPC payload is rejected end-to-end through the dispatcher.
- `ipc_versioning.rs` (ADR 0021) — bare and `.v1` aliases return identical results; the deprecation-window guard logic is correct; unknown version suffixes are `command_not_found`.
- `ipc_schema_emit.rs` (P0-2 / WI-36, `ts-export` only) — emits per-handler JSON Schemas and asserts `additionalProperties: false` everywhere.

**Per-service IPC integration suites** (boot a real runtime, drive `context.ipc_call`): `forge_ipc.rs` (storage forge-tree handlers), `agent_ipc.rs`, `ai_runtime_ipc.rs`, `database_ipc.rs`, `editor_ipc.rs`, `editor_excerpts_ipc.rs` (BL-141), `formats_ipc.rs`, `notifications_inbox_ipc.rs` (BL-136), `skills_ipc.rs`, `templates_ipc.rs`, `terminal_repl_ipc.rs` (BL-142), `theme_ipc.rs`, `workflow_ipc.rs`, plus `community_to_core_ipc.rs` (a community dispatcher falling through to core plugins via `CompositeIpcDispatcher`) and `phase1_smoke.rs` (boot → list_dir → bus event → clean shutdown).

**Protocol-host + remote/CRDT suites:** `{acp,dap,lsp,mcp}_contribution_wiring.rs` (wire/unwire contributions against a booted runtime), `acp_server.rs` (BL-145 inbound JSON-RPC proxy over a duplex pipe), `crdt_publisher_e2e.rs`, `remote_runtime_loop.rs` (BL-140 `RemoteRuntime` over duplex), `reconnect_loop.rs` (BL-140 reconnect-after-drop), `subscription_replay.rs` (BL-146 subscription replay across transport drops), and `build_runtime.rs` (the baseline "runtime builds + storage round-trips + unknown plugin/command errors" smoke test).
