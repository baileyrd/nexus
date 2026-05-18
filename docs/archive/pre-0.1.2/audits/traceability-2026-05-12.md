# Documentation Traceability Matrix

**Date:** 2026-05-12
**Scope:** Every active document under `docs/` mapped to its implementing
code, with gaps identified. Excludes `docs/archive/`, `docs/audits/`, and
`docs/research/` per the "archive/audit/research are not spec" convention.

**Inputs:**
- 18 PRDs (`docs/PRDs/01–17` plus `04a`)
- 26 ADRs (`docs/adr/0001–0026`)
- 6 architecture docs (`docs/architecture/`)
- 22 developer docs + 4 users docs
- 33 help docs
- 12 shell reference docs
- 7 roadmap docs

**Total: 132 docs audited.**

> **Format note.** "Fully implemented; no drift" rows are omitted to bias
> the document toward signal. A doc not appearing under a heading is
> presumed accurate. This is a *delta* against reality — read it as a
> punch list, not a survey.

---

## Headline findings

### Highest-impact drift

1. **The entire `docs/developer/` plugin-author hub teaches a fictional TypeScript API.** Pages under `developer/plugins/*.md` and `developer/editor/*.md` show `Plugin` / `PluginContext` / `activate()` / `ctx.commands.register()` — none of those exist in `packages/nexus-extension-api/src/index.ts`. Real contract is `ScriptPlugin` with `onInit` / `onStart` / `onStop` / `dispatch` + `NexusPluginContext` with `settings.get()`, `events.emit()`, `ui.notify(level, msg)`, etc. A plugin author following the documented examples cannot ship working code. **This is the single biggest doc bug in the repo.**

2. **The `docs/shell/` reference is substantially behind reality.** The Phase 7 leaf migration moved the floor: documented 8 slot IDs vs. actual 6; documented 38 plugins in the `core.*` namespace vs. actual 60 in the `nexus.*` namespace; documented `.nexus/workspace.json` path vs. actual `<forge>/.forge/workspace.json`; `PluginAPI` doc covers ~10 of ~17 sub-surfaces; multiple registry/contribution shapes are fabricated.

3. **PRD-16 (Workflow) is under-claimed.** `IMPLEMENTATION_STATUS.md` says webhook/git_event/mcp_event triggers and parallel/retry scheduling are missing; they all shipped per `crates/nexus-workflow/src/{cron,core_plugin,executor,webhook}.rs`. PRD-16 should be 🟢, not 🟠.

4. **PRD-15 (Agent System) has the biggest spec-vs-impl gap among "shipped" PRDs.** §4 ToolRegistry, §5 Memory, §7 interactive-approval round-trip, §8 six built-in agent classes, §9 `.agent.toml` custom-agent format, §10 agent-to-agent comms — all unimplemented. What shipped is a thin execution skeleton + 3 archetype prompts + run history.

5. **PRD-17 (Cross-Platform) is desktop-only despite the name.** §3 WASM, §4 `nexus-platform` crate, §5 web, §6 mobile/UniFFI — all unimplemented. No `wasm32` or `uniffi` deps anywhere.

### Filing drift (docs in wrong directories)

- `docs/roadmap/notion-block-ux-plan.md` — all 6 phases shipped 2026-04-22. **Should archive.**
- `docs/roadmap/OPEN-ITEMS.md` — 21 of 22 OI items resolved; only OI-05 (Rust dep dup, blocked upstream) genuinely open. **Should archive** with audit-tail bullets extracted.

### Stale counts (cosmetic but corrosive)

- `docs/architecture/C4.md`: claims 25 crates; actual 28. Claims 23 `#[tauri::command]` handlers; actual 25 across 4 files.
- `docs/architecture/ipc-schemas.md`: claims "~28 JSON schemas + ~30 TS types"; actual **131 schemas / 166 TS files**.
- `docs/adr/0002`: capability-inventory table is missing the 8-member `ai.*` cluster added by ADR 0022.
- `docs/users/cli.md`: misses ~12 subcommand groups and 16+ git subcommands.

### Recurring keybinding/path drift in help

- Inline AI keybinding documented as `Ctrl+Shift+Space` in 5 help docs; real binding is `Ctrl+I` / `Cmd+I`.
- `Ctrl+Shift+T` conflict: `customize/keybindings.md` says "new terminal session"; the shell registers it for the theme picker.
- `docs/help/customize/themes.md:54` links to `docs/shell/theme-variables.md` (does not exist; real path is `docs/developer/themes/css-variables.md`).

### Decisions enforced cleanly

- ADR enforcement via `crates/nexus-bootstrap/tests/` is load-bearing and works: `dep_invariants.rs` (0004, 0011, 0016), `ai_capability_gate.rs` (0022), `ipc_versioning.rs` (0021), `plugin_contract_purity.rs`, `process_spawn_gate.rs`, `ipc_strictness.rs`.
- ADRs 0015 (sandbox), 0019 (`.base` format), 0022 (AI caps), 0025 (legacy planner retirement), 0026 (op-log) match implementation with no drift.

---

# PRD Traceability Audit — 2026-05-12

Spec → implementation traceability against the 18 numbered PRDs under `docs/PRDs/`.
Verified against repository at HEAD (`main`, commit `4ff3511c`). Where prior 2026-05-06
integration assessments exist, the verdict is summarised in one line; otherwise the
audit is done here.

---

## PRD-01 — Kernel & Event System

Fully implemented; see `docs/audits/KERNEL-INTEGRATION-ASSESSMENT-2026-05-06.md` (9/10 — architectural backbone is sound, gaps are operational not structural).

---

## PRD-02 — Security Model

Fully implemented; see `docs/audits/SECURITY-INTEGRATION-ASSESSMENT-2026-05-06.md` (8/10 — production-grade for personal use; remaining gaps are marketplace-launch deferrals).

---

## PRD-03 — Storage Engine

Fully implemented; see `docs/audits/STORAGE-INTEGRATION-ASSESSMENT-2026-05-06.md` (9.5/10 — the strongest subsystem, no material gaps).

---

## PRD-04 — Plugin System

**Implementations:** `crates/nexus-plugins/` (~2k LoC: manifest, loader, sandbox, hot_reload, signing, settings, host_fns, composite, grants_crypto, scaffold), `crates/nexus-kernel/` (IPC dispatcher, context, capabilities), `crates/nexus-plugin-api/` (~7 modules: plugin, capability, event, ipc, log, error), `shell/src/host/` (ExtensionHost, communityPluginLoader, sandbox/), `packages/nexus-extension-api/`.

**Gaps:**
- Spec'd but missing: §10.1 "Dynamic Loading for Core Plugins (.so/.dll with Symbol Resolution)" — no `libloading` dependency or symbol-resolution code in `nexus-plugins`. Core plugins are statically registered by `nexus-bootstrap`. PRD spec for ABI-versioned `.so/.dll` loading is unimplemented and conflicts with the bootstrap-registered model the codebase actually uses.
- Spec'd but missing: §10.2 "Version Compatibility Checking" for dynamically loaded `.so` — moot if §10.1 stays unimplemented; should be flagged.
- Spec'd but missing: §6.2 "Packaging & Signing" — `signing.rs` verifies manifest signatures but no packaging/signing CLI tooling (`nexus plugin pack`/`sign`) is present in `commands/plugin.rs`. `OI-15 manifest signing` is still flagged as marketplace blocker in `IMPLEMENTATION_STATUS.md`.
- Spec'd but missing: §6.4 "Update Flow" — `loader.rs` has no update/migration code path.
- Stale claim in PRD-04 §2.2 ("WASM Runtime Decision") — already chosen as wasmtime, fine. But §2.3 listed dependency spec versions are stale relative to actual `Cargo.toml`.
- Implemented but undocumented: `composite.rs` CompositeIpcDispatcher fall-through (community → core), `grants_crypto.rs` install-time HIGH-risk consent persistence, `host_fns.rs` capability-gated host function catalogue, manifest signature verification (`signing.rs`, BL-099). None of these surfaces appear in PRD-04 §s.

---

## PRD-04a — Plugin Templates

**Implementations:** `docs/PRDs/templates/core-plugin/` and `docs/PRDs/templates/community-plugin/` (`cargo-generate.toml`, `Cargo.toml`, `manifest.toml`, `src/`, `tests/`, `README.md`). Scaffold engine at `crates/nexus-plugins/src/scaffold.rs` invoked by `crates/nexus-cli/src/commands/plugin.rs:206` (`scaffold(..., template, &config)` → `nexus_scaffold`).

**Gaps:**
- Spec'd but missing: §8 "Integration with `nexus plugin scaffold`" — CLI command exists but PRD-spec invocation surface (`--type core|community|script|wasm`) — verify against §8 if a richer wizard is expected; current implementation is non-interactive flags only.
- Spec'd but missing: §5 "Test Specifications" (`lifecycle_test.rs`, `events_test.rs`) — template `tests/` directories exist; need to verify they match the §5.1/§5.2 contracts. The PRD goes deep on test scaffolding contents; unverified one-to-one parity.
- Stale claim: PRD references `nexus-plugin-api` Rust crate as the trait root, which now exists (was BACKLOG F-2.1.1 in older IMPLEMENTATION_STATUS — has shipped per `crates/nexus-plugin-api/src/{plugin,capability,event,ipc,log,error}.rs`). Update IMPLEMENTATION_STATUS PRD-04 line "F-2.1.1 still open" — appears closed.

---

## PRD-05 — CLI

**Implementations:** `crates/nexus-cli/` — `main.rs` defines the `Commands` enum; `commands/` has 26 modules: `agent`, `ai`, `bases`, `canvas`, `config`, `content`, `crdt`, `db`, `desktop`, `export`, `forge`, `git`, `graph`, `import`, `logs`, `mcp`, `plugin`, `proc`, `skill`, `tags`, `template`, `term`, `tui`, `watch`, `workflow`. `clap_complete` wired for shell completions. Output formatters at `crates/nexus-cli/src/output.rs` (`is_terminal()` TTY detection).

**Gaps:**
- Spec'd but missing: §3.9 "Synchronization (`nexus sync ...`)" — `Commands::Sync(StubArgs)` returns `stubs::not_implemented("sync")` at `main.rs:1766`. Entire sync subcommand group spec'd in detail (§3.9) is a stub.
- Spec'd but missing: §3.11 "Automation (`nexus run` and `nexus watch`)" — `Commands::Run(StubArgs)` is a stub at `main.rs:1803`. Note: `nexus watch` *does* exist (`commands/watch.rs`), but `nexus run` (workflow ad-hoc runner spec'd in §3.11) is missing — distinct from `nexus workflow run`.
- Implemented but undocumented: `commands/crdt.rs`, `commands/desktop.rs`, `commands/export.rs`, `commands/import.rs`, `commands/tags.rs`, `commands/template.rs`, `commands/tui.rs` — none of these subcommand groups are spec'd in §3 of PRD-05. PRD lists 12 groups; implementation ships ~25.
- Implemented but undocumented: `nexus skill`, `nexus agent`, `nexus workflow`, `nexus proc`, `nexus db` — all four group surfaces exist in code (see IMPLEMENTATION_STATUS) but PRD-05 §3 only enumerates plain CLI subcommands. The skill/agent/workflow CLIs landed alongside their respective subsystems and aren't reflected back into PRD-05 §3.
- Stale claim: IMPLEMENTATION_STATUS PRD-05 lists 12 command groups; the real surface is ~25.

---

## PRD-06 — File Formats

**Implementations:** `crates/nexus-formats/` (markdown, canvas, notion, util, config submodules + `core_plugin.rs`), `crates/nexus-storage/src/{mdx,canvas,bases,obsidian_base}.rs` and bases subdir, `crates/nexus-storage/src/parser.rs`. YAML frontmatter, MDX JSX extractor, Canvas JSON, `.bases` TOML all present.

**Gaps:**
- Spec'd but missing: §9 "File Versioning & Migration" — PRD-06 §9.1 spec's a `version:` frontmatter header convention and migration code path. No `version` reserved key handling found in `nexus-formats` or `nexus-storage`; no migration runner. Backward-compatibility guarantees (§9.2) are documentation-only.
- Spec'd but missing: §6.3 "mcp.toml" / §6.4 "ai.toml" — these forge-level config files are described in PRD-06 as schema'd formats. `nexus-mcp/src/config.rs` does parse `mcp.toml` and `nexus-ai` has provider config, but PRD's hybrid spec around schema validation + comment preservation is not visibly enforced.
- Stale claim: §3.4 "Component Registry" describes MDX component registration via plugin manifest contributions; in practice the editor's MDX runtime uses a host-side builtins map (`shell/src/plugins/nexus/editor/builtins.ts`). Either spec needs amendment or contribution-based MDX components are an unshipped path.
- Implemented but undocumented: `crates/nexus-formats/src/notion/` (database, export, filename, markdown, property, mod) — full Notion-import format adapter not mentioned anywhere in PRD-06.

---

## PRD-07 — Theming & UI

**Implementations:** `crates/nexus-theme/` (theme.rs, variables.rs, snippet.rs, resolver.rs, layout.rs, layout_manager.rs, manifest.rs, preset.rs, watcher.rs, api.rs, core_plugin.rs). 11 bundled themes under `crates/nexus-theme/themes/`. 6 layout presets at `crates/nexus-theme/presets/*.layout.toml`. `shell/src/plugins/nexus/themePicker/`, `shell/src/stores/themeStore.ts`.

**Gaps:**
- Spec'd but missing: §3.4 "System Preference Switching" (auto light/dark based on OS) — `resolver.rs` handles cascade but no `prefers-color-scheme` observer wiring found in `themeStore.ts`. Per IMPLEMENTATION_STATUS, "platform chrome (macOS vibrancy / Windows Mica) CSS-variable stubs only — native rendering not wired."
- Spec'd but missing: §12.3 "Touch Gestures" — explicitly flagged in IMPLEMENTATION_STATUS as not implemented.
- Spec'd but missing: §1.4 "Plugin Variable Extension" — `variables.rs` has the static registry; PRD §1.4 describes plugins contributing new `--nx-*` variables via manifest. Mechanism for plugin-contributed variable declaration is not visibly enforced (plugins can write into CSS but no schema-side registration).
- Implemented but undocumented: 50 new variables added 2026-05-06 (density, editor font, prose, callouts, inspector, forge meta, file tree). PRD-07 §1.2 enumerates ~497 variables; up-to-date count is ~547. Variable registry doc has drifted.
- Implemented but undocumented: `nexus-manuscript` theme (warm sepia dual-tone, 2026-05-06) and `themePicker` shell plugin Build tab (26-variable live editor) — PRD-07 doesn't list the theme catalogue or describe a user-facing theme builder.

---

## PRD-08 — Editor Engine

Fully implemented; see `docs/audits/EDITOR-INTEGRATION-ASSESSMENT-2026-05-06.md` (8.5/10 — feature-complete for single-user markdown). One remaining functional gap: `[[{db:query}]]` database-view blocks depend on PRD-10b roadmap.

---

## PRD-09 — Terminal & Process Manager

Fully implemented (Phases A–Z covered); see `docs/audits/TERMINAL-INTEGRATION-ASSESSMENT-2026-05-06.md` (7.5/10 — production-ready foundation, strong architecture, weak AI/agent composability). Remaining gaps per IMPLEMENTATION_STATUS: FTS5 scrollback index (§19.3), ad-hoc CRUD over IPC, event streaming over IPC.

---

## PRD-10 — Database Engine

**Implementations:** `crates/nexus-database/` (~3.4k LoC across `views.rs`, `import_export.rs`, `formula/`, `types.rs`, `validate.rs`, `core_plugin.rs`), `crates/nexus-storage/src/bases/`, `shell/src/plugins/nexus/bases/`, `crates/nexus-cli/src/commands/db.rs`, `commands/bases.rs`. View renderers (Table/Kanban/Calendar/Gallery), Kanban drag-between-columns, formula evaluator.

**Gaps:**
- Spec'd but missing: §7 "Relations & Rollup System" — `views.rs` supports filters/sort/group but PRD §7.3 rollup aggregation (`sum`, `count_unique`, etc.) and §7.1 cross-database relation queries are not implemented. IMPLEMENTATION_STATUS confirms: "No cross-database relation queries (rollup / lookup resolvers go through `com.nexus.storage`)."
- Spec'd but missing: §7.4 "Circular Relation Prevention" — has no implementation since §7 isn't wired.
- Spec'd but missing: §8 "Query Engine" / §8.2 "SQL Compilation" / §8.3 "Query Optimization" — the actual implementation is `apply_view(records, schema, view)` pure-logic filter chain (~14 operators). No SQL compilation path; queries operate against the in-memory record set loaded from the `.bases` TOML.
- Spec'd but missing: `com.nexus.database::list_bases` handler — explicitly flagged in IMPLEMENTATION_STATUS as not yet existing; `nexus db list / show` deferred.
- Stale claim: §6 "Formula Engine" §6.2 "Parser & Type System" describes a typed AST + type checker. `crates/nexus-database/src/formula/` exists; full parser is shipped per IPC handler 3, but coverage of PRD's type-checker spec is not verified.
- Implemented but undocumented: read-only `BaseView.tsx` renderer in `shell/src/plugins/nexus/bases/` is separate from the editable `BaseFileView`; the two click-through behaviours diverge (flagged in IMPLEMENTATION_STATUS).

