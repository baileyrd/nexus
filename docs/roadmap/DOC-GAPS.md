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
**Status:** Resolved 2026-05-12

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

**Resolution.** Dropped the STALE warning banner and regenerated the
"Command surface" table from `crates/nexus-cli/src/main.rs` enums
(`Commands` plus 22 sub-enums). The new table covers all 24 top-level
groups including the previously-missing ones (`forge`, `skill`,
`workflow`, `proc`, `term`, `bases`, `template`, `import`, `export`,
`crdt`, `completions`, `watch`, `logs`) and documents the nested
structure for `git branch` / `git stash` / `workflow template`. All
fictional commands flagged by DG-03 are gone: `tags locate`,
`bases validate`, `agent list`, `agent history`, `proc kill`,
`term saved`, `config get/set/list` — replaced by their real
counterparts. The two stub groups (`sync`, `run`) are explicitly
labelled "coming soon"; the plugin-defined `External` subcommand
pathway is documented. Added a "Source of truth: `nexus --help`"
admonition at the top of the section that points readers at clap's
generated help. Lower sections (`tui`, `desktop`, plugin install /
list / scaffold) are still accurate against current source — left
untouched.

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
**Status:** Resolved 2026-05-12

`docs/help/customize/keybindings.md` documents `Ctrl+Shift+T` as "new
terminal session". `shell/src/plugins/nexus/themePicker/index.ts:37`
registers the same binding for the theme picker. Either the docs are
wrong, or the binding needs to move.

**Definition of done:** Decide which feature owns `Ctrl+Shift+T` and
update either the registration or the doc.

**Resolution.** Theme picker keeps `Ctrl+Shift+T` (already shipped +
registered at `shell/src/plugins/nexus/themePicker/index.ts:37`). The
terminal plugin does **not** register any "new session" keybinding —
its only registration is `Ctrl+\`` for toggling the integrated
terminal (`shell/src/plugins/nexus/terminal/index.ts:148`). Two help
docs claimed the wrong owner and got rewritten to the real surface:

- `docs/help/customize/keybindings.md`: replaced the fictional
  "New terminal session | — | `Ctrl+Shift+T`" row with two real
  rows — "Toggle integrated terminal | `Cmd+\`` | `Ctrl+\``" and
  "Open theme picker | `Cmd+Shift+T` | `Ctrl+Shift+T`".
- `docs/help/advanced/terminal.md`: dropped the fictional
  `Ctrl+Shift+T | New session` and `Ctrl+Shift+W | Close session`
  rows (neither is registered). Added a note that new/close session
  are panel-header buttons today and `Ctrl+Shift+T` belongs to the
  theme picker.

---

## DG-06 — `editing/comments.md` describes wrong storage model

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 2
**Status:** Resolved 2026-05-12

Doc claimed comments live in YAML frontmatter. Real storage is a JSON
sidecar per `crates/nexus-comments/src/store.rs`.

**Definition of done:** Rewrite the storage-model section against the
actual sidecar layout.

**Resolution.** Rewrote §"Where comments live" against the real
sidecar layout (`<forge>/.forge/comments/<relpath>.json`) sourced from
`crates/nexus-comments/src/{store.rs,types.rs}`. The replacement
section shows the actual JSON shape (`version`, `file_path`, `threads`
with anchor `block_id`, comments array), describes the
delete-when-empty behaviour, and corrects the three downstream claims
the old "block properties + YAML frontmatter" model implied:

- Markdown body / `git diff` stays clean — comments are out-of-band.
- The sidecar sits inside `.forge/comments/` so VCS sees it either way
  (commit to share, gitignore to keep local).
- Renames do **not** auto-relocate the sidecar — no in-tree rename
  hook on `com.nexus.comments` (the "survives a rename via
  comment-store migration" comment in `core_plugin.rs` is aspirational;
  no migrate code path exists). Manual move is the workaround.

---

## DG-07 — `editing/embeds-and-mdx.md` describes aspirational components

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 3
**Status:** Resolved 2026-05-12

Doc describes `<Card />`, `<Alert />`, `<Badge />` and
`editor.registerMdxComponent` — none exist under `shell/src`. The
real MDX component contract requires `render` to return a `PanelNode`
tree (declarative, host-walked); the doc shows React JSX.

**Definition of done:** Either build the components (promote to a BL
entry under PRD-08) or remove the section and add a "planned" pointer.

**Resolution.** Doc-rewrite path (no components built). The MDX
extractor + renderer ship today (per
`IMPLEMENTATION_STATUS.md` and the CM6 `mdxComponentExtension`); the
gap was the doc claiming Card/Callout/Alert/Badge as built-ins and
showing a fictional `registerMdxComponent(name, options)` shape. The
real API is `ctx.editor.registerMdxComponent(component: MdxComponent)`
where `MdxComponent = { id, name, description?, render(props): PanelNode }`
— host-walked declarative output, not JSX/DOM. Rewrote the §MDX
components and §Custom components sections to: (a) make explicit that
no components ship built-in, plugins author them; (b) show the real
single-argument `MdxComponent` shape with a `PanelNode`-returning
`render`; (c) cite `packages/nexus-extension-api/src/index.ts` as the
authoritative source. Embeds section was untouched — embeds
(`![[Note]]`, `![[image.png]]`) are real per PRD-06 ✅.

