# Nexus Full-App Integration Review

> **Historical document** — Written before the `app/` → `shell/` migration (Phase 4 WI-37, 2026-04-24). Paths below reference the legacy `app/` and `crates/nexus-app/` tree that has since been deleted. For current locations see `docs/legacy-shell-retirement.md`.

**Date:** 2026-04-23
**Author:** Claude (audit run)
**Scope:** Full-repo deep audit of Nexus (Rust microkernel + Tauri desktop shell + CLI/TUI/MCP frontends + WASM plugin system), assessing the path to a unified, ship-ready v1 desktop product.

---

## 1. Executive Summary

Nexus is further along than its "Alpha (v0.1.0)" banner suggests. The microkernel substrate (`nexus-kernel`), the file-as-truth storage engine (`nexus-storage`), the IPC-first boundary between frontends and services, the WASM plugin sandbox, the theme engine, and the MCP server are all shipping. 340+ tests pass. A guardrail test in `nexus-bootstrap` actively prevents architectural regressions by failing the build if, say, the CLI imports `rusqlite` directly.

The integration blocker is **not** any single missing service. It is that the project is currently carrying **two parallel Tauri desktop shells** at different architectural generations, and most other integration questions are downstream of picking one:

- **Legacy shell: `app/` + `crates/nexus-app`** — React/Zustand/CodeMirror frontend with a Rust host that exposes ~95 `#[tauri::command]` handlers (registered in the `generate_handler!` block in `crates/nexus-app/src/lib.rs`). Tightly integrated with the kernel via `nexus-bootstrap::build_cli_runtime`, has ~12 feature panels wired to live data, ships a theme engine, terminal, AI/agent streaming, and editor transactions. Actively developed through mid-April 2026 (sessions 02593422, 6e9c0339, 49c21a02).
- **New shell: `shell/` + `shell/src-tauri` (crate `nexus-shell`)** — "The shell starts completely empty. There is no sidebar, no title bar, no editor, no status bar until plugins load them." (shell/README.md) A VS-Code-style plugin-first substrate with an `ExtensionHost`, `PluginRegistry`, slot system, context keys, and a typed event bus. Carries 32 `nexus.*` plugins already registered in `shell/src/main.tsx` covering editor, canvas, bases, graph, outline, backlinks, search, AI, agent, workflow, skills, MCP, terminal, processes, and more. Built per the `shell-kernel-bridge-plan.md` and `leaf-architecture.md` documents. Excluded from the Cargo workspace (`Cargo.toml` line 3: `exclude = ["shell"]`) and links to kernel crates by path.

Both shells are functional. They are **not** interoperable — they instantiate separate Tauri binaries, load different React trees, and persist to different state files. This is the integration question.

This review makes one primary recommendation and a phased plan:

> **Recommendation:** Complete the migration to `shell/` (the plugin-first shell) and retire `app/` + `crates/nexus-app`. Treat the feature coverage in `app/` as the functional acceptance bar for declaring the migration complete. The new shell's architecture is what the docs (leaf, bridge, canvas, bases, notion-blocks, graph, editor-transaction) have been designed around, and almost all of those design docs mark Phases 1–6 **complete against the new shell**, not the legacy one.

Everything else — unifying CLI/TUI/Desktop command surfaces, finishing the plugin pipeline, shipping v1 — follows from that choice.

---

## 2. What's Actually Wired Up

### 2.1 Kernel and services layer — solid

The microkernel follows a clean four-layer crate DAG with no cycles:

```
Layer 0 (foundation):      nexus-types, nexus-plugin-api, nexus-formats
Layer 1 (kernel):          nexus-kernel, nexus-kv, nexus-plugins
Layer 2 (service plugins): nexus-storage, nexus-security, nexus-database,
                           nexus-editor, nexus-ai, nexus-agent, nexus-mcp,
                           nexus-git, nexus-terminal, nexus-theme,
                           nexus-workflow, nexus-linkpreview, nexus-skills
Layer 3 (orchestrator):    nexus-bootstrap  (registers 13 core plugins in order)
Layer 4 (frontends):       nexus-cli, nexus-tui, nexus-app, (shell/nexus-shell)
```