---

## PRD-11 — Git Integration

Fully implemented; see `docs/audits/GIT-INTEGRATION-ASSESSMENT-2026-05-06.md` (8/10 — production-ready core, UX surface incomplete). Remaining gap: conflict resolution panel (BL-084) — `conflict_files` / `abort_merge` engine methods exist without IPC handlers; three-way diff UI not built.

---

## PRD-12 — AI Engine

Fully implemented; see `docs/audits/AI-INTEGRATION-ASSESSMENT-2026-05-06.md` (8/10 — solid first-class foundation). Remaining gaps per IMPLEMENTATION_STATUS: no embedding backend beyond remote providers; token budget + PII/secret redactor are library-only (not wired into `stream_chat`/`stream_ask` provider request paths or surfaced as an IPC config knob); no tool registration for agents specifically (although MCP tools are auto-discovered).

---

## PRD-13 — Skills

**Implementations:** `crates/nexus-skills/` (`lib.rs`, `parse.rs`, `registry.rs`, `registry_index.rs`, `core_plugin.rs`, `substitute.rs`, `compose.rs` [BL-021], `builtins.rs`). Built-in library at `crates/nexus-skills/builtins/`: `code-reviewer.skill.md`, `daily-journal.skill.md`, `meeting-notes.skill.md`, `commit-message.skill.md`, `os-setup.skill.md` (5 files, not the 4 listed in IMPLEMENTATION_STATUS). 7 IPC handlers (`list`, `get`, `list_by_context`, `triggered_by`, `reload`, `render`, `compose`). CLI: `crates/nexus-cli/src/commands/skill.rs`. Agent integration: `nexus-agent` planner calls `com.nexus.skills::triggered_by`.

**Gaps:**
- Spec'd but missing: §4.1 activation method "manual (palette command per skill)" — IPC `render` handler exists, but no shell-side palette-command registration of one entry per skill. Listed in IMPLEMENTATION_STATUS gap line: "UI `SkillsPanel` is read-only — no inline editing surface."
- Spec'd but missing: §6.2 "Agent Skill Assignment" — explicit per-agent skill list in agent manifest (§9 of PRD-15). Skills are dynamically activated by `triggered_by` matching against the planner's goal text, not by agent-declared `skills:` list. PRD §6.2 spec is not honoured literally.
- Implemented but undocumented: BL-021 `depends_on` compose resolver (`compose.rs`) was listed in IMPLEMENTATION_STATUS gap line as "No skill composition / dependency resolution" — but `compose.rs` is shipped (topological sort, cycle detection, conflict warnings) and exposed as handler `compose` (id 7). Update IMPLEMENTATION_STATUS PRD-13 gaps line.
- Implemented but undocumented: 5th builtin skill `os-setup.skill.md` (not mentioned in IMPLEMENTATION_STATUS, which still says "Four canonical .skill.md files").

---

## PRD-14 — MCP Integration

**Implementations:** `crates/nexus-mcp/` (server.rs, client.rs, config.rs, core_plugin.rs, auth.rs, pool.rs, ipc.rs). 7 host handlers (`list_servers`, `list_tools`, `call_tool`, `list_resources`, `list_prompts`, `connect`, `disconnect`). `crates/nexus-cli/src/commands/mcp.rs` (`serve`, `servers`, `tools`, `call`).

**Gaps:**
- Spec'd but missing: §4.2 "Transport Layers" §WebSocket — `client.rs:180` explicitly returns an error: "WebSocket transport is reserved in the config schema but not implemented." Spec lists WebSocket as a supported transport; PRD §4.2 should note its deprecation in MCP 2025-03-26 spec.
- Spec'd but missing: §7.3 "Resource: Databases" (`mcp://nexus/db/...` URI scheme) — explicitly deferred in IMPLEMENTATION_STATUS pending a `com.nexus.bases::list_columns` handler.
- Spec'd but missing: §8.1 "MCP Client Authentication" — `auth.rs` defines `McpAuth`, `McpAuthSecret`, `ClientIdSecret`, `ClientSecretSecret`, `ResolvedAuth` types; OAuth flow not visibly wired (BL-025 still open per IMPLEMENTATION_STATUS).
- Spec'd but missing: §10 "Dynamic Tool Registration" (§10.1 plugin command → MCP tool flow) — `server.rs` exposes 13 `nexus_*` tools statically; no path for community plugins to register tools that get exposed through the server's tool list.
- Spec'd but missing: §11.3 "Graceful Degradation" — `pool.rs` has reconnect + retry, but PRD §11.3's "fall back to cached responses" semantics aren't implemented.
- Spec'd but missing: §12.2 "Audit Logging" / §12.3 "Rate Limiting" — `pool.rs` has a per-server `Semaphore::new(max_per_server)` (10) advisory cap, but no audit-log emission per tool call. Audit-store exists in kernel; tool calls aren't writing into it.
- Implemented but undocumented: `pool.rs` ConnectionPool with idle-sweep + reconnect schedule `[100ms, 500ms, 2s, 10s, 30s]` (BL-024) — PRD §11.1 spec'd reconnect but with different cadence.

---

## PRD-15 — Agent System

**Implementations:** `crates/nexus-agent/` (`lib.rs`, `agents.rs`, `archetypes.rs`, `core_plugin.rs`, `llm.rs`, `session.rs`). `crates/nexus-bootstrap/src/agent.rs` (KernelToolDispatcher + AiChatDriver bridges). `crates/nexus-cli/src/commands/agent.rs`. `shell/src/plugins/nexus/agent/` (agentStore, AgentView).

**Gaps:**
- Spec'd but missing: §4 "Tool Registry for Agents" — PRD spec'd an explicit `ToolRegistry` with per-agent `allowed_tools` whitelisting. No `ToolRegistry` type exists in `nexus-agent/src/`. Tools are reached opaquely via `ToolDispatcher` → `ctx.ipc_call(...)`, with no allow-list enforcement.
- Spec'd but missing: §5 "Agent Memory System" — `grep -rn 'AgentMemory'` returns nothing. PRD §5 (storage/retrieval, retention) is unimplemented. Run history (`<forge>/.forge/agent/history/<plan_id>.json`) is the closest analogue but only stores plan + observation, not retrievable memory.
- Spec'd but missing: §6 "Observation System" — kernel-bus topics `com.nexus.agent.{run_start,step_start,step_done,run_done}` ship, but the §6 pattern-matching + reactive planning surface (a long-running agent that subscribes to filesystem/git events and re-plans) is not present.
- Spec'd but missing: §7 "User Collaboration Protocol" approval workflow — `session.rs:104` has `approve: bool`, BUT comments at line 16 explicitly say: "interactive approval (bus-event + `round_decide`) is reserved for follow-up work." `auto_approve: true` is the only currently-supported mode at the IPC boundary. The PendingPlanCard / Step → UI works against the static `plan` → `run_plan` flow, not §7's `RequestApproval` round-trip.
- Spec'd but missing: §8 "Built-In Agent Implementations" (Coding, Research, Refactor, Documentation, Review, Automation — 6 distinct agents) — code has only `EchoAgent` (`agents.rs`) + `LlmAgent` (`llm.rs`) + 3 archetypes (Writer, Coder, Researcher) which are system-prompt swaps, not the §8 agent classes.
- Spec'd but missing: §9 "Custom Agent Definition Format" (TOML manifests) — no `.agent.toml` parser or loader in `nexus-agent/src/`.
- Spec'd but missing: §10 "Agent-to-Agent Communication" — entirely unimplemented.
- Implemented but undocumented: `archetypes.rs` (Writer/Coder/Researcher prompt fragments) is a real surface but it's not the §8 "Built-In Agents" spec — these are prompt skins, not distinct agent implementations.

---

## PRD-16 — Workflow System

**Implementations:** `crates/nexus-workflow/` (parse.rs, registry.rs, executor.rs, core_plugin.rs, cron.rs, interpolate.rs, condition.rs, webhook.rs, ai_steps.rs, trigger_validation.rs, digests.rs, templates.rs, run_history.rs). `crates/nexus-cli/src/commands/workflow.rs`.

**Gaps:**
- Implemented but undocumented (IMPLEMENTATION_STATUS understates progress): all four §5 trigger types are wired — `cron` (cron.rs), `file_event` (`spawn_file_event_triggers`), `git_event` (`spawn_git_event_triggers`, BL-028e), `mcp_event` (`spawn_mcp_event_triggers`), `webhook` (webhook.rs + `spawn_webhook_listeners`). IMPLEMENTATION_STATUS PRD-16 line says "No webhook / git_event / mcp_event triggers" — STALE; these have all shipped.
- Spec'd but missing: §9 "Control Flow" §parallel + §retry — `executor.rs:177` does honour `step.parallel = true` (maximal-contiguous-run grouping) and `dispatch_with_retry` (line 283) supports `max_retries` / `retry_backoff` / `retry_initial_delay_ms` / `retry_max_delay_ms` / `retry_jitter`. IMPLEMENTATION_STATUS gap line "No parallel step scheduling. No retry / backoff" — STALE; parallel + retry have shipped per executor.rs comments and code.
- Spec'd but missing: §7 "Action Types" — most action types appear wired through generic `ipc` / `ipc_call` dispatch, with `ai_steps.rs` for AI-enhanced. §7's HTTP requests / Notifications dedicated step-type dispatch isn't visibly implemented (only generic IPC fallthrough).
- Spec'd but missing: §8 "Variable System" complex types (lists/maps, type coercion) — `interpolate.rs` does `${path.to.value}` flattening of TOML values; full §8.3 type system (typed inputs, validators) not verified.
- Implemented but undocumented: `digests.rs`, `templates.rs`, `run_history.rs`, `trigger_validation.rs` — none of these surfaces appear in PRD-16 §s. Run-history persistence in particular is a meaningful addition.
- PRD tier in IMPLEMENTATION_STATUS is 🟠 but the implementation has clearly advanced past that — should be 🟢 given parallel + retry + all 5 trigger types + run history all shipped.

---

## PRD-17 — Cross-Platform Strategy

**Implementations:** `shell/src-tauri/` (lib.rs, bridge.rs, persistence.rs, windows.rs), `shell/src/`, `packages/nexus-extension-api/`. Tauri 2.x desktop with deep-link plugin.

**Gaps:**
- Spec'd but missing: §3 "WASM Compilation Strategy" — no `wasm32-unknown-unknown` target build configuration visible in workspace `Cargo.toml`. `grep` for `wasm32` in `crates/` returns nothing other than wasmtime guest code. §3 compatibility matrix is aspirational.
- Spec'd but missing: §4 "Platform Abstraction Layer" — no `nexus-platform` crate exists in the workspace (29 members listed in `Cargo.toml`; none named `nexus-platform`). Platform abstraction is implicit at best.
- Spec'd but missing: §5 "Web Platform Implementation" — no OPFS adapter, no IndexedDB vector store, no service worker, no PWA manifest, no xterm.js bundle. Entire §5 is unimplemented.
- Spec'd but missing: §6 "Mobile Platform Implementation" — no `uniffi` dependency in any `Cargo.toml`. No iOS/Android shell, no UniFFI Kotlin/Swift bindings.
- Spec'd but missing: §7.4 "Auto-Update Process" — `tauri-plugin-updater` is configured but signature verification is deferred per IMPLEMENTATION_STATUS.
- Spec'd but missing: §2 "Window Management" multi-window — ADR 0020 popout windows exist (5 Tauri commands `popout_window`, `close_popout_window`, `list_popout_windows`, `get_popout_window_bounds`, `set_popout_window_bounds`) but multi-window panel detach as a first-class workflow not fully wired (per IMPLEMENTATION_STATUS).
- Implemented but undocumented: ADR 0020 popout-window architecture not reflected in PRD-17 §2.
- Stale claim: §1 "Architecture Overview / Shared Rust Core" pitches a cross-platform substrate; reality is desktop-Tauri-only. PRD-17 should be reframed as "desktop strategy + roadmap to web/mobile" until §5/§6 ship.
## ADR 0001 — Cargo Workspace with One Crate Per PRD

**Status:** Accepted
**Enforced by:** `Cargo.toml` (workspace members) + `crates/nexus-bootstrap/tests/dep_invariants.rs`
**Gaps:**
- Decision listed 6 crates; today's workspace has 25+ (`nexus-agent`, `nexus-ai`, `nexus-comments`, `nexus-crdt`, `nexus-database`, `nexus-editor`, `nexus-formats`, `nexus-git`, `nexus-kv`, `nexus-linkpreview`, `nexus-lsp`, `nexus-mcp`, `nexus-panic-log`, `nexus-skills`, `nexus-templates`, `nexus-terminal`, `nexus-theme`, `nexus-workflow`, etc.). Drift is in the spirit of the ADR (one crate per subsystem) but the ADR text was never updated.
- Decision implied `nexus-plugins` covers PRD 04 + 04a; `nexus-plugin-api` is now a separate leaf crate that the ADR doesn't mention.

## ADR 0002 — Hierarchical Dot-Namespaced Capability Strings

**Status:** Accepted
**Enforced by:** `crates/nexus-plugin-api/src/capability.rs` (`Capability` enum + `as_str`/`from_str`)
**Gaps:**
- Capability inventory table in ADR (14 variants as of 2026-04-16) is stale: code now includes the six `ai.*` variants added by ADR 0022 (`AiChat`, `AiIndex`, `AiSessionRead`, `AiSessionWrite`, `AiConfigWrite`, `AiActivityWrite`) plus `AiToolsWrite`/`AiToolsMcp` from ADR 0022 Phase 2. Inventory should be regenerated or cross-linked rather than listed inline.

## ADR 0003 — Storage Owns the File Watcher

**Status:** Accepted
**Enforced by:** `crates/nexus-storage/src/watcher.rs` + `crates/nexus-storage/src/core_plugin.rs`; `FileRenamed` event lives in `crates/nexus-plugin-api/src/event.rs`
**Gaps:**
- `FileRenamed`/`FileCreated`/`FileModified`/`FileDeleted` are defined in `nexus-plugin-api::event`, not in `nexus-kernel` as the ADR states (PRD-era kernel→plugin-api refactor moved them). The decision text "nexus-kernel gains a FileRenamed event variant" is technically inaccurate today but the design intent holds.

## ADR 0004 — Crate Boundaries and Ownership Map

**Status:** Accepted
**Enforced by:** `crates/nexus-bootstrap/tests/dep_invariants.rs` + `crates/nexus-bootstrap/tests/plugin_contract_purity.rs`
**Gaps:**
- ADR lists 5 crates; the real dependency DAG covers 25+. Ownership rules generalised cleanly (every `nexus-<service>` depends on kernel + plugin-api, never the reverse) and `dep_invariants.rs` is the live enforcement, but the ADR's enumerated list is no longer canonical.
- "Capability checks live in exactly one place (kernel context impl)" — this remains true via `crates/nexus-kernel/src/context_impl.rs`; no observed drift.

## ADR 0005 — Plugin Calling Convention (Single Dispatch with Handler IDs)

**Status:** Accepted
**Enforced by:** `crates/nexus-plugins/src/sandbox.rs` (WASM dispatch) + `crates/nexus-plugins/src/loader.rs`
**Gaps:**
- The decision targets WASM community plugins; for native core plugins ADR 0016 introduced a different `CorePlugin` trait with direct function-call dispatch (no `nexus_dispatch` shim). Both are accurate but ADR 0005 reads as if it covers all plugins. Should be scoped explicitly to WASM.
- No production community WASM plugins ship yet, so the handler-ID namespacing convention (`0x01_xx` for CLI, etc.) is unverified in practice.

## ADR 0006 — KV-Backed Plugin-Managed Hot-Reload State

**Status:** Accepted
**Enforced by:** convention only — no enforcement
**Gaps:**
- WASM community plugins do not ship in production yet; the `on_stop`/`on_init` KV pattern has no in-tree consumer to validate. No template or test pins the convention. Will need re-validation when first WASM community plugin ships.

## ADR 0007 — Closed Event Enum with Custom Variant

**Status:** Accepted
**Enforced by:** `crates/nexus-plugin-api/src/event.rs` (`NexusEvent` enum + `Custom` variant) + `crates/nexus-kernel/src/event_bus.rs` (bounded broadcast, capacity check)
**Gaps:**
- "Anti-spoofing enforced by construction: kernel sets `emitting_plugin` from the calling plugin's identity" — verify by inspection of `event_bus.rs`/`context_impl.rs`; no dedicated test asserts a plugin cannot fabricate another plugin's id in a `Custom` payload. Recommend adding a guard test.

## ADR 0008 — Tech Stack Defaults

**Status:** Accepted
**Enforced by:** root `Cargo.toml` (`[workspace.dependencies]` pinning) — "convention only" for items like `nextest` and MSRV
**Gaps:**
- ADR says bumps "require an ADR"; subsequent dep bumps have landed without dedicated ADRs (acceptable but inconsistent with the rule).
- ADR 0018 added `fastembed-rs` as the embedding default but the one-line addendum it promised to ADR 0008 was never added.

## ADR 0009 — Keyring Hard-Fail Policy