---

## DG-08 — `customize/themes.md` references nonexistent scaffold template

**Severity:** Should-fix (doc-bug)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §Help; agent finding 4
**Status:** Resolved 2026-05-12

Doc recommends `nexus plugin scaffold --template theme`. Real
templates are `script` (default), `core`, `community` per
`crates/nexus-cli/src/commands/plugin.rs:216-219`. There is no theme
template.

**Definition of done:** Either add a theme template (BL entry) or
rewrite the doc to use the CSS-snippet path instead.

**Resolution.** Took the CSS-snippet path — no theme template was
added. Rewrote `docs/help/customize/themes.md` §"Author a theme" to
direct readers at `<forge>/.forge/snippets/*.css` (toggled in
Settings → Appearance → CSS Snippets) with a 3-step procedure and a
worked override example targeting `data-nexus-theme="nexus-dark"`.
The 497-token reference at `developer/themes/css-variables.md` is
still linked. Plugin-packaged themes are explicitly deferred to
WI-44 (marketplace).

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
**Status:** Resolved 2026-05-12

Doc shows `build_runtime(forge_root)` + `kernel.register_core_plugin(...)`.
Actual API is `build_cli_runtime(PathBuf)` / `build_tui_runtime(PathBuf)`
with private internal registration. Core-plugin authors following the
doc cannot wire their plugin into the bootstrap.

**Definition of done:** Match the example to the real bootstrap entry
points and the actual `CorePlugin` registration mechanism.

**Resolution.** Rewrote five sections of
`docs/developer/core-plugins/authoring.md` against the real surface
in `crates/nexus-plugins/src/loader.rs` and `crates/nexus-bootstrap/src/lib.rs`:

- **The contract.** Replaced the fictional `#[async_trait] impl CorePlugin`
  with the real synchronous trait. Lifecycle hooks are sync, take no
  `ctx` (`on_init/start: &mut self -> Result<(), PluginError>`,
  `on_stop: &mut self` with no Result). Dispatch is `fn dispatch(&mut self,
  handler_id: u32, args: &serde_json::Value) -> Result<…, PluginError>` —
  **numeric handler IDs**, not string command matching. Added
  `dispatch_async`/`wire_context` and called out that there is no
  `fn info(&self)` method on the trait (identity lives on the manifest).
- **Register in bootstrap.** Replaced the fictional
  `build_runtime` + `kernel.register_core_plugin` example with the real
  pattern: edit the private `register_core_plugins` in
  `crates/nexus-bootstrap/src/lib.rs` to add a
  `loader.register_core(core_manifest_with_ipc(id, name, LifecycleFlags{…},
  &with_v1_aliases(&[("cmd", HANDLER_CMD), …])), forge_root, Box::new(plugin))
  .or_lifecycle_skip(event_bus, …)?` call. Documented the v1-alias
  convention (ADR 0021) and the skip-and-continue failure mode.
- **IPC: handle vs call.** `ctx.ipc_call(...)` requires a `timeout:
  Duration` argument (matches `PluginContext` trait); errors are
  `IpcError`, not the fictional `?` propagation.
