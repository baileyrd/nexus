# Implementation Plan

Acts on the findings in [`SUMMARY.md`](SUMMARY.md) and [`DEPENDENCIES.md`](DEPENDENCIES.md). Each task lists effort (S/M/L/XL), risk, prerequisites, and acceptance criteria. Decision points that require human input are flagged **DECISION** and should not be started until resolved.

## Sequencing rationale

```
Phase 0 — Doc fixes              ┐ Cheap, surface immediate value, zero code risk.
Phase 1 — Missing dependsOn      ┘ One-liners; make hidden coupling visible.

Phase 2 — Dead-code deletion       Safe to do AFTER Phase 1 because the deps you're
                                   about to delete are now declared, so CI / dep
                                   checks will refuse cuts that break consumers.

Phase 3 — Schema extension         Add real dependsOn fields to manifests. Once
                                   landed, Phase 4 refactors get a static-analysis
                                   safety net.

Phase 4 — Refactors                Behavior-preserving cleanup with the safety net.

Phase 5 — Strategic decisions      Ship/cut calls. Best made after Phases 0-4
                                   give you accurate evidence.
```

Each phase is a separate PR (or small PR series) — no single mega-PR.

## Phase 0 — Documentation fixes

Pure-prose corrections to claims contradicted by code. No behavior change. Single PR.

### 0.1 Fix bootstrap order in `docs/0.1.2/plugins/core.md`
- **Effort:** S
- **Risk:** none
- **Change:** Replace the numbered list at lines 32–58 with the actual `register_all` order: `security → storage → database → editor → theme → ai-runtime → ai → skills → templates → formats → workflow → linkpreview → notifications → audio → comments → agent → mcp → lsp → dap → acp → git → terminal → collab`. Cross-check by reading `crates/nexus-bootstrap/src/plugins/mod.rs::register_all`.
- **Acceptance:** numbered list matches code; PR includes a `grep`-based test or comment pointing readers to `register_all` as the source of truth.

### 0.2 Remove `plugin.toml`-as-string claim
- **Effort:** S
- **Risk:** none
- **Change:** In `docs/0.1.2/plugins/core.md` "Authoring a new core plugin" step 3, replace the claim that "every core plugin embeds its `plugin.toml` as a string constant" with: "manifests are built inline by `core_manifest_with_ipc(...)` in `crates/nexus-bootstrap/src/plugins/mod.rs` from each crate's `IPC_HANDLERS: &[(&str, u32)]` slice."
- **Acceptance:** prose matches reality.

### 0.3 Update `crates/nexus-terminal/src/lib.rs:18`
- **Effort:** S
- **Risk:** none
- **Change:** The doc comment says terminal "is **not** a core plugin yet — mirroring the positioning of `nexus-git` (PRD-11)". Both are now registered core plugins. Replace with a sentence describing the current state.
- **Acceptance:** grep for the stale claim returns no hits.

### 0.4 Update `crates/nexus-mcp/src/lib.rs` doc
- **Effort:** S
- **Risk:** none
- **Change:** The doc comment "no IPC surface, no core plugin wrapper" is stale — `McpHostPlugin` exposes 12 handlers. Replace with a sentence pointing to `core_plugin.rs::McpHostPlugin` and the binary frontend split.
- **Acceptance:** comment reflects the McpHostPlugin + server-binary structure.

### 0.5 Document `[digests]` and `[webhooks]` in forge-config
- **Effort:** S
- **Risk:** none
- **Change:** Add table rows in `docs/0.1.2/settings/forge-config.md` for the `[digests]` and `[webhooks]` blocks loaded by `nexus_bootstrap::load_digest_config` and `load_webhook_config`. Cross-link from `docs/0.1.2/settings/hardcoded-rust.md` if any related rows live there.
- **Acceptance:** every block actually loaded from forge `config.toml` has a row.

**Phase 0 deliverable:** one PR, ~80 lines net.