`nexus-bootstrap::build_cli_runtime(forge_root)` and `build_tui_runtime(forge_root)` return an identical `Runtime { kernel, context, loader }` triple, and every frontend routes subsystem calls exclusively through `context.ipc_call(plugin_id, cmd, args)`. The MCP server is the same pattern: 13 MCP tools over stdio, each translated to an IPC call.

ADRs 0001–0009 lock in the important decisions: one crate per PRD, hierarchical dot-namespaced capabilities, storage owns the file watcher, keyring hard-fail, file-as-truth, IPC-first boundary, ts-rs-generated TypeScript types across the Tauri bridge.

### 2.2 Frontends — three working, one in transition

- **`nexus-cli`** — ~12 subcommands (forge, content, graph, canvas, ai, plugin, mcp, git, watch, logs, config) all routed through kernel IPC. `--format=json|jsonl|table` output plus clap completions. Fully shipped.
- **`nexus-tui`** — ratatui-based interactive explorer with file tree, viewer, backlinks, FTS overlay, and task list. Routes through the same IPC contract as the CLI. Fully shipped.
- **`nexus-mcp`** — 13 MCP tools (`forge_read`, `forge_search`, `note_create`, `task_list`, etc.) over stdio; routes through kernel IPC with 30/120-second timeouts. Validates the IPC boundary is reachable from external AI clients.
- **Desktop shell — two implementations coexist:**
  - Legacy (`app/` + `crates/nexus-app`): ~95 Tauri commands, ~12 live feature panels, theme engine, CodeMirror editor, xterm.js terminal, agent streaming. Last active session summary: 2026-04-17.
  - New (`shell/` + `nexus-shell`): plugin-first substrate, 32 `nexus.*` plugins registered, implements the slot/registry/context-key architecture from `shell/README.md` and the bridge from `docs/shell-kernel-bridge-plan.md`.

### 2.3 Plugin system — two tiers, three runtimes

- **Core plugins:** native Rust, registered in `nexus-bootstrap::register_core_plugins()` in a deterministic order (security → storage → database → editor → theme → ai → skills → workflow → linkpreview → agent → mcp.host → git → terminal). Trust level: `Core`, capabilities: ALL.
- **Community plugins (WASM tier):** `.wasm` modules loaded via wasmtime, capability-gated, manifest-driven. Loader in `nexus-plugins` crate. Discovered from `.forge/plugins/` lazily at frontend boot.
- **Community plugins (script tier, JS):** ES modules loaded in the Tauri WebView via Blob URL `import()`. **Currently bypass the capability sandbox entirely.** `DEPRECATED.md` marks community script plugins as "first-party / core only until UI F-8.1.1 and UI F-2.2.1 land."

### 2.4 Integration plan docs — designed for the new shell

The twelve planning documents in `docs/` (`leaf-architecture.md`, `leaf-migration-plan.md`, `shell-kernel-bridge-plan.md`, `editor-transaction-architecture.md`, `editor-transaction-wiring-plan.md`, `canvas-shell-plan.md`, `bases-shell-plan.md`, `global-graph-view-plan.md`, `notion-block-ux-plan.md`, `tab-context-menu-plan.md`, `editor-shell-auditor.md`, `FORGE-UI-PLAN.md`) all describe the **new shell**. File paths cited throughout point to `shell/src/plugins/nexus/...` and `shell/src-tauri/src/...`. The legacy `app/` shell is referenced only for feature parity; it is not the target of ongoing architectural work.

Of note:
- `canvas-shell-plan.md` and `bases-shell-plan.md` both list Phases 1–6 complete as of 2026-04-22.
- `notion-block-ux-plan.md` lists Phases 1–6 complete as of 2026-04-22.
- `shell-kernel-bridge-plan.md` Phases 0–3 are delivered; Phase 4 polish is in progress.
- `leaf-migration-plan.md` — foundation scaffolded, Phases 0–7 in progress.

---

## 3. The Two-Shells Problem

### 3.1 Structural evidence

