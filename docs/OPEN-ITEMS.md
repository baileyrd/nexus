# Open Items — Post-Migration Carryover Gaps

> Capabilities described in legacy `app/` documentation that were not carried over to `shell/` during the Phase 4 WI-37 retirement (2026-04-24). Surfaced by the capability-presence sweep on 2026-04-24.
>
> Listed here rather than in [PRDs/BACKLOG.md](PRDs/BACKLOG.md) because these are regressions from prior-shipped behavior, not new features. Linked from BACKLOG.md under "Post-migration carryover gaps."

---

## OI-01 — Settings modal / `registerSettingsTab` API

**Severity:** Should-fix (user-visible parity gap)
**Surfaced by:** `docs/references/obsidian-settings-modal.md`
**Status:** Resolved 2026-04-24. Extension point landed; built-in tabs preserved; typecheck + 271 tests green.

### Outcome
- **`SettingsTabRegistry`** in `shell/src/registry/`, following the `CommandRegistry` two-phase pattern (`registerFromManifest` → `register` → `all`). Six unit tests cover the sort order, manifest-then-register sequence, and `unregister`.
- **`SettingsTabContribution`** DTO in `@nexus/extension-api`; **`SettingsTabEntry`** shell-side shape. `PluginContributions.settingsTabs` accepts the declarative form; `ExtensionHost.registerManifestContributions` calls `registerFromManifest` so the rail entry appears before the plugin activates.
- **`api.settings.registerTab(id, renderer, meta?)`** on `PluginAPI`, tracked by `PluginRegistry` so unloads sweep the tab automatically (`settingsTab:<id>` ownership key).
- **`SettingsPanelView`** renders three classes of tabs: the four existing built-ins (Settings / Appearance / Keybindings / Plugins), plugin-contributed tabs (from the registry), and **auto-tabs** for any plugin that declared a `configuration` schema without an explicit `settings_tabs` entry — matches the Obsidian "one tab per plugin with settings" convention. Plugins with both paths get the explicit tab only.
- **Last-open tab persisted** to `plugin:core.settings:last-tab` in `localStorage` (same namespacing as keybinding overrides), hydrated on the next open. `Ctrl/Cmd+,` continues to route through `workbench.action.openSettings`.

### Follow-up (not blocking)
- The header still uses a horizontal tab bar. The Obsidian spec's vertical left rail with uppercase section headers (`OPTIONS` / `CORE PLUGINS` / `COMMUNITY PLUGINS`) is visual polish; the grouping metadata already exists in the registry so the rail-style layout is a styling-only follow-up.
- Auto-tab `core` vs `community` classification currently uses a `com.nexus.` / `core.` prefix heuristic. A richer split would join on `pluginList` which surfaces the real `core: boolean`.

---

## OI-02 — Split-size persistence

**Severity:** Should-fix (UX regression)
**Surfaced by:** `docs/archive/superpowers/specs/2026-04-17-split-size-persistence-design.md`
**Status:** Resolved 2026-04-24. The editor-split gap is closed; sidedock persistence was already landed post-migration but mis-attributed here.

### Audit correction
When I went to implement the scope from the original OI-02 text, I found that two of the three claims had already been fixed:

- **Sidedock resize + persistence already works.** `WorkspaceRenderer.DockResizeHandle` drives `workspace.setSidedockSize`, which emits `layout-change` → `installAutoSave` debounces a write through `workspace.serialize()` → `<vault>/.forge/workspace.json`. Hydrate reads it back via `hydrateNode`. So "sidebar / right panel / bottom panel do not report drag-end" is stale — those three do.
- **Sizes schema already exists.** `SerializedSplit.sizes?: number[]` is serialised per node in the workspace JSON. No rust-side `split_sizes` shell-state field was needed — the design doc predates the `.forge/workspace.json` persistence channel.
- **Genuine remaining gap:** editor internal splits (`SplitNode` at `shell/src/workspace/WorkspaceRenderer.tsx`) render children with flex weights but had **no drag handle** — so `node.sizes` was never mutated by user interaction, meaning a user couldn't actually resize an editor split at all.

