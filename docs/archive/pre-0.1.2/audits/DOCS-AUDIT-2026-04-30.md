# Nexus Documentation Audit & Reorganization Plan

> **Archived 2026-04-30** — superseded by execution. The findings here
> drove the `docs-reorg` branch (P0 + P1 + P2 changes); this report is
> kept as the audit-trail snapshot. For the active doc structure see
> [`../README.md`](../README.md). Will be moved into `archive/` via
> `git mv` on a follow-up commit so history follows the file.

> **Audit date:** 2026-04-30
> **Scope:** Every markdown file in the workspace (root, `docs/`, `docs/adr/`, `docs/PRDs/`, `docs/archive/`, `shell/docs/`, `packages/`, `crates/`).
> **Method:** Inventory → cross-check claims against current code (Cargo workspace members, Tauri bridge surface, MCP tool registrations, file existence) → assess coherence and overlap → propose a target structure.
> **Status:** Findings only. No files moved, renamed, or rewritten at the time of writing — the docs-reorg branch executes the recommendations.

---

## 1. Headline findings

The good news first: the doc set is **substantively complete**. Every active subsystem has a written record somewhere — PRDs for product intent, ADRs for design decisions, narrative architecture docs for the big picture, per-crate READMEs in a few cases, and a clean shell-side reference under `shell/docs/`. The archive is well-curated, with a top-level `docs/archive/README.md` (dated 2026-04-26) that explains *what was archived and why*, and per-file `> **Archived <date>** — <reason>` headers on the moved files. That kind of hygiene is rare and worth preserving.

The bad news: **the entry points lie.** Specifically:

1. **Both architecture overviews — `docs/ARCHITECTURE.md` and `docs/architecture/C4.md` — still describe the retired legacy shell** (`nexus-app` crate, `app/` directory) as a current container. That code was deleted in v0.4.0 on 2026-04-24. A new contributor reading either doc today would build a wrong mental model on their first afternoon.
2. **`README.md` lists the wrong MCP tool names.** It claims tools called `forge_read`, `forge_write`, `note_create`, `export_html`, `plugin_call`, etc. The actual tools registered in `crates/nexus-mcp/src/server.rs` use a `nexus_*` prefix and a different shape (e.g., `nexus_read_note`, `nexus_search`, `nexus_render_skill`). Tool *count* is also off (15 actual vs 13 claimed across three docs).
3. **`CLAUDE.md` understates the Tauri bridge.** It claims exactly 7 commands. The actual `invoke_handler` in `shell/src-tauri/src/lib.rs` (lines 443-466) registers 22. The "thin bridge" guardrail is real and important, but the bridge has accreted plugin-management, persistence, and popout-window commands without the doc keeping up.
4. **The crate inventory is incomplete in several places.** The Cargo workspace has 24 members; `README.md`, `CLAUDE.md`, and `docs/ARCHITECTURE.md` each list a subset. `nexus-comments` and (in some lists) `nexus-plugins` are consistently missed.
5. **Two docs contain the same architecture content with different staleness.** `ARCHITECTURE.md` and `architecture/C4.md` are 80% the same C4 model — both stale in the same way. One of them needs to die.
6. **The original inventory under-counts active docs.** `/docs/` actually contains a load of in-flight planning docs the first sweep didn't surface: `OPEN-ITEMS.md`, `REQUIRED-FOR-FORMAL-RELEASE.md`, `AI-INTEGRATION-DIRECTIONS.md`, `AI-MEMORY-LAYER-PLAN.md`, `AI-AMBIENT-COPILOT-PLAN.md`, `leaf-architecture.md`, `architecture/C4.md`. The archive README's "what's *not* archived" table (lines 65-81 of `docs/archive/README.md`) is a much better inventory of active docs than any other index in the repo.
7. **Nothing wires it all together.** There is no top-level `docs/README.md` index telling a contributor "start here." Six different docs are plausible entry points (`README.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/architecture/C4.md`, `docs/PRDs/00-index.md`) and none of them links comprehensively to the others.

If you do nothing else from this report: kill the legacy-shell references in the two architecture docs, fix the MCP tool table in the README, and add a `docs/README.md` index. Those three changes remove ~80% of the confusion.

