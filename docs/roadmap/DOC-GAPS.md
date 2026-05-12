# Doc Gaps — Traceability Audit Findings

> Gaps surfaced by the 2026-05-12 doc-traceability audit
> ([../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md)).
> Items are doc-bugs (spec disagrees with code) or product-gaps (spec'd
> in a PRD/ADR but unimplemented).
>
> Filed here rather than in [../PRDs/BACKLOG.md](../PRDs/BACKLOG.md)
> because most entries are documentation drift, not new features.
> Genuine unimplemented features still get cross-listed from BACKLOG.md
> under "Doc-audit-surfaced product gaps."

**Severity scale:**
- **Critical** — following the doc gives wrong code that won't run
- **Should-fix** — confuses readers but the misleading bit is recoverable
- **Cosmetic** — stale count or label; no semantic impact

**Kind tags:**
- `doc-bug` — spec wrong, fix the doc
- `product-gap` — spec right, code missing
- `filing` — doc in the wrong directory
- `status-drift` — IMPLEMENTATION_STATUS or other tracker contradicts code

---

## DG-01 — `docs/developer/` teaches a fictional plugin API

**Severity:** Critical (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Developer hub; agent finding 1
**Status:** Open

The plugin-author hub at `docs/developer/plugins/*.md` and
`docs/developer/editor/*.md` teaches a TypeScript API that does not
exist. Documented surface: `Plugin` / `PluginContext` / `activate()` /
`deactivate()` / `ctx.commands.register()` / `ctx.events.subscribe()` /
`ctx.config.get()` / `ctx.kv.get()` / `ctx.statusBar.add()` /
`ctx.views.register()` / `ctx.ui.modal()` / `mockContext`. Actual
surface at `packages/nexus-extension-api/src/index.ts`: `ScriptPlugin`
with `onInit` / `onStart` / `onStop` / `dispatch`, and
`NexusPluginContext` with `settings.get()`, `events.emit()`,
`ipc.call()`, `editor.register*()`, `ui.notify(level, message)`,
`ui.registerPanelView()`, etc.

A plugin author following the documented examples cannot ship working
code. This is the single biggest doc bug in the repo.

**Definition of done:**
- Every `developer/plugins/*.md` and `developer/editor/*.md` example
  compiles against the real `@nexus/extension-api` surface.
- `developer/getting-started.md` example matches the `script` template
  scaffold output verbatim.
- Add a "where the API lives" pointer at the top of
  `developer/README.md` linking `packages/nexus-extension-api/src/index.ts`
  as authoritative source of truth.

**Affected docs:**
- `developer/getting-started.md`
- `developer/reference.md`
- `developer/plugins/{overview,manifest,lifecycle,capabilities,ipc,events,settings,testing,publishing}.md` (9 files)
- `developer/editor/{overview,slash-commands,mdx-components}.md`

---

## DG-02 — `docs/shell/` reference is post-leaf-migration stale

**Severity:** Critical (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Shell reference; agent finding 2
**Status:** Open

The Phase 7 leaf migration (2026-04) moved the floor; shell reference
docs never caught up. Concrete drift:

- **Slot count.** `slot-system.md` and `architecture.md` claim 8 slot
  IDs; `writing-a-plugin.md` lists 11. Real slot store ships 6:
  `overlay` / `titleBar` / `activityBar` / `statusBarLeft` /
  `statusBarRight` / `paneMode`. The three removed by leaf migration
  (`sidebar`, `editorArea`, `panelArea`) and the four never-shipped
  (`sidebarContent`, `rightPanelContent`, `bottomPanel`, …) are still
  documented as live.
- **Plugin count + namespace.** Docs assume `core.*` namespace with 38
  plugins (19 default-on + 17 default-off). Real catalog at
  `shell/src/plugins/catalog.ts` has **60** plugins (31 on + 29 off)
  in the `nexus.*` namespace.
- **Workspace path.** `workspace-layout.md` says `.nexus/workspace.json`;
  real path is `<forge>/.forge/workspace.json` (`persistence.ts:33`).
  Schema discriminator wrong (`type` vs `kind`).
- **PluginAPI coverage.** `plugin-api.md` covers ~10 of the ~17
  sub-surfaces on `PluginAPI` in `shell/src/types/plugin.ts`.
  Missing entirely: `workspace`, `viewRegistry`, `keybindings`,
  `kernel`, `platform`, `activityBar`, `input`, `settings`, `uri`,
  `editor`.
- **Registry-system.** Lists `views` / `menus` / `slots` registries on
  `PluginRegistry` — none are fields on the real type. Slot store is
  separate Zustand singleton; no menu registry exists; `ViewRegistry`
  is a workspace concept.
- **Fabricated shapes.** `definePlugin({ views: [{ serialize, deserialize }] })`
  is fictional. Real registration is `viewRegistry.register(type, creator)`
  with a `View` interface (`getState` / `setState` / `onOpen` /
  `onClose`).
- **Activation events.** `onFileOpen:` is documented but not honoured.
  Real triggers: `onView:` / `onCommand:` / `onUri:` / `onLanguage:`.

**Definition of done:**
- Each shell doc verified claim-by-claim against the current source.
- A regenerated "Plugin API surface" table sourced from the actual
  `PluginAPI` type.
- A regenerated slot-ID listing sourced from `SlotRegistry.ts`.
- Workspace JSON schema regenerated from the actual persistence
  format.

**Affected docs:**
- `docs/shell/{architecture,plugin-api,plugin-system,extension-host,registry-system,slot-system,workspace-layout,writing-a-plugin,core-plugins}.md`

---

## DG-03 — `users/cli.md` misses ~12 subcommand groups

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Users; agent finding 4
**Status:** Open

CLI surface in `docs/users/cli.md` is badly out of date.
- **Missing entire groups:** `template`, `crdt`, `skill`, `import`,
  `export`, `completions`, `proc`, `workflow`, `bases`, `forge`, plus
  16+ `git` subcommands.
- **Fictional listings:** `tags locate`, `bases validate`, `agent list`,
  `agent history`, `proc kill`, `term saved`, `config get/set/list`
  are documented but don't exist.
- **Renamed:** `nexus mcp` should be `nexus mcp serve`.
- **Fictional flag:** `--watch` on `nexus plugin install` does not
  exist.

**Definition of done:** Regenerate the CLI table from
`crates/nexus-cli/src/main.rs` + the `commands/` subdirectory. Add a
note that the source-of-truth listing is `nexus --help` and the
generated clap help text.

---

## DG-04 — Inline-AI keybinding documented wrong in 5 help docs

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 1
**Status:** Resolved 2026-05-12

`Ctrl+Shift+Space` appears as the inline-AI keybinding in 5 help
docs. The real binding is `Ctrl+I` / `Cmd+I` per
`shell/src/plugins/nexus/ai/index.ts:260`. The string `Ctrl+Shift+Space`
appears in *no* keybinding registration anywhere in the codebase.

**Definition of done:** Replace `Ctrl+Shift+Space` with `Ctrl+I` (or
`Cmd+I` for macOS examples) across all 5 affected help docs.

### Outcome
Bulk `sed` across the 5 affected files: `help/customize/keybindings.md`,
`help/getting-started/quick-tour.md`, `help/ai/overview.md`,
`help/editing/editor.md`, `help/ai/inline-completion.md`. Both
`Ctrl+Shift+Space` → `Ctrl+I` and `Cmd+Shift+Space` → `Cmd+I`. No
remaining occurrences of the wrong binding in any live doc.

---

## DG-05 — `Ctrl+Shift+T` keybinding conflict

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 5
**Status:** Open

`docs/help/customize/keybindings.md` documents `Ctrl+Shift+T` as "new
terminal session". `shell/src/plugins/nexus/themePicker/index.ts:37`
registers the same binding for the theme picker. Either the docs are
wrong, or the binding needs to move.

**Definition of done:** Decide which feature owns `Ctrl+Shift+T` and
update either the registration or the doc.

---

## DG-06 — `editing/comments.md` describes wrong storage model

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 2
**Status:** Open

Doc claims comments live in YAML frontmatter. Real storage is a JSON
sidecar per `crates/nexus-comments/src/store.rs`.

**Definition of done:** Rewrite the storage-model section against the
actual sidecar layout.

---

## DG-07 — `editing/embeds-and-mdx.md` describes aspirational components

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 3
**Status:** Open

Doc describes `<Card />`, `<Alert />`, `<Badge />` and
`editor.registerMdxComponent` — none exist under `shell/src`. The
real MDX component contract requires `render` to return a `PanelNode`
tree (declarative, host-walked); the doc shows React JSX.

**Definition of done:** Either build the components (promote to a BL
entry under PRD-08) or remove the section and add a "planned" pointer.

---

## DG-08 — `customize/themes.md` references nonexistent scaffold template

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 4
**Status:** Open

Doc recommends `nexus plugin scaffold --template theme`. Real
templates are `script` (default), `core`, `community` per
`crates/nexus-cli/src/commands/plugin.rs:216-219`. There is no theme
template.

**Definition of done:** Either add a theme template (BL entry) or
rewrite the doc to use the CSS-snippet path instead.

---

## DG-09 — Broken doc link in `customize/themes.md`

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 8
**Status:** Resolved 2026-05-12

`docs/help/customize/themes.md:54` links to
`docs/shell/theme-variables.md`. Real path is
`docs/developer/themes/css-variables.md`.

**Definition of done:** One-line link fix.

### Outcome
`help/customize/themes.md:54-55` now uses a real link to
`../../developer/themes/css-variables.md`.

---

## DG-10 — `developer/core-plugins/authoring.md` shows fictional bootstrap API

**Severity:** Critical (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Developer hub; agent finding 7
**Status:** Open

Doc shows `build_runtime(forge_root)` + `kernel.register_core_plugin(...)`.
Actual API is `build_cli_runtime(PathBuf)` / `build_tui_runtime(PathBuf)`
with private internal registration. Core-plugin authors following the
doc cannot wire their plugin into the bootstrap.

**Definition of done:** Match the example to the real bootstrap entry
points and the actual `CorePlugin` registration mechanism.

---

## DG-11 — `notion-block-ux-plan.md` should archive (shipped)

**Severity:** Should-fix (filing)
**Kind:** `filing`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Roadmap
**Status:** Resolved 2026-05-12

All 6 phases of the notion-block UX plan shipped 2026-04-22. Doc still
lives in `roadmap/` as if in-flight. Also has a duplicated/contradictory
Phase 4 entry in its "Phasing recap" block.

**Definition of done:** `git mv docs/roadmap/notion-block-ux-plan.md
docs/archive/`, add archive header citing the shipping date and the
landing commits.

**Resolution.** `git mv docs/roadmap/notion-block-ux-plan.md
docs/archive/notion-block-ux-plan.md`, replaced the stale "SHIPPED —
should archive" callout with a standard archive header (BL-048..BL-051
follow-up mapping included). Updated path references in five shell
CodeMirror plugin files, `crates/nexus-editor/src/transaction.rs`,
`docs/PRDs/BACKLOG.md`'s Future-directions mapping, and removed the
row from `docs/roadmap/README.md`. The body's internal "still open"
notes are preserved unedited — the archive header flags they're stale
by definition.

---

## DG-12 — `OPEN-ITEMS.md` should archive (21/22 resolved)

**Severity:** Should-fix (filing)
**Kind:** `filing`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Roadmap
**Status:** Resolved 2026-05-12

Of the 22 OI-NN items, 21 are resolved (per the existing `Status:
Resolved` lines in the file itself). Only OI-05 (Rust dep duplication,
blocked upstream) is genuinely open. The file mis-presents itself as
a live tracker.

**Definition of done:** Move resolved OIs to `docs/archive/`; promote
OI-05 to a single BL entry in `PRDs/BACKLOG.md` or leave a one-page
`OPEN-ITEMS.md` containing just OI-05 + a pointer to the archived
audit trail.

**Resolution.** Chose option B (keep a one-page live tracker). The
full 22-entry audit trail was moved verbatim via `git mv` to
`docs/archive/OPEN-ITEMS-resolved-2026-04-26.md` and gained a standard
archive header; the live `docs/roadmap/OPEN-ITEMS.md` was rewritten as
a slim file with just OI-05 and a pointer to the archive. Cross-links
updated in `docs/roadmap/README.md` and `docs/PRDs/BACKLOG.md` (the
"Resolved" subsection and the ADR-0009 verification note both now
point at the archive).

---

## DG-13 — OI-13 outcome claims C4 update that never landed

**Severity:** Should-fix (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Architecture cross-file
**Status:** Open

OI-13's resolution outcome says it updated `docs/architecture/C4.md`
to drop the `PluginRegistry` component box. The current C4 diagram
still ships those relationships. Either C4.md needs the edit, or
OI-13's outcome wording is wrong.

**Definition of done:** Apply the documented C4 change, *or* correct
OI-13's outcome line.

---

## DG-14 — `C4.md` stale concrete counts

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Architecture
**Status:** Resolved 2026-05-12

- Claims **25 crates**; workspace has **28** (`nexus-lsp`, `nexus-crdt`,
  `nexus-fuzz` missing).
- Claims **23** `#[tauri::command]` handlers; actual **25** across 4
  files.
- MCP "15 `nexus_*` tools" verified correct.

**Definition of done:** Regenerate counts from `Cargo.toml workspace
members` and from the `invoke_handler!` block in
`shell/src-tauri/src/lib.rs`.

### Outcome
- `C4.md:77` "Core (Rust workspace — 25 crates)" → 28 crates (verified
  by `awk '/^members = \[/,/^\]/' Cargo.toml | grep -cE '"[a-z]'`).
- `C4.md:404` "23 `#[tauri::command]` handlers" → 25 (verified by
  `grep -cE '#\[tauri::command\]' shell/src-tauri/src/**/*.rs`).
- MCP "15 `nexus_*` tools" left unchanged — already correct.

---

## DG-15 — `ipc-schemas.md` claims wildly understate reality

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Architecture
**Status:** Resolved 2026-05-12

Header claims "~28 JSON schemas + ~30 TS types committed". Actual:
**131 JSON schemas, 166 TS files**. The "pilot" language is
six months out of date.

**Definition of done:** Replace pilot-era counts with `wc -l` of the
generated directories, or omit counts and just point at the generated
trees.

### Outcome
- `ipc-schemas.md:3` updated: "131 JSON schemas + 166 TS types
  committed" with a note that the generated dirs are authoritative.
  Pilot framing dropped.

---

## DG-16 — ADR 0002 capability table missing `ai.*` cluster

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug` (NB: ADR is immutable; remediation is a per-ADR
addendum, *not* an edit to the original ADR body)
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Open

`docs/adr/0002-hierarchical-capability-strings.md` enumerates the
capability inventory; the 8 `ai.*` capabilities added by ADR 0022 are
not in the table. ADR 0022 supersedes the inventory but the original
table reads as authoritative.

**Definition of done:** Add a `## Inventory note (2026-05-12)` section
to ADR 0022 with the full current capability list (22 capabilities),
and add a forward-pointer at the top of ADR 0002. Do not edit ADR
0002's body (immutable convention).

---

## DG-17 — Capability count stale across developer hub

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Developer hub; agent finding 2
**Status:** Resolved 2026-05-12

`developer/reference.md` and `developer/plugins/capabilities.md`
claim 14 capabilities. Real count is 22 (the 8-member `ai.*` cluster
is missing from the docs).

**Definition of done:** Regenerate the table from
`crates/nexus-plugin-api/src/capability.rs`.

### Outcome
- `developer/reference.md:27` updated: 22 variants (6 HIGH-risk), with
  pointer noting ADR 0022 added the 8 `ai.*` variants.
- `developer/plugins/capabilities.md:9-10` updated: 22 total, 6 HIGH
  risk, with link to ADR 0022.

---

## DG-18 — IMPLEMENTATION_STATUS marks PRD-16 🟠; actually 🟢

**Severity:** Should-fix (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Resolved 2026-05-12

`docs/PRDs/IMPLEMENTATION_STATUS.md` says PRD-16 (Workflow) has no
webhook / git_event / mcp_event triggers and no parallel / retry
scheduling. All four shipped per
`crates/nexus-workflow/src/{cron,core_plugin,executor,webhook}.rs`.

**Definition of done:** Bump PRD-16's status tier from 🟠 to 🟢; cite
the four landing commits.

### Outcome
- PRD-16 status tier bumped 🟠 → 🟢.
- Added "Shipped (webhook / git_event / mcp_event triggers)" entry
  citing `crates/nexus-workflow/src/webhook.rs` (BL-028g) and the
  `spawn_git_event_triggers` / `spawn_mcp_event_triggers` functions
  in `core_plugin.rs`.
- Added "Shipped (parallel steps + retry/backoff)" entry citing
  `executor.rs`'s `futures::future::join_all` + per-step retry config
  (`max_retries` / `retry_backoff` / `retry_initial_delay_ms` /
  `retry_max_delay_ms` / `retry_jitter`).
- Replaced "Gaps: No webhook…" line with "Gaps: None remaining
  against PRD-16."

---

## DG-19 — IMPLEMENTATION_STATUS PRD-13 entries stale

**Severity:** Cosmetic (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Resolved 2026-05-12

Two PRD-13 (Skills) claims in `IMPLEMENTATION_STATUS.md`:
- "No skill composition / dependency resolution" — BL-021 `compose`
  resolver shipped.
- "4 built-in skills" — 5 exist.

**Definition of done:** Update both lines.

### Outcome
- "Four canonical .skill.md files" → "Five canonical" — added
  `os-setup` to the list. Verified by `ls
  crates/nexus-skills/builtins/`.
- "No skill composition / dependency resolution" gap removed and
  replaced with a "Shipped (composition resolver, BL-021)" line
  citing `crates/nexus-skills/src/compose.rs` and handler id 8
  (`com.nexus.skills::invoke`).
- Remaining gap "UI SkillsPanel is read-only" kept as a future
  concern (separate from this DG).
- Removed the now-redundant top-of-file warning that flagged
  DG-18/DG-19.

---

## DG-20 — ADR 0014 deprecated `ribbon` still referenced in extension-api

**Severity:** Should-fix (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Open

ADR 0014 deprecated the `ribbon` slot/concept. `ribbon` still appears
in `packages/nexus-extension-api/src/sandbox/{context,runtime}.ts`
and `index.ts`. Either the deprecation needs follow-through (remove
the API surface) or the ADR needs a "left in place for compat" note.

**Definition of done:** Decide and document. If removing,
breaking-change pass through extension-api consumers.

---

## DG-21 — Stale ADR crate enumerations (0001, 0004)

**Severity:** Cosmetic (doc-bug; ADR-addendum required)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Open

ADRs 0001 / 0004 enumerate 5–6 crates. Workspace has 28. The *spirit*
of those ADRs (microkernel + crate-per-subsystem) still holds; only
the inventory listing is stale.

**Definition of done:** Add an "Inventory updated 2026-05" addendum
to ADR 0026 (or the most recent ADR) with the current 28-crate
listing and a forward-pointer from ADRs 0001 / 0004.

---

## DG-22 — ADR 0003 says `FileRenamed` lives in `nexus-kernel`; it doesn't

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Resolved 2026-05-12

Event type lives in `crates/nexus-storage/src/watcher.rs` (variant
`StorageEvent::FileRenamed`), not `nexus-kernel` as ADR 0003 states.
Decision still correct; just the filing claim is wrong. *Correction:*
the original audit said it lived in `nexus-plugin-api::event`; the
actual location, verified during resolution, is
`nexus-storage::watcher::StorageEvent` — emitted in the watcher
debounce loop at line 427 and dispatched to the kernel-bus topic
`com.nexus.storage.file_renamed` in `core_plugin.rs:1515`. The
kernel bus carries it as a topic-string payload; no kernel enum
variant exists.

**Definition of done:** ADR-addendum pattern (don't edit 0003 body)
noting the event-types relocation.

### Outcome
Appended `## Addendum 2026-05-12` block at the bottom of
`docs/adr/0003-storage-owns-file-watcher.md` citing the real
`watcher.rs:48` definition, `watcher.rs:427` emit site, and
`core_plugin.rs:1515` dispatch site. Original ADR body left
unchanged per the immutability convention.

---

## DG-23 — ADR 0008 promised fastembed-rs addendum never landed

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Resolved 2026-05-12

ADR 0008 declared a follow-up addendum once the `fastembed-rs`
adoption decision (ADR 0018) settled. Never written.

**Definition of done:** Write the addendum at the bottom of ADR 0018,
add a forward-pointer from ADR 0008.

### Outcome
- Appended `## Addendum 2026-05-12 — ADR 0008 tech-stack-defaults update`
  at the bottom of `docs/adr/0018-embedding-backend.md` with a defaults
  table (fastembed-rs `nomic-embed-text-v1.5` local default; Ollama /
  OpenAI remote alternatives) and a pointer to the
  `EmbeddingProvider` trait.
- Added a forward-pointer line under the ADR 0008 header (top of
  `docs/adr/0008-tech-stack-defaults.md`) directing readers to the
  ADR 0018 addendum as the operative tech-stack-defaults update for
  the embeddings row. Both ADR bodies left unchanged.

---

## DG-24 — ADR 0007 anti-spoofing lacks a guard test

**Severity:** Should-fix (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Open

ADR 0007 declares anti-spoofing properties of the event bus. No
dedicated test enforces them. The architectural invariants test
suite is the right home.

**Definition of done:** Add `event_bus_anti_spoofing.rs` (or similar)
under `crates/nexus-bootstrap/tests/`.

---

## DG-25 — ADR 0020 `popoutCompatible` allowlist unpoliced

**Severity:** Should-fix (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Open

ADR 0020 specifies a `popoutCompatible` allowlist for plugins that
can render in pop-out windows. Nothing verifies that new chrome-only
plugins set the flag correctly. A plugin that forgets it gets a
runtime surprise.

**Definition of done:** Add a contract test verifying every shipped
plugin's `popoutCompatible` value matches its actual capability.

---

## DG-26 — `developer/plugins/events.md` broken path

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Developer hub; agent finding 5
**Status:** Resolved 2026-05-12

References `packages/nexus-extension-api/src/events.ts` — does not
exist. Event types are co-located with the surface they belong to.

**Definition of done:** Replace with pointer to actual sources.

### Outcome
`developer/plugins/events.md:69-70` now points at
`packages/nexus-extension-api/src/generated/NexusEvent.ts` (the
ts-rs-generated event types, the actual authoritative shape).

---

## DG-27 — `developer/plugins/testing.md` broken path

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Developer hub; agent finding 5
**Status:** Open

References `packages/nexus-extension-api/src/testing/` — does not
exist. Either build the test harness (BL entry) or rewrite the doc
around the existing `node --test` flow that real plugins use.

**Definition of done:** Decide on the test-helper story; align the
doc.

---

## DG-28 — `developer/core-plugins/authoring.md` broken template paths

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Developer hub; agent finding 5
**Status:** Resolved 2026-05-12

References `docs/templates/community-plugin/README.md` and
`docs/templates/core-plugin/README.md`. Real templates live under
`docs/PRDs/templates/` (and the actual scaffolding source lives in
`crates/nexus-plugins/templates/`).

**Definition of done:** Update the paths in the doc.

### Outcome
- `developer/reference.md:74-75` template paths updated to
  `docs/PRDs/templates/{community,core}-plugin/` with live links.
- `developer/core-plugins/authoring.md:240` template path updated to
  match.

---

## DG-29 — `developer/themes/css-variables.md` broken style paths

**Severity:** Cosmetic (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Developer hub; agent finding 5
**Status:** Resolved 2026-05-12

References `shell/src/styles/tokens/` and `shell/src/styles/themes/` —
do not exist. Theme tokens live under `shell/src/shell/`.

**Definition of done:** Update the paths.

### Outcome
- `developer/themes/css-variables.md:9-19` now points at the real
  consolidated stylesheet `shell/src/shell/shell.css` with its
  `:root` / `[data-theme="…"]` / `[data-density="…"]` blocks (~547
  custom properties).
- "See also" footer link at line 218 updated to match.

---

## DG-30 — Help CLI subcommand listings drift

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 6
**Status:** Open

`docs/help/` files reference these subcommands that do not exist or
have different semantics:
- `nexus agent list`, `nexus agent history`
- `nexus content delete`, `nexus content links`, `nexus content update --rename`
- `nexus tags locate`
- `nexus ai ask --stdin`
- `nexus plugin reset` (only `reset-crash-count` exists)

**Definition of done:** Either implement (BL entries) or rewrite the
help docs to the actual surface.

---

## DG-31 — Plugin URL install + signature verification doc'd but not built

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 7
**Status:** Open (blocked on marketplace WI-44)

`docs/help/plugins/install-community.md` describes installing from a
URL plus signature verification. `commands/plugin.rs::install_dispatch`
only handles local paths. URL install + signing is part of WI-44
(marketplace, deferred to formal release).

**Definition of done:** Either add a "planned — see
REQUIRED-FOR-FORMAL-RELEASE.md WI-44" pointer to the doc, or
implement install-from-URL as a standalone item (BL entry).

---

# Product gaps — spec'd but not implemented

> These are *features* the audit found in PRDs/ADRs but missing from
> code. Cross-listed in [../PRDs/BACKLOG.md](../PRDs/BACKLOG.md) so
> they show up in the normal backlog flow.

## DG-32 — PRD-15 §4 ToolRegistry not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

PRD-15 specifies a `ToolRegistry` abstraction the agent system calls
into. Not implemented. (Agents currently use ad-hoc dispatch.)

**Definition of done:** Per PRD-15 §4 — typed registry, capability
checks, registration discoverable from `nexus tool list`. Promote to
BL when prioritized.

---

## DG-33 — PRD-15 §5 Memory not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

PRD-15 §5 specifies agent-scoped persistent memory. Not implemented;
runs are stateless.

**Definition of done:** Per PRD-15 §5. Related to AI-MEMORY-LAYER-PLAN
(roadmap exploratory).

---

## DG-34 — PRD-15 §7 interactive approval round-trip not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

PRD-15 §7 requires the agent loop to pause and request user approval
for high-risk tool calls. Today the loop runs through to completion;
nothing surfaces an approval prompt.

**Definition of done:** Per PRD-15 §7; UI work coordinated with
ADR 0024 (shell approval UI).

---

## DG-35 — PRD-15 §8 six built-in agent classes (3 of 6 shipped)

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

PRD-15 specifies 6 built-in agent classes. Three archetype prompts
shipped (`researcher`, `writer`, `coder`). Missing per the PRD:
`auditor`, `librarian`, `coach`.

**Definition of done:** Build out the three missing archetypes, or
amend PRD-15 to reflect a 3-archetype design.

---

## DG-36 — PRD-15 §9 `.agent.toml` custom-agent format not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

User-authored `.agent.toml` files for custom agents are spec'd; no
parser or loader exists.

**Definition of done:** Per PRD-15 §9.

---

## DG-37 — PRD-15 §10 agent-to-agent communication not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

Spec'd; not built.

**Definition of done:** Per PRD-15 §10.

---

## DG-38 — PRD-17 (Cross-Platform) is desktop-only

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open — needs scoping decision

PRD-17 §3 (WASM target), §4 (`nexus-platform` crate), §5 (web target),
§6 (mobile / UniFFI bindings) are all unimplemented. No `wasm32`,
`uniffi`, or `wasm-pack` deps anywhere.

**Definition of done — needs a scoping call first:**
- Option A: Reframe PRD-17 as "Desktop strategy" and move web/mobile
  to exploratory `roadmap/` or `research/`.
- Option B: Commit to multi-platform; promote each platform to BL
  entries.

---

## DG-39 — PRD-14 §10 dynamic MCP tool registration not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

Community plugins can't publish tools to the MCP server. Tool surface
is static at startup.

**Definition of done:** Per PRD-14 §10.

---

## DG-40 — PRD-14 §12.2 MCP audit logging not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

Kernel audit store exists but isn't called from `crates/nexus-mcp/src/server.rs::call_tool`. MCP tool calls leave no audit trail.

**Definition of done:** Wire `AuditEvent::McpToolCall` (or similar)
through `call_tool`.

---

## DG-41 — PRD-10 §7 relations + rollup not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

Real impl is an in-memory filter chain in `apply_view`. PRD-10 §7
specifies relations between bases plus computed rollups.

**Definition of done:** Per PRD-10 §7.

---

## DG-42 — PRD-10 §8 SQL compilation not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

PRD-10 §8 specifies compiling Bases queries into SQL against the
storage SQLite index. The current implementation does in-memory
filtering only.

**Definition of done:** Per PRD-10 §8.

---

## DG-43 — PRD-06 §9 versioning + migration not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Open

PRD-06 §9 specifies a `version:` frontmatter field and a migration
runner. Neither exists.

**Definition of done:** Per PRD-06 §9. Needed before any
forge-format-breaking change.

---

## DG-44 — PRD-04 §10 dynamic `.so` / `.dll` loading — reject

**Severity:** Cosmetic (product-gap, obsolete)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Should-reject

PRD-04 §10 mentions dynamic loading of `.so`/`.dll` plugins.
Architecturally obsolete: the bootstrap is static, native plugins
compile into the binary, and the community surface is WASM + JS
sandboxes. No `libloading` dep anywhere.

**Definition of done:** Mark PRD-04 §10 as superseded by ADR 0011
+ ADR 0016 (static bootstrap, WASM/JS for community).

---

## DG-45 — ADR 0013 macOS menu-bar plugin never built

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Open

ADR 0013 specifies a Phase-4 macOS menu-bar plugin. No `macos-menu`
plugin exists under `shell/src/plugins/`.

**Definition of done:** Build the plugin, *or* supersede ADR 0013.

---

## DG-46 — ADR 0006 has no in-tree consumer

**Severity:** Cosmetic (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Open (pending community plugin landings)

ADR 0006 is convention-only; no in-tree consumer exists because
community WASM plugins haven't shipped yet. Decision still holds
in spirit.

**Definition of done:** Add a "first consumer landed in <commit>"
note once one ships, or supersede ADR 0006 if the model changes.

---

# Footer / how to use this file

- **Adding a new DG-NN entry:** copy the template at the top, append.
- **Closing a DG:** flip Status to `Resolved <date>`, add an
  `### Outcome` block citing the commits + files touched, exactly
  like `OPEN-ITEMS.md`.
- **Promoting a `product-gap` DG to BL:** open a BL-NN entry in
  `PRDs/BACKLOG.md`, cite this DG-NN in its body, set DG status to
  `Promoted to BL-NN`.
- **Cosmetic items** can be fixed on touch — no need to formally
  resolve; just delete or strike the DG entry in the same commit.