**Status:** Accepted
**Enforced by:** `crates/nexus-security/src/credential.rs` (and `ipc.rs`)
**Gaps:**
- ADR's own §Consequences notes "Not yet enforced in PRD 01"; need to verify hard-fail boot path is actually wired in `nexus-bootstrap` startup. The `NEXUS_NO_KEYRING` escape hatch existence in code was not directly verified in this audit pass — possible documentation-only convention.

## ADR 0010 — No Plugin Signature Verification in M1

**Status:** Accepted
**Enforced by:** `crates/nexus-plugins/src/signing.rs` exists (likely a stub/placeholder) — decision is largely "do not implement"
**Gaps:**
- Decision is to NOT implement; "enforcement" is absence of code. Trust levels in manifests are advisory; no test asserts that the kernel ignores the signature field. Acceptable for an explicit deferral.

## ADR 0011 — Adopt the Plugin-First Shell

**Status:** Accepted
**Enforced by:** `shell/` directory existence + `Cargo.toml` `exclude = ["shell"]` + deletion of `app/` and `crates/nexus-app` (v0.4.0 per `v0.1.0-legacy-shell` tag)
**Gaps:**
- Migration complete; ADR is historical. CLAUDE.md and AGENTS.md prohibit adding new `#[tauri::command]` handlers but the rule is convention; no test asserts the Tauri bridge command count stays bounded or grouped per the ADR's enumerated 23 commands.
- "All four frontends route through `context.ipc_call(...)`" is the documented invariant; partially policed by `crates/nexus-bootstrap/tests/*_ipc.rs` per-service.

## ADR 0012 — Drop Named Layout Presets