| Aspect | `app/` + `crates/nexus-app` (legacy) | `shell/` + `shell/src-tauri` (new) |
|---|---|---|
| Cargo workspace | Member (`crates/nexus-app`) | **Excluded** (`shell/` is outside workspace) |
| Crate name | `nexus-app` | `nexus-shell` |
| Tauri host path | `crates/nexus-app/src/{main,lib}.rs` | `shell/src-tauri/src/{main,lib}.rs` |
| React entry | `app/src/main.tsx` | `shell/src/main.tsx` |
| Package manager | npm | pnpm |
| Architectural model | Hardcoded tri-pane layout (top bar, left tree, center editor tabs, right inspector, bottom panel) | "Empty shell" + slot registry + plugin contributions |
| Plugin system | Frontend loader (`scriptRuntime.ts`) bypassing capability checks | `ExtensionHost` + `PluginRegistry` + kernel bridge |
| Kernel integration | `nexus_bootstrap::build_cli_runtime()` held in Tauri state | Same, plus `kernel_invoke`/`kernel_subscribe` bridge commands |
| Feature coverage | ~12 live panels, streaming AI/agent, multi-tab editor, PTY terminal, persisted layout | 32 plugins registered but not all parity-equivalent; editor transactions and canvas/bases/graph ahead of legacy |
| Theme engine | Wired to `com.nexus.theme` plugin with event forwarding | Wired; shell-level theme store has its own state (see `shell/src/main.tsx` which clears `shell-theme` localStorage on boot) |
| Last session activity | Mid-April 2026 (sessions 02593422, 6e9c0339, 49c21a02 — PRD-07 multi-tab, layout persistence, VI mode) | Active; most planning-doc deliverables land here |

### 3.2 Why both exist

Reading the session history in order: the team shipped PRD-07 (theming & UI) into `app/` first, then during Phase B decoupling recognized the need for a plugin-first substrate (VS-Code-style) that could properly host the Leaf/ViewRegistry and bridge architectures described in docs. Rather than rewriting the existing shell in place, they scaffolded `shell/` as a clean reference and excluded it from the workspace so both could build independently. This is a sensible engineering choice — the two ship separate binaries, so they can both live in the tree. But it is also a transient state, and staying in it indefinitely is expensive:

- **Duplicate IPC surfaces.** Every Rust handler in `crates/nexus-app/src/*.rs` (theme, editor, forge, ai, agent, etc.) would need a parallel implementation in `shell/src-tauri/src/*.rs` to keep feature parity. Drift is inevitable.
- **Duplicate frontend plugin loaders.** `app/src/plugins/scriptRuntime.ts` and `shell/src/host/ExtensionHost.ts` solve the same problem differently.
- **Divergent persistence.** `app/` writes to `layout-persistence.json` / `shell-state.json` managed by Tauri; `shell/` writes `.forge/workspace.json` (Leaf model) plus `<app_config_dir>/shell-state.json`.
- **Two sets of security holes.** The JS-plugin-in-webview audit findings (F-5.5.1, F-8.1.1) apply to both; fixes must be done twice.
- **Plugin authors have no stable contract.** `DEPRECATED.md` mentions a future `@nexus/extension-api` TypeScript package (UI F-2.1.1) — that package presumably targets one of the two shells. If it targets `shell/`, `app/`'s custom contribution API goes stale.

### 3.3 Which one wins

The new shell (`shell/`) is the right target for three reasons:

1. **Architectural alignment with the plans.** All twelve integration plan documents are written against `shell/` paths. Canvas, Bases, Notion blocks, and the editor transaction model have their Phase 1–6 implementations in `shell/src/plugins/nexus/...`. Pointing at `app/` now would invalidate months of design work and require retrofitting those Phase-6 implementations into the legacy tri-pane model.
2. **Extensibility ceiling.** The plugin-first substrate is what a knowledge app needs to compete with Obsidian/Logseq/Reflect/Notion. The legacy shell's hardcoded layout can't host a community marketplace because third-party plugins would have nowhere to mount except the single "activity bar" strip.
3. **Security model.** The bridge plan's invariant — "shell plugins are JS clients. They never link Rust, never implement `CorePlugin`" — is only enforceable in the new shell where the ExtensionHost mediates all contributions. Legacy `app/` gives plugins direct `invoke()` access to arbitrary Tauri commands.