- **Events.** Real methods are `ctx.publish(type_id, payload)` and
  `ctx.subscribe(EventFilter)`, not `events_publish` / `events_subscribe`.
  Namespace rule documented (type_id must start with the plugin's id).
- **Tests.** Replaced the fictional `build_test_runtime()` with the
  real pattern: `tempfile::tempdir() + build_cli_runtime(path)`,
  then `runtime.context.ipc_call(plugin, cmd, args, Duration::from_secs(5))`.
  Pointed at the `common::MinimalForge` helper in
  `crates/nexus-bootstrap/tests/common/mod.rs` for new bootstrap-tests.

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
**Status:** Resolved 2026-05-12

OI-13's resolution outcome says it updated `docs/architecture/C4.md`
to drop the `PluginRegistry` component box. The current C4 diagram
still ships those relationships. Either C4.md needs the edit, or
OI-13's outcome wording is wrong.

**Definition of done:** Apply the documented C4 change, *or* correct
OI-13's outcome line.

**Resolution.** C4.md's Component diagram 3a referenced an undeclared
`pluginRegistry` identifier in three `Rel(...)` lines (Mermaid was
auto-creating phantom boxes). The kernel doesn't own a plugin registry
since OI-13 deleted `nexus_kernel::PluginRegistry`; `PluginLoader::loaded`
in `nexus-plugins` is authoritative. Edits in `docs/architecture/C4.md`:
- Dropped `Rel(kernelFacade, pluginRegistry, "Owns")`.
- Dropped `Rel(plugins, pluginRegistry, "Populates")`.
- Reworded `Rel(ipcDispatcher, pluginRegistry, "Looks up backend via")` to
  `Rel(ipcDispatcher, plugins, "Looks up backend via PluginLoader in")`,
  pointing at the real container instead of the deleted type.

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
**Status:** Resolved 2026-05-12

`docs/adr/0002-hierarchical-capability-strings.md` enumerates the
capability inventory; the 8 `ai.*` capabilities added by ADR 0022 are
not in the table. ADR 0022 supersedes the inventory but the original
table reads as authoritative.

**Definition of done:** Add a `## Inventory note (2026-05-12)` section
to ADR 0022 with the full current capability list (22 capabilities),
and add a forward-pointer at the top of ADR 0002. Do not edit ADR
0002's body (immutable convention).

**Resolution.** Inventory note appended to ADR 0022 with all 22
capabilities (14 from ADR 0002 + 6 Phase 1 + 2 Phase 2), each row
citing variant, risk, and origin ADR. Pointer at the source of truth:
`crates/nexus-plugin-api/src/capability.rs::Capability::ALL`, asserted
at length 22 by `tests::all_slice_covers_all_discriminants`. ADR 0002
gained a 2026-05-12 addendum (append-only, body preserved) pointing
forward to the ADR 0022 inventory note.

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
**Status:** Resolved 2026-05-12

ADR 0014 deprecated the `ribbon` slot/concept. `ribbon` still appears
in `packages/nexus-extension-api/src/sandbox/{context,runtime}.ts`
and `index.ts`. Either the deprecation needs follow-through (remove
the API surface) or the ADR needs a "left in place for compat" note.

**Definition of done:** Decide and document. If removing,
breaking-change pass through extension-api consumers.

**Resolution.** The specific finding was already stale at audit time:
`grep -r ribbon packages/nexus-extension-api/` returns zero matches,
and `git log --all -S ribbon -- packages/nexus-extension-api/` shows
no history of the term ever landing there. The script-plugin API is
`activityBar`-only.

What remains in the tree:

- WASM manifest schema in `crates/nexus-plugins/src/{lib.rs,manifest.rs}`
  still uses `ui_ribbon_item` / `UiRibbonItemReg` / `UiRibbonItemContribution`
  + `ui_ribbon_items()`. Rename is a breaking manifest change for WASM
  plugin authors; only in-tree consumer is `plugins/hello-nexus/manifest.toml`,
  so the cost is small but there are no external WASM authors yet to
  justify churn either. **Deferred** to a marketplace-shaped ABI break
  (pair with WI-44 manifest versioning).
- Shell-internal CSS class names (`.workspace-ribbon`, `--ribbon-width`,
  body class `show-ribbon`) stay per ADR 0014's original "Consequences"
  section — Obsidian-parity selectors used by every Obsidian-derived
  theme.

Documented both decisions in a new "Addendum 2026-05-12 — residual
`ribbon` surfaces audit" block at the bottom of ADR 0014, so future
readers know which residues are deliberate (CSS, manifest compat) and
which are pending (manifest rename gated on WI-44). PRD-04 §1.4
already honestly documents the legacy `[[registrations.ui_ribbon_item]]`
manifest key — no edit needed there.

---

## DG-21 — Stale ADR crate enumerations (0001, 0004)

**Severity:** Cosmetic (doc-bug; ADR-addendum required)
**Kind:** `doc-bug`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Resolved 2026-05-12

ADRs 0001 / 0004 enumerate 5–6 crates. Workspace has 28. The *spirit*
of those ADRs (microkernel + crate-per-subsystem) still holds; only
the inventory listing is stale.

**Definition of done:** Add an "Inventory updated 2026-05" addendum
to ADR 0026 (or the most recent ADR) with the current 28-crate
listing and a forward-pointer from ADRs 0001 / 0004.

**Resolution.** Filed the inventory in ADR 0001's appendix rather than
ADR 0026 (which is topically about the CRDT layer, not workspace
structure). ADR 0001 gained an `Addendum 2026-05-12 — workspace grew
to 28 crates` block with the full categorised listing (leaf primitives,
kernel + lifecycle, security, storage plane, editor/content surfaces,
AI/agent, external-system bridges, frontend binaries, quality), naming
the original-decision sextet that ships unchanged and the 22 added
crates by category. ADR 0004 gained a shorter `Addendum 2026-05-12 —
additional crate boundaries` forward-pointer to ADR 0001 plus
post-2026-05-12 boundary highlights (bootstrap is sole all-service
consumer; plugin-api is a leaf; service crates obey microkernel-isolation;
frontends route through `ipc_call`). Both addenda are append-only,
ADR bodies preserved per immutable-body convention.

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
**Status:** Resolved 2026-05-12

ADR 0007 declares anti-spoofing properties of the event bus. No
dedicated test enforces them. The architectural invariants test
suite is the right home.

**Definition of done:** Add `event_bus_anti_spoofing.rs` (or similar)
under `crates/nexus-bootstrap/tests/`.

**Resolution.** Added `crates/nexus-bootstrap/tests/event_bus_anti_spoofing.rs`
with three end-to-end tests against a fully-booted `MinimalForge`
runtime — one per ADR 0007 property:

1. `ctx_publish_produces_custom_with_kernel_set_emitter` — pins
   that publishing through `PluginContext::publish` produces a
   `NexusEvent::Custom` variant (not a spoofed kernel-tier
   variant like `PluginStarted`) and that `emitting_plugin` is set
   by the kernel from the caller's plugin id, regardless of the
   payload shape.
2. `ctx_publish_rejects_foreign_namespace` — pins that
   `type_id` outside the calling plugin's namespace is rejected
   with `BusError::TypeIdNamespaceMismatch` before the event
   reaches the bus (subscribers see nothing).
3. `ctx_publish_rejects_prefix_substring_spoof` — pins the
   dot-separated namespace check by sending `com.nexus.cli-evil.foo`
   (a substring-prefix of `com.nexus.cli`) and asserting it's
   rejected. The substring spoof is the specific class
   `type_id_in_namespace` exists to defend against.

Kernel-internal unit tests already cover (2) and (3) at the helper
level (`event_bus.rs`, `context_impl.rs`); this file locks the
same invariants end-to-end through a real runtime so wiring or
context-impl regressions surface in CI. All three tests pass:
`cargo test -p nexus-bootstrap --test event_bus_anti_spoofing`.

---

## DG-25 — ADR 0020 `popoutCompatible` allowlist unpoliced

**Severity:** Should-fix (status-drift)
**Kind:** `status-drift`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Resolved 2026-05-12

ADR 0020 specifies a `popoutCompatible` allowlist for plugins that
can render in pop-out windows. Nothing verifies that new chrome-only
plugins set the flag correctly. A plugin that forgets it gets a
runtime surprise.

**Definition of done:** Add a contract test verifying every shipped
plugin's `popoutCompatible` value matches its actual capability.

### Outcome

New contract test `shell/src/plugins/popoutCompatible.test.ts`
(surfaced via `shell/tests/popout-compatible.test.ts`) statically
parses each plugin's manifest declaration out of its source file and
asserts it agrees with the catalog entry. The manifest is the plugin's
self-described capability; the catalog is what the runtime filters on
at popout boot. Drift between the two is the "runtime surprise" the
gap calls out.

The test caught three real bugs on first run:

1. `nexus.osArchitecture`, `nexus.osObservability`, and
   `nexus.viewBuilder` declared `popoutCompatible: false` in their
   manifests but the catalog entries were missing the flag. Catalog
   entries fixed in `shell/src/plugins/catalog.ts`.
2. The popout-boot filter in `shell/src/main.tsx` only applied to
   `DEFAULT_ON_PLUGINS`. User-enabled opt-in plugins from
   `DEFAULT_OFF_PLUGINS` were loaded unconditionally, so a chrome-only
   opt-in (e.g. one of the three above, if a user enabled it) still
   booted into popouts regardless of the flag. The boot path now
   applies the same `popoutCompatible !== false` filter to both sets.

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
**Status:** Resolved 2026-05-12

References `packages/nexus-extension-api/src/testing/` — does not
exist. Either build the test harness (BL entry) or rewrite the doc
around the existing `node --test` flow that real plugins use.

**Definition of done:** Decide on the test-helper story; align the
doc.

**Resolution.** Decision: no test harness ships, and there are no
plans to build one — in-tree plugins under `shell/src/plugins/nexus/**`
test with `node:test` + hand-rolled fakes of the `KernelAPI` /
`NexusPluginContext` surface the plugin actually touches. `testing.md`
rewritten around that flow with an opening banner stating the decision,
worked examples mirroring `shell/src/plugins/nexus/status/statusStore.test.ts`,
and a "build the minimum fake your code touches" framing. Remaining
fictional-API drift in this file (the broader DG-01 scope) is unrelated
to the broken `testing/` path and stays open under DG-01.

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
**Status:** Resolved 2026-05-12

`docs/help/` files reference these subcommands that do not exist or
have different semantics:
- `nexus agent list`, `nexus agent history`
- `nexus content delete`, `nexus content links`, `nexus content update --rename`
- `nexus tags locate`
- `nexus ai ask --stdin`
- `nexus plugin reset` (only `reset-crash-count` exists)

**Definition of done:** Either implement (BL entries) or rewrite the
help docs to the actual surface.

**Resolution.** Rewrote the help docs to the actual CLI surface
verified against `crates/nexus-cli/src/main.rs`:

- **Confirmed-real (no doc change needed)**: `nexus content delete`,
  `nexus content links`, `nexus plugin reset` — the last entry on the
  list above was an audit miss; `PluginCommand::Reset { plugin_id }`
  renders as `nexus plugin reset <id>` and is the public surface, even
  though the implementation calls `plugin::reset_crash` under the
  hood.
- `docs/help/advanced/agents.md`: replaced the fictional `agent run
  <archetype> --task <…>` with the real `nexus agent run <goal>
  --archetype <archetype>` form and added an `agent plan` example.
  Removed the `agent list` / `agent history --session <…>` block and
  noted that session listing is shell-only; on-disk transcripts at
  `<forge>/.forge/agents/<session-id>.json` are authoritative.
- `docs/help/linking/tags-and-properties.md`: `tags locate <name>` →
  `tags list --name <name>`. Also dropped the fictional `tags list
  --format json` (no `--format` flag).
- `docs/help/forge-and-files.md`: dropped the fictional `content
  update --rename`; document the real story (no rename subcommand;
  rename at the filesystem and the watcher reindexes).
- `docs/help/advanced/skills.md`: `nexus ai ask --stdin --no-rag` →
  capture the rendered prompt in a shell variable and pass it as the
  positional argument to `nexus ai ask`. `ai ask` takes one positional
  question and no flags.

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
**Status:** Resolved 2026-05-12

PRD-15 specifies a `ToolRegistry` abstraction the agent system calls
into. Not implemented. (Agents currently use ad-hoc dispatch.)

**Definition of done:** Per PRD-15 §4 — typed registry, capability
checks, registration discoverable from `nexus tool list`. Promote to
BL when prioritized.

### Outcome

Typed agent-facing tool registry shipped as
[`crates/nexus-agent/src/tool_registry.rs`](../../crates/nexus-agent/src/tool_registry.rs):

- `Capability` enum covering the ten capability domains PRD-15 §4
  lists. Serializes as the canonical dotted-id string (`fs.read`,
  `terminal.execute`, …) so wire / CLI / `.agent.toml` (DG-36) all
  speak the same form. `Capability::as_str` / `Capability::from_str`
  is the round-trip surface.
- `AgentToolSpec` carries name, description, JSON-Schema input
  schema, `requires_approval` flag, `estimated_duration_ms` hint,
  `required_capabilities`, and the kernel IPC route
  (`target_plugin_id` / `command_id`) so a session loop can dispatch
  through the existing `ToolDispatcher` without re-implementing
  transport.
- `AgentToolRegistry` is a process-global (`OnceLock<Arc<…>>` —
  same pattern DG-39 introduced for MCP dynamic tools) catalogue.
  `bootstrap::register_core_plugins` calls
  `nexus_agent::seed_default_tools()` after registering
  `com.nexus.agent`, seeding the eight in-tree tools that mirror the
  AI executor's catalogue (`read_file`, `write_file`, `search_forge`,
  `list_backlinks`, `git_log`, `terminal_run_saved`,
  `terminal_get_status`, `terminal_send_signal`). `write_file` and
  the two terminal-write tools are marked `requires_approval: true`
  so DG-34 (interactive approval) can read the flag without a
  separate config surface.