### Outcome
- **`workspace.setSplitSizes(splitId, sizes)`** mutator in `workspaceStore.ts` — walks the tree for the named split, guards arity, clamps per-child weight to `MIN_SPLIT_WEIGHT` (0.1) so a lane can't vanish, dedupes identical writes, emits `layout-change`.
- **`SplitResizeHandle`** in `WorkspaceRenderer.tsx` between each pair of `SplitNode` children. Drag math: capture pixel rects of all children at mousedown, transfer the delta between the two adjacent lanes on move, normalise to proportional weights, call `setSplitSizes`. Matches `DockResizeHandle` styling (4px gutter).
- Persistence path is unchanged — writes flow through the existing `installAutoSave` → `saveWorkspace` pipeline, and hydrate reads `split.sizes` back.
- Five unit tests cover: write + event, arity-mismatch rejection, weight clamp, unknown-id no-op, idempotent re-write.

### Clean-uninstall behavior (AC #2)
`.forge/workspace.json` missing → `loadWorkspace` returns null → `buildDefaultLayout` creates a fresh tree without `sizes`, so `SplitNode` falls back to equal-flex (weight 1 per child). No errors, no panics. Also verified for the separate shell-state path: `get_shell_state` returns default on missing file (unchanged from prior behaviour).

---

## OI-03 — Workspace-wide clippy `-D warnings` sweep

**Severity:** Tech debt (blocks strict-CI adoption)
**Surfaced by:** audit 2026-04-24
**Status:** Resolved 2026-04-24. `cargo clippy --workspace --no-deps --all-targets -- -D warnings` exits 0; all tests pass.

### Outcome
Swept every crate — `nexus-security`, `nexus-ai`, `nexus-agent`, `nexus-workflow`, `nexus-mcp`, `nexus-storage`, `nexus-database`, `nexus-plugins`, `nexus-bootstrap`, `nexus-terminal`, plus follow-ons in `nexus-git`, `nexus-kv`, `nexus-theme`, `nexus-editor`, `nexus-skills`, `nexus-kernel`, `nexus-cli`.

Suppressions added carry a one-line justification; they're concentrated on deserialization helpers (`struct_field_names` for fields that mirror JSON/TOML keys), `needless_pass_by_value` on functions used as `map_err` function pointers, `too_many_lines` on single-jump-table `dispatch` methods, and `missing_errors_doc` at the `nexus-bootstrap` crate level (27 pass-through wrappers with identical failure modes).

Leftover open follow-up: consider flipping CI on with `-D warnings` now that the floor is clean.

---

## OI-04 — Load-bearing TODOs for kernel contract promotion

**Severity:** Design debt (type duplication)
**Surfaced by:** audit 2026-04-24
**Status:** Resolved 2026-04-24. Both TODOs cleared.

### Outcome

**TODO 1 — `SlotId` promotion** (`shell/src/types/plugin.ts:75`). `SlotId` and `ViewContribution` moved from the shell registry layer into `packages/nexus-extension-api/src/index.ts`. The shell-side `SlotRegistry.ts` now imports + re-exports `SlotId` so in-tree consumers keep their existing paths. `shell/src/types/plugin.ts` re-exports both types from the package alongside the other portable contribution DTOs (`CommandContribution`, `SettingsTabContribution`, etc.). Native (Rust) and community (JS) plugins see identical slot names at the contract boundary.

**TODO 2 — `list_archetypes` IPC** (`shell/src/plugins/nexus/agent/agentStore.ts:104`). New `HANDLER_LIST_ARCHETYPES` (id 8) on `com.nexus.agent` returns the short-name catalogue (`["writer", "coder", "researcher"]`) — the exact strings `resolve_prompt` accepts back as the `archetype` arg, so the shell picker round-trips them verbatim. Served by the sync `dispatch` path; `dispatch_async` returns `None` for this handler so the kernel's `ipc_call` drops straight to the blocking-pool shortcut (no tokio frame for a compile-time constant). Bootstrap manifest registers the new command.

Shell side:
- `KNOWN_ARCHETYPES` hardcoded catalogue deleted; replaced by `FALLBACK_ARCHETYPES` (same three entries, installed at store init so the picker renders on first paint) plus `describeArchetype(id)` which joins ids to a shell-side label/description lookup.
- `ArchetypeId` widened from `'writer' | 'coder' | 'researcher'` to `string` so a new Rust-side archetype surfaces without a shell release.
- `loadArchetypes()` runtime helper fires on workspace open, calls the IPC, overwrites the fallback. Idempotent via `archetypesLoaded`; `reset()` deliberately preserves the catalogue across workspace close/open since it's kernel-global.
- `AgentView.ArchetypeSelect` reads `useAgentStore(s => s.archetypes)` rather than the deleted const.