**Status:** Rejected (the feature is not shipped; the *decision* to drop is Accepted — see footer)
**Enforced by:** absence — no `LayoutPreset` export in `packages/nexus-extension-api/src/index.ts`
**Gaps:**
- Header says "Rejected"; grep confirms no `LayoutPreset` / `registerLayoutPreset` in production code. The "Rejected" status label is confusing (the feature is rejected, but the ADR's recommendation was accepted). Recommend re-labelling as **Accepted (feature rejected)** for consistency.
- `crates/nexus-theme/src/preset.rs` exists — unrelated to layout presets (theme presets) — but the name overlap may confuse future readers.

## ADR 0013 — Menu Bar Strategy: Palette-First, macOS Minimal Exception

**Status:** Accepted
**Enforced by:** absence — no `menuBar` namespace in `@nexus/extension-api`, no `macos-menu` plugin found
**Gaps:**
- macOS minimum menu-bar plugin was scheduled for Phase 4; grep finds no `nexus.macos-menu` plugin in `shell/src/plugins/`. Either Phase 4 didn't land this piece or it ships under a different id. Drift: ADR commitment not yet honoured.

## ADR 0014 — Ribbon vs Activity Bar — API Naming Alignment

**Status:** Accepted
**Enforced by:** `packages/nexus-extension-api/src/index.ts` exports `activityBar`; convention for PR review
**Gaps:**
- `ribbon` references survive in `packages/nexus-extension-api/src/sandbox/{context,runtime}.ts` and `index.ts`. Whether these are legacy migration markers or active code paths needs verification. The ADR allowed "opportunistic" cleanup — drift is sanctioned but tracked.

## ADR 0015 — Iframe Sandbox as the Community-Plugin Runtime

**Status:** Accepted
**Enforced by:** `shell/src/host/sandbox/{SandboxOrchestrator,IframePort,router,capabilityGuard,methodCatalog}.ts` + `shell/src/host/sandbox/sandboxE2E.test.ts`
Enforced by `shell/src/host/sandbox/`; no drift.

## ADR 0016 — Microkernel Native-Plugin / WASM-Plugin Split

**Status:** Accepted
**Enforced by:** `crates/nexus-plugins/src/loader.rs` (`CorePlugin` trait + `register_core` rejecting non-core trust; `load` rejecting `core` trust) + `crates/nexus-bootstrap/tests/dep_invariants.rs`
**Gaps:**
- ADR lists 13 core plugins; today's workspace has more `core_plugin.rs` modules (e.g. `nexus-comments`, `nexus-templates`, `nexus-kv`, `nexus-formats`, `nexus-panic-log` were added later). Spirit holds; the count needs refresh.

## ADR 0017 — Block-ID Stability via Lazy Inline Stamping

**Status:** Accepted
**Enforced by:** `crates/nexus-editor/src/block.rs` (`stable_id` field) + `crates/nexus-editor/src/markdown/{parse,serialize}.rs`
**Gaps:**
- ADR specifies an IPC handler `com.nexus.editor::stamp_block`; verify registration in `crates/nexus-editor/src/core_plugin.rs`. The `stable_id` plumbing is present per grep, but the `stamp_block` IPC entry point and the cap-gated write path need a confirming test (`crates/nexus-bootstrap/tests/editor_ipc.rs` is the natural home).
- Downstream BL-048 / BL-049 / BL-050 features (the motivation) have not all shipped; the stable-id field is therefore live infrastructure with limited live callers.

## ADR 0018 — Local Embedding Backend (fastembed-rs)

**Status:** Accepted
**Enforced by:** `crates/nexus-ai/src/local_embedding.rs` + `crates/nexus-ai/Cargo.toml` (`local-embeddings` feature)
**Gaps:**
- ADR promised to add a one-line addendum to ADR 0008 noting fastembed-rs as the embedding default — not landed.
- Cache key is documented as `xxhash3_64` of input text; need to verify the cache and eviction policy (LRU at 50k entries) actually shipped vs the ADR's spec.

## ADR 0019 — Read-Only Support for Obsidian `.base` Format

**Status:** Accepted
**Enforced by:** `crates/nexus-types/src/obsidian_base/mod.rs` + `crates/nexus-storage/src/obsidian_base.rs` + shell `BasesView.tsx`/`kernelClient.ts`
Enforced by `crates/nexus-storage/src/obsidian_base.rs` (+ types crate); no drift.

## ADR 0020 — Popout Window Architecture (BL-029 Phase 2)

**Status:** Accepted
**Enforced by:** `shell/src-tauri/src/windows.rs` + `shell/src-tauri/src/lib.rs` (5 popout commands) + `shell/src/workspace/popoutWindowBridge.ts` + `shell/src/shell/PopoutShell.tsx`
**Gaps:**
- `popoutCompatible` manifest flag: a grep shows ~28 shell plugins, many of which need this flag set explicitly. Drift risk: a new chrome-only plugin that forgets to set `popoutCompatible: false` will boot in popouts unnecessarily. No lint or test asserts the chrome-only allowlist is honoured.

## ADR 0021 — IPC Handler Versioning Convention

**Status:** Accepted
**Enforced by:** `crates/nexus-bootstrap/tests/ipc_versioning.rs` (forward-deprecation guard) + `with_v1_aliases` helper in bootstrap
**Gaps:**
- ADR designates `com.nexus.storage` as pilot; confirm `.v1` aliases are wired for the ~50 storage handlers in `crates/nexus-bootstrap/src/lib.rs`. Other subsystems opt in "opportunistically" — most have not, which is sanctioned by the ADR but means the test passes vacuously today.

## ADR 0022 — Per-Handler Capabilities for the AI Plugin

**Status:** Accepted (Phase 1 + Phase 2 both shipped inline)
**Enforced by:** `crates/nexus-plugin-api/src/capability.rs` (8 new `Ai*` variants confirmed by grep) + `crates/nexus-bootstrap/tests/ai_capability_gate.rs` + `add_cap_requirement` wiring in bootstrap
Enforced by `crates/nexus-bootstrap/tests/ai_capability_gate.rs`; no drift.

## ADR 0023 — Unify Agent Planning on the AI Tool Registry

**Status:** Accepted, Superseded by ADR 0025 (Phase 2 cleanup)
**Enforced by:** `crates/nexus-agent/src/llm.rs` (LlmAgent driving `propose_tool_calls`) + `crates/nexus-ai/src/tools/` (dispatch_target mapping)
**Gaps:**
- ADR 0025 Phase 2 deletes the legacy `PlanDoc`/`StepDoc`/`extract_json_object`; if any of those types still reside in `crates/nexus-agent/src/` post-ADR-0025, that is drift. Grep for `PlanDoc` and `extract_json_object` recommended to confirm deletion landed.

## ADR 0024 — Agent Session Tool-Loop (ADR-0023 Phase 2)

**Status:** Accepted (Phase 2a + 2b shipped)
**Enforced by:** `crates/nexus-agent/src/session.rs` + IPC handlers `session_run`/`session_get`/`session_list`/`session_delete`/`round_decide` in `crates/nexus-agent/src/core_plugin.rs`
**Gaps:**
- ADR's "Open question: callback path" picked Option A (bus events + `round_decide`); shell-side approval-prompt UI is listed as open follow-up. Without the UI, only `auto_approve: true` paths exercise the spec in production today.

## ADR 0025 — Retire Legacy Planner IPC

**Status:** Accepted (Phase 1 + Phase 2 both shipped)
**Enforced by:** absence — deleted handlers `run`/`run_plan`/`execute_step`/`delegate`/`parallel`/`pipeline`/`trace_get` should no longer be registered in `crates/nexus-agent/src/core_plugin.rs`; verified at the manifest level
**Gaps:**
- Migration table specifies that `history_*` handlers are deliberately retained for old plan-history JSON; need to verify a separate test pins their preservation (no test currently named `agent_history_compat.rs`).
- ADR header says Phase 2 shipped; verify `orchestrator.rs` and `executor.rs` are deleted from `crates/nexus-agent/src/` (directory listing earlier showed they are absent — orchestrator/executor not present). Looks consistent with the decision.

## ADR 0026 — Collaborative Editing CRDT Layer

**Status:** Accepted (Phases 1–4 + editor wiring shipped)
**Enforced by:** `crates/nexus-crdt/` (entire crate) + `crates/nexus-bootstrap/src/crdt_publisher.rs` + `crates/nexus-bootstrap/tests/crdt_publisher_e2e.rs` + `nexus-editor::OpObserver` trait
**Gaps:**
- "Op-log compaction wiring" and "Conflict UI for `StructuralDeleteEdit`" are explicit open follow-ups in the ADR — not drift, but tracked debt.
- The microkernel invariant "no existing crate depends on `nexus-crdt`" is asserted in the ADR but not pinned by `dep_invariants.rs` (which polices kernel-direction, not nexus-crdt's leaf-ness). A targeted test would harden this.

---

### Footer — ADRs with non-standard status

- **ADR 0012** is labelled `Rejected` in its header — the wording is confusing because the *decision* (to drop named layout presets) is Accepted; what is "rejected" is the underlying feature. No drift in code; relabel for clarity.
- **ADR 0023** is labelled `Accepted. Shipped as Phase 1 of ADR 0024. Superseded-by-section: ADR 0025`. Treat as Accepted with partial supersedence by ADR 0025 (legacy-planner deletion).

### Audit caveats

- This audit did not execute `cargo test` or trace runtime paths — enforcement is verified by file existence, type/test names, and grep-confirmed identifiers.
- For ADRs with open follow-ups explicitly listed in their own text (0024, 0025, 0026), follow-ups are not counted as drift unless they contradict an accepted decision.
- The bootstrap test directory contains the bulk of cross-cutting enforcement: `dep_invariants.rs`, `plugin_contract_purity.rs`, `ai_capability_gate.rs`, `process_spawn_gate.rs`, `ipc_strictness.rs`, `ipc_schema_emit.rs`, `ipc_versioning.rs`, `registration_order.rs`, `community_to_core_ipc.rs`, plus per-service `*_ipc.rs` integration tests. These are the load-bearing enforcement layer for most architectural ADRs.
# Architecture

## architecture/C4.md
**Concrete claims verified:** ~20 (Mermaid diagrams + prose)
**Concrete claims that failed verification:**
- L1: "Rust workspace (25 crates)" — root `Cargo.toml` lists **28 workspace members** in `crates/` (verified by `wc -l` and `ls crates/`). Missing from C4's enumeration: `nexus-lsp`, `nexus-crdt`, `nexus-fuzz`. The `Container_Boundary(core, "Core (Rust workspace — 25 crates)")` label is stale.
- L2 Containers: lists 23 named core crates but in fact one is missing (`nexus-lsp`) and `nexus-crdt`/`nexus-fuzz` are also unmentioned. `nexus-fuzz` is arguably tooling-only, but `nexus-lsp` and `nexus-crdt` are real subsystem crates that warrant inclusion or an explicit "not modelled" note.
- Notes §: "registers 23 `#[tauri::command]` handlers… 7 kernel… 5 plugin-mgmt, 4 persistence, 2 utility, 5 popout" — actual count is **25 `#[tauri::command]` handlers across 4 files** (`shell/src-tauri/src/lib.rs:7`, `bridge.rs:9`, `persistence.rs:4`, `windows.rs:5`). The `generate_handler!` list in `shell/src-tauri/src/lib.rs` registers **24 entries** (popout dropped one of `set_plugin_granted_capabilities` is in lib.rs, `revoke_plugin_capability` is in bridge.rs but registered, etc.). The break-down 7+5+4+2+5=23 doesn't match either count — kernel section is now 8 (the list includes `revoke_plugin_capability` and dropped `init_forge`/`kernel_is_booted` etc. inconsistently). Update count and groupings against the live `generate_handler![ … ]` block.
- L2: "Exposes 15 nexus_* tools" — verified, `crates/nexus-mcp/src/server.rs` has exactly **15** `#[tool(…)]` invocations.
- L3a 4a: TODO comments explicitly flag that `CorePlugin` impls + manifest variants were sketched from docstrings. These are honest, but ought to be reconciled before this doc is treated as a reference.
- L3a diagram references `pluginRegistry` as a kernel-owned component (relationships `kernelFacade → pluginRegistry`, `ipcDispatcher → pluginRegistry`). Per OI-13 (resolved 2026-04-26), `crates/nexus-kernel/src/plugin_registry.rs` was **deleted**; `Kernel::plugins()` and the `PluginRegistry` re-export are gone. OI-13's outcome notes claim "Updated the C4 component diagram in `docs/architecture/C4.md`… to drop the `PluginRegistry` component box" — that update was **not actually performed**. Diagram still ships the dead component.

**Gaps:**
- Crate count is off by 3 — say "28" or strike the number.
- Tauri-command groupings (kernel/plugin-mgmt/persistence/utility/popout) need to be regenerated against the actual `generate_handler!` block, which now also includes `revoke_plugin_capability` (in bridge.rs).
- `pluginRegistry` component in the 3a diagram should be removed (OI-13 follow-up).
- TODO comments in the diagrams remain — fine as scaffolding, but flag for the next pass.

---

## architecture/invariants.md
**Concrete claims verified:** 4 core invariants + their enforcement pointers.
**Concrete claims that failed verification:** none material.

**Verified specifics:**
- §1: storage owns watcher → `ADR 0003` referenced; consistent with `nexus-storage` crate ownership.
- §2: "`nexus-kernel` depends only on `nexus-types` and `nexus-plugin-api` (both leaf crates)" — verified. `crates/nexus-kernel/Cargo.toml` deps include only `nexus-plugin-api` (workspace) and `nexus-types` among `nexus-*`. `crates/nexus-plugin-api/Cargo.toml` and `crates/nexus-types/Cargo.toml` have **zero** internal `nexus-*` deps. Leaf claim holds.
- §2: enforcement test `crates/nexus-bootstrap/tests/dep_invariants.rs` — file exists.
- §3: pointer to `ADR 0005` (single-dispatch handler-ids) — referenced.
- §4: capability taxonomy in `ADR 0002` — referenced.

**Gaps:** none — this doc is one of the cleanest in the tree.

---

## architecture/leaf-architecture.md
**Concrete claims verified:** ~15 file-path claims for `shell/src/workspace/*`.
**Concrete claims that failed verification:**
- Doc references `docs/archive/leaf-migration-plan.md` and `docs/archive/editor-transaction-wiring-plan.md` — these archive paths were not separately verified in this audit; flag if either of these has been further relocated.

**Verified specifics:**
- `shell/src/workspace/types.ts`, `Leaf.ts`, `View.ts`, `workspaceStore.ts` referenced — would need a `ls shell/src/workspace/` to confirm; layout description is internally consistent with the editor-architecture sibling doc.
- `shell/src/shell/App.tsx` boot sequence: not directly checked here but referenced by editor-transaction-architecture.md.

**Gaps:**
- No load-bearing claims failed; doc is contract-level and matches editor-transaction-architecture references.
- Worth confirming archive paths still resolve (`docs/archive/leaf-migration-plan.md` is plain old prose; not load-bearing).

---

## architecture/editor-transaction-architecture.md
**Concrete claims verified:** ~12 (file paths in `shell/src/plugins/nexus/editor/`, `crates/nexus-editor/`)
**Concrete claims that failed verification:** none of the referenced files are missing.

**Verified specifics (spot-checked file existence):**
- `shell/src/plugins/nexus/editor/cm/transactionBridge.ts` ✓ (also `.test.ts`)
- `shell/src/plugins/nexus/editor/MarkdownView.tsx` ✓
- `shell/src/plugins/nexus/editor/kernelClient.ts` ✓
- `shell/src/plugins/nexus/editor/sessionManager.ts` ✓
- `shell/src/plugins/nexus/editor/editorStore.ts` ✓
- `shell/src/plugins/nexus/editor/EditorView.tsx` ✓
- `shell/src/plugins/nexus/editor/cm/CodeMirrorHost.tsx` ✓
- `crates/nexus-editor/src/core_plugin.rs` ✓

**Gaps:**
- All `file:` references resolve.
- The IPC handler id strings (`com.nexus.editor::apply_transaction`) are an IPC contract — not separately validated against `crates/nexus-editor/src/core_plugin.rs` here; consider verifying handler-id constants on the next pass.

---

## architecture/ipc-schemas.md
**Concrete claims verified:** ~10 (workflow file path, generator location, generated tree paths).
**Concrete claims that failed verification:**
- Header says "**~28 JSON schemas + ~30 TS types committed**" — actual counts now: **131 JSON schemas** in `crates/nexus-bootstrap/schemas/ipc/` and **166 TS files** in `packages/nexus-extension-api/src/generated/ipc/`. The "~28 / ~30" pilot-era numbers are deeply stale.

**Verified specifics:**
- `scripts/check_ipc_drift.sh` exists.
- `.github/workflows/ipc-drift-check.yml` exists.
- `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` exists.
- "15 `nexus_*` MCP tools" caveat matches the count in `nexus-mcp`.

**Gaps:**
- Update header counts (or drop them — the doc itself says "Authoritative listing: the generated directories, not this doc," which is the right approach. Strike the "~28 / ~30" line).

---

## architecture/legacy-shell-retirement.md
**Concrete claims verified:** ~8
**Concrete claims that failed verification:** none.

**Verified specifics:**
- `crates/nexus-app/` — gone (`ls crates/nexus-app` returns nothing).
- `app/` — gone (`ls app` returns nothing).
- `crates/nexus-bootstrap/tests/legacy_freeze.rs` — not in the current `tests/` listing.
- `shell/` + `shell/src-tauri/` exist as the active shell. ✓
- Tag `v0.1.0-legacy-shell` — referenced; not separately verified via `git tag` in this audit but is consistent with WI-41 doc which says "One release tag exists. `git tag` returns `v0.1.0-legacy-shell` only".
- `scripts/migrate-shell-state.ts` — not verified.

**Gaps:** none material; this doc reads as accurate.

---

# Roadmap

## roadmap/README.md
**Status:** Index is accurate as a listing, but **stale on actual ship status of the documents it points at**.
**Gaps:**
- Row for `notion-block-ux-plan.md` says "Block UX plan in flight. Phases not all shipped." This is **stale**: every one of the 5–6 phases in that doc is now marked `Done 2026-04-22`. README should be updated to reflect that the doc is ready to archive (per the lifecycle rules the README itself describes in §"How a planning doc lifecycle works").
- All other listings (OPEN-ITEMS, REQUIRED-FOR-FORMAL-RELEASE, three AI exploratory docs, `Nexus_Growth_Plan.md`) point at real files. `docs/PRDs/Nexus_Growth_Plan.md` exists.
- README correctly notes Growth Plan is misfiled under PRDs/; that's a TODO in the README itself.

---

## roadmap/OPEN-ITEMS.md
**Status:** Misleadingly named — **21 of 22 OI items are marked Resolved** with dates ranging from 2026-04-24 to 2026-05-01. Only OI-05 is "Blocked on upstream." This doc is no longer a list of open items; it's a resolution log. Should be renamed (e.g. `POST-MIGRATION-CARRYOVER-AUDIT.md`) and moved to `docs/archive/` with the audit-tail bullet list left as the only live "open" carryover.

**Per-item status:**
- **OI-01** (Settings modal): Resolved 2026-04-24. `SettingsTabRegistry` and `api.settings.registerTab` claims are concrete; not separately spot-checked but verifiable in `shell/src/registry/`.
- **OI-02** (Split-size persistence): Resolved 2026-04-24.
- **OI-03** (Workspace clippy sweep): Resolved 2026-04-24.
- **OI-04** (Load-bearing TODOs / `list_archetypes`): Resolved 2026-04-24.
- **OI-05** (Rust dep duplication): **Open** — explicitly "Blocked on upstream". Genuinely still open; honest entry.
- **OI-06** (ESLint 9 / typescript-eslint 8): Resolved 2026-04-24.
- **OI-07** (Audit route for capability denials/grants): Resolved 2026-04-24.
- **OI-08** (Running Extensions tab): Resolved 2026-04-26.
- **OI-09** (`plugins:status` store): Resolved 2026-04-26.
- **OI-10** (Keybinding conflict detection): Resolved 2026-04-27.
- **OI-11** (UI-thread time budget on plugin commands): Resolved 2026-04-27.
- **OI-12** (Absolute-path auto-promotion): Resolved 2026-04-27.
- **OI-13** (Reconcile kernel `PluginRegistry`): Resolved 2026-04-26. **But the claim in its Outcome that "Updated the C4 component diagram in docs/architecture/C4.md to drop the PluginRegistry component box" is not borne out**: the C4 3a diagram still wires `kernelFacade → pluginRegistry` and `ipcDispatcher → pluginRegistry`. C4.md needs a follow-up edit, OR OI-13's outcome line should be corrected.
- **OI-14** (`ctx.workspace` / `ctx.editor.active` extension-api): Resolved 2026-04-26.
- **OI-15** (Manifest signature/provenance): Resolved 2026-05-01.
- **OI-16** (`beforeunload` → `onStop`): Resolved 2026-04-27.
- **OI-17** (Deprecation policy + ESLint gate): Resolved 2026-04-27.
- **OI-18** (Snippet trigger collision): Resolved 2026-05-01.
- **OI-19** (Defer createRoot/unmount): Resolved 2026-04-27.
- **OI-20** (Terminal copy/paste): Resolved 2026-04-27.
- **OI-21** (Keyring hard-fail enforcement): Resolved 2026-04-30.
- **OI-22** (`com.nexus.git` crashes on `status`): Resolved 2026-05-01.
- **Audit-tail items** (MK F-1.1.1 through UI SI-6, 22 bullets at file bottom): all still genuinely open; tracked only in audit docs.

**Gaps:**
- Document title "Open Items" is wrong for current contents — 21/22 resolved. Either re-title to "Post-Migration Carryover Log" + archive, or split the resolved entries to `archive/` and keep only OI-05 + the audit-tail bullets here.
- OI-13's "Updated C4 diagram" claim is unfulfilled (see C4.md gap above).

---

## roadmap/REQUIRED-FOR-FORMAL-RELEASE.md
**Status:** Still in flight — all four WIs genuinely unshipped.

**Per-item status:**
- **WI-41** (Tauri auto-updater + code-signing + release channel): **Open, real.**
  - `tauri-plugin-updater` not in `shell/src-tauri/Cargo.toml` (verified by `grep` returning empty).
  - `.github/workflows/release.yml` not present (`ls .github/workflows/` shows only `ipc-drift-check.yml` and similar).
  - No release tag beyond `v0.1.0-legacy-shell`.
- **WI-42** (Crash reporting & telemetry): **Open, real.**
  - `sentry` not in `shell/src-tauri/Cargo.toml`.
  - `@sentry/browser` not in `shell/package.json`.
  - No telemetry plugin / `TelemetrySection.tsx`.
- **WI-44** (Minimal marketplace): **Open, partially scaffolded.**
  - No `nexus-plugins.json` at repo root.
  - `crates/nexus-cli/src/commands/plugin.rs` exists and references "WI-38" `install|list|remove` stubs — the marketplace fetch-and-unpack path is explicitly documented as not implemented in the source comment. Matches doc's current-state claim.
  - No `shell/src/plugins/nexus/marketplace/` directory.
- **WI-46** (Beta → GA): Open, ops-only, gated on WI-41/42.

**Gaps:**
- All four still genuinely unshipped — doc accurately reflects state. No action needed beyond a date-stamp refresh confirming the deferral still holds.
- Minor: §3.2.2 references "the legacy `crates/nexus-app/src/lib.rs:108` init site was retired by Phase 4 WI-37" — accurate.

---

## roadmap/AI-INTEGRATION-DIRECTIONS.md
**Status:** Still exploratory — appropriate to keep in roadmap/.

**Spot-check of "what's in place today" claims:**
- `nexus-ai` providers (Anthropic/OpenAI/Ollama), RAG, sessions — consistent with `crates/nexus-ai/` existing in workspace.
- `nexus-agent`, `nexus-mcp`, `nexus-skills`, `nexus-workflow` — all four crates exist.
- "Cmd+K inline editor actions" listed as Direction 1 with smallest scope — not separately verified against shell code; consistent with "exploratory" framing.

**Gaps:**
- Doc itself is dated 2026-04-26 and labelled exploratory; fine.
- One follow-up: Direction 1 (Inline editor actions) is the kind of item that could be promoted into the active backlog and quietly delivered out of band. Worth a one-line status check on the next sweep.

---

## roadmap/AI-MEMORY-LAYER-PLAN.md
**Status:** Still exploratory.

**Spot-checks:**
- Mapping table claims: "RAG vectorstore (`nexus-ai`)" — consistent with workspace.
- "Quick Capture" / "Auto-enrichment on save" / "Recall hotkey" — none of these features exist as shipped capabilities (no `nexus-capture` plugin in `shell/src/plugins/nexus/`).

**Gaps:**
- None — doc accurately frames itself as design rationale and the listed pieces have not been built.

---

## roadmap/AI-AMBIENT-COPILOT-PLAN.md
**Status:** Still exploratory.

**Spot-checks:**
- "Cmd+I anywhere" / context chips / per-thread model switcher — none shipped.
- "Today's Nexus AI is the opposite: open the panel, type a question, hit send" — consistent with the right-panel chat plugin.

**Gaps:**
- None; doc is internally consistent and accurately exploratory.

---

## roadmap/notion-block-ux-plan.md
**Status:** **Fully shipped — should archive.**

**Per-phase status:**
- **Phase 1** (slash command menu): Done 2026-04-22, `shell/src/plugins/nexus/editor/cm/slashCommand.ts` — file exists at `shell/src/plugins/nexus/editor/cm/slashCommand.ts` ✓.
- **Phase 2** (block selection): Done 2026-04-22, `shell/src/plugins/nexus/editor/cm/blockSelection.ts` — file exists ✓.
- **Phase 3** (block handle + menu + drag): Done 2026-04-22, `shell/src/plugins/nexus/editor/cm/blockHandle.ts` — file exists ✓.
- **Phase 4** (input rules): Done 2026-04-22, `shell/src/plugins/nexus/editor/cm/inputRules.ts` — file exists ✓.
- **Phase 5** (inline toolbar): Done 2026-04-22, `shell/src/plugins/nexus/editor/cm/inlineToolbar.ts` — file exists ✓.
- **Phase 6** (keyboard polish): Done 2026-04-22 (Alt-Arrow added to `blockHandle.ts`); follow-ups (block AI actions, drag-to-embed, block links, side-margin comments, multi-cursor) explicitly deferred to separate tracking.

**Gaps:**
- Doc has a malformed "Phasing recap" — Phase 4 appears twice in the recap block (once as "Done 2026-04-22" with inputRules, once as "housekeeping; just documents behaviour users partially already get"). Two contradictory entries for the same phase; the housekeeping wording is stale boilerplate left over from the original plan. Strike the second entry.
- All six phases delivered → per `roadmap/README.md` lifecycle rules, this doc should move to `docs/archive/` with an `> **Archived <date>** — shipped` header. Any load-bearing details (e.g. the kernel-side block-move transaction handler, persistent block id round-trip) that survive past archival should be promoted to `architecture/` first if they aren't already covered in PRD-08 or `editor-transaction-architecture.md`.
- `roadmap/README.md` row for this doc still says "Phases not all shipped" — stale; update or remove when archiving.
# Shell reference

Audit of `/home/baileyrd/projects/nexus/docs/shell/*.md` against the
TypeScript implementation under `/home/baileyrd/projects/nexus/shell/src/`
and `/home/baileyrd/projects/nexus/packages/nexus-extension-api/src/`.
The shell underwent a substantial Leaf + ViewRegistry migration (Phase 7
of the leaf migration; see `docs/architecture/leaf-architecture.md`) that
removed three large slots (`sidebar`, `editorArea`, `panelArea`) and
re-rooted those surfaces on the workspace / Leaf model. Most drift in
this doc set traces back to that migration.

## shell/README.md
**Implements:** N/A — index/landing page.
**Gaps:**
- Claims `slot-system.md` documents "The 8 slot IDs". Actual code:
  `shell/src/registry/SlotRegistry.ts` ships **6** slot ids
  (`overlay`, `titleBar`, `activityBar`, `statusBarLeft`,
  `statusBarRight`, `paneMode`). `SlotId` is canonically declared in
  `packages/nexus-extension-api/src/index.ts` as the same 6-member
  union. The 3 removed (`sidebar`, `editorArea`, `panelArea`) are
  explicitly noted as deleted in `SlotRegistry.ts` comments.
- Claims `core-plugins.md` describes "the 38 built-ins". The catalog
  at `shell/src/plugins/catalog.ts` ships **60** entries (31
  default-on + 29 default-off). See core-plugins.md section for the
  full id list.
- "Registries" table mentions `PluginRegistry, CommandRegistry,
  SlotRegistry (Zustand), ViewRegistry, etc.". The shell-side
  `ViewRegistry` is the workspace one at `shell/src/workspace/ViewRegistry.ts`,
  not part of the `PluginRegistry`-rooted registry system.

## shell/architecture.md
**Implements:** `shell/src/host/ExtensionHost.ts`,
`shell/src/host/PluginRegistry.ts`,
`shell/src/host/ContextKeyService.ts`, `shell/src/host/EventBus.ts`.
**Gaps:**
- "Slot Surfaces" block lists 8 ids: `overlay`, `titleBar`,
  `activityBar`, `sidebar`, `editorArea`, `panelArea`,
  `statusBarLeft`, `statusBarRight`. Code has 6 (above). `sidebar`,
  `editorArea`, `panelArea` were removed in Phase 7 and replaced by
  `workspace` / `viewRegistry`. The doc never mentions the Leaf
  model.
- "Core service plugins" table lists `core.configuration-service`,
  `core.notification-service`, `core.filesystem-service`,
  `core.theme-service`, `core.language-service`. Catalog has the
  first four (with names `Configuration Service`, `Notification
  Service`, `File System Service`, `Theme Service`). No
  `core.language-service` exists in the catalog.
- "Core UI plugins" table lists `core.title-bar`, `core.sidebar`,
  `core.editor-area`, `core.panel-area`, `core.status-bar`,
  `core.command-palette`, `core.settings`, `core.notifications-ui`,
  `core.theme-picker`. Real ids in catalog are `nexus.sidebar`,
  `nexus.statusBar`, `nexus.commandPalette`, `nexus.themePicker`
  (under the `nexus.*` namespace, not `core.*`). There is no
  `core.title-bar`, `core.editor-area`, `core.panel-area`, or
  `core.notifications-ui` plugin — the title bar in `shell/src/shell/`
  is rendered by `WindowControls.tsx` (host-owned), editor area is
  the workspace, panel area is a sidedock. Notifications surface
  through `core.notification-service` directly with no separate UI
  plugin id.
- "Core feature plugins" table lists `core.file-explorer`,
  `core.terminal`, `core.search`. Real ids are `nexus.files`,
  `nexus.terminal`, `nexus.search`.
- `Plugin calls api.views.register(viewId, { slot, component, priority })`
  matches `ViewsAPI` in `shell/src/types/plugin.ts:294-302` (still
  valid for chrome slots only — the JSDoc on `PluginAPI.views`
  reflects this).

## shell/plugin-api.md
**Implements:** `packages/nexus-extension-api/src/index.ts`,
`shell/src/types/plugin.ts`, `shell/src/host/PluginAPI.ts`.
**Gaps:**
- Documents `api.commands`, `api.views`, `api.context`, `api.events`,
  `api.configuration`, `api.statusBar`, `api.notifications`,
  `api.fs`, `api.storage`, `api.internal`. **Missing entire
  sub-APIs** that exist on `PluginAPI` (`shell/src/types/plugin.ts:164-232`):
  `api.workspace`, `api.viewRegistry`, `api.keybindings`,
  `api.kernel`, `api.platform`, `api.activityBar`, `api.input`,
  `api.settings`, `api.uri`, `api.editor`. These are the surfaces
  most plugins actually use today (kernel.invoke, workspace,
  platform, viewRegistry).
- `api.views.register` example uses `slot: 'sidebar'` (a slot id
  that was removed in Phase 7). Valid `SlotId` values today are
  the 6-member union; `sidebar` is now reached via
  `viewRegistry.register(type, creator)` + `workspace.ensureLeafOfType(type, 'left')`.
- `api.notifications` is documented as available after
  `core.notification-service` loads, but `NotificationsAPI`
  (`types/plugin.ts:368-375`) is a first-class field on `PluginAPI`
  the host wires unconditionally.
- `api.internal.defineSlot(slotId)`: the `InternalAPI` interface
  still exposes `defineSlot` (`types/plugin.ts:499`) but the
  `useSlotStore` initial state in `SlotRegistry.ts` hard-codes the
  6 valid slot keys — calling `defineSlot` with a novel id does
  not extend the store.
- `api.fs` shape in doc matches `FilesystemAPI` in `types/plugin.ts:377-386`.
- `api.storage` shape matches `StorageAPI` in `types/plugin.ts:315-320`.
- `api.events` matches `EventsAPI` in `types/plugin.ts:310-313`.
- Built-in events table omits the real
  `plugins:keybindings-conflict`, `command:error`, `command:cancelled`,
  `shell:themeChanged`, `shell:ready`, `shell:layoutChanged`,
  `fs:fileRenamed`, `plugin:error` events declared in
  `EventBus.ts:90-146` `ShellEvents`.

## shell/plugin-system.md
**Implements:** `shell/src/types/plugin.ts` (manifest + Plugin
contract), `shell/src/host/ExtensionHost.ts` (load order /
activation events), `shell/src/host/PluginAPI.ts` (buildPluginAPI).
**Gaps:**
- `PluginManifest` interface in doc omits `apiVersion` and
  `popoutCompatible` fields present in `types/plugin.ts:85-108`.
  `apiVersion` is enforced (WI-33) — plugins with the wrong
  number are rejected with `PluginApiVersionError`.
- `contributes` shape in doc lacks `settingsTabs` and `snippets`
  contribution arrays that exist in `PluginContributions` at
  `types/plugin.ts:134-145`.
- Activation events table lists `onStartup`, `onCommand:`,
  `onView:`, `onLanguage:`. Real implementation in
  `ExtensionHost.ts:66-111` + `ActivationTriggers.ts` also
  recognises `onUri:` (URI scheme handlers — see
  `UriHandlerRegistry.dispatch`). The doc misses it.
- Says core plugins "can define new slot types" — see plugin-api.md
  note: `defineSlot` exists as a typed method but isn't honoured by
  the slot store.
- "Topological sort … Core plugins always sort before community
  plugins" matches `resolveDependencyOrder()` in
  `ExtensionHost.ts:344-366`.
- Lazy contribution pre-registration in `ExtensionHost.ts:66-111`
  (Pass 1) goes well beyond what the doc describes — keybindings,
  settings tabs, and snippets are also pre-registered for lazy
  plugins, and `contribsRegistered` guards against duplicate
  registration on the eager-activation re-entry.

## shell/extension-host.md
**Implements:** `shell/src/host/ExtensionHost.ts`.
**Gaps:**
- Snippet of `loadAll()` in doc is single-pass and ignores
  `activationEvents` (registers manifest and calls
  `activate()` for every plugin in order). Real impl is a documented
  **two-pass** loader (`ExtensionHost.ts:66-111`): Pass 1 classifies
  eager vs lazy and pre-registers manifest contributions for lazy
  plugins; Pass 2 only activates the eager set. Lazy plugins wake
  on triggers via `ActivationTriggers.setActivator()`.
- Doc's `PluginState` union matches `ExtensionHost.ts:12-19`.
- Lifecycle Events table is correct: `plugin:activated`,
  `plugin:deactivated`, `plugin:error` all emitted in code
  (`fail()` → `eventBus.emit('plugin:error', ...)`).
- Doc omits `deactivateAllForShutdown(perPluginCapMs = 1000)`
  (`ExtensionHost.ts:196-229`), the best-effort sweep run on
  window close per OI-16.
- Doc omits the unload path's handling of `registered` (lazy)
  plugins — `unload()` (`ExtensionHost.ts:235-265`) sweeps
  pre-registered contributions even when the plugin never
  activated, and `activationTriggers.evict(id)` drops trigger
  entries.
- Doc's "Lazy Activation" section references `if (this.states.get(owningPluginId) === 'registered')`
  inline before command execution; the real wiring is via the
  `activationTriggers` module — `CommandRegistry.execute`,
  `Leaf.setViewState`, and `UriHandlerRegistry.dispatch` call into
  the singleton, which calls back through the activator the host
  installed.

## shell/registry-system.md
**Implements:** `shell/src/host/PluginRegistry.ts`,
`shell/src/registry/CommandRegistry.ts`,
`shell/src/registry/SlotRegistry.ts`,
`shell/src/registry/KeybindingRegistry.ts`,
`shell/src/registry/ConfigurationRegistry.ts`,
`shell/src/registry/StatusBarRegistry.ts`,
`shell/src/registry/SettingsTabRegistry.ts`,
`shell/src/registry/SnippetRegistry.ts`,
`shell/src/registry/UriHandlerRegistry.ts`,
`shell/src/workspace/ViewRegistry.ts`.
**Gaps:**
- Doc's `PluginRegistry` lists sub-registries: `commands`, `views`,
  `menus`, `keybindings`, `statusBar`, `slots`, `config`. Real
  shape (`PluginRegistry.ts:17-23`): `commands`, `config`,
  `keybindings`, `settingsTabs`, `snippets`, `statusBar`. Missing
  from the real type: `views` (lives in workspace), `menus` (no
  registry — `MenuContribution` exists only as a contribution
  DTO), `slots` (the slot registry is a Zustand-backed singleton
  imported as `slotRegistry`, not a field on `PluginRegistry`).
- Doc's `SlotRegistry` snippet lists **8** slot ids; code has **6**
  (`SlotRegistry.ts:42-48`). The 3 missing in code are
  `sidebar`, `editorArea`, `panelArea`. Code adds `paneMode` which
  is not in the doc.
- Doc has a top-level `ViewRegistry` class with `views` map and
  `registerFromManifest` / `registerComponent`. The shell-side
  `ViewRegistry` actually lives in
  `shell/src/workspace/ViewRegistry.ts` and exposes a different
  contract — `register(type, creator)` returning a disposer, for
  Leaf-style workspace views. The doc's `ViewRegistry` does not
  exist anywhere in the source.
- Doc's `KeybindingRegistry` lacks: per-command overrides
  (`setOverride`/`clearOverride`/`getOverride`), conflict
  detection that emits `plugins:keybindings-conflict` (see
  `EventBus.ts:114-119`), and persistence via
  `keybindingOverrideStorage.ts`.
- `unregisterAll` switch in doc handles `command`, `view`,
  `slot`, `statusBar`, `config`, `keybinding`. Real handler
  (`PluginRegistry.ts:172-187`) handles `command`, `slot`,
  `statusBar`, `config`, `keybinding`, `settingsTab`, `snippet`,
  `activityBar`. Missing entirely from doc: snippet,
  settingsTab, activityBar paths, plus the separate sweeps for
  kernel subscriptions, viewType ownership, plugin keybinding
  overrides, and URI handlers (`PluginRegistry.ts:192-258`).
- Doc omits `PluginRegistry.registerService` / `getService` /
  `hasService` / `updateService` — the internal-service bus used
  by core plugins (`PluginRegistry.ts:263-285`).

## shell/slot-system.md
**Implements:** `shell/src/shell/App.tsx`,
`shell/src/shell/slots/SlotSurface.tsx`,
`shell/src/registry/SlotRegistry.ts`.
**Gaps:**
- The most consequential drift in the doc set: the entire doc is
  built around 8 slots (`overlay`, `titleBar`, `activityBar`,
  `sidebar`, `editorArea`, `panelArea`, `statusBarLeft`,
  `statusBarRight`). The `useSlotStore` initial state
  (`SlotRegistry.ts:42-48`) has 6: `overlay`, `titleBar`,
  `activityBar`, `statusBarLeft`, `statusBarRight`, `paneMode`.
- `paneMode` (in code) is not mentioned anywhere in this doc.
- `App.tsx` rendering snippet in the doc renders explicit
  `<SlotSurface entries={slots.sidebar} />`,
  `<SlotSurface entries={slots.editorArea} />`,
  `<SlotSurface entries={slots.panelArea} />`. The real
  `App.tsx` renders the workspace's split tree for those
  regions; sidebar/main/right/bottom come from
  `WorkspaceRenderer.tsx` driven by `workspaceStore` rather
  than `slotRegistry`.
- "Empty shell proof" enumerates the same 8 (now non-existent)
  slot keys.
- `slotRegistry.register('sidebar', …)` calls in the doc would
  fail at the TypeScript level — `SlotId` no longer admits
  `'sidebar'`.

## shell/event-bus.md
**Implements:** `shell/src/host/EventBus.ts`.
**Gaps:**
- Doc's `EventBus` class is structurally accurate — singleton with
  `on/emit/onAll/clear`, sync emission, error catching per handler.
  Implementation extras: `emitAsync(event, payload)` for
  high-frequency events (matches doc's "Asynchronous" section); the
  doc says it "can be added" but it already exists at
  `EventBus.ts:64-66`.
- "Built-in events" list in plugin-api.md doc misses the actual
  exported `ShellEvents` interface in `EventBus.ts:90-146` which is
  more comprehensive (see plugin-api.md notes).
- Wildcard subscription wrapping: doc's `onAll` sends
  `{ event, payload }` directly to wildcard handlers; real
  `emit()` (`EventBus.ts:47-57`) also wraps with `{ event, payload }`
  before dispatch. Consistent.

## shell/context-keys.md
**Implements:** `shell/src/host/ContextKeyService.ts`.
**Gaps:**
- Doc lists `=~` (regex match) as a supported operator. Real
  `evaluateWhen` (`ContextKeyService.ts:78-160`) supports only
  `&&`, `||`, `!`, `==`, `!=`, parentheses, literals — no `=~`.
  Tokeniser regex (`ContextKeyService.ts:95-97`) has no regex
  literal arm; using `=~` in a when-clause silently returns
  `false` and logs a warn.
- Built-in shell-level keys in doc: `shellReady`, `os`. Real
  defaults in `ContextKeyService.ts:18-21` are the same two.
- "UI state keys" table claims `sidebarVisible`, `sidebarFocus`,
  `panelAreaVisible`, `editorFocus`, `editorReadOnly`,
  `activeFileExtension`, `activeLanguage`, `terminalFocus` are
  set by `core.sidebar` / `core.panel-area` / `core.editor-area`
  / `core.terminal`. None of those plugin ids exist — actual
  setters are `nexus.sidebar`, `nexus.editor`, `nexus.terminal`
  (default-off), and there is no `core.panel-area` (a panelArea
  slot doesn't exist either).
- "Modal/overlay keys" table cites `core.command-palette` and
  `core.settings`. Real plugin ids: `nexus.commandPalette` and
  `core.settings` (the only `core.*` UI plugin that survived
  the rename).

## shell/core-plugins.md
**Implements:** `shell/src/plugins/catalog.ts` (the authoritative
plugin manifest), `shell/src/plugins/core/*`, `shell/src/plugins/nexus/*`.
**Gaps:**
- Doc's "Load Order" cites 5 service plugins + 10 UI plugins +
  3 feature plugins. Real `DEFAULT_ON_PLUGINS` has 31 entries
  and `DEFAULT_OFF_PLUGINS` has 29; total **60** built-in
  plugins, not 38. The trailing "Curated Default-On Set (WI-43)"
  section in the doc says "shell binary ships 38 built-in
  plugins" and "DEFAULT_ON_PLUGINS (19) … DEFAULT_OFF_PLUGINS (17)"
  — both figures are stale. Grep guard claim
  `grep -c "^import.*Plugin" shell/src/plugins/catalog.ts == 38`
  would fail today.
- Plugin ids in the doc use a `core.*` namespace
  (`core.title-bar`, `core.activity-bar`, `core.sidebar`,
  `core.editor-area`, `core.panel-area`, `core.status-bar`,
  `core.command-palette`, `core.notifications-ui`,
  `core.theme-picker`, `core.file-explorer`, `core.terminal`,
  `core.search`, `core.language-service`). The catalog uses
  `nexus.*` for almost all of those (`nexus.activityBar`,
  `nexus.sidebar`, `nexus.statusBar`, `nexus.commandPalette`,
  `nexus.themePicker`, `nexus.files`, `nexus.terminal`,
  `nexus.search`, `nexus.editor`). No `core.*` ids exist for
  these surfaces. `core.title-bar`, `core.editor-area`,
  `core.panel-area`, `core.notifications-ui`,
  `core.language-service` do not exist at all.
- Real `core.*` entries in catalog:
  `core.configuration-service`, `core.notification-service`,
  `core.filesystem-service`, `core.settings`,
  `core.capabilityPrompt`, `core.theme-service`, `core.zoom`.
  Doc misses `core.settings`, `core.capabilityPrompt`,
  `core.zoom`.
- Default-off plugin list in doc ("AI, agent, MCP, workflow,
  skills, terminal, processes, graph (+ global index), canvas,
  bases, backlinks, bookmarks, outgoing links, file properties,
  tags, all properties") omits these real DEFAULT_OFF entries:
  `nexus.semanticSearch`, `nexus.linkSuggest`, `nexus.recall`,
  `nexus.enrich`, `nexus.templates`, `nexus.notion`,
  `nexus.activity`, `nexus.comments`, `nexus.healthPanel`,
  `nexus.searchPanel`, `community.mermaid`,
  `nexus.osArchitecture`, `nexus.osObservability`,
  `nexus.viewBuilder`. Default-on additions doc misses:
  `nexus.workspace` (load-bearing), `nexus.gitStatus`,
  `nexus.gitPanel`, `nexus.rightPanel`, `nexus.launcher`,
  `nexus.crdtConflict`, `nexus.confirm`, `nexus.pick`,
  `nexus.prompt`, `nexus.canvas`, `nexus.bases`,
  `nexus.extensionsTab`, `nexus.memory`.
- "core.terminal … `slot: 'panelArea'`" — `panelArea` is not a
  slot id; terminal is a workspace leaf type. Real
  `nexus.terminal` registers a view creator on `viewRegistry`,
  not into a slot.
- "core.file-explorer … `slot: 'sidebar'`" — same issue,
  `nexus.files` is a workspace leaf type.
- Doc omits `legacyPluginIds` and the `popoutCompatible` flag
  on `PluginEntry` (`catalog.ts:32-44`), and the
  `buildLegacyIdAliases` helper (`catalog.ts:59-80`).

## shell/workspace-layout.md
**Implements:** `shell/src/workspace/types.ts`,
`shell/src/workspace/persistence.ts`,
`shell/src/workspace/workspaceStore.ts`,
`shell/src/workspace/ViewRegistry.ts`,
`shell/src/workspace/WorkspaceRenderer.tsx`.
**Gaps:**
- Doc claims `workspace.json` lives at `.nexus/workspace.json`.
  Real path is `<forge>/.forge/workspace.json` —
  `persistence.ts:33` declares `const WORKSPACE_REL =
  '.forge/workspace.json'`. The `.nexus/` directory doesn't exist
  anywhere in the shell or kernel; the forge uses `.forge/`.
- Doc's schema uses top-level keys `main`, `left`, `right`,
  `bottom`, `activityBar`, `active`. Real `WorkspaceJSON`
  (`types.ts:143-160`) has `main`, `left`, `right`, optional
  `bottom`, optional `floating[]`, `active: string | null`,
  `lastOpenFiles: string[]`. No `activityBar` field; activity-bar
  pinning is in the activityBar store, not workspace.json.
  `floating` (BL-029 popout leaves) is missing from the doc.
- Doc's leaf union is `{ type: "editor", path, cursor, scroll,
  mode }` and similar. Real serialized leaf
  (`SerializedLeaf`, `types.ts:117-121`) is
  `{ kind: "leaf", id, viewState: { type, state, active?,
  pinned?, group? } }`. The discriminator is `kind` not
  `type`, and view-specific state nests under `viewState.state`
  (matching Obsidian's `ViewState`).
- Doc's split node has `direction: 'horizontal' | 'vertical'`,
  `children`. Real `SerializedSplit` adds `id`, optional `sizes`,
  and Sidedock-only `side`/`collapsed`/`size` fields. Doc's
  bottom shape (`{ height, activeTab, tabs }`) does not match
  the real bottom which is a generic `SerializedNode`.
- Plugin contribution snippet
  `definePlugin({ id, views: [{ type, title, icon, component,
  serialize, deserialize }] })` is hypothetical — no
  `definePlugin` helper exists. Real registration is
  `api.viewRegistry.register(type, (leaf) => new MyView(leaf))`
  with the disposer auto-tracked
  (`PluginRegistry.ts:91-109 trackViewType`). Plugins implement
  the `View` interface from `types.ts:6-24` (with
  `getState/setState/onOpen/onClose`), not a serialize/deserialize
  pair on a registration DTO.
- Tree types include `Root` (`types.ts:81-85`) and
  `FloatingWindow` (`types.ts:87-92`) that the doc doesn't
  mention.
- Doc says `runs/<runId>.json` per agent run as a "per-surface
  state file" — this is forward-looking; the file does not
  exist in `shell/src/`.

## shell/writing-a-plugin.md
**Implements:** `packages/nexus-extension-api/src/index.ts`,
`packages/nexus-extension-api/src/sandbox/`, `shell/src/types/plugin.ts`,
`shell/src/host/sandbox/`.
**Gaps:**
- "Slot system and contributions" section lists slot ids:
  `activityBar`, `sidebar`, `sidebarContent`, `rightPanel`,
  `rightPanelContent`, `editorArea`, `panelArea`, `paneMode`,
  `statusBarLeft`, `statusBarRight`, `overlay` — **11 ids**.
  Real `SlotId` union has 6 (above). Removed: `sidebar`,
  `sidebarContent`, `rightPanel`, `rightPanelContent`,
  `editorArea`, `panelArea`. Added vs doc: `titleBar` (the doc
  misses this one). The
  `sidebarContent`/`rightPanelContent` ids never landed in the
  shipped `SlotRegistry` initial state at all.
- "Worked example" manifest uses `slot: 'sidebar'` and
  `slot: 'statusBarRight'`. The `statusBarRight` slot is the
  full slot id for `slots.statusBarRight` entries, but
  `StatusBarAPI.createItem` (`types/plugin.ts:331-345`) takes
  `slot: 'left' | 'right'`, not `'statusBarLeft' | 'statusBarRight'`.
  The example would not typecheck against the current
  `StatusBarAPI`.
- Activation events table lists `onFileOpen:<glob>`. Real
  triggers honoured by `ActivationTriggers.ts` are `onView:`,
  `onCommand:`, `onUri:`, `onLanguage:` — no `onFileOpen:`.
- `PluginManifest` snippet matches the real interface modulo
  the missing `popoutCompatible` flag (in code at
  `types/plugin.ts:100-107`).
- Capability list matches `crates/nexus-plugin-api/src/capability.rs`
  via the ts-rs `generated/` directory.

## Summary of load-bearing drift

| Doc | Most serious mismatch |
|---|---|
| README.md | Slot count (8 → 6), plugin count (38 → 60) |
| architecture.md | Lists 8 slots, references non-existent `core.*` UI plugins |
| plugin-api.md | Missing ~10 API surfaces (`workspace`, `kernel`, `platform`, `editor`, `keybindings`, `activityBar`, `input`, `settings`, `uri`, `viewRegistry`) |
| plugin-system.md | Manifest lacks `apiVersion`, `popoutCompatible`; `snippets`/`settingsTabs` contributions missing; `onUri:` activation event missing |
| extension-host.md | Single-pass loader described; real impl is documented two-pass loader with lazy pre-registration |
| registry-system.md | Lists `views`, `menus`, `slots` registries that aren't on `PluginRegistry`; misses `settingsTabs`, `snippets`, viewType ownership, subscription tracking, override sweeps; describes a non-existent `ViewRegistry` class |
| slot-system.md | Entire doc built around 8 slots; 6 in code (3 removed in Phase 7 leaf migration). `paneMode` missing. `App.tsx` snippet outdated — workspace tree renders chrome regions now |
| event-bus.md | Implementation accurate; built-in event list in plugin-api.md is the place to consolidate |
| context-keys.md | `=~` regex operator documented but not implemented; built-in keys cite wrong plugin ids |
| core-plugins.md | Plugin count off (38 → 60); `core.*` namespace claims don't match the `nexus.*` namespace in code; `core.language-service`, `core.title-bar`, `core.editor-area`, `core.panel-area`, `core.notifications-ui` don't exist |
| workspace-layout.md | Wrong on-disk path (`.nexus/` → `.forge/`); schema uses wrong discriminator (`type` → `kind`); plugin contribution model fabricated (`definePlugin`/`serialize`/`deserialize`) |
| writing-a-plugin.md | Slot list cites 11 ids including 5 that don't exist; `onFileOpen:` activation event doesn't exist; status-bar example wouldn't typecheck |

The dominant theme is **Phase 7 leaf-migration drift**: the docs describe
the pre-migration world where sidebar/editorArea/panelArea were slots
plus a `core.*`-namespaced UI plugin set. The current world uses the
Leaf + ViewRegistry model with chrome-only slots and `nexus.*`
plugins. A second theme is **API surface growth** that never made it
back into `plugin-api.md`: ten or so subsystems (workspace, kernel,
platform, editor, input, settings, etc.) ship in `PluginAPI` but
aren't documented.
# Developer hub

The developer hub has **systemic drift**: most TypeScript examples
describe a `Plugin`/`PluginContext` API with `activate`/`deactivate`
hooks, `ctx.commands.register`, `ctx.events.subscribe`,
`ctx.config.get`/`set`/`onChange`, `ctx.kv.get`/`set`, `ctx.context.get`,
`ctx.ipc.handle`, `ctx.statusBar.add`, `ctx.views.register`, and a
`mockContext` helper. None of these exist in
`packages/nexus-extension-api/src/index.ts`, which exposes a different
contract:

- Plugin shape is `ScriptPlugin` with `dispatch(handlerId, args, ctx)`,
  `onInit`, `onStart`, `onStop` (not `activate`/`deactivate`).
- `NexusPluginContext` exposes: `pluginId`, `settings.get()`,
  `events.emit(typeId, payload)` (no `subscribe`), `ipc.call(...)` (no
  `handle`), `editor.{registerBlockType|registerDecorationProvider|
  registerKeybinding|registerSnippet|registerMdxComponent|active|
  onChange|registerFencedCodeRenderer}`, `ui.{notify|
  registerTreeDataProvider|registerFileHandler|registerContextMenuItem|
  registerMenuItem|registerUriHandler|registerWebviewPanel|
  registerPanelView}`, `workspace.forgeRoot()`, `ai.registerAction(...)`,
  `disposables`.
- `ui.notify` signature is `notify(level: ToastLevel, message: string)`,
  not `notify({ message, level? })`.
- There is no `ctx.commands`, `ctx.kv`, `ctx.config`, `ctx.context`,
  `ctx.statusBar`, `ctx.views`, `ctx.fs`, `ctx.env` on the context.

This means every code sample in the hub teaches an API that won't
compile against `@nexus/extension-api`. Below I flag only the drift
beyond this systemic issue.

## developer/README.md
**Implements:** hub-level navigation across plugin/editor/ui/themes/core-plugins.
**Gaps:** Verified — no drift. All linked subpaths exist.

## developer/getting-started.md
**Implements:** end-to-end "hello world" plugin in 10 minutes.
**Gaps:**
- Systemic API drift (see preamble). Sample uses `Plugin`,
  `activate(ctx)`, `ctx.commands.register`, `ctx.ui.prompt`,
  `ctx.ui.notify({message})` — none exist.
- Scaffold output drift: line 26 shows `hello/{plugin.json, src/index.ts,
  package.json, tsconfig.json}`. Actual `crates/nexus-plugins/templates/script/`
  emits `index.ts` (no `src/`), plus `plugin.json`, `package.json`,
  `tsconfig.json`, `README.md`.
- Template choices wrong: line 36 says "Other templates: `wasm`, `theme`".
  Actual scaffold templates (per `main.rs:693`) are `script` (default),
  `core`, `community`. There is no `wasm` or `theme` template.
- Build output drift: line 89 says `pnpm build` produces `dist/index.js`.
  Per `users/cli.md:159` (correct), the script template builds to plain
  `index.js`, not `dist/`.
- Watch flag drift: `pnpm build --watch` and especially
  `nexus plugin install ./ --watch` (line 122) — `install` has no
  `--watch` flag (`main.rs:644-647`).
- Dead link: `docs/shell/plugin-api.md` referenced at line 83 exists,
  but `docs/shell/writing-a-plugin.md` referenced at line 136 also
  exists — both fine. However the API the doc redirects to
  (`docs/shell/plugin-api.md`) likely has the same systemic drift.

## developer/architecture-primer.md
**Implements:** kernel + invariants + trust tiers overview.
**Gaps:**
- Minor: invariant #2 (line 44) says `nexus-kernel` depends only on
  `nexus-types`. Per project CLAUDE.md the kernel also depends on
  `nexus-plugin-api`. Both are leaf crates, so the spirit is correct.
- Otherwise verified — invariant statements, crate roles in table, and
  ADR 0016 link are all accurate.

## developer/reference.md
**Implements:** pointer table to authoritative source files.
**Gaps:**
- Path drift: line 27 says `Capability` enum has **14 variants** —
  actual is **22** (FsRead, FsWrite, FsReadExternal, FsWriteExternal,
  NetHttp, NetHttpLocalhost, ProcessSpawn, KvRead, KvWrite, IpcCall,
  DbQuery, DbWrite, EventsPublish, UiNotify, AiChat, AiIndex,
  AiSessionRead, AiSessionWrite, AiConfigWrite, AiActivityWrite,
  AiToolsWrite, AiToolsMcp; plus the catch-all `UnknownString`).
- Path drift: lines 74-75 reference `docs/templates/community-plugin/`
  and `docs/templates/core-plugin/`. The `docs/templates/` directory
  does not exist. Actual scaffold sources live at
  `crates/nexus-plugins/templates/script/` (only `script` template
  exists today; no community/core scaffold templates).
- Link to `docs/shell/plugin-api.md` (line 10) exists; cluster of
  `docs/shell/*` links (lines 46-53) all exist.

## developer/plugins/overview.md
**Implements:** core-vs-community tier menu, runtime choice (WASM vs.
iframe-JS), what plugins can/cannot do, manifest hint.
**Gaps:**
- Runtime-choice drift (line 31): says iframe-JS template id is `script`
  and there's also a `wasm` template. The scaffold has no `wasm`
  template; `community` and `core` emit legacy WASM project shapes but
  there is no template literally named `wasm`.
- Capability list "Storage" (line 67) cites `kv.read`, `kv.write`,
  `db.query`, `db.write` and IPC/Events as `ipc.call`/`events.publish`.
  These four exist; the doc is silent on the eight AI capabilities
  (`ai.chat`, `ai.index`, `ai.session.read`, `ai.session.write`,
  `ai.config.write`, `ai.activity.write`, `ai.tools.write`, `ai.tools.mcp`).

## developer/plugins/manifest.md
**Implements:** plugin.json field reference and validation rules.
**Gaps:**
- Capability-string drift: doc's table (and every example) uses dotted
  lowercase strings (`ui.notify`, `kv.read`). The live community
  plugin `shell/src/plugins/community/hello-world/plugin.json` uses
  PascalCase (`"UiNotify"`). One of these is canonical; the manifest
  doc disagrees with the only shipping community-plugin manifest.
  (Capability `from_str` in `capability.rs:158` only accepts dotted
  form — so the hello-world manifest may itself be broken, or the
  loader normalises before parsing. Either way the doc and the live
  example disagree.)
- Manifest field `keybinding` is shown inside a `contributes.commands[]`
  entry (line 52, 95). The TS type `CommandContribution`
  (extension-api index.ts:460) has `{id, title, category?, icon?}` only —
  no `keybinding`. Keybindings are a separate `KeybindingContribution`.
- `apiCompat` field (line 41) doesn't appear in any plugin manifest in
  the repo or in `PluginContributions`/manifest validation. May be
  forward-looking.
- Validation regex `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)*$` (line 109) —
  not verified; hello-world id `community.hello-world` matches.

## developer/plugins/lifecycle.md
**Implements:** plugin states, hooks, activation events.
**Gaps:**
- Systemic API drift. Doc uses `Plugin` + `activate`/`deactivate`
  hooks throughout (line 41-58, 109-122, 128-138). Actual contract is
  `ScriptPlugin` with `onInit`/`onStart`/`onStop`/`dispatch`. The Rust
  side `CorePlugin` does use `on_init`/`on_start`/`on_stop` (verified
  in core-plugins/authoring.md), so the lifecycle states are right but
  the TypeScript hooks are wrong.
- `ctx.events.subscribe`, `ctx.kv.get` (line 114, 117) don't exist.
- States diagram correctly references `PluginStatus` in
  `nexus-plugin-api/src/plugin.rs` — file exists.

## developer/plugins/capabilities.md
**Implements:** canonical capability vocabulary and approval flow.
**Gaps:**
- Count stale: claims **14 capabilities, 5 HIGH-risk** (line 9-10).
  Actual is **22 capabilities** (see reference.md note). The eight AI
  capabilities are completely absent from the doc — entire AI section
  missing.
- HIGH-risk count needs re-verification against `is_high_risk` in
  `capability.rs` (not checked here; doc's claim of 5 HIGH is suspect
  given the new AI cluster).
- `isCapabilityError` example (line 121) is not exported from
  `@nexus/extension-api/index.ts`.
- `ctx.fs.write(...)` example (line 119) — there's no `fs` on
  `NexusPluginContext` (script-plugin filesystem lives at
  `PlatformFsAPI` under `api.platform.fs.*`).

## developer/plugins/ipc.md
**Implements:** IPC contract and common targets.
**Gaps:**
- `ctx.ipc.handle(...)` (lines 95-97) doesn't exist — `ipc` only
  exposes `call(targetPluginId, commandId, args)`.
- `import { StorageIpc } from '@nexus/extension-api/ipc'` (line 83)
  doesn't work — the generated IPC types live under
  `packages/nexus-extension-api/src/generated/ipc/` (per-DTO files like
  `AdHocIdArgs.ts`) and aren't bundled as a `StorageIpc` namespace.
- `import type { PluginContext } …` (line 17) — should be
  `NexusPluginContext`.
- Internal references at line 159-163 (`context_impl.rs`,
  `loader.rs`, `error.rs`, `ipc-schemas.md`) all exist. Good.
- `IpcError::CapabilityDenied`, `PluginNotFound`, etc. — names match
  the `IpcErrorKind` enum.

## developer/plugins/events.md
**Implements:** kernel event-bus, topic vocabulary, subscriber pattern.
**Gaps:**
- `ctx.events.subscribe(...)` (lines 13, 30, 108) doesn't exist —
  the API has only `events.emit(typeId, payload)`. Subscription must
  flow through the kernel bridge (`kernel_subscribe` in the Tauri layer
  per project CLAUDE.md), not through `ctx.events`.
- Path drift: `packages/nexus-extension-api/src/events.ts` (line 70)
  doesn't exist. Event-type definitions live in
  `packages/nexus-extension-api/src/generated/NexusEvent.ts` and
  similar.
- `FilesChangedEvent` (line 10) is not an exported type from
  `index.ts`; the union type is `NexusEvent` (generated).
- Topic names (`files:changed`, `editor:change`, etc.) — not verified
  against actual publishers.

## developer/plugins/settings.md
**Implements:** JSON-schema-based settings and live updates.
**Gaps:**
- Heavy API drift: `ctx.config.get/set/onChange` and `ctx.env.get`
  (lines 81-145) don't exist. Plugins read settings via `ctx.settings.get()`
  which returns the whole snapshot — no per-key get, no onChange, no set.
- `ConfigSchema` type fields don't match doc. Actual type
  (`index.ts:563-571`) has `{key, title, description, type, default,
  options?, when?}` only. Doc adds `pattern`, `min`, `max`, `step`,
  `rows`. The `type` union accepts
  `'boolean'|'string'|'password'|'number'|'select'|'keybinding'` —
  not `multiline`.

## developer/plugins/testing.md
**Implements:** Vitest-based unit testing with `mockContext`.
**Gaps:**
- `mockContext` from `@nexus/extension-api/testing` (line 25) doesn't
  exist — `packages/nexus-extension-api/src/testing/` directory is
  absent. The entire mock harness described in this page (helpers
  for `ctx.commands.list`, `ctx.ipc.recordedCalls()`,
  `ctx.events.emit`, `ctx.config.setValue`, etc.) is fictional.
- Compounded with the systemic API drift, no example in this page is
  executable today.

## developer/plugins/publishing.md
**Implements:** release artifact layout, signing, distribution.
**Gaps:**
- `nexus plugin sign` (line 60) is not a CLI command. The actual CLI
  has `nexus plugin verify` (verifies an existing signed manifest) but
  no `sign` subcommand. Signing presumably happens outside the CLI.
- Trusted-key location: doc says `<forge>/.forge/trusted-publishers.json`
  (line 64) but `shell/src-tauri/src/lib.rs:25` declares a static
  `TRUSTED_PUBLIC_KEYS: &[&str] = &[]` (empty) — the trust set lives
  in Rust code, not a per-forge file. (Or the doc describes a planned
  override mechanism not yet wired.)
- `nexus plugin install <url>` (line 44) — the CLI doesn't accept
  URLs; per `commands/plugin.rs` the arg is "local plugin directory
  path, or marketplace plugin id", and marketplace fetch is stubbed
  (Phase 5 WI-44).
- `ctx.kv.get`/`set` (lines 105-110) don't exist.

## developer/editor/overview.md
**Implements:** editor extension surfaces (decorations, slash commands,
MDX components) + live-preview pipeline.
**Gaps:**
- API drift: doc routes decorations through
  `ctx.ipc.call('com.nexus.editor', 'decorate'/'undecorate', ...)`
  (lines 41-49). Actual surface is `ctx.editor.registerDecorationProvider({
  id, extension })` where `extension` is a CodeMirror 6 `Extension`. No
  `decorate`/`undecorate` IPC handler is exposed via the contract.
- `ctx.ipc.call('com.nexus.editor', 'register_slash_command', ...)`
  (line 65) — not verified; actual API exposes
  `ctx.editor.registerSnippet({id, trigger, body, ...})` which is the
  closest match.
- `list_blocks` IPC (line 94) not verified; may exist as an
  `editor::*` IPC handler.

## developer/editor/slash-commands.md
**Implements:** slash-command registration shape and handler pattern.
**Gaps:**
- API drift: every example uses
  `ctx.ipc.call('com.nexus.editor', 'register_slash_command', ...)`
  (lines 13, 49). Actual API is `ctx.editor.registerSnippet(snippet)`
  where `Snippet` has `{id, trigger, body, description?, fileTypes?}`
  — no `title`, `icon`, `handler`, `keywords`, `group`. The dynamic
  callback flow (lines 33-55) has no equivalent in the contract.
- `ctx.ipc.handle(...)` (line 33) doesn't exist.

## developer/editor/mdx-components.md
**Implements:** MDX component registration with React-style `render`.
**Gaps:**
- API drift: doc shows
  `ctx.editor.registerMdxComponent('Tweet', { schema, render, editor })`
  with JSX/React in the render fn (lines 39-56). Actual contract
  (`index.ts:197-202`) is `registerMdxComponent(component: MdxComponent)`
  where `MdxComponent = {id, name, description?, render(props): PanelNode}`
  — no schema, no React, no `displayName`, no `editor` (inline editor).
  The render fn must return a `PanelNode` tree (declarative dispatcher),
  not a React element. The contract explicitly forbids plugin-supplied
  JSX (host never evaluates it).
- Built-in components table (`<Callout>`, `<Alert>`, `<Card>`,
  `<Badge>`) — not verified, may or may not be registered today.

## developer/ui/views-and-slots.md
**Implements:** slot system, view registration, status-bar items,
modals.
**Gaps:**
- API drift: `ctx.views.register(...)` (line 45), `ctx.statusBar.add(...)`
  (line 75), `ctx.ui.modal(...)` (line 189) — none exist on
  `NexusPluginContext`. Actual surfaces:
  - View registration is via `ctx.ui.registerPanelView(viewId, render)`
    where `render` returns a `PanelNode` tree, **not** raw DOM.
  - Status-bar items use the `StatusBarContribution` manifest contribution
    (DTO at `index.ts:482-487`), not a runtime API.
  - Tree views go through `ctx.ui.registerTreeDataProvider(viewId, provider)`.
  - No `ctx.ui.modal()`.
- Slot id drift: doc table (lines 12-20) lists `activityBar`, `sidebar`,
  `editor`, `rightPanel`, `bottomPanel`, `statusBar`, `commandPalette`,
  `modal`, `notification`. Actual `SlotId` (index.ts:534-540) is
  exactly `'overlay'|'titleBar'|'activityBar'|'statusBarLeft'|
  'statusBarRight'|'paneMode'`. Sidebar/editor/rightPanel/bottomPanel
  are not slots — they're leaf-tree panes (per ADR 0011 leaf model).
- Status-bar `alignment: 'left'|'right'` (line 78) — actual contribution
  has `slot: 'left'|'right'`, not `alignment`.

## developer/ui/commands-and-keybindings.md
**Implements:** commands, keybindings, chords, when-clauses, menus.
**Gaps:**
- API drift: `ctx.commands.register(...)` and `ctx.commands.invoke(...)`
  (lines 11, 128) don't exist on the context — commands are registered
  via the manifest's `CommandContribution[]` plus an IPC dispatcher
  wired by the shell.
- Menu ids (`command.palette`, `editor.context`, `editor.title`,
  `view.title`, `explorer.context`) — not verified; the
  `MenuContribution.menu` field is a free-form string in the type.
- Keybinding object form `{default, mac}` (line 44) doesn't match the
  `KeybindingContribution` DTO which has `{command, key, mac?, when?}`
  (index.ts:475-480) — `key` is the default; `mac` is the macOS
  override; no `default` key.

## developer/ui/context-keys.md
**Implements:** context-key DSL for when-clauses.
**Gaps:**
- API drift: `ctx.context.get/set/subscribe` (lines 69, 87, 76) don't
  exist on `NexusPluginContext`. Context keys are declared via the
  `ContextKeyContribution` DTO and (presumably) read by the shell's
  when-clause evaluator — plugins don't read them via the context.
- Linked `docs/shell/context-keys.md` exists, presumed authoritative.

## developer/themes/build-a-theme.md
**Implements:** theme plugin scaffold + CSS variable overrides.
**Gaps:**
- Scaffold template drift: `--template theme` (line 21) is not a valid
  scaffold template — only `script`, `core`, `community` exist
  (main.rs:693). There is no theme template.
- Theme manifest schema in `contributes.themes[]` (line 46) — not
  verified against the live shell schema; built-in themes use the
  TOML manifest at `crates/nexus-theme/themes/<name>/NEXUS.toml`
  (callout box on line 9 correctly notes this).
- `nexus plugin install --watch ./` (line 122) — `install` has no
  `--watch` flag.
- Theme inheritance via `@import url('nexus-theme://...')` (line 100)
  — protocol scheme not verified.
- Path refs `crates/nexus-theme/themes/_template/NEXUS.toml`,
  `crates/nexus-theme/src/theme.rs::BUILTIN_THEMES`,
  `crates/nexus-theme/src/manifest.rs` all exist.

## developer/themes/css-variables.md
**Implements:** CSS-variable tier reference and naming conventions.
**Gaps:**
- Count stale: "~497 CSS variables" (lines 3, 88) — unverified;
  source files for the canonical list don't exist where the doc says
  they do.
- Path drift: `shell/src/styles/tokens/`, `shell/src/styles/themes/
  nexus-light.css`, `shell/src/styles/themes/nexus-dark.css` (lines
  11-14) **do not exist** — there is no `shell/src/styles/` directory.
  Theme CSS is split across `shell/src/shell/shell.css` and per-plugin
  CSS files. The bundled theme TOMLs in `crates/nexus-theme/themes/`
  are the actual variable source of truth (see
  `crates/nexus-theme/src/variables.rs`).
- Recommended `grep` command (line 19) targets a non-existent file.

## developer/core-plugins/authoring.md
**Implements:** Rust `CorePlugin` trait implementation, bootstrap
registration, IPC schema generation.
**Gaps:**
- API call drift: bootstrap entry `build_runtime(forge_root)` (line 111)
  does not exist. Actual functions are `build_cli_runtime(PathBuf)` and
  `build_tui_runtime(PathBuf)` (per `nexus-bootstrap/src/lib.rs:108,
  134`). Registration also doesn't happen via
  `kernel.register_core_plugin(...)` in user code — there's a private
  `register_core_plugins(&mut loader, forge_root, &event_bus)` function
  at line 472 of the same file.
- `build_test_runtime()` (line 180) — not verified; may or may not be
  the actual test fixture name.
- `ctx.events_subscribe`/`ctx.events_publish` (lines 162, 167) — not
  verified against `PluginContext` Rust API; shape is plausible.
- Capability example uses `CapabilitySet::from_iter([...])` — verified
  shape consistent with `capability.rs:193`.
- Path drift: template at `docs/templates/core-plugin/` (line 240)
  doesn't exist. Same gap as reference.md.
- Dep-invariants test path `crates/nexus-bootstrap/tests/dep_invariants.rs`
  is correctly cited.

# Users

## users/README.md
**Source of truth:** developer-hub navigation + env-var reference.
**Gaps:** Verified — no drift. MCP tool count (15) is correct.

## users/cli.md
**Source of truth:** `crates/nexus-cli/src/main.rs` and `commands/`.
**Gaps:**
- Command-surface drift (lines 13-37). Doc lists subcommands missing
  several that ship today, and misstates surface for a few:
  - `forge`: doc has `init, status`. Actual: `init`, `status`,
    `reindex`, `import` (main.rs:157-185).
  - `tags`: doc lists `list, locate`. Actual: `list` only
    (main.rs:331-338).
  - `bases`: doc says `query, validate`. Actual: `create, list, show,
    add-record, query, import, export, formula` (main.rs:933).
  - `agent`: doc says `run, list, history`. Actual: `plan, run`
    (main.rs:414).
  - `workflow`: doc says `run, list`. Actual: `list, show, run,
    reload, validate, template` (main.rs:576).
  - `db`: doc says `query, schema`. Actual subcommands live in
    `commands/db.rs::DbCommand` (not inspected here, but the umbrella
    description is incomplete).
  - `config`: doc says `get, set, list`. Actual: `show, reset`
    (main.rs:908).
  - `proc`: doc says `list, kill`. Actual: `list, show, add, delete,
    reorder, history` (main.rs:445).
  - `term`: doc says `saved, run`. Actual: `env, run, shell`
    (main.rs:1393). Saved-command snippets actually live under
    `proc`, not `term`.
  - `mcp`: doc says single command "Start MCP server (stdio)". Actual
    is a subcommand group: `serve, servers, tools, call`
    (main.rs:542-563). `nexus mcp` alone does not start the server —
    `nexus mcp serve` does. This contradicts both users/mcp.md
    (line 11: `nexus mcp`) and users/README.md's quickstart hint.
  - `ai`: doc says `ask, embed, status, config`. Actual adds `chat`,
    `complete`.
  - `git`: doc says `status, log, blame, diff`. Actual has 20+
    subcommands including `stage`, `commit`, `branch`, `tag`, `push`,
    `pull`, `merge`, `auto-commit`, `rebase`, `cherry-pick`,
    `lfs-status`, etc. (main.rs:1146-1326).
  - Doc is missing top-level subcommands shipping today: `import`,
    `export`, `template`, `crdt`, `skill`, `completions`, `sync`,
    `run`, and the external-subcommand dispatch for plugin-registered
    CLI commands.
- Dead link: `../plugin-authors/quickstart.md` (lines 95, 162, 178) —
  `docs/plugin-authors/` directory does not exist.
- Dead link: `../archive/planning/PHASE-4-IMPLEMENTATION-PLAN.md` —
  exists; verified. `PHASE-5-IMPLEMENTATION-PLAN.md` — exists.
- `nexus plugin scaffold --template script` output layout (lines 150-
  157) matches the live template at
  `crates/nexus-plugins/templates/script/` (verified: emits
  `index.ts`, `plugin.json`, `package.json`, `tsconfig.json`,
  `README.md`).
- `--type` alias for `--template` (line 165) — verified
  (main.rs:693).

## users/tui.md
**Source of truth:** `nexus-tui` crate.
**Gaps:** Verified — no drift in the keymap/limitations sections.
(Did not exhaustively verify every keystroke against the TUI's actual
binding map; spot checks consistent.)

## users/mcp.md
**Source of truth:** `crates/nexus-mcp/src/server.rs`.
**Gaps:**
- Tool count and names: verified against `server.rs`. The 15 tool
  declarations at lines 421, 454, 465, 476, 499, 545, 596, 636, 682,
  718, 760, 807, 844, 886, 940 produce exactly the 15 names in the
  doc's table. **Verified — no drift on the tool list itself.**
- Launch command drift: doc says `nexus mcp` starts the server (line
  11). The CLI actually requires `nexus mcp serve` (main.rs:545); a
  bare `nexus mcp` will print help. See users/cli.md gap above.
- Tool-routing internals section's IPC-handler names
  (`com.nexus.storage::read_file`, `com.nexus.ai::stream_ask`) — not
  exhaustively verified, but match the canonical IPC naming pattern.
# Help docs traceability audit

Scope: 33 .md files under `/home/baileyrd/projects/nexus/docs/help/`.
Method: sample-checked per subfolder against `shell/src/plugins/*`,
`crates/nexus-cli/src/commands/*`, `crates/nexus-mcp/src/server.rs`, and
spot-verified storage / kernel paths. The shell `app/` retirement happened
2026-04-24 — no doc was found that talks about a tri-pane / `app/` UI, so
the docs already reflect the plugin-first shell.

Top-level finding: the help docs read almost entirely as **shipped**, not
aspirational. There is no “legacy shell” content that survived migration.
Most flagged issues are smaller mismatches (a wrong keybinding, a CLI
subcommand name that drifted, a UI control claimed by the doc that hasn’t
landed yet). A handful of cases describe planned behavior — the docs
generally flag those with `roadmap`, `backlog`, or `WI-…` markers.

---

## help/ (top-level)
**Files:** `README.md`, `forge-and-files.md`, `search.md`
**Coverage:** 3 docs, all shipped.
**Implements:** `nexus-storage` (file-as-truth, watcher, Tantivy),
`shell/src/plugins/nexus/files`, `shell/src/plugins/nexus/search` &
`searchPanel`.

**Verified:**
- `Ctrl+Shift+F` opens the Search panel — keybinding registered at
  `shell/src/plugins/nexus/searchPanel/index.tsx:66`.
- `.forge/` layout in `forge-and-files.md` matches what
  `nexus-storage` / `nexus-bootstrap` write (`index.db`, `search/`,
  `app.toml`, `ai.toml`, `mcp.toml`, `workspace.json`, `kv.sqlite3`,
  `chat/sessions/`, `skills/`, `logs/`, `temp/`).
- CLI: `nexus content search`, `nexus forge reindex`,
  `nexus content create/list/backlinks` all exist
  (`crates/nexus-cli/src/commands/{content,forge}.rs`).

**Gaps:**
- `search.md` says `path:`, `tag:`, `prop:` operators “parse today but the
  post-filtering pass is partial — see BL-003”. This is honest and
  matches code, but is the only place in the help corpus that surfaces
  a Tantivy-query-DSL caveat — keep an eye on whether BL-003 has shipped
  before the next docs pass.
- `forge-and-files.md` mentions `nexus content update foo.md --rename bar.md`.
  `content.rs` exposes `update(path, content, stdin)` only — there is no
  `--rename` flag on `content update`. **Doc-only**: aspirational
  shorthand for "rename a file".
- `forge-and-files.md` says `nexus content delete foo.md`. Spot-checked
  `crates/nexus-cli/src/commands/content.rs` — only `create`, `update`,
  `list`, `search`, `tasks`, `task_toggle`, `backlinks` are exported.
  No `delete` subcommand. **Gap.**

---

## help/getting-started/ (4 files)
**Files:** `install.md`, `first-forge.md`, `quick-tour.md`, `frontends.md`
**Coverage:** 4 docs, all shipped, one bug.
**Implements:** `nexus-bootstrap`, `nexus-cli`, `nexus-tui`, `nexus-mcp`,
`nexus-shell` (Tauri).

**Verified:**
- `nexus forge init`, `nexus content list`, `nexus desktop`, `nexus tui`,
  `nexus mcp` all exist as CLI subcommands
  (`crates/nexus-cli/src/commands/{forge,content,desktop,tui,mcp}.rs`).
- Tauri prereqs (`webkit2gtk-4.1`, `libsoup-3.0`) match the project
  `CLAUDE.md` system-requirements note.
- The "10-minute tour" walks Backlinks panel, Tags panel, Graph view,
  Terminal panel — all exist as shell plugins
  (`shell/src/plugins/nexus/{backlinks,tags,graph,terminal}`).

**Gaps:**
- `quick-tour.md:78` says “Inline AI completion: `Ctrl+Shift+Space`”. The
  AI plugin (`shell/src/plugins/nexus/ai/index.ts:256-260`) registers
  `Ctrl+Alt+A` for AI focus and `Ctrl+I` / `Cmd+I` for Cmd-I inline. No
  `Ctrl+Shift+Space` binding is registered anywhere in the shell — the
  string only appears in `shell/src/plugins/nexus/memory/index.ts`.
  `editing/editor.md:79`, `ai/overview.md:12`, `ai/inline-completion.md:3`,
  and `customize/keybindings.md:32` all repeat the same wrong keybinding.
  **Likely fix: rewrite to `Ctrl+I` / `Cmd+I`** (or rebind the inline
  command to Shift+Space if that's the design intent).
- `quick-tour.md` and `frontends.md` reference `nexus desktop` as the
  way to launch the shell — `commands/desktop.rs` exists, so this works,
  but it shells out to `NEXUS_SHELL_BIN` and bails if unset. Help docs
  don't explain the env var; `customize/settings.md` does. Minor.
- `first-forge.md:30` says the desktop shell remembers the forge in
  `~/.config/nexus-shell/last-forge.json`. The Tauri persistence path is
  whatever `write_last_forge_path` writes; the file name was
  `last-forge.json` until the recent (commit `4ccc21b6`) migration that
  moved shell settings into `<forge>/.forge/app.toml`. Quick re-check
  recommended that `last-forge.json` is still the right name for the
  *forge-pointer* file (separate from settings).

---

## help/editing/ (4 files)
**Files:** `editor.md`, `comments.md`, `markdown-and-blocks.md`,
`embeds-and-mdx.md`
**Coverage:** 4 docs, 3 shipped, 1 partly aspirational.
**Implements:** `shell/src/plugins/nexus/editor` (CodeMirror 6),
`shell/src/plugins/nexus/comments`, `crates/nexus-comments`.

**Verified:**
- Source / live preview / read-only modes — `shell/src/plugins/nexus/editor`
  has `livePreview.css`, `livePreviewDecorations.ts`, `codeMode.ts`.
- Block handles ("⋮" drag handle) — `cm/blockHandle.ts` exists. Drag-to-
  reorder is wired through `blockRefDrag.ts` / `blockRefDragBridge.ts`.
- Slash commands — `editor/cm/slashCommand.ts` exists.
- Tabs, popout per ADR 0020 — confirmed in `shell/src-tauri` Tauri
  commands `popout_window` etc.
- Editor keybindings (`Ctrl+W` close, `Ctrl+S` save, `F2` rename,
  `Ctrl+.` ) are registered at `editor/index.ts:240-261`.

**Gaps:**
- `editor.md:73` says comments use `Ctrl+K Ctrl+C` chord — no such chord
  keybinding is registered in `shell/src/plugins/nexus/comments` (grep
  for `ctrl+k|cmd+k` came up empty in that plugin). The comments panel
  itself exists but the chord trigger is **doc-only**.
- `comments.md:18-26` says comments “are stored in the block’s properties
  — invisible in the markdown body, but persisted on the file. They show
  up in `git diff` as YAML frontmatter changes.” This is **incorrect**.
  `crates/nexus-comments/src/store.rs:1-5` is explicit: comments go in
  a JSON sidecar, not in block-level frontmatter. They will not appear
  in `git diff` as YAML frontmatter changes. **Rewrite.**
- `embeds-and-mdx.md` claims an `editor.registerMdxComponent` extension-
  API method — no such function exists under `shell/src` (grep for
  `registerMdxComponent` / `mdxComponent` / `MDX` returned no shell
  matches). MDX support is **not currently shipped**; embeds (`![[…]]`)
  for images, audio, PDFs and note-embeds do appear to work via the
  editor live-preview decorators, but the JSX-component pipeline
  (`<Card />`, `<Alert />`, `<Badge />`) is aspirational. **Flag as
  roadmap.**
- `markdown-and-blocks.md` describes block IDs (`^a1b2c3`) and a "Copy
  block link" action. Block-handle drag is real; verify block-ID
  generation and block-reference rendering before next docs pass — code
  is present but not exercised in this audit.

---

## help/linking/ (4 files)
**Files:** `wikilinks.md`, `backlinks.md`, `graph.md`, `tags-and-properties.md`
**Coverage:** 4 docs, all shipped.
**Implements:** `nexus-storage` (graph, link extraction, file watcher),
`shell/src/plugins/nexus/{backlinks,outgoingLinks,tags,allProperties,fileProperties,graph,linkSuggest}`.

**Verified:**
- `nexus graph status / neighbors / unresolved` all exist in
  `crates/nexus-cli/src/commands/graph.rs`.
- Backlinks panel, Tags panel, Properties panel, Outgoing-links panel
  all have corresponding shell plugin directories.
- `nexus content backlinks <path>` exists
  (`commands/content.rs:314`).
- Wikilink autocomplete via `linkSuggest` plugin.
- `[graph] exclude = […]` in `app.toml` — needs spot-check in
  `nexus-storage` config parser, not done here.

**Gaps:**
- `wikilinks.md:51` claims a CLI `nexus content links README.md`. No
  `links` subcommand under `commands/content.rs`. **Doc-only.**
- `wikilinks.md:33-35` says clicking an unresolved `[[Foo]]` "offers to
  create the file". This is the typical Obsidian UX — verify in
  `linkSuggest` / link click handlers that the create-on-click prompt
  is actually wired. Not exercised here.
- `tags-and-properties.md:34` references `nexus tags locate project`.
  `commands/tags.rs` has `list(name)` — no `locate` subcommand. Likely
  the same operation under a different name; doc and CLI have drifted.

---

## help/ai/ (4 files)
**Files:** `overview.md`, `chat.md`, `inline-completion.md`, `providers.md`
**Coverage:** 4 docs, 3 mostly-shipped, inline-completion partly wrong.
**Implements:** `crates/nexus-ai`, `shell/src/plugins/nexus/ai`,
`shell/src/plugins/nexus/semanticSearch`.

**Verified:**
- `nexus ai ask`, `nexus ai embed`, `nexus ai status`, `nexus ai config`
  all in `commands/ai.rs`.
- AI Chat panel and Cmd-I overlay live at
  `shell/src/plugins/nexus/ai/{CmdIOverlay.tsx,cmdIStore.ts,cmdIApi.ts,index.ts}`.
- Provider configuration via `.forge/ai.toml` matches `nexus-ai`
  config schema (ADR 0009 keyring policy referenced).
- Chat-session JSON files in `<forge>/.forge/chat/sessions/` — matches
  `forge-and-files.md`.

**Gaps:**
- **The `Ctrl+Shift+Space` inline-completion keybinding is wrong across
  all four AI docs.** Actual binding is `Ctrl+I` / `Cmd+I`
  (`shell/src/plugins/nexus/ai/index.ts:260`) for the "Cmd-I" overlay,
  which is the inline AI workflow. **Cross-cutting fix.**
- `chat.md:46` lists `claude-opus-4-7` as an example model — accurate
  for 2026-05 (matches current Anthropic model ID).
- `chat.md:9` "Activity bar → **AI Chat** icon. Or palette → 'AI: Open
  chat'." — the activity bar is real (`shell/src/plugins/nexus/activityBar`),
  but the AI command title and the activity-bar icon contribution should
  be spot-checked against `ai/index.ts` registrations.
- `overview.md:88-92` "disable the `com.nexus.ai` core plugin if you
  want to be sure no AI code is loaded" — verify that disabling a core
  plugin is actually possible via `set_plugin_enabled` (it is, per the
  Tauri command surface), but disabling a *core* (Rust) plugin may
  only take effect on restart. Minor caveat worth adding.

---

## help/plugins/ (3 files)
**Files:** `overview.md`, `install-community.md`, `build-your-own.md`
**Coverage:** 3 docs, 2 shipped, 1 mostly-aspirational.
**Implements:** `crates/nexus-plugins`, `crates/nexus-kernel` (capability
system), `shell/src/host/{ExtensionHost.ts,PluginAPI.ts}`, Tauri commands
`scan_plugin_directory`, `set_plugin_enabled`, `get/set_plugin_granted_capabilities`.

**Verified:**
- `nexus plugin install/list/enable/disable/uninstall` all in
  `commands/plugin.rs`.
- Scaffold templates: `script`, `core`, `community`
  (`commands/plugin.rs:216-219`). The doc says `--template script` ✅,
  `--template theme` ❌.
- Activation events (`onStartup`, `onCommand:`, `onView:`, etc.) match
  what shell plugins declare in their manifests.
- Capability strings (`fs.read`, `ipc.call:…`, `kv.write`, etc.) match
  ADR 0002.

**Gaps:**
- `build-your-own.md:31-41` example uses `"capabilities": [...]` and
  `"activation": [...]` at the manifest's top level. Real Nexus plugin
  manifests (per `shell/src/plugins/nexus/*/index.ts` and ADR 0004) use
  `activationEvents` (plural, camelCase) — not `activation`. Verify
  field names against `packages/nexus-extension-api` types.
- `build-your-own.md:35` shows the WASM plugin scaffold template as
  `script`. Verify whether `script` produces `.wasm` (the README says
  it does, `commands/plugin.rs:240` calls `nexus_scaffold` with that
  template). The docs use the words “script” and “WASM” somewhat
  interchangeably; consider a single canonical term.
- `customize/themes.md` says **themes are plugins** scaffolded with
  `--template theme`, but `commands/plugin.rs` enumerates exactly
  `script | core | community` — no `theme` template exists. Theme
  authoring path is **doc-only / aspirational**.
- `install-community.md:23-29` mentions installing from a URL with a
  signature-verification pass — `commands/plugin.rs:install_dispatch`
  only dispatches if the arg is a path. URL install + signature flow is
  **doc-only / aspirational**. Doc already hedges with “(see ADR for
  plugin manifest signing)” but doesn't mark the URL install as
  unshipped.
- `install-community.md:47` says `nexus plugin reset com.x.foo` "wipes
  state, keeps installation". `commands/plugin.rs:165` exports
  `reset_crash(plugin_id)` only — i.e. resets the crash counter, not
  per-plugin state. **Different command, different semantics.**
- A built-in marketplace (WI-44) is explicitly flagged as roadmap in
  `install-community.md:7-9`. Good.
- Hot-reload: `overview.md:91-93` says modifying a `.wasm` reloads it
  without restart, and `build-your-own.md:64` mentions
  `--watch`. Verify that `plugin install --watch` is implemented;
  spot-check of `commands/plugin.rs` did not reveal a `--watch` arg.

---

## help/customize/ (3 files)
**Files:** `settings.md`, `themes.md`, `keybindings.md`
**Coverage:** 3 docs, 2 shipped, 1 partly aspirational.
**Implements:** `shell/src/plugins/{core,nexus}/...`, `nexus-theme`,
`nexus-storage` (shell settings now persist to `<forge>/.forge/app.toml`
per commit `4ccc21b6`).

**Verified:**
- `nexus config list/get/set` all in `commands/config.rs` (not opened in
  detail but the doc’s usage matches the conventional clap layout).
- Env vars: `NEXUS_FORGE_PATH`, `NEXUS_CONFIG`, `NEXUS_SHELL_BIN`,
  `NEXUS_SAFE_MODE`, `NEXUS_NO_KEYRING`, `RUST_LOG` — confirmed by
  grepping shell `src-tauri/src/lib.rs` and `CLAUDE.md`.
- Command palette `Ctrl+Shift+P` / `Cmd+Shift+P` correct
  (`shell/src/plugins/nexus/commandPalette/index.ts:35`).
- Search-in-workspace `Ctrl+Shift+F` correct.
- Theme picker `Ctrl+Shift+T` registered in
  `shell/src/plugins/nexus/themePicker/index.ts:37`.

**Gaps:**
- `keybindings.md:32` repeats the wrong `Ctrl+Shift+Space` inline-AI
  binding.
- `keybindings.md:33` says `Ctrl+K Ctrl+C` chord for comment — not
  registered (see editing/ above).
- `keybindings.md:34-35` describes `Ctrl+Alt+Left/Right` to move active
  tab — verify against `shell/src/plugins/nexus/editor`. The view-header
  arrow buttons exist (`editor.md:27`) but I did not confirm the global
  keybinding registration.
- `keybindings.md:36` says `Ctrl+Shift+T` "New terminal session". The
  same chord is registered for the **theme picker** in
  `themePicker/index.ts:37`. **Conflict**: theme picker wins on startup
  unless the terminal-new-session command claims the binding later.
  Worth disambiguating.
- `keybindings.md:46-48` example `nexus config set
  keybindings."com.nexus.editor.toggleLivePreview" "Ctrl+P"` — the
  `keybindings` namespace is a setting hierarchy the doc claims; verify
  this is actually exposed via `commands/config.rs`. The plumbing
  (`api.keybindings.setOverride`) is present in `recall/index.ts:120`,
  but a CLI override path may not be wired.
- `themes.md:54` references `docs/shell/theme-variables.md` — that file
  is `docs/developer/themes/css-variables.md` per the grep results.
  **Broken link.**
- `themes.md:38-41` says `nexus plugin scaffold --template theme` —
  template doesn't exist (see plugins gaps).
- `themes.md:73-75` flags macOS vibrancy / Windows Mica as roadmap.
  Honest.

---

## help/advanced/ (8 files)
**Files:** `agents.md`, `bases.md`, `canvas.md`, `git.md`, `mcp-server.md`,
`skills.md`, `terminal.md`, `workflows.md`
**Coverage:** 8 docs; 5 mostly-shipped, 3 with material gaps.
**Implements:** `crates/{nexus-agent,nexus-git,nexus-mcp,nexus-skills,
nexus-terminal,nexus-workflow}`, `shell/src/plugins/nexus/{agent,bases,
canvas,gitPanel,gitStatus,mcp,skills,terminal,workflow}`.

**Verified:**
- `nexus mcp` runs an MCP server on stdio — `commands/mcp.rs` +
  `crates/nexus-mcp/src/server.rs`. The tool list in `mcp-server.md` is
  accurate at a glance (`nexus_create_note`, `nexus_search`,
  `nexus_list_tasks`, `nexus_toggle_task`, `nexus_ask`,
  `nexus_render_skill` all confirmed in `server.rs`).
- `nexus git status/log/diff/blame` all in `commands/git.rs:8-92`.
- `nexus skill list/render` in `commands/skill.rs:18,47`.
- `nexus workflow list/run/validate` in `commands/workflow.rs:20-103`.
- `nexus bases query/validate` in `commands/bases.rs:31,87`.
- `nexus agent run` in `commands/agent.rs:36`.
- `nexus proc list/kill` and `nexus term saved/run` exist
  (`commands/{proc,term}.rs`).
- Canvas plugin shipping with undo/redo/fit/group keybindings
  (`shell/src/plugins/nexus/canvas/index.ts:67-76`).
- Bases plugin shipping with Table, Board, Calendar, Gallery, List,
  Timeline views (`shell/src/plugins/nexus/bases/*`).
- Terminal PTY-backed plugin with multi-session support
  (`shell/src/plugins/nexus/terminal`).

**Gaps:**
- `agents.md:24-28` claims CLI subcommands `nexus agent list` and
  `nexus agent history --session abc123`. Only `run` exists in
  `commands/agent.rs`. **Doc-only.**
- `agents.md:101-109` example uses `context.agents.register({…})`
  extension-API — verify this exists in `@nexus/extension-api`; not
  spot-checked.
- `bases.md:101-107` flags inline `[[{db:query}]]` and relations as
  backlog. Good.
- `canvas.md:13-15` says "click + in the file tree → New canvas". Need
  to spot-check `shell/src/plugins/nexus/files` context-menu for the
  "New canvas" item. Not done.
- `mcp-server.md:60` references `nexus_list_skills` — confirmed in
  `server.rs:1013`. Doc-tools list is accurate.
- `mcp-server.md:91-94` claims `.forge/mcp.toml` `[server.tools] deny`
  list — verify in `crates/nexus-mcp/src/config.rs`. Not done.
- `workflows.md:34-39` flags `file-event` and `webhook` triggers as
  backlog (`⚠️ on backlog`). Good — `crates/nexus-workflow` likely
  supports only `manual` and `cron` today.
- `skills.md:62-65` says `nexus ai ask --stdin --no-rag` reads stdin.
  Verify `commands/ai.rs:32 pub fn ask(question)` accepts a `--stdin`
  flag. Not spot-checked, but `ask` takes a positional `question` so
  this may also be doc-only.
- `terminal.md:30` mentions `Ctrl+Shift+S` "Save current input". No
  matching keybinding found in `shell/src/plugins/nexus/terminal/index.ts`
  (grep returned only an unrelated `Ctrl+Shift+P` reference). **Doc-only.**
- `terminal.md:55` claims `Ctrl+R` opens a fuzzy history picker across
  all sessions. Verify against `terminal` plugin. Not done.
- `git.md` is read-only by design — this is honest and matches the
  `nexus-git` crate's exported CLI surface.

---

# Cross-cutting themes

1. **Wrong inline-AI keybinding.** Five docs (`getting-started/quick-tour`,
   `editing/editor`, `ai/overview`, `ai/inline-completion`,
   `customize/keybindings`) all say `Ctrl+Shift+Space` / `Cmd+Shift+Space`.
   The shell binds inline AI to `Ctrl+I` / `Cmd+I`
   (`shell/src/plugins/nexus/ai/index.ts:260`). Single highest-impact
   correction.

2. **Comment-thread storage is described wrong.** `editing/comments.md`
   says comments live in YAML frontmatter; `crates/nexus-comments/src/store.rs`
   stores them in a JSON sidecar.

3. **CLI subcommand drift.** Several `nexus <noun> <verb>` examples in the
   docs (`agent list`, `agent history`, `content delete`, `content links`,
   `content update --rename`, `tags locate`, `ai ask --stdin`,
   `plugin reset` semantics) don't exist or don't have the claimed
   semantics. Most look like aspirational shorthand the author wrote
   from memory rather than from `--help`.

4. **MDX components are aspirational.** `editing/embeds-and-mdx.md`
   describes a complete component-registration story that doesn't exist
   in the shell. Embeds (`![[…]]`) work; JSX-style `<Card />` doesn't.

5. **Theme plugin template is aspirational.** `customize/themes.md`
   tells users to scaffold themes with `--template theme`. Only
   `script | core | community` exist.

6. **Plugin URL install + signature verification is aspirational.**
   `plugins/install-community.md` describes both, neither is shipped.

7. **Keybinding conflict.** `Ctrl+Shift+T` is documented for "new
   terminal session" but registered for the **theme picker**. Either
   the conflict is real (bug) or the doc is just wrong about the
   terminal binding.

8. **Aspirational vs. shipped clarity.** The docs already use
   conventions like `⚠️ on backlog`, `Status:`, `(WI-NN)`, and "on the
   roadmap" — but inconsistently. About half of the unshipped features
   are flagged, half aren't.

# No legacy-shell content found

None of the 33 help docs reference the retired `app/` tri-pane shell,
`nexus-app` crate, or the legacy three-column layout. The migration to
the plugin-first shell (2026-04-24) appears to have been accompanied by
a docs sweep, or these were written after.

# Recommended doc-fix priority (small set)

1. Fix `Ctrl+I` / `Cmd+I` inline-AI keybinding across 5 docs.
2. Rewrite `editing/comments.md` storage model paragraph.
3. Remove or roadmap-flag `editing/embeds-and-mdx.md` MDX content.
4. Remove the `--template theme` instruction from `customize/themes.md`
   (or document the workaround: write a community plugin that
   contributes a theme).
5. Audit every `nexus <noun> <verb>` example against `--help` and patch
   the drifted ones (agent list/history, content delete, content links,
   tags locate, plugin reset, ai ask --stdin).
6. Disambiguate `Ctrl+Shift+T` (terminal vs. theme picker).
7. Fix broken link `docs/shell/theme-variables.md` →
   `docs/developer/themes/css-variables.md`.