The legacy shell (`app/`) still wins on **feature coverage today** — it has the live AI/agent streaming UX, the multi-tab editor, the working terminal. The migration plan below treats that as a set of acceptance gates, not a reason to keep both.

---

## 4. Recommended Integration Approach

### 4.1 One command bus, three frontends, one shell

The unifying abstraction is already in the tree: **`context.ipc_call(plugin_id, cmd_id, args)`** backed by the kernel event bus. Every frontend (CLI, TUI, MCP, desktop) funnels subsystem work through this single path. Going forward:

- **The IPC surface is the API contract.** Anything a frontend needs from a service lives as an IPC handler in that service's `CorePlugin::handle_ipc` impl. No frontend-specific Rust code that reaches into services directly.
- **Tauri commands are thin adapters.** Each `#[tauri::command]` is ≤10 lines — deserialize args, call `rt.context.ipc_call(...)`, serialize result. No business logic.
- **Event forwarders translate kernel bus → Tauri events.** One forwarder per topic prefix (`com.nexus.storage.*`, `com.nexus.theme.*`, `com.nexus.ai.stream_*`, `com.nexus.agent.*`, `com.nexus.editor.changed.*`). The shell subscribes via `api.kernel.on(topic, handler)`.
- **The CLI and TUI share the same handlers.** No divergence between `nexus content search` and what the desktop invokes — both go to `com.nexus.storage::search`.

This isn't new; it's already the pattern. The recommendation is to **stop adding Tauri-only logic to `crates/nexus-app`** and instead push every new capability into the service-layer plugin so it's reachable from all four frontends.

### 4.2 Leaf/ViewRegistry as the shell's sole layout primitive

Per `leaf-architecture.md`, the shell is built from three primitives:

- **`Leaf`** — a tabbed pane with a live `View` and a serializable state.
- **`View`** — content with `onOpen`/`onClose`/`setState`/`getState` hooks.
- **`ViewRegistry`** — maps `viewType → creator` and file extensions to view types.

Every UI contribution (sidebar panel, editor tab, canvas board, bases view, graph, terminal, AI chat, agent planner) becomes a `View`. `setViewState` is the sole mutation path. This is how VS Code, Obsidian, and JetBrains IDEs all work, and the design doc for it already exists — it just needs to be implemented in `shell/src/workspace/`. Leaf migration unblocks everything else because the downstream feature plugins (canvas, bases, graph, notion blocks) all assume this contract.

### 4.3 Plugin contract crate (`nexus-plugin-api` already exists — use it)

ADR 0001 and PRD 04 call for a separate contract crate so community plugins don't link against `nexus-kernel`. That crate exists: `crates/nexus-plugin-api` contains `Capability`, `NexusEvent`, `IpcDispatcher`, `LogLevel`, `PluginInfo`, `TrustLevel`, and error types. `nexus-kernel` re-exports them for convenience.

The **TypeScript twin** exists as a scaffolded package at `packages/nexus-extension-api/` (declared `@nexus/extension-api` v1.0.0, single `src/index.ts`), but neither shell currently imports from it — grep for `@nexus/extension-api` across `shell/src/` and `app/src/` returns zero hits. It's scaffolded but unwired. Finish the surface (derive types from `nexus-plugin-api` via ts-rs rather than hand-writing them, publish to a registry or make it the single internal import path), and have both shells (while both exist) consume it. This is called out in UI audit F-2.1.1 and `DEPRECATED.md`.

### 4.4 Capability enforcement and install-time prompt

The critical microkernel-audit findings (F-5.1.1, F-5.3.1, F-5.3.2, F-2.1.1, F-9.2.1) gate any public marketplace:

1. **Install-time capability prompt.** Wire `risk_level` classification into plugin install flow. User sees "This plugin wants: read your forge files, make network requests. Allow?" before enabling. Shell-side UI, kernel enforces.
2. **TOCTOU fix in `host::write_file` and `KernelPluginContext::write_file`.** Use `ForgePathValidator` (which already exists) instead of canonicalizing the parent and then writing to the non-canonical path.
3. **`api_version` enforcement at load time.** Every manifest already carries the field. Compare against a supported range in `PluginLoader::load_manifest`; reject on mismatch.
4. **JS plugin sandbox.** Move script plugins into an iframe with `sandbox="allow-scripts"` (no `allow-same-origin`), restore CSP in `tauri.conf.json`, and mediate all `invoke()` calls through a proxy that checks capabilities. This is UI F-8.1.1 — substantial but critical.

### 4.5 Kernel lifecycle and shell-kernel bridge

Per `shell-kernel-bridge-plan.md`, the shell launches, calls `boot_kernel(forge_root)`, gets a `KernelRuntime` into Tauri state, and forwards bus events. `kernel_invoke` and `kernel_subscribe` are the entire IPC contract. This is already implemented in `shell/src-tauri/src/bridge.rs` and covered by Phases 0–3 of the bridge plan.

The invariant to preserve: **shell plugins talk to the kernel only through `api.kernel.invoke(pluginId, cmd, args)` and `api.kernel.on(topic, handler)`.** No `window.__TAURI__.invoke("some_rust_function")` shortcuts; everything goes through the bridge. This is what makes the shell swappable (CLI/TUI/web variants later) and the kernel testable in isolation.

---

## 5. Phased Integration Roadmap

The roadmap takes the current state to a shippable v1 desktop app. It assumes one primary engineer + occasional reviewer; timelines are rough because your velocity varies.

### Phase 0 — Decision & Freeze (1 week)

1. Pin the decision: `shell/` is the target. Document it as an ADR (draft provided in `docs/adr/0011-adopt-plugin-first-shell.md` — see ADR deliverable).
2. Add a `DEPRECATED` banner at the top of `app/README.md` and `crates/nexus-app/src/lib.rs` pointing to `shell/`.
3. Tag the current legacy shell (`v0.1.0-legacy-shell`) so it's recoverable if the migration stalls.
4. Write a parity checklist derived from the ~95 `nexus-app` Tauri commands — this is the acceptance bar for retiring the legacy.

### Phase 1 — Bridge & Leaf Foundation (2–3 weeks)

Unblocks all downstream feature work.

1. **Complete Leaf/ViewRegistry implementation** per `leaf-migration-plan.md` Phases 0–4. Ship `Leaf.ts`, `View.ts`, `ViewRegistry.ts`, `WorkspaceStore`, `WorkspaceRenderer.tsx`. Persist to `<vault>/.forge/workspace.json`.
2. **Finalize bridge Phase 4 polish** per `shell-kernel-bridge-plan.md` — subscription cleanup on plugin deactivate, error surface from kernel to shell, deterministic shutdown.
3. **Wire the existing `@nexus/extension-api` package** (already scaffolded at `packages/nexus-extension-api/`, v1.0.0, unused). Regenerate its type surface from `nexus-plugin-api` via ts-rs so the Rust and TypeScript contracts cannot drift. Make it the single import path for both shells during migration, then for shell plugins after legacy is retired.
4. **Guardrail test** in `shell/src-tauri` that mirrors the workspace-level `dep_invariants.rs`: shell plugins cannot `import 'fs'` or reach into Tauri APIs outside `@nexus/extension-api`.

**Acceptance:** The shell boots empty, loads a single "hello-world" plugin that renders into the sidebar and can invoke `com.nexus.storage::list_dir`. Kernel bus events round-trip to the shell plugin.

### Phase 2 — Feature Parity Migration (6–10 weeks)

Walk the ~95-command parity checklist, moving each capability from `app/` → `shell/`. Most commands are already implemented as IPC handlers in service crates, so this is mostly frontend work: create a Nexus plugin in `shell/src/plugins/nexus/<feature>`, implement the UI against `api.kernel.invoke(...)`, and verify the kernel IPC handler covers all cases.

**Suggested order (driven by dependency chains in the planning docs):**