### Acceptance
- Native and community plugins now share `SlotId` / `ViewContribution` shapes through `@nexus/extension-api`.
- The agent archetype picker is driven by the kernel; a Rust-side addition to `ARCHETYPE_NAMES` + `resolve_prompt` shows up in the shell without touching the frontend.
- Per-target label strings remain shell-side (path c), mapped via `ARCHETYPE_DISPLAY`; unknown ids get a titlecased fallback so new Rust entries don't vanish from the dropdown.

### Coverage
- Rust: 2 new unit tests (`list_archetypes_returns_short_names`, `dispatch_async_yields_to_sync_for_list_archetypes`). `cargo clippy --workspace -- -D warnings` still exits 0.
- Shell: 8 new unit tests covering `decodeArchetypes` (happy path, unknown ids, non-array, empty, dedupe) and `loadArchetypes` (success, failure-keeps-fallback, idempotency). Full suite: 284 tests green; typecheck clean; production build succeeds.

---

## OI-05 — Rust dependency duplication

**Severity:** Build debt (compile time + binary size)
**Surfaced by:** audit 2026-04-24
**Status:** Blocked on upstream. Every duplicate identified on 2026-04-24 traces back to a transitive dependency we don't own.

### Upstream blockers
Reverse-tree walk via `cargo tree -i`:
- **`wasmtime` 42.0.2** (pulled via `nexus-plugins`) pins `toml 0.9`, `sha2 0.10`, `digest 0.10`, `rand_core 0.6`, `reqwest 0.13`, `rustix 0.38`, `nix 0.28`, `hashbrown 0.15/0.16/0.17`, plus wasmtime-internal crates (`pulley-interpreter`, `wasmtime-internal-core`, `cranelift-bitset`) and `wasm-encoder`/`wasmparser` 0.244. Resolving any of these requires a wasmtime point release that itself upgrades them.
- **`portable-pty`** (via `nexus-terminal`) pulls `filedescriptor` which pins `thiserror 1.0`. Upgrading portable-pty or switching PTY crates is a feature-level decision, not a drop-in bump.
- The "identical version twice" rows (`bitflags 2.11.1`, `semver 1.0.28`, `libc 0.2.185`, etc.) are feature-flag splits inside wasmtime/Tauri — same version, two feature configurations.

### When to revisit
- Next wasmtime major release — re-run `cargo tree --duplicates` and sweep anything that unified as a side effect.
- If the editor/terminal stack picks up a new PTY crate that doesn't depend on `filedescriptor`, `thiserror` 1.0 goes away.
- Any direct dependency we add that pulls the older version of one of these families should be resisted — keep the forge on the newer half so the cleanup lands automatically when upstream moves.

---

## OI-06 — ESLint 8 / typescript-eslint 7 upgrade

**Severity:** Tooling debt (ESLint 8 is EOL)
**Surfaced by:** audit 2026-04-24
**Status:** Resolved 2026-04-24. `pnpm lint` exits 0; all three ACs met.

### Outcome
- **ESLint 8.57 → 9.39** + **typescript-eslint 7 → 8.59** in `shell/package.json`. Added `typescript-eslint` (the flat-config meta package) + `eslint-plugin-react-hooks ^5.2` for React correctness.
- **`shell/eslint.config.js`** pins the config at the package root. ESLint 9 flat-config search stops at the nearest `eslint.config.{js,ts,mjs}`, so the personal `~/.eslintrc.json` that was shadowing `pnpm lint` is no longer reachable — the shadowing bug is structurally gone. Presets: `tseslint.configs.recommended` + `react-hooks/recommended`. `no-explicit-any` and `exhaustive-deps` set to `warn` so they surface without blocking CI; `no-unused-vars` honours the underscore-prefix convention already used in the codebase.
- **`lint` script** updated to drop the `--ext` flag (flat config reads file patterns from the config itself).
- **xterm → @xterm**: `xterm 5.3.0` + `xterm-addon-fit 0.8.0` replaced with `@xterm/xterm ^5.5` + `@xterm/addon-fit ^0.10`. Three imports in `TerminalView.tsx` updated to the scoped names (including the CSS import).
- **One real bug surfaced + fixed**: `CapabilityModalView.CapBucketSection` called `useMemo` after an early `return null`, a rules-of-hooks violation under `react-hooks/rules-of-hooks`. Hook moved above the guard.

### Acceptance
- `pnpm lint` exits 0 (0 errors, 46 advisory warnings — long-standing unused-var / explicit-any sites that pre-date this session).
- ESLint + typescript-eslint off the deprecated 8.x / 7.x lines.
- xterm packages on the `@xterm/*` scoped names.

Typecheck clean; 289 tests pass (unchanged); production build succeeds.

---