- `list_for_agent(&[Capability])` returns only the tools an agent's
  granted capabilities satisfy. `validate_params` runs the
  structural checks PRD-15 calls for (`required` keys present,
  `additionalProperties: false` honoured). `check_capabilities`
  returns a typed `AgentToolError::CapabilityDenied` on the first
  missing capability. `record_access` + `access_log` keep a bounded
  (1024-entry) in-memory audit trail.
- New IPC handler `com.nexus.agent::list_tools` (handler id 18) —
  sync path, no kernel context needed. Args:
  `{ capabilities?: ["fs.read", …] }`; reply: sorted
  `Vec<AgentToolSpec>`. Unknown capability ids fail loudly so a
  typo doesn't silently return the full catalogue.
- New CLI command `nexus tool list [--capability ID]…` in
  [`crates/nexus-cli/src/commands/tool.rs`](../../crates/nexus-cli/src/commands/tool.rs)
  — routes through the IPC handler, renders a fixed-width table
  with `NAME`, `APPR`, `~ms`, `CAPS`, `DESCRIPTION`.
- ts-rs / schemars bindings regenerated under
  `packages/nexus-extension-api/src/generated/ipc/{Capability,AgentToolSpec,AgentToolAccessRecord,ListToolsArgs}.ts`
  and `crates/nexus-bootstrap/schemas/ipc/`; `scripts/check_ipc_drift.sh`
  clean.