1. **Editor transactions** (`editor-transaction-wiring-plan.md` Phases 0–8). Includes CM6 mount, session refcount, undo delegation to kernel, echo suppression. Blocks everything that renders file content.
2. **File tree + file viewer + tab strip** — baseline workspace chrome.
3. **Notion block UX** (`notion-block-ux-plan.md` Phases 1–6). Much already shipped in `shell/src/plugins/nexus/editor/cm/`.
4. **Canvas** (`canvas-shell-plan.md` Phases 1–6, claimed complete). Validate against live kernel data.
5. **Bases** (`bases-shell-plan.md` Phases 1–6, claimed complete). Validate.
6. **Outline, backlinks, search panel** — simple `view` plugins against existing storage IPC handlers.
7. **Graph** — local first, then global (`global-graph-view-plan.md` Phase 1 kernel handler + Phases 2–4 frontend).
8. **AI chat / Agent panel** — event-driven streaming; the kernel event forwarders are already the model.
9. **Terminal** — xterm.js + `com.nexus.terminal::*` IPC handlers. Streaming output is the main gap (currently poll-based).
10. **Workflow / Skills / MCP management panels** — introspection UIs.
11. **Settings + keybindings editor** — wire to existing settings schemas.
12. **Command palette** — already registered as `nexus.commandPalette`; verify it aggregates from all loaded plugins.
13. **Status bar + activity bar + titlebar** — already plugins in `shell/src/plugins/core/`.

**Acceptance:** Every command in the parity checklist is reachable from the new shell, with equivalent UX. Run both shells side-by-side and confirm no regressions for a representative workflow (open forge → edit note → search → ask AI → run workflow).

### Phase 3 — Security Hardening (3–4 weeks)

These are gates for any public marketplace; scope them before any external plugins ship.

1. **JS plugin sandbox** (UI F-8.1.1). Iframe-per-plugin, `sandbox="allow-scripts"`, no `allow-same-origin`. Re-enable CSP in `tauri.conf.json`. Postmessage-based RPC proxy that enforces capabilities.
2. **Install-time capability prompt** (MK F-5.1.1). UI modal shows requested capabilities with risk levels; user approves. Persist approval per plugin.
3. **TOCTOU fixes** (MK F-5.3.1, F-5.3.2). Replace canonicalize-parent with `ForgePathValidator` in `host::write_file` and `KernelPluginContext::write_file`.
4. **`api_version` range check** (MK F-9.2.1, UI F-9.1.1) at `PluginLoader::load_manifest`. Reject incompatible plugins with a useful error.
5. **Separate plugin contract crate** (MK F-2.1.1). Already partially done — `nexus-plugin-api` exists — but verify community plugins don't transitively link against `nexus-kernel` internals. Guardrail test.
6. **Per-plugin crash quarantine** (stretch). Catch panics in `onInit`/`onActivate`/command handlers so one plugin can't abort the registration loop.

### Phase 4 — Frontend Unification Polish (2 weeks)

Keep CLI/TUI/MCP/Desktop all in sync.

1. **Shared command taxonomy.** Audit `nexus content search`, `com.nexus.storage::search`, MCP `forge_search`, and the shell search plugin. Normalize argument names and return shapes. One JSON schema per IPC handler, consumed by all four frontends.
2. **Retire `crates/nexus-app`.** Delete `app/` and the `nexus-app` member from the workspace `Cargo.toml`. Announce in the README.
3. **Fold CLI/TUI launcher flags into shell.** `nexus` binary should be able to launch the desktop shell (`nexus desktop`) or stay in headless mode (`nexus content search ...`). One binary, multiple modes, same kernel.
4. **Plugin dev experience.** `nexus plugin scaffold` should emit a plugin manifest + a TS shell-side plugin stub + a WASM stub in one go, tied together by the manifest ID.
5. **MCP parity check.** Ensure every MCP tool has a corresponding shell command and CLI subcommand. Currently MCP exposes 13 tools; the shell should expose all of them too.

### Phase 5 — v1 Polish & Ship (3–4 weeks)