## OI-07 — Route capability grants, denials & path-traversal through `audit::*`

**Severity:** Should-fix (auditability gap for third-party-untrusted trust model)
**Surfaced by:** MICROKERNEL-AUDIT.md F-10.1.2 reconciliation 2026-04-24
**Status:** Resolved 2026-04-24. Every capability grant/denial and path-traversal rejection now passes through `audit::*`; coverage tests assert the structured channel sees the events.

### Structural finding
`nexus-security` already depends on `nexus-kernel` and `nexus-plugins`, so the call sites couldn't import `nexus_security::audit` without inducing a dep cycle. That cycle is exactly why the helpers had zero callers — the audit module sat *above* the gates in the dep graph. Fix: moved `audit.rs` from `nexus-security` down to `nexus-kernel` (the helpers only need `tracing` + `std::path`), and re-exported via `pub use nexus_kernel::audit;` from `nexus-security` so `nexus_security::audit::*` keeps working for outside callers (e.g. `prd-02-smoke`).

### Outcome
- **Capability denials** route through `audit::log_capability_denied` from:
  - `crates/nexus-plugins/src/host_fns.rs::deny_capability` — the WASM host's KvRead/KvWrite/EventsPublish/FsRead/FsWrite/IpcCall/UiNotify gates.
  - `crates/nexus-kernel/src/context_impl.rs::require_capability` — the native plugin context's FS/KV/events/log gates.
  - `crates/nexus-kernel/src/context_impl.rs::ipc_call` — previously silent on `IpcCall` denial; now emits an audit event before returning `IpcError::CapabilityDenied`.
- **Path-traversal rejections** route through `audit::log_path_traversal_denied` from:
  - `host_fns.rs::deny_path_traversal` (WASM `write_file` validator failures).
  - `context_impl.rs::confine_path` (the canonicalize-then-prefix denial).
  - `context_impl.rs::write_file` (the TOCTOU-safe `validate_for_write` denial).
- **Capability grants** are emitted in `crates/nexus-plugins/src/loader.rs::build_capabilities` — one `audit::log_capability_granted` per granted capability per plugin, for both Core (full set) and Community (post-HIGH-risk-filter) loads.
- The crate-level path validator (`crates/nexus-security/src/path.rs`) is a pure error shim with no logging — call-site logging is the right home and now happens in both gates above.

### Coverage
- `audit::test_support::with_captured_events{,_async}` promoted to a `pub(crate)`, `#[cfg(test)]` helper so call-site tests can install a tracing subscriber and read back captured events.
- Two new gate-integration tests in `context_impl.rs::tests`: `capability_denial_emits_audit_event_through_gate` (calls `kv_get` on a no-cap context, asserts an `audit=true result=denied capability=kv.read` event) and `path_traversal_emits_audit_event_through_gate` (calls `read_file("/etc/passwd")`, asserts the traversal event reaches the channel). Both prove the gate → helper → tracing path end-to-end, not just the helper in isolation.
- Workspace: `cargo clippy --workspace --no-deps --all-targets -- -D warnings` exits 0; full test sweep green (~1300 tests).

### Acceptance (note: AC text amended)
The original AC mentioned filtering on `target = "audit"`. The implemented audit helpers (which predate this OI) emit a structured `audit = true` field instead — a richer model that survives format-layer reformatting. AC reads as: "every grant/denial/traversal rejection passes through an `audit::*` helper, and a subscriber filtering on `audit = true` sees all security-relevant events in one stream." Both halves now hold; the two coverage tests above filter exactly that way.

---

## OI-08 — "Running Extensions" Settings tab

**Severity:** Should-fix (observability)
**Surfaced by:** UI-AUDIT.md F-10.1.1 reconciliation 2026-04-24
**Status:** Partial

### Gap
`shell/src/plugins/nexus/processes/ProcessesView.tsx` surfaces plugin + event state in a pane, but it isn't a Settings tab and doesn't show per-plugin error messages, slowest-command observations, or a disable action.

### Scope
- Register a Settings tab via `api.settings.registerTab` (shipping as of OI-01) titled "Extensions".
- Show: plugin id, state (`active`/`error`/`inactive`), last error if any, declared capabilities, a Disable button, and the last N command durations.
- Surface the same `ExtensionHost.getError(id)` data `ProcessesView` already has, plus a `performance.measure` read once OI-17's instrumentation lands (or inline a minimal duration cache).

### Acceptance
- Settings → Extensions tab lists every loaded plugin with state + last error.
- A failing plugin shows its error message inline; clicking Disable flips `plugin enabled`.