- Test coverage: 16 unit tests in `tool_registry.rs` (capability
  round-trip, register / lookup / overwrite, capability-filtered
  listing, capability check, params validation matrix, access-log
  cap) + 3 IPC dispatch tests in `core_plugin.rs` (full catalog,
  unknown capability rejection, capability-filter narrowing). All
  41 lib tests green.

**Deferred** as documented follow-ups:

- `call_with_retry` / exponential backoff (PRD-15 §4) — the
  registry tracks `estimated_duration_ms` and `requires_approval`
  but doesn't own the dispatch loop yet. Retry policy lives in the
  session executor where it can see proposed-call context; once a
  user request needs it, the wiring is one method on `AgentToolRegistry`
  that takes a `&dyn ToolDispatcher`.
- `parse_result` / `ParsedToolResult` (PRD-15 §4) — the existing
  session loop already classifies tool outcomes into the
  `RoundDecision` shape. Lifting a separate `ParsedToolResult`
  would duplicate that surface; revisit if a non-session caller
  needs structured tool-result parsing.

---

## DG-33 — PRD-15 §5 Memory not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Resolved 2026-05-12

PRD-15 §5 specifies agent-scoped persistent memory. Not implemented;
runs are stateless.

**Definition of done:** Per PRD-15 §5. Related to AI-MEMORY-LAYER-PLAN
(roadmap exploratory).

### Outcome

Filesystem-backed agent-scoped memory shipped:

- New [`crates/nexus-agent/src/memory.rs`](../../crates/nexus-agent/src/memory.rs)
  defines the `MemoryEntry` enum covering the eight variants
  PRD-15 §5 calls out (`UserGoal`, `AgentPlan`, `StepExecution`,
  `ToolCall`, `UserFeedback`, `Error`, `Decision`, `Artifact`) plus
  the surrounding primitives: `agent_dir`, `history_path`,
  `normalize_agent_id` (validates ASCII alphanumeric / `.-_`,
  ≤96 chars — mirrors the existing plan-history slug rule),
  `append_entry_to_path`, `read_entries_from_path` (skips malformed
  lines with a warn), `query_entries` (case-insensitive substring,
  newest-first), `prune_entries` (drops everything older than the
  retention window **except** `Decision` entries — PRD-15 §5
  invariant), and `export_markdown`.
- Storage layout matches the PRD: `<forge>/.forge/agents/<agent_id>/`
  with `history.jsonl` (append-only one-entry-per-line log) and
  reserved `snapshots/` + `artifacts/` subdirs (the entry variants
  reference artifacts by path so the layout is forward-compatible
  even though dated snapshots aren't wired yet — see deferred).
- Four IPC handlers on `com.nexus.agent`:
  - `memory_record` (id 20) — append a `MemoryEntry` to the log.
    Read-modify-write through `ctx.read_file` + `ctx.write_file`
    so the capability surface stays correct (the kernel doesn't
    expose a raw append primitive).
  - `memory_query` (id 21) — substring filter + limit; default
    limit 50, newest-first.
  - `memory_prune` (id 22) — drop entries older than
    `retention_days * 86_400_000` ms while preserving `Decision`
    entries; rewrites the log via `write_file`.
  - `memory_export` (id 23) — render the entire log as
    human-readable markdown.
- ts-rs / schemars bindings generated for `MemoryEntry`,
  `MemoryRecordArgs`, `MemoryQueryArgs`, `MemoryPruneArgs`,
  `MemoryExportArgs` under
  `packages/nexus-extension-api/src/generated/ipc/`;
  `scripts/check_ipc_drift.sh` clean.
- 13 unit tests in `memory::tests` cover the parse / round-trip
  (append+read), query (substring case-insensitive, empty pattern
  newest-first, limit cap), prune (drops old non-decisions, keeps
  decisions), export (every variant renders), agent-id validator
  (reverse-DNS / empty / too-long / unsafe chars), and missing
  file handling. All 68 nexus-agent lib tests green.

**Deferred** as documented follow-ups:

- Auto-recording from the session loop. The session loop in
  `session.rs` writes per-session transcripts under
  `.forge/agent/sessions/` already; threading those events through
  `memory_record` so a Coder agent's prior decisions show up on
  its next invocation is the integration step. Foundation is in
  place (the handler exists; the session loop knows the agent id);
  one focused PR away.
- Dated `MemorySnapshot` rollups. The PRD-15 §5 `MemorySnapshot`
  type wasn't materialised — the JSONL log is the canonical
  source. A rollup writer that compresses old history into a
  weekly snapshot can land if log file sizes become an issue;
  retention-by-prune already keeps the active log bounded.
- Database backend (`MemoryStore::Database`). FS-only today; the
  IPC surface is backend-agnostic so swapping requires only a
  handler-side route change.
- Prompt-time recall (injecting prior memory into the planner's
  system prompt at session start). The query handler is in place;
  binding it into `system_prompt_with_skills` is the next step
  for a "memory layer" feel.

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
**Status:** Resolved 2026-05-12

User-authored `.agent.toml` files for custom agents are spec'd; no
parser or loader exists.

**Definition of done:** Per PRD-15 §9.

### Outcome

Parser + loader + IPC + CLI shipped:

- New [`crates/nexus-agent/src/custom_agent.rs`](../../crates/nexus-agent/src/custom_agent.rs)
  ships `CustomAgentManifest` plus the five sub-section types
  (`AgentSection`, `ExecutionSection`, `ToolsSection`,
  `MemorySection`, `SystemPromptSection`) matching the PRD-15 §9
  TOML schema. Each section carries `#[serde(deny_unknown_fields)]`
  so typos (e.g. `max_stepz`) fail loudly rather than silently
  defaulting.
- `parse_str(body, slug, path)` and `load_from_path(&Path)` parse
  one manifest; `scan_forge(&Path)` walks
  `<forge>/.forge/agents/*/agent.toml` and returns a tuple of
  loaded manifests + per-manifest errors so a single broken file
  doesn't poison the listing. Output is slug-sorted for stable
  CLI / shell diffs.
- `resolve_system_prompt` reads the prompt body — either inline
  via `[system_prompt].text` or by following
  `[system_prompt].path` relative to the manifest directory.
  Mutual-exclusivity and presence checks happen at parse time.
- New IPC handler `com.nexus.agent::list_custom` (handler id 19,
  async) walks `.forge/agents/` via `ctx.list_files`, parses each
  `agent.toml`, and replies `{ manifests, errors }`. Missing
  directory is a clean empty reply — most forges won't have a
  custom-agents directory yet.
- New CLI command `nexus agent list-custom` renders the result as
  a fixed-width `SLUG | NAME | ARCHETYPE` table with errors below.
- ts-rs / schemars bindings generated for `CustomAgentManifest`,
  `AgentSection`, `ExecutionSection`, `ToolsSection`,
  `MemorySection`, `SystemPromptSection` under
  `packages/nexus-extension-api/src/generated/ipc/`;
  `scripts/check_ipc_drift.sh` clean.
- 11 unit tests in `custom_agent::tests` cover the parse matrix
  (minimal / full PRD example / missing prompt / both text+path /
  invalid memory storage / unknown execution field), the forge
  scanner (missing dir / multiple manifests sorted / broken
  manifest isolated to errors), and prompt resolution (inline /
  file). All 55 nexus-agent lib tests green.

**Deferred** as documented follow-ups:

- Routing a custom agent through `nexus agent plan/run --archetype <slug>`
  isn't wired yet. `build_archetype` in `archetypes.rs` resolves
  built-in names only; a `--archetype` arg that matches a custom
  manifest's slug should layer that manifest's `system_prompt`
  over its `archetype` baseline. Foundation is in place — every
  manifest exposes the system prompt + base archetype — but the
  resolver itself is one follow-up away (read scan result during
  `handle_plan` / `handle_session_run`).
- Tool allow/deny enforcement against `AgentToolRegistry` likewise
  pending until the planning loop reads the manifest at request
  time.

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
**Status:** Resolved 2026-05-12 — **Option A** (reframe as desktop-only)

PRD-17 §3 (WASM target), §4 (`nexus-platform` crate), §5 (web target),
§6 (mobile / UniFFI bindings) are all unimplemented. No `wasm32`,
`uniffi`, or `wasm-pack` deps anywhere.

**Definition of done — needs a scoping call first:**
- Option A: Reframe PRD-17 as "Desktop strategy" and move web/mobile
  to exploratory `roadmap/` or `research/`.
- Option B: Commit to multi-platform; promote each platform to BL
  entries.

### Outcome

Option A chosen. PRD-17 reframed as "Desktop Strategy":

- Title and executive summary rewritten in
  [`docs/PRDs/17-cross-platform-strategy.md`](../PRDs/17-cross-platform-strategy.md);
  status line bumped to "1.0 (reframed 2026-05-12 per DG-38)".
- Header callout at the top spells out the scoping decision and lists
  every section that is now considered deferred design rationale
  rather than committed work.
- Per-section "Deferred (DG-38, 2026-05-12)" callouts inserted at
  §3 (WASM), §5 (Web), §6 (Mobile), §15 (Web Onboarding), §16
  (Mobile UX), and a "Partially deferred" callout at §4 (Platform
  Abstraction) noting that the desktop column ships today but the
  separate `nexus-platform` crate was never created — desktop callers
  use Tauri / keyring / portable-pty directly.
- [`docs/PRDs/00-index.md`](../PRDs/00-index.md) PRD-17 row retitled
  "Desktop Strategy" with a scope summary that names DG-38.
- [`docs/PRDs/IMPLEMENTATION_STATUS.md`](../PRDs/IMPLEMENTATION_STATUS.md)
  PRD-17 entry retitled and its Gaps line rewritten to point at this
  DG; web/mobile no longer count as gaps. Tauri-updater signing is
  left tracked under WI-41 (formal-release scope).

The PRD body sections are preserved verbatim so the design thinking
survives. If multi-platform is ever pursued, each platform should be
promoted to its own BL entry and re-validated against ADR 0011
(single-shell desktop) + ADR 0016 (microkernel native-vs-WASM split)
before any code lands.

---

## DG-39 — PRD-14 §10 dynamic MCP tool registration not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Resolved 2026-05-12

Community plugins can't publish tools to the MCP server. Tool surface
is static at startup.

**Definition of done:** Per PRD-14 §10.

### Outcome

- New module `crates/nexus-mcp/src/dynamic_tools.rs` ships
  `DynamicTool`, `DynamicToolRegistry`, and a process-global accessor
  (`global()`) following the same `OnceLock<Arc<…>>` pattern as
  `nexus_kernel::audit_store`. Eight unit tests cover register /
  duplicate-rejection / reserved-prefix rejection (the `nexus_`
  namespace is owned by the static router) / empty-name rejection /
  unregister-then-reregister / alphabetical-list / pointer-equality
  of `global()`.
- `McpHostPlugin::dispatch` gains three new sync handlers —
  `HANDLER_REGISTER_TOOL` (id 8), `HANDLER_UNREGISTER_TOOL` (id 9),
  `HANDLER_LIST_DYNAMIC_TOOLS` (id 10). Plugins announce or withdraw
  tools by calling these via `ipc_call("com.nexus.mcp.host", …)`.
  Four integration tests in `core_plugin::tests` exercise the
  dispatch path including reserved-prefix rejection at the IPC layer.
- `NexusMcpServer::list_tools` returns the static
  `tool_router.list_all()` followed by the dynamic registry's
  contents (`dynamic_tool_to_rmcp` adapts the descriptor into rmcp's
  `Tool`). `call_tool` checks the dynamic registry first; on hit it
  routes through `context.ipc_call(plugin_id, command, args)` and
  wraps the JSON reply as `CallToolResult::structured`, on miss it
  falls through to the static `tool_router` unchanged. Audit logging
  (DG-40) wraps both paths uniformly.
- PRD-14 §10's "Latency: < 100ms from registration to availability"
  is satisfied trivially — registration is an in-process map insert
  and the next `list_tools` request sees it. The PRD's
  `notifications/tools/list_changed` broadcast to connected clients
  is deferred: it's a nice-to-have and external MCP clients can
  re-poll `list_tools` cheaply. When a UI surfaces tool publication
  events, the broadcast can land as a follow-up that reads the
  registry on a kernel event-bus subscription.
- Capability gating is deferred to a follow-up. Today the registry
  trusts whichever plugin calls `register_tool`; the existing
  `ipc.call` capability check on the publisher's side is the gate.
  A dedicated `mcp.publish_tool` capability would tighten this
  further and pairs naturally with BL-099 (signed plugins).

---

## DG-40 — PRD-14 §12.2 MCP audit logging not implemented

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §PRDs
**Status:** Resolved 2026-05-12

Kernel audit store exists but isn't called from `crates/nexus-mcp/src/server.rs::call_tool`. MCP tool calls leave no audit trail.

**Definition of done:** Wire `AuditEvent::McpToolCall` (or similar)
through `call_tool`.

### Outcome

- New `nexus_kernel::audit::log_mcp_tool_call(tool, duration_ms, result, error)`
  and `log_mcp_resource_read(uri, duration_ms, result, error)` helpers
  emit a structured `tracing::info!` event (`audit = true`) and
  append a typed row (`mcp_tool_call` / `mcp_resource_read`) into the
  kernel audit store via the existing `audit_store::append` path. The
  SQLite-backed implementation in `nexus-bootstrap` picks them up
  unchanged.
- `crates/nexus-mcp/src/server.rs::call_tool` captures the tool name
  and wall-clock duration around `tool_router.call(...)` and calls
  `log_mcp_tool_call` with `"success"` or `"error"` + message.
- `read_resource` does the same for resource reads — both URI parse
  failures and downstream `storage::read_file` errors are audited as
  failures alongside successful reads.
- Five new unit tests in `crates/nexus-kernel/src/audit.rs` cover
  emission for both success and error paths.
- PRD-14 §12.2's separate `mcp-audit.jsonl` file with daily rotation
  is intentionally not built: the kernel audit store already provides
  a single durable audit surface across capability, lifecycle,
  credential, and now MCP events, and `nexus logs` already queries it.
  Adding a second sink for MCP-only events would split the audit
  trail. If a JSONL exporter is needed later it can read from the
  same store.

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
**Status:** Resolved 2026-05-12 (rejected)

PRD-04 §10 mentions dynamic loading of `.so`/`.dll` plugins.
Architecturally obsolete: the bootstrap is static, native plugins
compile into the binary, and the community surface is WASM + JS
sandboxes. No `libloading` dep anywhere.

**Definition of done:** Mark PRD-04 §10 as superseded by ADR 0011
+ ADR 0016 (static bootstrap, WASM/JS for community).

**Resolution.** `docs/PRDs/04-plugin-system.md §10` gained a
"Superseded by ADR 0011 + ADR 0016" callout at the top of the section.
The original body is preserved for historical context; the callout
makes clear `libloading` is not a workspace dep, bootstrap is static,
every core plugin compiles into the binary via `nexus-bootstrap`, and
community plugins are WASM + JS sandboxes only.

---

## DG-45 — ADR 0013 macOS menu-bar plugin never built

**Severity:** Should-fix (product-gap)
**Kind:** `product-gap`
**Surfaced by:** [../audits/traceability-2026-05-12.md](../audits/traceability-2026-05-12.md) §ADRs
**Status:** Resolved 2026-05-12 (re-phased to formal release)

ADR 0013 specifies a Phase-4 macOS menu-bar plugin. No `macos-menu`
plugin exists under `shell/src/plugins/`.

**Definition of done:** Build the plugin, *or* supersede ADR 0013.

### Outcome

ADR 0013's decision (palette-first everywhere with a macOS native
menu-bar exception for platform conformance) **still stands**. What
changed is the timing: the plugin's original Phase 4 target slipped,
Phase 4 closed 2026-04-24 (`app/` → `shell/` migration) without the
plugin, and no user friction has surfaced from its absence on macOS
in the months since.

Instead of either building the plugin blindly (no macOS build
environment) or superseding a still-correct decision, the resolution
re-phases the plugin to the formal-release Mac packaging window:

- New addendum on `docs/adr/0013-menu-bar-strategy.md` documents
  the slip, restates the decision, and points at the new tracker.
- New [WI-45](REQUIRED-FOR-FORMAL-RELEASE.md) entry in
  `docs/roadmap/REQUIRED-FOR-FORMAL-RELEASE.md` scopes the plugin
  (four menus, dispatch through existing `api.commands.execute()`,
  no contribution point, ~1 engineer-day) and ties its build window
  to WI-41 (Tauri auto-updater + code-signing + notarization), so
  both Mac-environment work items land in the same push.

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
