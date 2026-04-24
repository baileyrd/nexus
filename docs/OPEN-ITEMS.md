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
**Surfaced by:** `docs/superpowers/specs/2026-04-17-split-size-persistence-design.md`
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

## Relation to BACKLOG.md

These items are cross-listed in [PRDs/BACKLOG.md](PRDs/BACKLOG.md) under "Post-migration carryover gaps (2026-04-24)" with pointers back here. This file is the authoritative description; BACKLOG.md is the index.