---

## OI-09 — `plugins:status` store + per-plugin error surface

**Severity:** Should-fix (crash-failure observability)
**Surfaced by:** UI-AUDIT.md F-7.2.1 reconciliation 2026-04-24
**Status:** Partial — `ExtensionHost` already marks `'error'` state but no UI consumer

### Gap
`shell/src/host/ExtensionHost.ts:151-165` wraps each plugin's `activate()` in try/catch and stores the thrown error via `fail(id, error)`. The error is event-emitted (`plugin:error`), but no shell-side store aggregates it and no UI tab lists the failed plugins. Users currently hit a silent broken plugin that surfaces only in the dev console.

### Scope
- Zustand store `pluginsStatusStore` that subscribes to `plugin:error` / `plugin:activated` / `plugin:deactivated` and keeps a per-plugin `{ state, lastError }` map.
- Consumed by OI-08's Settings tab to render failed plugins with a Disable action.

### Acceptance
- A plugin that throws in `activate()` appears in the Extensions tab with its error message.
- Disabling the plugin removes it from the failed list and marks it `inactive` on next boot.

---

## OI-10 — Keybinding-conflict detection + UI

**Severity:** Should-fix (user-invisible collision hazard)
**Surfaced by:** UI-AUDIT.md F-4.1.1 reconciliation 2026-04-24
**Status:** Not started

### Gap
`KeybindingRegistry.registerFromManifest` pushes bindings unconditionally; two plugins that declare the same chord both land in the map and `match()` returns whichever was registered first. No event is emitted and the user has no way to see the collision.

### Scope
- `KeybindingRegistry.register{FromManifest,Override}` should detect a chord collision and emit a `plugins:keybindings-conflict` event with `{ chord, commands: [ids] }`.
- Settings → Keybindings (already exists as a built-in tab) shows a warning row per conflict with a "pick one" dropdown that records a user override on the losing command.

### Acceptance
- Install two plugins that share `Ctrl+K Ctrl+S`; the Keybindings tab shows a conflict row with both command titles.
- Picking one writes a user override on the other (unbound or re-bound) and the event clears.

---

## OI-11 — UI-thread time budget on plugin command dispatch

**Severity:** Should-fix (UX hazard from slow plugins)
**Surfaced by:** UI-AUDIT.md F-8.2.1 reconciliation 2026-04-24
**Status:** Not started

### Gap
`api.commands.execute(id)` awaits a plugin's command handler indefinitely. A community plugin with a slow synchronous body stalls the shell until it resolves. Kernel dispatches already enforce a `timeout_ms` (30 s default); shell-side command dispatch does not.

### Scope
- Wrap `CommandRegistry.execute` in an await-with-timeout (soft warn at 250 ms, hard cancel at 5 s configurable via the existing configuration registry).
- On cancel, publish `command:cancelled` with the command id so the palette can dismiss the in-flight state.

### Acceptance
- A command handler that sleeps 6 s is cancelled with a user-visible notification; the shell stays responsive.

---

## OI-12 — Document or remove absolute-path auto-promotion

**Severity:** Should-fix (silent capability escalation)
**Surfaced by:** MICROKERNEL-AUDIT.md F-6.3.1 reconciliation 2026-04-24
**Status:** Not started

### Gap
`crates/nexus-kernel/src/context_impl.rs:142-154` silently auto-promotes an absolute path in `read_file` / `write_file` to the corresponding `FsReadExternal` / `FsWriteExternal` capability check — behaviour not documented on the plugin-API contract. A plugin whose author thinks `FsRead` is enough can unexpectedly hit `CapabilityDenied`, or worse, if the plugin declares `FsReadExternal`, can silently escape the forge root.

### Scope
- Pick one: (a) keep auto-promotion but document it in `@nexus/extension-api` JSDoc on `PlatformFsAPI.read/write`, OR (b) remove auto-promotion and require plugins to pick their capability explicitly.
- If (a), add an audit-log line per auto-promotion (fits with OI-07).

### Acceptance
- A plugin author reading the extension-api docs knows whether an absolute path escalates to `*External` silently or fails loudly.
- If (b): `read_file("/abs/path")` without `FsReadExternal` returns a typed error, not a silent denial.

---

## OI-13 — Reconcile kernel-side `PluginRegistry` with `PluginLoader::loaded`

**Severity:** Tech debt (dead code path + two sources of truth)
**Surfaced by:** MICROKERNEL-AUDIT.md F-1.2.1 reconciliation 2026-04-24
**Status:** Not started