## Phase 1 — Declare hidden shell couplings

Each task is a one-line addition to a plugin's `registerExtension({dependsOn: [...]})` manifest. The current dep IS already exercised in code; declaring it makes the graph machine-readable and CI-checkable.

### 1.1 Per-plugin additions

| Plugin | Add to `dependsOn` | Source of evidence |
|--------|-------------------|---------------------|
| `nexus.editor` | `nexus.comments`, `nexus.workspace`, `nexus.files` | DEPENDENCIES.md §7 |
| `nexus.canvas` | `nexus.editor` | imports `../editor/blockRefDrag`, `../editor/markdownRender` |
| `nexus.diagnostics` | `nexus.editor`, `nexus.workspace` | imports `../editor/kernelClient`, `../workspace/workspaceStore` |
| `nexus.files` | `nexus.editor` | imports `../editor/editorStore` (post-status fix; see 2.2) |
| `nexus.outgoingLinks` | `nexus.editor`, `nexus.files` | imports `../editor/editorStore`, `../files/kernelClient` |
| `nexus.outline` | `nexus.editor` | imports editorRuntime + editorStore + types |
| `nexus.tags` | `nexus.editor`, `nexus.files` | imports `../editor/editorStore`, `../files/kernelClient` |
| `nexus.observability` | `nexus.activity` (activityTimeline) | type import |
| `nexus.templates` | `nexus.files` | dispatches `nexus.files.openByPath` command |
| `nexus.gitStatus` | `nexus.statusBar` | registers `statusBarLeft` view |
| `nexus.crdtConflict` | `nexus.collab` | dead weight without it |
| `core.settings` | `nexus.pluginsMgmt` | imports `PluginsMgmtView` |
| `core.capabilityPrompt` | `nexus.pluginsMgmt` | imports `capabilityInfo` |
| `core.zoom` | `core.configurationService` | calls `api.configuration` |
| `core.notificationService` | `core.configurationService` | reaches into `configStore` singleton |

- **Effort per item:** S
- **Risk:** low (manifests are advisory currently; no enforcement breaks)
- **Acceptance:** each plugin's `dependsOn` reflects what its code actually imports/invokes. Verified by a grep that finds zero `from '../<other-plugin>/...'` imports not represented in `dependsOn`.

### 1.2 Resolve `nexus.themePicker` catalog drift
- **Change:** Pick one source of truth. Either add `core.theme-service` to `shell/src/plugins/nexus/themePicker/index.ts:22` manifest, or remove it from `shell/src/plugins/catalog.ts:307`. Recommend keeping it in the plugin's own manifest (the catalog should reflect the manifest, not duplicate it).
- **Effort:** S
- **Risk:** low
- **Acceptance:** `catalog.ts` entry and plugin manifest agree.

### 1.3 Document `nexus.statusBar`'s deliberate omission of `nexus.backlinks`
- **Change:** The omission is intentional (see `nexus/statusBar/index.tsx:16-18`). Add a `softDependsOn: ['nexus.backlinks']` field if/when that field exists (Phase 3); for now, expand the existing comment to be the authoritative justification.
- **Effort:** S
- **Risk:** none
- **Acceptance:** comment makes the intent unambiguous.

**Phase 1 deliverable:** one PR with ~17 one-line manifest edits + one comment expansion. Net ~30 lines.

## Phase 2 — Dead-code deletion

Mechanical removal of unreachable plugin directories. Phase 1 already declared the legitimate consumers, so anything still imported from a deletion target is exposed.

### 2.1 Extract the two leaked exports from dead shell-core
- **Effort:** S
- **Risk:** low
- **Change:**
  - Move `Heading` type from `shell/src/plugins/core/editorArea/MarkdownDoc.tsx` to `shell/src/types/editor.ts` (or wherever editor types live). Update the import in `shell/src/stores/docStore.ts:6`.
  - Move `usePanelAreaStore` from `shell/src/plugins/core/panelArea/panelAreaStore.ts` to `shell/src/stores/panelAreaStore.ts`. Update any importers (notably the dead `core/terminal`).