1. **Auto-update.** Tauri updater integration, signing, release channel.
2. **Crash reporting & telemetry.** Opt-in, respect file-as-truth (never upload note content).
3. **Bundled core plugin set.** A curated list of `nexus.*` plugins that ship by default and are updatable via the marketplace.
4. **Marketplace (minimal).** Static JSON index of approved community plugins, installable via `nexus plugin install <id>`. Marketplace UI in the shell.
5. **Documentation pass.** Update `README.md`, archive the legacy `app/README.md`, publish `shell/docs/` to a website, write a "Writing your first Nexus plugin" tutorial.
6. **Beta → GA.** Two-week beta with the test group, triage, release `v1.0.0`.

**Total estimate:** ~16–24 weeks for one engineer working full-time, with capacity variance. Phase 2 is the largest by far.

---

## 6. Key Risks

1. **Leaf/ViewRegistry implementation slippage.** The design is clean but the React/TypeScript implementation is non-trivial (serializable `setViewState`, drag-drop, split/join). If this stalls, every downstream feature plugin stalls with it. Mitigation: ship Phase 1 end-to-end with a single throwaway plugin before starting parity work.
2. **Feature-parity drift during Phase 2.** New capabilities will land in `app/` because it's the one that "works." Mitigation: enforce the freeze in Phase 0 — all new capability work goes into the service-layer IPC handlers and the new shell, never `crates/nexus-app`.
3. **JS plugin sandbox timeline.** Moving plugins into iframes changes the plugin API non-trivially; existing first-party plugins (`nexus.editor`, `nexus.canvas`, etc.) will need to be rewritten against `postMessage` RPC. Mitigation: scope this carefully; consider doing it for community-tier plugins only initially and keep first-party plugins in the main webview until the API stabilizes.
4. **Kernel as in-process dependency.** Currently the Tauri binary embeds the kernel. Per the bridge plan, this is intentional ("shell proves itself before cutover to separate process"). If a future split to a separate process is needed (e.g., for multi-window or remote forges), the current in-process assumption will bite. Mitigation: keep the bridge protocol JSON-serializable end-to-end (no Rust-native types crossing) so a process split is mechanical.
5. **Community plugin API instability during migration.** ADR-level changes to `nexus-plugin-api` during Phase 2 break plugins. Mitigation: freeze `nexus-plugin-api` surface at Phase 1 and use the deprecation policy in `DEPRECATED.md` for any evolution.

---

## 7. Appendix: Cited Paths

- `Cargo.toml` — workspace root (note `exclude = ["shell"]`)
- `README.md`, `DEPRECATED.md`, `SESSION_SUMMARIES.md`
- `crates/nexus-kernel/src/{lib,kernel,context_impl,event_bus,ipc}.rs`
- `crates/nexus-bootstrap/src/lib.rs` (1008 LOC, registers 13 core plugins)
- `crates/nexus-plugin-api/src/lib.rs`
- `crates/nexus-app/src/{lib,main,commands,forge,editor,ai,agent,terminal,plugins,workflow,skills,database,keybindings,persistence,uri}.rs`
- `app/src/{main.tsx,App.tsx,components/,stores/,ipc/,plugins/scriptRuntime.ts}`
- `shell/README.md`, `shell/src/main.tsx` (272 LOC, 32 plugin registrations)
- `shell/src-tauri/{Cargo.toml, src/{lib,bridge,persistence}.rs}`
- `shell/src/plugins/{core/*, nexus/*, community/hello-world}`
- `docs/{leaf-architecture,leaf-migration-plan,shell-kernel-bridge-plan,editor-transaction-architecture,editor-transaction-wiring-plan,canvas-shell-plan,bases-shell-plan,global-graph-view-plan,notion-block-ux-plan,tab-context-menu-plan,FORGE-UI-PLAN,MICROKERNEL-AUDIT,UI-AUDIT,ARCHITECTURE}.md`
- `docs/adr/` — ten architecture decision records (0001–0009 plus index)
- `docs/PRDs/` — 17 PRDs, particularly 01 (Kernel), 04 (Plugin System), 05 (CLI), 07 (Theming & UI), 14 (MCP)