### Gap
`crates/nexus-kernel/src/plugin_registry.rs` still exists and `Kernel::plugins()` (`kernel.rs:139`) returns it — but nothing populates it at runtime; `PluginLoader::loaded` in `nexus-plugins` is the authoritative map. Callers that reach for `kernel.plugins()` get an empty view.

### Scope
- Either delete `kernel-side PluginRegistry` + `Kernel::plugins()` (preferred; zero callers today), OR delegate `Kernel::plugins()` to `PluginLoader::loaded`.
- Update the microkernel ADR if deletion is chosen.

### Acceptance
- Zero references to `nexus_kernel::PluginRegistry` outside `plugin_registry.rs` itself (already true) — plus the module itself is gone.

---

## OI-14 — Expose `ctx.workspace` / `ctx.editor.active` through extension-api

**Severity:** Should-fix (forces plugins to use raw `invoke`)
**Surfaced by:** UI-AUDIT.md F-6.1.1 reconciliation 2026-04-24
**Status:** Resolved 2026-04-26.

### Outcome
- **`api.workspace.forgeRoot(): string | null`** added to the leaf workspace facade in [`shell/src/workspace/workspaceStore.ts`](../shell/src/workspace/workspaceStore.ts) — reads the active forge root from `useWorkspaceStore` (the `nexus.workspace` plugin store) so plugins don't have to import shell-internal stores. Returns null between `workspace:closed` and the next `workspace:opened`.
- **`api.editor.active(): { relpath, revision } | null`** + **`api.editor.onChange(handler): () => void`** wired in [`shell/src/host/PluginAPI.ts`](../shell/src/host/PluginAPI.ts). `active()` projects `useEditorStore.{activeRelpath, sessionRevision}` into the public shape; `onChange` subscribes via `useEditorStore.subscribe` and dedupes redundant fires through `activeEditorEquals` so handlers only run on a real switch or revision advance. The returned disposer is idempotent and tracked through `PluginRegistry.trackSubscription` so plugin unload sweeps it (mirrors `kernel.on`).
- **Pure projection helpers** extracted to [`shell/src/host/activeEditor.ts`](../shell/src/host/activeEditor.ts) (`computeActiveEditor`, `activeEditorEquals`) so unit tests don't need to drag in `@tauri-apps/*` imports through PluginAPI.ts.
- **Type contract** in [`packages/nexus-extension-api/src/index.ts`](../packages/nexus-extension-api/src/index.ts) — new exported `ActiveEditor`, `EditorAPI`, `WorkspaceAPI` interfaces. The previous aspirational `editorActive` / `workspace.{root,name}` shape (never wired) is replaced; both had zero consumers.

### Coverage
- 10 new unit tests in [`shell/src/host/PluginAPI.editor.test.ts`](../shell/src/host/PluginAPI.editor.test.ts) — projection helpers (3), equality predicate (5), end-to-end dedupe over `useEditorStore` mutations (1), idempotent-disposer pattern (1). All pass.
- No new typecheck or lint errors. Subscription cleanup behaviour is already covered by [`shell/tests/subscription-cleanup.test.ts`](../shell/tests/subscription-cleanup.test.ts) since the disposer flows through the same `PluginRegistry.trackSubscription` path.

### Acceptance
- A plugin reads `api.editor.active()` to get `{ relpath, revision }` without knowing about `com.nexus.editor`'s handler ids.
- A plugin subscribes via `api.editor.onChange(handler)` and is auto-unsubscribed on deactivate; redundant store mutations don't trigger redundant handler fires.
- A plugin reads `api.workspace.forgeRoot()` to get the current forge path without importing shell stores.

---

## OI-15 — Manifest signature / provenance

**Severity:** Should-fix (marketplace prerequisite)
**Surfaced by:** MICROKERNEL-AUDIT.md F-3.2.2 reconciliation 2026-04-24
**Status:** Not started

### Gap
`crates/nexus-plugins/src/manifest.rs::parse_manifest` accepts any valid TOML. For a marketplace that ships community plugins, there's no way to verify who signed a given `manifest.toml` — install-time users depend on file-system trust alone.

### Scope
- Optional `manifest.toml.sig` Ed25519 signature over the manifest bytes, verified against a trusted-publisher keyring bundled with the shell.
- Unsigned manifests still load (backward compatible) but the `Plugins` tab shows an "Unverified" badge.
- This is a blocker for opening a community registry.

### Acceptance
- A plugin signed by a trusted key shows "Verified" in Plugins tab; unsigned shows "Unverified"; signed-but-untrusted is rejected with a clear error.