---

## 2. Inventory

Counts derived from a directory walk. ~136 markdown files total.

| Location | Files | Notes |
|---|---:|---|
| Repo root | 4 | `README.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `DEPRECATED.md` |
| `docs/` (top level) | ~13 | Active narrative docs + planning. See §3 — incomplete previous inventory. |
| `docs/architecture/` | 1 | `C4.md` (parallels `docs/ARCHITECTURE.md`) |
| `docs/adr/` | 20 | ADR 0001-0020. All Accepted except 0012 (Rejected). |
| `docs/PRDs/` | 21 | `00-index.md`, 17 numbered PRDs, `IMPLEMENTATION_STATUS.md`, `BACKLOG.md`, `Nexus_Growth_Plan.md` |
| `docs/PRDs/templates/` | 2 | `community-plugin/README.md`, `core-plugin/README.md` |
| `docs/references/` | 1 | `obsidian-settings-modal.md` |
| `docs/archive/` (top + planning + superpowers) | ~60 | Well-indexed. Top-level `archive/README.md` and `archive/planning/README.md` and `archive/superpowers/README.md` all exist. |
| `shell/docs/` | 11 | `architecture.md`, `plugin-api.md`, `plugin-system.md`, `core-plugins.md`, `registry-system.md`, `event-bus.md`, `slot-system.md`, `context-keys.md`, `workspace-layout.md`, `extension-host.md`, `writing-a-plugin.md` |
| `shell/docs/obsidian/` | 2 | `obsidian-runtime.md`, `obsidian-measurements.md` (reference for parity) |
| `shell/docs/archive/` | 5 | `README.md`, `MIGRATION_PLAN.md`, `predefined-layouts.md`, `plans/keybinding-storage-refactor.md`, `plans/keybindings-ui-fixes.md` |
| `shell/src/plugins/` | 1 | `community/mermaid/README.md` (the only plugin-level README in tree) |
| `packages/` | 2 | `nexus-extension-api/README.md`, `nexus-extension-api/DEPRECATED.md` |
| `crates/` | 5 | 4 skill definition files in `nexus-skills/builtins/`, plus `nexus-plugins/templates/script/README.md` |

---

## 3. Per-file findings

Each finding is labelled **STALE** (factual claim has been overtaken by code), **WRONG** (claim was never right or got corrupted), **MISSING** (referenced but not present), **DUPLICATE** (overlaps with another doc), or **OK** (verified correct). Severity in brackets.

### 3.1 Repo root

#### `README.md`

- **WRONG [HIGH]** — *MCP tool names.* Lines 150-163 list tools `forge_read`, `forge_write`, `forge_search`, `note_create`, `note_delete`, `note_list`, `graph_status`, `graph_unresolved`, `task_list`, `task_toggle`, `search`, `export_html`, `plugin_call`. The actual MCP server in `crates/nexus-mcp/src/server.rs` registers 15 tools using a `nexus_*` prefix: `nexus_read_note`, `nexus_create_note`, `nexus_update_note`, `nexus_delete_note`, `nexus_list_notes`, `nexus_search`, `nexus_backlinks`, `nexus_outgoing_links`, `nexus_graph_status`, `nexus_list_tags`, `nexus_list_tasks`, `nexus_toggle_task`, `nexus_ask`, `nexus_list_skills`, `nexus_render_skill`. There is **zero overlap** between the two name sets. This table appears to document an older or planned API that never shipped.
- **WRONG [HIGH]** — *MCP tool count.* Lines 17 and 100-104 both say "13 tools." The implementation registers 15.
- **STALE [MEDIUM]** — *Crate summary.* Lines 13-23 list 11 crates. The workspace has 24. Notably absent: `nexus-comments`, `nexus-plugins`, `nexus-formats`, `nexus-panic-log`, `nexus-agent`, `nexus-skills`, `nexus-workflow`, `nexus-editor`, `nexus-terminal`, `nexus-linkpreview`, `nexus-database`, `nexus-git`, `nexus-kv`, `nexus-plugin-api`. Some of those omissions are defensible for a top-level summary; quietly dropping `nexus-comments` is not.
- **OK** — Doc cross-links at lines 218-223 all resolve. `IMPLEMENTATION_STATUS.md`, `BACKLOG.md`, the ADR directory, and `writing-your-first-plugin.md` all exist where claimed.

#### `CLAUDE.md`

- **WRONG [HIGH]** — *Tauri bridge surface.* Lines 41-43 and 66 claim the bridge is exactly 7 commands (`init_forge`, `boot_kernel`, `kernel_invoke`, `kernel_subscribe`, `kernel_unsubscribe`, `kernel_is_booted`, `shutdown_kernel`). The `invoke_handler!` block at `shell/src-tauri/src/lib.rs:443-466` registers 22: those 7 plus `scan_plugin_directory`, `scan_plugin_directory_at`, `set_plugin_enabled`, `get_plugin_granted_capabilities`, `set_plugin_granted_capabilities` (plugin management), `path_exists`, `persistence::get_shell_state`, `persistence::save_shell_state`, `persistence::write_last_forge_path`, `persistence::forget_forge_path` (persistence/utility), and `windows::popout_window`, `windows::close_popout_window`, `windows::list_popout_windows`, `windows::get_popout_window_bounds`, `windows::set_popout_window_bounds` (popout — see ADR 0020). This isn't just a number — the guardrail "do not add bespoke `#[tauri::command]` handlers" reads as inconsistent with reality, which dilutes the rule.
- **STALE [MEDIUM]** — *Service crate list.* Line 57 lists 15 services. `nexus-comments` is absent; the workspace has it. `nexus-plugins` is debatable (it's a library, not a `CorePlugin`-implementing service crate) but if it's omitted, the doc should say why.
- **OK** — Build commands, IPC drift script reference, and the "single Tauri target" / "v0.4.0 retirement" claims match reality. The architectural invariants (file-as-truth, microkernel isolation, IPC over direct calls, capabilities) all hold.

#### `CONTRIBUTING.md`

- **WRONG [MEDIUM]** — *Feature plan paths.* Lines 73-74 say "Feature plans live in `docs/` (e.g. `leaf-architecture.md`, `shell-kernel-bridge-plan.md`, `canvas-shell-plan.md`, `bases-shell-plan.md`)." Of these, only `leaf-architecture.md` is still in `docs/`. The other three were moved to `docs/archive/` per `docs/archive/README.md` (lines 47-54: `shell-kernel-bridge-plan.md`, `bases-shell-plan.md`, `canvas-shell-plan.md` are listed there with "shipped" tags). Citing them as live planning docs is misleading.
- **WRONG [MEDIUM]** — *Same Tauri bridge claim as CLAUDE.md.* Same omission of 15 commands (22 actual vs 7 documented). Same fix.
- **OK** — `DEPRECATED.md` reference at line 96 is valid; the file exists at the repo root.

#### `DEPRECATED.md`

- **OK** — Coherent. Empty "Currently deprecated" section is appropriate. Trust policy section at the bottom describes script plugin sandboxing and matches ADR 0015.

### 3.2 `docs/` top-level

#### `docs/ARCHITECTURE.md`

- **STALE [HIGH]** — *Date stamp.* Line 4 says "Date generated: 2026-04-14." That's 16 days old at audit time, *and* it's after the 2026-04-24 legacy-shell retirement which the doc still describes as live.
- **WRONG [HIGH]** — *Legacy shell still in C4.* Line 131 includes `Container(app, "nexus-app (Desktop)", "Tauri 2 + Vite/React", ...)`. Section 4.7 (per the TOC) is titled "Tauri Desktop Shell" and describes `app/`. Both are gone — see `docs/legacy-shell-retirement.md`.
- **WRONG [MEDIUM]** — *MCP tool count.* Lines 55, 140, 215, and 384 all say "13-tool MCP server." Should be 15 (see §3.1).
- **STALE [LOW]** — *Crate count.* Line 3 says "~16 crates"; workspace has 24.
- **OK** — Microkernel invariants, capability taxonomy, RAG pipeline, event bus diagrams remain accurate to current code.

#### `docs/architecture/C4.md`

- **DUPLICATE [HIGH]** — *80% overlap with `ARCHITECTURE.md`.* Both walk the same C4 levels with similar Mermaid diagrams. Two architecture overviews of similar scope is one too many.
- **WRONG [HIGH]** — *Same legacy-shell staleness.* Lines 53-54 describe the desktop shell as "`nexus-app` crate + `shell/src-tauri` + `app/` React frontend." Lines 71-72 add `Container(desktopShell, "Desktop Shell", "Tauri 2 (nexus-app + shell/src-tauri)", ...)` and `Container(webUi, "Web UI", "React + Vite + TypeScript (app/)", ...)`. All references to `nexus-app` and `app/` need to be replaced with `nexus-shell` and `shell/`.
- **WRONG [MEDIUM]** — *Same MCP tool count of 13.*
- **OK** — System Context (Level 1) is generic enough to remain accurate.

#### `docs/leaf-architecture.md`

- **OK** (per spot-check of opening lines). Doc explains the chrome/content separation that the workspace-layout system depends on. Referenced from active docs and archive README. Active.

#### `docs/legacy-shell-retirement.md`

- **OK** — Coherent migration story. Status "Retired" with date 2026-04-24 matches reality. This is the canonical answer to "what happened to `app/` and `nexus-app`?"

#### `docs/ipc-schemas.md`

- **NOT VERIFIED in detail** — README says "pilot phase with 5 handlers documented." The actual generated schema directory `crates/nexus-bootstrap/schemas/ipc/` has 28 JSON schema files, and `packages/nexus-extension-api/src/generated/ipc/` has 30 TypeScript types. If `ipc-schemas.md` still describes only 5, it is significantly behind the generator. **Recommended deep-check** when fixing.

#### `docs/nexus-cli.md`

- **NOT VERIFIED in detail** — Listed as current per the archive README. Recommend cross-checking command list against `crates/nexus-cli/src/commands/` once you have time.

#### `docs/editor-transaction-architecture.md`

- **OK presumed** — Listed as current per archive README; outside this audit's depth.

#### `docs/notion-block-ux-plan.md`

- **OK presumed** — Listed as current "in-flight plan" per archive README.

#### `docs/OPEN-ITEMS.md`

- **OK** — Tracks post-migration capability gaps from the 2026-04-24 sweep. Active. Not surfaced in the original inventory pass.

#### `docs/REQUIRED-FOR-FORMAL-RELEASE.md`

- **OK** — Tracks WI-41 (auto-updater), WI-42 (Sentry), WI-44 (marketplace), WI-46 (beta→GA) deferred from personal-tool scope. Indexed from `BACKLOG.md`. Active.

#### `docs/AI-INTEGRATION-DIRECTIONS.md`, `AI-MEMORY-LAYER-PLAN.md`, `AI-AMBIENT-COPILOT-PLAN.md`

- **OK** — All three labelled "Status: exploratory," indexed from `BACKLOG.md → Future directions`. Active design rationale docs. Not surfaced in original inventory.

#### `docs/writing-your-first-plugin.md`

- **OK** — Plugin author tutorial. Capabilities and scaffold command match `nexus-cli` and `nexus-plugin-api`. Pairs with `shell/docs/writing-a-plugin.md` (reference).

#### `docs/references/obsidian-settings-modal.md`

- **OK** — UX reference for parity work. Active per archive README.

### 3.3 `docs/adr/`

ADRs are in good shape overall. Each has a status field; numbering is contiguous; topics don't blatantly contradict each other; ADR 0012 is correctly marked Rejected. Spot findings:

- **OK** — ADRs 0001-0010, 0013, 0014, 0017, 0019, 0020 all describe decisions still load-bearing in code. Specifically, ADR 0018 (fastembed-rs) is verified — `nexus-ai/Cargo.toml` includes the fastembed dependency.
- **OK** — ADR 0015 (iframe sandbox for JS/TS plugins) and ADR 0016 (microkernel native + WASM split) read as orthogonal, not contradictory: 0015 is the UI-layer JS/TS plugin runtime; 0016 is the kernel-layer split between native Rust and WASM plugins. Both still apply, but the relationship would benefit from a one-line cross-reference in each ADR's "Context" section so future readers don't suspect a conflict.
- **OK** — ADR 0011 (adopt plugin-first shell) is the historical record of the legacy retirement. Status Accepted is appropriate; the decision is *executed*, not *superseded*.
- **OK** — ADR 0020 (popout window) accepted 2026-04-30 (today). The 5 popout `#[tauri::command]` handlers in `shell/src-tauri` are this ADR's implementation. Update CLAUDE.md and CONTRIBUTING.md to reflect that those commands exist by design.
- **MINOR** — `docs/adr/` has no `README.md` index explaining the ADR convention (status values, supersession protocol). Adding one (~30 lines) would help newcomers and AI agents.

### 3.4 `docs/PRDs/`

- **OK** — `00-index.md` enumerates 17 PRDs across 6 phases. Each PRD has a status; the index points at `IMPLEMENTATION_STATUS.md` for live state.
- **OK** — `IMPLEMENTATION_STATUS.md` is the authoritative current-state doc, dated 2026-04-24 and explicitly aware of the WI-37 retirement. This is the **single best doc in the repo** for "what's actually shipped." It deserves more prominence — README links it, but `docs/ARCHITECTURE.md` and `architecture/C4.md` do not.
- **OK** — `BACKLOG.md` is the live work-item index referenced from many other docs.
- **OK** — `Nexus_Growth_Plan.md` exists; not deeply audited but listed as current planning.
- **MINOR** — Templates under `docs/PRDs/templates/{community-plugin,core-plugin}/README.md` are scaffolding examples. Their location nested under `PRDs/` is a little odd — they're plugin templates, not product requirements documents. Consider moving to `docs/templates/` or `examples/`.

### 3.5 `docs/archive/`

- **OK** — `archive/README.md` is excellent: lists what's archived and what's *not* archived, explains the per-file `> **Archived <date>**` convention, and was reorganized 2026-04-26.
- **OK** — `archive/planning/README.md` and `archive/superpowers/README.md` provide sub-indices.
- **MINOR** — The "What's *not* archived" table at `archive/README.md` lines 65-81 is in practice a better inventory of `docs/` than `docs/` itself has. The reorg should promote this list into a `docs/README.md`.

### 3.6 `shell/docs/`

- **OK** — All 11 active docs (`architecture.md`, `plugin-api.md`, `plugin-system.md`, `core-plugins.md`, `registry-system.md`, `event-bus.md`, `slot-system.md`, `context-keys.md`, `workspace-layout.md`, `extension-host.md`, `writing-a-plugin.md`) cover their topics coherently and align with current `shell/src/`.
- **OK** — `obsidian/obsidian-runtime.md` and `obsidian/obsidian-measurements.md` are explicit reference materials for design parity. They could carry a one-line preamble — "Reference notes for design parity with Obsidian. Not authoritative for Nexus implementation." — to head off future confusion.
- **OK** — `shell/docs/archive/README.md` (dated 2026-04-26) is well-curated and includes a "Still current" section listing the active docs.
- **MISSING [LOW]** — `shell/docs/` has no top-level `README.md` index. The shell archive README lists the active set, but there's no entry point inside `shell/docs/` itself.

### 3.7 Other locations

- **OK** — `packages/nexus-extension-api/README.md` and `packages/nexus-extension-api/DEPRECATED.md` are coherent and tracked.
- **OK** — Skill builtins under `crates/nexus-skills/builtins/*.skill.md` are skill definition files, not narrative docs.
- **MINOR** — `crates/` is otherwise doc-light. A handful of one-paragraph `README.md` files at the top of each service crate (linking to its IPC handlers + the relevant PRD/ADR) would help contributors navigate. Not a priority.

---

## 4. Cross-cutting issues

### 4.1 Two architecture docs

`docs/ARCHITECTURE.md` (~600+ lines) and `docs/architecture/C4.md` (~hundreds of lines) tell substantially the same C4 story with different Mermaid diagrams. Both contain the same legacy-shell staleness. **Pick one and delete the other.** My recommendation: keep `docs/architecture/C4.md` as the C4 reference (its filename is clearer about what it is), and replace `docs/ARCHITECTURE.md` with either a redirect or a higher-level narrative that points to `C4.md` for the canonical model and to `IMPLEMENTATION_STATUS.md` for the live state.

### 4.2 No top-level docs index

There are six plausible "start here" docs. None links to all the others. A new `docs/README.md` (~80-100 lines) curating "for contributors / for plugin authors / for end users / for AI agents" entry points would make the rest of the doc set legible.

### 4.3 IPC schema documentation lags the generator

`crates/nexus-bootstrap/schemas/ipc/` and `packages/nexus-extension-api/src/generated/ipc/` are auto-generated and drift-checked (`scripts/check_ipc_drift.sh`). `docs/ipc-schemas.md` appears to predate the generator's full coverage — pilot-phase phrasing for what is now broad coverage. The generator's output is the source of truth; the prose doc should explain *the policy* (drift gate, ts-rs / schemars), not enumerate handlers.

### 4.4 The "where to learn what's actually shipped" gap

`IMPLEMENTATION_STATUS.md` is the canonical answer. It is currently buried two directories deep and not linked from `CLAUDE.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, or `docs/architecture/C4.md`. Promote it.

### 4.5 Plugin author journey

A plugin author currently has to find: `docs/writing-your-first-plugin.md` (quickstart), `shell/docs/writing-a-plugin.md` (reference), `shell/docs/plugin-api.md` (API surface), `docs/adr/0015-iframe-sandbox-plugin-runtime.md` (sandbox model), `docs/adr/0002-hierarchical-capability-strings.md` (capabilities), and `packages/nexus-extension-api/README.md`. None of these links to the others as a coherent path. A `docs/plugin-authors/README.md` (or section in the new `docs/README.md`) tying them together would close this.

---

## 5. Proposed reorganization

The principle: **organize by reader intent, not by artifact type.** Four audiences were specified:
- **Contributors to Nexus core** — building or modifying the kernel, services, IPC, bootstrap.
- **Plugin authors** — writing community WASM plugins or shell JS/TS plugins.
- **End users of the forge** — running CLI/TUI/shell against their markdown.
- **AI agents** (Claude Code et al.) — picking up context fast.

The current tree is already close to this; it mostly needs a navigation layer and a few moves.

### 5.1 Target structure

```
nexus/
├── README.md                          # End-user-facing front door (CLI quickstart, MCP, install)
├── CLAUDE.md                          # AI-agent guidance (kept, fixed)
├── CONTRIBUTING.md                    # Contributor entry point (kept, fixed)
├── DEPRECATED.md                      # Plugin-API deprecation policy (kept)
└── docs/
    ├── README.md                      # NEW: master index, curated by audience
    │
    ├── architecture/                  # Single home for architecture
    │   ├── README.md                  # NEW: short narrative overview
    │   ├── C4.md                      # The canonical C4 model (formerly two docs)
    │   ├── invariants.md              # NEW: extracted from CLAUDE.md "four invariants"
    │   ├── leaf-architecture.md       # MOVED from docs/
    │   ├── editor-transaction.md      # RENAMED from editor-transaction-architecture.md
    │   └── ipc-schemas.md             # MOVED + rewritten to describe the policy, not enumerate handlers
    │
    ├── adr/
    │   ├── README.md                  # NEW: status conventions, supersession protocol, ADR template
    │   └── 0001-..0020-...md          # Unchanged
    │
    ├── PRDs/
    │   ├── 00-index.md                # Updated with explicit pointer to IMPLEMENTATION_STATUS.md
    │   ├── IMPLEMENTATION_STATUS.md   # Promoted via cross-links from README, CLAUDE.md, architecture/
    │   ├── BACKLOG.md
    │   └── 01-...17-...md             # Unchanged
    │
    ├── plugin-authors/                # NEW directory: bundles the plugin journey
    │   ├── README.md                  # Quickstart → reference → ADRs map
    │   ├── quickstart.md              # MOVED from docs/writing-your-first-plugin.md
    │   ├── reference.md               # MOVED+RENAMED from shell/docs/writing-a-plugin.md (or kept there + linked)
    │   └── capabilities.md            # NEW: extracted policy from ADR 0002 + lookup table for capability strings used in code
    │
    ├── users/                         # NEW directory: end-user docs
    │   ├── README.md                  # Quickstart, env vars, forge layout
    │   ├── cli.md                     # MOVED from docs/nexus-cli.md (and verified against current CLI)
    │   ├── tui.md                     # NEW: extracted TUI keybindings + behaviour from README.md
    │   └── mcp.md                     # NEW: accurate tool list (15 tools), pulled from generator output if possible
    │
    ├── roadmap/                       # NEW: gathers the in-flight planning docs
    │   ├── OPEN-ITEMS.md              # MOVED from docs/
    │   ├── REQUIRED-FOR-FORMAL-RELEASE.md   # MOVED
    │   ├── AI-INTEGRATION-DIRECTIONS.md     # MOVED
    │   ├── AI-MEMORY-LAYER-PLAN.md          # MOVED
    │   ├── AI-AMBIENT-COPILOT-PLAN.md       # MOVED
    │   ├── notion-block-ux-plan.md          # MOVED
    │   └── Nexus_Growth_Plan.md             # MOVED from PRDs/ — not a PRD, it's a plan
    │
    ├── references/
    │   └── obsidian-settings-modal.md
    │
    ├── templates/                     # NEW location for plugin scaffolding refs
    │   ├── community-plugin/README.md # MOVED from PRDs/templates/
    │   └── core-plugin/README.md      # MOVED from PRDs/templates/
    │
    └── archive/                       # Existing, no change to structure
        ├── README.md
        ├── planning/
        ├── superpowers/
        └── (archived files)

shell/
└── docs/
    ├── README.md                      # NEW: index of shell docs (currently implicit only)
    ├── architecture.md
    ├── extension-host.md
    ├── plugin-system.md
    ├── plugin-api.md
    ├── slot-system.md
    ├── registry-system.md
    ├── event-bus.md
    ├── core-plugins.md
    ├── context-keys.md
    ├── workspace-layout.md
    ├── writing-a-plugin.md            # Stays here; docs/plugin-authors/reference.md is just a pointer
    ├── obsidian/
    │   ├── README.md                  # NEW: 3-line preamble explaining these are reference notes
    │   ├── obsidian-runtime.md
    │   └── obsidian-measurements.md
    └── archive/
```

### 5.2 What this gets you

- **One-doc onboarding for each audience.** A new contributor reads `CONTRIBUTING.md` → `docs/architecture/README.md` → `IMPLEMENTATION_STATUS.md`. A plugin author reads `docs/plugin-authors/README.md`. A user reads `docs/users/README.md`. An AI agent reads `CLAUDE.md`.
- **One architecture doc.** `architecture/C4.md` is canonical. The legacy-shell ghosts get exorcised in one place, not two.
- **Active planning docs visible.** Today they sit at the same level as the architecture doc with no signal that they're different. `docs/roadmap/` separates the in-flight stuff from the structural docs.
- **Plugin journey stitched together.** Today's three-doc scavenger hunt becomes a single landing page.

### 5.3 What stays

- The PRD numbering and `BACKLOG.md` / `IMPLEMENTATION_STATUS.md` pair. This is the mature part of the system.
- The ADR sequence and conventions. Just add a `README.md`.
- The archive structure and discipline. Don't break what's working.
- `shell/docs/` — these are good docs for shell-internal contributors. The reorg just adds an index.

---

## 6. Prioritized action list

Ordered by impact-per-effort. Items 1-4 should land in one focused pass.

### P0 — Stop teaching the wrong system (do this first, ~half a day)

1. **Fix the two architecture docs.** Replace every `nexus-app` and `app/` reference with `nexus-shell` and `shell/`. Pick one of `ARCHITECTURE.md` and `architecture/C4.md` to keep; the other becomes a one-line redirect ("Moved to ..."). Update the date stamp.
2. **Rewrite the README MCP section.** Replace the table at lines 150-163 with the actual 15 `nexus_*` tools from `crates/nexus-mcp/src/server.rs`. Update the count at lines 17 and 100-104. Consider auto-generating this section in CI from the rmcp registration list.
3. **Fix the Tauri bridge claim in `CLAUDE.md` and `CONTRIBUTING.md`.** Either expand the list to all 22 commands grouped by purpose (kernel / plugin-mgmt / persistence / popout), or rephrase the guardrail to "the *kernel* bridge is minimal — kernel_invoke is the path for service capability — but a small number of shell-management commands are intentional (plugin discovery, persistence, popout)." Cross-reference ADR 0020 for the popout commands.
4. **Add `docs/README.md`.** Curate by audience. Promote `IMPLEMENTATION_STATUS.md`. Use the "What's *not* archived" table from `docs/archive/README.md` as the seed.

### P1 — Reduce drift between docs and code (next sprint)

5. **Verify and update the README crate summary.** Use the 24-crate Cargo workspace as the source of truth. Drop the per-crate one-liner table or auto-generate from `Cargo.toml [package].description` fields.
6. **Verify `docs/ipc-schemas.md`.** If it still says "5 handlers documented," rewrite it as a *policy* doc (what the drift script does, ts-rs / schemars roles, when to regenerate) and link to the generated TS/JSON dirs as the authoritative listing.
7. **Verify `docs/nexus-cli.md` against `crates/nexus-cli/src/commands/`.** Spot-checks suggest it's broadly current but the README's command summary at lines 117-126 should match it line-for-line.
8. **Add `shell/docs/README.md` and `docs/adr/README.md`.** Both are mechanical (~30-50 lines each).

### P2 — Reorganize for the four audiences (when there's time)

9. **Create `docs/plugin-authors/`, `docs/users/`, `docs/roadmap/`, `docs/templates/` and move files** per §5.1. Update internal links. The git history is preserved; reviewers will need to confirm no external link is broken.
10. **Move `docs/architecture/` to be the only architecture home.** Move `leaf-architecture.md` and `editor-transaction-architecture.md` in. Delete or redirect `docs/ARCHITECTURE.md`.
11. **Add a per-crate `README.md` to each `nexus-*` service crate** with: one-paragraph purpose, IPC handlers it owns, capability(ies) it requires, link to relevant PRD/ADR. This is mechanical drudgery but pays off long-term.

### P3 — Forward-looking improvements

12. **Add a CI lint that fails on stale doc references.** A simple grep-for-`nexus-app`-and-`app/`-in-docs check would have caught the architecture doc problem on day one.
13. **Add `> **Last verified <date>**` headers to every active doc.** Even a manual rotation through these headers gives you a staleness floor.
14. **Wire MCP tool list and Tauri command list into doc generation.** Both surfaces are programmatically discoverable; both are wrong in the docs today. The same drift-check pattern that already works for IPC schemas would work here.

---

## 7. What I did *not* verify

In the interest of finishing the audit in one pass, the following were treated as "OK presumed" based on cross-references in `docs/archive/README.md` and the absence of obvious staleness signals. They should be spot-checked when their owners next touch them:

- `docs/editor-transaction-architecture.md` — content not deeply read.
- `docs/notion-block-ux-plan.md` — content not deeply read.
- `docs/leaf-architecture.md` — opening lines only.
- `docs/PRDs/IMPLEMENTATION_STATUS.md` — first ~5 lines read; full file is 30k+ tokens. Skim it before relying on any specific status.
- All 17 numbered PRDs beyond the index. The PRD/ADR/BACKLOG triangulation is solid; individual PRDs may still drift.
- `shell/docs/` content claims about extension-host APIs, slot system, and plugin manifest fields. Cross-spot-checks against `shell/src/` were not exhaustive.
- `crates/nexus-skills/builtins/*.skill.md` — these are skill definitions, not narrative docs, so the "is this stale" question doesn't apply in the same way.

---

## 8. Summary

The repo's documentation discipline is **above average for a project this size** — there are PRDs, ADRs, an `IMPLEMENTATION_STATUS.md`, a curated archive with explanations, and per-crate skill definitions where they make sense. What's missing is the navigation layer that would make all that visible to a new reader, and a small number of high-traffic pages have drifted from code in ways that mislead.

Three changes (fix architecture docs, fix README MCP table, add `docs/README.md` index) get you most of the way to a doc set that a new contributor, plugin author, end user, or AI agent could actually navigate without slack-asking for the real entry point.
