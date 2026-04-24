# Open Items — Post-Migration Carryover Gaps

> Capabilities described in legacy `app/` documentation that were not carried over to `shell/` during the Phase 4 WI-37 retirement (2026-04-24). Surfaced by the capability-presence sweep on 2026-04-24.
>
> Listed here rather than in [PRDs/BACKLOG.md](PRDs/BACKLOG.md) because these are regressions from prior-shipped behavior, not new features. Linked from BACKLOG.md under "Post-migration carryover gaps."

---

## OI-01 — Settings modal / `registerSettingsTab` API

**Severity:** Should-fix (user-visible parity gap)
**Surfaced by:** `docs/references/obsidian-settings-modal.md`
**Status:** Not started

### Gap
The Obsidian-style tabbed settings modal with a plugin-extensible `registerSettingsTab` API is entirely absent in `shell/`.

- `shell/src/plugins/core/settings/SettingsPanelView.tsx` exists but is a single-panel view, not a tabbed modal with a plugin extension point.
- Zero hits for `SettingsModal`, `registerSettingsTab`, or `workspace.settings` across `shell/src/`.
- Legacy `app/src/contributions/builtins.ts` registered settings tabs declaratively; no equivalent contribution registry entry exists.

### Scope
- Design an extension-point contribution (`settings.tabs` or similar) aligned with the existing contribution-registry pattern (see `shell/src/registry/`).
- Build a tabbed modal view. Register a default set of tabs for bundled plugins (appearance, keybindings, file-system, AI providers, etc.).
- Expose `api.settings.registerTab(...)` on the plugin API surface.
- Persist last-open tab in shell state.

### Acceptance
- A plugin can declare `contributions.settings` in its manifest and have a tab appear in the settings modal.
- Default bundled tabs cover appearance, keybindings, and any plugin-level settings that existed in the legacy shell.
- `Ctrl/Cmd+,` opens the modal to the last-viewed tab.

---

## OI-02 — Split-size persistence

**Severity:** Should-fix (UX regression)
**Surfaced by:** `docs/superpowers/specs/2026-04-17-split-size-persistence-design.md`
**Status:** Not started — the design spec was written before `app/` retirement and never implemented in `shell/`.

### Gap
Pane-splitter positions are not persisted across reloads. Users drag a sidebar wider, reload, and the split reverts.

- `shell/src-tauri/src/persistence.rs` `get_shell_state`/`save_shell_state` has no `split_sizes` field.
- `shell/src/stores/workspaceStore` / `shell/src/workspace/` — zero hits for `splitSizes` or `split_sizes`.
- Frontend splitter components (sidebar, right panel, bottom panel, editor splits) do not report drag-end to a persistence layer.

### Scope
- Extend the shell-state schema with a `split_sizes: Record<string, number[]>` keyed by split ID.
- Emit a debounced save from splitter components on drag-end.
- Hydrate on boot, fall back to defaults if the key is missing.
- See the original design doc (now bannered as historical) for the proposed schema; reconfirm before implementing since naming conventions may have drifted.

### Acceptance
- Drag any pane splitter, reload — position restored.
- Clean uninstall (`~/.nexus-shell/` deleted) falls back to default layout without error.

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
**Status:** Not started

### Gap
Two TODOs in the shell type layer note that types used by plugin manifests should move into the kernel contract so native and community plugins see the same shape:

- `shell/src/types/plugin.ts:74` — slot-registration types should be "promoted to kernel contract" rather than duplicated between `SlotRegistry` and `plugin.ts`.
- `shell/src/plugins/nexus/agent/agentStore.ts:104` — `RESEARCHER_ID` and related agent-identity constants "TODO(WI-07 follow-up): replace with a kernel" contract.

These aren't bugs — they're incomplete migrations from earlier WIs. Leaving them means two sources of truth for plugin-visible types, which will bite during the next kernel-API evolution.

### Scope
- Inventory types that are declared shell-side but conceptually belong to the kernel contract (start with the two TODOs, then widen the sweep).
- Move them into `packages/nexus-extension-api/src/` or equivalent so they ship to both host and sandbox.
- Update the TODO sites to import from the canonical location.

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
**Status:** Not started

### Gap
`shell/package.json` pins `eslint ^8.57` (the deprecated 8.x line) and `@typescript-eslint/* ^7`. ESLint 8 is end-of-life; upstream no longer publishes security fixes. typescript-eslint 7 pairs with ESLint 8 and will not track new rule additions from ESLint 9.

Also, the audit noted the user's global `~/.eslintrc.json` breaks the shell's `pnpm lint` by referencing plugins that aren't resolvable in the shell workspace — the personal config shadows the workspace config. A project-level `.eslintrc` that explicitly forbids global fallback would fix the environment issue.

### Scope
- Joint upgrade: ESLint 9 + `@typescript-eslint/*` 8. The flat-config migration is non-trivial but well-documented.
- Pin a project-level `.eslintrc` (or equivalent flat-config file) so the global personal config never shadows it.
- Also evaluate: `xterm` / `xterm-addon-fit` use the deprecated pre-scoped npm names; `@xterm/xterm` is the current package name. Worth migrating while the package.json is open.

### Acceptance
- `pnpm lint` runs to completion without environment errors and reports whatever the new-ruleset floor is.
- ESLint and typescript-eslint are off the deprecated major lines.
- xterm packages migrated to `@xterm/*` scoped names.

---

## Relation to BACKLOG.md

These items are cross-listed in [PRDs/BACKLOG.md](PRDs/BACKLOG.md) under "Post-migration carryover gaps (2026-04-24)" with pointers back here. This file is the authoritative description; BACKLOG.md is the index.