---

## OI-16 — `beforeunload` → `onStop` for script plugins

**Severity:** Nice-to-have (cleanup on window close)
**Surfaced by:** UI-AUDIT.md F-7.3.1 reconciliation 2026-04-24
**Status:** Not started

### Gap
Script plugins register `onStop` handlers that run on explicit deactivation but never on window close — a graceful shutdown hook is missing. Plugins that flush state to disk (cache, preferences) lose last-edit data on quit.

### Scope
- `shell/src/shell/App.tsx` `beforeunload` listener dispatches `window:closing` event.
- `ExtensionHost` subscribes; for each active plugin, await `deactivate()` with a 1 s per-plugin soft cap so a misbehaving plugin can't stall the close.

### Acceptance
- A plugin with a flush-on-stop hook writes its state when the user hits ⌘Q.

---

## OI-17 — Deprecation policy + `@deprecated` JSDoc on contribution DTOs

**Severity:** Should-fix (API evolution hygiene)
**Surfaced by:** UI-AUDIT.md F-9.3.1 reconciliation 2026-04-24
**Status:** Not started

### Gap
`@nexus/extension-api` is stamped `1.0.0` but there is no deprecation channel. Removing a DTO field mid-1.x would silently break plugins.

### Scope
- `packages/nexus-extension-api/DEPRECATED.md` lists deprecated surfaces with a target removal version.
- `@deprecated` JSDoc tag on each deprecated field in `src/index.ts`.
- ESLint rule `no-restricted-syntax` (or `import/no-deprecated` via the typescript-eslint plugin) fails the shell lint when it imports a deprecated name.

### Acceptance
- A plugin author gets a TS compile-time warning when they use a deprecated DTO.

---

## OI-18 — Snippet trigger collision detection

**Severity:** Nice-to-have (silent overwrite hazard)
**Surfaced by:** UI-AUDIT.md SI-7 reconciliation 2026-04-24
**Status:** Not started

### Gap
Two plugins that register snippets with the same trigger string silently overwrite each other — the same hazard OI-10 describes for keybindings but for snippets.

### Scope
- The snippet-registration path (in the appearance/theme snippet store today) checks for duplicate triggers and emits `plugins:snippet-conflict`.
- Settings → Appearance (or a new Snippets section) surfaces the conflict with a per-trigger "which plugin wins" control.

### Acceptance
- Install two plugins with the same snippet trigger; the conflict is visible and resolvable before the user types the trigger.

---

## OI-19 — Defer createRoot/unmount in pane views

**Severity:** Nice-to-have (warnings only — no functional breakage today, but a real concurrency hazard)
**Surfaced by:** Manual smoke test 2026-04-27 — collapsing/reopening the bottom drawer with the terminal mounted prints two React warnings per re-home.
**Status:** Not started

### Gap
`Leaf.attachContainer` re-homes a view to a fresh container via `await view.onClose(); await view.onOpen(el)` (see `shell/src/workspace/Leaf.ts:186-189`). Both `TerminalPaneView` (`shell/src/plugins/nexus/terminal/TerminalPaneView.tsx:28-31`) and `EmptyView` invoke `root.unmount()` and `createRoot(el)` synchronously inside those calls. Because `attachContainer` runs from a `LeafHostInner` `useEffect` whose work overlaps with React 18's commit phase elsewhere in the tree, this trips two warnings:
- "Attempted to synchronously unmount a root while React was already rendering."
- "You are calling ReactDOMClient.createRoot() on a container that has already been passed to createRoot() before."

The warnings fire on every sidedock collapse/reopen and every leaf move; xterm currently survives because it re-mounts cleanly, but the race is real and will eventually drop input or duplicate roots under heavier workspace churn.

### Scope
- Wrap `root.unmount()` in `queueMicrotask(() => root.unmount())` inside `TerminalPaneView.onClose` and `EmptyView.onClose` (or whatever the cleanest defer primitive is for these views).
- Re-create the root only after the deferred unmount has actually run — either by chaining the microtask or by storing the new root creation in the same microtask.
- Audit the rest of `shell/src/plugins/**` for other `ViewBase` implementations doing imperative `createRoot` and apply the same pattern.

### Acceptance
- Collapse and reopen the bottom drawer with the terminal panel mounted; no React warnings in the console.
- Drag the terminal leaf between sidedock and main split; no warnings.
- xterm session state (scrollback, cursor) still survives the round-trip.

---

## OI-20 — Terminal copy/paste