- **Acceptance:** zero imports remain from `shell/src/plugins/core/{activityBar,commandPalette,editorArea,fileExplorer,panelArea,rightPanel,sidebar,statusBar,terminal,titleBar}/`.

### 2.2 Move `shell/src/plugins/nexus/status/` out of `plugins/`
- **Effort:** S
- **Risk:** low
- **Change:** It's not a plugin (no manifest, not in catalog). Move the directory to `shell/src/lib/status/` (or fold into `shell/src/plugins/nexus/files/status/` if consumers are predominantly `files`). Update importers: `nexus/files/*`, `core/editorArea/MarkdownDoc.tsx` (which is being deleted in 2.3 anyway).
- **Acceptance:** `shell/src/plugins/nexus/status/` is gone; grep for the old path returns no hits.

### 2.3 Delete the 10 dead shell-core stub directories
- **Effort:** S
- **Risk:** low (Phase 2.1 prereq removed all real importers)
- **Change:** `rm -rf` the 10 directories under `shell/src/plugins/core/` listed in SUMMARY.md. Verify they are still not present in `shell/src/plugins/catalog.ts`.
- **Acceptance:** directory listing of `shell/src/plugins/core/` shows only the 7 live plugins; build + tests pass.

### 2.4 Delete `GraphGlobalView` dead code
- **Effort:** S
- **Risk:** low
- **Change:** `nexus.graph` ships a full-forge view component that the manifest never registers. Either register it (if you want it) or remove the file. **DECISION** — leaving as register vs delete pending product input; recommend delete.
- **Acceptance:** either a new manifest entry exposes `GraphGlobalView` and a test verifies it renders, or the file is removed.

### 2.5 Decide fate of `nexus.linkSuggest`
- **Effort:** S
- **Risk:** low
- **Change:** **DECISION** — `nexus.linkSuggest` is a config-only shim; its behavior lives in `nexus.editor`'s CM6 module. Two options:
  - **Fold:** move its 2 settings into `nexus.editor`'s manifest and delete the plugin directory.
  - **Keep:** leave as-is — the plugin is the only place the settings live and removing it would break user configs already in forges.
- **Acceptance:** decision recorded; if "fold," a settings migration is included.