**Severity:** UX gap (basic terminal expectation)
**Surfaced by:** Manual smoke test 2026-04-27 — terminal panel has no copy/paste wired up; selection works (xterm built-in) but there is no way to get the selection onto the clipboard or paste from it.
**Status:** Not started

### Gap
`shell/src/plugins/nexus/terminal/TerminalView.tsx` mounts xterm with default options and never wires `navigator.clipboard` reads/writes, never registers keybindings for copy/paste, and never adds a context-menu action. Plain `Ctrl+C` must stay reserved for SIGINT to the PTY (the existing `send_raw_input` path), so the convention every terminal emulator follows is **Ctrl+Shift+C / Ctrl+Shift+V** on Linux/Windows and **Cmd+C / Cmd+V** on macOS. Bracketed-paste mode (`\e[200~ … \e[201~`) should be honored when the shell enables it so multi-line pastes don't accidentally execute prematurely.

### Scope
- Add a copy keybinding: when xterm has a non-empty selection, write `term.getSelection()` to `navigator.clipboard.writeText(...)`. Fall back to `@tauri-apps/plugin-clipboard-manager` if the Web API is denied (Tauri webview permissions).
- Add a paste keybinding: read clipboard text and forward to `send_raw_input` via the existing IPC path. Wrap in `\e[200~ … \e[201~` only when the shell has signaled bracketed-paste mode (xterm exposes this via `term.modes.bracketedPasteMode`).
- Add a right-click context menu (or at least a right-click → paste handler) so users without keyboard chords can still paste.
- Document the keybindings in the manifest's `keybindings` contribution so they show up in Settings → Keybindings and respect user overrides.

### Acceptance
- Select a region in the terminal, hit `Ctrl+Shift+C` (or `Cmd+C` on macOS); paste into another app — content matches.
- Copy text from another app, hit `Ctrl+Shift+V` (or `Cmd+V`); shell receives the text. With bracketed paste enabled (e.g. inside `bash` 4+ or `zsh`), a multi-line paste does not auto-execute until the user hits Enter.
- Plain `Ctrl+C` still sends SIGINT to a running process inside the terminal.

---

## Audit-tail OPEN items without a separate OI entry

Low-impact items from the 2026-04-24 audit reconciliation that are tracked only in `MICROKERNEL-AUDIT.md` / `UI-AUDIT.md` rather than here. Adding an OI entry is warranted if impact justifies the tracking cost:

- **MK F-1.1.1** — `Kernel::start` / `shutdown` consolidation (doc-only today).
- **MK F-1.3.1** — narrow `nexus-plugins` → `nexus-kernel` dep surface.
- **MK F-2.2.1** — split `PluginContext` sync / async traits.
- **MK F-3.3.1** — honour `plugin_search_paths` from kernel config in bootstrap.
- **MK F-4.2.2** — deterministic reverse-registration shutdown order.
- **MK F-4.3.1** — `reload_plugin` retry / backoff.
- **MK F-4.4.1** — `PluginReloading` state exposed to dispatch.
- **MK F-5.1.2** — `TrustLevel::Invoker` with reduced caps.
- **MK F-6.2.1** — `settings.read` capability gate on `get_settings`.
- **MK F-9.4.1** — capability alias map for renames.
- **MK F-10.3.1** — metrics / OpenTelemetry integration.
- **MK SI-tauri-xss** — re-audit plugin-supplied string sanitisation in the contribution bridge.
- **MK SI-hotreload** — cross-platform `notify-debouncer-mini` reliability pass.
- **UI F-1.1.1** — editor as a content-type contribution.
- **UI F-3.2.1** — activation events default for script plugins.
- **UI F-3.3.1** — explicit `runtime` field in manifest schema.
- **UI F-4.3.1** — menu-item ordering groups.
- **UI F-5.2.1** — declarative plugin-panel primitives (fixed vocabulary).
- **UI F-6.3.1** — multi-root workspace decision.
- **UI F-8.3.1** — per-script-plugin memory accounting.
- **UI F-10.3.1** — `performance.measure` around plugin lifecycle.
- **UI SI-4** — tree-data-provider cache-on-forge-change.
- **UI SI-5** — CommandPalette modal-overlap visual check.
- **UI SI-6** — PluginManager mutex contention load test.

---

## Relation to BACKLOG.md

These items are cross-listed in [PRDs/BACKLOG.md](PRDs/BACKLOG.md) under "Post-migration carryover gaps (2026-04-24)" with pointers back here. This file is the authoritative description; BACKLOG.md is the index.