### 2.6 Decide fate of `nexus.sidebar`
- **Effort:** S–M
- **Risk:** low–medium
- **Change:** **DECISION** — the plugin is a no-op stub kept alive only so ~10 other plugins' `dependsOn: ['nexus.sidebar']` declarations resolve. Two options:
  - **Drop:** remove `nexus.sidebar` from every other plugin's `dependsOn` and delete the stub. Cheap mechanical edit + one delete.
  - **Repurpose:** give it a real responsibility (host the left rail's view-registry) and consolidate `core.sidebar` callers through it.
- **Acceptance:** decision recorded; chosen action applied.

**Phase 2 deliverable:** one PR per task (six small PRs) OR one PR combining 2.1–2.3 (mechanical) and separate PRs for the decisions 2.4–2.6.

## Phase 3 — Schema extensions for declared dependencies

The biggest architectural fix. Today no dependency is machine-readable (Rust has no field, shell can't express cross-tier). After this phase, both tiers can declare deps and CI can audit them.

### 3.1 Add `dependsOn` to Rust `PluginManifest`
- **Effort:** L
- **Risk:** medium
- **Prereq:** none
- **Change:**
  - Add `dependsOn: Vec<String>` to `PluginManifest` in `crates/nexus-plugins/src/manifest.rs` with `#[serde(default)]`.
  - Extend `core_manifest_with_ipc(...)` in `crates/nexus-bootstrap/src/plugins/mod.rs` to take a `&'static [&'static str]` deps slice and emit it into the manifest.
  - Add a `MANIFEST_DEPS: &[&str]` const next to each core plugin's `IPC_HANDLERS`.
  - Populate from the data in DEPENDENCIES.md §1 (Rust core table).
  - Have `register_all` validate that the hand-curated boot order is a topological sort of the declared dep graph. Fail boot with a clear error if not.
- **Acceptance:**
  - All 23 core plugins declare `dependsOn`.
  - `cargo test -p nexus-bootstrap` includes a topological-sort check.
  - `cap_matrix_complete --ignored` (existing CI guard) passes with the new field.
- **Out of scope here:** changing community-plugin TOML schema; that piggybacks naturally once the field exists.

### 3.2 Allow cross-tier deps in shell `dependsOn`
- **Effort:** M
- **Risk:** medium
- **Prereq:** 3.1
- **Change:** **DECISION** between two designs:
  - **Option A — new field:** add `kernelDependsOn: string[]` to the shell `PluginManifest` type in `packages/nexus-extension-api/src/manifest.ts`. Update `ExtensionHost` to surface "kernel plugin X not active" errors instead of leaving runtime `api.kernel.invoke` to fail.
  - **Option B — normalize:** keep one `dependsOn` array, accept both shell ids (`nexus.foo`) and kernel ids (`com.nexus.foo`). The host routes each id to the right registry.
- **Recommendation:** Option B — single namespace, less surface area. The id prefix (`com.nexus.*` vs `nexus.*` vs `core.*`) already disambiguates.
- **Change:**
  - Extend manifest schema validation to accept kernel ids.
  - At activation time, check kernel availability via `api.kernel.available(id)`; throw a structured `KernelPluginNotActive` error on miss.
  - Update every shell plugin's manifest with its kernel deps (see DEPENDENCIES.md §3, columns "kernel IPC targets").
- **Acceptance:**
  - Every shell plugin that calls `api.kernel.invoke('com.nexus.*', ...)` declares the kernel plugin in `dependsOn`.
  - A test forces a kernel plugin off and verifies dependent shell plugins refuse to activate with a clear error.

### 3.3 Static-analysis CI check
- **Effort:** M
- **Risk:** low
- **Prereq:** 3.1, 3.2
- **Change:** Add a script — `scripts/check_plugin_deps.sh` or `cargo test -p nexus-bootstrap deps_match_code` — that:
  - greps every shell plugin's `index.ts` for `api.kernel.invoke(<id>, ...)` and `from '../<plugin>/...'` and fails if any target isn't in `dependsOn`.
  - greps every Rust plugin's `src/` for `ipc_call(<id>, ...)` and fails likewise.
- **Acceptance:** CI is green on a baseline run; intentionally adding an undeclared call fails the check.

**Phase 3 deliverable:** one PR for 3.1, one for 3.2, one for 3.3 — each large.

## Phase 4 — Refactors

Behavior-preserving cleanups, each with their own risk profile. Order independent except where noted.

### 4.1 Replace module-scope singletons
- **Effort:** M
- **Risk:** medium
- **Targets:** `recallApi`, `pickerRuntime`, `searchRuntime`, `notificationsSettingsRuntime`, `themePicker.getPickerApi`. Each caches the PluginAPI handle in module-level state.
- **Change:** Pass the handle through PluginAPI prop drilling (or via React context where the consumer is JSX). Each is 20–80 lines.
- **Acceptance:** grep for `let _api` / `let pickerApi` / similar module-scope handle storage returns zero hits; behavior unchanged (manual smoke test of the affected overlays/views).

### 4.2 Merge `nexus.fileProperties` + `nexus.allProperties`
- **Effort:** M
- **Risk:** low
- **Change:** Both call `com.nexus.storage::read_frontmatter` and differ only in chrome. Consolidate into a single `nexus.properties` plugin with two views (sidebar + dialog). Delete the dropped plugin's manifest and entry in `catalog.ts`. Migrate any user keybindings via a one-time settings rewrite (or accept the break — both are Optional plugins).
- **Acceptance:** one plugin replaces two; behavior preserved.

### 4.3 Consolidate links plugins
- **Effort:** L
- **Risk:** medium
- **Change:** `outgoingLinks`, `tags`, `backlinks`, and the per-file `graph` view all read `com.nexus.storage::backlinks/outgoing_links` for the active file. Merge into a single `nexus.links` panel with tabs for "Backlinks", "Outgoing", "Tags", "Graph". Keep `nexus.graph`'s global view if 2.4 chose to ship it.
- **Acceptance:** four plugins collapse into one; existing keybindings forward to the new panel.

### 4.4 Resolve duplicate command palettes
- **Effort:** S
- **Risk:** low (most of the work was done in Phase 2 — `core.commandPalette` is dead)
- **Change:** Confirm only `nexus.commandPalette` registers `Ctrl/Cmd+Shift+P` after Phase 2.3. Audit catalog for any leftover reference to `core.command-palette`.
- **Acceptance:** one palette, one keybinding.

### 4.5 Resolve `Ctrl/Cmd+Shift+F` collision
- **Effort:** S
- **Risk:** low
- **Change:** `nexus.search` and `nexus.searchPanel` both register the same chord. **DECISION** which owns it. Recommendation: `nexus.searchPanel` (the panel is the richer surface; `nexus.search` is the launcher overlay and can move to `Ctrl/Cmd+Shift+Space` or remain command-only).
- **Acceptance:** one binding registered; the other plugin's chord is either removed or remapped.

### 4.6 Fold `nexus.multibufferSync` into `nexus.editor`
- **Effort:** M
- **Risk:** low
- **Change:** Multibuffer sync has no consumers outside `nexus.editor`. Move `multibufferRegistry.ts` and surrounding wire-up into `nexus.editor` under `editor/multibuffer/`. Delete the plugin directory.
- **Acceptance:** one plugin; tests pass.

### 4.7 Rename `nexus.notion` → `nexus.notionImport`
- **Effort:** S
- **Risk:** low (no users in 0.1.2)
- **Change:** The current name implies a block-editor port; the plugin only wraps `com.nexus.formats::import_notion/export_notion`. Rename in `index.ts`, `package.json`, `catalog.ts`. Update display strings.
- **Acceptance:** name reflects what it does.

### 4.8 Storage compile-time deps — withdrawn after code-level review

- **Status:** **No action.** Initial framing in DEPENDENCIES.md called the `nexus-storage → {nexus-database, nexus-formats}` dependency a "compile-time leak past the IPC seam." Code-level review during execution found this is intentional layering:
  - `nexus-formats/src/lib.rs:5` explicitly declares itself a "pure-Rust parsers and serializers" library with "No runtime services; no SQLite."
  - `nexus-database/src/lib.rs:3-8` explicitly declares itself "**pure-logic** — it does not touch `rusqlite`. The SQL-backed query engine, schema migrations, and relation/rollup resolution that previously lived here moved into `nexus-storage` so that `nexus-storage` is the sole owner of the forge's SQLite database."
  - `crates/nexus-bootstrap/tests/dep_invariants.rs::FORBIDDEN` enforces the layering from the other side — `nexus-database` is forbidden from linking `rusqlite`.
- The IPC seam separates **plugins** for cross-trust dispatch, not **crates** for library reuse. `nexus-storage` consuming pure-logic types (`DatabaseError`, `PropertyConfig`, `FormulaValue`) and parsers (canvas, config, `sha256_hex`) from its pure-logic dep crates is correct architecture, not a leak.
- DEPENDENCIES.md §6 and §8 and `_extract-rust-deps.md` updated to reflect the corrected analysis.
- **No code change.** Nothing to do.

### 4.9 Document or remove `agent ↔ skills` cycle
- **Effort:** S
- **Risk:** low
- **Change:** **DECISION** — if the cycle is intentional, add a docstring at the top of both `core_plugin.rs` files describing the contract. If not, change one side to use an event-driven coupling instead of a direct call.
- **Acceptance:** either both crates' module docs describe the cycle, or the cycle is broken.

**Phase 4 deliverable:** ~9 PRs, one per task.

## Phase 5 — Strategic decisions

Each requires product/architecture sign-off, not just engineering. Captured here so they don't get forgotten.

### 5.1 Ship or cut `com.nexus.acp` for 0.1.2
- **Context:** No in-tree shell consumer. Exercised only by `nexus acp serve` CLI and tests. The plugin id is referenced from `shell/src/types/pluginIds.ts` but no shell plugin invokes it.
- **DECISION criteria:** Is there a 0.1.2 roadmap item that needs ACP? If yes, keep; if no, mark the plugin behind a `--experimental` bootstrap flag or remove until a consumer lands.

### 5.2 `com.nexus.audio` local backend
- **Context:** Default backend is `local`, but the shipped build stubs it. First dispatch returns `BackendNotEnabled` unless built with the `local-audio` feature.
- **DECISION criteria:** Either fix the default (switch shipping default to `provider` since that's what works) or commit to enabling `local-audio` in release builds.

### 5.3 `com.nexus.dap`, `com.nexus.lsp`
- **Context:** Both are leaf plugins (no IPC out, publish only `started` events). LSP has one shell consumer (`nexus.diagnostics`); DAP has none in-tree (one shell `debugger` plugin exists but its end-to-end story isn't verified in this assessment).
- **DECISION criteria:** Confirm or remove `nexus.debugger`'s end-to-end story. If DAP has no consumer, treat like 5.1.

### 5.4 `agent ↔ skills` cycle intent
- See 4.9. Tagged here too because the answer is a design call, not an implementation detail.

### 5.5 Whether to keep `nexus.sidebar` as a stub
- See 2.6.

### 5.6 `nexus.linkSuggest` shim
- See 2.5.

## Effort summary

| Phase | Tasks | Total effort | Risk |
|-------|-------|--------------|------|
| 0 — Doc fixes | 5 | S × 5 | none |
| 1 — Declare hidden couplings | 17 + 2 | S × 19 | low |
| 2 — Dead-code deletion | 6 | S × 4 + decisions | low–medium |
| 3 — Schema extensions | 3 | L + M + M | medium |
| 4 — Refactors | 9 | mixed S/M/L | low–medium |
| 5 — Strategic decisions | 6 | decisions only | depends |

Rough calendar if a single engineer takes it on: Phase 0+1 in 1 day; Phase 2 in 2–3 days incl. decisions; Phase 3 in 1–2 weeks; Phase 4 in 1–2 weeks; Phase 5 is human time, not engineering time.

## What this plan does NOT do

- It does not change product behavior beyond what is described per task.
- It does not introduce new features.
- It does not touch capabilities, the security plugin, or any IPC contract that flows through `ts-rs` boundary types — IPC drift checks (`scripts/check_ipc_drift.sh`) should still pass cleanly at the end of every phase.
- It does not delete `com.nexus.acp`, `com.nexus.dap`, `com.nexus.audio` outright — those decisions are flagged in Phase 5.

## Cross-cutting acceptance

At the end of all phases:

1. `cargo test --workspace` passes.
2. `pnpm --filter nexus-shell test` and `tsc --noEmit` pass.
3. `scripts/check_ipc_drift.sh` passes.
4. New `scripts/check_plugin_deps.sh` (added in 3.3) passes.
5. `shell/src/plugins/core/` contains 7 directories, not 17.
6. Every shell plugin's `dependsOn` covers every cross-plugin TS import and `api.kernel.invoke` target in its own code.
7. Every Rust core plugin declares `dependsOn` in its manifest.
8. The DEPENDENCIES.md "hidden couplings" §7 table is empty (or only contains entries for which a deliberate `softDependsOn` is recorded).
